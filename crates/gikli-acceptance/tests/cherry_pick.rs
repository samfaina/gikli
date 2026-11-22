//! Cherry-pick: the user brings commits they select in the log onto the current branch, in order.

use gikli_acceptance::{Sandbox, TestRepository};
use std::sync::mpsc::Receiver;

use gikli_core::{
    Event, Failure, Gikli, OperationAction, OperationInProgress, OperationKind, Repository,
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

/// The whole message of the last commit of the current branch of `repository`.
fn last_message(repository: &TestRepository) -> String {
    repository
        .git(&["log", "-1", "--format=%B"])
        .trim_end()
        .to_owned()
}

/// What the file at `path` of `repository` holds on disk.
fn read(repository: &TestRepository, path: &str) -> String {
    std::fs::read_to_string(repository.root().join(path)).expect("read the file")
}

/// A repository whose `main` holds `notes.txt`, and whose branch `other` went on with the commits
/// One, Two and Three, which each add a file of their own, opened as a Project on `main`.
fn with_three_on_other(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository) {
    let work = sandbox.repository("work");
    work.write("notes.txt", "notes\n");
    work.commit_all("Base");
    work.git(&["switch", "--quiet", "--create", "other"]);
    for name in ["One", "Two", "Three"] {
        work.write(
            &format!("{}.txt", name.to_lowercase()),
            &format!("{name}\n"),
        );
        work.commit_all(name);
    }
    work.git(&["switch", "--quiet", "main"]);
    let root = opened(gikli, &work);
    (work, root)
}

#[test]
fn cherry_picking_a_commit_brings_it_onto_the_current_branch() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_three_on_other(&sandbox, &gikli);
    let two = commit_of(&work, "other~1");

    let in_progress = gikli
        .cherry_pick(work.root(), &root, &[two])
        .expect("cherry-pick Two");

    assert_eq!(in_progress, None, "the cherry-pick is over");
    assert_eq!(subjects(&work), vec!["Two", "Base"]);
    assert_eq!(
        last_message(&work),
        "Two",
        "nothing is added to the message"
    );
    assert_eq!(read(&work, "two.txt"), "Two\n");
    assert!(work.status_by_git().is_empty());
}

#[test]
fn cherry_picking_several_commits_picks_them_in_the_order_given() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_three_on_other(&sandbox, &gikli);
    let selected = [commit_of(&work, "other~2"), commit_of(&work, "other")];

    let in_progress = gikli
        .cherry_pick(work.root(), &root, &selected)
        .expect("cherry-pick One and Three");

    assert_eq!(in_progress, None);
    assert_eq!(subjects(&work), vec!["Three", "One", "Base"]);
    assert!(!work.root().join("two.txt").exists(), "Two is not picked");
}

#[test]
fn a_commit_already_on_the_current_branch_is_not_cherry_picked() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_three_on_other(&sandbox, &gikli);
    let base = commit_of(&work, "main");
    let one = commit_of(&work, "other~2");

    let failure = gikli
        .cherry_pick(work.root(), &root, &[one, base.clone()])
        .expect_err("Base is on main already");

    assert_eq!(
        failure,
        Failure::CommitOnCurrentBranch {
            commit: base.clone()
        }
    );
    assert_eq!(commit_of(&work, "HEAD"), base, "nothing is picked");
    assert!(work.status_by_git().is_empty());
}

/// A clone whose `main` holds Base and Fix, both pushed to `main` of its remote, and whose branch
/// `release`, the current one, starts at Base. `feature`, pushed to a branch of the same name of
/// the remote, holds Feature on top of Fix.
fn release_of_a_clone(sandbox: &Sandbox) -> TestRepository {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.clone_of(&remote, "work");
    work.git(&["switch", "--quiet", "--create", "main"]);
    work.write("notes.txt", "notes\n");
    work.commit_all("Base");
    work.write("fix.txt", "fix\n");
    work.commit_all("Fix");
    work.git(&["push", "--quiet", "origin", "main"]);
    work.git(&["switch", "--quiet", "--create", "feature"]);
    work.write("feature.txt", "feature\n");
    work.commit_all("Feature");
    work.git(&["push", "--quiet", "origin", "feature"]);
    work.git(&["switch", "--quiet", "--create", "release", "main~1"]);
    work
}

#[test]
fn a_commit_from_a_protected_branch_is_cherry_picked_with_where_it_came_from() {
    let sandbox = Sandbox::new();
    let work = release_of_a_clone(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let fix = commit_of(&work, "main");

    gikli
        .cherry_pick(work.root(), &root, std::slice::from_ref(&fix))
        .expect("cherry-pick Fix");

    assert_eq!(
        last_message(&work),
        format!("Fix\n\n(cherry picked from commit {fix})")
    );
}

#[test]
fn a_commit_from_a_branch_that_is_not_protected_is_cherry_picked_as_it_is() {
    let sandbox = Sandbox::new();
    let work = release_of_a_clone(&sandbox);
    work.git(&["switch", "--quiet", "--create", "hotfix", "main"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .cherry_pick(work.root(), &root, &[commit_of(&work, "feature")])
        .expect("cherry-pick Feature");

    assert_eq!(last_message(&work), "Feature");
}

#[test]
fn where_a_commit_came_from_is_added_for_protected_branches_by_default() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();

    assert!(
        gikli
            .settings()
            .add_cherry_picked_from_for_protected_branches
    );
}

#[test]
fn with_the_setting_off_a_commit_from_a_protected_branch_is_cherry_picked_as_it_is() {
    let sandbox = Sandbox::new();
    let work = release_of_a_clone(&sandbox);
    let gikli = sandbox.gikli();
    let settings = gikli
        .set_add_cherry_picked_from_for_protected_branches(false)
        .expect("turn the setting off");
    assert!(!settings.add_cherry_picked_from_for_protected_branches);
    let root = opened(&gikli, &work);

    gikli
        .cherry_pick(work.root(), &root, &[commit_of(&work, "main")])
        .expect("cherry-pick Fix");

    assert_eq!(last_message(&work), "Fix");
    assert!(
        !sandbox
            .gikli()
            .settings()
            .add_cherry_picked_from_for_protected_branches,
        "the setting is saved"
    );
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

/// [`with_three_on_other`], where `main` went on with a commit that changes `notes.txt`, and so
/// does Two on `other`.
fn with_two_in_conflict(sandbox: &Sandbox, gikli: &Gikli) -> (TestRepository, Repository) {
    let work = sandbox.repository("work");
    work.write("notes.txt", "notes\n");
    work.commit_all("Base");
    work.git(&["switch", "--quiet", "--create", "other"]);
    work.write("one.txt", "One\n");
    work.commit_all("One");
    work.write("notes.txt", "theirs\n");
    work.commit_all("Two");
    work.write("three.txt", "Three\n");
    work.commit_all("Three");
    work.git(&["switch", "--quiet", "main"]);
    work.write("notes.txt", "ours\n");
    work.commit_all("Ours");
    let root = opened(gikli, &work);
    (work, root)
}

#[test]
fn a_cherry_pick_that_stops_on_a_conflict_shows_it_in_the_changes_view_and_is_in_progress() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_in_conflict(&sandbox, &gikli);
    let selected = [
        commit_of(&work, "other~2"),
        commit_of(&work, "other~1"),
        commit_of(&work, "other"),
    ];
    let events = gikli.subscribe();

    let in_progress = gikli
        .cherry_pick(work.root(), &root, &selected)
        .expect("the cherry-pick stops on a conflict");

    assert_eq!(
        in_progress,
        Some(OperationInProgress {
            kind: OperationKind::CherryPick,
            actions: vec![
                OperationAction::Continue,
                OperationAction::Skip,
                OperationAction::Abort
            ],
        }),
        "the status area offers to continue and abort it"
    );
    assert_eq!(conflicts_left(&events), vec![root.clone()]);
    let changes = gikli.read_changes(&root).expect("read the changes");
    let conflicted: Vec<_> = changes
        .conflicts
        .changes
        .iter()
        .map(|change| change.path.to_string_lossy().into_owned())
        .collect();
    assert_eq!(conflicted, vec!["notes.txt"]);
    assert_eq!(subjects(&work), vec!["One", "Ours", "Base"]);
}

#[test]
fn continuing_a_cherry_pick_stopped_on_a_conflict_picks_the_rest_in_order() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_in_conflict(&sandbox, &gikli);
    let selected = [
        commit_of(&work, "other~2"),
        commit_of(&work, "other~1"),
        commit_of(&work, "other"),
    ];
    gikli
        .cherry_pick(work.root(), &root, &selected)
        .expect("the cherry-pick stops on a conflict");
    work.write("notes.txt", "both\n");
    gikli
        .mark_resolved(work.root(), &root, &["notes.txt".to_owned()])
        .expect("mark notes.txt resolved");

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the cherry-pick");

    assert_eq!(in_progress, None);
    assert_eq!(subjects(&work), vec!["Three", "Two", "One", "Ours", "Base"]);
    assert_eq!(read(&work, "notes.txt"), "both\n");
}

#[test]
fn aborting_a_cherry_pick_stopped_on_a_conflict_brings_back_the_branch_as_it_was() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_two_in_conflict(&sandbox, &gikli);
    let before = commit_of(&work, "HEAD");
    gikli
        .cherry_pick(work.root(), &root, &[commit_of(&work, "other~1")])
        .expect("the cherry-pick stops on a conflict");

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Abort)
        .expect("abort the cherry-pick");

    assert_eq!(in_progress, None);
    assert_eq!(commit_of(&work, "HEAD"), before);
    assert!(work.status_by_git().is_empty());
}

#[test]
fn a_cherry_pick_that_stops_on_a_commit_left_empty_is_in_progress_to_skip_it() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_three_on_other(&sandbox, &gikli);
    // main makes the change of One on its own, so picking One leaves nothing to commit.
    work.write("one.txt", "One\n");
    work.commit_all("One on main");
    let selected = [commit_of(&work, "other~2"), commit_of(&work, "other~1")];

    let in_progress = gikli
        .cherry_pick(work.root(), &root, &selected)
        .expect("the cherry-pick stops on One");

    assert_eq!(
        in_progress.map(|operation| operation.kind),
        Some(OperationKind::CherryPick)
    );
    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Skip)
        .expect("skip One");
    assert_eq!(in_progress, None);
    assert_eq!(subjects(&work), vec!["Two", "One on main", "Base"]);
}

#[test]
fn a_cherry_pick_does_not_start_while_another_operation_is_in_progress() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let (work, root) = with_three_on_other(&sandbox, &gikli);
    work.write("one.txt", "One\n");
    work.commit_all("One on main");
    // Picking One leaves nothing to commit, so git stops on it with no file in conflict.
    assert!(!work.git_succeeds(&["cherry-pick", "other~2"]));
    let head = commit_of(&work, "HEAD");

    let failure = gikli
        .cherry_pick(work.root(), &root, &[commit_of(&work, "other")])
        .expect_err("a cherry-pick is half done");

    assert_eq!(
        failure,
        Failure::OperationAlreadyInProgress {
            operation: OperationKind::CherryPick
        }
    );
    assert_eq!(commit_of(&work, "HEAD"), head);
    assert!(
        !work.root().join("three.txt").exists(),
        "Three is not picked"
    );
}

/// [`release_of_a_clone`], where `release` went on with a commit that adds `fix.txt` too, so
/// picking Fix stops on a conflict.
fn release_in_conflict_with_fix(sandbox: &Sandbox) -> TestRepository {
    let work = release_of_a_clone(sandbox);
    work.write("fix.txt", "ours\n");
    work.commit_all("Ours");
    work
}

#[test]
fn the_commits_left_after_a_stop_in_a_selection_from_protected_and_other_branches_are_picked_on_continue()
 {
    let sandbox = Sandbox::new();
    let work = release_in_conflict_with_fix(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let fix = commit_of(&work, "main");
    let selected = [fix.clone(), commit_of(&work, "feature")];
    let in_progress = gikli
        .cherry_pick(work.root(), &root, &selected)
        .expect("the cherry-pick stops on a conflict at Fix");
    assert_eq!(
        in_progress.map(|operation| operation.kind),
        Some(OperationKind::CherryPick)
    );
    work.write("fix.txt", "both\n");
    gikli
        .mark_resolved(work.root(), &root, &["fix.txt".to_owned()])
        .expect("mark fix.txt resolved");

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the cherry-pick");

    assert_eq!(in_progress, None);
    assert_eq!(subjects(&work), vec!["Feature", "Fix", "Ours", "Base"]);
    assert_eq!(last_message(&work), "Feature");
    assert_eq!(
        work.git(&["log", "-1", "--format=%B", "HEAD~1"]).trim_end(),
        format!("Fix\n\n(cherry picked from commit {fix})")
    );
}

#[test]
fn aborting_a_cherry_pick_gives_up_the_commits_left_to_pick() {
    let sandbox = Sandbox::new();
    let work = release_in_conflict_with_fix(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let ours = commit_of(&work, "HEAD");
    let selected = [commit_of(&work, "main"), commit_of(&work, "feature")];
    gikli
        .cherry_pick(work.root(), &root, &selected)
        .expect("the cherry-pick stops on a conflict at Fix");
    gikli
        .take_operation_action(work.root(), &root, OperationAction::Abort)
        .expect("abort the cherry-pick");
    // A cherry-pick the user starts outside gikli and continues from gikli later.
    assert!(!work.git_succeeds(&["cherry-pick", "main"]));
    work.write("fix.txt", "both\n");
    work.git(&["add", "fix.txt"]);

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the cherry-pick");

    assert_eq!(in_progress, None);
    assert_eq!(
        subjects(&work),
        vec!["Fix", "Ours", "Base"],
        "Feature is not picked"
    );
    assert_eq!(commit_of(&work, "HEAD~1"), ours);
}

/// A clone whose `main` holds Base, then Fix, which adds `fix.txt`, then More, which adds
/// `more.txt`, all pushed to `main` of its remote, and whose branch `release`, the current one,
/// starts at Base. `feature` holds Feature on top of More.
fn release_before_fix_and_more(sandbox: &Sandbox) -> TestRepository {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.clone_of(&remote, "work");
    work.git(&["switch", "--quiet", "--create", "main"]);
    work.write("notes.txt", "notes\n");
    work.commit_all("Base");
    work.write("fix.txt", "fix\n");
    work.commit_all("Fix");
    work.write("more.txt", "more\n");
    work.commit_all("More");
    work.git(&["push", "--quiet", "origin", "main"]);
    work.git(&["switch", "--quiet", "--create", "feature"]);
    work.write("feature.txt", "feature\n");
    work.commit_all("Feature");
    work.git(&["switch", "--quiet", "--create", "release", "main~2"]);
    work
}

/// Fix, More and Feature of [`release_before_fix_and_more`], in that order. Fix and More, from a
/// protected branch, go in a run of their own, and Feature in the one after it.
fn fix_more_and_feature(work: &TestRepository) -> [String; 3] {
    [
        commit_of(work, "main~1"),
        commit_of(work, "main"),
        commit_of(work, "feature"),
    ]
}

#[test]
fn the_commits_left_are_picked_once_a_cherry_pick_that_stopped_again_on_continue_is_over() {
    let sandbox = Sandbox::new();
    let work = release_before_fix_and_more(&sandbox);
    work.write("fix.txt", "ours\n");
    work.write("more.txt", "ours\n");
    work.commit_all("Ours");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .cherry_pick(work.root(), &root, &fix_more_and_feature(&work))
        .expect("the cherry-pick stops on a conflict at Fix");
    work.write("fix.txt", "both\n");
    gikli
        .mark_resolved(work.root(), &root, &["fix.txt".to_owned()])
        .expect("mark fix.txt resolved");
    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the cherry-pick, which stops on a conflict at More");
    assert_eq!(
        in_progress.map(|operation| operation.kind),
        Some(OperationKind::CherryPick)
    );
    work.write("more.txt", "both\n");
    gikli
        .mark_resolved(work.root(), &root, &["more.txt".to_owned()])
        .expect("mark more.txt resolved");

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the cherry-pick");

    assert_eq!(in_progress, None);
    assert_eq!(
        subjects(&work),
        vec!["Feature", "More", "Fix", "Ours", "Base"]
    );
}

#[test]
fn the_commits_left_are_picked_on_continue_after_committing_what_is_staged_in_the_middle_of_a_run()
{
    let sandbox = Sandbox::new();
    let work = release_before_fix_and_more(&sandbox);
    work.write("fix.txt", "ours\n");
    work.commit_all("Ours");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .cherry_pick(work.root(), &root, &fix_more_and_feature(&work))
        .expect("the cherry-pick stops on a conflict at Fix");
    work.write("fix.txt", "both\n");
    gikli
        .mark_resolved(work.root(), &root, &["fix.txt".to_owned()])
        .expect("mark fix.txt resolved");
    // git takes CHERRY_PICK_HEAD away with the commit, and has More left to pick.
    gikli
        .commit_staged(&root, "Fix")
        .expect("commit Fix from the commit panel in Staging mode");

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the cherry-pick with More");

    assert_eq!(in_progress, None);
    assert_eq!(
        subjects(&work),
        vec!["Feature", "More", "Fix", "Ours", "Base"]
    );
}

#[test]
fn the_commits_left_are_picked_on_continue_after_committing_the_files_in_the_middle_of_a_run() {
    let sandbox = Sandbox::new();
    let work = release_before_fix_and_more(&sandbox);
    work.write("fix.txt", "ours\n");
    work.commit_all("Ours");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .cherry_pick(work.root(), &root, &fix_more_and_feature(&work))
        .expect("the cherry-pick stops on a conflict at Fix");
    work.write("fix.txt", "both\n");
    gikli
        .mark_resolved(work.root(), &root, &["fix.txt".to_owned()])
        .expect("mark fix.txt resolved");
    gikli
        .commit_files(&root, &["fix.txt".to_owned()], "Fix")
        .expect("commit Fix from the commit panel in Changelist mode");

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the cherry-pick with More");

    assert_eq!(in_progress, None);
    assert_eq!(
        subjects(&work),
        vec!["Feature", "More", "Fix", "Ours", "Base"]
    );
}

#[test]
fn a_cherry_pick_started_outside_gikli_after_its_own_was_aborted_picks_none_of_the_commits_left() {
    let sandbox = Sandbox::new();
    let work = release_in_conflict_with_fix(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let selected = [commit_of(&work, "main"), commit_of(&work, "feature")];
    gikli
        .cherry_pick(work.root(), &root, &selected)
        .expect("the cherry-pick stops on a conflict at Fix, with Feature left to pick");
    // Outside gikli, the user aborts it, and starts another one on another HEAD.
    work.git(&["cherry-pick", "--abort"]);
    work.write("other.txt", "other\n");
    work.commit_all("Other");
    assert!(!work.git_succeeds(&["cherry-pick", "main"]));
    work.write("fix.txt", "both\n");
    work.git(&["add", "fix.txt"]);

    let in_progress = gikli
        .take_operation_action(work.root(), &root, OperationAction::Continue)
        .expect("continue the cherry-pick");

    assert_eq!(in_progress, None);
    assert_eq!(
        subjects(&work),
        vec!["Fix", "Other", "Ours", "Base"],
        "Feature is not picked"
    );
}
