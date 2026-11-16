//! Changes made outside gikli: gikli watches a Repository and announces when its files change,
//! so the changes view can read them again.

use std::path::{Path, PathBuf};
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
    let (gikli, mut watched, events) = watch_each(sandbox, &[repository]);
    (gikli, watched.remove(0), events)
}

/// gikli, watching the Repository of each of `repositories` from one Project each, with a listener
/// of its events. Returns once the file system has gone quiet, as [`watch`] does.
fn watch_each(
    sandbox: &Sandbox,
    repositories: &[&TestRepository],
) -> (Gikli, Vec<Repository>, Receiver<Event>) {
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();
    let watched = repositories
        .iter()
        .map(|repository| {
            let project = gikli.open_project(repository.root()).expect("open project");
            let watched = project.repositories[0].clone();
            gikli.watch_changes(&watched).expect("watch changes");
            watched
        })
        .collect();
    while events.recv_timeout(Duration::from_millis(500)).is_ok() {}
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
    committed_repository_named(sandbox, "app")
}

/// A repository in the folder `name` with one commit.
fn committed_repository_named(sandbox: &Sandbox, name: &str) -> TestRepository {
    let repository = sandbox.repository(name);
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

/// A linked worktree whose branch follows a remote branch, and the repositories around it.
struct LinkedWorktree {
    /// Where others commit and push to the `main` of the remote.
    others: TestRepository,
    /// A clone of the remote: its git directory is the common one of `worktree`.
    main: TestRepository,
    /// A linked worktree of `main`, on a branch `feature` that follows `origin/main`.
    worktree: TestRepository,
}

/// A [`LinkedWorktree`] whose remote's `main` holds one commit, the one `feature` is on.
fn linked_worktree_with_upstream(sandbox: &Sandbox) -> LinkedWorktree {
    let remote = sandbox.bare_remote("remote.git");
    let others = committed_repository(sandbox);
    others.add_remote("origin", &remote);
    others.git(&["push", "--quiet", "origin", "main"]);
    let main = sandbox.clone_of(&remote, "main");
    let worktree = main.linked_worktree("feature");
    worktree.git(&["branch", "--quiet", "--set-upstream-to", "origin/main"]);
    LinkedWorktree {
        others,
        main,
        worktree,
    }
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
    let LinkedWorktree { worktree, .. } = linked_worktree_with_upstream(&sandbox);
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
    let LinkedWorktree {
        others, worktree, ..
    } = linked_worktree_with_upstream(&sandbox);
    others.commit_at(2, "Theirs");
    others.git(&["push", "--quiet", "origin", "main"]);
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
    let LinkedWorktree { main, worktree, .. } = linked_worktree_with_upstream(&sandbox);
    let theirs = main.git(&["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "Theirs"]);
    let (_gikli, watched, events) = watch_each(&sandbox, &[&main, &worktree]);

    main.git(&["update-ref", "refs/remotes/origin/main", theirs.trim()]);

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
        main.git(&["rev-parse", "origin/main"])
    );
}

/// Objects, reflogs and gikli's own files never change what gikli shows, wherever in the common
/// git directory they are written.
#[test]
fn writing_objects_reflogs_or_gikli_files_in_the_common_git_directory_is_not_announced() {
    let sandbox = Sandbox::new();
    let LinkedWorktree { main, worktree, .. } = linked_worktree_with_upstream(&sandbox);
    let common_git_dir = main.root().join(".git");
    worktree.write("notes.txt", "not yet added\n");
    let (_gikli, watched, events) = watch(&sandbox, &worktree);

    let object = worktree.git(&["hash-object", "-w", "notes.txt"]);
    append_a_copy_of_the_last_line(&common_git_dir.join("logs/HEAD"));
    std::fs::create_dir_all(common_git_dir.join("gikli")).expect("create gikli's folder");
    std::fs::write(common_git_dir.join("gikli/notes.json"), "{}").expect("write a gikli file");

    assert_eq!(next_announcement(&events, &watched, SILENCE), None);
    assert_eq!(worktree.git(&["cat-file", "-t", object.trim()]), "blob\n");
    assert_eq!(main.git(&["reflog", "HEAD"]).lines().count(), 2);
}

/// Appends to the file at `path` a copy of its last line, as git appends an entry to a reflog.
fn append_a_copy_of_the_last_line(path: &Path) {
    let mut lines = std::fs::read_to_string(path).expect("read the file");
    let last = lines.lines().last().expect("a line").to_owned();
    lines.push_str(&last);
    lines.push('\n');
    std::fs::write(path, lines).expect("append to the file");
}

/// A submodule keeps its index, HEAD and refs in one git directory, under `modules/` of the git
/// directory of the Repository that holds it: its git directory is also its common one.
#[test]
fn staging_a_file_from_a_terminal_in_a_submodule_is_announced_and_its_objects_are_not() {
    let sandbox = Sandbox::new();
    sandbox.allow_local_submodules();
    let library = committed_repository_named(&sandbox, "library");
    let app = committed_repository(&sandbox);
    let url = library.root().to_string_lossy().into_owned();
    app.git(&["submodule", "--quiet", "add", &url, "lib"]);
    app.commit_all("Add the library");
    let submodule = sandbox.existing_repository(&app.root().join("lib"));
    submodule.write("TODO.md", "todo\n");
    let (gikli, watched, events) = watch(&sandbox, &submodule);

    submodule.git(&["hash-object", "-w", "TODO.md"]);
    assert_eq!(next_announcement(&events, &watched, SILENCE), None);
    submodule.git(&["add", "TODO.md"]);

    assert!(next_announcement(&events, &watched, ANNOUNCEMENT_DEADLINE).is_some());
    let change_set = gikli.read_changes(&watched).expect("read changes");
    assert_eq!(change_set.changes[0].status, ChangeStatus::Added);
    assert_eq!(submodule.status_by_git(), vec!["A  TODO.md"]);
    assert!(submodule.root().join(".git").is_file());
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
