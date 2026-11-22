//! Committing from the commit panel in Changelist mode while an Operation in progress is there: the
//! selected changes go through the index, as git refuses `git commit --only` during a merge or a
//! cherry-pick.

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{ChangeSet, Failure, Gikli, MoveTarget, OperationKind, Repository};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

fn paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

/// The paths the last commit touched, one `status path` entry each.
fn last_commit_files(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["show", "--format=", "--name-status", "--no-renames", "HEAD"])
        .lines()
        .map(|line| line.replace('\t', " "))
        .collect()
}

/// The kind of the Operation in progress in `repository`, if any.
fn in_progress(
    gikli: &Gikli,
    work: &TestRepository,
    repository: &Repository,
) -> Option<OperationKind> {
    gikli
        .operation_in_progress(work.root(), repository)
        .expect("read the operation in progress")
        .map(|operation| operation.kind)
}

/// A repository whose `main` holds `notes.txt`, and whose branch `other` changes it and adds
/// `plan.txt` in its last commit, while `main` changed `notes.txt` too. A cherry-pick of `other`
/// onto `main` stopped on the conflict in `notes.txt`, which the user resolved.
fn cherry_pick_resolved(sandbox: &Sandbox) -> TestRepository {
    let work = sandbox.repository("work");
    work.write("notes.txt", "notes\n");
    work.write("todo.txt", "todo\n");
    work.commit_all("Base");
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.write("notes.txt", "theirs\n");
    work.write("plan.txt", "plan\n");
    work.commit_all("Theirs");
    work.git(&["switch", "--quiet", "main"]);
    work.write("notes.txt", "ours\n");
    work.commit_all("Ours");
    assert!(
        !work.git_succeeds(&["cherry-pick", "other"]),
        "the cherry-pick stops on a conflict"
    );
    work.write("notes.txt", "both\n");
    work.git(&["add", "--", "notes.txt"]);
    work
}

#[test]
fn committing_the_files_of_a_cherry_pick_stopped_on_a_resolved_conflict_concludes_it() {
    let sandbox = Sandbox::new();
    let work = cherry_pick_resolved(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .commit_files(&root, &paths(&["notes.txt", "plan.txt"]), "Theirs")
        .expect("commit");

    assert_eq!(last_commit_files(&work), vec!["M notes.txt", "A plan.txt"]);
    assert_eq!(work.git(&["show", "HEAD:notes.txt"]), "both\n");
    assert_eq!(in_progress(&gikli, &work, &root), None);
    assert!(work.status_by_git().is_empty());
}

/// A repository whose `main` holds `notes.txt` and `todo.txt`, and whose branch `other` changes
/// `notes.txt` and adds `plan.txt`, while `main` changed `notes.txt` too. A merge of `other` into
/// `main` stopped on the conflict in `notes.txt`, which the user resolved.
fn merge_resolved(sandbox: &Sandbox) -> TestRepository {
    let work = sandbox.repository("work");
    work.write("notes.txt", "notes\n");
    work.write("todo.txt", "todo\n");
    work.commit_all("Base");
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.write("notes.txt", "theirs\n");
    work.write("plan.txt", "plan\n");
    work.commit_all("Theirs");
    work.git(&["switch", "--quiet", "main"]);
    work.write("notes.txt", "ours\n");
    work.commit_all("Ours");
    assert!(
        !work.git_succeeds(&["merge", "--no-edit", "other"]),
        "the merge stops on a conflict"
    );
    work.write("notes.txt", "both\n");
    work.git(&["add", "--", "notes.txt"]);
    work
}

#[test]
fn committing_the_files_of_a_merge_stopped_on_a_resolved_conflict_makes_the_merge_commit() {
    let sandbox = Sandbox::new();
    let work = merge_resolved(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .commit_files(&root, &paths(&["notes.txt", "plan.txt"]), "Merge other")
        .expect("commit");

    assert_eq!(
        work.git(&["rev-parse", "HEAD^1", "HEAD^2"]),
        work.git(&["rev-parse", "main@{1}", "other"]),
        "the commit has main as it was and other as its parents"
    );
    assert_eq!(work.git(&["show", "HEAD:notes.txt"]), "both\n");
    assert_eq!(work.git(&["show", "HEAD:plan.txt"]), "plan\n");
    assert_eq!(in_progress(&gikli, &work, &root), None);
    assert!(work.status_by_git().is_empty());
}

/// The paths in the Changelist called `name`.
fn changelist_paths(changes: &ChangeSet, name: &str) -> Vec<String> {
    changes
        .changelists
        .iter()
        .find(|list| list.name == name)
        .expect("the changelist is there")
        .group
        .changes
        .iter()
        .map(|change| change.path.to_string_lossy().into_owned())
        .collect()
}

/// A Changelist called Other holding `todo.txt` of `work`, which has a local edit.
fn todo_edited_in_its_own_changelist(gikli: &Gikli, work: &TestRepository, root: &Repository) {
    work.write("todo.txt", "todo, edited\n");
    let changes = gikli
        .create_changelist(root, "Other", None)
        .expect("create Other");
    let other = changes
        .changelists
        .iter()
        .find(|list| list.name == "Other")
        .expect("Other is there")
        .id
        .clone();
    gikli
        .move_changes(
            root,
            &paths(&["todo.txt"]),
            &MoveTarget::Changelist { id: other },
            false,
        )
        .expect("move todo.txt to Other");
}

#[test]
fn an_unticked_local_edit_stays_out_of_the_merge_commit_and_in_its_changelist() {
    let sandbox = Sandbox::new();
    let work = merge_resolved(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    todo_edited_in_its_own_changelist(&gikli, &work, &root);

    let changes = gikli
        .commit_files(&root, &paths(&["notes.txt", "plan.txt"]), "Merge other")
        .expect("commit");

    assert_eq!(work.git(&["show", "HEAD:todo.txt"]), "todo\n");
    assert_eq!(work.status_by_git(), vec![" M todo.txt"]);
    assert_eq!(changelist_paths(&changes, "Other"), vec!["todo.txt"]);
}

#[test]
fn an_unticked_local_edit_stays_out_of_the_commit_of_a_cherry_pick_and_in_its_changelist() {
    let sandbox = Sandbox::new();
    let work = cherry_pick_resolved(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    todo_edited_in_its_own_changelist(&gikli, &work, &root);

    let changes = gikli
        .commit_files(&root, &paths(&["notes.txt", "plan.txt"]), "Theirs")
        .expect("commit");

    assert_eq!(last_commit_files(&work), vec!["M notes.txt", "A plan.txt"]);
    assert_eq!(work.status_by_git(), vec![" M todo.txt"]);
    assert_eq!(changelist_paths(&changes, "Other"), vec!["todo.txt"]);
    assert_eq!(in_progress(&gikli, &work, &root), None);
}

#[test]
fn an_unticked_file_a_cherry_pick_brought_stays_out_of_the_commit_as_a_local_change() {
    let sandbox = Sandbox::new();
    let work = cherry_pick_resolved(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let changes = gikli
        .commit_files(&root, &paths(&["notes.txt"]), "Theirs, without the plan")
        .expect("commit");

    assert_eq!(last_commit_files(&work), vec!["M notes.txt"]);
    assert_eq!(work.status_by_git(), vec!["A  plan.txt"]);
    let left: Vec<_> = changes.changes.iter().map(|change| &change.path).collect();
    assert_eq!(left, vec![std::path::Path::new("plan.txt")]);
    assert_eq!(in_progress(&gikli, &work, &root), None);
}

#[test]
fn a_conflict_left_refuses_the_commit_and_changes_nothing() {
    let sandbox = Sandbox::new();
    let work = cherry_pick_resolved(&sandbox);
    let gikli = sandbox.gikli();
    // Back in conflict, as the cherry-pick left it.
    work.git(&["checkout", "--merge", "--", "notes.txt"]);
    let root = opened(&gikli, &work);
    let status_before = work.status_by_git();

    let result = gikli.commit_files(&root, &paths(&["plan.txt"]), "Only the plan");

    assert!(
        matches!(result, Err(Failure::ConflictsNotResolved)),
        "the conflict is resolved first: {result:?}"
    );
    assert_eq!(work.git(&["log", "-1", "--format=%s"]), "Ours\n");
    assert_eq!(work.status_by_git(), status_before);
    assert_eq!(
        in_progress(&gikli, &work, &root),
        Some(OperationKind::CherryPick)
    );
}
