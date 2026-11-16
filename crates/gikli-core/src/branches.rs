//! The branch engine's lists: every branch and tag of a Repository, as the branch selector and the
//! branches pane of the log show them.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::git::GitRunner;
use crate::sync::{self, CurrentBranch, Upstream};
use crate::{
    Failure, RefKind, RefLabel, Repository, Settings, failure_of, git_failed_to_start, log,
};

/// The branches of a Repository, in the groups the branch selector shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branches {
    pub repository: Repository,
    /// What HEAD is on.
    pub current: CurrentBranch,
    /// The local branches checked out last, newest first, other than the current one. Empty when
    /// the user hides Recent.
    pub recent: Vec<Branch>,
    /// The local branches, by name.
    pub local: Vec<BranchNode>,
    /// The remote branches, by name.
    pub remote: Vec<BranchNode>,
    /// The tags, by name. Empty when the user hides them.
    pub tags: Vec<BranchNode>,
}

/// A branch or a tag in the lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    /// Its whole name, such as `feature/login` or `origin/main`, and its kind.
    pub label: RefLabel,
    /// The upstream of a local branch, with its incoming and outgoing commits. None for a local
    /// branch without one, and for remote branches and tags.
    pub upstream: Option<Upstream>,
}

/// One entry of a group of the lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BranchNode {
    /// A directory of branches or tags, when they are grouped by directory. Its subdirectories
    /// come first, then its branches, each in order of name.
    Directory {
        /// Its whole path, such as `feature` or `origin/feature`, with `/` between directories.
        path: String,
        /// The subdirectories and branches in the directory.
        children: Vec<BranchNode>,
    },
    /// A branch or a tag.
    Branch(Branch),
}

/// What a directory of branches holds while the lists are put together.
#[derive(Default)]
struct DirectoryContents {
    directories: BTreeMap<String, DirectoryContents>,
    branches: Vec<Branch>,
}

impl DirectoryContents {
    /// Puts `branch` in the directory its name `rest` leads to from here.
    fn insert(&mut self, rest: &str, branch: Branch) {
        match rest.split_once('/') {
            Some((directory, rest)) => self
                .directories
                .entry(directory.to_owned())
                .or_default()
                .insert(rest, branch),
            None => self.branches.push(branch),
        }
    }

    /// The entries of the directory at `path`, empty for the top of a group.
    fn into_nodes(self, path: &str) -> Vec<BranchNode> {
        let mut nodes: Vec<BranchNode> = self
            .directories
            .into_iter()
            .map(|(name, contents)| {
                let path = match path {
                    "" => name,
                    _ => format!("{path}/{name}"),
                };
                BranchNode::Directory {
                    children: contents.into_nodes(&path),
                    path,
                }
            })
            .collect();
        nodes.extend(self.branches.into_iter().map(BranchNode::Branch));
        nodes
    }
}

/// `branches`, in the order given, as the entries of a group: under their directories when
/// `by_directory`.
fn group(branches: Vec<Branch>, by_directory: bool) -> Vec<BranchNode> {
    if !by_directory {
        return branches.into_iter().map(BranchNode::Branch).collect();
    }
    let mut top = DirectoryContents::default();
    for branch in branches {
        let name = branch.label.name.clone();
        top.insert(&name, branch);
    }
    top.into_nodes("")
}

/// The branches of `repository`, grouped as `settings` say, that `search` finds.
pub(crate) fn list(
    runner: &GitRunner,
    repository: &Repository,
    settings: &Settings,
    search: &str,
) -> Result<Branches, Failure> {
    let search = search.trim().to_lowercase();
    let found = |label: &RefLabel| label.name.to_lowercase().contains(&search);
    let current = sync::current_branch(runner, repository)?;
    let references = log::references(runner, repository)?;
    let upstreams: HashMap<String, Option<Upstream>> = sync::local_branches(runner, repository)?
        .into_iter()
        .map(|branch| (branch.name, branch.upstream))
        .collect();
    let of_kind = |kind: RefKind| -> Vec<BranchNode> {
        let branches = references
            .iter()
            .filter(|label| label.kind == kind && found(label))
            .map(|label| Branch {
                label: label.clone(),
                upstream: match label.kind {
                    RefKind::Local => upstreams.get(&label.name).cloned().flatten(),
                    _ => None,
                },
            })
            .collect();
        group(branches, settings.group_branches_by_directory)
    };
    let mut recent = match settings.show_recent_branches {
        true => recent(runner, repository, &current, &upstreams)?,
        false => Vec::new(),
    };
    recent.retain(|branch| found(&branch.label));
    Ok(Branches {
        repository: repository.clone(),
        current,
        recent,
        local: of_kind(RefKind::Local),
        remote: of_kind(RefKind::Remote),
        tags: match settings.show_tags_in_branch_selector {
            true => of_kind(RefKind::Tag),
            false => Vec::new(),
        },
    })
}

/// How many branches Recent holds at most.
const RECENT_BRANCHES: usize = 5;

/// How an entry of the reflog of HEAD for a checkout starts, as `git checkout` and `git switch`
/// write it. The branch or commit HEAD left follows, then ` to ` and the one it went to.
const CHECKOUT: &str = "checkout: moving from ";

/// The local branches of `repository` HEAD was last checked out on, newest first, other than the
/// `current` one, up to [`RECENT_BRANCHES`] of them, from the reflog of HEAD. `upstreams` holds
/// the upstream of each local branch there is now, so a branch since deleted, or a commit HEAD
/// was detached on, is left out.
fn recent(
    runner: &GitRunner,
    repository: &Repository,
    current: &CurrentBranch,
    upstreams: &HashMap<String, Option<Upstream>>,
) -> Result<Vec<Branch>, Failure> {
    let grep = format!("--grep-reflog={CHECKOUT}");
    let args = [
        "log",
        "--walk-reflogs",
        "--fixed-strings",
        &grep,
        "--format=%gs",
        "HEAD",
        "--",
    ];
    let read = runner
        .run(&repository.root, &args)
        .map_err(git_failed_to_start)?;
    let current_name = match current {
        CurrentBranch::Branch { name, .. } => Some(name.as_str()),
        CurrentBranch::Detached { .. } => None,
    };
    if !read.success {
        // git cannot walk the reflog of a HEAD on a branch with no commit yet.
        let unborn = current_name.is_some_and(|name| !upstreams.contains_key(name));
        if unborn {
            return Ok(Vec::new());
        }
        let fallback = sync::branches_unreadable(repository);
        return Err(failure_of(runner, &repository.root, &read, fallback));
    }
    let mut recent: Vec<Branch> = Vec::new();
    for line in read.stdout_text().lines() {
        let Some((_, name)) = line
            .strip_prefix(CHECKOUT)
            .and_then(|moved| moved.rsplit_once(" to "))
        else {
            continue;
        };
        let Some(upstream) = upstreams.get(name) else {
            continue;
        };
        if Some(name) == current_name || recent.iter().any(|branch| branch.label.name == name) {
            continue;
        }
        recent.push(Branch {
            label: RefLabel {
                name: name.to_owned(),
                kind: RefKind::Local,
            },
            upstream: upstream.clone(),
        });
        if recent.len() == RECENT_BRANCHES {
            break;
        }
    }
    Ok(recent)
}
