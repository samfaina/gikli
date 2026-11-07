//! The status reader: turns git's porcelain status into the changes of a Repository.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::changelists::Changelists;
use crate::git::path_from_bytes;

/// The uncommitted changes of one Repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeSet {
    pub repository: Repository,
    /// One entry per changed file, ordered by path.
    pub changes: Vec<Change>,
    /// The Changelists of the Repository, each with its changes. Exactly one is active.
    pub changelists: Vec<Changelist>,
    /// The new files git does not track yet.
    pub unversioned_files: ChangeGroup,
    /// The files git ignores, listed only when the user turns ignored files on.
    pub ignored_files: ChangeGroup,
}

/// A named group of uncommitted changes that belongs to gikli and that git does not know about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changelist {
    /// What identifies the Changelist, whatever its name.
    pub id: String,
    pub name: String,
    /// Whether this is the Active changelist, the one new changes go into.
    pub active: bool,
    pub group: ChangeGroup,
}

/// Changes that the changes view shows together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeGroup {
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

impl ChangeGroup {
    /// The `changes`, with their rows arranged by `grouping`.
    fn new(changes: Vec<Change>, grouping: ChangeGrouping) -> Self {
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
        Self { changes, nodes }
    }
}

impl ChangeSet {
    /// The `changes` of `repository`, sorted into `lists` and arranged by `grouping`.
    pub(crate) fn new(
        repository: Repository,
        changes: Vec<Change>,
        lists: &Changelists,
        grouping: ChangeGrouping,
    ) -> Self {
        let mut in_lists = vec![Vec::new(); lists.lists().len()];
        let mut unversioned = Vec::new();
        let mut ignored = Vec::new();
        for change in &changes {
            if change.status.is_in_changelists() {
                in_lists[lists.place_of(&change.path)].push(change.clone());
            } else if change.status == ChangeStatus::Untracked {
                unversioned.push(change.clone());
            } else {
                ignored.push(change.clone());
            }
        }
        let changelists = lists
            .lists()
            .iter()
            .zip(in_lists)
            .map(|(list, changes)| Changelist {
                id: list.id.clone(),
                name: list.name.clone(),
                active: list.id == lists.active(),
                group: ChangeGroup::new(changes, grouping),
            })
            .collect();
        Self {
            repository,
            changes,
            changelists,
            unversioned_files: ChangeGroup::new(unversioned, grouping),
            ignored_files: ChangeGroup::new(ignored, grouping),
        }
    }

    /// The path the file at `path` was moved or copied from, if it was.
    pub(crate) fn original_path_of(&self, path: &Path) -> Option<PathBuf> {
        self.changes
            .iter()
            .find(|change| change.path == path)
            .and_then(|change| change.original_path.clone())
    }

    /// The files before and after `path` in the changes view when arranged by `grouping`, leaving
    /// out ignored files. A file that is not a change takes the place it would have as a change in
    /// the Active changelist, as the user can still be looking at it when it stops being one.
    pub(crate) fn neighbors_of(
        &self,
        path: &Path,
        grouping: ChangeGrouping,
    ) -> (Option<PathBuf>, Option<PathBuf>) {
        let mut groups: Vec<Vec<Change>> = self
            .changelists
            .iter()
            .map(|list| list.group.changes.clone())
            .collect();
        groups.push(self.unversioned_files.changes.clone());
        let is_a_change = groups.iter().flatten().any(|change| change.path == path);
        let active = self.changelists.iter().position(|list| list.active);
        if !is_a_change && let Some(changes) = active.map(|active| &mut groups[active]) {
            changes.push(Change {
                path: path.to_path_buf(),
                status: ChangeStatus::Modified,
                original_path: None,
                conflict: None,
                submodule: None,
            });
            changes.sort_by(|left, right| left.path.cmp(&right.path));
        }
        let mut files = Vec::new();
        for changes in groups {
            for node in ChangeGroup::new(changes, grouping).nodes {
                node.collect_files(&mut files);
            }
        }
        let Some(place) = files.iter().position(|file| file == path) else {
            return (None, None);
        };
        let before = place.checked_sub(1).and_then(|before| files.get(before));
        (before.cloned(), files.get(place + 1).cloned())
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
    fn collect_files(&self, files: &mut Vec<PathBuf>) {
        match self {
            Self::Folder { children, .. } => {
                for child in children {
                    child.collect_files(files);
                }
            }
            Self::File(change) => files.push(change.path.clone()),
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

impl ChangeStatus {
    /// Whether a file with this status belongs to a Changelist. New files are in Unversioned Files
    /// until they are added, and ignored files are no changes.
    pub(crate) fn is_in_changelists(self) -> bool {
        !matches!(self, Self::Untracked | Self::Ignored)
    }
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
        // For the commit HEAD is on. Counting commits against the upstream can take long.
        "--branch",
        "--no-ahead-behind",
    ];
    if show_ignored {
        args.push("--ignored=matching");
    }
    args
}

/// What git reports about the working tree of a Repository, in one read.
pub(crate) struct Status {
    pub changes: Vec<Change>,
    /// The commit HEAD is on, or None when it is on no commit yet.
    pub head: Option<String>,
}

impl Status {
    /// Reads the output of `git` [`status_args`].
    pub fn parse(output: &[u8]) -> Self {
        Self {
            changes: parse_changes(output),
            head: parse_head(output),
        }
    }
}

fn parse_head(output: &[u8]) -> Option<String> {
    output
        .split(|byte| *byte == 0)
        .find_map(|record| record.strip_prefix(b"# branch.oid "))
        .filter(|oid| *oid != b"(initial)")
        .map(|oid| String::from_utf8_lossy(oid).into_owned())
}

fn parse_changes(output: &[u8]) -> Vec<Change> {
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
