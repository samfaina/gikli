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

/// The arguments of the git that fetches every remote of those `remotes` lists, pruning the remote
/// branches that are gone from them. A single remote is named: `git fetch --all` before git 2.37
/// fetches each remote in a git of its own that reports no progress, even when there is only one.
/// Several remotes go through `--all`.
pub(crate) fn fetch_all_args(remotes: &[String]) -> Vec<&str> {
    match remotes {
        [remote] => vec!["fetch", "--progress", "--prune", "--end-of-options", remote],
        _ => vec!["fetch", "--progress", "--all", "--prune"],
    }
}

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
            let remote_branch = format!("refs/remotes/{}", branch.name);
            fetched_into(runner, repository, &remote_branch)?.ok_or_else(|| {
                Failure::NoRemoteToFetchFrom {
                    folder: repository.root.clone(),
                }
            })
        }
        RefKind::Head | RefKind::Tag => Err(no_upstream()),
    }
}

/// The value of the configuration `key` in `repository`, if it is set.
pub(crate) fn configured(
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
/// remote branch `remote_branch`, a full reference name, if one does. The first refspec that maps
/// into it counts, as two remotes fetching into the same remote branch would overwrite each other.
fn fetched_into(
    runner: &GitRunner,
    repository: &Repository,
    remote_branch: &str,
) -> Result<Option<BranchSource>, Failure> {
    Ok(fetch_refspecs(runner, repository)?
        .iter()
        .find_map(|refspec| {
            Some(BranchSource {
                remote: refspec.remote.clone(),
                reference: refspec.source_of(remote_branch)?,
            })
        }))
}

/// The remote branch the configuration of `remote` in `repository` fetches its branch `reference`,
/// a full reference name such as `refs/heads/main`, into, if it fetches it anywhere. The first
/// refspec of the remote that maps it counts, as git's own does.
pub(crate) fn fetched_from(
    runner: &GitRunner,
    repository: &Repository,
    remote: &str,
    reference: &str,
) -> Result<Option<String>, Failure> {
    Ok(fetch_refspecs(runner, repository)?
        .iter()
        .filter(|refspec| refspec.remote == remote)
        .find_map(|refspec| refspec.destination_of(reference)))
}

/// Where the fetch configuration of `remote` in `repository` writes the branches it fetches: each
/// refspec's destination, such as `refs/remotes/origin/*`, in the order git lists them.
pub(crate) fn fetch_destinations(
    runner: &GitRunner,
    repository: &Repository,
    remote: &str,
) -> Result<Vec<String>, Failure> {
    Ok(fetch_refspecs(runner, repository)?
        .into_iter()
        .filter(|refspec| refspec.remote == remote)
        .map(|refspec| refspec.destination)
        .collect())
}

/// The names of the branches of `remote`, such as `main`, as `repository` holds them since it last
/// fetched them, in the remote branches its fetch configuration writes them into, by name.
pub(crate) fn remote_branches(
    runner: &GitRunner,
    repository: &Repository,
    remote: &str,
) -> Result<Vec<String>, Failure> {
    let refspecs: Vec<FetchRefspec> = fetch_refspecs(runner, repository)?
        .into_iter()
        .filter(|refspec| refspec.remote == remote)
        .collect();
    if refspecs.is_empty() {
        return Ok(Vec::new());
    }
    // A pattern of `for-each-ref` matches the names under it as a folder, and the refspecs keep
    // only the remote branches they write.
    let mut args = vec!["for-each-ref", "--format=%(refname)"];
    let patterns: Vec<&str> = refspecs
        .iter()
        .map(|refspec| match refspec.destination.split_once('*') {
            Some((start, _)) => start,
            None => refspec.destination.as_str(),
        })
        .collect();
    args.extend(&patterns);
    let listed = read_branches(runner, repository, &args)?;
    let names: BTreeSet<String> = listed
        .lines()
        .filter_map(|remote_branch| {
            refspecs
                .iter()
                .find_map(|refspec| refspec.source_of(remote_branch))
        })
        .filter_map(|source| source.strip_prefix("refs/heads/").map(str::to_owned))
        // What the remote's HEAD points to is no branch of its own.
        .filter(|name| name != "HEAD")
        .collect();
    Ok(names.into_iter().collect())
}

/// One refspec of the fetch configuration of a remote, such as
/// `+refs/heads/*:refs/remotes/origin/*`: what it fetches, and where it writes it.
struct FetchRefspec {
    remote: String,
    source: String,
    destination: String,
}

impl FetchRefspec {
    /// The reference of the remote this refspec fetches into `destination`, if it does.
    fn source_of(&self, destination: &str) -> Option<String> {
        Self::map(&self.destination, &self.source, destination)
    }

    /// Where this refspec writes the reference `source` of the remote, if it fetches it.
    fn destination_of(&self, source: &str) -> Option<String> {
        Self::map(&self.source, &self.destination, source)
    }

    /// `reference` matched against the side `from` of a refspec and written as its side `to`.
    fn map(from: &str, to: &str, reference: &str) -> Option<String> {
        match (from.split_once('*'), to.split_once('*')) {
            (Some((from_start, from_end)), Some((to_start, to_end))) => {
                let matched = reference.strip_prefix(from_start)?.strip_suffix(from_end)?;
                Some(format!("{to_start}{matched}{to_end}"))
            }
            (None, None) if from == reference => Some(to.to_owned()),
            _ => None,
        }
    }
}

/// The fetch configuration of every remote of `repository`, in the order git lists it.
fn fetch_refspecs(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Vec<FetchRefspec>, Failure> {
    let listed = runner
        .run(
            &repository.root,
            &["config", "--null", "--get-regexp", r"^remote\..*\.fetch$"],
        )
        .map_err(git_failed_to_start)?;
    // Each entry is the key, a line break, then the refspec.
    Ok(listed
        .stdout_text()
        .split('\0')
        .filter_map(|entry| {
            let (key, refspec) = entry.split_once('\n')?;
            let remote = key.strip_prefix("remote.")?.strip_suffix(".fetch")?;
            let (source, destination) = refspec.trim_start_matches('+').split_once(':')?;
            Some(FetchRefspec {
                remote: remote.to_owned(),
                source: source.to_owned(),
                destination: destination.to_owned(),
            })
        })
        .collect())
}

/// The remote a branch goes to or comes from when nothing else says which: `origin`, or the first
/// of `remotes` when there is no `origin`. None when there is no remote.
pub(crate) fn default_remote(remotes: &[String]) -> Option<&String> {
    remotes
        .iter()
        .find(|remote| *remote == "origin")
        .or(remotes.first())
}

/// The name of the local branch HEAD of `repository` is on, which may have no commit yet. None
/// when HEAD is detached.
pub(crate) fn head_branch(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Option<String>, Failure> {
    let head = runner
        .run(&repository.root, &["symbolic-ref", "--quiet", "HEAD"])
        .map_err(git_failed_to_start)?;
    Ok(head
        .stdout_text()
        .trim_end()
        .strip_prefix("refs/heads/")
        .filter(|_| head.success)
        .map(str::to_owned))
}

/// The names of the remotes of `repository`, as git lists them.
pub(crate) fn remotes(runner: &GitRunner, repository: &Repository) -> Result<Vec<String>, Failure> {
    let listed = read_branches(runner, repository, &["remote"])?;
    Ok(listed.lines().map(str::to_owned).collect())
}

/// What `for-each-ref` writes of each local branch: its full name, and that of its upstream, empty
/// when it has none.
const BRANCH_FORMAT: &str = "--format=%(refname)%00%(upstream)";

/// The local branches of `repository`, by name, each with how far it is from its upstream.
pub(crate) fn local_branches(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Vec<LocalBranch>, Failure> {
    let listed = read_branches(
        runner,
        repository,
        &["for-each-ref", BRANCH_FORMAT, "refs/heads/"],
    )?;
    with_upstreams(runner, repository, branch_refs(&listed))
}

/// What HEAD of `repository` is on. Only the current branch is counted.
pub(crate) fn current_branch(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<CurrentBranch, Failure> {
    let head = runner
        .run(&repository.root, &["symbolic-ref", "--quiet", "HEAD"])
        .map_err(git_failed_to_start)?;
    let head_ref = head.stdout_text();
    let head_ref = head_ref.trim_end();
    if head.success
        && let Some(name) = head_ref.strip_prefix("refs/heads/")
    {
        // A branch with no commit yet is not listed, and has nothing to count.
        let listed = read_branches(
            runner,
            repository,
            &["for-each-ref", BRANCH_FORMAT, head_ref],
        )?;
        let mut refs = branch_refs(&listed);
        refs.retain(|branch| branch.reference == head_ref);
        let upstream = with_upstreams(runner, repository, refs)?
            .into_iter()
            .next()
            .and_then(|branch| branch.upstream);
        return Ok(CurrentBranch::Branch {
            name: name.to_owned(),
            upstream,
        });
    }
    let hash = read_branches(runner, repository, &["rev-parse", "--verify", "HEAD"])?;
    Ok(CurrentBranch::Detached {
        hash: hash.trim_end().to_owned(),
    })
}

/// A local branch as `for-each-ref` lists it in [`BRANCH_FORMAT`].
struct BranchRef<'a> {
    reference: &'a str,
    /// Empty when the branch has none.
    upstream: &'a str,
}

/// The local branches `listed` in [`BRANCH_FORMAT`].
fn branch_refs(listed: &str) -> Vec<BranchRef<'_>> {
    listed
        .lines()
        .filter_map(|line| {
            let (reference, upstream) = line.split_once('\0')?;
            Some(BranchRef {
                reference,
                upstream,
            })
        })
        .collect()
}

/// The local branches `refs` of `repository`, each with how far it is from its upstream. An
/// upstream that does not exist, wherever the configuration puts it, is gone.
fn with_upstreams(
    runner: &GitRunner,
    repository: &Repository,
    refs: Vec<BranchRef>,
) -> Result<Vec<LocalBranch>, Failure> {
    let mut upstreams: Vec<&str> = refs
        .iter()
        .map(|branch| branch.upstream)
        .filter(|upstream| !upstream.is_empty())
        .collect();
    upstreams.sort_unstable();
    upstreams.dedup();
    let listed = if upstreams.is_empty() {
        String::new()
    } else {
        let mut args = vec!["for-each-ref", "--format=%(refname)"];
        args.extend(&upstreams);
        read_branches(runner, repository, &args)?
    };
    // A pattern of `for-each-ref` also matches the names under it as a folder, which the exact
    // comparison leaves out.
    let existing: BTreeSet<&str> = listed.lines().collect();
    let mut branches = Vec::new();
    for branch in refs {
        let Some(name) = branch.reference.strip_prefix("refs/heads/") else {
            continue;
        };
        let upstream = match branch.upstream {
            "" => None,
            upstream => Some(Upstream {
                name: short_name(upstream).to_owned(),
                commits: match existing.contains(upstream) {
                    true => Some(count(runner, repository, branch.reference, upstream)?),
                    false => None,
                },
            }),
        };
        branches.push(LocalBranch {
            name: name.to_owned(),
            upstream,
        });
    }
    Ok(branches)
}

/// `reference` as the user knows it: a remote branch or a local branch without its folder, as
/// `origin/main` or `main`, and any other reference in full. git's own short form depends on the
/// user's configuration.
fn short_name(reference: &str) -> &str {
    reference
        .strip_prefix("refs/remotes/")
        .or_else(|| reference.strip_prefix("refs/heads/"))
        .unwrap_or(reference)
}

/// How far the branch `reference` is from `upstream`, both full reference names.
fn count(
    runner: &GitRunner,
    repository: &Repository,
    reference: &str,
    upstream: &str,
) -> Result<IncomingOutgoing, Failure> {
    let range = format!("{reference}...{upstream}");
    let counted = read_branches(
        runner,
        repository,
        &["rev-list", "--left-right", "--count", &range, "--"],
    )?;
    let mut numbers = counted.split_whitespace().map(str::parse::<usize>);
    match (numbers.next(), numbers.next()) {
        (Some(Ok(outgoing)), Some(Ok(incoming))) => Ok(IncomingOutgoing { incoming, outgoing }),
        _ => Err(branches_unreadable(repository)),
    }
}

/// The standard output of git run with `args` in `repository` to read its branches, which has to
/// succeed. A failure the failure explainer does not know says the branches could not be read.
fn read_branches(
    runner: &GitRunner,
    repository: &Repository,
    args: &[&str],
) -> Result<String, Failure> {
    let root = &repository.root;
    let invocation = runner.run(root, args).map_err(git_failed_to_start)?;
    if !invocation.success {
        let fallback = branches_unreadable(repository);
        return Err(failure_of(runner, root, &invocation, fallback));
    }
    Ok(invocation.stdout_text())
}

pub(crate) fn branches_unreadable(repository: &Repository) -> Failure {
    Failure::BranchesUnreadable {
        folder: repository.root.clone(),
    }
}
