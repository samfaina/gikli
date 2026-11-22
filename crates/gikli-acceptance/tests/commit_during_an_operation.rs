//! Committing from the commit panel in Changelist mode while an Operation in progress is there: the
//! selected changes go through the index, as git refuses `git commit --only` during a merge or a
//! cherry-pick. An amend in the middle of either, or of a rebase that stopped on a merge, is refused
//! before git runs, as git refuses it.

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{ChangeSet, Changelist, Failure, Gikli, MoveTarget, OperationKind, Repository};

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

/// A repository whose `main` holds `notes.txt` and `todo.txt`, and whose branch `other` changes
/// `notes.txt` and adds `plan.txt` in one commit, while `main` changed `notes.txt` too. git, run
/// with `operation` on `other` from `main`, stopped on the conflict in `notes.txt`, which the user
/// resolved.
fn resolved_after(sandbox: &Sandbox, operation: &[&str]) -> TestRepository {
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
    let mut args = operation.to_vec();
    args.push("other");
    assert!(!work.git_succeeds(&args), "git stops on a conflict");
    work.write("notes.txt", "both\n");
    work.git(&["add", "--", "notes.txt"]);
    work
}

/// [`resolved_after`] a cherry-pick.
fn cherry_pick_resolved(sandbox: &Sandbox) -> TestRepository {
    resolved_after(sandbox, &["cherry-pick"])
}

/// [`resolved_after`] a merge.
fn merge_resolved(sandbox: &Sandbox) -> TestRepository {
    resolved_after(sandbox, &["merge", "--no-edit"])
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

/// The Changelist called `name`.
fn list<'a>(change_set: &'a ChangeSet, name: &str) -> &'a Changelist {
    change_set
        .changelists
        .iter()
        .find(|list| list.name == name)
        .expect("the Changelist exists")
}

/// The paths of the changes in the Changelist called `name`.
fn paths_in(change_set: &ChangeSet, name: &str) -> Vec<String> {
    list(change_set, name)
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
    let other = list(&changes, "Other").id.clone();
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
    assert_eq!(paths_in(&changes, "Other"), vec!["todo.txt"]);
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
    assert_eq!(paths_in(&changes, "Other"), vec!["todo.txt"]);
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

/// Each Backup git holds, as `reference commit`.
fn backups_by_git(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/gikli/backups/",
        ])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// What an amend refused during `operation` must leave as it was in `work`: HEAD, the status of
/// every file, no Backup, no notification, and `operation` still in progress.
fn assert_refused_during(
    result: Result<ChangeSet, Failure>,
    operation: OperationKind,
    gikli: &Gikli,
    work: &TestRepository,
    root: &Repository,
    head_before: &str,
    status_before: &[String],
) {
    assert_eq!(
        result.map(|_| ()),
        Err(Failure::AmendDuringOperation { operation })
    );
    assert_eq!(work.git(&["rev-parse", "HEAD"]), head_before);
    assert_eq!(work.status_by_git(), status_before);
    assert_eq!(backups_by_git(work), Vec::<String>::new());
    assert_eq!(gikli.notifications(), vec![]);
    assert_eq!(in_progress(gikli, work, root), Some(operation));
}

#[test]
fn an_amend_of_the_files_of_a_merge_is_refused_and_changes_nothing() {
    let sandbox = Sandbox::new();
    let work = merge_resolved(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let head_before = work.git(&["rev-parse", "HEAD"]);
    let status_before = work.status_by_git();

    let result = gikli.amend_last_commit(
        work.root(),
        &root,
        &paths(&["notes.txt", "plan.txt"]),
        "Ours, and the merge",
    );

    assert_refused_during(
        result,
        OperationKind::Merge,
        &gikli,
        &work,
        &root,
        &head_before,
        &status_before,
    );
}

#[test]
fn an_amend_of_the_files_of_a_cherry_pick_is_refused_and_changes_nothing() {
    let sandbox = Sandbox::new();
    let work = cherry_pick_resolved(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let head_before = work.git(&["rev-parse", "HEAD"]);
    let status_before = work.status_by_git();

    let result = gikli.amend_last_commit(
        work.root(),
        &root,
        &paths(&["notes.txt", "plan.txt"]),
        "Ours, and theirs",
    );

    assert_refused_during(
        result,
        OperationKind::CherryPick,
        &gikli,
        &work,
        &root,
        &head_before,
        &status_before,
    );
}

/// A gikli in Staging mode.
fn in_staging_mode(sandbox: &Sandbox) -> Gikli {
    let gikli = sandbox.gikli();
    gikli.set_staging_mode(true).expect("turn on Staging mode");
    gikli
}

/// Amends with what is staged in `work`, while `operation` is in progress there, and checks that
/// it is refused and changes nothing.
fn assert_staged_amend_refused(sandbox: &Sandbox, work: &TestRepository, operation: OperationKind) {
    let gikli = in_staging_mode(sandbox);
    let root = opened(&gikli, work);
    let head_before = work.git(&["rev-parse", "HEAD"]);
    let status_before = work.status_by_git();

    let result = gikli.amend_with_staged(work.root(), &root, "Amended");

    assert_refused_during(
        result,
        operation,
        &gikli,
        work,
        &root,
        &head_before,
        &status_before,
    );
}

#[test]
fn an_amend_with_what_a_merge_staged_is_refused_and_changes_nothing() {
    let sandbox = Sandbox::new();
    let work = merge_resolved(&sandbox);

    assert_staged_amend_refused(&sandbox, &work, OperationKind::Merge);
}

#[test]
fn an_amend_with_what_a_cherry_pick_staged_is_refused_and_changes_nothing() {
    let sandbox = Sandbox::new();
    let work = cherry_pick_resolved(&sandbox);

    assert_staged_amend_refused(&sandbox, &work, OperationKind::CherryPick);
}

#[test]
fn an_amend_during_a_merge_that_staged_nothing_is_refused_for_the_merge() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.write("notes.txt", "notes\n");
    work.commit_all("Base");
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.write("notes.txt", "theirs\n");
    work.commit_all("Theirs");
    work.git(&["switch", "--quiet", "main"]);
    // Kept as main has it, so the merge stages nothing.
    work.git(&[
        "merge",
        "--quiet",
        "--no-commit",
        "--strategy=ours",
        "other",
    ]);
    assert!(work.status_by_git().is_empty());

    assert_staged_amend_refused(&sandbox, &work, OperationKind::Merge);
}

#[test]
fn an_amend_during_a_merge_is_refused_before_a_conflict_left_is_looked_at() {
    let sandbox = Sandbox::new();
    let work = merge_resolved(&sandbox);
    // Back in conflict, as the merge left it.
    work.git(&["checkout", "--merge", "--", "notes.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let head_before = work.git(&["rev-parse", "HEAD"]);
    let status_before = work.status_by_git();

    let result = gikli.amend_last_commit(work.root(), &root, &paths(&["plan.txt"]), "Ours");

    assert_refused_during(
        result,
        OperationKind::Merge,
        &gikli,
        &work,
        &root,
        &head_before,
        &status_before,
    );
}

/// The subject of the last commit.
fn last_subject(repository: &TestRepository) -> String {
    repository.git(&["log", "-1", "--format=%s"])
}

/// How many commits HEAD holds, as plain git counts them.
fn commit_count(repository: &TestRepository) -> String {
    repository.git(&["rev-list", "--count", "HEAD"])
}

#[test]
fn an_amend_during_a_rebase_stopped_at_a_commit_to_edit_amends_that_commit() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.write("notes.txt", "notes\n");
    work.commit_all("Base");
    work.write("notes.txt", "edited\n");
    work.commit_all("Edit");
    work.git(&[
        "-c",
        "sequence.editor=sed -i -e s/^pick/edit/",
        "rebase",
        "--quiet",
        "--interactive",
        "HEAD~1",
    ]);
    work.write("todo.txt", "todo\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .amend_last_commit(
            work.root(),
            &root,
            &paths(&["todo.txt"]),
            "Edit, with a todo",
        )
        .expect("amend");

    assert_eq!(commit_count(&work), "2\n");
    assert_eq!(last_subject(&work), "Edit, with a todo\n");
    assert_eq!(last_commit_files(&work), vec!["M notes.txt", "A todo.txt"]);
    assert_eq!(
        in_progress(&gikli, &work, &root),
        Some(OperationKind::Rebase)
    );
}

/// A repository whose `main` holds `Base`, then `Main` and `Side`, which both change `f.txt`, and a
/// merge of `Side` resolved by hand. git, run with `--rebase-merges` from `main` onto a branch with
/// an unrelated commit on `Base`, replayed `Main` and `Side` and stopped on the conflict of the
/// merge in `f.txt`, which the user resolved.
fn rebase_stopped_on_a_merge_resolved(sandbox: &Sandbox) -> TestRepository {
    let work = sandbox.repository("work");
    work.write("f.txt", "base\n");
    work.commit_all("Base");
    work.git(&["switch", "--quiet", "--create", "side"]);
    work.write("f.txt", "side\n");
    work.commit_all("Side");
    work.git(&["switch", "--quiet", "main"]);
    work.write("f.txt", "main\n");
    work.commit_all("Main");
    assert!(
        !work.git_succeeds(&["merge", "--no-edit", "side"]),
        "git stops on a conflict"
    );
    work.write("f.txt", "both\n");
    work.commit_all("Merge side");
    work.git(&["switch", "--quiet", "--create", "onto", "HEAD~2"]);
    work.write("other.txt", "other\n");
    work.commit_all("Unrelated");
    work.git(&["switch", "--quiet", "main"]);
    assert!(
        !work.git_succeeds(&["rebase", "--quiet", "--rebase-merges", "onto"]),
        "git stops on the conflict of the merge"
    );
    work.write("f.txt", "both\n");
    work.git(&["add", "--", "f.txt"]);
    work
}

#[test]
fn an_amend_of_the_files_of_a_rebase_stopped_on_a_merge_is_refused_and_changes_nothing() {
    let sandbox = Sandbox::new();
    let work = rebase_stopped_on_a_merge_resolved(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let head_before = work.git(&["rev-parse", "HEAD"]);
    let status_before = work.status_by_git();

    let result = gikli.amend_last_commit(
        work.root(),
        &root,
        &paths(&["f.txt"]),
        "Main, and the merge",
    );

    assert_refused_during(
        result,
        OperationKind::Rebase,
        &gikli,
        &work,
        &root,
        &head_before,
        &status_before,
    );
}

#[test]
fn an_amend_with_what_a_rebase_stopped_on_a_merge_staged_is_refused_and_changes_nothing() {
    let sandbox = Sandbox::new();
    let work = rebase_stopped_on_a_merge_resolved(&sandbox);

    assert_staged_amend_refused(&sandbox, &work, OperationKind::Rebase);
}

#[test]
fn an_amend_during_a_rebase_stopped_on_a_resolved_conflict_of_a_pick_amends_the_last_commit() {
    let sandbox = Sandbox::new();
    let work = resolved_after(&sandbox, &["rebase", "--quiet"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .amend_last_commit(
            work.root(),
            &root,
            &paths(&["notes.txt"]),
            "Theirs, with ours",
        )
        .expect("amend");

    assert_eq!(commit_count(&work), "2\n");
    assert_eq!(last_subject(&work), "Theirs, with ours\n");
    assert_eq!(work.git(&["show", "HEAD:notes.txt"]), "both\n");
    assert_eq!(
        in_progress(&gikli, &work, &root),
        Some(OperationKind::Rebase)
    );
}

#[test]
fn an_amend_during_a_revert_stopped_on_a_resolved_conflict_goes_through() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.write("notes.txt", "notes\n");
    work.commit_all("Base");
    work.write("notes.txt", "changed\n");
    work.commit_all("Change");
    work.write("notes.txt", "changed again\n");
    work.commit_all("Change again");
    assert!(
        !work.git_succeeds(&["revert", "--no-edit", "HEAD~1"]),
        "git stops on a conflict"
    );
    work.write("notes.txt", "notes again\n");
    work.git(&["add", "--", "notes.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .amend_last_commit(
            work.root(),
            &root,
            &paths(&["notes.txt"]),
            "Change again, undone",
        )
        .expect("amend");

    assert_eq!(commit_count(&work), "3\n");
    assert_eq!(last_subject(&work), "Change again, undone\n");
    assert_eq!(work.git(&["show", "HEAD:notes.txt"]), "notes again\n");
}

#[test]
fn an_amend_during_a_cherry_pick_of_several_goes_through_once_the_one_it_stopped_on_is_committed() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.write("notes.txt", "notes\n");
    work.commit_all("Base");
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.write("notes.txt", "theirs\n");
    work.commit_all("Theirs");
    work.write("plan.txt", "plan\n");
    work.commit_all("Plan");
    work.git(&["switch", "--quiet", "main"]);
    work.write("notes.txt", "ours\n");
    work.commit_all("Ours");
    assert!(
        !work.git_succeeds(&["cherry-pick", "other~1", "other"]),
        "git stops on a conflict"
    );
    work.write("notes.txt", "both\n");
    work.git(&["add", "--", "notes.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .commit_files(&root, &paths(&["notes.txt"]), "Theirs")
        .expect("commit the one it stopped on");
    work.write("notes.txt", "both, fixed\n");

    gikli
        .amend_last_commit(work.root(), &root, &paths(&["notes.txt"]), "Theirs, fixed")
        .expect("amend");

    assert_eq!(commit_count(&work), "3\n");
    assert_eq!(last_subject(&work), "Theirs, fixed\n");
    assert_eq!(work.git(&["show", "HEAD:notes.txt"]), "both, fixed\n");
    assert_eq!(
        in_progress(&gikli, &work, &root),
        Some(OperationKind::CherryPick),
        "the cherry-pick still has Plan to pick"
    );
}
