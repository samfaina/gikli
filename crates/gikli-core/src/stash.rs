//! The stash engine: the user puts work aside in a stash, sees what each stash holds, and drops
//! one or clears them all.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::{GitRunner, path_from_bytes};
use crate::rename_branch::run;
use crate::{
    Change, ChangeStatus, DiffContent, Failure, Repository, commit, details, diff, failure_of,
    git_failed_to_start, log,
};

/// What the user chose in the stash dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StashOptions {
    /// The message of the stash, as the user wrote it. Without text, git names the stash after the
    /// commit HEAD is on.
    pub message: String,
    /// Whether what is staged also stays in the index and the working tree.
    pub keep_index: bool,
    /// Whether the stash also takes the files git does not track, which then leave the working
    /// tree.
    pub include_untracked: bool,
}

/// The stashes of a Repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stashes {
    pub repository: Repository,
    /// Why the Repository cannot stash, when it cannot. Its stash actions are unavailable then.
    pub unavailable: Option<StashUnavailable>,
    /// The stashes, newest first.
    pub entries: Vec<Stash>,
}

/// Why a Repository cannot stash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StashUnavailable {
    /// The Repository has no commit yet, and git stashes changes against one.
    NoCommits,
}

/// One stash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stash {
    /// How git names it now, such as `stash@{0}`. Newer stashes push it down the list.
    pub name: String,
    /// The full hash of its commit, which stays the same whatever its place in the list.
    pub hash: String,
    /// What git says of it, such as `On main: Half done`.
    pub message: String,
    /// When it was made, in seconds since the epoch.
    pub created_at: i64,
}

/// What the diff of a file in a stash compares the stashed file with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StashDiffBase {
    /// The file on disk now: the older side is the stashed file, and the newer side the local one.
    #[default]
    Local,
    /// The file in the commit the stash was made on: the older side is that file, and the newer
    /// side the stashed one.
    Parent,
}

/// What the diff of a file in a stash shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashFileDiff {
    pub repository: Repository,
    /// The full hash of the stash's commit.
    pub stash: String,
    /// What the stashed file is compared with.
    pub against: StashDiffBase,
    /// The file's path in the stash, relative to the Repository root, with `/` between folders.
    pub path: PathBuf,
    /// The path the file had in the commit the stash was made on, for a file the stash moved.
    pub original_path: Option<PathBuf>,
    /// The two sides, as [`StashDiffBase`] tells them. Neither side can be edited.
    pub content: DiffContent,
}

/// What git prints of each stash, a field per NUL. Its name comes from its place in the list, the
/// order of the reflog of `refs/stash`, as `%gd` names it by date under some date options.
const LIST_FORMAT: &str = "--format=%H%x00%ct%x00%gs";

/// Puts the uncommitted changes of `repository` aside in a stash, as `options` say. When git
/// finds nothing to stash, it succeeds and makes none: that returns [`Failure::NothingToStash`].
pub(crate) fn push(
    runner: &GitRunner,
    repository: &Repository,
    options: &StashOptions,
) -> Result<(), Failure> {
    let before = newest(available(runner, repository)?);
    let mut args = vec!["stash", "push"];
    if options.keep_index {
        args.push("--keep-index");
    }
    if options.include_untracked {
        args.push("--include-untracked");
    }
    if !options.message.trim().is_empty() {
        args.extend(["--message", &options.message]);
    }
    run(
        runner,
        repository,
        &args,
        Failure::NotStashed {
            folder: repository.root.clone(),
        },
    )?;
    if newest(list(runner, repository)?) == before {
        return Err(Failure::NothingToStash {
            folder: repository.root.clone(),
        });
    }
    Ok(())
}

/// How many stashes there are, with the hash of the newest one: a stash made since changes the
/// count, even one with the hash of the stash before it, as two of the same changes made in the
/// same second have.
fn newest(stashes: Stashes) -> (usize, Option<String>) {
    let count = stashes.entries.len();
    (
        count,
        stashes.entries.into_iter().next().map(|stash| stash.hash),
    )
}

/// The stashes of `repository`, newest first.
pub(crate) fn list(runner: &GitRunner, repository: &Repository) -> Result<Stashes, Failure> {
    let root = &repository.root;
    let invocation = runner
        .run(root, &["stash", "list", "--no-show-signature", LIST_FORMAT])
        .map_err(git_failed_to_start)?;
    if !invocation.success {
        return Err(failure_of(
            runner,
            root,
            &invocation,
            Failure::StashesUnreadable {
                folder: root.clone(),
            },
        ));
    }
    let entries = invocation
        .stdout_text()
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let mut fields = line.splitn(3, '\0');
            let (Some(hash), Some(created_at), Some(message)) =
                (fields.next(), fields.next(), fields.next())
            else {
                return None;
            };
            Some(Stash {
                name: format!("stash@{{{index}}}"),
                hash: hash.to_owned(),
                message: message.to_owned(),
                created_at: created_at.parse().unwrap_or_default(),
            })
        })
        .collect();
    Ok(Stashes {
        repository: repository.clone(),
        unavailable: unavailable(runner, repository)?,
        entries,
    })
}

/// The stashes of `repository`, as [`list`] reads them, when it can stash, and
/// [`Failure::StashUnavailable`] otherwise.
fn available(runner: &GitRunner, repository: &Repository) -> Result<Stashes, Failure> {
    let stashes = list(runner, repository)?;
    match stashes.unavailable {
        Some(reason) => Err(Failure::StashUnavailable {
            folder: repository.root.clone(),
            reason,
        }),
        None => Ok(stashes),
    }
}

/// Why `repository` cannot stash, when it cannot.
fn unavailable(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Option<StashUnavailable>, Failure> {
    Ok((!commit::has_commits(runner, &repository.root)?).then_some(StashUnavailable::NoCommits))
}

/// The stash of `stashes` whose commit is `hash`, or [`Failure::StashNotFound`] when there is none
/// such: it was dropped, or `hash` is no stash.
fn find(stashes: Stashes, hash: &str) -> Result<Stash, Failure> {
    stashes
        .entries
        .into_iter()
        .find(|stash| stash.hash == hash)
        .ok_or_else(|| Failure::StashNotFound {
            hash: hash.to_owned(),
        })
}

/// The commits a stash is made of.
pub(crate) struct StashCommits {
    /// The commit HEAD was on when the stash was made.
    pub base: String,
    /// The stash's own commit, which holds the working tree with every change to a tracked file.
    pub commit: String,
    /// The files git did not track, when the stash includes them: a commit with no parent.
    pub untracked: Option<String>,
}

impl StashCommits {
    /// The commits of the stash `hash` of `repository`.
    pub(crate) fn of(
        runner: &GitRunner,
        repository: &Repository,
        hash: &str,
    ) -> Result<Self, Failure> {
        let stash = find(list(runner, repository)?, hash)?;
        let mut parents = details::parents_of(runner, repository, &stash.hash)?.into_iter();
        let base = parents.next().ok_or_else(|| Failure::StashNotFound {
            hash: hash.to_owned(),
        })?;
        Ok(Self {
            base,
            commit: stash.hash,
            untracked: parents.nth(1),
        })
    }

    /// The files the stash holds, by path: each change to a tracked file against the commit it was
    /// made on, then the files git did not track.
    pub(crate) fn files(
        &self,
        runner: &GitRunner,
        repository: &Repository,
    ) -> Result<Vec<Change>, Failure> {
        let mut files = details::changed_files(runner, repository, Some(&self.base), &self.commit)?;
        if let Some(untracked) = &self.untracked {
            let listed = log::run_git(
                runner,
                repository,
                &[
                    "ls-tree",
                    "-r",
                    "-z",
                    "--name-only",
                    "--end-of-options",
                    untracked,
                ],
            )?;
            files.extend(
                listed
                    .stdout
                    .split(|byte| *byte == 0)
                    .filter(|path| !path.is_empty())
                    .map(|path| Change {
                        path: path_from_bytes(path),
                        status: ChangeStatus::Untracked,
                        original_path: None,
                        conflict: None,
                        submodule: None,
                    }),
            );
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(files)
    }
}

/// Drops the stash `hash` of `repository`, wherever it is in the list now. git drops a stash by its
/// place in the list, so the place is checked to still hold that stash right before.
pub(crate) fn drop(runner: &GitRunner, repository: &Repository, hash: &str) -> Result<(), Failure> {
    let stash = find(available(runner, repository)?, hash)?;
    if details::commit_if_any(runner, repository, &stash.name)?.as_deref() != Some(hash) {
        return Err(Failure::StashNotFound {
            hash: hash.to_owned(),
        });
    }
    run(
        runner,
        repository,
        &["stash", "drop", "--quiet", &stash.name],
        Failure::StashNotDropped {
            folder: repository.root.clone(),
        },
    )
}

/// Drops every stash of `repository`, when they are still `seen`, the hashes of the stashes the
/// user confirmed, newest first. Otherwise this returns [`Failure::StashesChanged`] and drops
/// none, so no stash the user did not see goes.
pub(crate) fn clear(
    runner: &GitRunner,
    repository: &Repository,
    seen: &[String],
) -> Result<(), Failure> {
    let stashes = available(runner, repository)?;
    if !stashes.entries.iter().map(|stash| &stash.hash).eq(seen) {
        return Err(Failure::StashesChanged {
            folder: repository.root.clone(),
        });
    }
    run(
        runner,
        repository,
        &["stash", "clear"],
        Failure::StashesNotCleared {
            folder: repository.root.clone(),
        },
    )
}

/// The diff of the file at `path` in the stash `hash` of `repository`, against what `against`
/// says.
pub(crate) fn file_diff(
    runner: &GitRunner,
    repository: &Repository,
    hash: &str,
    path: &str,
    against: StashDiffBase,
) -> Result<StashFileDiff, Failure> {
    let commits = StashCommits::of(runner, repository, hash)?;
    let change = commits
        .files(runner, repository)?
        .into_iter()
        .find(|change| change.path == Path::new(path));
    let untracked = change
        .as_ref()
        .is_some_and(|change| change.status == ChangeStatus::Untracked);
    let original_path = change.and_then(|change| change.original_path);
    // A file git did not track is in a commit of its own, which has no parent.
    let (holding_commit, parent) = match (&commits.untracked, untracked) {
        (Some(untracked), true) => (untracked.as_str(), None),
        _ => (commits.commit.as_str(), Some(commits.base.as_str())),
    };
    let content = match against {
        StashDiffBase::Local => {
            diff::stored_against_working_tree(runner, repository, holding_commit, path)?
        }
        StashDiffBase::Parent => diff::stored_against_parent(
            runner,
            repository,
            holding_commit,
            parent,
            path,
            original_path.as_deref(),
        )?,
    };
    Ok(StashFileDiff {
        repository: repository.clone(),
        stash: commits.commit,
        against,
        path: PathBuf::from(path),
        original_path,
        content,
    })
}
