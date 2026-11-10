//! Committing: what git has to do for the changes the user selected to end up in one commit.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::git::GitRunner;
use crate::status::{Change, ChangeSet, ChangeStatus, Status};
use crate::{Failure, failure_of, git_failed_to_start, trace};

/// How many previous commit messages the user can choose from.
const PREVIOUS_MESSAGES_OFFERED: usize = 20;

/// What a commit of a selection of changes works on.
pub(crate) struct Selection {
    /// The selected Unversioned Files, which git has to know before it can commit them.
    pub unversioned: Vec<String>,
    /// Every path the commit records.
    pub committed: Vec<String>,
}

impl Selection {
    /// The changes in `status` at `paths` that a commit can record: those in a Changelist and the
    /// Unversioned Files. Ignored files are no changes. `status` must list the ignored files, as one
    /// can sit where a moved file was.
    pub fn of(status: &Status, paths: &[String]) -> Self {
        let chosen: Vec<&Change> = status
            .changes
            .iter()
            .filter(|change| change.status != ChangeStatus::Ignored)
            .filter(|change| paths.iter().any(|path| Path::new(path) == change.path))
            .collect();
        let mut committed = Vec::new();
        for change in &chosen {
            committed.push(text_of(&change.path));
            // A move is two changes to git: leaving out one would leave it staged. Unless something
            // new sits at the old path, as git would commit that from the working tree instead.
            committed.extend(
                change
                    .renamed_from()
                    .filter(|from| !is_taken_by_a_new_file(status, from))
                    .map(text_of),
            );
            committed.extend(
                status
                    .moved_and_removed
                    .get(&change.path)
                    .map(|to| text_of(to)),
            );
        }
        Self {
            unversioned: chosen
                .iter()
                .filter(|change| change.status == ChangeStatus::Untracked)
                .map(|change| text_of(&change.path))
                .collect(),
            committed,
        }
    }
}

/// Whether a file git does not track is at `path`, or a folder of them, or a file in the way of it.
fn is_taken_by_a_new_file(status: &Status, path: &Path) -> bool {
    status.changes.iter().any(|file| {
        !file.status.is_in_changelists()
            && (file.path.starts_with(path) || path.starts_with(&file.path))
    })
}

/// Whether a commit is a new one or replaces the last one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitKind {
    New,
    Amend,
}

/// What git reads for one commit: the message and the paths, which do not fit on a command line
/// when a Changelist is large. The files are in gikli's folder of the git directory, the one place
/// git inside WSL and gikli on Windows both reach by a path of their own. They go when this is
/// dropped.
pub(crate) struct CommitInput {
    /// Where gikli wrote them.
    folder: PathBuf,
    /// The same folder, relative to the root of the Repository, as git gets it.
    folder_for_git: String,
    has_unversioned_files: bool,
}

const MESSAGE: &str = "message";
const PATHS: &str = "paths";
const UNVERSIONED_PATHS: &str = "unversioned-paths";

impl CommitInput {
    /// Writes what git reads to commit `selection` with `message`, under the git directory at
    /// `git_dir` relative to `root`.
    pub fn write(
        root: &Path,
        git_dir: &str,
        selection: &Selection,
        message: &str,
    ) -> io::Result<Self> {
        let name = trace::new_run_name();
        let folder = root.join(git_dir).join("gikli").join(&name);
        fs::create_dir_all(&folder)?;
        let input = Self {
            folder,
            folder_for_git: format!("{git_dir}/gikli/{name}"),
            has_unversioned_files: !selection.unversioned.is_empty(),
        };
        // git would take a last line without an end as it is, and other tools do not expect that.
        let line_end = if message.ends_with('\n') { "" } else { "\n" };
        fs::write(input.folder.join(MESSAGE), format!("{message}{line_end}"))?;
        fs::write(
            input.folder.join(PATHS),
            nul_separated(&selection.committed),
        )?;
        fs::write(
            input.folder.join(UNVERSIONED_PATHS),
            nul_separated(&selection.unversioned),
        )?;
        Ok(input)
    }

    /// The arguments after `git` that add the selected Unversioned Files, when there are any.
    pub fn add_command(&self) -> Option<Vec<String>> {
        self.has_unversioned_files
            .then(|| self.command_on(UNVERSIONED_PATHS, &["add"]))
    }

    /// The arguments after `git` that take the selected Unversioned Files out of git again, when
    /// there are any. `--force` as well: git refuses otherwise when a file changed after it was added.
    pub fn forget_added_command(&self) -> Option<Vec<String>> {
        self.has_unversioned_files
            .then(|| self.command_on(UNVERSIONED_PATHS, &["rm", "--quiet", "--force", "--cached"]))
    }

    /// The arguments after `git` that commit the selection. The message goes in as written: the
    /// cleanup is not left to `commit.cleanup`, and `commit.verbose` would cut it at a scissors line.
    /// An amend may end up empty: git would otherwise refuse to reword an empty commit, or to take
    /// back all of the last one.
    pub fn commit_command(&self, kind: CommitKind) -> Vec<String> {
        let mut command = self.command_on(
            PATHS,
            &["commit", "--only", "--cleanup=verbatim", "--no-verbose"],
        );
        if kind == CommitKind::Amend {
            command.extend(["--amend".to_owned(), "--allow-empty".to_owned()]);
        }
        command.push(format!("--file={}/{MESSAGE}", self.folder_for_git));
        command
    }

    /// `git` with `command` on the paths in the file `paths`, taken literally.
    fn command_on(&self, paths: &str, command: &[&str]) -> Vec<String> {
        let mut args = vec!["--literal-pathspecs".to_owned()];
        args.extend(command.iter().map(|arg| (*arg).to_owned()));
        args.push(format!(
            "--pathspec-from-file={}/{paths}",
            self.folder_for_git
        ));
        args.push("--pathspec-file-nul".to_owned());
        args
    }
}

impl Drop for CommitInput {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}

fn nul_separated(paths: &[String]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for path in paths {
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(0);
    }
    bytes
}

/// Fails when git has no name or no email to write in a commit made in the Repository at `root`.
/// git could make some up from the name of the machine, as it does when it is not told otherwise,
/// and a commit would then carry them for good, so only what the user configured counts.
pub(crate) fn ensure_identity(runner: &GitRunner, root: &Path) -> Result<(), Failure> {
    let is_missing = |key: &str| -> Result<bool, Failure> {
        let invocation = runner
            .run(root, &["config", "--get", key])
            .map_err(git_failed_to_start)?;
        Ok(!invocation.success || invocation.stdout_text().trim().is_empty())
    };
    let name_missing = is_missing("user.name")?;
    let email_missing = is_missing("user.email")?;
    if name_missing || email_missing {
        return Err(Failure::CommitIdentityMissing {
            name_missing,
            email_missing,
        });
    }
    Ok(())
}

/// Whether the Repository at `root` has a commit for HEAD to be on.
pub(crate) fn has_commits(runner: &GitRunner, root: &Path) -> Result<bool, Failure> {
    Ok(runner
        .run(root, &["rev-parse", "--verify", "--quiet", "HEAD"])
        .map_err(git_failed_to_start)?
        .success)
}

/// The messages of the user's last commits in the Repository at `root`, on any local branch, newest
/// first and each one once. A commit is theirs when it has the `user.email` git has now, so without
/// one none is. Merges are left out, as git wrote their messages.
pub(crate) fn previous_messages(runner: &GitRunner, root: &Path) -> Result<Vec<String>, Failure> {
    let email = runner
        .run(root, &["config", "--get", "user.email"])
        .map_err(git_failed_to_start)?;
    let email = email.stdout_text();
    let email = email.trim();
    if email.is_empty() || !has_commits(runner, root)? {
        return Ok(Vec::new());
    }
    let author = format!("--author=<{email}>");
    let max_count = format!("--max-count={PREVIOUS_MESSAGES_OFFERED}");
    let invocation = runner
        .run(
            root,
            &[
                "log",
                "--no-mailmap",
                "--no-merges",
                "--fixed-strings",
                &author,
                &max_count,
                "--encoding=UTF-8",
                "-z",
                "--format=%B",
                "HEAD",
                "--branches",
            ],
        )
        .map_err(git_failed_to_start)?;
    if !invocation.success {
        return Err(failure_of(
            runner,
            root,
            &invocation,
            Failure::CommitMessagesUnreadable {
                folder: root.to_path_buf(),
            },
        ));
    }
    let mut messages: Vec<String> = Vec::new();
    for message in invocation.stdout_text().split('\0') {
        let message = message.trim_end_matches('\n');
        if !message.is_empty() && !messages.iter().any(|seen| seen == message) {
            messages.push(message.to_owned());
        }
    }
    Ok(messages)
}

/// The message of the last commit in the Repository at `root`. `log.showSignature` would put the
/// output of the signature check in front of it.
pub(crate) fn last_message(runner: &GitRunner, root: &Path) -> Result<Option<String>, Failure> {
    if !has_commits(runner, root)? {
        return Ok(None);
    }
    let invocation = runner
        .run(
            root,
            &[
                "log",
                "--no-show-signature",
                "--max-count=1",
                "--encoding=UTF-8",
                "--format=%B",
                "HEAD",
                "--",
            ],
        )
        .map_err(git_failed_to_start)?;
    if !invocation.success {
        return Err(failure_of(
            runner,
            root,
            &invocation,
            Failure::CommitMessagesUnreadable {
                folder: root.to_path_buf(),
            },
        ));
    }
    Ok(Some(
        invocation.stdout_text().trim_end_matches('\n').to_owned(),
    ))
}

/// The comment of the Changelist the changes at `paths` are in, when there is exactly one such
/// Changelist. Unversioned Files are in none, so they do not count.
pub(crate) fn comment_of_selection(changes: &ChangeSet, paths: &[String]) -> Option<String> {
    let mut lists = changes.changelists.iter().filter(|list| {
        list.group
            .changes
            .iter()
            .any(|change| paths.iter().any(|path| Path::new(path) == change.path))
    });
    match (lists.next(), lists.next()) {
        (Some(list), None) => list.comment.clone(),
        _ => None,
    }
}

/// An alias that prints the file `commit.template` names. Git reads it, as the path can be one only
/// the git inside WSL understands. Its shell starts at the top of the working tree, where git
/// resolves a relative template too.
const PRINT_TEMPLATE: &str = "alias.gikli-print-template=!template=$(git config --path --get commit.template) || exit 0; cat -- \"$template\"";

/// The text of the file `commit.template` names for the Repository at `root`. None when there is
/// no such setting, its file cannot be read or it holds no text: a commit does not depend on it.
pub(crate) fn template(runner: &GitRunner, root: &Path) -> Result<Option<String>, Failure> {
    let invocation = runner
        .run(root, &["-c", PRINT_TEMPLATE, "gikli-print-template"])
        .map_err(git_failed_to_start)?;
    let text = invocation.stdout_text();
    Ok((invocation.success && !text.trim().is_empty()).then_some(text))
}

/// Where a commit may not be meant to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CommitWarning {
    /// HEAD is on no branch, so the commit will be on none either.
    DetachedHead,
    /// A rebase is in progress, so the commit goes into the history being rewritten.
    Rebase,
}

/// What to warn of before a commit in the Repository at `root`.
pub(crate) fn warning(runner: &GitRunner, root: &Path) -> Result<Option<CommitWarning>, Failure> {
    let git_dir = runner
        .git_dir(root)
        .map_err(git_failed_to_start)?
        .ok_or_else(|| Failure::ChangesUnreadable {
            folder: root.to_path_buf(),
        })?;
    // HEAD is detached during a rebase too, so the rebase is looked for first.
    if is_rebasing(&git_dir) {
        return Ok(Some(CommitWarning::Rebase));
    }
    let on_a_branch = runner
        .run(root, &["symbolic-ref", "--quiet", "HEAD"])
        .map_err(git_failed_to_start)?
        .success;
    Ok((!on_a_branch).then_some(CommitWarning::DetachedHead))
}

/// Whether the git directory `git_dir` holds a rebase in progress. `git am` keeps its state in
/// `rebase-apply` too, and marks it with a file of its own.
fn is_rebasing(git_dir: &Path) -> bool {
    let applying_patches = git_dir.join("rebase-apply");
    git_dir.join("rebase-merge").is_dir()
        || (applying_patches.is_dir() && !applying_patches.join("applying").exists())
}

fn text_of(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
