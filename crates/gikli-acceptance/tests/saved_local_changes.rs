//! Saved local changes: an operation that needs a clean working tree puts the user's changes aside
//! in a stash and restores them afterwards, in their original Changelists.

use gikli_acceptance::{Sandbox, TestRepository};
use std::sync::mpsc::Receiver;

use gikli_core::{ChangeSet, Event, Failure, Gikli, MoveTarget, Repository, StashUnavailable};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// The stashes of `repository` as plain git lists them, newest first: the subject of each.
fn stashes_by_git(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["stash", "list", "--format=%gs"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A repository whose one commit holds `notes.txt` and `todo.txt`, opened as a Project.
fn with_two_files(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository) {
    let work = sandbox.repository("work");
    work.write("notes.txt", "one\ntwo\n");
    work.write("todo.txt", "first\n");
    work.commit_all("Add notes and todo");
    let root = opened(gikli, &work);
    (work, root)
}

#[test]
fn saving_puts_the_changes_aside_in_a_stash_and_restoring_brings_them_back() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_files(&sandbox, &gikli);
    work.write("notes.txt", "one\ntwo\nthree\n");

    let saved = gikli.save_local_changes(&root).expect("save local changes");

    assert_eq!(work.status_by_git(), Vec::<String>::new());
    assert_eq!(stashes_by_git(&work).len(), 1);

    let restored = gikli
        .restore_local_changes(work.root(), &saved)
        .expect("restore local changes");

    assert!(!restored.conflicts_left);
    assert_eq!(work.status_by_git(), vec![" M notes.txt"]);
    assert_eq!(
        std::fs::read_to_string(work.root().join("notes.txt")).expect("read notes"),
        "one\ntwo\nthree\n"
    );
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
}

/// The paths of the changes in the Changelist called `name`.
fn paths_in(change_set: &ChangeSet, name: &str) -> Vec<String> {
    change_set
        .changelists
        .iter()
        .find(|list| list.name == name)
        .expect("the Changelist exists")
        .group
        .changes
        .iter()
        .map(|change| change.path.to_string_lossy().into_owned())
        .collect()
}

/// Moves the change at `path` to a new Changelist called Feature, which does not become active.
fn move_to_feature(gikli: &Gikli, root: &Repository, path: &str) {
    gikli
        .move_changes(
            root,
            &[path.to_owned()],
            &MoveTarget::NewChangelist {
                name: "Feature".to_owned(),
            },
            false,
        )
        .expect("move the change to Feature");
}

#[test]
fn restoring_puts_each_change_back_in_its_changelist() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_files(&sandbox, &gikli);
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.write("todo.txt", "first\nsecond\n");
    move_to_feature(&gikli, &root, "notes.txt");

    let saved = gikli.save_local_changes(&root).expect("save local changes");
    let clean = gikli.read_changes(&root).expect("read the clean tree");
    assert_eq!(paths_in(&clean, "Feature"), Vec::<String>::new());
    gikli
        .restore_local_changes(work.root(), &saved)
        .expect("restore local changes");

    let changes = gikli.read_changes(&root).expect("read changes");
    assert_eq!(paths_in(&changes, "Feature"), vec!["notes.txt"]);
    assert_eq!(paths_in(&changes, "Changes"), vec!["todo.txt"]);
}

#[test]
fn changes_go_back_to_their_changelists_after_the_operation_moved_head() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_files(&sandbox, &gikli);
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.write("notes.txt", "zero\none\ntwo\n");
    work.commit_all("Start notes earlier");
    work.git(&["switch", "--quiet", "main"]);
    work.write("notes.txt", "one\ntwo\nthree\n");
    move_to_feature(&gikli, &root, "notes.txt");

    let saved = gikli.save_local_changes(&root).expect("save local changes");
    work.git(&["switch", "--quiet", "other"]);
    gikli
        .restore_local_changes(work.root(), &saved)
        .expect("restore local changes");

    assert_eq!(
        std::fs::read_to_string(work.root().join("notes.txt")).expect("read notes"),
        "zero\none\ntwo\nthree\n"
    );
    let changes = gikli.read_changes(&root).expect("read changes");
    assert_eq!(paths_in(&changes, "Feature"), vec!["notes.txt"]);
}

/// Stashes the uncommitted changes of `repository` with plain git, as the user would elsewhere.
fn stash_by_git(repository: &TestRepository, message: &str) {
    repository.git(&["stash", "push", "--quiet", "--message", message]);
}

#[test]
fn when_nothing_was_saved_nothing_is_restored() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_files(&sandbox, &gikli);
    work.write("notes.txt", "one\ntwo\nthree\n");
    stash_by_git(&work, "The user's own");

    let saved = gikli.save_local_changes(&root).expect("save local changes");
    assert_eq!(saved.stash, None);
    let restored = gikli
        .restore_local_changes(work.root(), &saved)
        .expect("restore nothing");

    assert!(!restored.conflicts_left);
    assert_eq!(work.status_by_git(), Vec::<String>::new());
    assert_eq!(stashes_by_git(&work), vec!["On main: The user's own"]);
}

#[test]
fn files_git_does_not_track_are_not_saved() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_files(&sandbox, &gikli);
    work.write("new.txt", "new\n");

    let saved = gikli.save_local_changes(&root).expect("save local changes");

    assert_eq!(saved.stash, None);
    assert_eq!(work.status_by_git(), vec!["?? new.txt"]);
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
}

#[test]
fn only_what_this_operation_saved_is_restored() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_files(&sandbox, &gikli);
    work.write("notes.txt", "one\ntwo\nthree\n");
    let saved = gikli.save_local_changes(&root).expect("save local changes");
    work.write("todo.txt", "first\nsecond\n");
    stash_by_git(&work, "Made meanwhile");

    gikli
        .restore_local_changes(work.root(), &saved)
        .expect("restore local changes");

    assert_eq!(work.status_by_git(), vec![" M notes.txt"]);
    assert_eq!(stashes_by_git(&work), vec!["On main: Made meanwhile"]);
}

/// The Repositories the events received so far said an operation left in conflict.
fn conflicts_left(events: &Receiver<Event>) -> Vec<Repository> {
    events
        .try_iter()
        .filter_map(|event| match event {
            Event::ConflictsLeft { repository } => Some(repository),
            _ => None,
        })
        .collect()
}

#[test]
fn a_restore_that_conflicts_keeps_the_saved_changes_and_opens_conflict_resolution() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_files(&sandbox, &gikli);
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.write("todo.txt", "first\nsecond\n");
    move_to_feature(&gikli, &root, "notes.txt");
    let saved = gikli.save_local_changes(&root).expect("save local changes");
    work.write("notes.txt", "one\ntwo\nTHREE\n");
    work.commit_all("Change notes");
    let events = gikli.subscribe();

    let restored = gikli
        .restore_local_changes(work.root(), &saved)
        .expect("restore onto a conflict");

    assert!(restored.conflicts_left);
    assert_eq!(conflicts_left(&events), vec![root.clone()]);
    assert_eq!(
        stashes_by_git(&work),
        vec!["On main: Local changes saved by gikli"]
    );
    let changes = gikli.read_changes(&root).expect("read changes");
    let conflicted: Vec<_> = changes
        .conflicts
        .changes
        .iter()
        .map(|change| change.path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(conflicted, vec!["notes.txt"]);
    assert_eq!(paths_in(&changes, "Changes"), vec!["todo.txt"]);

    gikli
        .mark_resolved(work.root(), &root, &["notes.txt".to_owned()])
        .expect("mark the conflict resolved");
    let resolved = gikli.read_changes(&root).expect("read changes");
    assert_eq!(paths_in(&resolved, "Feature"), vec!["notes.txt"]);
}

#[test]
fn a_repository_with_no_commits_cannot_save_local_changes_and_says_why() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let work = sandbox.repository("work");
    work.write("notes.txt", "one\n");
    work.git(&["add", "notes.txt"]);
    let root = opened(&gikli, &work);

    let failure = gikli
        .save_local_changes(&root)
        .expect_err("no commit to save local changes against");

    assert_eq!(
        failure,
        Failure::LocalChangesNotSavable {
            folder: root.root.clone(),
            reason: StashUnavailable::NoCommits,
        }
    );
    assert_eq!(work.status_by_git(), vec!["A  notes.txt"]);
}
