//! A shallow clone: a Repository whose history is cut, as `git clone --depth` leaves it, and the
//! fetch that brings the rest.

use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;

use crate::git::{GitRunner, Invocation};
use crate::{Failure, Repository, git_failed_to_start};

/// The hashes of the commits where the history of `repository` is cut, as git keeps them in the
/// `shallow` file of its git directory, one a line. Empty when the history is whole, and git
/// keeps no file. A git that cannot say where its git directory is fails with [`io::ErrorKind::NotFound`].
pub(crate) fn cut_commits(
    runner: &GitRunner,
    repository: &Repository,
) -> io::Result<BTreeSet<String>> {
    let path = runner
        .relative_git_path(&repository.root, "shallow")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "git names no git directory"))?;
    match std::fs::read_to_string(repository.root.join(path)) {
        Ok(cut) => Ok(cut.split_whitespace().map(str::to_owned).collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(error) => Err(error),
    }
}

/// The lock git takes on the `shallow` file of `repository` while it changes where the history is
/// cut, as a fetch of the rest does from its start. A git killed while it holds it, as a cancelled
/// fetch is, leaves it behind, and every later fetch of the rest fails on it.
pub(crate) fn cut_lock(runner: &GitRunner, repository: &Repository) -> io::Result<PathBuf> {
    let path = runner
        .relative_git_path(&repository.root, "shallow.lock")?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "git names no git directory"))?;
    Ok(repository.root.join(path))
}

/// The arguments of the git that fetches the rest of the history of a Repository from `remote`,
/// into the remote branches its configuration names.
pub(crate) fn fetch_rest_args(remote: &str) -> [&str; 5] {
    [
        "fetch",
        "--progress",
        "--unshallow",
        "--end-of-options",
        remote,
    ]
}

/// Has `remote` of `repository` fetch every branch of the remote from now on, as `git remote
/// set-branches <remote> '*'` does, unless its configuration already names its branches with a
/// pattern. `git clone --depth` tracks a single branch, and `git fetch --unshallow` alone keeps it
/// that way, so the other branches would not come, or would go stale. Returns the run of git that
/// failed, if one did.
pub(crate) fn track_every_branch(
    runner: &GitRunner,
    repository: &Repository,
    remote: &str,
) -> Result<Option<Invocation>, Failure> {
    let root = &repository.root;
    let key = format!("remote.{remote}.fetch");
    let configured = runner
        .run(root, &["config", "--get-all", &key])
        .map_err(git_failed_to_start)?;
    if configured.stdout_text().contains('*') {
        return Ok(None);
    }
    let tracked = runner
        .run(root, &["remote", "set-branches", "--", remote, "*"])
        .map_err(git_failed_to_start)?;
    Ok((!tracked.success).then_some(tracked))
}

/// The remote git fetches from in `repository` when it is given none: the one the current branch
/// fetches from, otherwise `origin`. When neither is a remote of the Repository, it is the first
/// remote git lists, and None when there is none.
pub(crate) fn remote_to_fetch_from(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Option<String>, Failure> {
    let root = &repository.root;
    let run = |args: &[&str]| runner.run(root, args).map_err(git_failed_to_start);
    let listed = run(&["remote"])?;
    let remotes: Vec<String> = listed.stdout_text().lines().map(str::to_owned).collect();
    // `--quiet` fails quietly on a detached HEAD, which fetches from no branch's remote.
    let head = run(&["symbolic-ref", "--quiet", "HEAD"])?;
    let branch = head.stdout_text();
    let upstream = match branch.trim_end().strip_prefix("refs/heads/") {
        Some(branch) if head.success => {
            let key = format!("branch.{branch}.remote");
            let configured = run(&["config", "--get", &key])?;
            configured
                .success
                .then(|| configured.stdout_text().trim_end().to_owned())
        }
        _ => None,
    };
    let preferred = [upstream, Some("origin".to_owned())];
    Ok(preferred
        .into_iter()
        .flatten()
        .find(|remote| remotes.contains(remote))
        .or_else(|| remotes.first().cloned()))
}
