//! The status reader: turns git's porcelain status into the changes of a Repository.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::git::path_from_bytes;

/// The uncommitted changes of one Repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSet {
    pub repository: Repository,
    /// One entry per changed file, ordered by path.
    pub changes: Vec<Change>,
    /// The changes as the view shows them, grouped as the user chose.
    pub nodes: Vec<ChangeNode>,
}

/// How the changes view arranges the changes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeGrouping {
    /// A tree of the folders that hold changes.
    #[default]
    Folder,
    /// One list of every change, ordered by path.
    Flat,
}

/// One row of the changes view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChangeNode {
    /// A folder holding changes, when they are grouped by folder. Its subfolders come first,
    /// then its files, each in order of name.
    #[serde(rename_all = "camelCase")]
    Folder {
        /// Relative to the Repository root, with `/` between folders.
        path: PathBuf,
        /// Whether anything in the folder is a change, not only ignored files. The view
        /// highlights such folders.
        has_changes: bool,
        /// The subfolders and files in the folder.
        children: Vec<ChangeNode>,
    },
    /// A changed file.
    File(Change),
}

impl ChangeSet {
    /// The `changes` of `repository`, with their rows arranged by `grouping`.
    pub(crate) fn new(
        repository: Repository,
        changes: Vec<Change>,
        grouping: ChangeGrouping,
    ) -> Self {
        let nodes = match grouping {
            ChangeGrouping::Flat => changes.iter().cloned().map(ChangeNode::File).collect(),
            ChangeGrouping::Folder => {
                let mut root = FolderContents::default();
                for change in &changes {
                    root.insert(change);
                }
                root.into_nodes(Path::new(""))
            }
        };
        Self {
            repository,
            changes,
            nodes,
        }
    }
}

impl ChangeSet {
    /// The files before and after `path` in the changes view when arranged by `grouping`, leaving
    /// out ignored files. A file that is not a change keeps the place it would have as one, as the
    /// user can still be looking at it when it stops being one.
    pub(crate) fn neighbors_of(
        &self,
        path: &Path,
        grouping: ChangeGrouping,
    ) -> (Option<PathBuf>, Option<PathBuf>) {
        let mut changes: Vec<Change> = self
            .changes
            .iter()
            .filter(|change| change.status != ChangeStatus::Ignored)
            .cloned()
            .collect();
        if !changes.iter().any(|change| change.path == path) {
            changes.push(Change {
                path: path.to_path_buf(),
                status: ChangeStatus::Modified,
                original_path: None,
                conflict: None,
                submodule: None,
            });
        }
        changes.sort_by(|left, right| left.path.cmp(&right.path));
        let arranged = Self::new(self.repository.clone(), changes, grouping);
        let mut files = Vec::new();
        for node in &arranged.nodes {
            node.collect_files(&mut files);
        }
        let Some(place) = files.iter().position(|file| *file == path) else {
            return (None, None);
        };
        let before = place.checked_sub(1).and_then(|before| files.get(before));
        (
            before.map(|file| file.to_path_buf()),
            files.get(place + 1).map(|file| file.to_path_buf()),
        )
    }
}

/// The subfolders and files of one folder, while the tree is being built.
#[derive(Default)]
struct FolderContents {
    folders: BTreeMap<OsString, FolderContents>,
    files: Vec<Change>,
}

impl FolderContents {
    fn insert(&mut self, change: &Change) {
        let mut folder = self;
        let mut names: Vec<_> = change
            .path
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => Some(name),
                _ => None,
            })
            .collect();
        names.pop();
        for name in names {
            folder = folder.folders.entry(name.to_owned()).or_default();
        }
        folder.files.push(change.clone());
    }

    /// The rows of this folder, whose path is `path`.
    fn into_nodes(self, path: &Path) -> Vec<ChangeNode> {
        let folders = self.folders.into_iter().map(|(name, contents)| {
            let mut folder_path = path.as_os_str().to_owned();
            if !folder_path.is_empty() {
                folder_path.push("/");
            }
            folder_path.push(name);
            let children = contents.into_nodes(&PathBuf::from(folder_path.clone()));
            ChangeNode::Folder {
                path: PathBuf::from(folder_path),
                has_changes: children.iter().any(ChangeNode::has_changes),
                children,
            }
        });
        let files = self.files.into_iter().map(ChangeNode::File);
        folders.chain(files).collect()
    }
}

impl ChangeNode {
    /// Adds the paths of the files in this row to `files`, in the order the view shows them.
    fn collect_files<'a>(&'a self, files: &mut Vec<&'a Path>) {
        match self {
            Self::Folder { children, .. } => {
                for child in children {
                    child.collect_files(files);
                }
            }
            Self::File(change) => files.push(&change.path),
        }
    }

    /// Whether the row is, or holds, a change other than an ignored file.
    fn has_changes(&self) -> bool {
        match self {
            Self::Folder { has_changes, .. } => *has_changes,
            Self::File(change) => change.status != ChangeStatus::Ignored,
        }
    }
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// The file's path relative to the Repository root, with `/` between folders, as git
    /// writes it.
    pub path: PathBuf,
    pub status: ChangeStatus,
    /// The path the file was moved or copied from, for a renamed or copied file.
    pub original_path: Option<PathBuf>,
    /// How the two sides of a merge disagree, for a conflicted file.
    pub conflict: Option<ConflictKind>,
    /// What changed inside the submodule, when the file is a submodule.
    pub submodule: Option<SubmoduleState>,
}

impl Change {
    fn new(path: &[u8], status: ChangeStatus) -> Self {
        Self {
            path: path_from_bytes(path),
            status,
            original_path: None,
            conflict: None,
            submodule: None,
        }
    }

    /// The change with the submodule state of the `sub` field of its record.
    fn in_submodule(self, sub: &[u8]) -> Self {
        Self {
            submodule: SubmoduleState::from_sub(sub),
            ..self
        }
    }
}

/// What changed inside a submodule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmoduleState {
    /// The submodule has another commit checked out than the one the Repository records.
    pub commit_changed: bool,
    /// Files the submodule tracks have uncommitted changes.
    pub has_tracked_changes: bool,
    /// The submodule has new files it does not track.
    pub has_untracked_files: bool,
}

impl SubmoduleState {
    /// The state from the `sub` field of a record: `N...` for a file that is not a submodule,
    /// and `S<c><m><u>` for a submodule, each flag a letter when set and `.` when not.
    fn from_sub(sub: &[u8]) -> Option<Self> {
        let [b'S', commit, tracked, untracked] = sub else {
            return None;
        };
        Some(Self {
            commit_changed: *commit == b'C',
            has_tracked_changes: *tracked == b'M',
            has_untracked_files: *untracked == b'U',
        })
    }
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
    /// A tracked file moved to a new path, staged as a move.
    Renamed,
    /// A new file staged as a copy of another file. git looks for copies only when the user's
    /// `status.renames` is `copies`.
    Copied,
    /// A new file git does not track yet.
    Untracked,
    /// A file git ignores, listed only when the user turns ignored files on. A folder git
    /// ignores as a whole appears once, with its path ending in `/`.
    Ignored,
    /// A file a merge, rebase, cherry-pick, revert or stash could not combine on its own.
    Conflicted,
}

/// What each side of a conflict did to the file. "Us" is the branch the operation runs on and
/// "them" the changes it brings in, as git names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKind {
    /// Both sides changed the file.
    BothModified,
    /// Both sides added a file at this path.
    BothAdded,
    /// Both sides deleted the file, which they had renamed differently.
    BothDeleted,
    /// Only our side added the file.
    AddedByUs,
    /// Only their side added the file.
    AddedByThem,
    /// Our side deleted the file and theirs changed it.
    DeletedByUs,
    /// Our side changed the file and theirs deleted it.
    DeletedByThem,
}

impl ConflictKind {
    /// The kind from the `XY` field of a conflict record.
    fn from_xy(xy: &[u8]) -> Option<Self> {
        Some(match xy {
            b"UU" => Self::BothModified,
            b"AA" => Self::BothAdded,
            b"DD" => Self::BothDeleted,
            b"AU" => Self::AddedByUs,
            b"UA" => Self::AddedByThem,
            b"DU" => Self::DeletedByUs,
            b"UD" => Self::DeletedByThem,
            _ => return None,
        })
    }
}

/// The arguments after `git` that list the changes [`parse`] reads, with the ignored files when
/// `show_ignored` is set.
///
/// Every untracked file is listed on its own, not only the new folder that holds it, whatever
/// `status.showUntrackedFiles` says. Renames are always detected, whatever `status.renames` says:
/// the view shows every rename with its original path. Copies are detected only when
/// `status.renames` is `copies`, as git does: looking for them costs more, so it is the user's
/// choice.
/// A folder git ignores as a whole is listed once, not file by file: it can hold a whole build.
pub(crate) fn status_args(show_ignored: bool) -> Vec<&'static str> {
    let mut args = vec![
        "status",
        "--porcelain=v2",
        "-z",
        "--untracked-files=all",
        "--find-renames",
    ];
    if show_ignored {
        args.push("--ignored=matching");
    }
    args
}

/// Reads the output of `git` [`status_args`].
pub(crate) fn parse(output: &[u8]) -> Vec<Change> {
    let mut records = output.split(|byte| *byte == 0);
    let mut changes = Vec::new();
    while let Some(record) = records.next() {
        let change = match record.split_first() {
            // `1 XY sub mH mI mW hH hI path`: an ordinary change to a tracked file.
            Some((b'1', _)) => {
                let mut fields = record.splitn(9, |byte| *byte == b' ').skip(1);
                let status = fields.next().and_then(tracked_status);
                let sub = fields.next().unwrap_or_default();
                status
                    .zip(fields.nth(5))
                    .map(|(status, path)| Change::new(path, status).in_submodule(sub))
            }
            // `2 XY sub mH mI mW hH hI Xscore path`, then `origPath` as the next record: a
            // renamed or copied file. `X` is `R` or `C`.
            Some((b'2', _)) => {
                let original_path = records.next();
                let mut fields = record.splitn(10, |byte| *byte == b' ').skip(1);
                let xy = fields.next().unwrap_or_default();
                let sub = fields.next().unwrap_or_default();
                fields
                    .nth(6)
                    .zip(original_path)
                    .and_then(|(path, original_path)| moved(xy, path, original_path))
                    .map(|change| change.in_submodule(sub))
            }
            // `u XY sub m1 m2 m3 mW h1 h2 h3 path`: a conflicted file.
            Some((b'u', _)) => {
                let mut fields = record.splitn(11, |byte| *byte == b' ').skip(1);
                let conflict = fields.next().and_then(ConflictKind::from_xy);
                let sub = fields.next().unwrap_or_default();
                fields.nth(7).map(|path| Change {
                    conflict,
                    ..Change::new(path, ChangeStatus::Conflicted).in_submodule(sub)
                })
            }
            // `? path`
            Some((b'?', rest)) => rest
                .strip_prefix(b" ")
                .map(|path| Change::new(path, ChangeStatus::Untracked)),
            // `! path`
            Some((b'!', rest)) => rest
                .strip_prefix(b" ")
                .map(|path| Change::new(path, ChangeStatus::Ignored)),
            _ => None,
        };
        changes.extend(change);
    }
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    changes
}

/// The change of a file renamed or copied to `path` from `original_path`, from its `XY` field.
/// An edit after the move or copy keeps it a rename or a copy. None when the file is no change.
fn moved(xy: &[u8], path: &[u8], original_path: &[u8]) -> Option<Change> {
    let [index, worktree] = xy else {
        return None;
    };
    let status = match (index, worktree) {
        // Renamed and then removed from disk: the file is gone from where the last commit had it.
        (b'R', b'D') => return Some(Change::new(original_path, ChangeStatus::Deleted)),
        // Copied and then removed from disk: in neither the last commit nor the working tree,
        // like a file added and then deleted.
        (b'C', b'D') => return None,
        (b'C', _) => ChangeStatus::Copied,
        _ => ChangeStatus::Renamed,
    };
    Some(Change {
        original_path: Some(path_from_bytes(original_path)),
        ..Change::new(path, status)
    })
}

/// The status of a tracked file from its `XY` field: `X` compares the index with the last commit,
/// `Y` the working tree with the index. None when the file is no change.
fn tracked_status(xy: &[u8]) -> Option<ChangeStatus> {
    let [index, worktree] = xy else {
        return None;
    };
    Some(match (index, worktree) {
        // Added and then deleted: in neither the last commit nor the working tree. JetBrains
        // shows no change for it, and neither does gikli.
        (b'A', b'D') => return None,
        (b'A', _) => ChangeStatus::Added,
        (b'D', _) | (_, b'D') => ChangeStatus::Deleted,
        _ => ChangeStatus::Modified,
    })
}
