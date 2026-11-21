//! Merge into current: the user merges a branch into the one HEAD is on from the branch selector.
//! A merge that stops on a conflict leads to the conflict list, and one that local changes block
//! says which, for the user to save them, merge and restore them through Saved local changes.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    ChangeSet, ConflictSide, Event, Failure, Gikli, MoveTarget, OperationAction, OperationKind,
    RefKind, RefLabel, Repository,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

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

/// What the file at `path` of `repository` holds on disk.
fn read(repository: &TestRepository, path: &str) -> String {
    std::fs::read_to_string(repository.root().join(path)).expect("read the file")
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
    assert_eq!(read(&work, "notes.txt"), "zero\none\ntwo\n");
    assert_eq!(read(&work, "todo.txt"), "first\nsecond\n");
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
fn a_merge_that_stops_on_a_conflict_leads_to_the_conflict_list() {
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

/// `paths`, relative to the Repository root, as the core names files.
fn paths(paths: &[&str]) -> Vec<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
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
            local_changes: paths(&["notes.txt"]),
            untracked_files: Vec::new(),
        },
        "other does not change todo.txt, so it does not block"
    );
    assert_eq!(work.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M todo.txt"]);
}

/// The stashes of `repository`, as plain git lists them.
fn stashes_by_git(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["stash", "list", "--format=%gs"])
        .lines()
        .map(str::to_owned)
        .collect()
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
    assert_eq!(read(&work, "notes.txt"), "zero\none\ntwo\nthree\n");
    assert_eq!(read(&work, "todo.txt"), "first\nsecond\nthird\n");
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M todo.txt"]);
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
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
        stashes_by_git(&work),
        vec!["On main: Local changes saved by gikli"]
    );
    assert_eq!(
        read(&work, "todo.txt"),
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
    assert_eq!(read(&work, "notes.txt"), "one\ntwo\nthree\n");
    assert_eq!(read(&work, "todo.txt"), "first\nsecond\nthird\n");
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M todo.txt"]);
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
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
    assert_eq!(read(&work, "notes.txt"), "one\ntwo\nthree\n");
    assert_eq!(read(&work, "todo.txt"), "first\nsecond\nthird\n");
    assert_eq!(work.status_by_git(), vec![" M notes.txt", " M todo.txt"]);
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
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
        local_changes: paths(local_changes),
        untracked_files: paths(&["plan.txt"]),
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
    assert_eq!(read(&work, "notes.txt"), "one\ntwo\nthree\n");
    assert_eq!(read(&work, "plan.txt"), "my own plan\n");
    assert_eq!(
        work.status_by_git(),
        vec![" M notes.txt", "?? plan.txt", "?? scratch.txt"]
    );
    assert_eq!(stashes_by_git(&work), Vec::<String>::new());
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
            local_changes: paths(&["todo.txt"]),
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
            local_changes: paths(&["notes.txt"]),
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
    assert_eq!(read(&work, "plan.txt"), "plan\n");
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
