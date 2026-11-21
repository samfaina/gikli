//! Saved local changes: the uncommitted changes gikli puts aside in a stash before an operation
//! that needs a clean working tree, and restores when the operation finishes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Repository;

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
