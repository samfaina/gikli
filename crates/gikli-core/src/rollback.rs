//! Rollback: discarding the uncommitted changes of the files the user chose, once they have
//! confirmed the list of what goes.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::status::{Change, ChangeStatus, StagingSide, Status};

/// What rolling back a selection of changes would discard, for the user to confirm first. A rollback
/// takes the plan they confirmed, and runs only while it still holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackPlan {
    /// The files whose changes would be discarded, and the new or ignored files the rollback would
    /// write over, ordered by path.
    pub files: Vec<Change>,
    /// Whether any of them is a file the rollback takes out of git, whose local copy the user can
    /// choose to delete as well.
    pub has_added_files: bool,
    /// The side of the index the plan is about, in Staging mode. None for a rollback of changes in
    /// Changelists, which discards every version of a file.
    pub side: Option<StagingSide>,
}

impl RollbackPlan {
    /// The paths of the files on the plan.
    pub(crate) fn paths(&self) -> Vec<String> {
        self.files.iter().map(|file| text_of(&file.path)).collect()
    }
}

/// The changes in `status` at `paths` that a rollback on `side` can discard. Only a change in a
/// Changelist has something to go back to. A submodule is left out unless it is new: git moves only
/// what the Repository records, so a changed one would stay as it is, and a moved one would lose its
/// files.
fn selected<'s>(
    status: &'s Status,
    paths: &[String],
    side: Option<StagingSide>,
) -> Vec<&'s Change> {
    let rows = match side {
        None => &status.changes,
        Some(StagingSide::Staged) => &status.staged,
        Some(StagingSide::Unstaged) => &status.unstaged,
    };
    rows.iter()
        .filter(|change| change.status.is_in_changelists())
        .filter(|change| change.status == ChangeStatus::Added || change.submodule.is_none())
        .filter(|change| paths.iter().any(|path| Path::new(path) == change.path))
        .collect()
}

/// What rolling back the changes in `status` at `paths` on `side` would discard. `status` must list
/// the ignored files, as a rollback can write over them too.
pub(crate) fn plan(status: &Status, paths: &[String], side: Option<StagingSide>) -> RollbackPlan {
    let chosen = selected(status, paths, side);
    let has_added_files = chosen.iter().any(|change| {
        matches!(reach(status, change, side), Reach::LastCommit) && takes_out_of_git(change)
    });
    let restored: Vec<&Path> = chosen
        .iter()
        .filter_map(|change| match reach(status, change, side) {
            Reach::IndexOnly => None,
            Reach::DiskOnly => Some(change.path.as_path()),
            Reach::LastCommit => (!takes_out_of_git(change)).then(|| restored_path(change)),
        })
        .collect();
    // A file at a path that comes back, or a folder or a file in the way of one.
    let written_over = status.changes.iter().filter(|file| {
        !file.status.is_in_changelists()
            && restored
                .iter()
                .any(|path| file.path.starts_with(path) || path.starts_with(&file.path))
    });
    let mut files: Vec<Change> = chosen
        .iter()
        .copied()
        .chain(written_over)
        .cloned()
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    RollbackPlan {
        files,
        has_added_files,
        side,
    }
}

/// The arguments after `git` of each command that rolls back the changes in `status` at `paths` on
/// `side`, in the order to run them. The file of an added change goes too if `delete_added_files`.
pub(crate) fn commands(
    status: &Status,
    paths: &[String],
    side: Option<StagingSide>,
    delete_added_files: bool,
) -> Vec<Vec<String>> {
    let mut steps = Steps::default();
    for change in selected(status, paths, side) {
        let path = text_of(&change.path);
        match reach(status, change, side) {
            Reach::DiskOnly => steps.back_to_index.push(path),
            // The last commit does not hold it, and the file on disk is the other side.
            Reach::IndexOnly if takes_out_of_git(change) => steps.unindex.push(path),
            Reach::IndexOnly => {
                steps.index_to_head.push(path);
                // A move is two changes to git.
                steps
                    .index_to_head
                    .extend(change.renamed_from().map(text_of));
            }
            Reach::LastCommit if takes_out_of_git(change) => {
                if delete_added_files {
                    steps.delete.push(path);
                } else {
                    steps.unindex.push(path);
                }
            }
            Reach::LastCommit => {
                steps.restore.push(text_of(restored_path(change)));
                if change.renamed_from().is_some() {
                    // Going back from a move leaves nothing at the new path, as a modified file
                    // keeps nothing of what it held.
                    steps.delete.push(path);
                } else if let Some(moved_to) = status.moved_and_removed.get(&change.path) {
                    steps.delete.push(text_of(moved_to));
                }
            }
        }
    }
    steps.into_commands()
}

/// How far back a rollback takes the file of a change.
enum Reach {
    /// Every version of the file goes, down to the last commit.
    LastCommit,
    /// Only the index goes back to the last commit: the file on disk is the other side, and stays.
    IndexOnly,
    /// Only the file on disk goes back to the index: what is staged is the other side, and stays.
    DiskOnly,
}

/// How far back a rollback on `side` takes the file of `change`. A file on both sides of the index
/// keeps the other side.
fn reach(status: &Status, change: &Change, side: Option<StagingSide>) -> Reach {
    let holds_file = |rows: &[Change]| rows.iter().any(|row| row.path == change.path);
    match side {
        Some(StagingSide::Staged) if holds_file(&status.unstaged) => Reach::IndexOnly,
        Some(StagingSide::Unstaged) if holds_file(&status.staged) => Reach::DiskOnly,
        _ => Reach::LastCommit,
    }
}

fn text_of(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Where the file of `change`, which the last commit holds, comes back.
fn restored_path(change: &Change) -> &Path {
    change.renamed_from().unwrap_or(&change.path)
}

/// Whether rolling back `change` takes the file out of git, as the last commit does not hold it.
/// The file of a rename is not one of these: the rollback moves it back.
fn takes_out_of_git(change: &Change) -> bool {
    match change.status {
        ChangeStatus::Added | ChangeStatus::Copied => true,
        ChangeStatus::Conflicted => change
            .conflict
            .is_some_and(|conflict| !conflict.is_in_last_commit()),
        _ => false,
    }
}

/// The paths each git command works on.
#[derive(Default)]
struct Steps {
    /// Taken out of git, with their local copies left.
    unindex: Vec<String>,
    /// Taken out of git with their local copies.
    delete: Vec<String>,
    /// Brought back as the last commit has them.
    restore: Vec<String>,
    /// Brought back in the index as the last commit has them, and left on disk.
    index_to_head: Vec<String>,
    /// Brought back on disk as the index has them.
    back_to_index: Vec<String>,
}

impl Steps {
    /// The commands in the order to run them. What leaves git goes first: on a disk that ignores
    /// case, a file renamed only in case is one file, and removing the new name after restoring the
    /// old one would delete it.
    fn into_commands(self) -> Vec<Vec<String>> {
        [
            // `--force` as well: git refuses otherwise when the file changed after it was added.
            (vec!["rm", "--quiet", "--force", "--cached"], self.unindex),
            (vec!["rm", "--quiet", "--force"], self.delete),
            (
                vec!["restore", "--source=HEAD", "--staged", "--worktree"],
                self.restore,
            ),
            (
                vec!["restore", "--source=HEAD", "--staged"],
                self.index_to_head,
            ),
            (vec!["restore", "--worktree"], self.back_to_index),
        ]
        .into_iter()
        .filter(|(_, paths)| !paths.is_empty())
        .map(|(command, paths)| {
            std::iter::once("--literal-pathspecs")
                .chain(command)
                .chain(std::iter::once("--"))
                .map(str::to_owned)
                .chain(paths)
                .collect()
        })
        .collect()
    }
}
