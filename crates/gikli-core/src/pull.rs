//! Pull: what the pull dialog offers to integrate into the current branch, and from where.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::git::GitRunner;
use crate::project::write_at_once;
use crate::push;
use crate::sync::{default_remote, head_branch, remote_branches, remotes};
use crate::{Failure, Repository, git_failed_to_start};

/// A remote of the Repository, with the branches it had when it was last fetched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRemote {
    pub name: String,
    /// The names of its branches, such as `main`, by name.
    pub branches: Vec<String>,
}

/// What the pull dialog shows: the Repository, its current branch, which the pull integrates
/// into, and where the pull takes the work from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullPlan {
    pub repository: Repository,
    /// The current branch, which the pull integrates into.
    pub branch: String,
    /// The remotes of the Repository, as git lists them.
    pub remotes: Vec<PullRemote>,
    /// The remote the dialog starts from. None when the Repository has no remote.
    pub remote: Option<String>,
    /// The branch of `remote` the dialog starts from. None when there is none to start from.
    pub source_branch: Option<String>,
}

/// Where a pull takes the work it integrates from: the branch `branch` of the remote `remote`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullSource {
    pub remote: String,
    /// The name of the branch on the remote, such as `main`.
    pub branch: String,
}

/// How a pull integrates the work it brings, as the options of the pull dialog say. None is
/// ticked to start with, which merges.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PullOptions {
    /// Rebases the current branch onto what the pull brings, as `--rebase`, instead of merging.
    pub rebase: bool,
    /// Integrates only when the current branch can fast-forward, as `--ff-only`.
    pub ff_only: bool,
    /// Makes a merge commit even when the current branch could fast-forward, as `--no-ff`.
    pub no_ff: bool,
    /// Puts what the pull brings in the working tree and the index without a commit, as
    /// `--squash`.
    pub squash: bool,
    /// Merges without committing the merge, as `--no-commit`.
    pub no_commit: bool,
    /// Runs the merge without the `pre-merge-commit` and `commit-msg` hooks, as `--no-verify`.
    pub no_verify: bool,
}

/// The pairs of options of the pull dialog that contradict each other, as git's options: a rebase
/// makes no merge to shape, and a squash makes no merge commit, nor does a fast-forward.
const CONTRADICTIONS: [(&str, &str); 6] = [
    ("--rebase", "--ff-only"),
    ("--rebase", "--no-ff"),
    ("--rebase", "--squash"),
    ("--rebase", "--no-commit"),
    ("--ff-only", "--no-ff"),
    ("--no-ff", "--squash"),
];

impl PullOptions {
    /// The options ticked, as git's options, in the order the dialog shows them.
    fn ticked(self) -> Vec<&'static str> {
        [
            (self.rebase, "--rebase"),
            (self.ff_only, "--ff-only"),
            (self.no_ff, "--no-ff"),
            (self.squash, "--squash"),
            (self.no_commit, "--no-commit"),
            (self.no_verify, "--no-verify"),
        ]
        .into_iter()
        .filter(|(ticked, _)| *ticked)
        .map(|(_, option)| option)
        .collect()
    }

    /// Fails with [`Failure::PullOptionsIncompatible`] when two of the options ticked contradict
    /// each other, naming the first such pair.
    pub(crate) fn check(self) -> Result<(), Failure> {
        let ticked = self.ticked();
        match CONTRADICTIONS
            .iter()
            .find(|(first, second)| ticked.contains(first) && ticked.contains(second))
        {
            Some((first, second)) => Err(Failure::PullOptionsIncompatible {
                first: (*first).to_owned(),
                second: (*second).to_owned(),
            }),
            None => Ok(()),
        }
    }
}

/// The arguments of the git that pulls `source` into the current branch with `options`. The
/// method goes on the command line whatever `pull.rebase` says, as the user chose it.
pub(crate) fn pull_args(source: &PullSource, options: PullOptions) -> Vec<String> {
    let mut args = vec!["pull".to_owned(), "--progress".to_owned()];
    if !options.rebase {
        args.push("--no-rebase".to_owned());
    }
    args.extend(options.ticked().into_iter().map(str::to_owned));
    args.extend([
        "--end-of-options".to_owned(),
        source.remote.clone(),
        format!("refs/heads/{}", source.branch),
    ]);
    args
}

/// What the pull dialog shows for `repository`. It starts from the source the user last pulled
/// the current branch from, when that was not the one it starts from by default and its remote is
/// still there, and otherwise from the default: the upstream of the branch when that is a branch
/// of a remote, and otherwise `origin`, or the first remote when there is no `origin`, with the
/// branch of the same name when that remote has one.
pub(crate) fn plan(runner: &GitRunner, repository: &Repository) -> Result<PullPlan, Failure> {
    let branch = current_branch(runner, repository)?;
    let pull_remotes = pull_remotes(runner, repository)?;
    let remembered = RememberedSources::of(runner, repository)?
        .source_of(&branch)
        .filter(|source| {
            pull_remotes
                .iter()
                .any(|remote| remote.name == source.remote)
        });
    let (remote, source_branch) = match remembered {
        Some(source) => (Some(source.remote), Some(source.branch)),
        None => default_source(runner, repository, &branch, &pull_remotes)?,
    };
    Ok(PullPlan {
        repository: repository.clone(),
        branch,
        remotes: pull_remotes,
        remote,
        source_branch,
    })
}

/// Keeps `source` as the one the pull dialog starts from next time for `branch` of `repository`,
/// when it is not the one it starts from by default, and forgets any other otherwise. Remembering
/// is a help, so when it cannot be written the pull still goes on, and only the next pull dialog
/// does not see it.
pub(crate) fn remember(
    runner: &GitRunner,
    repository: &Repository,
    branch: &str,
    source: &PullSource,
) -> Result<(), Failure> {
    let pull_remotes = pull_remotes(runner, repository)?;
    let default = default_source(runner, repository, branch, &pull_remotes)?;
    let is_default = default == (Some(source.remote.clone()), Some(source.branch.clone()));
    let mut remembered = RememberedSources::of(runner, repository)?;
    let changed = if is_default {
        remembered.sources.remove(branch).is_some()
    } else {
        remembered.sources.insert(branch.to_owned(), source.clone()) != Some(source.clone())
    };
    if changed {
        drop(remembered.save());
    }
    Ok(())
}

/// The remotes of `repository`, as git lists them, each with the branches it had when it was last
/// fetched.
fn pull_remotes(runner: &GitRunner, repository: &Repository) -> Result<Vec<PullRemote>, Failure> {
    remotes(runner, repository)?
        .into_iter()
        .map(|name| {
            let branches = remote_branches(runner, repository, &name)?;
            Ok(PullRemote { name, branches })
        })
        .collect()
}

/// The remote and branch the pull dialog starts from for `branch` of `repository`, with the
/// remotes `pull_remotes`, when the user has not pulled it from elsewhere.
fn default_source(
    runner: &GitRunner,
    repository: &Repository,
    branch: &str,
    pull_remotes: &[PullRemote],
) -> Result<(Option<String>, Option<String>), Failure> {
    if let Some(upstream) = push::upstream(runner, repository, branch)? {
        return Ok((Some(upstream.remote), Some(upstream.branch)));
    }
    let names: Vec<String> = pull_remotes
        .iter()
        .map(|remote| remote.name.clone())
        .collect();
    let remote = default_remote(&names)
        .and_then(|name| pull_remotes.iter().find(|remote| &remote.name == name));
    Ok(match remote {
        Some(remote) => (
            Some(remote.name.clone()),
            remote.branches.iter().find(|name| *name == branch).cloned(),
        ),
        None => (None, None),
    })
}

/// The sources the user pulled local branches of a Repository from, when they were not the ones
/// the pull dialog starts from by default, by branch. They are kept in the git directory the
/// worktrees of the Repository share, as the branches are.
struct RememberedSources {
    file: PathBuf,
    sources: BTreeMap<String, PullSource>,
}

impl RememberedSources {
    /// Those of `repository`. A missing or unreadable file gives none.
    fn of(runner: &GitRunner, repository: &Repository) -> Result<Self, Failure> {
        let common_git_dir = runner
            .common_git_dir(&repository.root)
            .map_err(git_failed_to_start)?
            .ok_or_else(|| Failure::BranchesUnreadable {
                folder: repository.root.clone(),
            })?;
        let file = common_git_dir.join("gikli").join("pull-sources.json");
        let sources = fs::read(&file)
            .ok()
            .and_then(|json| serde_json::from_slice(&json).ok())
            .unwrap_or_default();
        Ok(Self { file, sources })
    }

    /// The source `branch` was last pulled from, if it is remembered.
    fn source_of(&self, branch: &str) -> Option<PullSource> {
        self.sources.get(branch).cloned()
    }

    fn save(&self) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(&self.sources).map_err(std::io::Error::other)?;
        write_at_once(&self.file, &json)
    }
}

/// The name of the branch HEAD of `repository` is on, the one a pull integrates into, which may
/// have no commit yet. A detached HEAD returns [`Failure::DetachedHeadNotPulled`].
pub(crate) fn current_branch(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<String, Failure> {
    head_branch(runner, repository)?.ok_or(Failure::DetachedHeadNotPulled)
}
