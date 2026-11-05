//! The changes watcher: announces when the files of a Repository change outside gikli.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::event::{AccessKind, MetadataKind, ModifyKind};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::Repository;
use crate::events::{Event, EventBus};

/// How long the files must stay quiet before a burst of changes is announced.
const QUIET: Duration = Duration::from_millis(250);

/// The longest a burst that does not stop is held back before it is announced anyway.
const LONGEST_WAIT: Duration = Duration::from_secs(2);

/// Folders of the git directory whose changes never change the changes view: git's object
/// store and its reflogs.
const UNWATCHED_GIT_FOLDERS: &[&str] = &["objects", "logs"];

/// Watches one Repository until dropped.
pub(crate) struct ChangesWatcher {
    _watcher: RecommendedWatcher,
}

impl ChangesWatcher {
    /// Starts watching the working tree of `repository`, with the git directory inside it, and
    /// sends [`Event::FilesChanged`] to `events` after each burst of changes.
    pub fn start(repository: Repository, events: Arc<EventBus>) -> notify::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let unwatched: Vec<PathBuf> = UNWATCHED_GIT_FOLDERS
            .iter()
            .map(|folder| repository.root.join(".git").join(folder))
            .collect();
        let mut watcher = notify::recommended_watcher(move |result: notify::Result<_>| {
            // An error can mean lost events: count it as a change, so nothing goes unseen.
            let changed = result.map_or(true, |event| is_change(&event, &unwatched));
            if changed {
                let _ = sender.send(());
            }
        })?;
        watcher.watch(&repository.root, RecursiveMode::Recursive)?;
        std::thread::spawn(move || announce_bursts(&receiver, &repository, &events));
        Ok(Self { _watcher: watcher })
    }
}

/// Whether `event` is a change to a file that can change the changes view. Reading a file is
/// not: git reads the working tree every time gikli reads the changes.
fn is_change(event: &notify::Event, unwatched: &[PathBuf]) -> bool {
    let reads = matches!(
        event.kind,
        EventKind::Access(
            AccessKind::Read
                | AccessKind::Open(_)
                | AccessKind::Close(notify::event::AccessMode::Read)
                | AccessKind::Any
        ) | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))
    );
    !reads
        && (event.paths.is_empty()
            || event
                .paths
                .iter()
                .any(|path| !is_inside_any(path, unwatched)))
}

fn is_inside_any(path: &Path, folders: &[PathBuf]) -> bool {
    folders.iter().any(|folder| path.starts_with(folder))
}

/// Announces each burst of changes signalled on `changes` once it goes quiet, or once it has
/// lasted [`LONGEST_WAIT`]. Stops when the watcher is dropped.
fn announce_bursts(changes: &Receiver<()>, repository: &Repository, events: &EventBus) {
    while changes.recv().is_ok() {
        let first = Instant::now();
        loop {
            let left = LONGEST_WAIT.saturating_sub(first.elapsed());
            match changes.recv_timeout(QUIET.min(left)) {
                Ok(()) if !left.is_zero() => {}
                Ok(()) | Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        events.send(Event::FilesChanged {
            repository: repository.clone(),
        });
    }
}
