//! Checkout of a remote branch: it creates a local branch that follows it, and a local branch of
//! that name is never overwritten silently. It moves to the remote branch when no commit would be
//! lost, and otherwise the user drops its commits, with a Backup, or rebases them onto it.

use gikli_acceptance::{Sandbox, TestRepository};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use gikli_core::{
    CheckoutTarget, CurrentBranch, Event, Failure, Gikli, IncomingOutgoing, LocalCommits,
    NotificationAction, NotificationKind, NotificationOutcome, OperationKind, OverLocalChanges,
    Repository, Rewrite, Upstream,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// The commit `revision` names in `repository`, as git resolves it.
fn commit_of(repository: &TestRepository, revision: &str) -> String {
    repository
        .git(&["rev-parse", &format!("{revision}^{{commit}}")])
        .trim()
        .to_owned()
}

/// The branch HEAD of `repository` is on, as plain git names it.
fn head_by_git(repository: &TestRepository) -> String {
    repository
        .git(&["symbolic-ref", "--short", "HEAD"])
        .trim()
        .to_owned()
}

/// The upstream of the local branch `branch` of `repository`, as plain git names it.
fn upstream_by_git(repository: &TestRepository, branch: &str) -> String {
    repository
        .git(&[
            "rev-parse",
            "--abbrev-ref",
            &format!("{branch}@{{upstream}}"),
        ])
        .trim()
        .to_owned()
}

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
    assert_eq!(head_by_git(&work), "feature/login");
    assert_eq!(
        upstream_by_git(&work, "feature/login"),
        "origin/feature/login"
    );
    assert_eq!(
        commit_of(&work, "HEAD"),
        commit_of(&work, "refs/remotes/origin/feature/login")
    );
    assert!(work.root().join("login.txt").exists(), "the files follow");
    assert_eq!(work.status_by_git(), Vec::<String>::new());
}

/// The local branch has none of its own commits, so moving it loses nothing: it moves to the
/// remote branch, and follows it from then on, though it followed nothing before.
#[test]
fn checking_out_a_remote_branch_moves_a_local_branch_behind_it_to_it() {
    let sandbox = Sandbox::new();
    let (publisher, work) = with_remote_branch(&sandbox);
    work.git(&["branch", "--no-track", "feature/login", "main"]);
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
    assert_eq!(head_by_git(&work), "feature/login");
    assert_eq!(
        upstream_by_git(&work, "feature/login"),
        "origin/feature/login"
    );
    assert_eq!(
        commit_of(&work, "feature/login"),
        commit_of(&work, "refs/remotes/origin/feature/login")
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
    work.write(
        "remember.txt",
        "remember
",
    );
    work.commit_all("Remember me");
    work.git(&["switch", "--quiet", "main"]);
    publisher.write(
        "login.txt",
        "login
again
",
    );
    publisher.commit_all("Log in again");
    publisher.git(&["push", "--quiet", "origin", "feature/login"]);
    work.git(&["fetch", "--quiet", "origin"]);
    work
}

#[test]
fn checking_out_a_remote_branch_that_would_lose_local_commits_asks_what_to_do_with_them() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    let local = commit_of(&work, "feature/login");
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
    assert_eq!(head_by_git(&work), "main", "nothing changed");
    assert_eq!(commit_of(&work, "feature/login"), local);
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

#[test]
fn dropping_the_local_commits_saves_a_backup_and_the_notification_offers_undo() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    let local = commit_of(&work, "feature/login");
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
    let remote = commit_of(&work, "refs/remotes/origin/feature/login");
    assert_eq!(commit_of(&work, "HEAD"), remote);
    assert_eq!(head_by_git(&work), "feature/login");
    assert!(
        !work.root().join("remember.txt").exists(),
        "its commit went"
    );
    let backups = backups_by_git(&work);
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
        commit_of(&work, "feature/login"),
        local,
        "the commits are back"
    );
    assert_eq!(head_by_git(&work), "feature/login");
}

/// The subjects of the last `count` commits of `revision` in `repository`, newest first.
fn subjects(repository: &TestRepository, revision: &str, count: usize) -> Vec<String> {
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
    assert_eq!(head_by_git(&work), "feature/login");
    assert_eq!(
        upstream_by_git(&work, "feature/login"),
        "origin/feature/login"
    );
    assert_eq!(
        subjects(&work, "HEAD", 3),
        vec!["Remember me", "Log in again", "Add login"]
    );
    assert!(work.root().join("remember.txt").exists());
    assert_eq!(work.status_by_git(), Vec::<String>::new());
    assert_eq!(
        backups_by_git(&work),
        Vec::<String>::new(),
        "no commit went"
    );
}

/// The Repositories the `events` received say were left with files in conflict.
fn conflicts_left(events: &Receiver<Event>) -> Vec<Repository> {
    events
        .try_iter()
        .filter_map(|event| match event {
            Event::ConflictsLeft { repository } => Some(repository),
            _ => None,
        })
        .collect()
}

#[test]
fn a_rebase_onto_the_remote_branch_that_stops_on_a_conflict_is_in_progress() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    work.git(&["switch", "--quiet", "feature/login"]);
    work.write(
        "login.txt",
        "login
mine
",
    );
    work.commit_all("Log in my way");
    work.git(&["switch", "--quiet", "main"]);
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
        upstream_by_git(&work, "feature/login"),
        "origin/feature/login",
        "the branch follows the remote branch already"
    );
    assert_eq!(work.status_by_git(), vec!["UU login.txt"]);
}

#[test]
fn a_checkout_of_a_remote_branch_that_files_block_lists_them_and_creates_no_branch() {
    let sandbox = Sandbox::new();
    let (_publisher, work) = with_remote_branch(&sandbox);
    work.write(
        "login.txt",
        "mine
",
    );
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
    assert_eq!(head_by_git(&work), "main");
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
    work.write(
        "notes.txt",
        "notes
more
",
    );
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
    assert_eq!(head_by_git(&work), "main", "nothing changed");

    gikli
        .check_out_over_local_changes(
            work.root(),
            &root,
            &target,
            &OverLocalChanges::SaveAndRestore,
        )
        .expect("save, rebase and restore");

    assert_eq!(head_by_git(&work), "feature/login");
    assert_eq!(
        subjects(&work, "HEAD", 2),
        vec!["Remember me", "Log in again"]
    );
    assert_eq!(work.status_by_git(), vec![" M notes.txt"]);
}

/// Over local changes, a checkout that drops local commits drops them as it would without.
#[test]
fn discarding_local_changes_before_dropping_local_commits_saves_the_backup_too() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    let local = commit_of(&work, "feature/login");
    work.write(
        "login.txt",
        "login
mine
",
    );
    work.git(&["add", "login.txt"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let target = CheckoutTarget::RemoteBranch {
        name: "origin/feature/login".to_owned(),
        local_commits: Some(LocalCommits::Drop),
    };

    gikli
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
        commit_of(&work, "HEAD"),
        commit_of(&work, "refs/remotes/origin/feature/login")
    );
    assert_eq!(work.status_by_git(), Vec::<String>::new());
    let backups = backups_by_git(&work);
    assert_eq!(backups.len(), 1, "one Backup: {backups:?}");
    assert!(backups[0].ends_with(&format!(" {local}")));
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
    assert_eq!(head_by_git(&work), "main");
}

/// Restoring onto files in conflict would fail, so the saved changes wait in the stash list, for
/// the user to restore once the rebase is over.
#[test]
fn saved_local_changes_stay_saved_while_the_rebase_stops_on_a_conflict() {
    let sandbox = Sandbox::new();
    let work = with_local_commits(&sandbox);
    work.git(&["switch", "--quiet", "feature/login"]);
    work.write(
        "login.txt",
        "login
mine
",
    );
    work.commit_all("Log in my way");
    work.git(&["switch", "--quiet", "main"]);
    work.write(
        "notes.txt",
        "notes
more
",
    );
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
        commit_of(&work, "HEAD"),
        commit_of(&work, "refs/remotes/origin/feature/login")
    );
    assert_eq!(work.status_by_git(), Vec::<String>::new());
}
