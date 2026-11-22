//! Committing: what git has to do for the changes the user selected to end up in one commit.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::git::GitRunner;
use crate::status::{ChangeSet, ChangeStatus, Status};
use crate::{Failure, failure_of, git_failed_to_start, in_progress, trace};

/// How many previous commit messages the user can choose from.
const PREVIOUS_MESSAGES_OFFERED: usize = 20;

/// What a commit of a selection of changes works on. The commit goes through the index, as git
/// refuses `git commit --only` during a merge or a cherry-pick (ADR 0010): what is staged at the
/// paths it touches is taken out, the selected paths are added as they are in the working tree, and
/// once git committed, what was left out is staged again.
#[derive(Default)]
pub(crate) struct Selection {
    /// Every path the commit records as it is in the working tree, Unversioned Files among them.
    pub committed: Vec<String>,
    /// Paths with something staged that the commit records as in the last commit, as nothing is
    /// there to add: where a staged move went, when its file was then removed from disk.
    pub cleared: Vec<String>,
    /// Paths with something staged that the user did not select. It stays out of the commit, and
    /// is staged again after it.
    pub left_out: Vec<String>,
    /// The selected files added with `--intent-to-add`. The index holds no content for them, so
    /// the tree of the index before the commit leaves them out.
    pub intended: Vec<String>,
}

impl Selection {
    /// The changes in `status` at `paths` that a commit can record: those in a Changelist and the
    /// Unversioned Files. Ignored files are no changes. `status` must list the ignored files, as one
    /// can sit where a moved file was.
    pub fn of(status: &Status, paths: &[String]) -> Self {
        let chosen = status
            .changes
            .iter()
            .filter(|change| change.status != ChangeStatus::Ignored)
            .filter(|change| paths.iter().any(|path| Path::new(path) == change.path));
        let mut committed = Vec::new();
        let mut cleared = Vec::new();
        let mut intended = Vec::new();
        for change in chosen {
            committed.push(text_of(&change.path));
            // The working tree side says added only for a file added with `--intent-to-add`.
            let is_intended = status
                .unstaged
                .iter()
                .any(|file| file.path == change.path && file.status == ChangeStatus::Added);
            if is_intended {
                intended.push(text_of(&change.path));
            }
            // A move is two changes to git: leaving out one would leave it staged. Unless something
            // new sits at the old path, as git would add that from the working tree instead.
            committed.extend(
                change
                    .renamed_from()
                    .filter(|from| !is_taken_by_a_new_file(status, from))
                    .map(text_of),
            );
            cleared.extend(
                status
                    .moved_and_removed
                    .get(&change.path)
                    .map(|to| text_of(to)),
            );
        }
        let touched = |path: &Path| {
            let path = text_of(path);
            committed.contains(&path) || cleared.contains(&path)
        };
        let mut left_out = Vec::new();
        for change in &status.staged {
            let paths = std::iter::once(change.path.as_path()).chain(change.renamed_from());
            left_out.extend(paths.filter(|path| !touched(path)).map(text_of));
        }
        Self {
            committed,
            cleared,
            left_out,
            intended,
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

/// A commit to make: what it records, with which message, and whether it is a new one.
#[derive(Clone, Copy)]
pub(crate) struct Commit<'a> {
    /// The selected files it records, in Changelist mode.
    pub selection: &'a Selection,
    /// Its message, as the user wrote it.
    pub message: &'a str,
    pub scope: CommitScope,
    pub kind: CommitKind,
}

/// What a commit records.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitScope {
    /// The working tree content of the selected paths, as in Changelist mode. gikli prepares the
    /// index for it.
    Selection,
    /// The git index as it is, as in Staging mode.
    Index,
}

/// What git reads for one commit or one unstaging: the message and the paths, which do not fit on
/// a command line when a Changelist is large. The files are in gikli's folder of the git directory, the one place
/// git inside WSL and gikli on Windows both reach by a path of their own. They go when this is
/// dropped.
pub(crate) struct CommitInput {
    /// Where gikli wrote them.
    folder: PathBuf,
    /// The same folder, relative to the root of the Repository, as git gets it.
    folder_for_git: String,
    /// The files of paths that hold any. git takes an empty list of paths for the whole
    /// Repository, so a command on one of the others does not run.
    with_paths: Vec<&'static str>,
}

const MESSAGE: &str = "message";
/// The paths a commit records from the working tree.
const PATHS: &str = "paths";
/// Every path whose staged content a commit of a selection takes out of the index first.
const TAKEN_OUT_PATHS: &str = "taken-out-paths";
/// The paths whose staged content a commit of a selection stages again after it.
const LEFT_OUT_PATHS: &str = "left-out-paths";
/// The selected files added with `--intent-to-add`.
const INTENDED_PATHS: &str = "intended-paths";

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
        let mut input = Self {
            folder,
            folder_for_git: format!("{git_dir}/gikli/{name}"),
            with_paths: Vec::new(),
        };
        // git would take a last line without an end as it is, and other tools do not expect that.
        let line_end = if message.ends_with('\n') { "" } else { "\n" };
        fs::write(input.folder.join(MESSAGE), format!("{message}{line_end}"))?;
        let taken_out: Vec<String> = [
            &selection.committed,
            &selection.cleared,
            &selection.left_out,
        ]
        .into_iter()
        .flatten()
        .cloned()
        .collect();
        input.write_paths(PATHS, &selection.committed)?;
        input.write_paths(TAKEN_OUT_PATHS, &taken_out)?;
        input.write_paths(LEFT_OUT_PATHS, &selection.left_out)?;
        input.write_paths(INTENDED_PATHS, &selection.intended)?;
        Ok(input)
    }

    /// Writes `paths` to the file `name`.
    fn write_paths(&mut self, name: &'static str, paths: &[String]) -> io::Result<()> {
        fs::write(self.folder.join(name), nul_separated(paths))?;
        if !paths.is_empty() {
            self.with_paths.push(name);
        }
        Ok(())
    }

    /// The arguments after `git` that write the index as a tree and print its name, for what a
    /// commit of a selection puts back. git refuses while a Conflict is left.
    pub fn index_tree_command() -> Vec<String> {
        vec!["write-tree".to_owned()]
    }

    /// The arguments after `git` that take what is staged at every path a commit of a selection
    /// touches out of the index, leaving the working tree, when it touches any. `git reset` needs
    /// no commit to reset to: on none it takes the paths out.
    pub fn take_out_command(&self) -> Option<Vec<String>> {
        self.command_on_any(TAKEN_OUT_PATHS, &["reset", "--quiet"])
    }

    /// The arguments after `git` that stage the paths a commit of a selection records as they are
    /// in the working tree, removals and Unversioned Files too.
    pub fn add_command(&self) -> Option<Vec<String>> {
        self.command_on_any(PATHS, &["add", "--all"])
    }

    /// The arguments after `git` that stage again what a commit of a selection left out, as it was
    /// in `tree`, the index before the commit, when it left out anything.
    pub fn stage_again_command(&self, tree: &str) -> Option<Vec<String>> {
        self.command_on_any(LEFT_OUT_PATHS, &["reset", "--quiet", tree])
    }

    /// The arguments after `git` of each command that puts the index back at every path a commit
    /// of a selection touches as it was in `tree`, the index before the commit, when the commit
    /// fails. A selected Unversioned File is not in it, so it goes back to being unversioned, and a
    /// file added with `--intent-to-add` is added that way again.
    pub fn put_back_commands(&self, tree: &str) -> Vec<Vec<String>> {
        [
            self.command_on_any(TAKEN_OUT_PATHS, &["reset", "--quiet", tree]),
            self.command_on_any(INTENDED_PATHS, &["add", "--intent-to-add"]),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// The arguments after `git` that take the paths out of the index and leave the working tree.
    /// `git restore` needs a commit to restore from, when `has_commits` says there is none.
    pub fn unstage_command(&self, has_commits: bool) -> Vec<String> {
        let command: &[&str] = if has_commits {
            &["restore", "--staged"]
        } else {
            &["rm", "--quiet", "--force", "--cached"]
        };
        self.command_on(PATHS, command)
    }

    /// The arguments after `git` that commit what the index holds. The message goes in as written:
    /// the cleanup is not left to `commit.cleanup`, and `commit.verbose` would cut it at a scissors
    /// line. No paths: git refuses a commit of some paths during a merge or a cherry-pick.
    pub fn commit_command(&self, kind: CommitKind) -> Vec<String> {
        let mut command: Vec<String> = ["commit", "--cleanup=verbatim", "--no-verbose"]
            .map(str::to_owned)
            .to_vec();
        if kind == CommitKind::Amend {
            command.push("--amend".to_owned());
        }
        command.push(format!("--file={}/{MESSAGE}", self.folder_for_git));
        command
    }

    /// [`Self::command_on`], when the file `paths` holds any.
    fn command_on_any(&self, paths: &'static str, command: &[&str]) -> Option<Vec<String>> {
        self.with_paths
            .contains(&paths)
            .then(|| self.command_on(paths, command))
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

/// The email of the user's Commit identity in the Repository at `root`: the `user.email` git has
/// now, if it is not empty.
pub(crate) fn user_email(runner: &GitRunner, root: &Path) -> Result<Option<String>, Failure> {
    let email = runner
        .run(root, &["config", "--get", "user.email"])
        .map_err(git_failed_to_start)?
        .stdout_text();
    let email = email.trim();
    Ok((!email.is_empty()).then(|| email.to_owned()))
}

/// The messages of the user's last commits in the Repository at `root`, on any local branch, newest
/// first and each one once. A commit is theirs when it has the `user.email` git has now, so without
/// one none is. Merges are left out, as git wrote their messages.
pub(crate) fn previous_messages(runner: &GitRunner, root: &Path) -> Result<Vec<String>, Failure> {
    let Some(email) = user_email(runner, root)? else {
        return Ok(Vec::new());
    };
    if !has_commits(runner, root)? {
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
    if in_progress::is_rebasing(&git_dir) {
        return Ok(Some(CommitWarning::Rebase));
    }
    let on_a_branch = runner
        .run(root, &["symbolic-ref", "--quiet", "HEAD"])
        .map_err(git_failed_to_start)?
        .success;
    Ok((!on_a_branch).then_some(CommitWarning::DetachedHead))
}

fn text_of(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
