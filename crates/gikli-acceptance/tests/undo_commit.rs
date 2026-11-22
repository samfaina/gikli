//! Undo commit: the user takes back the last commit of the current branch, and gets its changes
//! back as uncommitted, after gikli saved a Backup of it.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use gikli_acceptance::{Sandbox, TestRepository, opened_project};
use gikli_core::{
    ChangeSet, Failure, Gikli, Notification, NotificationAction, NotificationKind,
    NotificationOutcome, Rewrite,
};

/// A repository with two commits: the first holds `a.txt`, the last changes it and adds `b.txt`.
fn repository_with_a_last_commit(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.commit_all("Initial commit");
    repository.write("a.txt", "a changed\n");
    repository.write("b.txt", "b\n");
    repository.commit_all("Change a and add b");
    repository
}

/// The one notification gikli raised.
fn the_notification(gikli: &Gikli) -> Notification {
    let notifications = gikli.notifications();
    assert_eq!(
        notifications.len(),
        1,
        "one notification: {notifications:?}"
    );
    notifications[0].clone()
}

/// The paths of the changes in the Active changelist.
fn active_paths(changes: &ChangeSet) -> Vec<&Path> {
    let active = changes
        .changelists
        .iter()
        .find(|list| list.active)
        .expect("one Changelist is active");
    active
        .group
        .changes
        .iter()
        .map(|change| change.path.as_path())
        .collect()
}

#[test]
fn undoing_the_last_commit_puts_its_changes_back_as_uncommitted_in_the_active_changelist() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let initial = repository.commit_of("HEAD~");
    let last = repository.commit_of("HEAD");
    let gikli = sandbox.gikli();
    let (project, root) = opened_project(&gikli, &repository);
    let created = gikli
        .create_changelist(&root, "Feature", None)
        .expect("create Feature");
    let feature = created
        .changelists
        .iter()
        .find(|list| list.name == "Feature")
        .expect("Feature exists")
        .id
        .clone();
    gikli
        .set_active_changelist(&root, &feature)
        .expect("make Feature active");

    let changes = gikli
        .undo_commit(&project.folder, &root, &last)
        .expect("undo commit");

    assert_eq!(repository.commit_of("HEAD"), initial);
    assert_eq!(repository.git(&["show", "HEAD:a.txt"]), "a\n");
    assert_eq!(repository.git(&["show", ":a.txt"]), "a changed\n");
    assert_eq!(
        repository.status_by_git(),
        vec!["M  a.txt", "A  b.txt"],
        "the changes are uncommitted again, and still staged"
    );
    assert_eq!(
        active_paths(&changes),
        vec![Path::new("a.txt"), Path::new("b.txt")],
        "they are in the Active changelist, Feature"
    );
}

#[test]
fn undoing_a_commit_saves_a_backup_of_it_first_and_the_notification_offers_undo() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let last = repository.commit_of("HEAD");
    let gikli = sandbox.gikli();
    let (project, root) = opened_project(&gikli, &repository);

    gikli
        .undo_commit(&project.folder, &root, &last)
        .expect("undo commit");

    let backups = repository.backups_by_git();
    assert_eq!(backups.len(), 1, "one Backup: {backups:?}");
    let (reference, commit) = backups[0].split_once(' ').expect("reference and commit");
    assert_eq!(commit, last, "the Backup holds the commit undone");
    assert!(
        reference.ends_with("/main"),
        "the Backup names the branch it saved: {reference}"
    );
    let notification = the_notification(&gikli);
    assert_eq!(notification.project, project.folder);
    let NotificationKind::HistoryRewritten {
        repository: rewritten,
        rewrite,
        backup,
        ..
    } = &notification.kind
    else {
        panic!("a notification of the rewrite: {notification:?}");
    };
    assert_eq!(rewritten, &root);
    assert_eq!(rewrite, &Rewrite::UndoCommit);
    assert_eq!(backup.reference, reference);
    assert_eq!(backup.commit, last);
    assert_eq!(backup.branch.as_deref(), Some("main"));
    assert_eq!(
        notification.actions,
        vec![NotificationAction::RestoreBackup]
    );
}

#[test]
fn undo_in_the_notification_brings_the_commit_back() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let last = repository.commit_of("HEAD");
    let gikli = sandbox.gikli();
    let (project, root) = opened_project(&gikli, &repository);
    gikli
        .undo_commit(&project.folder, &root, &last)
        .expect("undo commit");
    let notification = the_notification(&gikli);

    let outcome = gikli
        .act_on_notification(notification.id, &NotificationAction::RestoreBackup)
        .expect("undo the undo commit");

    assert_eq!(outcome, NotificationOutcome::HeadMoved(root));
    assert_eq!(repository.commit_of("HEAD"), last);
    assert_eq!(
        repository.git(&["symbolic-ref", "HEAD"]),
        "refs/heads/main\n",
        "HEAD stays on the branch"
    );
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
    assert_eq!(gikli.notifications(), vec![]);
}

#[test]
fn undoing_the_root_commit_keeps_its_changes_in_the_index_and_undo_brings_it_back() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.commit_all("Initial commit");
    let root_commit = repository.commit_of("HEAD");
    let gikli = sandbox.gikli();
    let (project, root) = opened_project(&gikli, &repository);

    let changes = gikli
        .undo_commit(&project.folder, &root, &root_commit)
        .expect("undo the root commit");

    assert!(
        !repository.git_succeeds(&["rev-parse", "--verify", "--quiet", "HEAD"]),
        "the branch has no commit any more"
    );
    assert_eq!(
        repository.git(&["symbolic-ref", "HEAD"]),
        "refs/heads/main\n",
        "HEAD stays on the branch"
    );
    assert_eq!(repository.status_by_git(), vec!["A  a.txt"]);
    assert_eq!(active_paths(&changes), vec![Path::new("a.txt")]);
    let backups = repository.backups_by_git();
    assert_eq!(backups.len(), 1, "one Backup: {backups:?}");
    assert!(backups[0].ends_with(&format!(" {root_commit}")));
    let notification = the_notification(&gikli);
    let NotificationKind::HistoryRewritten { head, .. } = &notification.kind else {
        panic!("a notification of the rewrite: {notification:?}");
    };
    assert_eq!(head, &None, "the rewrite left the branch on no commit");

    gikli
        .act_on_notification(notification.id, &NotificationAction::RestoreBackup)
        .expect("undo the undo commit");

    assert_eq!(repository.commit_of("HEAD"), root_commit);
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn only_the_last_commit_of_the_current_branch_can_be_undone() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let initial = repository.commit_of("HEAD~");
    let last = repository.commit_of("HEAD");
    let gikli = sandbox.gikli();
    let (project, root) = opened_project(&gikli, &repository);

    let older = gikli.undo_commit(&project.folder, &root, &initial);
    repository.git(&["switch", "--quiet", "--detach", "HEAD"]);
    let detached = gikli.undo_commit(&project.folder, &root, &last);

    let not_last = Failure::NotLastCommit {
        commit: initial.clone(),
    };
    assert_eq!(older, Err(not_last));
    assert_eq!(
        detached,
        Err(Failure::NotLastCommit {
            commit: last.clone()
        }),
        "a detached HEAD is on no branch"
    );
    assert_eq!(repository.commit_of("HEAD"), last);
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
    assert_eq!(repository.backups_by_git(), Vec::<String>::new());
    assert_eq!(gikli.notifications(), vec![]);
}

/// A clone of a new remote, with two commits on `branch`, both pushed there as `pushed_as`.
fn clone_with_pushed_commits(sandbox: &Sandbox, branch: &str, pushed_as: &str) -> TestRepository {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.clone_of(&remote, "work");
    work.git(&["switch", "--quiet", "--create", branch]);
    work.write("a.txt", "a\n");
    work.commit_all("First");
    work.write("a.txt", "a changed\n");
    work.commit_all("Second");
    work.git(&[
        "push",
        "--quiet",
        "origin",
        &format!("{branch}:{pushed_as}"),
    ]);
    work
}

#[test]
fn a_commit_pushed_to_main_or_master_is_not_undone() {
    for protected in ["main", "master"] {
        let sandbox = Sandbox::new();
        let work = clone_with_pushed_commits(&sandbox, "mine", protected);
        let last = work.commit_of("HEAD");
        let gikli = sandbox.gikli();
        let (project, root) = opened_project(&gikli, &work);

        let undone = gikli.undo_commit(&project.folder, &root, &last);

        assert_eq!(
            undone,
            Err(Failure::CommitPushedToProtectedBranch {
                commit: last.clone(),
                branch: format!("origin/{protected}"),
            }),
            "a commit on {protected} of the remote stays"
        );
        assert_eq!(work.commit_of("HEAD"), last);
        assert_eq!(work.backups_by_git(), Vec::<String>::new());
        assert_eq!(gikli.notifications(), vec![]);
    }
}

#[test]
fn a_commit_pushed_only_to_another_branch_or_not_pushed_yet_can_be_undone() {
    let sandbox = Sandbox::new();
    let work = clone_with_pushed_commits(&sandbox, "main", "mainly");
    let pushed = work.commit_of("HEAD");
    let gikli = sandbox.gikli();
    let (project, root) = opened_project(&gikli, &work);

    gikli
        .undo_commit(&project.folder, &root, &pushed)
        .expect("a commit pushed to a branch that is not protected goes");

    work.git(&["push", "--quiet", "origin", "HEAD:main"]);
    work.commit_all("Not pushed yet");
    let local = work.commit_of("HEAD");
    gikli
        .undo_commit(&project.folder, &root, &local)
        .expect("a commit main of the remote does not have goes");
    assert_eq!(work.commit_of("HEAD"), work.commit_of("origin/main"));
}

/// The name of a Backup of `branch` saved `days` days ago, as gikli names them.
fn backup_saved_days_ago(days: u64, branch: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs();
    format!("refs/gikli/backups/{}/{branch}", now - days * 24 * 60 * 60)
}

#[test]
fn backups_older_than_30_days_are_deleted() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let initial = repository.commit_of("HEAD~");
    let last = repository.commit_of("HEAD");
    let expired = backup_saved_days_ago(31, "main");
    let kept = backup_saved_days_ago(29, "feature/old");
    repository.git(&["update-ref", &expired, &initial]);
    repository.git(&["update-ref", &kept, &initial]);
    let gikli = sandbox.gikli();
    let (project, root) = opened_project(&gikli, &repository);

    gikli
        .undo_commit(&project.folder, &root, &last)
        .expect("undo commit");

    let backups = repository.backups_by_git();
    let references: Vec<&str> = backups
        .iter()
        .filter_map(|backup| backup.split(' ').next())
        .collect();
    assert_eq!(
        references.len(),
        2,
        "the new Backup and the recent one: {backups:?}"
    );
    assert!(!references.contains(&expired.as_str()));
    assert!(references.contains(&kept.as_str()));
}

#[test]
fn undo_does_nothing_once_the_branch_moved_since_the_rewrite() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let last = repository.commit_of("HEAD");
    let gikli = sandbox.gikli();
    let (project, root) = opened_project(&gikli, &repository);
    gikli
        .undo_commit(&project.folder, &root, &last)
        .expect("undo commit");
    let notification = the_notification(&gikli);
    repository.git(&["commit", "--quiet", "--message", "Redone"]);
    let redone = repository.commit_of("HEAD");

    let undone = gikli.act_on_notification(notification.id, &NotificationAction::RestoreBackup);

    assert_eq!(
        undone,
        Err(Failure::BranchMovedSinceRewrite {
            branch: Some("main".to_owned()),
        })
    );
    assert_eq!(repository.commit_of("HEAD"), redone, "the new commit stays");
    assert_eq!(
        gikli.notifications(),
        vec![notification],
        "the notification stays"
    );
}

#[test]
fn undo_brings_the_branch_back_even_with_head_on_another_one() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let initial = repository.commit_of("HEAD~");
    let last = repository.commit_of("HEAD");
    let gikli = sandbox.gikli();
    let (project, root) = opened_project(&gikli, &repository);
    gikli
        .undo_commit(&project.folder, &root, &last)
        .expect("undo commit");
    let notification = the_notification(&gikli);
    repository.git(&["switch", "--quiet", "--create", "other"]);

    gikli
        .act_on_notification(notification.id, &NotificationAction::RestoreBackup)
        .expect("undo the undo commit");

    assert_eq!(repository.commit_of("main"), last);
    assert_eq!(
        repository.commit_of("HEAD"),
        initial,
        "HEAD stays where it is"
    );
    assert_eq!(
        repository.git(&["symbolic-ref", "HEAD"]),
        "refs/heads/other\n"
    );
    assert_eq!(repository.status_by_git(), vec!["M  a.txt", "A  b.txt"]);
}

#[test]
fn a_backup_saved_the_same_second_as_another_of_the_branch_gets_a_name_of_its_own() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let initial = repository.commit_of("HEAD~");
    let last = repository.commit_of("HEAD");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs();
    // Backups of the branch for each second the undo may run in.
    for second in now..now + 10 {
        let taken = format!("refs/gikli/backups/{second}/main");
        repository.git(&["update-ref", &taken, &initial]);
    }
    let gikli = sandbox.gikli();
    let (project, root) = opened_project(&gikli, &repository);

    gikli
        .undo_commit(&project.folder, &root, &last)
        .expect("undo commit");

    let backups = repository.backups_by_git();
    assert_eq!(
        backups.len(),
        11,
        "the Backups already there stay: {backups:?}"
    );
    let NotificationKind::HistoryRewritten { backup, .. } = the_notification(&gikli).kind else {
        panic!("a notification of the rewrite");
    };
    assert!(backups.contains(&format!("{} {last}", backup.reference)));
}

#[test]
fn an_undo_git_refuses_leaves_no_backup() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    repository.git(&["switch", "--quiet", "--create", "other", "HEAD~"]);
    repository.write("a.txt", "a other\n");
    repository.commit_all("Change a elsewhere");
    repository.git(&["switch", "--quiet", "main"]);
    let last = repository.commit_of("HEAD");
    assert!(
        !repository.git_succeeds(&["merge", "--quiet", "other"]),
        "the merge stops on a conflict"
    );
    let gikli = sandbox.gikli();
    let (project, root) = opened_project(&gikli, &repository);

    let undone = gikli.undo_commit(&project.folder, &root, &last);

    assert!(
        undone.is_err(),
        "git does not reset during a merge: {undone:?}"
    );
    assert_eq!(repository.commit_of("HEAD"), last);
    assert_eq!(repository.backups_by_git(), Vec::<String>::new());
    assert_eq!(gikli.notifications(), vec![]);
}

#[test]
fn backups_older_than_30_days_are_deleted_when_the_project_opens_too() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_last_commit(&sandbox);
    let initial = repository.commit_of("HEAD~");
    let expired = backup_saved_days_ago(31, "main");
    let kept = backup_saved_days_ago(29, "main");
    repository.git(&["update-ref", &expired, &initial]);
    repository.git(&["update-ref", &kept, &initial]);

    opened_project(&sandbox.gikli(), &repository);

    assert_eq!(
        repository.backups_by_git(),
        vec![format!("{kept} {initial}")]
    );
}
