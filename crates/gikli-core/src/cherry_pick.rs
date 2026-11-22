//! The integration engine's cherry-pick: commits the user selected in the log, brought onto the
//! current branch in order.

use serde::{Deserialize, Serialize};

use crate::git::GitRunner;
use crate::{Failure, Repository, details, git_failed_to_start, undo_commit};

/// The commits `revisions` of `repository` name, by their full hashes, in the same order, once
/// none of them is on the current branch already. One that is returns
/// [`Failure::CommitOnCurrentBranch`], and one that names no commit
/// [`Failure::RevisionNotFound`].
pub(crate) fn commits_to_pick(
    runner: &GitRunner,
    repository: &Repository,
    revisions: &[String],
) -> Result<Vec<String>, Failure> {
    let mut commits = Vec::with_capacity(revisions.len());
    for revision in revisions {
        let commit = details::resolve(runner, repository, revision)?;
        if on_current_branch(runner, repository, &commit)? {
            return Err(Failure::CommitOnCurrentBranch { commit });
        }
        commits.push(commit);
    }
    Ok(commits)
}

/// Whether HEAD of `repository` reaches `commit`, so the current branch holds it.
pub(crate) fn on_current_branch(
    runner: &GitRunner,
    repository: &Repository,
    commit: &str,
) -> Result<bool, Failure> {
    let reached = runner
        .run(
            &repository.root,
            &["merge-base", "--is-ancestor", commit, "HEAD"],
        )
        .map_err(git_failed_to_start)?;
    Ok(reached.success)
}

/// Commits that one git cherry-pick picks in a row, all with where they came from added to their
/// message or all without.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Run {
    /// Whether git adds `(cherry picked from commit <hash>)` to the message of each commit.
    pub adds_picked_from: bool,
    /// The commits, by their full hashes, in the order to pick them.
    pub commits: Vec<String>,
}

impl Run {
    /// The arguments after `git` of the cherry-pick of the run.
    pub fn args(&self) -> Vec<String> {
        let mut args = vec!["cherry-pick".to_owned()];
        if self.adds_picked_from {
            args.push("-x".to_owned());
        }
        args.push("--end-of-options".to_owned());
        args.extend(self.commits.iter().cloned());
        args
    }
}

/// `commits` of `repository`, in order, as the runs git picks them in: with `adds_picked_from`, each commit
/// that a protected branch of a remote holds has where it came from added to its message, and
/// the others do not, so a run ends where that changes.
pub(crate) fn runs(
    runner: &GitRunner,
    repository: &Repository,
    commits: &[String],
    adds_picked_from: bool,
) -> Result<Vec<Run>, Failure> {
    let mut runs: Vec<Run> = Vec::new();
    for commit in commits {
        let adds = adds_picked_from
            && undo_commit::protected_branch_holding(runner, repository, commit)?.is_some();
        match runs.last_mut() {
            Some(run) if run.adds_picked_from == adds => run.commits.push(commit.clone()),
            _ => runs.push(Run {
                adds_picked_from: adds,
                commits: vec![commit.clone()],
            }),
        }
    }
    Ok(runs)
}
