//! Saved local changes: the uncommitted changes gikli puts aside in a stash before an operation
//! that needs a clean working tree, and restores when the operation finishes.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::project::{take_once, write_at_once};
use crate::{Failure, Repository};

/// The message of the stash that holds Saved local changes.
pub(crate) const STASH_MESSAGE: &str = "Local changes saved by gikli";

/// What saving the local changes of a Repository put aside, for restoring them afterwards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedLocalChanges {
    pub repository: Repository,
    /// The full hash of the stash that holds them, or None when there was nothing to save.
    pub stash: Option<String>,
    /// The identifier of the Changelist each saved change belonged to, by path relative to the
    /// Repository root, so restoring puts it back there.
    pub changelist_of_path: BTreeMap<String, String>,
}

/// What restoring Saved local changes came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredLocalChanges {
    /// Whether restoring left files in conflict, which conflict resolution then lists. The stash
    /// that holds the changes is kept then.
    pub conflicts_left: bool,
}

/// Where the Saved local changes that wait for a merge stopped half done in one worktree are kept,
/// in its git directory beside its Changelists, so they wait there whatever window is open, and
/// whoever ends the merge.
pub(crate) struct AwaitingMergeStore {
    file: PathBuf,
}

impl AwaitingMergeStore {
    /// The store in `git_dir`, which is the git directory of the worktree.
    pub fn in_git_dir(git_dir: &Path) -> Self {
        Self {
            file: git_dir.join("gikli").join("saved-for-merge.json"),
        }
    }

    /// Keeps `saved`, in place of what waited before.
    pub fn keep(&self, saved: &SavedLocalChanges) -> Result<(), Failure> {
        let json = serde_json::to_vec_pretty(saved).map_err(io::Error::other);
        json.and_then(|json| write_at_once(&self.file, &json))
            .map_err(|error| Failure::LocalChangesNotKeptForMerge {
                file: self.file.clone(),
                message: error.to_string(),
            })
    }

    /// Takes what waits, if anything does, so that one restore alone brings it back, even when
    /// another one in this window or another reads the merge over at the same time. What cannot be
    /// read does not wait: its stash stays in the stash list, for the user to apply.
    pub fn take(&self) -> Option<SavedLocalChanges> {
        serde_json::from_slice(&take_once(&self.file)?).ok()
    }
}
