//! Revert: the user undoes commits of the current branch with inverse commits, one per commit,
//! each reviewed in the commit panel with an automatic message, where files can be unticked.

use gikli_acceptance::{Sandbox, TestRepository};
use std::sync::mpsc::Receiver;

use gikli_core::{
    ConflictSide, Event, Failure, Gikli, OperationAction, OperationInProgress, OperationKind,
    PreparedRevert, Repository,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// The full hash of the commit `revision` names in `repository`, as git resolves it.
fn commit_of(repository: &TestRepository, revision: &str) -> String {
    repository
        .git(&["rev-parse", "--verify", revision])
        .trim_end()
        .to_owned()
}

/// The subjects of the commits of the current branch of `repository`, newest first.
fn subjects(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["log", "--format=%s"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// What the file at `path` of `repository` holds on disk.
fn read(repository: &TestRepository, path: &str) -> String {
    std::fs::read_to_string(repository.root().join(path)).expect("read the file")
}

/// The message git gives the inverse commit of `commit`, whose subject is `subject`.
fn revert_message(subject: &str, commit: &str) -> String {
    format!("Revert \"{subject}\"\n\nThis reverts commit {commit}.")
}

/// A repository on `main` with Base, which holds `notes.txt`, then One, which changes it and adds
/// `one.txt`, then Two, which adds `two.txt`, opened as a Project.
fn with_two_commits(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository) {
    let work = sandbox.repository("work");
    work.write("notes.txt", "notes\n");
    work.commit_all("Base");
    work.write("notes.txt", "notes of one\n");
    work.write("one.txt", "one\n");
    work.commit_all("One");
    work.write("two.txt", "two\n");
    work.commit_all("Two");
    let root = opened(gikli, &work);
    (work, root)
}

#[test]
fn reverting_a_commit_prepares_its_inverse_for_the_commit_panel() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_commits(&sandbox, &gikli);
    let one = commit_of(&work, "HEAD~1");

    let prepared = gikli
        .revert_commits(&root, std::slice::from_ref(&one))
        .expect("revert One");

    assert_eq!(
        prepared,
        PreparedRevert {
            commit: one.clone(),
            message: revert_message("One", &one),
            paths: vec!["notes.txt".to_owned(), "one.txt".to_owned()],
        }
    );
    assert_eq!(
        subjects(&work),
        vec!["Two", "One", "Base"],
        "nothing is committed yet"
    );
    assert_eq!(read(&work, "notes.txt"), "notes\n");
    assert!(!work.root().join("one.txt").exists());
    let in_progress = gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress")
        .expect("the revert is in progress");
    assert_eq!(in_progress.kind, OperationKind::Revert);
    assert_eq!(
        gikli
            .prefilled_commit_message(&root, &prepared.paths)
            .expect("prefill the message"),
        Some(revert_message("One", &one)),
        "the commit panel starts from the automatic message"
    );
}

#[test]
fn committing_the_prepared_revert_with_a_file_unticked_leaves_that_file_out() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_commits(&sandbox, &gikli);
    let one = commit_of(&work, "HEAD~1");
    let prepared = gikli
        .revert_commits(&root, std::slice::from_ref(&one))
        .expect("revert One");

    gikli
        .commit_files(&root, &["one.txt".to_owned()], &prepared.message)
        .expect("commit the revert without notes.txt");

    assert_eq!(
        subjects(&work),
        vec!["Revert \"One\"", "Two", "One", "Base"]
    );
    assert_eq!(
        work.git(&["log", "-1", "--format=%B"]).trim_end(),
        revert_message("One", &one)
    );
    assert_eq!(
        work.git(&["show", "--format=", "--name-only", "HEAD"])
            .trim_end(),
        "one.txt",
        "the inverse commit holds only the ticked file"
    );
    assert_eq!(
        read(&work, "notes.txt"),
        "notes\n",
        "the unticked file keeps its reverted content, uncommitted"
    );
    assert_eq!(
        gikli
            .operation_in_progress(work.root(), &root)
            .expect("read the operation in progress"),
        None,
        "the revert is over"
    );
    assert_eq!(gikli.prepared_revert(&root).expect("read the revert"), None);
}

#[test]
fn reverting_several_commits_makes_one_inverse_commit_for_each_in_the_order_given() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_commits(&sandbox, &gikli);
    let two = commit_of(&work, "HEAD");
    let one = commit_of(&work, "HEAD~1");

    let first = gikli
        .revert_commits(&root, &[two.clone(), one.clone()])
        .expect("revert Two and One");
    assert_eq!(first.commit, two, "the newest goes first, as given");
    assert_eq!(first.paths, vec!["two.txt".to_owned()]);
    gikli
        .commit_files(&root, &first.paths, &first.message)
        .expect("commit the revert of Two");

    let second = gikli
        .prepared_revert(&root)
        .expect("read the revert")
        .expect("One is reverted next, for the commit panel");
    assert_eq!(second.commit, one);
    assert_eq!(second.message, revert_message("One", &one));
    gikli
        .commit_files(&root, &second.paths, &second.message)
        .expect("commit the revert of One");

    assert_eq!(
        subjects(&work),
        vec!["Revert \"One\"", "Revert \"Two\"", "Two", "One", "Base"]
    );
    assert_eq!(gikli.prepared_revert(&root).expect("read the revert"), None);
    assert!(work.status_by_git().is_empty());
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

/// [`with_two_commits`], then Three, which changes `notes.txt` again, so the inverse of One is in
/// conflict with it.
fn with_one_in_conflict(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository) {
    let (work, root) = with_two_commits(sandbox, gikli);
    work.write(
        "notes.txt",
        "notes of three
",
    );
    work.commit_all("Three");
    (work, root)
}

#[test]
fn a_revert_that_stops_on_a_conflict_leads_to_the_conflict_list_and_is_in_progress() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_one_in_conflict(&sandbox, &gikli);
    let one = commit_of(&work, "HEAD~2");
    let events = gikli.subscribe();

    let prepared = gikli
        .revert_commits(&root, std::slice::from_ref(&one))
        .expect("the revert stops on a conflict");

    assert_eq!(prepared.commit, one);
    assert_eq!(prepared.message, revert_message("One", &one));
    assert_eq!(
        conflicts_left(&events),
        vec![root.clone()],
        "the conflicts dialog opens"
    );
    let conflicted: Vec<_> = gikli
        .conflicts(work.root(), &root)
        .expect("read the conflicts")
        .into_iter()
        .map(|file| file.path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(conflicted, vec!["notes.txt"]);
    assert_eq!(
        gikli
            .operation_in_progress(work.root(), &root)
            .expect("read the operation in progress"),
        Some(OperationInProgress {
            kind: OperationKind::Revert,
            actions: vec![
                OperationAction::Continue,
                OperationAction::Skip,
                OperationAction::Abort
            ],
        }),
        "the status area offers to abort it"
    );
}

#[test]
fn aborting_a_stopped_revert_brings_back_what_was_there_and_gives_up_the_rest() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_one_in_conflict(&sandbox, &gikli);
    let three = commit_of(&work, "HEAD");
    let one = commit_of(&work, "HEAD~2");
    let two = commit_of(&work, "HEAD~1");
    gikli
        .revert_commits(&root, &[one, two])
        .expect("the revert of One stops on a conflict");

    let after = gikli
        .take_operation_action(work.root(), &root, OperationAction::Abort)
        .expect("abort the revert");

    assert_eq!(after, None, "the revert is over");
    assert_eq!(commit_of(&work, "HEAD"), three);
    assert!(
        work.status_by_git().is_empty(),
        "the files are as they were"
    );
    assert_eq!(
        read(&work, "notes.txt"),
        "notes of three
"
    );
    assert_eq!(
        gikli.prepared_revert(&root).expect("read the revert"),
        None,
        "Two is not reverted after it"
    );
}

#[test]
fn a_revert_stopped_on_a_conflict_is_committed_once_resolved_and_the_next_one_follows() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_one_in_conflict(&sandbox, &gikli);
    let one = commit_of(&work, "HEAD~2");
    let two = commit_of(&work, "HEAD~1");
    let prepared = gikli
        .revert_commits(&root, &[one.clone(), two.clone()])
        .expect("the revert of One stops on a conflict");
    gikli
        .accept_side(
            work.root(),
            &root,
            &["notes.txt".to_owned()],
            ConflictSide::Yours,
        )
        .expect("keep notes.txt as Three left it");

    gikli
        .commit_files(&root, &prepared.paths, &prepared.message)
        .expect("commit the revert of One");

    assert_eq!(subjects(&work)[0], "Revert \"One\"");
    assert!(!work.root().join("one.txt").exists());
    assert_eq!(
        read(&work, "notes.txt"),
        "notes of three
"
    );
    let next = gikli
        .prepared_revert(&root)
        .expect("read the revert")
        .expect("Two is reverted next");
    assert_eq!(next.commit, two);
}

#[test]
fn a_commit_the_current_branch_does_not_hold_is_not_reverted() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_commits(&sandbox, &gikli);
    let two = commit_of(&work, "HEAD");
    work.git(&["switch", "--quiet", "--create", "older", "HEAD~1"]);
    let one = commit_of(&work, "HEAD");

    let failure = gikli
        .revert_commits(&root, &[one.clone(), two.clone()])
        .expect_err("Two is not on older");

    assert_eq!(failure, Failure::CommitNotOnCurrentBranch { commit: two });
    assert_eq!(commit_of(&work, "HEAD"), one, "nothing is reverted");
    assert!(work.status_by_git().is_empty());
    assert_eq!(gikli.prepared_revert(&root).expect("read the revert"), None);
}

#[test]
fn no_revert_starts_while_another_operation_is_in_progress() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_one_in_conflict(&sandbox, &gikli);
    let one = commit_of(&work, "HEAD~2");
    let two = commit_of(&work, "HEAD~1");
    gikli
        .revert_commits(&root, std::slice::from_ref(&one))
        .expect("the revert of One stops on a conflict");

    let failure = gikli
        .revert_commits(&root, &[two])
        .expect_err("the revert of One is in progress");

    assert_eq!(
        failure,
        Failure::OperationAlreadyInProgress {
            operation: OperationKind::Revert
        }
    );
    assert_eq!(
        gikli
            .prepared_revert(&root)
            .expect("read the revert")
            .map(|prepared| prepared.commit),
        Some(one)
    );
}

#[test]
fn skipping_a_stopped_revert_reverts_the_next_one() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_one_in_conflict(&sandbox, &gikli);
    let three = commit_of(&work, "HEAD");
    let one = commit_of(&work, "HEAD~2");
    let two = commit_of(&work, "HEAD~1");
    gikli
        .revert_commits(&root, &[one, two.clone()])
        .expect("the revert of One stops on a conflict");

    gikli
        .take_operation_action(work.root(), &root, OperationAction::Skip)
        .expect("skip the revert of One");

    assert_eq!(commit_of(&work, "HEAD"), three, "One is left as it is");
    assert_eq!(
        gikli
            .prepared_revert(&root)
            .expect("read the revert")
            .map(|prepared| prepared.commit),
        Some(two),
        "Two is reverted next"
    );
    assert!(!work.root().join("two.txt").exists());
}

#[test]
fn a_revert_committed_outside_gikli_gives_up_the_commits_after_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_commits(&sandbox, &gikli);
    let two = commit_of(&work, "HEAD");
    let one = commit_of(&work, "HEAD~1");
    gikli
        .revert_commits(&root, &[two, one.clone()])
        .expect("revert Two and One");
    work.git(&["commit", "--quiet", "--no-edit"]);
    assert_eq!(
        gikli
            .operation_in_progress(work.root(), &root)
            .expect("read the operation in progress"),
        None
    );
    work.git(&["revert", "--no-commit", &one]);

    gikli
        .commit_files(
            &root,
            &["notes.txt".to_owned(), "one.txt".to_owned()],
            "Undo One",
        )
        .expect("commit the revert of One the user started");

    assert_eq!(
        gikli.prepared_revert(&root).expect("read the revert"),
        None,
        "One is not reverted after it"
    );
}
