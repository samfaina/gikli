//! What the log tells of one commit: its details and the files it changed.

use serde::Serialize;

use crate::git::{GitRunner, path_from_bytes};
use crate::{
    Change, ChangeGroup, ChangeGrouping, ChangeStatus, Failure, RefKind, RefLabel, Repository,
    failure_of, git_failed_to_start,
};

/// The details of a commit, as the log shows them beside the commit list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitDetails {
    pub hash: String,
    /// The hashes of its parents, the first parent first.
    pub parents: Vec<String>,
    /// Its whole message, in parts, so the hashes of commits it names can lead to them.
    pub message: Vec<MessagePart>,
    pub author: String,
    pub author_email: String,
    /// When it was written, in seconds since the epoch.
    pub authored_at: i64,
    pub committer: String,
    pub committer_email: String,
    /// When it was committed, in seconds since the epoch.
    pub committed_at: i64,
    /// The local and remote branches that contain it, local ones first, each kind by name.
    pub branches: Vec<RefLabel>,
}

impl CommitDetails {
    /// The whole message as one text.
    pub fn message_text(&self) -> String {
        self.message.iter().map(|part| part.text.as_str()).collect()
    }
}

/// A piece of a commit message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePart {
    pub text: String,
    /// The full hash of the commit the text names, when it is the hash of one.
    pub commit: Option<String>,
}

/// The files a commit changed against one of its parents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitChanges {
    /// The hash of the parent, or None for a commit with no parent, whose files are all new.
    pub parent: Option<String>,
    pub group: ChangeGroup,
}

/// What git prints of the commit, a field per NUL.
const DETAILS_FORMAT: &str = "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%cn%x00%ce%x00%ct%x00%B";

/// The details of the commit `revision` names in `repository`.
pub(crate) fn read(
    runner: &GitRunner,
    repository: &Repository,
    revision: &str,
) -> Result<CommitDetails, Failure> {
    let hash = resolve(runner, repository, revision)?;
    let invocation = runner
        .run(
            &repository.root,
            &[
                "log",
                "-1",
                "--no-walk",
                "--encoding=UTF-8",
                DETAILS_FORMAT,
                "--end-of-options",
                &hash,
                "--",
            ],
        )
        .map_err(git_failed_to_start)?;
    if !invocation.success {
        return Err(not_found(revision));
    }
    let output = invocation.stdout_text();
    let fields: Vec<&str> = output.splitn(9, '\0').collect();
    let [
        hash,
        parents,
        author,
        author_email,
        authored_at,
        committer,
        committer_email,
        committed_at,
        message,
    ] = fields[..]
    else {
        return Err(not_found(revision));
    };
    Ok(CommitDetails {
        hash: hash.to_owned(),
        parents: parents.split_whitespace().map(str::to_owned).collect(),
        message: linked(runner, repository, message.trim_end()),
        author: author.to_owned(),
        author_email: author_email.to_owned(),
        authored_at: authored_at.parse().unwrap_or_default(),
        committer: committer.to_owned(),
        committer_email: committer_email.to_owned(),
        committed_at: committed_at.parse().unwrap_or_default(),
        branches: containing_branches(runner, repository, hash)?,
    })
}

/// The local and remote branches of `repository` that contain the commit `hash`, in the order
/// [`CommitDetails::branches`] has them.
fn containing_branches(
    runner: &GitRunner,
    repository: &Repository,
    hash: &str,
) -> Result<Vec<RefLabel>, Failure> {
    let listed = run_git(
        runner,
        repository,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(symref)",
            "--contains",
            hash,
            "refs/heads/",
            "refs/remotes/",
        ],
    )?;
    let mut branches: Vec<RefLabel> = String::from_utf8_lossy(&listed)
        .lines()
        .filter_map(|line| {
            let (name, symref) = line.split_once('\0')?;
            // A symbolic ref, such as `origin/HEAD`, only repeats the branch it leads to.
            symref.is_empty().then(|| RefLabel::of_ref(name)).flatten()
        })
        .filter(|label| matches!(label.kind, RefKind::Local | RefKind::Remote))
        .collect();
    branches.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    Ok(branches)
}

/// The files the commit `revision` names in `repository` changed against each of its parents,
/// arranged by `grouping`.
pub(crate) fn changes(
    runner: &GitRunner,
    repository: &Repository,
    revision: &str,
    grouping: ChangeGrouping,
) -> Result<Vec<CommitChanges>, Failure> {
    let hash = resolve(runner, repository, revision)?;
    let parents = parents_of(runner, repository, &hash)?;
    let against: Vec<Option<&str>> = if parents.is_empty() {
        vec![None]
    } else {
        parents.iter().map(|parent| Some(parent.as_str())).collect()
    };
    against
        .into_iter()
        .map(|parent| {
            let changes = changed_files(runner, repository, parent, &hash)?;
            Ok(CommitChanges {
                parent: parent.map(str::to_owned),
                group: ChangeGroup::new(changes, grouping),
            })
        })
        .collect()
}

/// The parents of the commit `hash`, the first parent first.
fn parents_of(
    runner: &GitRunner,
    repository: &Repository,
    hash: &str,
) -> Result<Vec<String>, Failure> {
    let listed = run_git(
        runner,
        repository,
        &[
            "rev-list",
            "--no-walk",
            "--parents",
            "--end-of-options",
            hash,
        ],
    )?;
    Ok(String::from_utf8_lossy(&listed)
        .split_whitespace()
        .skip(1)
        .map(str::to_owned)
        .collect())
}

/// The files that differ from `parent` to the commit `hash`, by path, with renames found. Without
/// a parent, every file of the commit is new.
fn changed_files(
    runner: &GitRunner,
    repository: &Repository,
    parent: Option<&str>,
    hash: &str,
) -> Result<Vec<Change>, Failure> {
    let mut args = vec![
        "diff-tree",
        "-r",
        "-z",
        "-M",
        "--name-status",
        "--no-commit-id",
    ];
    match parent {
        Some(parent) => args.extend(["--end-of-options", parent, hash]),
        None => args.extend(["--root", "--end-of-options", hash]),
    }
    let listed = run_git(runner, repository, &args)?;
    // Each file is its status, then its path, or for a rename or a copy the path it came from and
    // its new path.
    let mut fields = listed.split(|byte| *byte == 0);
    let mut changes = Vec::new();
    while let Some(status) = fields.next().filter(|status| !status.is_empty()) {
        let (status, moved) = match status[0] {
            b'A' => (ChangeStatus::Added, false),
            b'D' => (ChangeStatus::Deleted, false),
            b'R' => (ChangeStatus::Renamed, true),
            b'C' => (ChangeStatus::Copied, true),
            _ => (ChangeStatus::Modified, false),
        };
        let original_path = if moved {
            fields.next().map(path_from_bytes)
        } else {
            None
        };
        let Some(path) = fields.next() else {
            break;
        };
        changes.push(Change {
            path: path_from_bytes(path),
            status,
            original_path,
            conflict: None,
            submodule: None,
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

/// Runs git with `args` in `repository`, for what the log tells of a commit, and returns what it
/// printed.
fn run_git(runner: &GitRunner, repository: &Repository, args: &[&str]) -> Result<Vec<u8>, Failure> {
    let root = &repository.root;
    let invocation = runner.run(root, args).map_err(git_failed_to_start)?;
    if !invocation.success {
        let unreadable = Failure::LogUnreadable {
            folder: root.clone(),
        };
        return Err(failure_of(runner, root, &invocation, unreadable));
    }
    Ok(invocation.stdout)
}

/// The shortest and longest hashes a message can name a commit by: git abbreviates to 7 characters
/// at least, and a SHA-256 hash has 64.
const HASH_LENGTHS: std::ops::RangeInclusive<usize> = 7..=64;

/// The most hashes of one message gikli looks up, each with a git of its own.
const LINKED_HASHES: usize = 32;

/// `message` in parts, each word that is the hash of a commit in `repository`, whole or
/// abbreviated, a part of its own that leads to that commit.
fn linked(runner: &GitRunner, repository: &Repository, message: &str) -> Vec<MessagePart> {
    let mut parts = Vec::new();
    let mut text_start = 0;
    let mut looked_up: Vec<(&str, Option<String>)> = Vec::new();
    for (start, word) in words(message) {
        let is_hash =
            HASH_LENGTHS.contains(&word.len()) && word.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !is_hash {
            continue;
        }
        let commit = match looked_up.iter().find(|(seen, _)| *seen == word) {
            Some((_, commit)) => commit.clone(),
            None if looked_up.len() < LINKED_HASHES => {
                let commit = resolve(runner, repository, word)
                    .ok()
                    .filter(|hash| hash.starts_with(&word.to_ascii_lowercase()));
                looked_up.push((word, commit.clone()));
                commit
            }
            None => None,
        };
        let Some(commit) = commit else {
            continue;
        };
        if text_start < start {
            parts.push(MessagePart {
                text: message[text_start..start].to_owned(),
                commit: None,
            });
        }
        parts.push(MessagePart {
            text: word.to_owned(),
            commit: Some(commit),
        });
        text_start = start + word.len();
    }
    if text_start < message.len() {
        parts.push(MessagePart {
            text: message[text_start..].to_owned(),
            commit: None,
        });
    }
    parts
}

/// The words of `text`, runs of ASCII letters and digits, each with where it starts.
fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut rest = text.char_indices().peekable();
    std::iter::from_fn(move || {
        let (start, _) = rest
            .by_ref()
            .find(|(_, char)| char.is_ascii_alphanumeric())?;
        let mut end = start + 1;
        while let Some((at, _)) = rest.next_if(|(_, char)| char.is_ascii_alphanumeric()) {
            end = at + 1;
        }
        Some((start, &text[start..end]))
    })
}

/// The full hash of the commit `revision` names in `repository`: a hash, whole or in part, a
/// branch, a tag or any other revision git understands.
pub(crate) fn resolve(
    runner: &GitRunner,
    repository: &Repository,
    revision: &str,
) -> Result<String, Failure> {
    let wanted = format!("{}^{{commit}}", revision.trim());
    let invocation = runner
        .run(
            &repository.root,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &wanted,
            ],
        )
        .map_err(git_failed_to_start)?;
    let hash = invocation.stdout_text().trim_end().to_owned();
    if !invocation.success || hash.is_empty() {
        return Err(not_found(revision));
    }
    Ok(hash)
}

fn not_found(revision: &str) -> Failure {
    Failure::RevisionNotFound {
        revision: revision.trim().to_owned(),
    }
}
