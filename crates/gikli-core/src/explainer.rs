//! The failure explainer: it classifies a failed git invocation and returns a plain message, a
//! next step and the actions to offer. git's raw output goes to the operation log only.
//!
//! Classification never reads git's translated sentences. It relies on what stays the same in
//! every language: what git records in its trace of the run, the file names and the commands it
//! quotes, the sentences git never translates and those of programs that have no translations,
//! such as ssh and curl, what a server says in words of its own, which git passes on as they are,
//! the sentences of a run git was told not to translate, the state of the repository and what is
//! at the path of a local URL. ADR-0001 pins what gikli parses, but git's error output has no format to pin, which
//! is why only these fixed parts of it count.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::{GitRunner, Invocation};
use crate::url_passwords::hide_url_passwords;
use crate::{CloneTarget, Failure, Repository, canonicalize, processes};

/// A failure explained in plain words. The UI words the message and the next step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Explanation {
    pub message: ExplainedMessage,
    pub next_step: NextStep,
    /// What gikli can do about it, in the order to offer them.
    pub actions: Vec<FailureAction>,
}

/// What went wrong.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExplainedMessage {
    /// A hook of the user's stopped the operation. `output` is what the hook wrote, the one part
    /// of a run shown outside the operation log.
    HookFailed { hook: String, output: String },
    /// git refuses the repository at `folder`, as it belongs to someone else. `folder` is written
    /// as git writes it, which is how git compares it with what the user trusts.
    DubiousOwnership { folder: String },
    /// The index of the Repository is locked: another git operation is using it, or one ended
    /// without cleaning up after itself and left `lock` behind.
    IndexLocked { lock: PathBuf },
    /// A path in the Repository is longer than Windows allows by default.
    LongPaths,
    /// The remote did not accept the credentials it got, or got none because the user declined to
    /// give them or nobody could be asked.
    AuthenticationFailed,
    /// ssh does not know the host key of the server, and nobody accepted it.
    HostKeyUnknown,
    /// The server answers with another host key than the one ssh knows for it, so it may not be
    /// the same server.
    HostKeyChanged,
    /// git could not get to the server: its name does not resolve, or it does not answer.
    ServerUnreachable,
    /// `url` holds no repository git can reach: nothing is at its path, or the server says it has
    /// none, which a server also says of a repository the user may not see. `remote` is the remote
    /// whose URL it is, and None for a clone, which has no remote yet. A password in `url` is
    /// hidden.
    RemoteWithoutRepository { remote: Option<String>, url: String },
    /// The target of a clone, `folder`, already holds something, so git refused to clone into it.
    CloneTargetNotEmpty { folder: PathBuf },
    /// The clone into `folder` got its files, and git could not clone its submodules. git leaves
    /// the folder as it is. `cause` is what kept them from cloning, when gikli knows it.
    SubmodulesNotCloned {
        folder: PathBuf,
        cause: Option<Box<ExplainedMessage>>,
    },
}

/// What the user can do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NextStep {
    /// Fix what the hook reports. gikli runs hooks without a terminal, so one that asks for input
    /// has to work without one.
    FixHook,
    /// Trust the folder, if the user knows where it comes from: a repository can run commands of
    /// its owner's choosing, which is why git asks.
    TrustFolder,
    /// No other git operation is running, so the lock is stale and can go.
    RemoveIndexLock,
    /// Wait for the other git operation to finish, then try again. gikli offers to remove the
    /// lock only when it can tell that none is running, which it cannot for the git inside WSL.
    WaitForOtherGitOperation,
    /// Set `core.longpaths` in git. gikli recommends it and never sets it: it is the user's
    /// configuration, and not every tool on Windows copes with such paths.
    EnableLongPaths,
    /// Check the credentials, the SSH keys and the credential helper git uses, then try again.
    CheckCredentials,
    /// Check the fingerprint of the key with whoever runs the server, then try again and accept it.
    VerifyHostKey,
    /// Check the new key with whoever runs the server before connecting again, and only then remove
    /// the old one. gikli leaves `known_hosts` to the user, as trusting a key is theirs to decide.
    CheckChangedHostKey,
    /// Check the URL and the connection to the server, then try again.
    CheckConnection,
    /// Check the URL, and that the user has access to the repository there, then try again: the
    /// URL given to a clone, or the one in the configuration of the remote. gikli only points
    /// there, and changes no URL.
    CheckRemoteUrl,
    /// Choose an empty folder, or empty this one.
    ChooseEmptyFolder,
    /// See in the operation log why the submodules failed. The clone can be opened from its folder
    /// without them, and the folder has to go before cloning into it again.
    UseOrDeleteClone,
}

/// Something gikli can do about a failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum FailureAction {
    /// Adds `folder` to the folders the user trusts, in their global git configuration, once they
    /// have confirmed. `repository` is where git refused it, so the same git trusts it.
    TrustFolder {
        repository: Repository,
        folder: String,
    },
    /// Removes the stale lock of the index of `repository`, with the `next-index-*.lock` files
    /// `git commit --only` keeps next to it, once the user has confirmed and as long as no other
    /// git operation has started running there.
    RemoveIndexLock { repository: Repository },
}

/// The explanation of the failure of `invocation`, run in `folder`, when gikli knows this failure.
pub(crate) fn explain(
    runner: &GitRunner,
    folder: &Path,
    invocation: &Invocation,
) -> Option<Explanation> {
    if let Some(hook) = &invocation.failed_hook {
        return Some(Explanation {
            message: ExplainedMessage::HookFailed {
                hook: hook.clone(),
                output: invocation.stderr.clone(),
            },
            next_step: NextStep::FixHook,
            actions: vec![],
        });
    }
    let stderr = &invocation.stderr;
    let repository = Repository {
        root: folder.to_path_buf(),
    };
    if let Some(untrusted) = untrusted_folder(stderr) {
        return Some(Explanation {
            message: ExplainedMessage::DubiousOwnership {
                folder: untrusted.clone(),
            },
            next_step: NextStep::TrustFolder,
            actions: vec![FailureAction::TrustFolder {
                repository,
                folder: untrusted,
            }],
        });
    }
    // git names the file in every language. It may be gone by now, with the operation that held it.
    if stderr.contains(INDEX_LOCK)
        && let Some(lock) = existing_index_lock(runner, folder)
    {
        return Some(explain_index_lock(runner, &repository, lock));
    }
    if cfg!(windows) && runner.runs_natively() && stderr.contains(PATH_TOO_LONG) {
        return Some(explained(
            ExplainedMessage::LongPaths,
            NextStep::EnableLongPaths,
        ));
    }
    // What ssh writes, which OpenSSH does not translate. A changed key comes first: ssh may go on
    // to fail in other ways, and this is what the user needs to hear about.
    if any_line(stderr, is_changed_host_key) {
        return Some(explained(
            ExplainedMessage::HostKeyChanged,
            NextStep::CheckChangedHostKey,
        ));
    }
    if any_line(stderr, is_failed_host_key_verification) {
        return Some(explained(
            ExplainedMessage::HostKeyUnknown,
            NextStep::VerifyHostKey,
        ));
    }
    if invocation.credentials_refused || any_line(stderr, is_failed_authentication) {
        return Some(explained(
            ExplainedMessage::AuthenticationFailed,
            NextStep::CheckCredentials,
        ));
    }
    if any_line(stderr, is_unreachable_server) {
        return Some(explained(
            ExplainedMessage::ServerUnreachable,
            NextStep::CheckConnection,
        ));
    }
    None
}

/// What a run of git reached out to, which the failure explainer checks for a repository.
pub(crate) enum Contacted<'a> {
    /// These remotes of the Repository, by name, as a fetch does. Empty for a run that reaches out
    /// to nothing.
    Remotes(&'a [String]),
    /// The URL given to a clone, which has no remote yet.
    Url(&'a str),
}

/// The explanation of the failure of `invocation`, run in `folder`, which reached out to what
/// `contacted` names, when gikli knows this failure. A remote without a repository comes last,
/// after what else a run that reached it can fail with: the credentials a server refused above all,
/// as a server can say it has no repository to someone it does not let in.
pub(crate) fn explain_contacting(
    runner: &GitRunner,
    folder: &Path,
    invocation: &Invocation,
    contacted: &Contacted,
) -> Option<Explanation> {
    explain(runner, folder, invocation)
        .or_else(|| explain_remote_without_repository(runner, folder, invocation, contacted))
}

/// The explanation of a remote without a repository, of those `contacted` names, as
/// [`remote_without_repository`] tells it.
fn explain_remote_without_repository(
    runner: &GitRunner,
    folder: &Path,
    invocation: &Invocation,
    contacted: &Contacted,
) -> Option<Explanation> {
    let message = remote_without_repository(runner, folder, invocation, contacted)?;
    Some(explained(message, NextStep::CheckRemoteUrl))
}

/// The remote of those `contacted` names whose URL holds no repository, when gikli can tell. A
/// local URL tells by what is at its path, whatever git said about it. A server tells in words of
/// its own, which name no remote, so they count for the one remote on a server that can have said
/// them: the only one, or the only one whose git failed, as git's trace names it.
fn remote_without_repository(
    runner: &GitRunner,
    folder: &Path,
    invocation: &Invocation,
    contacted: &Contacted,
) -> Option<ExplainedMessage> {
    // Each remote, by name, with its URL as the user knows it and the URL git uses.
    let urls: Vec<(Option<&str>, String, String)> = match contacted {
        Contacted::Remotes(remotes) => remotes
            .iter()
            .filter_map(|remote| {
                let url = url_of(runner, folder, remote)?;
                Some((Some(remote.as_str()), url.clone(), url))
            })
            .collect(),
        Contacted::Url(given) => {
            let url = url_of(runner, folder, given).unwrap_or_else(|| (*given).to_owned());
            vec![(None, (*given).to_owned(), url)]
        }
    };
    let (local, on_servers): (Vec<_>, Vec<_>) = urls
        .into_iter()
        .map(|(remote, shown, url)| (remote, shown, local_path(runner, folder, &url)))
        .partition(|(_, _, path)| path.is_some());
    let without_repository = local
        .into_iter()
        .find(|(_, _, path)| path.as_deref().is_some_and(|path| !holds_repository(path)));
    let (remote, shown, _) = match without_repository {
        Some(local) => local,
        None if any_line(&invocation.stderr, is_repository_unknown_to_server) => {
            // A fetch of every remote that fetched each in a git of its own knows which failed.
            let failed = &invocation.failed_fetches;
            let on_servers = on_servers.into_iter().filter(|(remote, _, _)| {
                failed.is_empty() || failed.iter().any(|failed| Some(failed.as_str()) == *remote)
            });
            match <[_; 1]>::try_from(on_servers.collect::<Vec<_>>()) {
                Ok([on_server]) => on_server,
                Err(_) => return None,
            }
        }
        None => return None,
    };
    Some(ExplainedMessage::RemoteWithoutRepository {
        remote: remote.map(str::to_owned),
        url: hide_url_passwords(&shown),
    })
}

/// The URL git uses for `remote`, a remote's name or a URL, run in `folder`: with what the user's
/// `url.<base>.insteadOf` rewrites in it.
fn url_of(runner: &GitRunner, folder: &Path, remote: &str) -> Option<String> {
    let read = runner
        .run(folder, &["ls-remote", "--get-url", "--", remote])
        .ok()?;
    let url = read.stdout_text();
    let url = url.trim_end();
    (read.success && !url.is_empty()).then(|| url.to_owned())
}

/// The folder `url` names, as this machine sees it, when git run in `folder` reads `url` as a path
/// on its own machine rather than as a server's. That is a `file://` URL, and what has no `:`
/// before its first `/`, as `host:path` is an SSH address. git for Windows also reads a drive, as
/// in `C:/repo`, as a path.
fn local_path(runner: &GitRunner, folder: &Path, url: &str) -> Option<PathBuf> {
    let windows_paths = cfg!(windows) && runner.runs_natively();
    let path = match url.strip_prefix("file://") {
        // `file:///C:/repo` names `C:/repo` on Windows.
        Some(path) if windows_paths && has_drive(path.trim_start_matches('/')) => {
            path.trim_start_matches('/')
        }
        Some(path) => path,
        None if url.contains("://") => return None,
        None => {
            let local = match (url.find(':'), url.find('/')) {
                (None, _) => true,
                (Some(colon), Some(slash)) => slash < colon,
                (Some(_), None) => false,
            };
            if !local && !(windows_paths && has_drive(url)) {
                return None;
            }
            url
        }
    };
    runner.path_on_this_machine(folder, path)
}

/// Whether `path` starts with a drive, as `C:/` or `C:\` do.
fn has_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

/// Whether git finds something to fetch from at `path`, as it looks there: the path itself, its
/// `.git` folder, the same with `.git` added to its name, and a bundle. Any file there counts: it
/// can point to a git directory, or be a bundle, which git reads too.
fn holds_repository(path: &Path) -> bool {
    let mut with_suffix = path.as_os_str().to_owned();
    with_suffix.push(".git");
    let with_suffix = PathBuf::from(with_suffix);
    let mut bundle = path.as_os_str().to_owned();
    bundle.push(".bundle");
    [
        path.join(".git"),
        path.to_path_buf(),
        with_suffix.join(".git"),
        with_suffix,
        PathBuf::from(bundle),
    ]
    .iter()
    .any(|candidate| candidate.is_file() || is_git_directory(candidate))
}

/// Whether `folder` is a git directory, by what git checks: `HEAD`, and the folders of its objects
/// and its references.
fn is_git_directory(folder: &Path) -> bool {
    folder.join("HEAD").exists() && folder.join("objects").is_dir() && folder.join("refs").is_dir()
}

/// The explanation of the failure of `invocation`, a `git clone` of `url` run in `parent` into
/// `target`, when gikli knows this failure. What git and the remote report as they clone is no
/// part of what a hook wrote, so only what a server says of its repository is read in it. A clone
/// that has its files got them from `url`, so a server's words are about a submodule's.
pub(crate) fn explain_failed_clone(
    runner: &GitRunner,
    parent: &Path,
    target: &CloneTarget,
    url: &str,
    invocation: &Invocation,
) -> Option<Explanation> {
    if target.was_in_use() {
        return Some(explained(
            ExplainedMessage::CloneTargetNotEmpty {
                folder: target.folder.clone(),
            },
            NextStep::ChooseEmptyFolder,
        ));
    }
    // git removes a clone that fails before it has its files.
    let has_files = target.folder.join(".git").exists();
    let explanation = explain(runner, parent, &invocation.without_clone_reports()).or_else(|| {
        if has_files {
            return None;
        }
        explain_remote_without_repository(runner, parent, invocation, &Contacted::Url(url))
    });
    // git goes on to the submodules once the clone has its files, unless a hook or a path too long
    // stopped it first.
    let reached_submodules = target.has_submodules_to_clone()
        && has_files
        && !explanation.as_ref().is_some_and(|explanation| {
            matches!(
                explanation.message,
                ExplainedMessage::HookFailed { .. } | ExplainedMessage::LongPaths
            )
        });
    if !reached_submodules {
        return explanation;
    }
    let (cause, actions) = match explanation {
        Some(explanation) => (Some(Box::new(explanation.message)), explanation.actions),
        None => (None, vec![]),
    };
    Some(Explanation {
        message: ExplainedMessage::SubmodulesNotCloned {
            folder: target.folder.clone(),
            cause,
        },
        next_step: NextStep::UseOrDeleteClone,
        actions,
    })
}

fn explained(message: ExplainedMessage, next_step: NextStep) -> Explanation {
    Explanation {
        message,
        next_step,
        actions: vec![],
    }
}

/// Whether a line of `stderr` is what `reports` looks for. What it looks for ends a line, unlike a
/// file name that looks like it, which git quotes inside a sentence.
fn any_line(stderr: &str, reports: fn(&str) -> bool) -> bool {
    stderr.lines().any(|line| reports(line.trim()))
}

fn is_changed_host_key(line: &str) -> bool {
    line.starts_with('@') && line.contains("REMOTE HOST IDENTIFICATION HAS CHANGED!")
}

fn is_failed_host_key_verification(line: &str) -> bool {
    line == "Host key verification failed."
}

fn is_failed_authentication(line: &str) -> bool {
    is_refused_by_ssh_server(line)
        || is_tired_ssh_server(line)
        || is_unanswered_credential_prompt(line)
        || is_refused_by_http_server(line)
}

/// `git@host: Permission denied (publickey,password).`, with the ways the server takes in brackets.
/// A file that cannot be read says `Permission denied` too, without them, and what a server says
/// comes after `remote: `, without a user.
fn is_refused_by_ssh_server(line: &str) -> bool {
    line.ends_with(").")
        && line
            .split_once("Permission denied (")
            .is_some_and(|(before, _)| {
                before.is_empty() || (before.ends_with(": ") && before.contains('@'))
            })
}

/// `Received disconnect from 192.0.2.1 port 22:2: Too many authentication failures`.
fn is_tired_ssh_server(line: &str) -> bool {
    line.starts_with("Received disconnect from ")
        && line.ends_with(": Too many authentication failures")
}

/// git could not get an answer for a credential prompt: the user declined it, nobody could be asked,
/// or the user forbids asking with `credential.interactive`. git never translates these sentences.
fn is_unanswered_credential_prompt(line: &str) -> bool {
    (line.contains("could not read ") && line.ends_with(": terminal prompts disabled"))
        || line.ends_with("unable to get password from user")
}

/// `... 'https://host/repo': The requested URL returned error: 403`, which is the text of a curl
/// error, whatever language git speaks. A 401 never gets here: git tries credentials for it, and
/// tells the helpers when they are refused.
fn is_refused_by_http_server(line: &str) -> bool {
    const FORBIDDING_STATUSES: [&str; 2] = ["403", "407"];
    line.rsplit_once("The requested URL returned error: ")
        .is_some_and(|(_, status)| {
            !status.contains('\'')
                && status
                    .split(' ')
                    .next()
                    .is_some_and(|status| FORBIDDING_STATUSES.contains(&status))
        })
}

/// What a server says of a repository it does not have, or that the user may not see, in words
/// of its own that git passes on as they are: the text an HTTP server answers with, after
/// `remote: `, which git writes without translating it, and what an SSH server writes, which ssh
/// passes on, with `remote: ` in front when it comes through git's own channel. These are GitHub's
/// and GitLab's.
fn is_repository_unknown_to_server(line: &str) -> bool {
    let said = line.strip_prefix("remote: ").unwrap_or(line);
    matches!(
        said,
        "Repository not found." | "ERROR: Repository not found."
    ) || said.starts_with("ERROR: The project you were looking for could not be found")
}

/// What ssh and curl write when they cannot get to a server, which neither translates. What a server
/// says about servers of its own comes after `remote: `.
fn is_unreachable_server(line: &str) -> bool {
    line.starts_with("ssh: Could not resolve hostname ")
        || line.starts_with("ssh: connect to host ")
        || (!line.starts_with("remote: ")
            && (line.contains("Could not resolve host: ")
                || line.contains("Failed to connect to ")))
}

/// What a run cancelled in `folder` left in the user's way, with what to do about it: the lock of
/// the index, as killing git leaves it where it is.
pub(crate) fn left_behind_by_cancelled_run(
    runner: &GitRunner,
    folder: &Path,
) -> Option<Explanation> {
    let lock = existing_index_lock(runner, folder)?;
    let repository = Repository {
        root: folder.to_path_buf(),
    };
    Some(explain_index_lock(runner, &repository, lock))
}

/// Removes the lock of the index of `repository`, and the locks of `git commit --only` next to it,
/// unless a git operation may have started running there since the user was offered this. A lock
/// already gone is fine.
pub(crate) fn remove_stale_index_lock(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<(), Failure> {
    let Some(lock) = existing_index_lock(runner, &repository.root) else {
        return Ok(());
    };
    let explanation = explain_index_lock(runner, repository, lock.clone());
    if explanation.next_step != NextStep::RemoveIndexLock {
        return Err(Failure::Explained { explanation });
    }
    remove_lock(&lock)?;
    let git_dir = lock.parent().unwrap_or(&repository.root);
    for entry in std::fs::read_dir(git_dir).into_iter().flatten().flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(NEXT_INDEX_LOCK_PREFIX) && name.ends_with(".lock") {
            remove_lock(&entry.path())?;
        }
    }
    Ok(())
}

fn remove_lock(lock: &Path) -> Result<(), Failure> {
    std::fs::remove_file(lock).map_err(|error| Failure::IndexLockNotRemoved {
        lock: lock.to_path_buf(),
        message: error.to_string(),
    })
}

/// Whether `invocation`, a run git was told to write in its own language, found no repository at or
/// above its folder. Anything else that stops git must not read as this.
pub(crate) fn found_no_repository(invocation: &Invocation) -> bool {
    invocation.stderr.lines().any(|line| {
        NO_REPOSITORY_FOUND
            .iter()
            .any(|sentence| line.starts_with(sentence))
    })
}

/// How git starts to say it found no repository, before it says where it stopped looking: at the
/// root of the disk or at the edge of a file system. Both in the words git uses untranslated.
const NO_REPOSITORY_FOUND: [&str; 2] = [
    "fatal: not a git repository (or any of the parent directories)",
    "fatal: not a git repository (or any parent up to mount point ",
];

/// Whether `invocation`, a run git was told to write in its own language, was refused the bare
/// repository it found because of the user's `safe.bareRepository`.
pub(crate) fn refused_bare_repository(invocation: &Invocation) -> bool {
    invocation
        .stderr
        .lines()
        .any(|line| line.starts_with(BARE_REPOSITORY_REFUSED))
}

/// How git starts to say that it will not use a bare repository, before the path of the repository
/// and the value of the setting. In the words git uses untranslated.
const BARE_REPOSITORY_REFUSED: &str = "fatal: cannot use bare repository '";

/// How git for Windows reports a path that is too long. It is the text of the system's error,
/// which comes from Windows' C library in English, not from git's translations.
const PATH_TOO_LONG: &str = "Filename too long";

/// The name of the file that locks the index.
const INDEX_LOCK: &str = "index.lock";

/// How the names of the locks of the temporary indexes of `git commit --only` start, in the same
/// folder as the lock of the index. The rest of each is the number of the git that made it.
const NEXT_INDEX_LOCK_PREFIX: &str = "next-index-";

/// The lock of the index of the repository `folder` is in, when there is one. Each linked worktree
/// has an index, and a lock, of its own.
fn existing_index_lock(runner: &GitRunner, folder: &Path) -> Option<PathBuf> {
    let git_dir = runner.git_dir(folder).ok().flatten()?;
    let lock = canonicalize(&git_dir).ok()?.join(INDEX_LOCK);
    lock.exists().then_some(lock)
}

/// The explanation of the index of `repository` being locked by `lock`. Removing the lock is
/// offered only when gikli can tell that no other git operation is running there: removing the
/// lock of a running one would corrupt the index.
fn explain_index_lock(runner: &GitRunner, repository: &Repository, lock: PathBuf) -> Explanation {
    let git_dir = lock.parent().unwrap_or(&repository.root);
    let stale =
        runner.runs_natively() && !processes::git_runs_in(&[repository.root.as_path(), git_dir]);
    let (next_step, actions) = if stale {
        (
            NextStep::RemoveIndexLock,
            vec![FailureAction::RemoveIndexLock {
                repository: repository.clone(),
            }],
        )
    } else {
        (NextStep::WaitForOtherGitOperation, vec![])
    };
    Explanation {
        message: ExplainedMessage::IndexLocked { lock },
        next_step,
        actions,
    }
}

/// The folder in the command git recommends when it refuses a repository for its owner, on a line
/// of its own. Translations keep the command as it is.
fn untrusted_folder(stderr: &str) -> Option<String> {
    let folder = stderr.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("git config --global --add safe.directory ")
    })?;
    Some(without_shell_quotes(folder.trim()))
}

/// `text` as a shell reads it, when git quoted it with single quotes for one. Recent versions of
/// git quote what needs it, such as a space, and older ones quote nothing.
fn without_shell_quotes(text: &str) -> String {
    match text
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        Some(quoted) => quoted.replace(r"'\''", "'"),
        None => text.to_owned(),
    }
}
