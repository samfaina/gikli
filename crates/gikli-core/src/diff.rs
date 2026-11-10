//! The diff model: a changed file against HEAD, as the side-by-side view shows it.
//!
//! git compares the two versions (ADR-0001), so what git takes for a change, and only that, is a
//! difference: a line ending `core.autocrlf` converts is none. The texts have `\n` between lines
//! whatever the file holds, so the view can count lines as git does.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::git::{GitRunner, Invocation};
use crate::{
    ChangeSet, Failure, Repository, Settings, StagingSide, failure_of, git_failed_to_start,
};

/// A changed file, compared with its version in HEAD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub repository: Repository,
    /// The file's path relative to the Repository root, with `/` between folders.
    pub path: PathBuf,
    /// The path in HEAD the file was moved or copied from, for a renamed or copied file: the head
    /// side is the file at this path.
    pub original_path: Option<PathBuf>,
    /// Which comparison of Staging mode this is, or None when it compares HEAD with the file on
    /// disk. On [`StagingSide::Staged`] the head side is HEAD and the local side the index, and on
    /// [`StagingSide::Unstaged`] the head side is the index and the local side the file on disk.
    pub side: Option<StagingSide>,
    /// The file before this one in the changes view, as the user arranged it.
    pub previous_file: Option<PathBuf>,
    /// The file after this one in the changes view, as the user arranged it.
    pub next_file: Option<PathBuf>,
    /// The file the review moves on to from the last difference of this one, when it moves on.
    pub after_last_difference: Option<PathBuf>,
    pub content: DiffContent,
}

/// What the view can show of a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DiffContent {
    Text(TextDiff),
    /// A binary file, which the view shows without a text diff.
    Binary,
    /// A submodule or a nested Repository, which the view shows without a text diff: git compares
    /// those by commit.
    Folder,
}

/// The two versions of a text file and where they differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDiff {
    /// The file in HEAD, or None when HEAD does not have it.
    pub head: Option<String>,
    /// The file on disk, or None when there is none.
    pub local: Option<String>,
    /// Identifies what is on disk, for saving over it: see [`crate::Gikli::save_local_file`].
    pub local_version: String,
    /// Why the local side cannot be edited, when it cannot.
    pub local_read_only: Option<LocalReadOnly>,
    /// Where the two sides differ, from the top of the file down.
    pub differences: Vec<Difference>,
    /// The unchanged lines the view hides, with [`CONTEXT_LINES`] lines left around each
    /// difference, from the top of the file down. Nothing is hidden without differences.
    pub collapsed: Vec<UnchangedRegion>,
}

/// Why the local side of a diff cannot be edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalReadOnly {
    /// The user turned editing off in the settings.
    TurnedOff,
    /// There is no file on disk.
    NoLocalFile,
    /// The file is not UTF-8, so what the view shows of it is not what it holds.
    NotUtf8,
    /// The file is a symbolic link, and the view shows where it points.
    SymbolicLink,
    /// This side is the git index, and a diff writes only to the file on disk.
    Index,
}

/// Unchanged lines, the same on both sides, and in the same number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnchangedRegion {
    pub head: LineRange,
    pub local: LineRange,
}

/// How many unchanged lines the view shows next to a difference.
const CONTEXT_LINES: usize = 4;

/// Hiding fewer lines than this would save nothing: the marker that says so takes a line too.
const FEWEST_COLLAPSED_LINES: usize = 2;

/// Lines that differ between the two sides. A side without lines here shows where the lines of
/// the other side would go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Difference {
    pub head: LineRange,
    pub local: LineRange,
}

/// The lines from `start` up to, and not including, `end`, counted from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRange {
    pub start: usize,
    pub end: usize,
}

/// Reads the diff of the file at `path` in `changes`, the changes of its Repository as they are
/// now, which tell where a moved file came from.
pub(crate) fn read(
    runner: &GitRunner,
    changes: &ChangeSet,
    path: &str,
    settings: &Settings,
) -> Result<FileDiff, Failure> {
    let original_path = changes.original_path_of(Path::new(path));
    let head_path = head_path_of(path, original_path.as_deref());
    let file = ComparedFile::new(runner, changes, path, &head_path);
    let content = file.working_tree_against_head(settings)?;
    let neighbors = changes.neighbors_of(Path::new(path), settings.change_grouping);
    Ok(file_diff(
        changes,
        path,
        original_path,
        None,
        content,
        neighbors,
        settings,
    ))
}

/// Reads the diff of the file at `path` in `changes` on `side` of the index. A conflicted file is
/// compared with HEAD, as the index holds no one version of it to compare with.
pub(crate) fn read_side(
    runner: &GitRunner,
    changes: &ChangeSet,
    path: &str,
    side: StagingSide,
    settings: &Settings,
) -> Result<FileDiff, Failure> {
    let neighbors = changes.neighbors_on_side(Path::new(path), side, settings.change_grouping);
    if side == StagingSide::Unstaged && changes.is_conflicted(Path::new(path)) {
        let file = ComparedFile::new(runner, changes, path, path);
        let content = file.working_tree_against_head(settings)?;
        return Ok(file_diff(
            changes, path, None, None, content, neighbors, settings,
        ));
    }
    let original_path = match side {
        StagingSide::Staged => changes.staged_original_path_of(Path::new(path)),
        StagingSide::Unstaged => None,
    };
    let head_path = head_path_of(path, original_path.as_deref());
    let file = ComparedFile::new(runner, changes, path, &head_path);
    let content = match side {
        StagingSide::Staged => file.index_against_head()?,
        StagingSide::Unstaged => file.working_tree_against_index(settings)?,
    };
    Ok(file_diff(
        changes,
        path,
        original_path,
        Some(side),
        content,
        neighbors,
        settings,
    ))
}

/// Where HEAD has the file at `path`, which was moved from `original_path` if it was.
fn head_path_of(path: &str, original_path: Option<&Path>) -> String {
    match original_path {
        Some(original) => original.to_string_lossy().into_owned(),
        None => path.to_owned(),
    }
}

fn file_diff(
    changes: &ChangeSet,
    path: &str,
    original_path: Option<PathBuf>,
    side: Option<StagingSide>,
    content: DiffContent,
    (previous_file, next_file): (Option<PathBuf>, Option<PathBuf>),
    settings: &Settings,
) -> FileDiff {
    FileDiff {
        repository: changes.repository.clone(),
        path: PathBuf::from(path),
        original_path,
        side,
        after_last_difference: next_file
            .clone()
            .filter(|_| settings.go_to_next_file_after_last_change),
        previous_file,
        next_file,
        content,
    }
}

/// Writes `text`, which has `\n` between lines, as the file at `path` in `root`, when what is there
/// is still what `version` identifies. The lines keep the line endings they have in the file.
pub(crate) fn save(
    root: &Path,
    path: &str,
    text: &str,
    version: &str,
    editable: bool,
) -> Result<(), Failure> {
    let as_path = || PathBuf::from(path);
    let inside_root = Path::new(path)
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    if !inside_root {
        return Err(Failure::LocalSideNotEditable { path: as_path() });
    }
    let local = LocalFile::read(root, path)?;
    if local.version() != version {
        return Err(Failure::LocalFileChanged { path: as_path() });
    }
    let bytes = match (&local, local.read_only(editable)) {
        (LocalFile::File(bytes), None) if !bytes.contains(&0) => bytes,
        _ => return Err(Failure::LocalSideNotEditable { path: as_path() }),
    };
    std::fs::write(root.join(path), with_line_endings_of(bytes, text)).map_err(|error| {
        Failure::LocalFileNotSaved {
            path: as_path(),
            message: error.to_string(),
        }
    })
}

/// `text`, with `\n` between lines, with the line endings of `file`: the unchanged lines at its top
/// and bottom keep theirs, and the lines between get the one most of the file has.
fn with_line_endings_of(file: &[u8], text: &str) -> Vec<u8> {
    let old: Vec<&[u8]> = file.split(|byte| *byte == b'\n').collect();
    let new: Vec<&[u8]> = text.as_bytes().split(|byte| *byte == b'\n').collect();
    // What follows the last line ending has no ending of its own.
    let old_lines = &old[..old.len() - 1];
    let is_crlf = |line: &[u8]| line.ends_with(b"\r");
    let crlf_lines = old_lines.iter().filter(|line| is_crlf(line)).count();
    let most_are_crlf = crlf_lines * 2 > old_lines.len();
    let same_line =
        |(old, new): &(&&[u8], &&[u8])| old == new || old.strip_suffix(b"\r") == Some(**new);
    let same_at_top = old.iter().zip(&new).take_while(same_line).count();
    let same_at_bottom = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(old.len().min(new.len()) - same_at_top)
        .take_while(same_line)
        .count();
    let mut written = Vec::with_capacity(text.len());
    for (index, line) in new.iter().enumerate() {
        written.extend_from_slice(line);
        let from_bottom = new.len() - 1 - index;
        if from_bottom == 0 {
            break;
        }
        let same_line_in_file = if index < same_at_top {
            Some(index)
        } else if from_bottom < same_at_bottom {
            Some(old.len() - 1 - from_bottom)
        } else {
            None
        };
        let crlf = same_line_in_file
            .and_then(|place| old_lines.get(place))
            .map_or(most_are_crlf, |line| is_crlf(line));
        written.extend_from_slice(if crlf { b"\r\n" } else { b"\n" });
    }
    written
}

/// The file at a path on disk, as git sees it.
enum LocalFile {
    Missing,
    Folder,
    /// The bytes of a file.
    File(Vec<u8>),
    /// The target of a symbolic link, which is what git compares.
    Link(Vec<u8>),
}

impl LocalFile {
    fn read(root: &Path, path: &str) -> Result<Self, Failure> {
        let full_path = root.join(path);
        let read = || -> std::io::Result<Self> {
            let file_type = std::fs::symlink_metadata(&full_path)?.file_type();
            Ok(if file_type.is_dir() {
                Self::Folder
            } else if file_type.is_symlink() {
                let target = std::fs::read_link(&full_path)?;
                Self::Link(target.to_string_lossy().into_owned().into_bytes())
            } else {
                Self::File(std::fs::read(&full_path)?)
            })
        };
        match read() {
            Ok(local) => Ok(local),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::Missing),
            Err(error) => Err(Failure::LocalFileUnreadable {
                path: PathBuf::from(path),
                message: error.to_string(),
            }),
        }
    }

    fn is_folder(&self) -> bool {
        matches!(self, Self::Folder)
    }

    /// Why the user cannot edit this file, when `editable` is what the settings say.
    fn read_only(&self, editable: bool) -> Option<LocalReadOnly> {
        match self {
            Self::Missing | Self::Folder => Some(LocalReadOnly::NoLocalFile),
            Self::Link(_) => Some(LocalReadOnly::SymbolicLink),
            Self::File(bytes) if std::str::from_utf8(bytes).is_err() => {
                Some(LocalReadOnly::NotUtf8)
            }
            Self::File(_) if !editable => Some(LocalReadOnly::TurnedOff),
            Self::File(_) => None,
        }
    }

    /// Identifies what is on disk, so that a save can tell it is still what the user saw. It only
    /// has to last while gikli runs.
    fn version(&self) -> String {
        let mut hasher = DefaultHasher::new();
        match self {
            Self::Missing | Self::Folder => "missing".hash(&mut hasher),
            Self::File(bytes) => bytes.hash(&mut hasher),
            Self::Link(target) => ("link", target).hash(&mut hasher),
        }
        format!("{:016x}", hasher.finish())
    }

    /// What git compares, when there is something to compare.
    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::File(bytes) | Self::Link(bytes) => Some(bytes),
            Self::Missing | Self::Folder => None,
        }
    }
}

/// The arguments after `git` that compare two versions of one file. git's settings for how a diff
/// looks, or for a program to make it, such as `diff.context` or `diff.external`, have no say.
/// `diff.algorithm` does: it only changes which lines git pairs, and that is the user's to choose.
const DIFF_ARGS: &[&str] = &[
    "--literal-pathspecs",
    "diff",
    "--numstat",
    "--patch",
    "--unified=0",
    "--inter-hunk-context=0",
    "--no-ext-diff",
    "--no-textconv",
    "--no-renames",
];

/// What git says of two versions of a file.
struct Comparison {
    binary: bool,
    differences: Vec<Difference>,
}

impl Comparison {
    /// What there is to say of a file that is on neither side.
    fn of_nothing() -> Self {
        Self {
            binary: false,
            differences: Vec::new(),
        }
    }
}

/// A file to compare: its place on disk and in the index, and its place in HEAD, which differs for
/// a moved file.
struct ComparedFile<'a> {
    runner: &'a GitRunner,
    root: &'a Path,
    path: &'a str,
    head_path: &'a str,
}

impl<'a> ComparedFile<'a> {
    fn new(
        runner: &'a GitRunner,
        changes: &'a ChangeSet,
        path: &'a str,
        head_path: &'a str,
    ) -> Self {
        Self {
            runner,
            root: &changes.repository.root,
            path,
            head_path,
        }
    }

    /// What the view shows of the file on disk against HEAD.
    fn working_tree_against_head(&self, settings: &Settings) -> Result<DiffContent, Failure> {
        let head = self.head_entry()?;
        let local = LocalFile::read(self.root, self.path)?;
        if local.is_folder() || head.as_ref().is_some_and(StoredEntry::is_folder) {
            return Ok(DiffContent::Folder);
        }
        let comparison = self.compare(head.is_some(), local.bytes().is_some())?;
        self.content(
            &comparison,
            head.as_ref(),
            local.bytes(),
            local.version(),
            local.read_only(settings.diff_local_side_editable),
        )
    }

    /// What the view shows of the index against HEAD. The index is read-only here.
    fn index_against_head(&self) -> Result<DiffContent, Failure> {
        let head = self.head_entry()?;
        let index = self.index_entry()?;
        if [&head, &index]
            .into_iter()
            .flatten()
            .any(StoredEntry::is_folder)
        {
            return Ok(DiffContent::Folder);
        }
        let comparison = self.compare_staged(head.is_some(), index.is_some())?;
        let index_bytes = match &index {
            Some(entry) => Some(self.read_blob(&entry.id)?),
            None => None,
        };
        self.content(
            &comparison,
            head.as_ref(),
            index_bytes.as_deref(),
            String::new(),
            Some(LocalReadOnly::Index),
        )
    }

    /// What the view shows of the file on disk against the index.
    fn working_tree_against_index(&self, settings: &Settings) -> Result<DiffContent, Failure> {
        let index = self.index_entry()?;
        let local = LocalFile::read(self.root, self.path)?;
        if local.is_folder() || index.as_ref().is_some_and(StoredEntry::is_folder) {
            return Ok(DiffContent::Folder);
        }
        let comparison = self.compare_unstaged(index.is_some(), local.bytes().is_some())?;
        self.content(
            &comparison,
            index.as_ref(),
            local.bytes(),
            local.version(),
            local.read_only(settings.diff_local_side_editable),
        )
    }

    /// What the view shows of this file, from what `comparison` says, with `older` what the older
    /// side has of it, `newer` what the newer side holds, `local_version` what identifies that, and
    /// `local_read_only` why it cannot be edited.
    fn content(
        &self,
        comparison: &Comparison,
        older: Option<&StoredEntry>,
        newer: Option<&[u8]>,
        local_version: String,
        local_read_only: Option<LocalReadOnly>,
    ) -> Result<DiffContent, Failure> {
        if comparison.binary {
            return Ok(DiffContent::Binary);
        }
        let older_bytes = match older {
            Some(entry) => Some(self.read_blob(&entry.id)?),
            None => None,
        };
        if [older_bytes.as_deref(), newer]
            .into_iter()
            .flatten()
            .any(|bytes| bytes.contains(&0))
        {
            return Ok(DiffContent::Binary);
        }
        let head = older_bytes.as_deref().map(shown_text);
        let local = newer.map(shown_text);
        let collapsed = collapsed_regions(
            &comparison.differences,
            line_count(head.as_deref()),
            line_count(local.as_deref()),
        );
        Ok(DiffContent::Text(TextDiff {
            head,
            local,
            local_version,
            local_read_only,
            differences: comparison.differences.clone(),
            collapsed,
        }))
    }

    /// Has git compare the file in HEAD, when `in_head`, with the one on disk, when `on_disk`.
    fn compare(&self, in_head: bool, on_disk: bool) -> Result<Comparison, Failure> {
        let head_blob = format!("HEAD:{}", self.head_path);
        let targets: &[&str] = match (in_head, on_disk) {
            // A blob against a file, as `HEAD -- path` would not find a moved file. git converts
            // the file as it does for any comparison with HEAD.
            (true, true) => &[&head_blob, "--", self.path],
            (true, false) => &["HEAD", "--", self.head_path],
            // HEAD has no such file, and may have nothing at all.
            (false, true) => &["--no-index", "--", "/dev/null", self.path],
            (false, false) => return Ok(Comparison::of_nothing()),
        };
        self.compare_targets(targets)
    }

    /// Has git compare the file in HEAD, when `in_head`, with the one in the index, when `in_index`.
    fn compare_staged(&self, in_head: bool, in_index: bool) -> Result<Comparison, Failure> {
        if !in_head && !in_index {
            return Ok(Comparison::of_nothing());
        }
        let head_blob = format!("HEAD:{}", self.head_path);
        let index_blob = format!(":0:{}", self.path);
        let targets: &[&str] = if in_head && in_index && self.head_path != self.path {
            // Two blobs, as `--cached -- path` would not find a moved file.
            &[&head_blob, &index_blob]
        } else {
            // Without naming HEAD, which a Repository with no commits does not have.
            &["--cached", "--", self.path]
        };
        self.compare_targets(targets)
    }

    /// Has git compare the file in the index, when `in_index`, with the one on disk, when `on_disk`.
    fn compare_unstaged(&self, in_index: bool, on_disk: bool) -> Result<Comparison, Failure> {
        let targets: &[&str] = match (in_index, on_disk) {
            (true, _) => &["--", self.path],
            (false, true) => &["--no-index", "--", "/dev/null", self.path],
            (false, false) => return Ok(Comparison::of_nothing()),
        };
        self.compare_targets(targets)
    }

    fn compare_targets(&self, targets: &[&str]) -> Result<Comparison, Failure> {
        let mut args = DIFF_ARGS.to_vec();
        args.extend(targets);
        let invocation = self.run(&args)?;
        // With `--no-index`, git says the files differ by failing, and writes the difference.
        if !invocation.success && invocation.stdout.is_empty() {
            return Err(self.unreadable(&invocation));
        }
        Ok(Comparison {
            // The first line of `--numstat`, which counts no lines in a binary file.
            binary: invocation.stdout.starts_with(b"-\t-\t"),
            differences: parse_differences(&invocation.stdout),
        })
    }

    /// What the index has of this file, if anything.
    fn index_entry(&self) -> Result<Option<StoredEntry>, Failure> {
        let invocation = self.run(&[
            "--literal-pathspecs",
            "ls-files",
            "--stage",
            "-z",
            "--",
            self.path,
        ])?;
        if !invocation.success {
            return Err(self.unreadable(&invocation));
        }
        // `mode id stage`, a tab, and the path.
        let entry = invocation.stdout.split(|byte| *byte == b'\t').next();
        let mut fields = entry.unwrap_or_default().split(|byte| *byte == b' ');
        let (Some(mode), Some(id)) = (fields.next(), fields.next()) else {
            return Ok(None);
        };
        Ok(Some(StoredEntry {
            mode: mode.to_vec(),
            id: String::from_utf8_lossy(id).into_owned(),
        }))
    }

    /// What HEAD has of this file, if anything. A Repository with no commits has nothing.
    fn head_entry(&self) -> Result<Option<StoredEntry>, Failure> {
        if !self
            .run(&["rev-parse", "--verify", "--quiet", "HEAD"])?
            .success
        {
            return Ok(None);
        }
        let invocation = self.run(&[
            "--literal-pathspecs",
            "ls-tree",
            "-z",
            "--full-tree",
            "HEAD",
            "--",
            self.head_path,
        ])?;
        if !invocation.success {
            return Err(self.unreadable(&invocation));
        }
        // `mode type id`, a tab, and the path.
        let entry = invocation.stdout.split(|byte| *byte == b'\t').next();
        let mut fields = entry.unwrap_or_default().split(|byte| *byte == b' ');
        let (Some(mode), Some(id)) = (fields.next(), fields.nth(1)) else {
            return Ok(None);
        };
        Ok(Some(StoredEntry {
            mode: mode.to_vec(),
            id: String::from_utf8_lossy(id).into_owned(),
        }))
    }

    fn read_blob(&self, id: &str) -> Result<Vec<u8>, Failure> {
        let invocation = self.run(&["cat-file", "blob", id])?;
        if !invocation.success {
            return Err(self.unreadable(&invocation));
        }
        Ok(invocation.stdout)
    }

    fn run(&self, args: &[&str]) -> Result<Invocation, Failure> {
        self.runner
            .run(self.root, args)
            .map_err(git_failed_to_start)
    }

    fn unreadable(&self, invocation: &Invocation) -> Failure {
        failure_of(
            self.runner,
            self.root,
            invocation,
            Failure::DiffUnreadable {
                path: PathBuf::from(self.path),
            },
        )
    }
}

/// A file or folder that git stores, in HEAD or in the index.
struct StoredEntry {
    mode: Vec<u8>,
    id: String,
}

impl StoredEntry {
    /// Whether this is a submodule or a folder, not a file.
    fn is_folder(&self) -> bool {
        self.mode == b"160000" || self.mode == b"040000"
    }
}

/// `bytes` as the view shows them, with `\n` between lines.
fn shown_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

/// How many lines git counts in `text`: a last line without a line ending counts too.
fn line_count(text: Option<&str>) -> usize {
    text.map_or(0, |text| {
        let endings = text.bytes().filter(|byte| *byte == b'\n').count();
        endings + usize::from(!text.is_empty() && !text.ends_with('\n'))
    })
}

/// A line on each side, where the same unchanged lines start.
#[derive(Clone, Copy)]
struct SameLine {
    head: usize,
    local: usize,
}

/// What the view hides of the unchanged lines around `differences`, between sides of `head_lines`
/// and `local_lines` lines.
fn collapsed_regions(
    differences: &[Difference],
    head_lines: usize,
    local_lines: usize,
) -> Vec<UnchangedRegion> {
    if differences.is_empty() {
        return Vec::new();
    }
    let mut regions = Vec::new();
    let mut after_last = SameLine { head: 0, local: 0 };
    for (index, difference) in differences.iter().enumerate() {
        let context_above = if index == 0 { 0 } else { CONTEXT_LINES };
        let unchanged = difference.head.start.saturating_sub(after_last.head);
        let hidden = unchanged.saturating_sub(context_above + CONTEXT_LINES);
        regions.extend(region_of(after_last, context_above, hidden));
        after_last = SameLine {
            head: difference.head.end,
            local: difference.local.end,
        };
    }
    let unchanged = head_lines
        .saturating_sub(after_last.head)
        .min(local_lines.saturating_sub(after_last.local));
    regions.extend(region_of(
        after_last,
        CONTEXT_LINES,
        unchanged.saturating_sub(CONTEXT_LINES),
    ));
    regions
}

/// The `hidden` lines after the `context` lines from `start`, when there are enough of them to hide.
fn region_of(start: SameLine, context: usize, hidden: usize) -> Option<UnchangedRegion> {
    let lines_from = |start: usize| LineRange {
        start: start + context,
        end: start + context + hidden,
    };
    (hidden >= FEWEST_COLLAPSED_LINES).then(|| UnchangedRegion {
        head: lines_from(start.head),
        local: lines_from(start.local),
    })
}

/// The differences in the output of `git diff --unified=0`, from its hunk headers. Only a hunk
/// header starts with `@@`: every line of a hunk starts with a marker of its own. A file that
/// became or stopped being a symbolic link comes as a deletion and an addition, both from the top,
/// and that is one difference.
fn parse_differences(output: &[u8]) -> Vec<Difference> {
    let hunks = output
        .split(|byte| *byte == b'\n')
        .filter_map(|line| line.strip_prefix(b"@@ -"))
        .filter_map(parse_hunk_header);
    let mut differences: Vec<Difference> = Vec::new();
    for hunk in hunks {
        match differences.last_mut() {
            Some(last) if hunk.head.start < last.head.end || hunk.local.start < last.local.end => {
                last.head = last.head.joined_with(hunk.head);
                last.local = last.local.joined_with(hunk.local);
            }
            _ => differences.push(hunk),
        }
    }
    differences
}

impl LineRange {
    /// The lines from the first of either range to the last of either.
    fn joined_with(self, other: Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// The difference in `old[,count] +new[,count] @@`, the hunk header after its `@@ -`.
fn parse_hunk_header(header: &[u8]) -> Option<Difference> {
    let mut fields = header.split(|byte| *byte == b' ');
    let head = line_range(fields.next()?)?;
    let local = line_range(fields.next()?.strip_prefix(b"+")?)?;
    Some(Difference { head, local })
}

/// The lines of `start[,count]` in a hunk header, where lines count from 1, the count defaults to
/// 1, and without lines the start is the line before the place.
fn line_range(field: &[u8]) -> Option<LineRange> {
    let field = std::str::from_utf8(field).ok()?;
    let (start, count) = match field.split_once(',') {
        Some((start, count)) => (start.parse::<usize>().ok()?, count.parse::<usize>().ok()?),
        None => (field.parse::<usize>().ok()?, 1),
    };
    let start = if count == 0 {
        start
    } else {
        start.checked_sub(1)?
    };
    Some(LineRange {
        start,
        end: start + count,
    })
}
