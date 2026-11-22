//! Operations in progress: a merge, rebase, revert or cherry-pick that git stopped half done, as
//! git keeps it in the git directory of the worktree, whoever started it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::GitRunner;
use crate::{Failure, Repository, git_failed_to_start};

/// A merge, rebase, revert or cherry-pick that is half done in a Repository, with what the user can
/// do about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationInProgress {
    pub kind: OperationKind,
    /// What the user can do about it, in the order to offer it.
    pub actions: Vec<OperationAction>,
}

/// Which operation is half done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    Merge,
    Rebase,
    Revert,
    CherryPick,
}

impl OperationKind {
    /// What the user can do about an operation of this kind, in the order to offer it.
    fn actions(self) -> Vec<OperationAction> {
        match self {
            Self::Merge => vec![OperationAction::CommitMerge, OperationAction::Abort],
            // A commit the resolution left empty cannot be continued, only skipped.
            Self::Rebase | Self::Revert | Self::CherryPick => vec![
                OperationAction::Continue,
                OperationAction::Skip,
                OperationAction::Abort,
            ],
        }
    }

    /// The git command that started it, which continues and aborts it too.
    fn command(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Rebase => "rebase",
            Self::Revert => "revert",
            Self::CherryPick => "cherry-pick",
        }
    }
}

/// The names git gives the commit a cherry-pick or a revert stopped on.
const STOPPED_ON: [(&str, OperationKind); 2] = [
    ("CHERRY_PICK_HEAD", OperationKind::CherryPick),
    ("REVERT_HEAD", OperationKind::Revert),
];

/// What the user can do about an operation in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationAction {
    /// Goes on with the operation once the user resolved the conflicts it stopped on, or did what
    /// it stopped for.
    Continue,
    /// Leaves out the commit a rebase, revert or cherry-pick stopped on, and goes on with the next
    /// one.
    Skip,
    /// Finishes a merge with a commit, with the message git prepared for it.
    CommitMerge,
    /// Gives up the operation, and brings back what HEAD and the files were before it started.
    Abort,
}

/// The operation in progress in `repository`, if one is.
pub(crate) fn read(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Option<OperationInProgress>, Failure> {
    let git_dir = git_dir(runner, repository)?;
    // A rebase that stops on a merge leaves what the merge left too, so it is looked for first.
    let kind = if is_rebasing(&git_dir) {
        Some(OperationKind::Rebase)
    } else if is_merging(&git_dir) {
        Some(OperationKind::Merge)
    } else {
        picking(runner, repository, &git_dir)?
    };
    Ok(kind.map(|kind| OperationInProgress {
        kind,
        actions: kind.actions(),
    }))
}

/// The git directory of the worktree `repository` is, where git keeps what it has half done there.
pub(crate) fn git_dir(runner: &GitRunner, repository: &Repository) -> Result<PathBuf, Failure> {
    runner
        .git_dir(&repository.root)
        .map_err(git_failed_to_start)?
        .ok_or_else(|| Failure::ChangesUnreadable {
            folder: repository.root.clone(),
        })
}

/// The arguments of the git that takes `action` on an operation of `kind`.
pub(crate) fn action_args(kind: OperationKind, action: OperationAction) -> [&'static str; 2] {
    let option = match action {
        OperationAction::Continue | OperationAction::CommitMerge => "--continue",
        OperationAction::Skip => "--skip",
        OperationAction::Abort => "--abort",
    };
    [kind.command(), option]
}

/// Where git stopped the operation in progress in a Repository, which tells gikli one operation
/// from another: its kind, the commit it stopped on and the commit HEAD was on then. Another one
/// that stops on the same commit with the same HEAD counts as the same one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OperationStop {
    pub kind: OperationKind,
    /// The commit a cherry-pick or a revert stopped on, by its full hash, as git names it
    /// `CHERRY_PICK_HEAD` or `REVERT_HEAD`, if it names one.
    pub stopped_on: Option<String>,
    /// The commit HEAD is on, by its full hash, if it is on one.
    pub head: Option<String>,
}

/// Where git stopped the operation of `kind` in progress in `repository`.
pub(crate) fn stop(
    runner: &GitRunner,
    repository: &Repository,
    kind: OperationKind,
) -> Result<OperationStop, Failure> {
    let name = STOPPED_ON.iter().find(|(_, named)| *named == kind);
    let stopped_on = match name {
        Some((name, _)) => commit_named(runner, repository, name)?,
        None => None,
    };
    Ok(OperationStop {
        kind,
        stopped_on,
        head: head(runner, repository)?,
    })
}

/// The commit HEAD of `repository` is on, if it is on one.
pub(crate) fn head(runner: &GitRunner, repository: &Repository) -> Result<Option<String>, Failure> {
    commit_named(runner, repository, "HEAD")
}

/// The commit `name` names in `repository`, by its full hash, if it names one.
fn commit_named(
    runner: &GitRunner,
    repository: &Repository,
    name: &str,
) -> Result<Option<String>, Failure> {
    let named = runner
        .run(
            &repository.root,
            &["rev-parse", "--quiet", "--verify", name],
        )
        .map_err(git_failed_to_start)?;
    Ok(named
        .success
        .then(|| named.stdout_text().trim_end().to_owned()))
}

/// The revert or cherry-pick half done in `repository`, whose git directory is `git_dir`, if one
/// is. git names the commit it stopped on `REVERT_HEAD` or `CHERRY_PICK_HEAD`, which the refs of
/// the Repository may keep outside the git directory. A commit the user makes in the middle of a
/// sequence of several takes that name away, and the sequencer of git still holds the commits
/// left to pick or revert.
fn picking(
    runner: &GitRunner,
    repository: &Repository,
    git_dir: &Path,
) -> Result<Option<OperationKind>, Failure> {
    for (name, kind) in STOPPED_ON {
        if commit_named(runner, repository, name)?.is_some() {
            return Ok(Some(kind));
        }
    }
    Ok(sequence_left(git_dir))
}

/// What the sequencer in the git directory `git_dir` still has to do, as the first command of its
/// list says, if it has anything.
fn sequence_left(git_dir: &Path) -> Option<OperationKind> {
    let todo = fs::read_to_string(git_dir.join("sequencer").join("todo")).ok()?;
    let command = todo
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))?
        .split_whitespace()
        .next()?;
    match command {
        "pick" | "p" => Some(OperationKind::CherryPick),
        "revert" => Some(OperationKind::Revert),
        _ => None,
    }
}

/// Whether the git directory `git_dir` holds a merge in progress.
pub(crate) fn is_merging(git_dir: &Path) -> bool {
    git_dir.join("MERGE_HEAD").is_file()
}

/// Whether the git directory `git_dir` holds a rebase in progress. `git am` keeps its state in
/// `rebase-apply` too, and marks it with a file of its own.
pub(crate) fn is_rebasing(git_dir: &Path) -> bool {
    let applying_patches = git_dir.join("rebase-apply");
    git_dir.join("rebase-merge").is_dir()
        || (applying_patches.is_dir() && !applying_patches.join("applying").exists())
}
