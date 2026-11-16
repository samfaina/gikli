//! File history: the revisions of one file, followed through its renames, and getting the file
//! back as it was in one of them.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// The revisions in what `git log --follow --name-status -z` printed with [`FORMAT`]. Each commit
/// comes with the file on a line of its own: its status, then its path, or for a rename or a copy
/// the path it came from and its new path. `--follow` lists no merge commit, so every commit has it.
fn parse(listed: &[u8]) -> Vec<FileRevision> {
    let mut revisions = Vec::new();
    let mut fields = listed.split(|byte| *byte == 0).peekable();
    while let Some(first) = fields.next() {
        let Some(hash) = first.strip_prefix(b"") else {
            continue;
        };
        let mut header = || String::from_utf8_lossy(fields.next().unwrap_or_default()).into_owned();
        let (parents, author, committed_at, subject) = (header(), header(), header(), header());
        let Some(&letter) = fields
            .next_if(|field| !field.starts_with(b""))
            .and_then(|field| {
                field
                    .strip_prefix(
                        b"
",
                    )
                    .unwrap_or(field)
                    .first()
            })
        else {
            continue;
        };
        let (status, moved) = details::name_status(letter);
        let original_path = if moved {
            fields.next().map(path_from_bytes)
        } else {
            None
        };
        let Some(path) = fields.next().map(path_from_bytes) else {
            continue;
        };
        revisions.push(FileRevision {
            hash: String::from_utf8_lossy(hash).into_owned(),
            parents: parents.split_whitespace().map(str::to_owned).collect(),
            subject,
            author,
            committed_at: committed_at.parse().unwrap_or_default(),
            path,
            original_path,
            status,
        });
    }
    revisions
}

/// Brings the file at `path` in `repository` back to what the commit `revision` names had at
/// `revision_path`, on disk alone, or deletes it when the commit has no such file. git writes it
/// (ADR-0001), as a checkout of `path` would: with its mode, as a symbolic link if it is one, and
/// through the filters and line endings of `path`.
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
    // What git prints, or why it could not do what it was asked.
    let output = |runner: &GitRunner, args: &[&str]| -> Result<Vec<u8>, Failure> {
        let invocation = runner.run(root, args).map_err(git_failed_to_start)?;
        if invocation.success {
            Ok(invocation.stdout)
        } else {
            Err(failure_of(runner, root, &invocation, not_restored()))
        }
    };
    let run = |runner: &GitRunner, args: &[&str]| output(runner, args).map(drop);
    let listed = output(
        runner,
        &[
            "--literal-pathspecs",
            "ls-tree",
            "-z",
            "--full-tree",
            "--end-of-options",
            &hash,
            "--",
            revision_path,
        ],
    )?;
    let Some((mode, id)) = entry_of(&listed, revision_path) else {
        // The commit has no such file. git deletes one it tracks as a restore from the commit, and
        // one it does not track as a clean of that one path.
        let tracked = !output(
            runner,
            &["--literal-pathspecs", "ls-files", "-z", "--", path],
        )?
        .is_empty();
        let source = format!("--source={hash}");
        return if tracked {
            run(
                runner,
                &[
                    "--literal-pathspecs",
                    "restore",
                    &source,
                    "--worktree",
                    "--",
                    path,
                ],
            )
        } else {
            run(
                runner,
                &[
                    "--literal-pathspecs",
                    "clean",
                    "--force",
                    "--quiet",
                    "-x",
                    "--",
                    path,
                ],
            )
        };
    };
    // An index of its own holds the version at `path`, which it may not have had in the commit,
    // so the user's index stays as it is, and git checks the file out from there.
    let git_dir = runner
        .relative_git_dir(root)
        .map_err(git_failed_to_start)?
        .ok_or_else(not_restored)?;
    let index = format!(
        "{git_dir}/gikli-get-version-{}-{}.index",
        std::process::id(),
        NEXT_INDEX.fetch_add(1, Ordering::Relaxed)
    );
    let own_index = runner.with_index_file(&index);
    let checked_out = run(
        &own_index,
        &["update-index", "--add", "--cacheinfo", &mode, &id, path],
    )
    .and_then(|()| run(&own_index, &["checkout-index", "--force", "--", path]));
    // The index was only for this, and git leaves no lock behind once it ends.
    let _ = std::fs::remove_file(root.join(&index));
    checked_out
}

/// Tells apart the indexes of Get a file version that run at once.
static NEXT_INDEX: AtomicU64 = AtomicU64::new(0);

/// The mode and object of the file at `path` in what `git ls-tree -z` `listed`, if it is there.
fn entry_of(listed: &[u8], path: &str) -> Option<(String, String)> {
    // `mode type object`, a tab, and the path. A folder with the name of the file matches too.
    let entry = listed.split(|byte| *byte == 0).find_map(|line| {
        let tab = line.iter().position(|byte| *byte == b'\t')?;
        (&line[tab + 1..] == path.as_bytes()).then_some(&line[..tab])
    });
    let mut fields = entry
        .unwrap_or_default()
        .split(|byte| *byte == b' ')
        .map(|field| String::from_utf8_lossy(field).into_owned());
    match (fields.next(), fields.next(), fields.next()) {
        (Some(mode), Some(kind), Some(id)) if kind != "tree" => Some((mode, id)),
        _ => None,
    }
}
