//! The changes watcher: announces when the files of a Repository change outside gikli.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode, MetadataKind, ModifyKind};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::Repository;
use crate::events::{Event, EventBus};

/// How long the files must stay quiet before a burst of changes is announced.
const QUIET_PERIOD: Duration = Duration::from_millis(250);

/// The longest a burst that does not stop is held back before it is announced anyway.
const LONGEST_WAIT: Duration = Duration::from_secs(2);

/// Folders of a git directory whose changes never change the changes view: git's object store,
/// its reflogs, and gikli's own files, which it writes when it reads the changes. They count at any
/// depth, so those of submodules under `modules/` and worktrees under `worktrees/` count too.
const UNWATCHED_GIT_FOLDERS: &[&str] = &["objects", "logs", "gikli"];

/// Watches one Repository until dropped.
pub(crate) struct ChangesWatcher {
    _watcher: RecommendedWatcher,
}

impl ChangesWatcher {
    /// Starts watching the working tree of `repository` and its `git_dir`, when git named one,
    /// and sends [`Event::FilesChanged`] to `events` after each burst of changes.
    pub fn start(
        repository: Repository,
        git_dir: Option<PathBuf>,
        events: Arc<EventBus>,
    ) -> notify::Result<Self> {
        let inside = repository.root.join(".git");
        let outside = git_dir.filter(|git_dir| !git_dir.starts_with(&repository.root));
        let git_dirs: Vec<PathBuf> = std::iter::once(inside).chain(outside.clone()).collect();
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<_>| {
            // An error can mean lost events: count it as a change, so nothing goes unseen.
            if result.map_or(true, |event| is_change(&event, &git_dirs)) {
                let _ = sender.send(());
            }
        })?;
        watcher.watch(&repository.root, RecursiveMode::Recursive)?;
        // A linked worktree or a submodule keeps its index and HEAD outside its working tree.
        if let Some(git_dir) = &outside {
            watcher.watch(git_dir, RecursiveMode::Recursive)?;
        }
        std::thread::spawn(move || announce_bursts(&receiver, &repository, &events));
        Ok(Self { _watcher: watcher })
    }
}

/// Whether `event` is a change to a file that can change the changes view. Reading a file is
/// not: git reads the working tree every time gikli reads the changes. Neither is anything in
/// the [`UNWATCHED_GIT_FOLDERS`] of the `git_dirs`.
fn is_change(event: &notify::Event, git_dirs: &[PathBuf]) -> bool {
    let reads = matches!(
        event.kind,
        EventKind::Access(
            AccessKind::Read | AccessKind::Open(_) | AccessKind::Close(AccessMode::Read)
        ) | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
    );
    !reads
        && (event.paths.is_empty()
            || event
                .paths
                .iter()
                .any(|path| !is_in_unwatched_git_folder(path, git_dirs)))
}

fn is_in_unwatched_git_folder(path: &Path, git_dirs: &[PathBuf]) -> bool {
    git_dirs.iter().any(|git_dir| {
        path.strip_prefix(git_dir).is_ok_and(|inside| {
            inside.components().any(|component| {
                matches!(component, Component::Normal(name)
                    if UNWATCHED_GIT_FOLDERS.iter().any(|folder| name == *folder))
            })
        })
    })
}

/// Announces each burst of changes signalled on `changes` once it goes quiet, or once it has
/// lasted [`LONGEST_WAIT`]. Stops when the watcher is dropped.
fn announce_bursts(changes: &Receiver<()>, repository: &Repository, events: &EventBus) {
    while changes.recv().is_ok() {
        let burst_start = Instant::now();
        loop {
            let time_left = LONGEST_WAIT.saturating_sub(burst_start.elapsed());
            match changes.recv_timeout(QUIET_PERIOD.min(time_left)) {
                Ok(()) if !time_left.is_zero() => {}
                Ok(()) | Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        events.send(Event::FilesChanged {
            repository: repository.clone(),
        });
    }
}
