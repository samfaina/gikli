//! The failure explainer: it classifies a failed git invocation and returns a plain message, a
//! next step and the actions to offer. git's raw output goes to the operation log only.
//!
//! Classification never reads git's translated sentences. It relies on what stays the same in
//! every language: what git records in its trace of the run, the file names and the commands it
//! quotes, and the state of the repository. ADR-0001 pins what gikli parses, but git's error output
//! has no format to pin, which is why only these fixed parts of it count.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::git::{GitRunner, Invocation};
use crate::{Failure, Repository, canonicalize, processes};

/// A failure explained in plain words. The UI words the message and the next step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Explanation {
    pub message: ExplainedMessage,
    pub next_step: NextStep,
    /// What gikli can do about it, in the order to offer them.
    pub actions: Vec<FailureAction>,
}

/// What went wrong.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExplainedMessage {
    /// A hook of the user's stopped the operation. `output` is what the hook wrote, the one part
    /// of a run shown outside the operation log.
    HookFailed { hook: String, output: String },
    /// git refuses the repository at `folder`, as it belongs to someone else. `folder` is written
    /// as git writes it, which is how git compares it with what the user trusts.
    DubiousOwnership { folder: String },
    /// The index of the Repository is locked: another git operation is using it, or one ended
    /// without cleaning up after itself and left `lock` behind.
    IndexLocked { lock: PathBuf },
    /// A path in the Repository is longer than Windows allows by default.
    LongPaths,
}

/// What the user can do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NextStep {
    /// Fix what the hook reports. gikli runs hooks without a terminal, so one that asks for input
    /// has to work without one.
    FixHook,
    /// Trust the folder, if the user knows where it comes from: a repository can run commands of
    /// its owner's choosing, which is why git asks.
    TrustFolder,
    /// No other git operation is running, so the lock is stale and can go.
    RemoveIndexLock,
    /// Wait for the other git operation to finish, then try again. gikli offers to remove the
    /// lock only when it can tell that none is running, which it cannot for the git inside WSL.
    WaitForOtherGitOperation,
    /// Set `core.longpaths` in git. gikli recommends it and never sets it: it is the user's
    /// configuration, and not every tool on Windows copes with such paths.
    EnableLongPaths,
}

/// Something gikli can do about a failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum FailureAction {
    /// Adds `folder` to the folders the user trusts, in their global git configuration, once they
    /// have confirmed. `repository` is where git refused it, so the same git trusts it.
    TrustFolder {
        repository: Repository,
        folder: String,
    },
    /// Removes the stale lock of the index of `repository`, once the user has confirmed and as
    /// long as no other git operation has started running there.
    RemoveIndexLock { repository: Repository },
}

/// The explanation of the failure of `invocation`, run in `folder`, when gikli knows this failure.
pub(crate) fn explain(
    runner: &GitRunner,
    folder: &Path,
    invocation: &Invocation,
) -> Option<Explanation> {
    if let Some(hook) = &invocation.failed_hook {
        return Some(Explanation {
            message: ExplainedMessage::HookFailed {
                hook: hook.clone(),
                output: String::from_utf8_lossy(&invocation.stderr).into_owned(),
            },
            next_step: NextStep::FixHook,
            actions: vec![],
        });
    }
    let stderr = String::from_utf8_lossy(&invocation.stderr);
    let repository = Repository {
        root: folder.to_path_buf(),
    };
    if let Some(untrusted) = untrusted_folder(&stderr) {
        return Some(Explanation {
            message: ExplainedMessage::DubiousOwnership {
                folder: untrusted.clone(),
            },
            next_step: NextStep::TrustFolder,
            actions: vec![FailureAction::TrustFolder {
                repository,
                folder: untrusted,
            }],
        });
    }
    // git names the file in every language. It may be gone by now, with the operation that held it.
    if stderr.contains(INDEX_LOCK)
        && let Some(lock) = existing_index_lock(runner, folder)
    {
        return Some(explain_index_lock(runner, &repository, lock));
    }
    if cfg!(windows) && runner.runs_natively() && stderr.contains(PATH_TOO_LONG) {
        return Some(Explanation {
            message: ExplainedMessage::LongPaths,
            next_step: NextStep::EnableLongPaths,
            actions: vec![],
        });
    }
    None
}

/// Removes the lock of the index of `repository`, unless a git operation may have started running
/// there since the user was offered this. A lock already gone is fine.
pub(crate) fn remove_stale_index_lock(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<(), Failure> {
    let Some(lock) = existing_index_lock(runner, &repository.root) else {
        return Ok(());
    };
    let explanation = explain_index_lock(runner, repository, lock.clone());
    if explanation.next_step != NextStep::RemoveIndexLock {
        return Err(Failure::Explained { explanation });
    }
    std::fs::remove_file(&lock).map_err(|error| Failure::IndexLockNotRemoved {
        lock,
        message: error.to_string(),
    })
}

/// How git for Windows reports a path that is too long. It is the text of the system's error,
/// which comes from Windows' C library in English, not from git's translations.
const PATH_TOO_LONG: &str = "Filename too long";

/// The name of the file that locks the index.
const INDEX_LOCK: &str = "index.lock";

/// The lock of the index of the repository `folder` is in, when there is one. Each linked worktree
/// has an index, and a lock, of its own.
fn existing_index_lock(runner: &GitRunner, folder: &Path) -> Option<PathBuf> {
    let git_dir = runner.git_dir(folder).ok().flatten()?;
    let lock = canonicalize(&git_dir).ok()?.join(INDEX_LOCK);
    lock.exists().then_some(lock)
}

/// The explanation of the index of `repository` being locked by `lock`. Removing the lock is
/// offered only when gikli can tell that no other git operation is running there: removing the
/// lock of a running one would corrupt the index.
fn explain_index_lock(runner: &GitRunner, repository: &Repository, lock: PathBuf) -> Explanation {
    let git_dir = lock.parent().unwrap_or(&repository.root);
    let stale =
        runner.runs_natively() && !processes::git_runs_in(&[repository.root.as_path(), git_dir]);
    let (next_step, actions) = if stale {
        (
            NextStep::RemoveIndexLock,
            vec![FailureAction::RemoveIndexLock {
                repository: repository.clone(),
            }],
        )
    } else {
        (NextStep::WaitForOtherGitOperation, vec![])
    };
    Explanation {
        message: ExplainedMessage::IndexLocked { lock },
        next_step,
        actions,
    }
}

/// The folder in the command git recommends when it refuses a repository for its owner, on a line
/// of its own. Translations keep the command as it is.
fn untrusted_folder(stderr: &str) -> Option<String> {
    let folder = stderr.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("git config --global --add safe.directory ")
    })?;
    Some(without_shell_quotes(folder.trim()))
}

/// `text` as a shell reads it, when git quoted it with single quotes for one. Recent versions of
/// git quote what needs it, such as a space, and older ones quote nothing.
fn without_shell_quotes(text: &str) -> String {
    match text
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        Some(quoted) => quoted.replace(r"'\''", "'"),
        None => text.to_owned(),
    }
}
