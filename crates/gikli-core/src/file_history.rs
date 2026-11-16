//! File history: the revisions of one file, followed through its renames, and getting the file
//! back as it was in one of them.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::git::{GitRunner, path_from_bytes};
use crate::log::run_git;
use crate::{
    ChangeStatus, Failure, RefLabel, Repository, details, failure_of, git_failed_to_start,
};

/// The history of one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHistory {
    pub repository: Repository,
    /// The file's path relative to the Repository root, with `/` between folders, as the user
    /// asked for its history.
    pub path: PathBuf,
    /// The commits that changed the file, newest first.
    pub revisions: Vec<FileRevision>,
    /// Whether the file has older revisions than those listed, which the row limit left out.
    pub more: bool,
}

/// A commit that changed the file, and what it did to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRevision {
    /// The full hash of the commit.
    pub hash: String,
    /// The hashes of its parents, the first parent first.
    pub parents: Vec<String>,
    pub subject: String,
    pub author: String,
    /// When it was committed, in seconds since the epoch.
    pub committed_at: i64,
    /// The path the file has in this commit, which differs from [`FileHistory::path`] before a
    /// rename.
    pub path: PathBuf,
    /// The path the file had before this commit, for a commit that renamed or copied it.
    pub original_path: Option<PathBuf>,
    /// What the commit did to the file.
    pub status: ChangeStatus,
}

/// What git prints of each commit, a field per NUL, after a byte that marks where it starts. git
/// ends it with a NUL of its own before the files of the commit.
const FORMAT: &str = "--format=%x01%H%x00%P%x00%an%x00%ct%x00%s";

/// The history of the file at `path` in `repository`, from the tips of `branches`, or from HEAD
/// when there are none, with at most `limit` revisions. A branch that is gone, or an unborn HEAD,
/// has no history to read.
pub(crate) fn read(
    runner: &GitRunner,
    repository: &Repository,
    path: &str,
    branches: &[RefLabel],
    limit: usize,
) -> Result<FileHistory, Failure> {
    let tips = tips(runner, repository, branches)?;
    // One revision past the limit tells whether there are more.
    let count = format!("--max-count={}", limit.saturating_add(1));
    let mut revisions = if tips.is_empty() {
        Vec::new()
    } else {
        let mut args = vec![
            "--literal-pathspecs",
            "log",
            "--follow",
            "--name-status",
            "-z",
            "--encoding=UTF-8",
            FORMAT,
            &count,
            "--end-of-options",
        ];
        args.extend(tips.iter().map(String::as_str));
        args.extend(["--", path]);
        parse(&run_git(runner, repository, &args)?.stdout)
    };
    let more = revisions.len() > limit;
    revisions.truncate(limit);
    Ok(FileHistory {
        repository: repository.clone(),
        path: PathBuf::from(path),
        revisions,
        more,
    })
}

/// The commits the history of `branches`, or of HEAD when there are none, starts from: those of
/// the ones that exist.
fn tips(
    runner: &GitRunner,
    repository: &Repository,
    branches: &[RefLabel],
) -> Result<Vec<String>, Failure> {
    let names: Vec<String> = if branches.is_empty() {
        vec!["HEAD".to_owned()]
    } else {
        branches.iter().map(RefLabel::full_name).collect()
    };
    let mut tips = Vec::new();
    for name in names {
        match details::resolve(runner, repository, &name) {
            Ok(hash) => tips.push(hash),
            Err(Failure::RevisionNotFound { .. }) => {}
            Err(failure) => return Err(failure),
        }
    }
    Ok(tips)
}

/// The revisions in what `git log --name-status -z` printed with [`FORMAT`].
fn parse(listed: &[u8]) -> Vec<FileRevision> {
    let mut revisions = Vec::new();
    let mut fields = listed.split(|byte| *byte == 0).peekable();
    while let Some(first) = fields.next() {
        let Some(hash) = first.strip_prefix(b"\x01") else {
            continue;
        };
        let mut header = || String::from_utf8_lossy(fields.next().unwrap_or_default()).into_owned();
        let (parents, author, committed_at, subject) = (header(), header(), header(), header());
        // The files of the commit follow on a line of their own: its status, then its path, or for
        // a rename or a copy the path it came from and its new path. A merge may list none.
        let status_field = fields
            .next_if(|field| !field.starts_with(b"\x01"))
            .map(|field| field.strip_prefix(b"\n").unwrap_or(field).to_vec());
        let (status, moved) = match status_field.as_deref().and_then(|status| status.first()) {
            Some(b'A') => (ChangeStatus::Added, false),
            Some(b'D') => (ChangeStatus::Deleted, false),
            Some(b'R') => (ChangeStatus::Renamed, true),
            Some(b'C') => (ChangeStatus::Copied, true),
            _ => (ChangeStatus::Modified, false),
        };
        let original_path = if moved {
            fields.next().map(path_from_bytes)
        } else {
            None
        };
        let path = if status_field.is_some() {
            fields.next().map(path_from_bytes)
        } else {
            None
        };
        revisions.push(FileRevision {
            hash: String::from_utf8_lossy(hash).into_owned(),
            parents: parents.split_whitespace().map(str::to_owned).collect(),
            subject,
            author,
            committed_at: committed_at.parse().unwrap_or_default(),
            path: path.unwrap_or_default(),
            original_path,
            status,
        });
    }
    revisions
}

/// Brings the file at `path` in `repository` back to what the commit `revision` names had at
/// `revision_path`, on disk alone, or deletes it when the commit has no such file.
pub(crate) fn get_version(
    runner: &GitRunner,
    repository: &Repository,
    revision: &str,
    revision_path: &str,
    path: &str,
) -> Result<(), Failure> {
    let hash = details::resolve(runner, repository, revision)?;
    let root = &repository.root;
    let not_restored = || Failure::FileVersionNotRestored {
        path: PathBuf::from(path),
    };
    let inside_root = |path: &str| {
        Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    };
    if !inside_root(path) || !inside_root(revision_path) {
        return Err(not_restored());
    }
    let run = |args: &[&str]| runner.run(root, args).map_err(git_failed_to_start);
    if revision_path == path {
        // git writes the file as a checkout would, with its mode and the user's filters, and
        // deletes it when the commit has none.
        let source = format!("--source={hash}");
        let restored = run(&[
            "--literal-pathspecs",
            "restore",
            &source,
            "--worktree",
            "--",
            path,
        ])?;
        if !restored.success {
            return Err(failure_of(runner, root, &restored, not_restored()));
        }
        return Ok(());
    }
    // The file had another path in the commit, which `restore` would write to. git converts what
    // the commit holds as for a file at `path`.
    let blob = format!("{hash}:{revision_path}");
    let file = root.join(path);
    let not_saved = |error: std::io::Error| Failure::LocalFileNotSaved {
        path: PathBuf::from(path),
        message: error.to_string(),
    };
    if !run(&["cat-file", "-e", &blob])?.success {
        return match std::fs::remove_file(&file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(not_saved(error)),
            _ => Ok(()),
        };
    }
    let as_path = format!("--path={path}");
    let version = run(&["cat-file", "--filters", &as_path, &blob])?;
    if !version.success {
        return Err(failure_of(runner, root, &version, not_restored()));
    }
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(not_saved)?;
    }
    std::fs::write(&file, &version.stdout).map_err(not_saved)
}
