//! Operation in progress: a merge, rebase, revert or cherry-pick that is half done is always in
//! view, with a way to continue or abort it, even when it was started outside gikli.

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    Failure, Gikli, Notification, NotificationAction, NotificationKind, NotificationOutcome,
    OperationAction, OperationInProgress, OperationKind, Repository,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A repository where `main` and `other` each changed the one line of `file.txt` differently since
/// they parted, so combining them stops on a conflict.
fn diverged(sandbox: &Sandbox) -> TestRepository {
    let work = sandbox.repository("work");
    work.write("file.txt", "base\n");
    work.commit_all("Base");
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.write("file.txt", "theirs\n");
    work.commit_all("Theirs");
    work.git(&["switch", "--quiet", "main"]);
    work.write("file.txt", "ours\n");
    work.commit_all("Ours");
    work
}

#[test]
fn a_merge_stopped_on_a_conflict_outside_gikli_is_in_progress() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");

    assert_eq!(
        operation,
        Some(OperationInProgress {
            kind: OperationKind::Merge,
            actions: vec![OperationAction::CommitMerge, OperationAction::Abort],
        })
    );
}

#[test]
fn a_repository_where_nothing_is_half_done_has_no_operation_in_progress() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");

    assert_eq!(operation, None);
}

/// The parents of HEAD in `repository`, as git lists them.
fn parents_of_head(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["rev-list", "--parents", "--max-count=1", "HEAD"])
        .split_whitespace()
        .skip(1)
        .map(str::to_owned)
        .collect()
}

/// The commit `revision` names in `repository`.
fn commit_of(repository: &TestRepository, revision: &str) -> String {
    repository.git(&["rev-parse", revision]).trim().to_owned()
}

#[test]
fn commit_merge_finishes_a_resolved_merge_with_the_message_git_prepared() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    let ours = commit_of(&work, "main");
    let theirs = commit_of(&work, "other");
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    work.write("file.txt", "both\n");
    work.git(&["add", "file.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .take_operation_action(work.root(), &root, OperationAction::CommitMerge)
        .expect("commit the merge");

    assert_eq!(operation, None);
    assert_eq!(parents_of_head(&work), vec![ours, theirs]);
    assert_eq!(
        work.git(&["log", "--max-count=1", "--format=%s"]).trim(),
        "Merge branch 'other'"
    );
    assert_eq!(work.git(&["show", "HEAD:file.txt"]), "both\n");
}

#[test]
fn aborting_a_merge_brings_back_head_and_the_files_as_they_were() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    let ours = commit_of(&work, "main");
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .take_operation_action(work.root(), &root, OperationAction::Abort)
        .expect("abort the merge");

    assert_eq!(operation, None);
    assert_eq!(commit_of(&work, "HEAD"), ours);
    assert_eq!(work.status_by_git(), Vec::<String>::new());
}

#[test]
fn an_operation_does_not_go_on_while_files_still_have_conflicts() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    let ours = commit_of(&work, "main");
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let result = gikli.take_operation_action(work.root(), &root, OperationAction::CommitMerge);

    assert_eq!(result, Err(Failure::ConflictsNotResolved));
    assert_eq!(commit_of(&work, "HEAD"), ours);
    assert!(
        gikli
            .operation_in_progress(work.root(), &root)
            .expect("read the operation in progress")
            .is_some(),
        "the merge is still in progress"
    );
}

#[test]
fn an_action_needs_an_operation_in_progress() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let result = gikli.take_operation_action(work.root(), &root, OperationAction::Abort);

    assert_eq!(result, Err(Failure::NoOperationInProgress));
}

#[test]
fn a_rebase_stopped_on_a_conflict_offers_to_continue_skip_or_abort() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");

    assert_eq!(
        operation,
        Some(OperationInProgress {
            kind: OperationKind::Rebase,
            actions: vec![
                OperationAction::Continue,
                OperationAction::Skip,
                OperationAction::Abort
            ],
        })
    );
}

/// [`diverged`], where `main` changed the line of `file.txt` once more after `Ours`, so rebasing
/// `main` onto `other` stops on a conflict at each of its two commits.
fn diverged_twice(sandbox: &Sandbox) -> TestRepository {
    let work = diverged(sandbox);
    work.write("file.txt", "ours again\n");
    work.commit_all("Ours again");
    work
}

/// The subjects of the commits of `repository` from HEAD back, as git lists them.
fn subjects(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["log", "--format=%s"])
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_rebase_that_continues_and_stops_on_the_next_conflict_is_still_in_progress() {
    let sandbox = Sandbox::new();
    let work = diverged_twice(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    work.write("file.txt", "both\n");
    work.git(&["add", "file.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the rebase");

    assert_eq!(
        operation.map(|operation| operation.kind),
        Some(OperationKind::Rebase)
    );
    assert_eq!(subjects(&work), vec!["Ours", "Theirs", "Base"]);
}

#[test]
fn a_rebase_that_continues_to_its_end_moves_the_branch_and_is_over() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    work.write("file.txt", "both\n");
    work.git(&["add", "file.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the rebase");

    assert_eq!(operation, None);
    assert_eq!(subjects(&work), vec!["Ours", "Theirs", "Base"]);
    assert_eq!(
        work.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "main"
    );
    assert_eq!(work.git(&["show", "HEAD:file.txt"]), "both\n");
}

#[test]
fn skipping_leaves_out_the_commit_the_rebase_stopped_on() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .take_operation_action(work.root(), &root, OperationAction::Skip)
        .expect("skip the commit");

    assert_eq!(operation, None);
    assert_eq!(subjects(&work), vec!["Theirs", "Base"]);
    assert_eq!(commit_of(&work, "main"), commit_of(&work, "other"));
}

#[test]
fn aborting_a_rebase_brings_the_branch_back_to_where_it_was() {
    let sandbox = Sandbox::new();
    let work = diverged_twice(&sandbox);
    let before = commit_of(&work, "main");
    assert!(!work.git_succeeds(&["rebase", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .take_operation_action(work.root(), &root, OperationAction::Abort)
        .expect("abort the rebase");

    assert_eq!(operation, None);
    assert_eq!(commit_of(&work, "main"), before);
    assert_eq!(
        work.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "main"
    );
}

#[test]
fn a_merge_offers_no_skip() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let result = gikli.take_operation_action(work.root(), &root, OperationAction::Skip);

    assert_eq!(
        result,
        Err(Failure::OperationActionNotOffered {
            action: OperationAction::Skip
        })
    );
}

/// The notifications raised about the rebase stopped in `repository`.
fn rebase_notifications(gikli: &Gikli, repository: &Repository) -> Vec<Notification> {
    gikli
        .notifications()
        .into_iter()
        .filter(|notification| {
            notification.kind
                == NotificationKind::RebaseStopped {
                    repository: repository.clone(),
                }
        })
        .collect()
}

#[test]
fn a_stopped_rebase_raises_one_notification_that_offers_to_continue_or_abort() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");
    gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress again");

    let raised = rebase_notifications(&gikli, &root);
    assert_eq!(raised.len(), 1, "one notification: {raised:?}");
    assert_eq!(raised[0].project, work.root());
    assert_eq!(
        raised[0].actions,
        vec![
            NotificationAction::ContinueOperation,
            NotificationAction::AbortOperation
        ]
    );
}

#[test]
fn abort_in_the_notification_aborts_the_rebase_and_the_notification_goes() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    let before = commit_of(&work, "main");
    assert!(!work.git_succeeds(&["rebase", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");
    let id = rebase_notifications(&gikli, &root)[0].id;

    let outcome = gikli
        .act_on_notification(id, &NotificationAction::AbortOperation)
        .expect("abort the rebase");

    assert_eq!(outcome, NotificationOutcome::HeadMoved(root.clone()));
    assert_eq!(commit_of(&work, "main"), before);
    assert_eq!(rebase_notifications(&gikli, &root), vec![]);
}

#[test]
fn continue_in_the_notification_that_stops_again_raises_it_again() {
    let sandbox = Sandbox::new();
    let work = diverged_twice(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    work.write("file.txt", "both\n");
    work.git(&["add", "file.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");
    let id = rebase_notifications(&gikli, &root)[0].id;

    gikli
        .act_on_notification(id, &NotificationAction::ContinueOperation)
        .expect("continue the rebase");

    let raised = rebase_notifications(&gikli, &root);
    assert_eq!(raised.len(), 1, "the rebase stopped again: {raised:?}");
    assert_ne!(raised[0].id, id);
}

#[test]
fn the_notification_goes_once_the_rebase_is_over_outside_gikli() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");
    work.git(&["rebase", "--abort"]);

    let operation = gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress again");

    assert_eq!(operation, None);
    assert_eq!(rebase_notifications(&gikli, &root), vec![]);
}

#[test]
fn a_dismissed_notification_comes_back_for_the_next_rebase() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");
    gikli.dismiss_notification(rebase_notifications(&gikli, &root)[0].id);
    gikli
        .operation_in_progress(work.root(), &root)
        .expect("read while the rebase is still stopped");
    assert_eq!(rebase_notifications(&gikli, &root), vec![]);
    work.git(&["rebase", "--abort"]);
    gikli
        .operation_in_progress(work.root(), &root)
        .expect("read once the rebase is over");
    assert!(!work.git_succeeds(&["rebase", "other"]));

    gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the next rebase");

    assert_eq!(rebase_notifications(&gikli, &root).len(), 1);
}

#[test]
fn a_stopped_revert_offers_to_continue_or_abort_and_can_be_aborted() {
    let sandbox = Sandbox::new();
    let work = diverged_twice(&sandbox);
    let before = commit_of(&work, "HEAD");
    assert!(!work.git_succeeds(&["revert", "--no-edit", "HEAD~1"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let operation = gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");
    assert_eq!(
        operation,
        Some(OperationInProgress {
            kind: OperationKind::Revert,
            actions: vec![OperationAction::Continue, OperationAction::Abort],
        })
    );

    let operation = gikli
        .take_operation_action(work.root(), &root, OperationAction::Abort)
        .expect("abort the revert");

    assert_eq!(operation, None);
    assert_eq!(commit_of(&work, "HEAD"), before);
    assert_eq!(work.status_by_git(), Vec::<String>::new());
}

/// [`diverged`], where `other` went on with two commits that each add a file of their own, so
/// cherry-picking its three commits onto `main` stops on a conflict at the first.
fn three_to_pick(sandbox: &Sandbox) -> TestRepository {
    let work = diverged(sandbox);
    work.git(&["switch", "--quiet", "other"]);
    work.write("two.txt", "two\n");
    work.commit_all("Two");
    work.write("three.txt", "three\n");
    work.commit_all("Three");
    work.git(&["switch", "--quiet", "main"]);
    work
}

#[test]
fn a_stopped_cherry_pick_offers_to_continue_or_abort() {
    let sandbox = Sandbox::new();
    let work = three_to_pick(&sandbox);
    assert!(!work.git_succeeds(&["cherry-pick", "other~2", "other~1", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");

    assert_eq!(
        operation,
        Some(OperationInProgress {
            kind: OperationKind::CherryPick,
            actions: vec![OperationAction::Continue, OperationAction::Abort],
        })
    );
}

#[test]
fn a_cherry_pick_sequence_stays_unfinished_after_a_commit_in_its_middle() {
    let sandbox = Sandbox::new();
    let work = three_to_pick(&sandbox);
    assert!(!work.git_succeeds(&["cherry-pick", "other~2", "other~1", "other"]));
    work.write("file.txt", "both\n");
    work.git(&["add", "file.txt"]);
    work.git(&["commit", "--quiet", "--no-edit"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");

    assert_eq!(
        operation.map(|operation| operation.kind),
        Some(OperationKind::CherryPick)
    );
}

#[test]
fn continuing_a_cherry_pick_sequence_after_a_commit_in_its_middle_picks_the_rest() {
    let sandbox = Sandbox::new();
    let work = three_to_pick(&sandbox);
    assert!(!work.git_succeeds(&["cherry-pick", "other~2", "other~1", "other"]));
    work.write("file.txt", "both\n");
    work.git(&["add", "file.txt"]);
    work.git(&["commit", "--quiet", "--no-edit"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let operation = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the cherry-pick");

    assert_eq!(operation, None);
    assert_eq!(
        subjects(&work),
        vec!["Three", "Two", "Theirs", "Ours", "Base"]
    );
}

#[test]
fn an_operation_in_progress_belongs_to_the_worktree_it_is_half_done_in() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    let linked = work.linked_worktree("linked");
    assert!(!linked.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let main_root = opened(&gikli, &work);
    let linked_root = opened(&gikli, &linked);

    let in_main = gikli
        .operation_in_progress(work.root(), &main_root)
        .expect("read the operation in progress of the main worktree");
    let in_linked = gikli
        .operation_in_progress(linked.root(), &linked_root)
        .expect("read the operation in progress of the linked worktree");

    assert_eq!(in_main, None);
    assert_eq!(
        in_linked.map(|operation| operation.kind),
        Some(OperationKind::Merge)
    );
}
