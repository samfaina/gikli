//! Conflicts: after an operation that leaves conflicts, the user resolves each file by picking a
//! side or by editing it by hand and marking it resolved.

use std::path::PathBuf;

use gikli_acceptance::{Sandbox, TestRepository, conflicts_left, opened};
use gikli_core::{
    ConflictSide, ConflictedFile, Failure, Gikli, Notification, NotificationAction,
    NotificationKind, NotificationOutcome, OperationAction, Repository, SideChange,
};

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
fn a_merge_that_stops_on_a_conflict_lists_the_conflicted_file() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let conflicts = gikli
        .conflicts(work.root(), &root)
        .expect("read the conflicts");

    assert_eq!(
        conflicts,
        vec![ConflictedFile {
            path: PathBuf::from("file.txt"),
            yours: SideChange::Modified,
            theirs: SideChange::Modified,
        }]
    );
    assert_eq!(work.conflicted_by_git(), vec!["file.txt"]);
}

/// A repository where `main` deleted `file.txt` and `other` changed it since they parted.
fn deleted_on_main(sandbox: &Sandbox) -> TestRepository {
    let work = sandbox.repository("work");
    work.write("file.txt", "base\n");
    work.commit_all("Base");
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.write("file.txt", "theirs\n");
    work.commit_all("Theirs");
    work.git(&["switch", "--quiet", "main"]);
    work.git(&["rm", "--quiet", "file.txt"]);
    work.git(&["commit", "--quiet", "--message", "Delete"]);
    work
}

#[test]
fn during_a_rebase_yours_is_the_side_of_the_branch_being_rebased() {
    let sandbox = Sandbox::new();
    let work = deleted_on_main(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let conflicts = gikli
        .conflicts(work.root(), &root)
        .expect("read the conflicts");

    assert_eq!(
        conflicts,
        vec![ConflictedFile {
            path: PathBuf::from("file.txt"),
            yours: SideChange::Deleted,
            theirs: SideChange::Modified,
        }]
    );
}

#[test]
fn accepting_theirs_in_a_merge_takes_their_version_and_resolves_the_file() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let left = gikli
        .accept_side(
            work.root(),
            &root,
            &["file.txt".to_owned()],
            ConflictSide::Theirs,
        )
        .expect("accept theirs");

    assert_eq!(left, vec![]);
    assert_eq!(work.conflicted_by_git(), Vec::<String>::new());
    assert_eq!(
        work.git(&["show", ":file.txt"]),
        "theirs
"
    );
    assert_eq!(
        std::fs::read_to_string(work.root().join("file.txt")).expect("read the file"),
        "theirs
"
    );
}

#[test]
fn accepting_yours_during_a_rebase_keeps_the_deletion_of_the_branch_being_rebased() {
    let sandbox = Sandbox::new();
    let work = deleted_on_main(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let left = gikli
        .accept_side(
            work.root(),
            &root,
            &["file.txt".to_owned()],
            ConflictSide::Yours,
        )
        .expect("accept yours");

    assert_eq!(left, vec![]);
    assert_eq!(work.conflicted_by_git(), Vec::<String>::new());
    assert!(
        !work.git_succeeds(&["cat-file", "-e", ":file.txt"]),
        "git no longer tracks it"
    );
    assert!(
        !work.root().join("file.txt").exists(),
        "the file is deleted"
    );
}

#[test]
fn accepting_theirs_in_a_delete_modify_conflict_keeps_the_file_they_changed() {
    let sandbox = Sandbox::new();
    let work = deleted_on_main(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    assert_eq!(
        gikli
            .conflicts(work.root(), &root)
            .expect("read the conflicts"),
        vec![ConflictedFile {
            path: PathBuf::from("file.txt"),
            yours: SideChange::Deleted,
            theirs: SideChange::Modified,
        }]
    );

    let left = gikli
        .accept_side(
            work.root(),
            &root,
            &["file.txt".to_owned()],
            ConflictSide::Theirs,
        )
        .expect("accept theirs");

    assert_eq!(left, vec![]);
    assert_eq!(work.conflicted_by_git(), Vec::<String>::new());
    assert_eq!(work.git(&["show", ":file.txt"]), "theirs\n");
    assert_eq!(
        std::fs::read_to_string(work.root().join("file.txt")).expect("read the file"),
        "theirs\n"
    );
}

#[test]
fn accepting_a_side_of_a_file_not_in_conflict_resolves_nothing() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    work.write("second.txt", "second\n");
    work.commit_all("Second");
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let result = gikli.accept_side(
        work.root(),
        &root,
        &["file.txt".to_owned(), "second.txt".to_owned()],
        ConflictSide::Yours,
    );

    assert_eq!(
        result,
        Err(Failure::FileNotInConflict {
            path: PathBuf::from("second.txt")
        })
    );
    assert_eq!(work.conflicted_by_git(), vec!["file.txt"]);
}

#[test]
fn accepting_yours_during_a_rebase_takes_the_version_of_the_branch_being_rebased() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .accept_side(
            work.root(),
            &root,
            &["file.txt".to_owned()],
            ConflictSide::Yours,
        )
        .expect("accept yours");

    assert_eq!(work.git(&["show", ":file.txt"]), "ours\n");
    assert_eq!(
        std::fs::read_to_string(work.root().join("file.txt")).expect("read the file"),
        "ours\n"
    );
}

#[test]
fn a_file_edited_by_hand_can_be_marked_resolved() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    work.write("file.txt", "both\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let left = gikli
        .mark_resolved(work.root(), &root, &["file.txt".to_owned()])
        .expect("mark the file resolved");

    assert_eq!(left, vec![]);
    assert_eq!(work.conflicted_by_git(), Vec::<String>::new());
    assert_eq!(work.git(&["show", ":file.txt"]), "both\n");
}

#[test]
fn a_file_deleted_by_hand_is_marked_resolved_as_deleted() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    work.delete("file.txt");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let left = gikli
        .mark_resolved(work.root(), &root, &["file.txt".to_owned()])
        .expect("mark the file resolved");

    assert_eq!(left, vec![]);
    assert_eq!(work.conflicted_by_git(), Vec::<String>::new());
    assert!(
        !work.git_succeeds(&["cat-file", "-e", ":file.txt"]),
        "git no longer tracks it"
    );
}

/// The paths of `changes`.
fn paths_of(changes: &[gikli_core::Change]) -> Vec<PathBuf> {
    changes.iter().map(|change| change.path.clone()).collect()
}

/// [`diverged`], where `other` also changed `clean.txt`, which merges without a conflict.
fn diverged_with_a_clean_file(sandbox: &Sandbox) -> TestRepository {
    let work = sandbox.repository("work");
    work.write("file.txt", "base\n");
    work.write("clean.txt", "base\n");
    work.commit_all("Base");
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.write("file.txt", "theirs\n");
    work.write("clean.txt", "theirs\n");
    work.commit_all("Theirs");
    work.git(&["switch", "--quiet", "main"]);
    work.write("file.txt", "ours\n");
    work.commit_all("Ours");
    work
}

#[test]
fn the_changes_view_shows_the_conflicted_files_as_a_group_of_their_own() {
    let sandbox = Sandbox::new();
    let work = diverged_with_a_clean_file(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let changes = gikli.read_changes(&root).expect("read the changes");

    assert_eq!(
        paths_of(&changes.conflicts.changes),
        vec![PathBuf::from("file.txt")]
    );
    let active = changes
        .changelists
        .iter()
        .find(|list| list.active)
        .expect("an Active changelist");
    assert_eq!(
        paths_of(&active.group.changes),
        vec![PathBuf::from("clean.txt")]
    );
}

#[test]
fn a_resolved_file_goes_back_to_its_changelist() {
    let sandbox = Sandbox::new();
    let work = diverged_with_a_clean_file(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli.read_changes(&root).expect("read the changes");

    gikli
        .accept_side(
            work.root(),
            &root,
            &["file.txt".to_owned()],
            ConflictSide::Theirs,
        )
        .expect("accept theirs");
    let changes = gikli.read_changes(&root).expect("read the changes again");

    assert_eq!(changes.conflicts.changes, vec![]);
    let active = changes
        .changelists
        .iter()
        .find(|list| list.active)
        .expect("an Active changelist");
    assert_eq!(
        paths_of(&active.group.changes),
        vec![PathBuf::from("clean.txt"), PathBuf::from("file.txt")]
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

#[test]
fn an_operation_that_stops_on_a_conflict_says_so_so_the_conflicts_dialog_opens() {
    let sandbox = Sandbox::new();
    let work = diverged_twice(&sandbox);
    assert!(!work.git_succeeds(&["rebase", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    work.write(
        "file.txt", "both
",
    );
    gikli
        .mark_resolved(work.root(), &root, &["file.txt".to_owned()])
        .expect("mark the file resolved");
    let events = gikli.subscribe();

    gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the rebase");

    assert_eq!(conflicts_left(&events), vec![root.clone()]);
    assert_eq!(work.conflicted_by_git(), vec!["file.txt"]);
}

#[test]
fn an_operation_that_leaves_no_conflict_does_not_open_the_conflicts_dialog() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let events = gikli.subscribe();

    gikli
        .take_operation_action(work.root(), &root, OperationAction::Abort)
        .expect("abort the merge");

    assert_eq!(conflicts_left(&events), vec![]);
}

/// The notifications raised about the conflicts left in `repository`.
fn conflict_notifications(gikli: &Gikli, repository: &Repository) -> Vec<Notification> {
    gikli
        .notifications()
        .into_iter()
        .filter(|notification| {
            notification.kind
                == NotificationKind::ConflictsUnresolved {
                    repository: repository.clone(),
                }
        })
        .collect()
}

#[test]
fn closing_the_conflicts_dialog_raises_one_notification_that_offers_resolve_and_abort() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .close_conflicts(work.root(), &root)
        .expect("close the conflicts dialog");
    gikli
        .close_conflicts(work.root(), &root)
        .expect("close the conflicts dialog again");

    let raised = conflict_notifications(&gikli, &root);
    assert_eq!(raised.len(), 1, "one notification: {raised:?}");
    assert_eq!(raised[0].project, work.root());
    assert_eq!(
        raised[0].actions,
        vec![
            NotificationAction::ResolveConflicts,
            NotificationAction::AbortOperation
        ]
    );
    assert_eq!(work.conflicted_by_git(), vec!["file.txt"]);
}

#[test]
fn resolve_in_the_notification_opens_the_conflicts_dialog_again() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .close_conflicts(work.root(), &root)
        .expect("close the conflicts dialog");
    let id = conflict_notifications(&gikli, &root)[0].id;

    let outcome = gikli
        .act_on_notification(id, &NotificationAction::ResolveConflicts)
        .expect("resolve the conflicts");

    assert_eq!(outcome, NotificationOutcome::ResolveConflicts(root.clone()));
    assert_eq!(conflict_notifications(&gikli, &root), vec![]);
    assert_eq!(work.conflicted_by_git(), vec!["file.txt"]);
}

#[test]
fn abort_in_the_notification_aborts_the_operation_that_left_the_conflicts() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    let before = work.git(&["rev-parse", "HEAD"]);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .close_conflicts(work.root(), &root)
        .expect("close the conflicts dialog");
    let id = conflict_notifications(&gikli, &root)[0].id;

    let outcome = gikli
        .act_on_notification(id, &NotificationAction::AbortOperation)
        .expect("abort the merge");

    assert_eq!(outcome, NotificationOutcome::HeadMoved(root.clone()));
    assert_eq!(conflict_notifications(&gikli, &root), vec![]);
    assert_eq!(work.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(work.status_by_git(), Vec::<String>::new());
    assert_eq!(
        gikli
            .operation_in_progress(work.root(), &root)
            .expect("read the operation in progress"),
        None
    );
}

#[test]
fn the_notification_goes_once_every_file_is_resolved() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .close_conflicts(work.root(), &root)
        .expect("close the conflicts dialog");

    gikli
        .accept_side(
            work.root(),
            &root,
            &["file.txt".to_owned()],
            ConflictSide::Yours,
        )
        .expect("accept yours");

    assert_eq!(conflict_notifications(&gikli, &root), vec![]);
}

#[test]
fn the_notification_goes_once_the_conflicts_are_resolved_outside_gikli() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .close_conflicts(work.root(), &root)
        .expect("close the conflicts dialog");
    work.git(&["merge", "--abort"]);

    gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");

    assert_eq!(conflict_notifications(&gikli, &root), vec![]);
}

/// A repository whose stash changed `file.txt` the other way from the commit after it, so applying
/// the stash stops on a conflict, with no operation in progress.
fn stash_in_conflict(sandbox: &Sandbox) -> TestRepository {
    let work = sandbox.repository("work");
    work.write("file.txt", "base\n");
    work.commit_all("Base");
    work.write("file.txt", "stashed\n");
    work.git(&["stash", "push", "--quiet"]);
    work.write("file.txt", "committed\n");
    work.commit_all("Committed");
    assert!(!work.git_succeeds(&["stash", "apply"]));
    work
}

#[test]
fn conflicts_with_no_operation_in_progress_offer_no_abort() {
    let sandbox = Sandbox::new();
    let work = stash_in_conflict(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    assert_eq!(
        gikli
            .conflicts(work.root(), &root)
            .expect("read the conflicts"),
        vec![ConflictedFile {
            path: PathBuf::from("file.txt"),
            yours: SideChange::Modified,
            theirs: SideChange::Modified,
        }]
    );

    gikli
        .close_conflicts(work.root(), &root)
        .expect("close the conflicts dialog");

    let raised = conflict_notifications(&gikli, &root);
    assert_eq!(raised.len(), 1, "one notification: {raised:?}");
    assert_eq!(
        raised[0].actions,
        vec![NotificationAction::ResolveConflicts]
    );
}

#[test]
fn closing_the_dialog_with_every_file_resolved_raises_nothing() {
    let sandbox = Sandbox::new();
    let work = diverged(&sandbox);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .accept_side(
            work.root(),
            &root,
            &["file.txt".to_owned()],
            ConflictSide::Theirs,
        )
        .expect("accept theirs");

    gikli
        .close_conflicts(work.root(), &root)
        .expect("close the conflicts dialog");

    assert_eq!(conflict_notifications(&gikli, &root), vec![]);
}

#[test]
fn a_file_one_side_never_had_is_absent_on_that_side_not_deleted() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.write("a.txt", "base\n");
    work.commit_all("Base");
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.git(&["mv", "a.txt", "theirs.txt"]);
    work.git(&["commit", "--quiet", "--message", "Theirs"]);
    work.git(&["switch", "--quiet", "main"]);
    work.git(&["mv", "a.txt", "ours.txt"]);
    work.git(&["commit", "--quiet", "--message", "Ours"]);
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let conflicts = gikli
        .conflicts(work.root(), &root)
        .expect("read the conflicts");

    assert_eq!(
        conflicts,
        vec![
            ConflictedFile {
                path: PathBuf::from("a.txt"),
                yours: SideChange::Deleted,
                theirs: SideChange::Deleted,
            },
            ConflictedFile {
                path: PathBuf::from("ours.txt"),
                yours: SideChange::Added,
                theirs: SideChange::Absent,
            },
            ConflictedFile {
                path: PathBuf::from("theirs.txt"),
                yours: SideChange::Absent,
                theirs: SideChange::Added,
            },
        ]
    );
}

/// A repository whose submodule at `lib` moved to a different commit on `main` and on `other` since
/// they parted, so merging them stops on a conflict over `lib`. Returns it with the commits each
/// side moved the submodule to, ours first.
fn submodule_moved_on_both_sides(sandbox: &Sandbox) -> (TestRepository, String, String) {
    sandbox.allow_local_submodules();
    let library = sandbox.repository("library");
    library.write("lib.rs", "base\n");
    library.commit_all("Base");
    let work = sandbox.repository("work");
    work.write("README.md", "hello\n");
    work.commit_all("Initial commit");
    let url = library.root().to_string_lossy().into_owned();
    work.git(&["submodule", "--quiet", "add", &url, "lib"]);
    work.commit_all("Add the library");
    let move_library = |contents: &str| {
        work.write("lib/lib.rs", contents);
        work.git(&[
            "-C",
            "lib",
            "commit",
            "--quiet",
            "--all",
            "--message",
            contents,
        ]);
        work.commit_all(contents);
        work.git(&["rev-parse", "HEAD:lib"]).trim().to_owned()
    };
    work.git(&["switch", "--quiet", "--create", "other"]);
    let theirs = move_library("theirs\n");
    work.git(&["switch", "--quiet", "main"]);
    work.git(&["-C", "lib", "checkout", "--quiet", "HEAD~1"]);
    let ours = move_library("ours\n");
    assert!(!work.git_succeeds(&["merge", "--no-edit", "other"]));
    (work, ours, theirs)
}

#[test]
fn accepting_theirs_for_a_submodule_records_the_commit_they_moved_it_to() {
    let sandbox = Sandbox::new();
    let (work, _, theirs) = submodule_moved_on_both_sides(&sandbox);
    assert_eq!(work.conflicted_by_git(), vec!["lib"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let left = gikli
        .accept_side(
            work.root(),
            &root,
            &["lib".to_owned()],
            ConflictSide::Theirs,
        )
        .expect("accept theirs");

    assert_eq!(left, vec![]);
    assert_eq!(work.conflicted_by_git(), Vec::<String>::new());
    assert_eq!(work.git(&["rev-parse", ":lib"]).trim(), theirs);
}

#[test]
fn accepting_yours_for_a_submodule_records_the_commit_you_moved_it_to() {
    let sandbox = Sandbox::new();
    let (work, ours, _) = submodule_moved_on_both_sides(&sandbox);
    // The submodule shows another commit than either side's.
    work.git(&["-C", "lib", "checkout", "--quiet", "HEAD~1"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .accept_side(work.root(), &root, &["lib".to_owned()], ConflictSide::Yours)
        .expect("accept yours");

    assert_eq!(work.conflicted_by_git(), Vec::<String>::new());
    assert_eq!(work.git(&["rev-parse", ":lib"]).trim(), ours);
}
