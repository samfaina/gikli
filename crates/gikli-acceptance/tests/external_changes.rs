//! Changes made outside gikli: gikli watches a Repository and announces when its files change,
//! so the changes view can read them again.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    Change, ChangeStatus, CurrentBranch, Event, Gikli, IncomingOutgoing, Repository, Upstream,
};

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

/// The repository `work`, which pushed its first commit to the `main` of a bare remote, a clone
/// `mine` of that remote, and a linked worktree of the clone on a branch `feature` that follows
/// `origin/main`.
fn linked_worktree_with_upstream(
    sandbox: &Sandbox,
) -> (TestRepository, TestRepository, TestRepository) {
    let remote = sandbox.bare_remote("remote.git");
    let work = committed_repository(sandbox);
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main"]);
    let mine = sandbox.clone_of(&remote, "mine");
    let worktree = mine.linked_worktree("feature");
    worktree.git(&["branch", "--quiet", "--set-upstream-to", "origin/main"]);
    (work, mine, worktree)
}

/// The current branch `feature`, `incoming` commits behind `origin/main` and `outgoing` ahead.
fn feature_following_origin_main(incoming: usize, outgoing: usize) -> CurrentBranch {
    CurrentBranch::Branch {
        name: "feature".to_owned(),
        upstream: Some(Upstream {
            name: "origin/main".to_owned(),
            commits: Some(IncomingOutgoing { incoming, outgoing }),
        }),
    }
}

/// A linked worktree keeps its refs in the common git directory, the one of the main working tree.
#[test]
fn a_ref_moved_from_a_terminal_in_a_linked_worktree_is_announced() {
    let sandbox = Sandbox::new();
    let (_work, _mine, worktree) = linked_worktree_with_upstream(&sandbox);
    let theirs = worktree.git(&["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "Theirs"]);
    let (gikli, watched, events) = watch(&sandbox, &worktree);
    assert_eq!(
        gikli.current_branch(&watched),
        Ok(feature_following_origin_main(0, 0))
    );

    worktree.git(&["update-ref", "refs/remotes/origin/main", theirs.trim()]);

    assert!(next_announcement(&events, &watched, ANNOUNCEMENT_DEADLINE).is_some());
    assert_eq!(
        gikli.current_branch(&watched),
        Ok(feature_following_origin_main(1, 0))
    );
    assert_eq!(
        worktree.git(&["rev-list", "--count", "feature..origin/main"]),
        "1\n"
    );
}

/// The fetch writes no FETCH_HEAD, which a linked worktree keeps in its own git directory, so
/// only the remote branch it moves in the common git directory can announce it.
#[test]
fn a_fetch_from_a_terminal_in_a_linked_worktree_is_announced() {
    let sandbox = Sandbox::new();
    let (work, _mine, worktree) = linked_worktree_with_upstream(&sandbox);
    work.commit_at(2, "Theirs");
    work.git(&["push", "--quiet", "origin", "main"]);
    let (gikli, watched, events) = watch(&sandbox, &worktree);

    worktree.git(&["fetch", "--quiet", "--no-write-fetch-head", "origin"]);

    assert!(next_announcement(&events, &watched, ANNOUNCEMENT_DEADLINE).is_some());
    assert_eq!(
        gikli.current_branch(&watched),
        Ok(feature_following_origin_main(1, 0))
    );
    assert_eq!(
        worktree.git(&["rev-list", "--count", "feature..origin/main"]),
        "1\n"
    );
}

/// The main working tree and its linked worktree share the refs of the common git directory.
#[test]
fn a_ref_moved_in_the_common_git_directory_is_announced_for_every_worktree_that_shares_it() {
    let sandbox = Sandbox::new();
    let (_work, mine, worktree) = linked_worktree_with_upstream(&sandbox);
    let theirs = mine.git(&["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "Theirs"]);
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();
    let watched: Vec<Repository> = [&mine, &worktree]
        .into_iter()
        .map(|repository| {
            let project = gikli.open_project(repository.root()).expect("open project");
            let watched = project.repositories[0].clone();
            gikli.watch_changes(&watched).expect("watch changes");
            watched
        })
        .collect();
    while events.recv_timeout(Duration::from_millis(500)).is_ok() {}

    mine.git(&["update-ref", "refs/remotes/origin/main", theirs.trim()]);

    let mut announced = Vec::new();
    let start = Instant::now();
    while announced.len() < watched.len() && start.elapsed() < ANNOUNCEMENT_DEADLINE {
        if let Ok(Event::FilesChanged { repository }) =
            events.recv_timeout(Duration::from_millis(100))
            && !announced.contains(&repository)
        {
            announced.push(repository);
        }
    }
    assert_eq!(announced.len(), 2, "announced only for {announced:?}");
    assert!(
        watched
            .iter()
            .all(|repository| announced.contains(repository))
    );
    assert_eq!(
        worktree.git(&["rev-parse", "origin/main"]),
        mine.git(&["rev-parse", "origin/main"])
    );
}

/// Objects, reflogs and gikli's own files never change what the changes view or the status area
/// shows, wherever in the common git directory they are written.
#[test]
fn writing_objects_reflogs_or_gikli_files_in_the_common_git_directory_is_not_announced() {
    let sandbox = Sandbox::new();
    let (_work, mine, worktree) = linked_worktree_with_upstream(&sandbox);
    let common_git_dir = mine.root().join(".git");
    let (_gikli, watched, events) = watch(&sandbox, &worktree);

    worktree.write("notes.txt", "not yet added\n");
    while next_announcement(&events, &watched, Duration::from_millis(500)).is_some() {}
    let object = worktree.git(&["hash-object", "-w", "notes.txt"]);
    let reflog = common_git_dir.join("logs/refs/heads/main");
    let mut entries = std::fs::read_to_string(&reflog).expect("read the reflog of main");
    entries.push_str(
        &entries
            .lines()
            .last()
            .expect("an entry")
            .replace("clone", "noted"),
    );
    entries.push('\n');
    std::fs::write(&reflog, entries).expect("append to the reflog of main");
    std::fs::create_dir_all(common_git_dir.join("gikli")).expect("create gikli's folder");
    std::fs::write(common_git_dir.join("gikli/notes.json"), "{}").expect("write a gikli file");

    assert_eq!(next_announcement(&events, &watched, SILENCE), None);
    assert_eq!(worktree.git(&["cat-file", "-t", object.trim()]), "blob\n");
    assert_eq!(worktree.git(&["reflog", "main"]).lines().count(), 2);
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
