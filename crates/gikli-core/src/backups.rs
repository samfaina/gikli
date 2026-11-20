//! The Backup engine: before a history rewrite, gikli saves the state of the branch as a ref in a
//! private namespace (ADR-0003), from which the rewrite can be undone.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::git::GitRunner;
use crate::{Failure, Repository, details, failure_of, git_failed_to_start, sync};

/// Where Backups live. Neither the log nor the branch lists show what is there, and a normal push
/// does not send it.
const NAMESPACE: &str = "refs/gikli/backups/";

/// How long a Backup is kept, in seconds: 30 days.
const KEPT_FOR: u64 = 30 * 24 * 60 * 60;

/// The state of a branch gikli saved before a history rewrite, so the rewrite can be undone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    /// The ref that holds it, under `refs/gikli/backups/`, as
    /// `refs/gikli/backups/<seconds since the epoch>/<branch>`, with `HEAD` for a detached HEAD.
    pub reference: String,
    /// The local branch it saved, or None for a detached HEAD.
    pub branch: Option<String>,
    /// The full hash of the commit the branch was on before the rewrite.
    pub commit: String,
}

/// A history rewrite gikli saved a Backup before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Rewrite {
    /// Undo commit took the last commit off the branch.
    UndoCommit,
    /// Amend replaced the last commit with another.
    Amend,
}

/// Saves `commit`, which `branch` is on, or a detached HEAD when there is no branch, as a Backup of
/// `repository`, and returns it. The Backups saved more than 30 days ago go.
pub(crate) fn save(
    runner: &GitRunner,
    repository: &Repository,
    branch: Option<&str>,
    commit: &str,
) -> Result<Backup, Failure> {
    let root = &repository.root;
    let backups = sync::read_branches(
        runner,
        repository,
        &["for-each-ref", "--format=%(refname)", NAMESPACE],
    )?;
    let backups: Vec<&str> = backups.lines().collect();
    let now = now();
    // One Backup a second: a rewrite in the same second as another takes the next second free,
    // so no name is taken twice, and none holds another.
    let saved_at = (now..)
        .find(|second| {
            !backups
                .iter()
                .any(|backup| saved_at(backup) == Some(*second))
        })
        .unwrap_or(now);
    let reference = format!("{NAMESPACE}{saved_at}/{}", branch.unwrap_or("HEAD"));
    // An empty old value makes git refuse to write over a Backup there.
    let saved = runner
        .run(
            root,
            &["update-ref", "-m", "gikli: backup", &reference, commit, ""],
        )
        .map_err(git_failed_to_start)?;
    if !saved.success {
        return Err(failure_of(
            runner,
            root,
            &saved,
            Failure::BackupNotSaved {
                folder: root.clone(),
            },
        ));
    }
    delete_expired(runner, repository, &backups, now);
    Ok(Backup {
        reference,
        branch: branch.map(str::to_owned),
        commit: commit.to_owned(),
    })
}

/// Deletes `backup`, saved for a rewrite that did not happen. The deletion is best-effort: when it
/// fails, the Backup stays until it expires, and the operation log says why.
pub(crate) fn forget(runner: &GitRunner, repository: &Repository, backup: &Backup) {
    let args = ["update-ref", "-d", &backup.reference, &backup.commit];
    let _ = runner.run(&repository.root, &args);
}

/// Deletes those of `backups`, the Backups of `repository`, saved more than 30 days before `now`. A
/// ref there whose name gikli did not write stays. The deletion is best-effort: one that fails
/// leaves the Backup for the next time, and the operation log says why, as it does not stand in
/// the way of the rewrite.
fn delete_expired(runner: &GitRunner, repository: &Repository, backups: &[&str], now: u64) {
    let expired = backups.iter().filter(|backup| {
        saved_at(backup).is_some_and(|saved_at| saved_at.saturating_add(KEPT_FOR) < now)
    });
    for backup in expired {
        let _ = runner.run(&repository.root, &["update-ref", "-d", backup]);
    }
}

/// When the Backup at `reference` was saved, in seconds since the epoch, as its name says.
fn saved_at(reference: &str) -> Option<u64> {
    let (seconds, _branch) = reference.strip_prefix(NAMESPACE)?.split_once('/')?;
    seconds.parse().ok()
}

/// Brings the branch `backup` saved back to the commit it holds, from `head`, the commit the
/// rewrite left it on, or no commit. What the rewrite changed in the files stays as it is,
/// uncommitted. When the branch is no longer on `head`, nothing changes and this returns
/// [`Failure::BranchMovedSinceRewrite`], as what was done since would go.
pub(crate) fn restore(
    runner: &GitRunner,
    repository: &Repository,
    backup: &Backup,
    head: Option<&str>,
) -> Result<(), Failure> {
    let moved = || Failure::BranchMovedSinceRewrite {
        branch: backup.branch.clone(),
    };
    let on_head = sync::head_branch(runner, repository)? == backup.branch;
    let reference = match &backup.branch {
        _ if on_head => "HEAD".to_owned(),
        Some(branch) => format!("refs/heads/{branch}"),
        // HEAD was detached, and is on a branch by now.
        None => return Err(moved()),
    };
    if commit_of(runner, repository, &reference)?.as_deref() != head {
        return Err(moved());
    }
    let root = &repository.root;
    let restored = if on_head {
        // Moves the branch with HEAD, and keeps the index and the files as they are.
        runner.run(
            root,
            &["reset", "--soft", "--end-of-options", &backup.commit],
        )
    } else {
        let message = "gikli: restore backup";
        let old_value = head.unwrap_or_default();
        let args = [
            "update-ref",
            "-m",
            message,
            &reference,
            &backup.commit,
            old_value,
        ];
        runner.run(root, &args)
    }
    .map_err(git_failed_to_start)?;
    if !restored.success {
        return Err(failure_of(
            runner,
            root,
            &restored,
            Failure::BackupNotRestored {
                folder: root.clone(),
            },
        ));
    }
    Ok(())
}

/// The commit `reference` of `repository` is on, if it is on one.
fn commit_of(
    runner: &GitRunner,
    repository: &Repository,
    reference: &str,
) -> Result<Option<String>, Failure> {
    match details::resolve(runner, repository, reference) {
        Ok(commit) => Ok(Some(commit)),
        Err(Failure::RevisionNotFound { .. }) => Ok(None),
        Err(failure) => Err(failure),
    }
}

/// The seconds since the epoch.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}
