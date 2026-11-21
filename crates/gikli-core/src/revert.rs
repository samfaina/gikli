//! The integration engine's revert: commits of the current branch the user selected in the log,
//! undone by inverse commits, one per commit, each reviewed in the commit panel.

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::git::GitRunner;
use crate::project::{take_once, write_at_once};
use crate::{Failure, Repository, cherry_pick, details, git_failed_to_start};

/// The inverse of a commit, which git brought into the working tree and the index without
/// committing it, for the user to commit from the commit panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedRevert {
    /// The commit it undoes, by its full hash.
    pub commit: String,
    /// The automatic message of the inverse commit, as git words it:
    /// `Revert "<subject>"`, a blank line and `This reverts commit <hash>.`
    pub message: String,
    /// The files the commit changed, ordered by path, which the commit panel ticks to begin with.
    /// A file it renamed is there at both paths.
    pub paths: Vec<String>,
}

/// The arguments after `git` that bring the inverse of `commit` into the working tree and the
/// index, without committing it. git keeps `REVERT_HEAD` meanwhile, so the revert is in progress
/// until the user commits it, or skips or aborts it.
pub(crate) fn prepare_args(commit: &str) -> [&str; 4] {
    ["revert", "--no-commit", "--end-of-options", commit]
}

/// The revert of one commit half done in `repository`, as git names it `REVERT_HEAD`, if one is.
pub(crate) fn prepared(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Option<PreparedRevert>, Failure> {
    let reverting = runner
        .run(
            &repository.root,
            &["rev-parse", "--quiet", "--verify", "REVERT_HEAD^{commit}"],
        )
        .map_err(git_failed_to_start)?;
    if !reverting.success {
        return Ok(None);
    }
    let commit = reverting.stdout_text().trim_end().to_owned();
    Ok(Some(PreparedRevert {
        message: message(runner, repository, &commit)?,
        paths: changed_paths(runner, repository, &commit)?,
        commit,
    }))
}

/// The message git gives the inverse commit of `commit`.
fn message(runner: &GitRunner, repository: &Repository, commit: &str) -> Result<String, Failure> {
    let subject = runner
        .run(
            &repository.root,
            &["log", "-1", "--format=%s", "--end-of-options", commit],
        )
        .map_err(git_failed_to_start)?;
    if !subject.success {
        return Err(Failure::RevisionNotFound {
            revision: commit.to_owned(),
        });
    }
    let subject = subject.stdout_text();
    let subject = subject.trim_end();
    Ok(format!(
        "Revert \"{subject}\"\n\nThis reverts commit {commit}."
    ))
}

/// The files `commit` changed against its first parent, or against nothing for a root commit,
/// ordered by path, a rename at both its paths.
fn changed_paths(
    runner: &GitRunner,
    repository: &Repository,
    commit: &str,
) -> Result<Vec<String>, Failure> {
    let changed = runner
        .run(
            &repository.root,
            &[
                "diff-tree",
                "-r",
                "-z",
                "--root",
                "--no-renames",
                "--no-commit-id",
                "--name-only",
                "--end-of-options",
                commit,
            ],
        )
        .map_err(git_failed_to_start)?;
    if !changed.success {
        return Err(Failure::RevisionNotFound {
            revision: commit.to_owned(),
        });
    }
    Ok(changed
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .collect())
}

/// The commits `revisions` of `repository` name, by their full hashes, in the same order, once
/// the current branch holds them all. One it does not hold returns
/// [`Failure::CommitNotOnCurrentBranch`], and one that names no commit
/// [`Failure::RevisionNotFound`].
pub(crate) fn commits_to_revert(
    runner: &GitRunner,
    repository: &Repository,
    revisions: &[String],
) -> Result<Vec<String>, Failure> {
    let mut commits = Vec::with_capacity(revisions.len());
    for revision in revisions {
        let commit = details::resolve(runner, repository, revision)?;
        if !cherry_pick::on_current_branch(runner, repository, &commit)? {
            return Err(Failure::CommitNotOnCurrentBranch { commit });
        }
        commits.push(commit);
    }
    Ok(commits)
}

/// Where the commits of a revert from gikli that wait for the one in progress are kept, in the
/// git directory of the worktree, so whoever goes on with it from gikli reverts them after it.
pub(crate) struct LeftToRevert {
    file: PathBuf,
}

impl LeftToRevert {
    /// The store in `git_dir`, which is the git directory of the worktree.
    pub fn in_git_dir(git_dir: &Path) -> Self {
        Self {
            file: git_dir.join("gikli").join("left-to-revert.json"),
        }
    }

    /// Keeps `commits`, in place of what waited before.
    pub fn keep(&self, commits: &[String]) -> Result<(), Failure> {
        let json = serde_json::to_vec_pretty(commits).map_err(io::Error::other);
        json.and_then(|json| write_at_once(&self.file, &json))
            .map_err(|error| Failure::CommitsToRevertNotKept {
                file: self.file.clone(),
                message: error.to_string(),
            })
    }

    /// Takes what waits, so that one taker alone reverts it. Nothing waits when nothing can be
    /// read.
    pub fn take(&self) -> Vec<String> {
        take_once(&self.file)
            .and_then(|json| serde_json::from_slice(&json).ok())
            .unwrap_or_default()
    }
}
