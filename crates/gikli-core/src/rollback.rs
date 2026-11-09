//! Rollback: discarding the uncommitted changes of the files the user chose, once they have
//! confirmed the list of what goes.

use std::path::Path;

use serde::Serialize;

use crate::status::{Change, ChangeStatus, Status};

/// What rolling back a selection of changes would discard, for the user to confirm first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollbackPlan {
    /// The files whose changes would be discarded, ordered by path.
    pub files: Vec<Change>,
    /// Whether any of them is a file the rollback takes out of git, whose local copy the user can
    /// choose to delete as well.
    pub has_added_files: bool,
}

/// The changes in `status` at `paths` that a rollback can discard. Only a change in a Changelist has
/// something to go back to. A changed submodule is left out: git moves only what the Repository
/// records, and the submodule would stay as it is.
fn selected<'s>(status: &'s Status, paths: &[String]) -> Vec<&'s Change> {
    status
        .changes
        .iter()
        .filter(|change| change.status.is_in_changelists())
        .filter(|change| change.status != ChangeStatus::Modified || change.submodule.is_none())
        .filter(|change| paths.iter().any(|path| Path::new(path) == change.path))
        .collect()
}

/// What rolling back the changes in `status` at `paths` would discard.
pub(crate) fn plan(status: &Status, paths: &[String]) -> RollbackPlan {
    let selected = selected(status, paths);
    let has_added_files = selected.iter().any(|change| takes_out_of_git(change));
    let written_over = status.changes.iter().filter(|new_file| {
        new_file.status == ChangeStatus::Untracked
            && selected.iter().any(|change| {
                change.status == ChangeStatus::Renamed
                    && change.original_path.as_ref() == Some(&new_file.path)
            })
    });
    let mut files: Vec<Change> = selected
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
        } else if let Some(original) = change
            .original_path
            .as_ref()
            .filter(|_| change.status == ChangeStatus::Renamed)
        {
            // Going back from a move leaves nothing at the new path, as a modified file keeps nothing
            // of what it held.
            steps.restore.push(text_of(original));
            steps.delete.push(path);
        } else {
            steps.restore.push(path);
            if let Some(moved_to) = status.moved_and_removed.get(&change.path) {
                steps.delete.push(text_of(moved_to));
            }
        }
    }
    steps.into_commands()
}

fn text_of(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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

/// The paths each git command works on. Those in the last commit come first, as that is the command
/// most likely to fail.
#[derive(Default)]
struct Steps {
    /// Brought back as the last commit has them.
    restore: Vec<String>,
    /// Taken out of git, with their local copies left.
    unindex: Vec<String>,
    /// Taken out of git with their local copies.
    delete: Vec<String>,
}

impl Steps {
    fn into_commands(self) -> Vec<Vec<String>> {
        [
            (
                vec!["restore", "--source=HEAD", "--staged", "--worktree"],
                self.restore,
            ),
            // `--force` as well: git refuses otherwise when the file changed after it was added.
            (vec!["rm", "--quiet", "--force", "--cached"], self.unindex),
            (vec!["rm", "--quiet", "--force"], self.delete),
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
