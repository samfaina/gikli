//! Saved local changes: the uncommitted changes gikli puts aside in a stash before an operation
//! that needs a clean working tree, and restores when the operation finishes.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::project::write_at_once;
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

/// Saved local changes that wait for a merge that stopped half done to be over, so they come back
/// when the user aborts it or commits it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AwaitingMerge {
    /// The full hash of the commit the merge brings in, as git names it `MERGE_HEAD`, which tells
    /// that merge from a later one.
    pub merging: String,
    pub saved: SavedLocalChanges,
}

/// Where the Saved local changes that wait for a merge in one worktree are kept, in its git
/// directory beside its Changelists, so they wait there whatever window is open.
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

    /// Keeps `awaiting`, in place of what waited before.
    pub fn keep(&self, awaiting: &AwaitingMerge) -> Result<(), Failure> {
        let json = serde_json::to_vec_pretty(awaiting).map_err(io::Error::other);
        json.and_then(|json| write_at_once(&self.file, &json))
            .map_err(|error| Failure::LocalChangesNotKept {
                file: self.file.clone(),
                message: error.to_string(),
            })
    }

    /// What waits for the merge of `merging`, if anything does. What waits for another merge, or
    /// cannot be read, does not: its stash stays in the stash list, for the user to apply.
    pub fn awaiting(&self, merging: &str) -> Option<AwaitingMerge> {
        let json = fs::read(&self.file).ok()?;
        let awaiting: AwaitingMerge = serde_json::from_slice(&json).ok()?;
        (awaiting.merging == merging).then_some(awaiting)
    }

    /// Forgets what waits, as it came back or there was nothing to wait. Nothing waiting is no
    /// failure.
    pub fn forget(&self) {
        // A file that cannot be removed waits for a merge that is over, which no later merge
        // takes for its own.
        let _ = fs::remove_file(&self.file);
    }
}
