//! Rollback: discarding the uncommitted changes of the files the user chose, once they have
//! confirmed the list of what goes.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::status::{Change, ChangeStatus, Status};

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
}

impl RollbackPlan {
    /// The paths of the files on the plan.
    pub(crate) fn paths(&self) -> Vec<String> {
        self.files.iter().map(|file| text_of(&file.path)).collect()
    }
}

/// The changes in `status` at `paths` that a rollback can discard. Only a change in a Changelist has
/// something to go back to. A submodule is left out unless it is new: git moves only what the
/// Repository records, so a changed one would stay as it is, and a moved one would lose its files.
fn selected<'s>(status: &'s Status, paths: &[String]) -> Vec<&'s Change> {
    status
        .changes
        .iter()
        .filter(|change| change.status.is_in_changelists())
        .filter(|change| change.status == ChangeStatus::Added || change.submodule.is_none())
        .filter(|change| paths.iter().any(|path| Path::new(path) == change.path))
        .collect()
}

/// What rolling back the changes in `status` at `paths` would discard. `status` must list the ignored
/// files, as a rollback can write over them too.
pub(crate) fn plan(status: &Status, paths: &[String]) -> RollbackPlan {
    let chosen = selected(status, paths);
    let has_added_files = chosen.iter().any(|change| takes_out_of_git(change));
    let restored: Vec<&Path> = chosen
        .iter()
        .filter(|change| !takes_out_of_git(change))
        .map(|change| restored_path(change))
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
    }
}

/// The arguments after `git` of each command that rolls back the changes in `status` at `paths`, in
/// the order to run them. The file of an added change goes too if `delete_added_files`.
pub(crate) fn commands(
    status: &Status,
    paths: &[String],
    delete_added_files: bool,
) -> Vec<Vec<String>> {
    let mut steps = Steps::default();
    for change in selected(status, paths) {
        let path = text_of(&change.path);
        if takes_out_of_git(change) {
            if delete_added_files {
                steps.delete.push(path);
            } else {
                steps.unindex.push(path);
            }
            continue;
        }
        steps.restore.push(text_of(restored_path(change)));
        if change.renamed_from().is_some() {
            // Going back from a move leaves nothing at the new path, as a modified file keeps
            // nothing of what it held.
            steps.delete.push(path);
        } else if let Some(moved_to) = status.moved_and_removed.get(&change.path) {
            steps.delete.push(text_of(moved_to));
        }
    }
    steps.into_commands()
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
