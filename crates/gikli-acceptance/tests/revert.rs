//! Revert: the user undoes commits of the current branch with inverse commits, one per commit,
//! each reviewed in the commit panel with an automatic message, where files can be unticked.

use gikli_acceptance::{Sandbox, TestRepository, conflicts_left, opened};

use gikli_core::{
    ConflictSide, Failure, Gikli, OperationAction, OperationInProgress, OperationKind,
    PreparedRevert, Repository,
};

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
    let one = work.commit_of("HEAD~1");

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
        work.subjects(),
        vec!["Two", "One", "Base"],
        "nothing is committed yet"
    );
    assert_eq!(work.read("notes.txt"), "notes\n");
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
    let one = work.commit_of("HEAD~1");
    let prepared = gikli
        .revert_commits(&root, std::slice::from_ref(&one))
        .expect("revert One");

    gikli
        .commit_files(&root, &["one.txt".to_owned()], &prepared.message)
        .expect("commit the revert without notes.txt");

    assert_eq!(
        work.subjects(),
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
        work.read("notes.txt"),
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
    let two = work.commit_of("HEAD");
    let one = work.commit_of("HEAD~1");

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
        work.subjects(),
        vec!["Revert \"One\"", "Revert \"Two\"", "Two", "One", "Base"]
    );
    assert_eq!(gikli.prepared_revert(&root).expect("read the revert"), None);
    assert!(work.status_by_git().is_empty());
}

/// [`with_two_commits`], then Three, which changes `notes.txt` again, so the inverse of One is in
/// conflict with it.
fn with_one_in_conflict(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository) {
    let (work, root) = with_two_commits(sandbox, gikli);
    work.write("notes.txt", "notes of three\n");
    work.commit_all("Three");
    (work, root)
}

#[test]
fn a_revert_that_stops_on_a_conflict_leads_to_the_conflict_list_and_is_in_progress() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_one_in_conflict(&sandbox, &gikli);
    let one = work.commit_of("HEAD~2");
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
    let three = work.commit_of("HEAD");
    let one = work.commit_of("HEAD~2");
    let two = work.commit_of("HEAD~1");
    gikli
        .revert_commits(&root, &[one, two])
        .expect("the revert of One stops on a conflict");

    let after = gikli
        .take_operation_action(work.root(), &root, OperationAction::Abort)
        .expect("abort the revert");

    assert_eq!(after, None, "the revert is over");
    assert_eq!(work.commit_of("HEAD"), three);
    assert!(
        work.status_by_git().is_empty(),
        "the files are as they were"
    );
    assert_eq!(work.read("notes.txt"), "notes of three\n");
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
    let one = work.commit_of("HEAD~2");
    let two = work.commit_of("HEAD~1");
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

    assert_eq!(work.subjects()[0], "Revert \"One\"");
    assert!(!work.root().join("one.txt").exists());
    assert_eq!(work.read("notes.txt"), "notes of three\n");
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
    let two = work.commit_of("HEAD");
    work.git(&["switch", "--quiet", "--create", "older", "HEAD~1"]);
    let one = work.commit_of("HEAD");

    let failure = gikli
        .revert_commits(&root, &[one.clone(), two.clone()])
        .expect_err("Two is not on older");

    assert_eq!(failure, Failure::CommitNotOnCurrentBranch { commit: two });
    assert_eq!(work.commit_of("HEAD"), one, "nothing is reverted");
    assert!(work.status_by_git().is_empty());
    assert_eq!(gikli.prepared_revert(&root).expect("read the revert"), None);
}

#[test]
fn no_revert_starts_while_another_operation_is_in_progress() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_one_in_conflict(&sandbox, &gikli);
    let one = work.commit_of("HEAD~2");
    let two = work.commit_of("HEAD~1");
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
    let three = work.commit_of("HEAD");
    let one = work.commit_of("HEAD~2");
    let two = work.commit_of("HEAD~1");
    gikli
        .revert_commits(&root, &[one, two.clone()])
        .expect("the revert of One stops on a conflict");

    gikli
        .take_operation_action(work.root(), &root, OperationAction::Skip)
        .expect("skip the revert of One");

    assert_eq!(work.commit_of("HEAD"), three, "One is left as it is");
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
    let two = work.commit_of("HEAD");
    let one = work.commit_of("HEAD~1");
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

#[test]
fn continuing_a_revert_started_outside_gikli_after_its_own_was_aborted_reverts_nothing_after_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_commits(&sandbox, &gikli);
    let two = work.commit_of("HEAD");
    let one = work.commit_of("HEAD~1");
    gikli
        .revert_commits(&root, &[two.clone(), one])
        .expect("revert Two, with One left to revert");
    // Outside gikli, the user aborts it, and reverts Two again on another HEAD.
    work.git(&["revert", "--abort"]);
    work.write("other.txt", "other\n");
    work.commit_all("Other");
    work.git(&["revert", "--no-commit", &two]);

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the revert");

    assert_eq!(in_progress, None);
    assert_eq!(
        work.subjects(),
        vec!["Revert \"Two\"", "Other", "Two", "One", "Base"]
    );
    assert_eq!(
        gikli.prepared_revert(&root).expect("read the revert"),
        None,
        "One is not reverted after it"
    );
}

#[test]
fn skipping_a_revert_started_outside_gikli_after_its_own_was_aborted_reverts_nothing_after_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_one_in_conflict(&sandbox, &gikli);
    let three = work.commit_of("HEAD");
    let one = work.commit_of("HEAD~2");
    let two = work.commit_of("HEAD~1");
    gikli
        .revert_commits(&root, &[one, two.clone()])
        .expect("the revert of One stops on a conflict, with Two left to revert");
    // Outside gikli, the user aborts it, and reverts another commit.
    work.git(&["revert", "--abort"]);
    work.git(&["revert", "--no-commit", &three]);

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Skip)
        .expect("skip the revert of Three");

    assert_eq!(in_progress, None);
    assert_eq!(work.commit_of("HEAD"), three, "Three is left as it is");
    assert_eq!(
        gikli.prepared_revert(&root).expect("read the revert"),
        None,
        "Two is not reverted after it"
    );
    assert!(work.root().join("two.txt").exists());
}

/// A repository on `main` with Base, which holds `notes.txt`, then One, which adds `one.txt`,
/// then Two, which adds `two-a.txt` and `two-b.txt`, opened as a Project.
fn with_two_files_in_two(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository) {
    let work = sandbox.repository("work");
    work.write("notes.txt", "notes\n");
    work.commit_all("Base");
    work.write("one.txt", "one\n");
    work.commit_all("One");
    work.write("two-a.txt", "two a\n");
    work.write("two-b.txt", "two b\n");
    work.commit_all("Two");
    let root = opened(gikli, &work);
    (work, root)
}

#[test]
fn a_file_left_out_of_an_inverse_commit_survives_aborting_the_next_revert() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_files_in_two(&sandbox, &gikli);
    let two = work.commit_of("HEAD");
    let one = work.commit_of("HEAD~1");
    let first = gikli
        .revert_commits(&root, &[two, one])
        .expect("revert Two and One");
    gikli
        .commit_files(&root, &["two-a.txt".to_owned()], &first.message)
        .expect("commit the revert of Two without two-b.txt");

    gikli
        .take_operation_action(work.root(), &root, OperationAction::Abort)
        .expect("abort the revert of One");

    assert!(work.root().join("one.txt").exists(), "One is not reverted");
    assert!(
        !work.root().join("two-b.txt").exists(),
        "two-b.txt keeps the change the user left out of the inverse commit"
    );
}

#[test]
fn a_revert_does_not_start_over_staged_changes_that_aborting_it_would_lose() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_commits(&sandbox, &gikli);
    let one = work.commit_of("HEAD~1");
    let two = work.commit_of("HEAD");
    work.write("two.txt", "two, staged\n");
    work.git(&["add", "two.txt"]);

    let failure = gikli
        .revert_commits(&root, std::slice::from_ref(&one))
        .expect_err("two.txt is staged");

    assert_eq!(
        failure,
        Failure::StagedChangesBlockRevert {
            folder: root.root.clone(),
            paths: vec!["two.txt".to_owned()],
        }
    );
    assert_eq!(work.commit_of("HEAD"), two, "nothing is reverted");
    assert_eq!(gikli.prepared_revert(&root).expect("read the revert"), None);
    assert_eq!(work.read("two.txt"), "two, staged\n");
}

#[test]
fn a_commit_that_goes_through_says_when_the_next_revert_could_not_follow() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_commits(&sandbox, &gikli);
    let two = work.commit_of("HEAD");
    let one = work.commit_of("HEAD~1");
    // git reverts nothing over a change on disk to a file the revert changes.
    work.write("one.txt", "one, edited\n");
    let first = gikli
        .revert_commits(&root, &[two, one.clone()])
        .expect("revert Two and One");

    let failure = gikli
        .commit_files(&root, &first.paths, &first.message)
        .expect_err("One cannot be reverted over one.txt");

    let Failure::NextRevertFailed { commit, left, .. } = failure else {
        panic!("not the failure of the next revert: {failure:?}");
    };
    assert_eq!(commit, one);
    assert!(left.is_empty());
    assert_eq!(
        work.subjects()[0],
        "Revert \"Two\"",
        "the commit went through"
    );
    assert_eq!(work.read("one.txt"), "one, edited\n");
    assert_eq!(gikli.prepared_revert(&root).expect("read the revert"), None);
}

#[test]
fn reverting_a_revert_reapplies_the_commit_it_reverted() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_commits(&sandbox, &gikli);
    let one = work.commit_of("HEAD~1");
    let prepared = gikli
        .revert_commits(&root, std::slice::from_ref(&one))
        .expect("revert One");
    gikli
        .commit_files(&root, &prepared.paths, &prepared.message)
        .expect("commit the revert of One");
    let reverted = work.commit_of("HEAD");

    let again = gikli
        .revert_commits(&root, std::slice::from_ref(&reverted))
        .expect("revert the revert of One");

    assert_eq!(
        again.message,
        format!("Reapply \"One\"\n\nThis reverts commit {reverted}.")
    );
}

#[test]
fn the_files_of_a_reverted_rename_are_ticked_at_both_paths() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_commits(&sandbox, &gikli);
    work.git(&["mv", "notes.txt", "renamed.txt"]);
    work.git(&["commit", "--quiet", "--message", "Rename"]);
    let rename = work.commit_of("HEAD");

    let prepared = gikli
        .revert_commits(&root, std::slice::from_ref(&rename))
        .expect("revert Rename");

    assert_eq!(
        prepared.paths,
        vec!["notes.txt".to_owned(), "renamed.txt".to_owned()]
    );
    gikli
        .commit_files(&root, &prepared.paths, &prepared.message)
        .expect("commit the revert of Rename");
    assert!(work.root().join("notes.txt").exists());
    assert!(!work.root().join("renamed.txt").exists());
    assert!(work.status_by_git().is_empty());
}

#[test]
fn continuing_a_revert_commits_it_with_the_automatic_message_and_the_next_one_follows() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_commits(&sandbox, &gikli);
    let two = work.commit_of("HEAD");
    let one = work.commit_of("HEAD~1");
    gikli
        .revert_commits(&root, &[two.clone(), one.clone()])
        .expect("revert Two and One");

    gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the revert of Two");

    assert_eq!(
        work.git(&["log", "-1", "--format=%B"]).trim_end(),
        revert_message("Two", &two)
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
fn in_staging_mode_a_revert_commits_what_is_staged_and_the_next_one_follows() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_files_in_two(&sandbox, &gikli);
    gikli.set_staging_mode(true).expect("turn Staging mode on");
    let two = work.commit_of("HEAD");
    let one = work.commit_of("HEAD~1");
    let first = gikli
        .revert_commits(&root, &[two, one.clone()])
        .expect("revert Two and One");
    gikli
        .unstage_files(&root, &["two-b.txt".to_owned()])
        .expect("leave two-b.txt out");

    gikli
        .commit_staged(&root, &first.message)
        .expect("commit the revert of Two");

    assert_eq!(
        work.git(&["show", "--format=", "--name-only", "HEAD"])
            .trim_end(),
        "two-a.txt"
    );
    assert!(!work.root().join("two-b.txt").exists());
    assert_eq!(
        gikli
            .prepared_revert(&root)
            .expect("read the revert")
            .map(|prepared| prepared.commit),
        Some(one)
    );
}
