//! The branch engine's delete: local branches go, merged or not, and can come back on the same
//! commit.

use serde::{Deserialize, Serialize};

use crate::git::GitRunner;
use crate::{
    CurrentBranch, Failure, ListedCommit, RefKind, RefLabel, Repository, details, failure_of,
    git_failed_to_start, new_branch, push, sync,
};

/// A local branch gikli deleted, with what it takes to bring it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedBranch {
    pub name: String,
    /// The full hash of the commit it was on, where restoring it puts it again.
    pub commit: String,
    /// The branch of a remote it followed, if any.
    pub upstream: Option<RemoteBranch>,
    /// Whether it had commits HEAD did not have when it went.
    pub unmerged: bool,
}

/// A branch of a remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBranch {
    pub remote: String,
    /// Its name on the remote, such as `main` for `refs/heads/main`.
    pub branch: String,
}

impl RemoteBranch {
    /// Its whole reference on the remote, such as `refs/heads/main`.
    fn reference(&self) -> String {
        format!("refs/heads/{}", self.branch)
    }
}

/// Checks that `repository` has each of the local branches `names`, which returns
/// [`Failure::RevisionNotFound`] for the first it lacks, and that HEAD is on none of them, which
/// returns [`Failure::CurrentBranchNotDeleted`]: git keeps the branch HEAD is on.
pub(crate) fn check_deletable(
    runner: &GitRunner,
    repository: &Repository,
    names: &[String],
) -> Result<(), Failure> {
    let current = match sync::current_branch(runner, repository)? {
        CurrentBranch::Branch { name, .. } => Some(name),
        CurrentBranch::Detached { .. } => None,
    };
    for name in names {
        if current.as_ref() == Some(name) {
            return Err(Failure::CurrentBranchNotDeleted { name: name.clone() });
        }
        if !new_branch::has_local_branch(runner, repository, name)? {
            return Err(Failure::RevisionNotFound {
                revision: name.clone(),
            });
        }
    }
    Ok(())
}

/// The commit HEAD of `repository` is on, which the commits of a deleted branch are merged into or
/// not. A HEAD on a branch with no commit yet has none.
pub(crate) fn head_commit(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Option<String>, Failure> {
    match details::resolve(runner, repository, "HEAD") {
        Ok(commit) => Ok(Some(commit)),
        Err(Failure::RevisionNotFound { .. }) => Ok(None),
        Err(failure) => Err(failure),
    }
}

/// Deletes the local branch `name` of `repository`, merged or not, as `git branch -D`, and returns
/// what it takes to bring it back, with whether it had commits that `head`, the commit HEAD is on,
/// does not have. A branch that does not exist returns [`Failure::RevisionNotFound`].
pub(crate) fn delete(
    runner: &GitRunner,
    repository: &Repository,
    name: &str,
    head: Option<&str>,
) -> Result<DeletedBranch, Failure> {
    let commit =
        details::resolve_known_as(runner, repository, &format!("refs/heads/{name}"), name)?;
    let unmerged = !unmerged_commits(runner, repository, &commit, head, Some(1))?.is_empty();
    let upstream = upstream(runner, repository, name)?;
    let root = &repository.root;
    let deleted = runner
        .run(root, &["branch", "-D", "--", name])
        .map_err(git_failed_to_start)?;
    if !deleted.success {
        return Err(failure_of(
            runner,
            root,
            &deleted,
            Failure::BranchNotDeleted {
                folder: root.clone(),
                name: name.to_owned(),
            },
        ));
    }
    Ok(DeletedBranch {
        name: name.to_owned(),
        commit,
        upstream,
        unmerged,
    })
}

/// The branch of a remote the local branch `name` of `repository` follows, if it follows one.
fn upstream(
    runner: &GitRunner,
    repository: &Repository,
    name: &str,
) -> Result<Option<RemoteBranch>, Failure> {
    let label = RefLabel {
        name: name.to_owned(),
        kind: RefKind::Local,
    };
    match sync::branch_source(runner, repository, &label) {
        Ok(source) => Ok(Some(RemoteBranch {
            branch: source
                .reference
                .strip_prefix("refs/heads/")
                .unwrap_or(&source.reference)
                .to_owned(),
            remote: source.remote,
        })),
        Err(Failure::NoUpstream { .. }) => Ok(None),
        Err(failure) => Err(failure),
    }
}

/// Whether `repository` holds the remote branch that stands for `upstream`, as the last fetch
/// left it: the remote has it, as far as gikli knows, so it can be deleted there.
pub(crate) fn has_remote_branch(
    runner: &GitRunner,
    repository: &Repository,
    upstream: &RemoteBranch,
) -> Result<bool, Failure> {
    match sync::fetched_from(runner, repository, &upstream.remote, &upstream.reference())? {
        Some(remote_branch) => details::exists(runner, repository, &remote_branch),
        None => Ok(false),
    }
}

/// The arguments of git that delete `upstream` on its remote, by its whole reference, so that a
/// tag with the same name does not get in the way. git deletes the remote branch here too.
pub(crate) fn delete_remote_branch_args(upstream: &RemoteBranch) -> Vec<String> {
    vec![
        "push".to_owned(),
        "--progress".to_owned(),
        "--end-of-options".to_owned(),
        upstream.remote.clone(),
        format!(":{}", upstream.reference()),
    ]
}

/// The commits of `commit` in `repository` that `head` does not have, newest first, the first
/// `limit` of them when there is a limit. Without `head`, every commit of `commit` counts.
pub(crate) fn unmerged_commits(
    runner: &GitRunner,
    repository: &Repository,
    commit: &str,
    head: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<ListedCommit>, Failure> {
    let limit = limit.map(|limit| format!("--max-count={limit}"));
    let mut revisions = vec![commit];
    revisions.extend(limit.as_deref());
    if let Some(head) = head {
        revisions.extend(["--not", head]);
    }
    push::commits(runner, repository, &revisions)
}

/// Creates the branches `branches` of `repository` again, each on the commit it was on. When a
/// branch has one of their names by now, none is created and this returns
/// [`Failure::BranchNameTaken`].
pub(crate) fn restore(
    runner: &GitRunner,
    repository: &Repository,
    branches: &[DeletedBranch],
) -> Result<(), Failure> {
    for branch in branches {
        if new_branch::has_local_branch(runner, repository, &branch.name)? {
            return Err(Failure::BranchNameTaken {
                name: branch.name.clone(),
            });
        }
    }
    for branch in branches {
        new_branch::create(runner, repository, &branch.name, &branch.commit)?;
        if let Some(upstream) = &branch.upstream {
            follow(runner, repository, &branch.name, upstream)?;
        }
    }
    Ok(())
}

/// Sets `upstream` as the upstream of the local branch `name` of `repository` again, as git had it,
/// even when the remote branch is gone by now.
fn follow(
    runner: &GitRunner,
    repository: &Repository,
    name: &str,
    upstream: &RemoteBranch,
) -> Result<(), Failure> {
    let merge = upstream.reference();
    let settings = [
        (format!("branch.{name}.remote"), upstream.remote.as_str()),
        (format!("branch.{name}.merge"), merge.as_str()),
    ];
    let root = &repository.root;
    for (key, value) in &settings {
        let set = runner
            .run(root, &["config", "--end-of-options", key, value])
            .map_err(git_failed_to_start)?;
        if !set.success {
            return Err(failure_of(
                runner,
                root,
                &set,
                new_branch::not_created(repository, name),
            ));
        }
    }
    Ok(())
}
