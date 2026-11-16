//! A shallow clone: a Repository whose history is cut, as `git clone --depth` leaves it, and the
//! fetch that brings the rest.

use crate::git::GitRunner;
use crate::{Failure, Repository, git_failed_to_start};

/// The commits where the history of `repository` is cut, one hash a line, as git keeps them in the
/// `shallow` file of its git directory. Nothing when the history is whole, and git keeps no file.
pub(crate) fn cut_commits(runner: &GitRunner, repository: &Repository) -> Result<String, Failure> {
    let unreadable = || Failure::LogUnreadable {
        folder: repository.root.clone(),
    };
    let path = runner
        .relative_git_path(&repository.root, "shallow")
        .map_err(git_failed_to_start)?
        .ok_or_else(unreadable)?;
    match std::fs::read_to_string(repository.root.join(path)) {
        Ok(cut) => Ok(cut),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(_) => Err(unreadable()),
    }
}

/// The arguments of the git that fetches the rest of the history of a Repository from `remote`.
/// It fetches every branch of the remote into its remote branches, whatever branches the remote's
/// configuration names: `git clone --depth` tracks a single branch, and `git fetch --unshallow`
/// alone keeps it that way. The configuration stays as it is.
pub(crate) fn fetch_rest_args(remote: &str) -> Vec<String> {
    [
        "fetch",
        "--progress",
        "--unshallow",
        "--end-of-options",
        remote,
        &format!("+refs/heads/*:refs/remotes/{remote}/*"),
    ]
    .map(str::to_owned)
    .to_vec()
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
