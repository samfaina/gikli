//! Delete: one or several local branches go, merged or not, and the notification that says so can
//! bring them back, show the commits no other branch had, and delete the remote branch too.

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    DeletedBranch, Event, Failure, Gikli, Notification, NotificationAction, NotificationKind,
    NotificationOutcome, Project, RemoteBranch, Repository,
};

/// Opens the repository's folder as a Project and returns it with its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> (Project, Repository) {
    let project = gikli.open_project(repository.root()).expect("open project");
    let root = project.repositories[0].clone();
    (project, root)
}

/// The local branches of `repository`, as git lists them.
fn branches_by_git(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["for-each-ref", "--format=%(refname:short)", "refs/heads/"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The commit `revision` names in `repository`, as git resolves it.
fn commit_of(repository: &TestRepository, revision: &str) -> String {
    repository
        .git(&["rev-parse", &format!("{revision}^{{commit}}")])
        .trim()
        .to_owned()
}

/// The one notification raised.
fn the_notification(gikli: &Gikli) -> Notification {
    let notifications = gikli.notifications();
    assert_eq!(
        notifications.len(),
        1,
        "one notification: {notifications:?}"
    );
    notifications[0].clone()
}

#[test]
fn a_deleted_branch_goes_and_its_notification_offers_to_restore_it() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "feature"]);
    let commit = commit_of(&work, "feature");
    let head = commit.clone();
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);

    gikli
        .delete_branches(&project.folder, &root, &["feature".to_owned()])
        .expect("delete the branch");

    assert_eq!(branches_by_git(&work), vec!["main"]);
    let notification = the_notification(&gikli);
    assert_eq!(notification.project, project.folder);
    assert_eq!(
        notification.kind,
        NotificationKind::BranchesDeleted {
            repository: root.clone(),
            branches: vec![DeletedBranch {
                name: "feature".to_owned(),
                commit,
                upstream: None,
                unmerged: false,
            }],
            head: Some(head),
        }
    );
    assert_eq!(
        notification.actions,
        vec![NotificationAction::RestoreBranches]
    );
}

#[test]
fn a_branch_that_is_not_merged_goes_too_and_restoring_brings_it_back_on_the_same_commit() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["checkout", "--quiet", "-b", "feature"]);
    work.commit_at(2, "Only on feature");
    work.git(&["checkout", "--quiet", "main"]);
    let commit = commit_of(&work, "feature");
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);
    gikli
        .delete_branches(&project.folder, &root, &["feature".to_owned()])
        .expect("delete the branch");
    assert_eq!(branches_by_git(&work), vec!["main"]);
    let notification = the_notification(&gikli);

    let outcome = gikli
        .act_on_notification(notification.id, &NotificationAction::RestoreBranches)
        .expect("restore the branch");

    assert_eq!(outcome, NotificationOutcome::BranchesChanged(root));
    assert_eq!(branches_by_git(&work), vec!["feature", "main"]);
    assert_eq!(commit_of(&work, "feature"), commit);
    assert_eq!(
        gikli.notifications(),
        vec![],
        "the branch is back, nothing to tell"
    );
}

#[test]
fn a_branch_with_commits_head_does_not_have_links_to_them() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["checkout", "--quiet", "-b", "feature"]);
    work.commit_at(2, "Login form");
    work.commit_at(3, "Login button");
    work.git(&["checkout", "--quiet", "main"]);
    work.commit_at(4, "On main");
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);
    gikli
        .delete_branches(&project.folder, &root, &["feature".to_owned()])
        .expect("delete the branch");
    let notification = the_notification(&gikli);
    let NotificationKind::BranchesDeleted { branches, .. } = &notification.kind else {
        panic!("not a deletion: {notification:?}");
    };
    assert!(branches[0].unmerged);
    let show = NotificationAction::ShowUnmergedCommits {
        branch: "feature".to_owned(),
    };
    assert_eq!(
        notification.actions,
        vec![NotificationAction::RestoreBranches, show.clone()]
    );
    // HEAD moving since does not change which commits the branch took away.
    work.git_at(5, &["merge", "--quiet", "--no-edit", &branches[0].commit]);

    let outcome = gikli
        .act_on_notification(notification.id, &show)
        .expect("show the unmerged commits");

    let NotificationOutcome::UnmergedCommits(commits) = outcome else {
        panic!("not the unmerged commits: {outcome:?}");
    };
    let subjects: Vec<&str> = commits
        .iter()
        .map(|commit| commit.subject.as_str())
        .collect();
    assert_eq!(subjects, vec!["Login button", "Login form"]);
    let hashes: Vec<String> = commits.iter().map(|commit| commit.hash.clone()).collect();
    let by_git: Vec<String> = work
        .git(&["rev-list", &branches[0].commit, "--not", "HEAD~1"])
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(hashes, by_git, "those HEAD lacked before the merge");
    assert_eq!(
        gikli.notifications(),
        vec![notification],
        "the notification stays"
    );
}

/// A clone of a local bare remote, `remote.git`, with a branch `feature` pushed there and following
/// `origin/feature`, and HEAD on `main`.
fn clone_with_pushed_feature(sandbox: &Sandbox) -> (TestRepository, TestRepository) {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.clone_of(&remote, "work");
    work.commit_at(1, "First");
    work.git(&["push", "--quiet", "--set-upstream", "origin", "main"]);
    work.git(&["checkout", "--quiet", "-b", "feature"]);
    work.commit_at(2, "On feature");
    work.git(&["push", "--quiet", "--set-upstream", "origin", "feature"]);
    work.git(&["checkout", "--quiet", "main"]);
    (remote, work)
}

/// The upstream of `branch` in `repository`, as git names it, if it has one.
fn upstream_by_git(repository: &TestRepository, branch: &str) -> Option<String> {
    let upstream = repository.git(&[
        "for-each-ref",
        "--format=%(upstream:short)",
        &format!("refs/heads/{branch}"),
    ]);
    let upstream = upstream.trim();
    (!upstream.is_empty()).then(|| upstream.to_owned())
}

#[test]
fn the_remote_branch_a_deleted_branch_followed_can_be_deleted_too() {
    let sandbox = Sandbox::new();
    let (remote, work) = clone_with_pushed_feature(&sandbox);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);
    gikli
        .delete_branches(&project.folder, &root, &["feature".to_owned()])
        .expect("delete the branch");
    let notification = the_notification(&gikli);
    let NotificationKind::BranchesDeleted { branches, .. } = &notification.kind else {
        panic!("not a deletion: {notification:?}");
    };
    let upstream = RemoteBranch {
        remote: "origin".to_owned(),
        branch: "feature".to_owned(),
    };
    assert_eq!(branches[0].upstream, Some(upstream.clone()));
    let show = NotificationAction::ShowUnmergedCommits {
        branch: "feature".to_owned(),
    };
    let delete_remote = NotificationAction::DeleteRemoteBranch { upstream };
    assert_eq!(
        notification.actions,
        vec![
            NotificationAction::RestoreBranches,
            show.clone(),
            delete_remote.clone()
        ]
    );

    let outcome = gikli
        .act_on_notification(notification.id, &delete_remote)
        .expect("delete the remote branch");

    assert_eq!(outcome, NotificationOutcome::BranchesChanged(root));
    assert_eq!(branches_by_git(&remote), vec!["main"]);
    assert!(
        !work.git_succeeds(&[
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/remotes/origin/feature"
        ]),
        "the remote branch is gone here too"
    );
    let left = the_notification(&gikli);
    assert_eq!(left.kind, notification.kind, "the notification stays");
    assert_eq!(
        left.actions,
        vec![NotificationAction::RestoreBranches, show]
    );
}

#[test]
fn restoring_a_branch_brings_back_its_upstream() {
    let sandbox = Sandbox::new();
    let (_remote, work) = clone_with_pushed_feature(&sandbox);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);
    gikli
        .delete_branches(&project.folder, &root, &["feature".to_owned()])
        .expect("delete the branch");
    let notification = the_notification(&gikli);

    gikli
        .act_on_notification(notification.id, &NotificationAction::RestoreBranches)
        .expect("restore the branch");

    assert_eq!(
        upstream_by_git(&work, "feature").as_deref(),
        Some("origin/feature")
    );
}

#[test]
fn several_branches_go_at_once_and_come_back_together() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "feature/a"]);
    work.git(&["checkout", "--quiet", "-b", "feature/b"]);
    work.commit_at(2, "On b");
    work.git(&["checkout", "--quiet", "main"]);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);

    gikli
        .delete_branches(
            &project.folder,
            &root,
            &["feature/a".to_owned(), "feature/b".to_owned()],
        )
        .expect("delete the branches");

    assert_eq!(branches_by_git(&work), vec!["main"]);
    let notification = the_notification(&gikli);
    let NotificationKind::BranchesDeleted { branches, .. } = &notification.kind else {
        panic!("not a deletion: {notification:?}");
    };
    let names: Vec<&str> = branches.iter().map(|branch| branch.name.as_str()).collect();
    assert_eq!(names, vec!["feature/a", "feature/b"]);
    gikli
        .act_on_notification(notification.id, &NotificationAction::RestoreBranches)
        .expect("restore the branches");
    assert_eq!(
        branches_by_git(&work),
        vec!["feature/a", "feature/b", "main"]
    );
}

#[test]
fn the_current_branch_is_not_deleted_and_neither_are_the_others_asked_with_it() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "feature"]);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);

    let deleted = gikli.delete_branches(
        &project.folder,
        &root,
        &["feature".to_owned(), "main".to_owned()],
    );

    assert_eq!(
        deleted,
        Err(Failure::CurrentBranchNotDeleted {
            name: "main".to_owned()
        })
    );
    assert_eq!(branches_by_git(&work), vec!["feature", "main"]);
    assert_eq!(gikli.notifications(), vec![]);
}

#[test]
fn a_branch_deleted_since_is_reported_and_nothing_goes() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "feature"]);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);

    let deleted = gikli.delete_branches(
        &project.folder,
        &root,
        &["feature".to_owned(), "gone".to_owned()],
    );

    assert_eq!(
        deleted,
        Err(Failure::RevisionNotFound {
            revision: "gone".to_owned()
        })
    );
    assert_eq!(branches_by_git(&work), vec!["feature", "main"]);
}

#[test]
fn a_branch_that_took_the_name_since_keeps_the_deleted_ones_from_coming_back() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "feature/a"]);
    work.git(&["branch", "feature/b"]);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);
    gikli
        .delete_branches(
            &project.folder,
            &root,
            &["feature/a".to_owned(), "feature/b".to_owned()],
        )
        .expect("delete the branches");
    let notification = the_notification(&gikli);
    work.git(&["branch", "feature/b"]);

    let restored = gikli.act_on_notification(notification.id, &NotificationAction::RestoreBranches);

    assert_eq!(
        restored,
        Err(Failure::BranchNameTaken {
            name: "feature/b".to_owned()
        })
    );
    assert_eq!(branches_by_git(&work), vec!["feature/b", "main"]);
    assert_eq!(
        gikli.notifications(),
        vec![notification],
        "the notification stays"
    );
}

#[test]
fn the_notification_stays_until_gikli_restarts() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "feature"]);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);
    gikli
        .delete_branches(&project.folder, &root, &["feature".to_owned()])
        .expect("delete the branch");
    let notification = the_notification(&gikli);

    gikli
        .open_project(work.root())
        .expect("open the project again");

    assert_eq!(gikli.notifications(), vec![notification]);
    assert_eq!(
        sandbox.gikli().notifications(),
        vec![],
        "a restart forgets it"
    );
}

#[test]
fn a_branch_git_cannot_delete_stops_the_deletion_and_the_notification_tells_of_those_gone() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "a"]);
    let _elsewhere = work.linked_worktree("b");
    work.git(&["branch", "c"]);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);

    let deleted = gikli.delete_branches(
        &project.folder,
        &root,
        &["a".to_owned(), "b".to_owned(), "c".to_owned()],
    );

    assert!(
        deleted.is_err(),
        "git keeps a branch another worktree has checked out: {deleted:?}"
    );
    assert_eq!(branches_by_git(&work), vec!["b", "c", "main"]);
    let notification = the_notification(&gikli);
    let NotificationKind::BranchesDeleted { branches, .. } = &notification.kind else {
        panic!("not a deletion: {notification:?}");
    };
    let names: Vec<&str> = branches.iter().map(|branch| branch.name.as_str()).collect();
    assert_eq!(names, vec!["a"]);
}

#[test]
fn branches_that_follow_one_remote_branch_offer_to_delete_it_once() {
    let sandbox = Sandbox::new();
    let (_remote, work) = clone_with_pushed_feature(&sandbox);
    work.git(&["branch", "--track", "feature-copy", "origin/feature"]);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);

    gikli
        .delete_branches(
            &project.folder,
            &root,
            &["feature".to_owned(), "feature-copy".to_owned()],
        )
        .expect("delete the branches");

    let notification = the_notification(&gikli);
    let offered: Vec<&NotificationAction> = notification
        .actions
        .iter()
        .filter(|action| matches!(action, NotificationAction::DeleteRemoteBranch { .. }))
        .collect();
    assert_eq!(
        offered,
        vec![&NotificationAction::DeleteRemoteBranch {
            upstream: RemoteBranch {
                remote: "origin".to_owned(),
                branch: "feature".to_owned(),
            }
        }]
    );
}

#[test]
fn a_deleted_branch_whose_remote_branch_is_gone_offers_no_deletion_of_it() {
    let sandbox = Sandbox::new();
    let (remote, work) = clone_with_pushed_feature(&sandbox);
    remote.git(&["branch", "-D", "feature"]);
    work.git(&["fetch", "--quiet", "--prune"]);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);

    gikli
        .delete_branches(&project.folder, &root, &["feature".to_owned()])
        .expect("delete the branch");

    let notification = the_notification(&gikli);
    assert!(
        !notification
            .actions
            .iter()
            .any(|action| matches!(action, NotificationAction::DeleteRemoteBranch { .. })),
        "nothing to delete on the remote: {:?}",
        notification.actions
    );
}

#[test]
fn deleting_the_remote_branch_keeps_the_notification_where_it_was() {
    let sandbox = Sandbox::new();
    let (_remote, work) = clone_with_pushed_feature(&sandbox);
    work.git(&["branch", "other"]);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);
    gikli
        .delete_branches(&project.folder, &root, &["feature".to_owned()])
        .expect("delete the branch");
    gikli
        .delete_branches(&project.folder, &root, &["other".to_owned()])
        .expect("delete the other branch");
    let before = gikli.notifications();
    let delete_remote = before[0]
        .actions
        .iter()
        .find(|action| matches!(action, NotificationAction::DeleteRemoteBranch { .. }))
        .expect("offers to delete the remote branch")
        .clone();
    let events = gikli.subscribe();

    gikli
        .act_on_notification(before[0].id, &delete_remote)
        .expect("delete the remote branch");

    let after = gikli.notifications();
    assert_eq!(after.len(), 2);
    assert_eq!(
        after[0].id, before[0].id,
        "the same notification, first still"
    );
    assert!(!after[0].actions.contains(&delete_remote));
    assert_eq!(after[1], before[1]);
    let changed: Vec<Notification> = events
        .try_iter()
        .filter_map(|event| match event {
            Event::NotificationChanged(notification) => Some(notification),
            _ => None,
        })
        .collect();
    assert_eq!(changed, vec![after[0].clone()]);
}

#[test]
fn deleting_the_same_branch_again_tells_of_it_again() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "feature"]);
    let gikli = sandbox.gikli();
    let (project, root) = opened(&gikli, &work);
    gikli
        .delete_branches(&project.folder, &root, &["feature".to_owned()])
        .expect("delete the branch");
    work.git(&["branch", "feature"]);

    gikli
        .delete_branches(&project.folder, &root, &["feature".to_owned()])
        .expect("delete it again");

    assert_eq!(gikli.notifications().len(), 2);
}
