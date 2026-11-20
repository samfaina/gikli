//! Amending: the user fixes the last commit from the commit panel.

use std::path::Path;

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    Change, Failure, Gikli, NotificationAction, NotificationKind, Repository, Rewrite,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A repository with two commits: the first holds `a.txt` and `b.txt`, the last changes `a.txt`.
fn repository_with_a_last_commit(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.write("b.txt", "b\n");
    repository.commit_all("Initial commit");
    repository.write("a.txt", "a changed\n");
    repository.commit_all("Change a");
    repository
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

fn commit_of(repository: &TestRepository, revision: &str) -> String {
    repository
        .git(&["rev-parse", "--verify", revision])
        .trim_end()
        .to_owned()
}

fn paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

fn paths_of(changes: &[Change]) -> Vec<&Path> {
    changes.iter().map(|change| change.path.as_path()).collect()
}

/// The message of the last commit, as git stores it.
fn last_message(repository: &TestRepository) -> String {
    repository.git(&["log", "-1", "--format=%B"])
}

fn commit_count(repository: &TestRepository) -> String {
    repository.git(&["rev-list", "--count", "HEAD"])
}

/// The paths the last commit touched, one `status path` entry each.
fn last_commit_files(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["show", "--format=", "--name-status", "--no-renames", "HEAD"])
        .lines()
        .map(|line| line.replace('\t', " "))
        .collect()
}

#[test]
fn a_selected_file_goes_into_the_last_commit_and_the_others_stay_changed() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    repository.write("b.txt", "b changed\n");
    repository.write("c.txt", "c\n");
    repository.git(&["add", "--", "c.txt"]);
    repository.write("a.txt", "a fixed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let changes = gikli
        .amend_last_commit(
            repository.root(),
            &root,
            &paths(&["b.txt"]),
            "Change a and b",
        )
        .expect("amend");

    assert_eq!(commit_count(&repository), "2\n");
    assert_eq!(last_message(&repository), "Change a and b\n\n");
    assert_eq!(last_commit_files(&repository), vec!["M a.txt", "M b.txt"]);
    assert_eq!(repository.git(&["show", "HEAD:a.txt"]), "a changed\n");
    assert_eq!(
        repository.status_by_git(),
        vec![" M a.txt", "A  c.txt"],
        "what was not selected stays as it was"
    );
    assert_eq!(
        paths_of(&changes.changes),
        vec![Path::new("a.txt"), Path::new("c.txt")]
    );
}

#[test]
fn the_last_commit_message_comes_whole_for_the_message_field() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    repository.git(&[
        "commit",
        "--quiet",
        "--amend",
        "--cleanup=verbatim",
        "--message",
        "Subject\n\n# 123 is the ticket\n  indented",
    ]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let message = gikli.last_commit_message(&root).expect("read the message");

    assert_eq!(
        message.as_deref(),
        Some("Subject\n\n# 123 is the ticket\n  indented")
    );
}

#[test]
fn a_repository_without_commits_has_no_last_message_and_nothing_to_amend() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    assert_eq!(gikli.last_commit_message(&root), Ok(None));
    assert_eq!(
        gikli.amend_last_commit(repository.root(), &root, &paths(&["a.txt"]), "First"),
        Err(Failure::NothingToAmend)
    );
    assert_eq!(repository.status_by_git(), vec!["?? a.txt"]);
}

#[test]
fn the_root_commit_can_be_amended() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.commit_all("Initial commit");
    repository.write("a.txt", "a fixed\n");
    repository.write("new.txt", "new\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .amend_last_commit(
            repository.root(),
            &root,
            &paths(&["a.txt", "new.txt"]),
            "The first commit",
        )
        .expect("amend");

    assert_eq!(commit_count(&repository), "1\n");
    assert_eq!(last_message(&repository), "The first commit\n\n");
    assert_eq!(repository.git(&["show", "HEAD:a.txt"]), "a fixed\n");
    assert_eq!(repository.git(&["show", "HEAD:new.txt"]), "new\n");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn an_amend_needs_selected_changes() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    repository.write("b.txt", "b changed\n");
    let before = repository.git(&["rev-parse", "HEAD"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.amend_last_commit(repository.root(), &root, &[], "A better message");

    assert_eq!(result, Err(Failure::NothingToCommit));
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(repository.status_by_git(), vec![" M b.txt"]);
}

#[test]
fn an_empty_root_commit_takes_the_selected_changes() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.git(&["commit", "--quiet", "--allow-empty", "--message", "Start"]);
    repository.write("a.txt", "a\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .amend_last_commit(
            repository.root(),
            &root,
            &paths(&["a.txt"]),
            "Start of the project",
        )
        .expect("amend");

    assert_eq!(commit_count(&repository), "1\n");
    assert_eq!(last_message(&repository), "Start of the project\n\n");
    assert_eq!(repository.git(&["show", "HEAD:a.txt"]), "a\n");
}

#[test]
fn a_selection_that_would_leave_the_commit_empty_changes_nothing() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    repository.write("a.txt", "a\n");
    let before = repository.git(&["rev-parse", "HEAD"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.amend_last_commit(
        repository.root(),
        &root,
        &paths(&["a.txt"]),
        "Change nothing after all",
    );

    assert!(
        matches!(result, Err(Failure::CommitFailed { .. })),
        "git refuses it: {result:?}"
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(repository.status_by_git(), vec![" M a.txt"]);
}

#[test]
fn selected_files_that_are_no_changes_any_more_leave_the_commit_as_it_is() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let before = repository.git(&["rev-parse", "HEAD"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.amend_last_commit(
        repository.root(),
        &root,
        &paths(&["b.txt"]),
        "Reworded by accident",
    );

    assert_eq!(result, Err(Failure::NothingToCommit));
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), before);
}

#[test]
fn an_amend_needs_a_message() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let before = repository.git(&["rev-parse", "HEAD"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.amend_last_commit(repository.root(), &root, &[], "  \n");

    assert_eq!(result, Err(Failure::CommitMessageEmpty));
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), before);
}

#[test]
fn a_file_named_like_head_does_not_get_in_the_way_of_the_last_message() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    repository.write("HEAD", "not a revision\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    assert_eq!(
        gikli.last_commit_message(&root),
        Ok(Some("Change a".to_owned()))
    );
}

#[test]
fn the_files_amended_in_leave_their_changelist_and_the_rest_of_the_last_commit_does_not() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    repository.write("a.txt", "a edited again\n");
    repository.write("b.txt", "b changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let changes = gikli
        .amend_last_commit(
            repository.root(),
            &root,
            &paths(&["b.txt"]),
            "Change a and b",
        )
        .expect("amend");

    assert_eq!(paths_of(&changes.changes), vec![Path::new("a.txt")]);
    assert_eq!(
        paths_of(&changes.changelists[0].group.changes),
        vec![Path::new("a.txt")]
    );
}

#[test]
fn a_selected_unversioned_file_goes_back_to_unversioned_when_the_amend_fails() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    sandbox.hook("pre-commit", "exit 1");
    repository.write("new.txt", "new\n");
    let before = repository.git(&["rev-parse", "HEAD"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.amend_last_commit(
        repository.root(),
        &root,
        &paths(&["new.txt"]),
        "With a new file",
    );

    assert!(result.is_err(), "the hook stops the amend: {result:?}");
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(repository.status_by_git(), vec!["?? new.txt"]);
}

#[test]
fn an_amend_saves_a_backup_of_the_commit_it_replaces_and_the_notification_offers_undo() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let replaced = commit_of(&repository, "HEAD");
    repository.write(
        "b.txt",
        "b changed
",
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .amend_last_commit(
            repository.root(),
            &root,
            &paths(&["b.txt"]),
            "Change a and b",
        )
        .expect("amend");

    let amended = commit_of(&repository, "HEAD");
    let backups = backups_by_git(&repository);
    assert_eq!(backups.len(), 1, "one Backup: {backups:?}");
    assert!(
        backups[0].ends_with(&format!("/main {replaced}")),
        "the Backup holds the commit the amend replaced: {backups:?}"
    );
    let notifications = gikli.notifications();
    assert_eq!(
        notifications.len(),
        1,
        "one notification: {notifications:?}"
    );
    let NotificationKind::HistoryRewritten {
        rewrite,
        backup,
        head,
        ..
    } = &notifications[0].kind
    else {
        panic!("a notification of the amend: {notifications:?}");
    };
    assert_eq!(rewrite, &Rewrite::Amend);
    assert_eq!(backup.commit, replaced);
    assert_eq!(head.as_deref(), Some(amended.as_str()));
    assert_eq!(
        notifications[0].actions,
        vec![NotificationAction::RestoreBackup]
    );
}

#[test]
fn undo_of_an_amend_brings_back_the_commit_it_replaced_and_leaves_what_it_added_uncommitted() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let replaced = commit_of(&repository, "HEAD");
    repository.write(
        "b.txt",
        "b changed
",
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    gikli
        .amend_last_commit(
            repository.root(),
            &root,
            &paths(&["b.txt"]),
            "Change a and b",
        )
        .expect("amend");
    let notification = gikli.notifications()[0].clone();

    gikli
        .act_on_notification(notification.id, &NotificationAction::RestoreBackup)
        .expect("undo the amend");

    assert_eq!(commit_of(&repository, "HEAD"), replaced);
    assert_eq!(
        last_message(&repository),
        "Change a

"
    );
    assert_eq!(repository.status_by_git(), vec!["M  b.txt"]);
    assert_eq!(gikli.notifications(), vec![]);
}

#[test]
fn each_amend_saves_a_backup_of_its_own() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    for message in ["Once", "Twice", "Three times"] {
        repository.write(
            "b.txt",
            &format!(
                "{message}
"
            ),
        );
        gikli
            .amend_last_commit(repository.root(), &root, &paths(&["b.txt"]), message)
            .expect("amend");
    }

    assert_eq!(backups_by_git(&repository).len(), 3);
    assert_eq!(gikli.notifications().len(), 3);
}

#[test]
fn an_amend_that_fails_leaves_no_backup() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    sandbox.hook("pre-commit", "exit 1");
    repository.write(
        "b.txt",
        "b changed
",
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.amend_last_commit(repository.root(), &root, &paths(&["b.txt"]), "Hooked");

    assert!(result.is_err(), "the hook stops the amend: {result:?}");
    assert_eq!(backups_by_git(&repository), Vec::<String>::new());
    assert_eq!(gikli.notifications(), vec![]);
}

#[test]
fn an_amend_in_detached_head_saves_a_backup_of_head_that_undo_brings_back() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let replaced = commit_of(&repository, "HEAD");
    repository.git(&["switch", "--quiet", "--detach", "HEAD"]);
    repository.write("b.txt", "b changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    gikli
        .amend_last_commit(repository.root(), &root, &paths(&["b.txt"]), "Detached")
        .expect("amend");
    let backups = backups_by_git(&repository);
    assert_eq!(backups.len(), 1, "one Backup: {backups:?}");
    assert!(backups[0].ends_with(&format!("/HEAD {replaced}")));
    let notification = gikli.notifications()[0].clone();

    gikli
        .act_on_notification(notification.id, &NotificationAction::RestoreBackup)
        .expect("undo the amend");

    assert_eq!(commit_of(&repository, "HEAD"), replaced);
    assert_eq!(commit_of(&repository, "main"), replaced);
    assert!(!repository.git_succeeds(&["symbolic-ref", "--quiet", "HEAD"]));
}
