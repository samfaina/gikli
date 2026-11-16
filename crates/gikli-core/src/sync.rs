//! The sync engine: what fetching from the remotes asks of git, and how far each local branch is
//! from its upstream.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::git::GitRunner;
use crate::{Failure, RefKind, RefLabel, Repository, failure_of, git_failed_to_start};

/// The branch a local branch is compared with: the remote branch it pulls from, or another local
/// branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Upstream {
    /// Its name as the user knows it, such as `origin/main`.
    pub name: String,
    /// How far the local branch is from it. None when it is gone: the branch is configured to
    /// follow it, and it no longer exists, as when someone deleted it on the remote and a fetch
    /// pruned it.
    pub commits: Option<IncomingOutgoing>,
}

/// How far a local branch is from its upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingOutgoing {
    /// The commits of the upstream the local branch does not have: what a pull would bring.
    pub incoming: usize,
    /// The commits of the local branch its upstream does not have: what a push would send.
    pub outgoing: usize,
}

/// A local branch, with its upstream if it has one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalBranch {
    pub name: String,
    pub upstream: Option<Upstream>,
}

/// What HEAD is on, as the status area shows it at all times.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CurrentBranch {
    /// HEAD is on the local branch `name`, which may have no commit yet.
    Branch {
        name: String,
        upstream: Option<Upstream>,
    },
    /// HEAD is detached, on the commit `hash`.
    Detached { hash: String },
}

/// The arguments of the git that fetches every remote, pruning the remote branches that are gone
/// from them.
pub(crate) const FETCH_ALL_ARGS: [&str; 4] = ["fetch", "--progress", "--all", "--prune"];

/// Where a branch the user fetches on its own comes from: the branch `reference` of `remote`.
pub(crate) struct BranchSource {
    pub remote: String,
    pub reference: String,
}

impl BranchSource {
    /// The arguments of the git that fetches it. git updates the remote branch the configuration of
    /// the remote maps it to, and no other.
    pub fn fetch_args(&self) -> [&str; 5] {
        [
            "fetch",
            "--progress",
            "--end-of-options",
            &self.remote,
            &self.reference,
        ]
    }
}

/// Where `branch` of `repository` comes from when the user fetches it on its own: for a local
/// branch, the branch of the remote its upstream is; for a remote branch, the branch of the remote
/// whose configuration fetches into it. A local branch without an upstream, or one that follows
/// another local branch, fails with [`Failure::NoUpstream`], as does what is no branch. A remote
/// branch no remote fetches into fails with [`Failure::NoRemoteToFetchFrom`].
pub(crate) fn branch_source(
    runner: &GitRunner,
    repository: &Repository,
    branch: &RefLabel,
) -> Result<BranchSource, Failure> {
    let no_upstream = || Failure::NoUpstream {
        branch: branch.name.clone(),
    };
    match branch.kind {
        RefKind::Local => {
            let remote = configured(
                runner,
                repository,
                &format!("branch.{}.remote", branch.name),
            )?;
            let merge = configured(runner, repository, &format!("branch.{}.merge", branch.name))?;
            match (remote, merge) {
                // `.` is the Repository itself: the upstream is another local branch.
                (Some(remote), Some(reference)) if remote != "." => {
                    Ok(BranchSource { remote, reference })
                }
                _ => Err(no_upstream()),
            }
        }
        RefKind::Remote => {
            let tracking = format!("refs/remotes/{}", branch.name);
            fetched_into(runner, repository, &tracking)?.ok_or_else(|| {
                Failure::NoRemoteToFetchFrom {
                    folder: repository.root.clone(),
                }
            })
        }
        RefKind::Head | RefKind::Tag => Err(no_upstream()),
    }
}

/// The value of the configuration `key` in `repository`, if it is set.
fn configured(
    runner: &GitRunner,
    repository: &Repository,
    key: &str,
) -> Result<Option<String>, Failure> {
    let read = runner
        .run(
            &repository.root,
            &["config", "--get", "--end-of-options", key],
        )
        .map_err(git_failed_to_start)?;
    Ok(read
        .success
        .then(|| read.stdout_text().trim_end().to_owned()))
}

/// The branch of a remote of `repository` that the configuration of the remote fetches into the
/// remote branch `tracking`, a full reference name, if one does.
fn fetched_into(
    runner: &GitRunner,
    repository: &Repository,
    tracking: &str,
) -> Result<Option<BranchSource>, Failure> {
    let listed = runner
        .run(
            &repository.root,
            &["config", "--get-regexp", r"^remote\..*\.fetch$"],
        )
        .map_err(git_failed_to_start)?;
    // Each line is the key, then the refspec, such as `+refs/heads/*:refs/remotes/origin/*`.
    Ok(listed.stdout_text().lines().find_map(|line| {
        let (key, refspec) = line.split_once(' ')?;
        let remote = key.strip_prefix("remote.")?.strip_suffix(".fetch")?;
        let (source, destination) = refspec.trim_start_matches('+').split_once(':')?;
        let reference = match (source.split_once('*'), destination.split_once('*')) {
            (Some((source_start, source_end)), Some((start, end))) => {
                let matched = tracking.strip_prefix(start)?.strip_suffix(end)?;
                format!("{source_start}{matched}{source_end}")
            }
            (None, None) if destination == tracking => source.to_owned(),
            _ => return None,
        };
        Some(BranchSource {
            remote: remote.to_owned(),
            reference,
        })
    }))
}

/// What `for-each-ref` writes of each reference: its name, and the name of its upstream in full and
/// as the user knows it, empty when it has none.
const BRANCH_FORMAT: &str = "--format=%(refname)%00%(upstream)%00%(upstream:short)";

/// The local branches of `repository`, by name, each with how far it is from its upstream.
pub(crate) fn local_branches(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Vec<LocalBranch>, Failure> {
    let listed = read(
        runner,
        repository,
        &[
            "for-each-ref",
            BRANCH_FORMAT,
            "refs/heads/",
            "refs/remotes/",
        ],
    )?;
    let lines: Vec<[&str; 3]> = listed
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\0').collect();
            <[&str; 3]>::try_from(fields).ok()
        })
        .collect();
    let existing: BTreeSet<&str> = lines.iter().map(|[name, ..]| *name).collect();
    let mut branches = Vec::new();
    for [reference, upstream, upstream_name] in &lines {
        let Some(name) = reference.strip_prefix("refs/heads/") else {
            continue;
        };
        let upstream = if upstream.is_empty() {
            None
        } else {
            let commits = if existing.contains(upstream) {
                Some(count(runner, repository, reference, upstream)?)
            } else {
                None
            };
            Some(Upstream {
                name: (*upstream_name).to_owned(),
                commits,
            })
        };
        branches.push(LocalBranch {
            name: name.to_owned(),
            upstream,
        });
    }
    Ok(branches)
}

/// What HEAD of `repository` is on.
pub(crate) fn current_branch(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<CurrentBranch, Failure> {
    let head = runner
        .run(&repository.root, &["symbolic-ref", "--quiet", "HEAD"])
        .map_err(git_failed_to_start)?;
    let head_ref = head.stdout_text();
    match head_ref.trim_end().strip_prefix("refs/heads/") {
        Some(name) if head.success => {
            let upstream = local_branches(runner, repository)?
                .into_iter()
                .find(|branch| branch.name == name)
                .and_then(|branch| branch.upstream);
            Ok(CurrentBranch::Branch {
                name: name.to_owned(),
                upstream,
            })
        }
        _ => {
            let hash = read(runner, repository, &["rev-parse", "--verify", "HEAD"])?;
            Ok(CurrentBranch::Detached {
                hash: hash.trim_end().to_owned(),
            })
        }
    }
}

/// How far the branch `reference` is from `upstream`, both full reference names.
fn count(
    runner: &GitRunner,
    repository: &Repository,
    reference: &str,
    upstream: &str,
) -> Result<IncomingOutgoing, Failure> {
    let range = format!("{reference}...{upstream}");
    let counted = read(
        runner,
        repository,
        &["rev-list", "--left-right", "--count", &range, "--"],
    )?;
    let mut numbers = counted.split_whitespace().map(str::parse::<usize>);
    match (numbers.next(), numbers.next()) {
        (Some(Ok(outgoing)), Some(Ok(incoming))) => Ok(IncomingOutgoing { incoming, outgoing }),
        _ => Err(unreadable(repository)),
    }
}

/// The standard output of git run with `args` in `repository`, which has to succeed.
fn read(runner: &GitRunner, repository: &Repository, args: &[&str]) -> Result<String, Failure> {
    let root = &repository.root;
    let invocation = runner.run(root, args).map_err(git_failed_to_start)?;
    if !invocation.success {
        return Err(failure_of(
            runner,
            root,
            &invocation,
            unreadable(repository),
        ));
    }
    Ok(invocation.stdout_text())
}

fn unreadable(repository: &Repository) -> Failure {
    Failure::BranchesUnreadable {
        folder: repository.root.clone(),
    }
}
