//! The branch engine's rename: a local branch takes a new name, and can let go of its upstream.

use crate::git::GitRunner;
use crate::{Failure, Repository, failure_of, git_failed_to_start, new_branch, sync};

/// Renames the local branch `name` of `repository` to `new_name`, as `git branch -m`, which keeps
/// its commits, its reflog and its upstream, and moves HEAD along when it is on the branch. With
/// `unset_upstream`, the branch then follows nothing, as `git branch --unset-upstream` leaves it.
/// A branch that does not exist returns [`Failure::RevisionNotFound`], and a new name git does not
/// take as it is or that another branch has fails as a new branch's name would.
pub(crate) fn rename(
    runner: &GitRunner,
    repository: &Repository,
    name: &str,
    new_name: &str,
    unset_upstream: bool,
) -> Result<(), Failure> {
    if !new_branch::has_local_branch(runner, repository, name)? {
        return Err(Failure::RevisionNotFound {
            revision: name.to_owned(),
        });
    }
    new_branch::check_name(runner, repository, new_name)?;
    run(
        runner,
        repository,
        name,
        &["branch", "-m", "--", name, new_name],
    )?;
    if unset_upstream && has_upstream(runner, repository, new_name)? {
        run(
            runner,
            repository,
            name,
            &["branch", "--unset-upstream", new_name],
        )?;
    }
    Ok(())
}

/// Whether the local branch `name` of `repository` follows a branch: git refuses to unset an
/// upstream it does not have.
fn has_upstream(runner: &GitRunner, repository: &Repository, name: &str) -> Result<bool, Failure> {
    Ok(sync::configured(runner, repository, &format!("branch.{name}.merge"))?.is_some())
}

/// Runs git with `args` on the branch `name` of `repository`, failing with what git's output
/// explains, or with [`Failure::BranchNotRenamed`].
fn run(
    runner: &GitRunner,
    repository: &Repository,
    name: &str,
    args: &[&str],
) -> Result<(), Failure> {
    let root = &repository.root;
    let invocation = runner.run(root, args).map_err(git_failed_to_start)?;
    if !invocation.success {
        return Err(failure_of(
            runner,
            root,
            &invocation,
            Failure::BranchNotRenamed {
                folder: root.clone(),
                name: name.to_owned(),
            },
        ));
    }
    Ok(())
}
