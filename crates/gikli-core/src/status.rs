//! The status reader: turns git's porcelain status into the changes of a Repository.

use std::path::PathBuf;

use serde::Serialize;

use crate::Repository;
use crate::git::path_from_bytes;

/// The uncommitted changes of one Repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSet {
    pub repository: Repository,
    /// One entry per changed file, ordered by path.
    pub changes: Vec<Change>,
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// The file's path relative to the Repository root, with `/` between folders, as git
    /// writes it.
    pub path: PathBuf,
    pub status: ChangeStatus,
}

/// How a file differs from the last commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeStatus {
    Modified,
    /// A new file added to git.
    Added,
    /// A tracked file removed from the working tree or from git.
    Deleted,
    /// A new file git does not track yet.
    Untracked,
}

/// The arguments after `git` that list the changes [`parse`] reads.
///
/// Every untracked file is listed on its own, not only the new folder that holds it, whatever
/// `status.showUntrackedFiles` says. Renames are off until they get their own status (#7): a
/// moved file shows as its deletion and its addition, so neither path goes missing.
pub(crate) const STATUS_ARGS: &[&str] = &[
    "status",
    "--porcelain=v2",
    "-z",
    "--untracked-files=all",
    "--no-renames",
];

/// Reads the output of `git` [`STATUS_ARGS`].
pub(crate) fn parse(output: &[u8]) -> Vec<Change> {
    let mut changes: Vec<Change> = output
        .split(|byte| *byte == 0)
        .filter_map(|record| match record.split_first()? {
            // `1 XY sub mH mI mW hH hI path`: an ordinary change to a tracked file.
            (b'1', _) => {
                let mut fields = record.splitn(9, |byte| *byte == b' ').skip(1);
                let status = tracked_status(fields.next()?)?;
                Some(Change {
                    path: path_from_bytes(fields.nth(6)?),
                    status,
                })
            }
            // `u XY sub m1 m2 m3 mW h1 h2 h3 path`: a file with a merge conflict. Conflicts get
            // their own status with #7; until then the file stays in the list as modified.
            (b'u', _) => Some(Change {
                path: path_from_bytes(record.splitn(11, |byte| *byte == b' ').nth(10)?),
                status: ChangeStatus::Modified,
            }),
            // `? path`
            (b'?', rest) => Some(Change {
                path: path_from_bytes(rest.strip_prefix(b" ")?),
                status: ChangeStatus::Untracked,
            }),
            _ => None,
        })
        .collect();
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    changes
}

/// The status of a tracked file from its `XY` field: `X` compares the index with the last commit,
/// `Y` the working tree with the index.
fn tracked_status(xy: &[u8]) -> Option<ChangeStatus> {
    let [index, worktree] = xy else {
        return None;
    };
    Some(match (index, worktree) {
        (b'A', _) => ChangeStatus::Added,
        (b'D', _) | (_, b'D') => ChangeStatus::Deleted,
        _ => ChangeStatus::Modified,
    })
}
