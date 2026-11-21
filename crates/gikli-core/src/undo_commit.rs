//! The integration engine's undo commit: the last commit of the current branch goes, and its
//! changes stay, uncommitted.

use crate::git::GitRunner;
use crate::{Failure, Repository, details, failure_of, git_failed_to_start, push, sync};

/// Takes `commit`, the last commit of `branch`, the current branch of `repository`, off the branch,
/// and leaves its changes in the index and the working tree, as `git reset --soft` does. A root
/// commit leaves the branch with no commit, and every file it held added in the index.
pub(crate) fn undo(
    runner: &GitRunner,
    repository: &Repository,
    branch: &str,
    commit: &str,
) -> Result<(), Failure> {
    let root = &repository.root;
    let reference = format!("refs/heads/{branch}");
    let undone = match details::resolve(runner, repository, &format!("{commit}~")) {
        // No `--end-of-options`: before git 2.44, `git reset` takes it for a revision. The parent
        // is a full hash, which no option starts like, and `--` ends the revision.
        Ok(parent) => runner.run(root, &["reset", "--soft", &parent, "--"]),
        // `git reset` cannot go back past the root commit. Deleting the branch, while HEAD stays on
        // it, leaves the index as it is.
        Err(Failure::RevisionNotFound { .. }) => {
            runner.run(root, &["update-ref", "-d", &reference, commit])
        }
        Err(failure) => return Err(failure),
    }
    .map_err(git_failed_to_start)?;
    if !undone.success {
        return Err(failure_of(
            runner,
            root,
            &undone,
            Failure::CommitNotUndone {
                folder: root.clone(),
            },
        ));
    }
    Ok(())
}

/// The remote branch of a protected branch, such as `origin/main`, that holds `commit` in
/// `repository`, as the last fetch or push left it, if one does: the commit is shared history.
pub(crate) fn protected_branch_holding(
    runner: &GitRunner,
    repository: &Repository,
    commit: &str,
) -> Result<Option<String>, Failure> {
    let remotes = sync::remotes(runner, repository)?;
    let contains = format!("--contains={commit}");
    let listed = sync::read_branches(
        runner,
        repository,
        &[
            "for-each-ref",
            "--format=%(refname:lstrip=2)",
            &contains,
            "refs/remotes/",
        ],
    )?;
    Ok(listed
        .lines()
        .find(|remote_branch| {
            remotes.iter().any(|remote| {
                remote_branch
                    .strip_prefix(remote.as_str())
                    .and_then(|rest| rest.strip_prefix('/'))
                    .is_some_and(push::is_protected)
            })
        })
        .map(str::to_owned))
}
