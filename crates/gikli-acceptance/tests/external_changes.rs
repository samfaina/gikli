//! Changes made outside gikli: gikli watches a Repository and announces when its files change,
//! so the changes view can read them again.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{Change, ChangeStatus, Event, Gikli, Repository};

/// How long a change may take to be announced before the test fails. Generous, for slow CI
/// machines; an announcement normally arrives well under a second after the change.
const ANNOUNCEMENT_DEADLINE: Duration = Duration::from_secs(10);

/// How long the tests wait to be sure no announcement is coming.
const SILENCE: Duration = Duration::from_millis(1500);

/// gikli, watching the Repository of `repository`, with a listener of its events. Returns once
/// the file system has gone quiet, so changes from setting up the repository are not counted.
fn watch(sandbox: &Sandbox, repository: &TestRepository) -> (Gikli, Repository, Receiver<Event>) {
    let gikli = sandbox.gikli();
    let project = gikli.open_project(repository.root()).expect("open project");
    let watched = project.repositories[0].clone();
    let events = gikli.subscribe();
    gikli.watch_changes(&watched).expect("watch changes");
    while next_announcement(&events, &watched, Duration::from_millis(500)).is_some() {}
    (gikli, watched, events)
}

/// Waits up to `timeout` for the next announcement that the files of `repository` changed, and
/// returns how long it took.
fn next_announcement(
    events: &Receiver<Event>,
    repository: &Repository,
    timeout: Duration,
) -> Option<Duration> {
    let start = Instant::now();
    loop {
        let remaining = timeout.checked_sub(start.elapsed())?;
        match events.recv_timeout(remaining) {
            Ok(Event::FilesChanged {
                repository: changed,
            }) if &changed == repository => {
                return Some(start.elapsed());
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => return None,
            Err(RecvTimeoutError::Disconnected) => panic!("gikli stopped sending events"),
        }
    }
}

fn committed_repository(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("README.md", "hello\n");
    repository.commit_all("Initial commit");
    repository
}

#[test]
fn a_file_edited_outside_gikli_is_announced_shortly_after() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    let (gikli, watched, events) = watch(&sandbox, &repository);

    repository.write("README.md", "hello, world\n");

    assert!(next_announcement(&events, &watched, ANNOUNCEMENT_DEADLINE).is_some());
    let change_set = gikli.read_changes(&watched).expect("read changes");
    assert_eq!(
        change_set.changes,
        vec![Change {
            path: PathBuf::from("README.md"),
            status: ChangeStatus::Modified,
            original_path: None,
            conflict: None,
            submodule: None,
        }]
    );
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

/// A pause of the machine between these writes or in delivering their events, or writes that
/// outlast the watcher's longest wait, rightly make more than one burst, so this test cannot ask
/// for one announcement. That a burst without a pause is announced once is proved in
/// `crates/gikli-core/src/watch.rs`, with no files involved.
#[test]
fn a_burst_of_changes_outside_gikli_is_not_announced_change_by_change() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    let (_gikli, watched, events) = watch(&sandbox, &repository);

    for index in 0..50 {
        repository.write(&format!("generated/file {index}.txt"), "generated\n");
    }
    repository.write("README.md", "hello, world\n");

    assert!(next_announcement(&events, &watched, ANNOUNCEMENT_DEADLINE).is_some());
    let mut announcements = 1;
    while next_announcement(&events, &watched, SILENCE).is_some() {
        announcements += 1;
    }
    assert!(
        announcements < 51,
        "51 changes were announced {announcements} times"
    );
    assert_eq!(repository.status_by_git().len(), 51);
}

#[test]
fn staging_a_file_from_a_terminal_is_announced() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("TODO.md", "todo\n");
    let (gikli, watched, events) = watch(&sandbox, &repository);

    repository.git(&["add", "TODO.md"]);

    assert!(next_announcement(&events, &watched, ANNOUNCEMENT_DEADLINE).is_some());
    let change_set = gikli.read_changes(&watched).expect("read changes");
    assert_eq!(change_set.changes[0].status, ChangeStatus::Added);
    assert_eq!(repository.status_by_git(), vec!["A  TODO.md"]);
}

/// Reading the changes runs git in the Repository. If that counted as a change, the view would
/// refresh itself forever.
#[test]
fn reading_the_changes_is_not_announced_as_a_change() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("README.md", "hello, world\n");
    let (gikli, watched, events) = watch(&sandbox, &repository);

    gikli.read_changes(&watched).expect("read changes");

    assert_eq!(next_announcement(&events, &watched, SILENCE), None);
}

/// The command that changes the Active changelist returns the changes itself.
#[test]
fn setting_the_active_changelist_is_not_announced_as_a_change() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    let (gikli, watched, events) = watch(&sandbox, &repository);
    let change_set = gikli.read_changes(&watched).expect("read changes");

    gikli
        .set_active_changelist(&watched, &change_set.changelists[0].id)
        .expect("set the Active changelist");

    assert_eq!(next_announcement(&events, &watched, SILENCE), None);
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

/// A linked worktree keeps its index and HEAD in a git directory outside its working tree.
#[test]
fn staging_a_file_from_a_terminal_in_a_linked_worktree_is_announced() {
    let sandbox = Sandbox::new();
    let main = committed_repository(&sandbox);
    let worktree = main.linked_worktree("feature");
    worktree.write("TODO.md", "todo\n");
    let (gikli, watched, events) = watch(&sandbox, &worktree);

    worktree.git(&["add", "TODO.md"]);

    assert!(next_announcement(&events, &watched, ANNOUNCEMENT_DEADLINE).is_some());
    let change_set = gikli.read_changes(&watched).expect("read changes");
    assert_eq!(change_set.changes[0].status, ChangeStatus::Added);
    assert_eq!(worktree.status_by_git(), vec!["A  TODO.md"]);
    assert!(worktree.root().join(".git").is_file());
}

#[test]
fn a_repository_no_longer_watched_is_not_announced() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    let (gikli, watched, events) = watch(&sandbox, &repository);

    gikli.unwatch_changes(&watched);
    repository.write("README.md", "hello, world\n");

    assert_eq!(next_announcement(&events, &watched, SILENCE), None);
    assert_eq!(repository.status_by_git(), vec![" M README.md"]);
}

#[test]
fn watching_a_repository_twice_announces_each_change_once() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    let (gikli, watched, events) = watch(&sandbox, &repository);
    gikli.watch_changes(&watched).expect("watch changes again");

    repository.write("README.md", "hello, world\n");

    assert!(next_announcement(&events, &watched, ANNOUNCEMENT_DEADLINE).is_some());
    assert_eq!(next_announcement(&events, &watched, SILENCE), None);
}
