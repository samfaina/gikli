//! What a cherry-pick or a revert from gikli left to do after the commit git stopped on, kept in
//! the git directory of the worktree together with which operation left it, so that only going on
//! with that same operation from gikli does it.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cherry_pick::Run;
use crate::in_progress::Identity;
use crate::project::{take_once, write_at_once};
use crate::{Failure, OperationKind};

/// What waits for the operation in progress to be over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Left {
    /// The runs of a cherry-pick after the one git stopped on, to pick in order.
    ToPick(Vec<Run>),
    /// The commits of a revert after the one in progress, by their full hashes, to revert in
    /// order.
    ToRevert(Vec<String>),
}

impl Left {
    /// The kind of operation that leaves this to do.
    pub fn kind(&self) -> OperationKind {
        match self {
            Self::ToPick(_) => OperationKind::CherryPick,
            Self::ToRevert(_) => OperationKind::Revert,
        }
    }
}

/// What the store holds: what waits, and the operation that left it.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Kept {
    operation: Identity,
    left: Left,
}

/// Where what a cherry-pick or a revert from gikli left waits, in the git directory of the
/// worktree.
pub(crate) struct LeftWaiting {
    file: PathBuf,
}

impl LeftWaiting {
    /// The store in `git_dir`, which is the git directory of the worktree.
    pub fn in_git_dir(git_dir: &Path) -> Self {
        Self {
            file: git_dir.join("gikli").join("left-waiting.json"),
        }
    }

    /// Keeps `left`, which `operation`, the operation in progress, left to do, in place of what
    /// waited before.
    pub fn keep(&self, left: Left, operation: Identity) -> Result<(), Failure> {
        let not_kept: fn(PathBuf, String) -> Failure = match left {
            Left::ToPick(_) => |file, message| Failure::CommitsToPickNotKept { file, message },
            Left::ToRevert(_) => |file, message| Failure::CommitsToRevertNotKept { file, message },
        };
        let json = serde_json::to_vec_pretty(&Kept { operation, left }).map_err(io::Error::other);
        json.and_then(|json| write_at_once(&self.file, &json))
            .map_err(|error| not_kept(self.file.clone(), error.to_string()))
    }

    /// Takes what waits, so that one taker alone does it, when `operation`, the operation in
    /// progress, is the one that left it. What another operation left is given up. Nothing waits
    /// when nothing can be read.
    pub fn take(&self, operation: &Identity) -> Option<Left> {
        let kept: Kept =
            take_once(&self.file).and_then(|json| serde_json::from_slice(&json).ok())?;
        (kept.operation == *operation && kept.left.kind() == operation.kind).then_some(kept.left)
    }

    /// Gives up what waits, as when the operation that left it is over.
    pub fn give_up(&self) {
        take_once(&self.file);
    }
}
