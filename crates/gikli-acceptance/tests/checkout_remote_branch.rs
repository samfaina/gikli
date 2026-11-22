//! Checkout of a remote branch: it creates a local branch that follows it, and a local branch of
//! that name is never overwritten silently. It moves to the remote branch when no commit would be
//! lost, and otherwise the user drops its commits, with a Backup, or rebases them onto it.

use gikli_acceptance::{Sandbox, TestRepository, conflicts_left, opened};
use std::path::PathBuf;

use gikli_core::{
    CheckoutTarget, CurrentBranch, Failure, IncomingOutgoing, LocalCommits, NotificationAction,
    NotificationKind, NotificationOutcome, OperationKind, OverLocalChanges, Rewrite, Upstream,
};

/// A local bare remote whose `main` holds `notes.txt` and whose `feature/login` adds `login.txt`,
/// and `work`, a clone of it on `main` that has no local `feature/login` yet.
fn with_remote_branch(sandbox: &Sandbox) -> (TestRepository, TestRepository) {
    let remote = sandbox.bare_remote("remote.git");
    let publisher = sandbox.repository("publisher");
    publisher.write("notes.txt", "notes\n");
    publisher.commit_all("Add notes");
    publisher.add_remote("origin", &remote);
    publisher.git(&["push", "--quiet", "origin", "main"]);
    publisher.git(&["switch", "--quiet", "--create", "feature/login"]);
    publisher.write("login.txt", "login\n");
    publisher.commit_all("Add login");
    publisher.git(&["push", "--quiet", "origin", "feature/login"]);
    let work = sandbox.clone_of(&remote, "work");
    (publisher, work)
}

/// HEAD on the local branch `name`, which follows `upstream` and is on the same commit.
fn on_branch_following(name: &str, upstream: &str) -> CurrentBranch {
    CurrentBranch::Branch {
        name: name.to_owned(),
        upstream: Some(Upstream {
            name: upstream.to_owned(),
            commits: Some(IncomingOutgoing {
                incoming: 0,
                outgoing: 0,
            }),
        }),
    }
}

#[test]
fn checking_out_a_remote_branch_creates_a_local_branch_that_follows_it() {
    let sandbox = Sandbox::new();
    let (_publisher, work) = with_remote_branch(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .check_out_remote_branch(work.root(), &root, "origin/feature/login", None)
        .expect("check out the remote branch");

    assert_eq!(
        head,
        on_branch_following("feature/login", "origin/feature/login")
    );
    assert_eq!(work.head_by_git(), "feature/login");
    assert_eq!(
        work.upstream_by_git("feature/login").as_deref(),
        Some("origin/feature/login")
    );
    assert_eq!(
        work.commit_of("HEAD"),
        work.commit_of("refs/remotes/origin/feature/login")
    );
    assert!(work.root().join("login.txt").exists(), "the files follow");
    assert_eq!(work.status_by_git(), Vec::<String>::new());
}

/// The local branch has none of its own commits, only incoming ones, so moving it loses nothing:
/// it moves to the remote branch, and follows it from then on, whatever it followed before.
#[test]
fn checking_out_a_remote_branch_moves_a_local_branch_that_loses_nothing_to_it() {
    let sandbox = Sandbox::new();
    let (publisher, work) = with_remote_branch(&sandbox);
    work.git(&["branch", "--track", "feature/login", "origin/main"]);
    publisher.write("login.txt", "login\nagain\n");
    publisher.commit_all("Log in again");
    publisher.git(&["push", "--quiet", "origin", "feature/login"]);
    work.git(&["fetch", "--quiet", "origin"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .check_out_remote_branch(work.root(), &root, "origin/feature/login", None)
        .expect("check out the remote branch");

    assert_eq!(
        head,
        on_branch_following("feature/login", "origin/feature/login")
    );
    assert_eq!(work.head_by_git(), "feature/login");
    assert_eq!(
        work.upstream_by_git("feature/login").as_deref(),
        Some("origin/feature/login")
    );
    assert_eq!(
        work.commit_of("feature/login"),
        work.commit_of("refs/remotes/origin/feature/login")
    );
    assert_eq!(
        std::fs::read_to_string(work.root().join("login.txt")).expect("read login.txt"),
        "login\nagain\n"
    );
}

/// `work` with a local `feature/login` that has a commit of its own the remote branch does not,
/// `Remember me`, while the remote branch got `Log in again` since. HEAD is on `main`.
fn with_local_commits(sandbox: &Sandbox) -> TestRepository {
    let (publisher, work) = with_remote_branch(sandbox);
    work.git(&["switch", "--quiet", "--track", "origin/feature/login"]);
    work.write("remember.txt", "remember\n");
    work.commit_all("Remember me");
    work.git(&["switch", "--quiet", "main"]);
    publisher.write("login.txt", "login\nagain\n");
    publisher.commit_all("Log in again");
    publisher.git(&["push", "--quiet", "origin", "feature/login"]);
    work.git(&["fetch", "--quiet", "origin"]);
    work
}

/// [`with_local_commits`], where `feature/login` also changes `login.txt` as the remote branch
/// does, so rebasing it stops on a conflict there.
fn with_conflicting_local_commits(sandbox: &Sandbox) -> TestRepository {
    let work = with_local_commits(sandbox);
    work.git(&["switch", "--quiet", "feature/login"]);
    work.write("login.txt", "login\nmine\n");
    work.commit_all("Log in my way");
    work.git(&["switch", "--quiet", "main"]);
    work
}

#[test]
fn checking_out_a_remote_branch_that_would_lose_local_commits_asks_what_to_do_with_them() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    let local = work.commit_of("feature/login");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    assert_eq!(
        gikli.check_out_remote_branch(work.root(), &root, "origin/feature/login", None),
        Err(Failure::LocalCommitsWouldBeLost {
            branch: "feature/login".to_owned(),
            remote_branch: "origin/feature/login".to_owned(),
            commits: 1,
        })
    );
    assert_eq!(work.head_by_git(), "main", "nothing changed");
    assert_eq!(work.commit_of("feature/login"), local);
}

#[test]
fn dropping_the_local_commits_saves_a_backup_and_the_notification_offers_undo() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    let local = work.commit_of("feature/login");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .check_out_remote_branch(
            work.root(),
            &root,
            "origin/feature/login",
            Some(LocalCommits::Drop),
        )
        .expect("check out the remote branch, dropping the local commits");

    assert_eq!(
        head,
        on_branch_following("feature/login", "origin/feature/login")
    );
    let remote = work.commit_of("refs/remotes/origin/feature/login");
    assert_eq!(work.commit_of("HEAD"), remote);
    assert_eq!(work.head_by_git(), "feature/login");
    assert!(
        !work.root().join("remember.txt").exists(),
        "its commit went"
    );
    let backups = work.backups_by_git();
    assert_eq!(backups.len(), 1, "one Backup: {backups:?}");
    let (reference, commit) = backups[0].split_once(' ').expect("reference and commit");
    assert_eq!(commit, local, "the Backup holds the commits dropped");
    assert!(
        reference.ends_with("/feature/login"),
        "the Backup names the branch it saved: {reference}"
    );
    let notifications = gikli.notifications();
    assert_eq!(
        notifications.len(),
        1,
        "one notification: {notifications:?}"
    );
    let notification = notifications[0].clone();
    assert_eq!(notification.project, work.root());
    let NotificationKind::HistoryRewritten {
        repository: rewritten,
        rewrite,
        backup,
        head,
    } = &notification.kind
    else {
        panic!("a notification of the rewrite: {notification:?}");
    };
    assert_eq!(rewritten, &root);
    assert_eq!(rewrite, &Rewrite::DropLocalCommits);
    assert_eq!(backup.commit, local);
    assert_eq!(backup.branch.as_deref(), Some("feature/login"));
    assert_eq!(head.as_deref(), Some(remote.as_str()));
    assert_eq!(
        notification.actions,
        vec![NotificationAction::RestoreBackup]
    );

    let outcome = gikli
        .act_on_notification(notification.id, &NotificationAction::RestoreBackup)
        .expect("undo dropping the commits");

    assert_eq!(outcome, NotificationOutcome::HeadMoved(root));
    assert_eq!(
        work.commit_of("feature/login"),
        local,
        "the commits are back"
    );
    assert_eq!(work.head_by_git(), "feature/login");
    assert!(
        work.root().join("remember.txt").exists(),
        "and so are their files"
    );
    assert_eq!(work.status_by_git(), Vec::<String>::new());
}

/// The subjects of the last `count` commits of `revision` in `repository`, newest first.
fn latest_subjects(repository: &TestRepository, revision: &str, count: usize) -> Vec<String> {
    repository
        .git(&["log", "--format=%s", &format!("-{count}"), revision])
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn rebasing_the_local_commits_puts_them_on_top_of_the_remote_branch() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .check_out_remote_branch(
            work.root(),
            &root,
            "origin/feature/login",
            Some(LocalCommits::Rebase),
        )
        .expect("check out the remote branch, rebasing the local commits");

    assert_eq!(
        head,
        CurrentBranch::Branch {
            name: "feature/login".to_owned(),
            upstream: Some(Upstream {
                name: "origin/feature/login".to_owned(),
                commits: Some(IncomingOutgoing {
                    incoming: 0,
                    outgoing: 1,
                }),
            }),
        }
    );
    assert_eq!(work.head_by_git(), "feature/login");
    assert_eq!(
        work.upstream_by_git("feature/login").as_deref(),
        Some("origin/feature/login")
    );
    assert_eq!(
        latest_subjects(&work, "HEAD", 3),
        vec!["Remember me", "Log in again", "Add login"]
    );
    assert!(work.root().join("remember.txt").exists());
    assert_eq!(work.status_by_git(), Vec::<String>::new());
    assert_eq!(
        work.backups_by_git(),
        Vec::<String>::new(),
        "no commit went"
    );
}

#[test]
fn a_rebase_onto_the_remote_branch_that_stops_on_a_conflict_is_in_progress() {
    let sandbox = Sandbox::new();
    let work = with_conflicting_local_commits(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let events = gikli.subscribe();

    gikli
        .check_out_remote_branch(
            work.root(),
            &root,
            "origin/feature/login",
            Some(LocalCommits::Rebase),
        )
        .expect("a conflict is no failure");

    assert_eq!(conflicts_left(&events), std::slice::from_ref(&root));
    assert_eq!(
        gikli
            .operation_in_progress(work.root(), &root)
            .expect("read the operation in progress")
            .map(|operation| operation.kind),
        Some(OperationKind::Rebase)
    );
    assert_eq!(
        work.upstream_by_git("feature/login").as_deref(),
        Some("origin/feature/login"),
        "the branch follows the remote branch already"
    );
    assert_eq!(work.status_by_git(), vec!["UU login.txt"]);
}

#[test]
fn a_checkout_of_a_remote_branch_that_files_block_lists_them_and_creates_no_branch() {
    let sandbox = Sandbox::new();
    let (_publisher, work) = with_remote_branch(&sandbox);
    work.write("login.txt", "mine\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    assert_eq!(
        gikli.check_out_remote_branch(work.root(), &root, "origin/feature/login", None),
        Err(Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: Vec::new(),
            untracked_files: vec![PathBuf::from("login.txt")],
        })
    );
    assert_eq!(work.head_by_git(), "main");
    assert!(
        !work.git_succeeds(&[
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/heads/feature/login"
        ]),
        "no local branch yet"
    );
}

/// git rebases nothing while a tracked file has uncommitted changes, so every one blocks the
/// rebase, even one the remote branch does not touch. Saving them, rebasing and restoring them
/// moves on.
#[test]
fn every_local_change_blocks_a_rebase_onto_the_remote_branch_until_it_is_saved() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    work.write("notes.txt", "notes\nmore\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let target = CheckoutTarget::RemoteBranch {
        name: "origin/feature/login".to_owned(),
        local_commits: Some(LocalCommits::Rebase),
    };

    assert_eq!(
        gikli.check_out_remote_branch(
            work.root(),
            &root,
            "origin/feature/login",
            Some(LocalCommits::Rebase),
        ),
        Err(Failure::CheckoutBlocked {
            folder: root.root.clone(),
            local_changes: vec![PathBuf::from("notes.txt")],
            untracked_files: Vec::new(),
        })
    );
    assert_eq!(work.head_by_git(), "main", "nothing changed");

    let head = gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &target,
            &OverLocalChanges::SaveAndRestore,
        )
        .expect("save, rebase and restore");

    assert_eq!(
        head,
        CurrentBranch::Branch {
            name: "feature/login".to_owned(),
            upstream: Some(Upstream {
                name: "origin/feature/login".to_owned(),
                commits: Some(IncomingOutgoing {
                    incoming: 0,
                    outgoing: 1,
                }),
            }),
        }
    );
    assert_eq!(work.head_by_git(), "feature/login");
    assert_eq!(
        latest_subjects(&work, "HEAD", 2),
        vec!["Remember me", "Log in again"]
    );
    assert_eq!(work.status_by_git(), vec![" M notes.txt"]);
}

/// Over local changes, a checkout that drops local commits drops them as it would without.
#[test]
fn discarding_local_changes_before_dropping_local_commits_saves_the_backup_too() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    let local = work.commit_of("feature/login");
    work.write("login.txt", "login\nmine\n");
    work.git(&["add", "login.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let target = CheckoutTarget::RemoteBranch {
        name: "origin/feature/login".to_owned(),
        local_commits: Some(LocalCommits::Drop),
    };

    let head = gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &target,
            &OverLocalChanges::Discard {
                files: vec![PathBuf::from("login.txt")],
            },
        )
        .expect("discard and drop");

    assert_eq!(
        head,
        on_branch_following("feature/login", "origin/feature/login")
    );
    assert_eq!(
        work.commit_of("HEAD"),
        work.commit_of("refs/remotes/origin/feature/login")
    );
    assert_eq!(work.status_by_git(), Vec::<String>::new());
    let backups = work.backups_by_git();
    assert_eq!(backups.len(), 1, "one Backup: {backups:?}");
    assert!(backups[0].ends_with(&format!(" {local}")));
    let notifications = gikli.notifications();
    assert!(
        matches!(
            &notifications[..],
            [notification] if matches!(
                notification.kind,
                NotificationKind::HistoryRewritten {
                    rewrite: Rewrite::DropLocalCommits,
                    ..
                }
            )
        ),
        "the notification offers to undo it: {notifications:?}"
    );
}

#[test]
fn checking_out_a_remote_branch_deleted_since_says_so() {
    let sandbox = Sandbox::new();
    let (_publisher, work) = with_remote_branch(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    work.git(&["update-ref", "-d", "refs/remotes/origin/feature/login"]);

    assert_eq!(
        gikli.check_out_remote_branch(work.root(), &root, "origin/feature/login", None),
        Err(Failure::RevisionNotFound {
            revision: "origin/feature/login".to_owned()
        })
    );
    assert_eq!(work.head_by_git(), "main");
}

/// Restoring onto files in conflict would fail, so the saved changes wait in the stash list, for
/// the user to restore once the rebase is over.
#[test]
fn saved_local_changes_stay_saved_while_the_rebase_stops_on_a_conflict() {
    let sandbox = Sandbox::new();
    let work = with_conflicting_local_commits(&sandbox);
    work.write("notes.txt", "notes\nmore\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let events = gikli.subscribe();
    let target = CheckoutTarget::RemoteBranch {
        name: "origin/feature/login".to_owned(),
        local_commits: Some(LocalCommits::Rebase),
    };

    gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &target,
            &OverLocalChanges::SaveAndRestore,
        )
        .expect("a conflict is no failure");

    assert_eq!(conflicts_left(&events), std::slice::from_ref(&root));
    assert_eq!(
        gikli
            .operation_in_progress(work.root(), &root)
            .expect("read the operation in progress")
            .map(|operation| operation.kind),
        Some(OperationKind::Rebase)
    );
    assert_eq!(work.status_by_git(), vec!["UU login.txt"]);
    assert_eq!(
        work.git(&["stash", "list"]).lines().count(),
        1,
        "the saved changes wait in the stash list"
    );
}

#[test]
fn dropping_the_local_commits_of_the_current_branch_moves_it_with_head() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    work.git(&["switch", "--quiet", "feature/login"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .check_out_remote_branch(
            work.root(),
            &root,
            "origin/feature/login",
            Some(LocalCommits::Drop),
        )
        .expect("drop the local commits of the current branch");

    assert_eq!(
        head,
        on_branch_following("feature/login", "origin/feature/login")
    );
    assert_eq!(
        work.commit_of("HEAD"),
        work.commit_of("refs/remotes/origin/feature/login")
    );
    assert_eq!(work.status_by_git(), Vec::<String>::new());
}

#[test]
fn rebasing_the_local_commits_of_the_current_branch_keeps_head_on_it() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    work.git(&["switch", "--quiet", "feature/login"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .check_out_remote_branch(
            work.root(),
            &root,
            "origin/feature/login",
            Some(LocalCommits::Rebase),
        )
        .expect("rebase the local commits of the current branch");

    assert!(
        matches!(&head, CurrentBranch::Branch { name, .. } if name == "feature/login"),
        "{head:?}"
    );
    assert_eq!(work.head_by_git(), "feature/login");
    assert_eq!(
        latest_subjects(&work, "HEAD", 2),
        vec!["Remember me", "Log in again"]
    );
}

/// The local branch is named as the branch is on its remote, which the remote's fetch
/// configuration says, wherever it puts the remote branches.
#[test]
fn a_remote_branch_names_its_local_branch_as_the_fetch_configuration_maps_it() {
    let sandbox = Sandbox::new();
    let (_publisher, work) = with_remote_branch(&sandbox);
    work.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/mirror/*",
    ]);
    work.git(&["fetch", "--quiet", "origin"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .check_out_remote_branch(work.root(), &root, "mirror/feature/login", None)
        .expect("check out the remote branch");

    assert_eq!(
        head,
        on_branch_following("feature/login", "mirror/feature/login")
    );
    assert_eq!(work.head_by_git(), "feature/login");
    assert_eq!(
        work.git(&["config", "branch.feature/login.merge"]).trim(),
        "refs/heads/feature/login"
    );
    assert_eq!(
        work.git(&["config", "branch.feature/login.remote"]).trim(),
        "origin"
    );
}

#[test]
fn a_remote_branch_no_remote_fetches_into_has_no_branch_to_follow() {
    let sandbox = Sandbox::new();
    let (_publisher, work) = with_remote_branch(&sandbox);
    work.git(&[
        "update-ref",
        "refs/remotes/gone/feature/login",
        "refs/remotes/origin/feature/login",
    ]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    assert_eq!(
        gikli.check_out_remote_branch(work.root(), &root, "gone/feature/login", None),
        Err(Failure::NoRemoteToFetchFrom {
            folder: root.root.clone(),
        })
    );
    assert_eq!(work.head_by_git(), "main");
    assert!(!work.git_succeeds(&[
        "rev-parse",
        "--verify",
        "--quiet",
        "refs/heads/feature/login"
    ]));
}

/// Undo brings back the files of the dropped commits, which would write over a local change to a
/// file they change too: that keeps Undo from changing anything.
#[test]
fn undoing_dropped_commits_keeps_the_local_changes_and_stops_at_one_they_change() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    gikli
        .check_out_remote_branch(
            work.root(),
            &root,
            "origin/feature/login",
            Some(LocalCommits::Drop),
        )
        .expect("drop the local commits");
    let remote = work.commit_of("HEAD");
    work.write("login.txt", "login\nagain\nmine\n");
    let notification = gikli.notifications()[0].clone();

    let undone = gikli.act_on_notification(notification.id, &NotificationAction::RestoreBackup);

    assert!(undone.is_err(), "{undone:?}");
    assert_eq!(work.commit_of("feature/login"), remote, "nothing changed");
    assert_eq!(work.status_by_git(), vec![" M login.txt"]);
    assert_eq!(gikli.notifications().len(), 1, "Undo is still offered");

    work.git(&["checkout", "--", "login.txt"]);
    work.write("notes.txt", "notes\nmore\n");
    gikli
        .act_on_notification(notification.id, &NotificationAction::RestoreBackup)
        .expect("undo, keeping the change to notes.txt");

    assert!(work.root().join("remember.txt").exists());
    assert_eq!(work.status_by_git(), vec![" M notes.txt"]);
}
