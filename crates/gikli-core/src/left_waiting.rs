//! What a cherry-pick or a revert from gikli left to do after the commit git stopped on, kept in
//! the git directory of the worktree together with where git stopped the operation that left it,
//! so that only going on with that same operation from gikli does it.

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::cherry_pick::Run;
use crate::git::GitRunner;
use crate::in_progress::{self, OperationStop};
use crate::project::{take_once, write_at_once};
use crate::{Failure, OperationKind, Repository};

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
    fn kind(&self) -> OperationKind {
        match self {
            Self::ToPick(_) => OperationKind::CherryPick,
            Self::ToRevert(_) => OperationKind::Revert,
        }
    }

    /// The failure of keeping this in `file`, as `message` says.
    fn not_kept(&self, file: PathBuf, message: String) -> Failure {
        match self {
            Self::ToPick(_) => Failure::CommitsToPickNotKept { file, message },
            Self::ToRevert(_) => Failure::CommitsToRevertNotKept { file, message },
        }
    }
}

/// Keeps `left` waiting in the git directory of `repository`, for the operation in progress
/// there, which left it, as git stopped it now, in place of what waited before.
pub(crate) fn keep(runner: &GitRunner, repository: &Repository, left: Left) -> Result<(), Failure> {
    let stop = in_progress::stop(runner, repository, left.kind())?;
    LeftWaiting::of(runner, repository)?.keep(left, stop)
}

/// Takes what waits in the git directory of `repository` when the operation of `kind` in progress
/// there is the one that left it, and gives it up otherwise.
pub(crate) fn take(
    runner: &GitRunner,
    repository: &Repository,
    kind: OperationKind,
) -> Result<Option<Left>, Failure> {
    let stop = in_progress::stop(runner, repository, kind)?;
    Ok(LeftWaiting::of(runner, repository)?.take(&stop))
}

/// What the store holds: what waits, and where git stopped the operation that left it.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Kept {
    operation: OperationStop,
    left: Left,
}

/// Where what a cherry-pick or a revert from gikli left waits, in the git directory of the
/// worktree.
pub(crate) struct LeftWaiting {
    file: PathBuf,
}

impl LeftWaiting {
    /// The store in the git directory of the worktree `repository` is.
    pub fn of(runner: &GitRunner, repository: &Repository) -> Result<Self, Failure> {
        let git_dir = in_progress::git_dir(runner, repository)?;
        Ok(Self {
            file: git_dir.join("gikli").join("left-waiting.json"),
        })
    }

    /// Keeps `left`, which the operation in progress, stopped at `operation`, left to do, in place
    /// of what waited before.
    pub fn keep(&self, left: Left, operation: OperationStop) -> Result<(), Failure> {
        let kept = Kept { operation, left };
        let json = serde_json::to_vec_pretty(&kept).map_err(io::Error::other);
        json.and_then(|json| write_at_once(&self.file, &json))
            .map_err(|error| kept.left.not_kept(self.file.clone(), error.to_string()))
    }

    /// Takes what waits, so that one taker alone does it, when the operation in progress, stopped
    /// at `operation`, is the one that left it. What another operation left is given up. Nothing
    /// waits when nothing can be read.
    pub fn take(&self, operation: &OperationStop) -> Option<Left> {
        let kept: Kept =
            take_once(&self.file).and_then(|json| serde_json::from_slice(&json).ok())?;
        (kept.operation == *operation).then_some(kept.left)
    }

    /// Gives up what waits, as when the operation that left it is over.
    pub fn give_up(&self) {
        take_once(&self.file);
    }
}
