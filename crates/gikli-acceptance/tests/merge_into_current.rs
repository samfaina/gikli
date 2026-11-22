//! Merge into current: the user merges a branch into the one HEAD is on from the branch selector.
//! A merge that stops on a conflict leads to the conflicts dialog, and one that local changes block
//! says which, for the user to save them, merge and restore them through Saved local changes.

use gikli_acceptance::{Sandbox, TestRepository, conflicts_left, opened, path_bufs, paths_in};
use gikli_core::{
    ConflictSide, Failure, Gikli, MoveTarget, OperationAction, OperationKind, RefKind, RefLabel,
    Repository,
};

/// A repository whose `main` holds `notes.txt` and `todo.txt`, and whose branch `other` changes
/// `notes.txt` and adds `plan.txt` while `main` changes `todo.txt`, opened as a Project on `main`.
fn with_other_branch(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository) {
    let work = sandbox.repository("work");
    work.write("notes.txt", "one\ntwo\n");
    work.write("todo.txt", "first\n");
    work.commit_all("Add notes and todo");
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.write("notes.txt", "zero\none\ntwo\n");
    work.write("plan.txt", "plan\n");
    work.commit_all("Start notes earlier and plan");
    work.git(&["switch", "--quiet", "main"]);
    work.write("todo.txt", "first\nsecond\n");
    work.commit_all("Add to todo");
    let root = opened(gikli, &work);
    (work, root)
}

/// The local branch `name`.
fn local(name: &str) -> RefLabel {
    RefLabel {
        name: name.to_owned(),
        kind: RefKind::Local,
    }
}

#[test]
fn merge_into_current_merges_the_selected_branch_with_the_message_git_writes() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);

    let in_progress = gikli
        .merge_into_current(work.root(), &root, &local("other"))
        .expect("merge other into main");

    assert_eq!(in_progress, None, "the merge is over");
    assert_eq!(
        work.git(&["log", "-1", "--format=%s"]).trim(),
        "Merge branch 'other'"
    );
    assert_eq!(
        work.git(&["rev-parse", "HEAD^2"]),
        work.git(&["rev-parse", "other"]),
        "the merge commit has other as its second parent"
    );
    assert_eq!(work.read("notes.txt"), "zero\none\ntwo\n");
    assert_eq!(work.read("todo.txt"), "first\nsecond\n");
    assert!(work.status_by_git().is_empty());
}

/// The paths of the files in conflict in `repository` of the Project at `project`, as the
/// conflicts dialog lists them.
fn conflicted(gikli: &Gikli, project: &TestRepository, repository: &Repository) -> Vec<String> {
    gikli
        .conflicts(project.root(), repository)
        .expect("read the conflicts")
        .iter()
        .map(|file| file.path.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn a_merge_that_stops_on_a_conflict_leads_to_the_conflicts_dialog() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write("notes.txt", "uno\ntwo\n");
    work.commit_all("Say one in Spanish");
    let events = gikli.subscribe();

    let in_progress = gikli
        .merge_into_current(work.root(), &root, &local("other"))
        .expect("the merge stops on a conflict");

    assert_eq!(
        in_progress.map(|operation| operation.kind),
        Some(OperationKind::Merge)
    );
    assert_eq!(conflicts_left(&events), vec![root.clone()]);
    assert_eq!(conflicted(&gikli, &work, &root), vec!["notes.txt"]);
}

#[test]
fn a_merge_local_changes_block_lists_the_files_that_block_it_with_nothing_changed() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    let head = work.git(&["rev-parse", "HEAD"]);
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.write("todo.txt", "first\nsecond\nthird\n");

    let failure = gikli
        .merge_into_current(work.root(), &root, &local("other"))
        .expect_err("local changes to notes.txt block the merge");

    assert_eq!(
        failure,
        Failure::MergeBlocked {
            folder: root.root.clone(),
            local_changes: path_bufs(&["notes.txt"]),
            untracked_files: Vec::new(),
        },
        "other does not change todo.txt, so it does not block"
    );
    assert_eq!(work.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M todo.txt"]);
}

/// Changes `notes.txt`, which `other` changes too, and `todo.txt`, in the Changelist `Feature`.
fn change_notes_and_todo_in_feature(gikli: &Gikli, work: &TestRepository, root: &Repository) {
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.write("todo.txt", "first\nsecond\nthird\n");
    gikli
        .move_changes(
            root,
            &["todo.txt".to_owned()],
            &MoveTarget::NewChangelist {
                name: "Feature".to_owned(),
            },
            false,
        )
        .expect("move todo.txt to Feature");
}

#[test]
fn saving_merging_and_restoring_brings_the_local_changes_back_in_their_changelists() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    change_notes_and_todo_in_feature(&gikli, &work, &root);

    let in_progress = gikli
        .merge_over_local_changes(work.root(), &root, &local("other"))
        .expect("save, merge and restore");

    assert_eq!(in_progress, None);
    assert_eq!(
        work.git(&["log", "-1", "--format=%s"]).trim(),
        "Merge branch 'other'"
    );
    assert_eq!(work.read("notes.txt"), "zero\none\ntwo\nthree\n");
    assert_eq!(work.read("todo.txt"), "first\nsecond\nthird\n");
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M todo.txt"]);
    assert_eq!(work.stashes_by_git(), Vec::<String>::new());
    let changes = gikli.read_changes(&root).expect("read changes");
    assert_eq!(paths_in(&changes, "Feature"), vec!["todo.txt"]);
    assert_eq!(paths_in(&changes, "Changes"), vec!["notes.txt"]);
}

/// Commits a change to `notes.txt` on `main` that conflicts with what `other` does to it.
fn commit_conflicting_notes(work: &TestRepository) {
    work.write("notes.txt", "uno\ntwo\n");
    work.commit_all("Say one in Spanish");
}

#[test]
fn a_merge_over_local_changes_that_stops_on_a_conflict_keeps_them_saved() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    commit_conflicting_notes(&work);
    change_notes_and_todo_in_feature(&gikli, &work, &root);
    let events = gikli.subscribe();

    let in_progress = gikli
        .merge_over_local_changes(work.root(), &root, &local("other"))
        .expect("save and merge onto a conflict");

    assert_eq!(
        in_progress.map(|operation| operation.kind),
        Some(OperationKind::Merge)
    );
    assert_eq!(conflicts_left(&events), vec![root.clone()]);
    assert_eq!(conflicted(&gikli, &work, &root), vec!["notes.txt"]);
    assert_eq!(
        work.stashes_by_git(),
        vec!["On main: Local changes saved by gikli"]
    );
    assert_eq!(
        work.read("todo.txt"),
        "first\nsecond\n",
        "todo.txt waits saved"
    );
}

#[test]
fn aborting_the_merge_restores_the_local_changes_in_their_changelists() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    commit_conflicting_notes(&work);
    let head = work.git(&["rev-parse", "HEAD"]);
    change_notes_and_todo_in_feature(&gikli, &work, &root);
    gikli
        .merge_over_local_changes(work.root(), &root, &local("other"))
        .expect("save and merge onto a conflict");

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Abort)
        .expect("abort the merge");

    assert_eq!(in_progress, None);
    assert_eq!(work.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(work.read("notes.txt"), "one\ntwo\nthree\n");
    assert_eq!(work.read("todo.txt"), "first\nsecond\nthird\n");
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M todo.txt"]);
    assert_eq!(work.stashes_by_git(), Vec::<String>::new());
    let changes = gikli.read_changes(&root).expect("read changes");
    assert_eq!(paths_in(&changes, "Feature"), vec!["todo.txt"]);
    assert_eq!(paths_in(&changes, "Changes"), vec!["notes.txt"]);
}

#[test]
fn committing_the_merge_once_resolved_restores_the_local_changes_in_their_changelists() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    commit_conflicting_notes(&work);
    change_notes_and_todo_in_feature(&gikli, &work, &root);
    gikli
        .merge_over_local_changes(work.root(), &root, &local("other"))
        .expect("save and merge onto a conflict");
    gikli
        .accept_side(
            work.root(),
            &root,
            &["notes.txt".to_owned()],
            ConflictSide::Yours,
        )
        .expect("keep my notes");

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::CommitMerge)
        .expect("commit the merge");

    assert_eq!(in_progress, None);
    assert_eq!(
        work.git(&["log", "-1", "--format=%s"]).trim(),
        "Merge branch 'other'"
    );
    assert_eq!(work.read("notes.txt"), "one\ntwo\nthree\n");
    assert_eq!(work.read("todo.txt"), "first\nsecond\nthird\n");
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M todo.txt"]);
    assert_eq!(work.stashes_by_git(), Vec::<String>::new());
    let changes = gikli.read_changes(&root).expect("read changes");
    assert_eq!(paths_in(&changes, "Feature"), vec!["todo.txt"]);
}

#[test]
fn a_merge_untracked_files_block_lists_them_and_still_does_after_saving_the_changes() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    let head = work.git(&["rev-parse", "HEAD"]);
    work.write("notes.txt", "one\ntwo\nthree\n");
    work.write("plan.txt", "my own plan\n");
    work.write("scratch.txt", "scratch\n");
    let blocked = |local_changes: &[&str]| Failure::MergeBlocked {
        folder: root.root.clone(),
        local_changes: path_bufs(local_changes),
        untracked_files: path_bufs(&["plan.txt"]),
    };

    assert_eq!(
        gikli.merge_into_current(work.root(), &root, &local("other")),
        Err(blocked(&["notes.txt"])),
        "scratch.txt is in no branch, so it does not block"
    );
    assert_eq!(
        gikli.merge_over_local_changes(work.root(), &root, &local("other")),
        Err(blocked(&[])),
        "saving leaves plan.txt in the way"
    );

    assert_eq!(work.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(work.read("notes.txt"), "one\ntwo\nthree\n");
    assert_eq!(work.read("plan.txt"), "my own plan\n");
    assert_eq!(
        work.status_by_git(),
        vec![" M notes.txt", "?? plan.txt", "?? scratch.txt"]
    );
    assert_eq!(work.stashes_by_git(), Vec::<String>::new());
}

#[test]
fn a_staged_change_blocks_a_merge_that_combines_both_sides() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.write("todo.txt", "first\nsecond\nthird\n");
    work.git(&["add", "todo.txt"]);

    let failure = gikli
        .merge_into_current(work.root(), &root, &local("other"))
        .expect_err("git merges nothing over a staged change");

    assert_eq!(
        failure,
        Failure::MergeBlocked {
            folder: root.root.clone(),
            local_changes: path_bufs(&["todo.txt"]),
            untracked_files: Vec::new(),
        }
    );
    assert_eq!(work.status_by_git(), vec!["M  todo.txt"]);
}

#[test]
fn a_merge_that_fast_forwards_takes_along_the_changes_the_branch_does_not_touch() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.git(&["switch", "--quiet", "--create", "behind", "other~1"]);
    work.write("todo.txt", "first\nlater\n");
    work.git(&["add", "todo.txt"]);
    work.write("notes.txt", "one\ntwo\nthree\n");

    assert_eq!(
        gikli.merge_into_current(work.root(), &root, &local("other")),
        Err(Failure::MergeBlocked {
            folder: root.root.clone(),
            local_changes: path_bufs(&["notes.txt"]),
            untracked_files: Vec::new(),
        }),
        "a fast-forward checks other out, which the staged todo.txt does not block"
    );
    work.git(&["restore", "notes.txt"]);

    let in_progress = gikli
        .merge_into_current(work.root(), &root, &local("other"))
        .expect("fast-forward to other");

    assert_eq!(in_progress, None);
    assert_eq!(
        work.git(&["rev-parse", "HEAD"]),
        work.git(&["rev-parse", "other"])
    );
    assert_eq!(work.status_by_git(), vec!["M  todo.txt"]);
}

#[test]
fn a_tag_with_the_name_of_the_branch_does_not_get_in_the_way() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.git(&["tag", "other", "main~1"]);

    gikli
        .merge_into_current(work.root(), &root, &local("other"))
        .expect("merge the branch other");

    assert_eq!(
        work.git(&["rev-parse", "HEAD^2"]),
        work.git(&["rev-parse", "refs/heads/other"])
    );
}

#[test]
fn merge_into_current_merges_a_remote_branch() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.git(&["update-ref", "refs/remotes/origin/other", "other"]);
    let remote = RefLabel {
        name: "origin/other".to_owned(),
        kind: RefKind::Remote,
    };

    gikli
        .merge_into_current(work.root(), &root, &remote)
        .expect("merge origin/other");

    assert_eq!(
        work.git(&["log", "-1", "--format=%s"]).trim(),
        "Merge remote-tracking branch 'origin/other'"
    );
    assert_eq!(work.read("plan.txt"), "plan\n");
}

#[test]
fn a_branch_that_no_longer_exists_says_so() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);

    assert_eq!(
        gikli.merge_into_current(work.root(), &root, &local("gone")),
        Err(Failure::RevisionNotFound {
            revision: "gone".to_owned()
        })
    );
}

/// Asserts that the local changes of `change_notes_and_todo_in_feature` are back in `work`, onto
/// HEAD as it is, in their Changelists, with no stash left.
fn assert_local_changes_back(gikli: &Gikli, work: &TestRepository, root: &Repository) {
    assert_eq!(work.read("notes.txt"), "one\ntwo\nthree\n");
    assert_eq!(work.read("todo.txt"), "first\nsecond\nthird\n");
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M todo.txt"]);
    assert_eq!(work.stashes_by_git(), Vec::<String>::new());
    let changes = gikli.read_changes(root).expect("read changes");
    assert_eq!(paths_in(&changes, "Feature"), vec!["todo.txt"]);
}

#[test]
fn a_merge_a_hook_stops_keeps_the_local_changes_saved_until_it_is_aborted() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    change_notes_and_todo_in_feature(&gikli, &work, &root);
    sandbox.hook("pre-merge-commit", "exit 1");

    let failure = gikli.merge_over_local_changes(work.root(), &root, &local("other"));

    assert!(failure.is_err(), "the hook refused the merge commit");
    assert_eq!(
        gikli
            .operation_in_progress(work.root(), &root)
            .expect("read the operation in progress")
            .map(|operation| operation.kind),
        Some(OperationKind::Merge),
        "git leaves the merge to commit"
    );
    assert_eq!(
        work.stashes_by_git(),
        vec!["On main: Local changes saved by gikli"],
        "the changes do not come back onto the merge half done"
    );

    gikli
        .take_operation_action(work.root(), &root, OperationAction::Abort)
        .expect("abort the merge");

    assert_local_changes_back(&gikli, &work, &root);
}

#[test]
fn a_merge_ended_outside_gikli_brings_the_local_changes_back_once_gikli_reads_it_is_over() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    commit_conflicting_notes(&work);
    change_notes_and_todo_in_feature(&gikli, &work, &root);
    gikli
        .merge_over_local_changes(work.root(), &root, &local("other"))
        .expect("save and merge onto a conflict");
    work.git(&["merge", "--abort"]);

    let in_progress = gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");

    assert_eq!(in_progress, None);
    assert_local_changes_back(&gikli, &work, &root);
}

#[test]
fn a_merge_committed_from_the_commit_panel_brings_the_local_changes_back() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    commit_conflicting_notes(&work);
    change_notes_and_todo_in_feature(&gikli, &work, &root);
    gikli
        .merge_over_local_changes(work.root(), &root, &local("other"))
        .expect("save and merge onto a conflict");
    gikli
        .accept_side(
            work.root(),
            &root,
            &["notes.txt".to_owned()],
            ConflictSide::Yours,
        )
        .expect("keep my notes");
    gikli
        .commit_staged(&root, "Merge other")
        .expect("commit the merge");

    gikli
        .operation_in_progress(work.root(), &root)
        .expect("read the operation in progress");

    assert_eq!(
        work.git(&["rev-parse", "HEAD^2"]),
        work.git(&["rev-parse", "other"])
    );
    assert_local_changes_back(&gikli, &work, &root);
}

#[test]
fn with_merge_ff_false_a_staged_change_blocks_a_merge_that_could_fast_forward() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.git(&["switch", "--quiet", "--create", "behind", "other~1"]);
    work.git(&["config", "merge.ff", "false"]);
    work.write("todo.txt", "first\nlater\n");
    work.git(&["add", "todo.txt"]);

    assert_eq!(
        gikli.merge_into_current(work.root(), &root, &local("other")),
        Err(Failure::MergeBlocked {
            folder: root.root.clone(),
            local_changes: path_bufs(&["todo.txt"]),
            untracked_files: Vec::new(),
        })
    );
}

#[test]
fn with_merge_ff_only_a_merge_that_cannot_fast_forward_is_not_blocked_by_files() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    work.git(&["config", "merge.ff", "only"]);
    work.write("todo.txt", "first\nsecond\nthird\n");
    work.git(&["add", "todo.txt"]);

    assert_eq!(
        gikli.merge_into_current(work.root(), &root, &local("other")),
        Err(Failure::MergeFailed {
            folder: root.root.clone(),
        })
    );
}

#[test]
fn local_changes_that_conflict_with_the_merge_committed_stay_saved_and_open_conflict_resolution() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_other_branch(&sandbox, &gikli);
    commit_conflicting_notes(&work);
    change_notes_and_todo_in_feature(&gikli, &work, &root);
    gikli
        .merge_over_local_changes(work.root(), &root, &local("other"))
        .expect("save and merge onto a conflict");
    gikli
        .accept_side(
            work.root(),
            &root,
            &["notes.txt".to_owned()],
            ConflictSide::Theirs,
        )
        .expect("take the notes of other");
    let events = gikli.subscribe();

    gikli
        .take_operation_action(work.root(), &root, OperationAction::CommitMerge)
        .expect("commit the merge");

    assert_eq!(
        work.git(&["rev-parse", "HEAD^2"]),
        work.git(&["rev-parse", "other"])
    );
    assert_eq!(conflicts_left(&events), vec![root.clone()]);
    assert_eq!(conflicted(&gikli, &work, &root), vec!["notes.txt"]);
    assert_eq!(
        work.stashes_by_git(),
        vec!["On main: Local changes saved by gikli"],
        "the stash stays until the user drops it"
    );
    let changes = gikli.read_changes(&root).expect("read changes");
    assert_eq!(paths_in(&changes, "Feature"), vec!["todo.txt"]);
}
