//! The branch engine's checkout of a local branch, a tag, a commit or a new branch, and what blocks
//! one: the files that keep git from checking out a commit, told from what the Repository holds
//! rather than from git's translated sentences, as the failure explainer does.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::{GitRunner, path_from_bytes};
use crate::{
    Failure, NewBranchBase, RefKind, RefLabel, Repository, details, git_failed_to_start, log,
    new_branch,
};

/// What a checkout takes HEAD to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CheckoutTarget {
    /// A local branch, which HEAD then is on.
    Branch { name: String },
    /// A tag, which leaves a detached HEAD on the commit it points at.
    Tag { name: String },
    /// A commit, which leaves a detached HEAD on it.
    Revision { revision: String },
    /// A new branch called `name`, created at `base` and checked out.
    NewBranch { name: String, base: NewBranchBase },
}

/// What happens to the local changes that block a checkout, as the user chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OverLocalChanges {
    /// Discards the changes to `files`, the files that block the checkout as the user confirmed
    /// them, by path relative to the Repository root. The other local changes stay, and the
    /// checkout takes them along.
    Discard { files: Vec<PathBuf> },
    /// Saves the local changes as Saved local changes, checks out, and restores them.
    SaveAndRestore,
}

/// A checkout, ready to run.
pub(crate) struct Planned {
    /// What the user knows the target by, which the operation names.
    pub label: String,
    /// The commit the checkout takes HEAD to, as git names it.
    pub revision: String,
    /// The arguments after `git` of the checkout.
    pub args: Vec<String>,
    /// The failure to tell when git fails and nothing explains why.
    pub fallback: Failure,
}

/// The checkout of `target` in `repository`, once the target is known to be there. A branch, a
/// tag or a revision that is not returns [`Failure::RevisionNotFound`] with the name the user
/// knows, and a new branch's name that git takes in no branch, [`Failure::BranchNameInvalid`].
pub(crate) fn plan(
    runner: &GitRunner,
    repository: &Repository,
    target: &CheckoutTarget,
) -> Result<Planned, Failure> {
    let failed = Failure::CheckoutFailed {
        folder: repository.root.clone(),
    };
    let to_strings = |args: &[&str]| args.iter().map(|arg| (*arg).to_owned()).collect();
    let detached = |known_as: &str, revision: &str| -> Result<Planned, Failure> {
        let hash = details::resolve_known_as(runner, repository, revision, known_as)?;
        Ok(Planned {
            label: known_as.to_owned(),
            args: to_strings(&["switch", "--detach", &hash]),
            revision: hash,
            fallback: failed.clone(),
        })
    };
    match target {
        CheckoutTarget::Branch { name } => {
            if !new_branch::has_local_branch(runner, repository, name)? {
                return Err(Failure::RevisionNotFound {
                    revision: name.clone(),
                });
            }
            // `--no-guess`, so a branch deleted since does not become a new branch that follows
            // a remote branch of that name.
            Ok(Planned {
                label: name.clone(),
                revision: format!("refs/heads/{name}"),
                args: to_strings(&["switch", "--no-guess", name]),
                fallback: failed,
            })
        }
        CheckoutTarget::Tag { name } => {
            // By its whole reference, so a branch with the same name does not get in the way.
            let tag = RefLabel {
                name: name.clone(),
                kind: RefKind::Tag,
            };
            detached(name, &tag.full_name())
        }
        CheckoutTarget::Revision { revision } => detached(revision, revision),
        CheckoutTarget::NewBranch { name, base } => {
            new_branch::check_name(runner, repository, name)?;
            let start = new_branch::start_point(runner, repository, base)?;
            Ok(Planned {
                label: name.clone(),
                args: to_strings(&["switch", "--create", name, &start]),
                revision: start,
                fallback: new_branch::not_created(repository, name),
            })
        }
    }
}

/// The files whose local changes to discard before checking out `revision` in `repository`, of
/// `confirmed`, the files the user confirmed: all of them when they are still the ones whose local
/// changes block the checkout, and none when no local change does any longer. Otherwise this
/// returns [`Failure::CheckoutBlocked`] with the files that block it now, as the user confirmed
/// another list, or files git does not track block it, which discarding leaves in the way.
pub(crate) fn to_discard(
    runner: &GitRunner,
    repository: &Repository,
    revision: &str,
    confirmed: &[PathBuf],
) -> Result<Vec<PathBuf>, Failure> {
    let blocking = Blocking::of(runner, repository, revision)?;
    let confirmed: BTreeSet<&PathBuf> = confirmed.iter().collect();
    if !blocking.untracked_files.is_empty() {
        Err(blocking.into_failure(repository))
    } else if blocking.local_changes.is_empty() {
        Ok(Vec::new())
    } else if blocking.local_changes.iter().collect::<BTreeSet<_>>() == confirmed {
        Ok(blocking.local_changes)
    } else {
        Err(blocking.into_failure(repository))
    }
}

/// The failure of a checkout of `revision` in `repository` that git refused, when files block it.
/// None when none does, and git failed for another reason.
pub(crate) fn blocked(
    runner: &GitRunner,
    repository: &Repository,
    revision: &str,
) -> Result<Option<Failure>, Failure> {
    let blocking = Blocking::of(runner, repository, revision)?;
    let blocks = !blocking.local_changes.is_empty() || !blocking.untracked_files.is_empty();
    Ok(blocks.then(|| blocking.into_failure(repository)))
}

/// The files that block a checkout, each by path.
struct Blocking {
    local_changes: Vec<PathBuf>,
    untracked_files: Vec<PathBuf>,
}

impl Blocking {
    /// The files that block a checkout of `revision` in `repository`. With no commit yet, HEAD
    /// holds nothing: every file of `revision` is new, and every file in the index is staged.
    fn of(runner: &GitRunner, repository: &Repository, revision: &str) -> Result<Self, Failure> {
        let (moved, staged) = if details::exists(runner, repository, "HEAD")? {
            (
                changed_paths(
                    runner,
                    repository,
                    &["diff-tree", "-r"],
                    &["HEAD", revision],
                )?,
                changed_paths(runner, repository, &["diff-index", "--cached"], &["HEAD"])?,
            )
        } else {
            (
                listed_paths(
                    runner,
                    repository,
                    &["ls-tree", "-r", "--name-only", revision],
                )?,
                listed_paths(runner, repository, &["ls-files", "--cached"])?,
            )
        };
        let local_changes =
            local_changes_in_the_way(runner, repository, revision, &moved, &staged)?;
        let untracked_files = untracked_in_the_way(runner, repository, &moved, &local_changes)?;
        Ok(Self {
            local_changes,
            untracked_files,
        })
    }

    /// The failure that tells of these files in `repository`.
    fn into_failure(self, repository: &Repository) -> Failure {
        Failure::CheckoutBlocked {
            folder: repository.root.clone(),
            local_changes: self.local_changes,
            untracked_files: self.untracked_files,
        }
    }
}

/// The files with uncommitted changes that a checkout of `revision` would overwrite, by path, of
/// the files `moved` between HEAD and `revision` and those `staged`. git takes a changed file
/// along when the commit checked out has it as HEAD has it, or holds what the index holds; any
/// other changed file it refuses to touch. A file deleted on disk only does not count, as nothing
/// on disk would be lost.
fn local_changes_in_the_way(
    runner: &GitRunner,
    repository: &Repository,
    revision: &str,
    moved: &BTreeSet<PathBuf>,
    staged: &BTreeSet<PathBuf>,
) -> Result<Vec<PathBuf>, Failure> {
    let unstaged = changed_paths(runner, repository, &["diff", "--diff-filter=d"], &[])?;
    let index_unlike_target =
        changed_paths(runner, repository, &["diff-index", "--cached"], &[revision])?;
    Ok(moved
        .iter()
        .filter(|path| index_unlike_target.contains(*path))
        .filter(|path| staged.contains(*path) || unstaged.contains(*path))
        .cloned()
        .collect())
}

/// The files git does not track that a checkout would overwrite or remove, by path, of the files
/// `moved` between HEAD and the commit checked out: one at a path that commit has, in a folder
/// where it has a file, or where it has a folder, and one HEAD has that the index no longer holds
/// and the commit does not have. Where git ignores case, so does the comparison, as a file that
/// differs only in case is the same file there. Ignored files do not count, as git writes over
/// them, and neither do the `local_changes` that block it, as which they are.
fn untracked_in_the_way(
    runner: &GitRunner,
    repository: &Repository,
    moved: &BTreeSet<PathBuf>,
    local_changes: &[PathBuf],
) -> Result<Vec<PathBuf>, Failure> {
    let ignore_case = ignores_case(runner, repository)?;
    let compared = |path: &Path| -> PathBuf {
        if ignore_case {
            PathBuf::from(path.to_string_lossy().to_lowercase())
        } else {
            path.to_path_buf()
        }
    };
    let moved: BTreeSet<PathBuf> = moved.iter().map(|path| compared(path)).collect();
    let untracked = listed_paths(
        runner,
        repository,
        &["ls-files", "--others", "--exclude-standard"],
    )?;
    Ok(untracked
        .into_iter()
        .filter(|path| !local_changes.contains(path))
        .filter(|path| {
            let path = compared(path);
            path.ancestors().any(|ancestor| moved.contains(ancestor))
                || moved.iter().any(|other| other.starts_with(&path))
        })
        .collect())
}

/// Whether git ignores the case of file names in `repository`, as `core.ignorecase` says, which
/// git sets where the file system ignores it.
fn ignores_case(runner: &GitRunner, repository: &Repository) -> Result<bool, Failure> {
    let config = runner
        .run(&repository.root, &["config", "--bool", "core.ignorecase"])
        .map_err(git_failed_to_start)?;
    Ok(config.success && config.stdout_text().trim() == "true")
}

/// The paths git names when it compares `revisions` as `command` says, ignoring renames, by path.
fn changed_paths(
    runner: &GitRunner,
    repository: &Repository,
    command: &[&str],
    revisions: &[&str],
) -> Result<BTreeSet<PathBuf>, Failure> {
    let mut args = command.to_vec();
    args.extend(["--name-only", "-z", "--no-renames"]);
    args.extend(revisions);
    listed_paths(runner, repository, &args)
}

/// The paths git lists when it runs as `command` says, by path.
fn listed_paths(
    runner: &GitRunner,
    repository: &Repository,
    command: &[&str],
) -> Result<BTreeSet<PathBuf>, Failure> {
    let mut args = command.to_vec();
    args.extend(["-z", "--"]);
    let output = log::run_git(runner, repository, &args)?.stdout;
    Ok(output
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(path_from_bytes)
        .collect())
}
