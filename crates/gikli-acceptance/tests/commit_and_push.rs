//! Commit and push: the commit goes through, and then the push dialog opens, or the push runs at
//! once when the settings suppress the dialog.

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{Failure, Gikli, PushAfterCommit, PushTarget, Repository};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A bare remote whose `main` holds First, and a clone `mine` of it.
fn remote_and_clone(sandbox: &Sandbox) -> (TestRepository, TestRepository) {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main"]);
    let mine = sandbox.clone_of(&remote, "mine");
    (remote, mine)
}

fn target(remote: &str, branch: &str) -> PushTarget {
    PushTarget {
        remote: remote.to_owned(),
        branch: branch.to_owned(),
    }
}

/// Writes `notes.txt` in the working tree of `repository`, `working_tree`, and commits it as Add
/// notes with Commit and push, returning what follows the commit.
fn commit_and_push(
    gikli: &Gikli,
    repository: &Repository,
    working_tree: &TestRepository,
) -> Result<PushAfterCommit, Failure> {
    working_tree.write("notes.txt", "notes\n");
    gikli
        .commit_files(repository, &["notes.txt".to_owned()], "Add notes")
        .expect("commit");
    gikli.push_after_commit(repository)
}

/// The subject of the last commit of `branch` in `repository`, as git tells it.
fn last_subject_by_git(repository: &TestRepository, branch: &str) -> String {
    repository
        .git(&["log", "-1", "--format=%s", branch])
        .trim_end()
        .to_owned()
}

#[test]
fn commit_and_push_commits_and_then_opens_the_push_dialog() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &mine);
    let remote_main = remote.git(&["rev-parse", "main"]);

    let after = commit_and_push(&gikli, &root, &mine).expect("commit and push");

    let PushAfterCommit::Dialog { plan } = after else {
        panic!("the push dialog opens, not {after:?}");
    };
    assert_eq!(plan.target, Some(target("origin", "main")));
    let subjects: Vec<&str> = plan.commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, ["Add notes"], "the dialog shows the new commit");
    assert_eq!(
        last_subject_by_git(&mine, "HEAD"),
        "Add notes",
        "git has the commit"
    );
    assert_eq!(
        remote.git(&["rev-parse", "main"]),
        remote_main,
        "nothing is pushed until the user pushes from the dialog"
    );
}

#[test]
fn the_push_dialog_shows_for_commit_and_push_and_not_only_for_protected_branches_by_default() {
    let sandbox = Sandbox::new();
    let settings = sandbox.gikli().settings();

    assert!(settings.show_push_dialog_for_commit_and_push);
    assert!(!settings.show_push_dialog_only_for_protected_branches);
}

#[test]
fn with_the_dialog_suppressed_commit_and_push_pushes_at_once() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    let gikli = sandbox.gikli();
    gikli
        .set_show_push_dialog_for_commit_and_push(false)
        .expect("save the setting");
    let root = opened(&gikli, &mine);

    let after = commit_and_push(&gikli, &root, &mine).expect("commit and push");

    assert_eq!(
        after,
        PushAfterCommit::Pushed {
            target: target("origin", "main")
        }
    );
    assert_eq!(
        last_subject_by_git(&remote, "main"),
        "Add notes",
        "git agrees the remote has the commit"
    );
    assert_eq!(
        remote.git(&["rev-parse", "main"]),
        mine.git(&["rev-parse", "main"])
    );
}

#[test]
fn with_the_dialog_only_for_protected_branches_a_push_to_another_branch_runs_at_once() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    mine.git(&["switch", "--quiet", "--create", "feature"]);
    let gikli = sandbox.gikli();
    gikli
        .set_show_push_dialog_only_for_protected_branches(true)
        .expect("save the setting");
    let root = opened(&gikli, &mine);

    let after = commit_and_push(&gikli, &root, &mine).expect("commit and push");

    assert_eq!(
        after,
        PushAfterCommit::Pushed {
            target: target("origin", "feature")
        }
    );
    assert_eq!(
        remote.git(&["rev-parse", "feature"]),
        mine.git(&["rev-parse", "feature"]),
        "git agrees the push created the branch on the remote"
    );
    assert_eq!(
        mine.git(&["rev-parse", "--abbrev-ref", "feature@{upstream}"])
            .trim_end(),
        "origin/feature",
        "the new branch is the upstream, as from the dialog"
    );
}

#[test]
fn with_the_dialog_only_for_protected_branches_a_push_to_main_or_master_opens_the_dialog() {
    for protected in ["main", "master"] {
        let sandbox = Sandbox::new();
        let (remote, mine) = remote_and_clone(&sandbox);
        if protected != "main" {
            mine.git(&["switch", "--quiet", "--create", protected]);
        }
        let gikli = sandbox.gikli();
        gikli
            .set_show_push_dialog_only_for_protected_branches(true)
            .expect("save the setting");
        let root = opened(&gikli, &mine);
        let remote_branches = remote.git(&["for-each-ref"]);

        let after = commit_and_push(&gikli, &root, &mine).expect("commit and push");

        let PushAfterCommit::Dialog { plan } = after else {
            panic!("a push to {protected} opens the dialog, not {after:?}");
        };
        assert_eq!(plan.target, Some(target("origin", protected)));
        assert_eq!(
            remote.git(&["for-each-ref"]),
            remote_branches,
            "nothing is pushed to {protected}"
        );
    }
}

#[test]
fn a_protected_branch_is_one_whose_whole_name_is_main_or_master() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    mine.git(&["switch", "--quiet", "--create", "maintenance"]);
    let gikli = sandbox.gikli();
    gikli
        .set_show_push_dialog_only_for_protected_branches(true)
        .expect("save the setting");
    let root = opened(&gikli, &mine);

    let after = commit_and_push(&gikli, &root, &mine).expect("commit and push");

    assert_eq!(
        after,
        PushAfterCommit::Pushed {
            target: target("origin", "maintenance")
        }
    );
    assert!(remote.git_succeeds(&["rev-parse", "--verify", "--quiet", "maintenance"]));
}

#[test]
fn the_settings_of_commit_and_push_are_kept_across_restarts() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();

    gikli
        .set_show_push_dialog_for_commit_and_push(false)
        .expect("save the setting");
    let settings = gikli
        .set_show_push_dialog_only_for_protected_branches(true)
        .expect("save the setting");

    assert!(!settings.show_push_dialog_for_commit_and_push);
    assert!(settings.show_push_dialog_only_for_protected_branches);
    let restarted = sandbox.gikli().settings();
    assert!(!restarted.show_push_dialog_for_commit_and_push);
    assert!(restarted.show_push_dialog_only_for_protected_branches);
}

#[test]
fn without_a_remote_the_dialog_opens_to_define_one_even_when_suppressed() {
    let sandbox = Sandbox::new();
    let mine = sandbox.repository("mine");
    mine.commit_at(1, "First");
    let gikli = sandbox.gikli();
    gikli
        .set_show_push_dialog_for_commit_and_push(false)
        .expect("save the setting");
    let root = opened(&gikli, &mine);

    let after = commit_and_push(&gikli, &root, &mine).expect("commit and push");

    let PushAfterCommit::Dialog { plan } = after else {
        panic!("the dialog opens to define a remote, not {after:?}");
    };
    assert_eq!(plan.target, None, "there is nowhere to push yet");
    assert_eq!(
        last_subject_by_git(&mine, "HEAD"),
        "Add notes",
        "git has the commit"
    );
}

#[test]
fn a_push_git_rejects_after_the_commit_fails_and_keeps_the_commit() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    let other = sandbox.clone_of(&remote, "other");
    other.commit_at(2, "Someone else's");
    other.git(&["push", "--quiet", "origin", "main"]);
    let remote_main = remote.git(&["rev-parse", "main"]);
    let gikli = sandbox.gikli();
    gikli
        .set_show_push_dialog_for_commit_and_push(false)
        .expect("save the setting");
    let root = opened(&gikli, &mine);

    let rejected = commit_and_push(&gikli, &root, &mine);

    assert_eq!(
        rejected,
        Err(Failure::PushFailed {
            folder: root.root.clone()
        }),
        "the remote has a commit the branch lacks"
    );
    assert_eq!(remote.git(&["rev-parse", "main"]), remote_main);
    assert_eq!(
        last_subject_by_git(&mine, "HEAD"),
        "Add notes",
        "git still has the commit"
    );
}

#[test]
fn with_the_dialog_off_a_push_to_a_protected_branch_runs_at_once_too() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    let gikli = sandbox.gikli();
    gikli
        .set_show_push_dialog_for_commit_and_push(false)
        .expect("save the setting");
    gikli
        .set_show_push_dialog_only_for_protected_branches(true)
        .expect("save the setting");
    let root = opened(&gikli, &mine);

    let after = commit_and_push(&gikli, &root, &mine).expect("commit and push");

    assert_eq!(
        after,
        PushAfterCommit::Pushed {
            target: target("origin", "main")
        },
        "only for protected branches narrows the dialog, it does not bring it back"
    );
    assert_eq!(
        remote.git(&["rev-parse", "main"]),
        mine.git(&["rev-parse", "main"]),
        "git agrees the remote has the commit"
    );
}

#[test]
fn commit_and_push_follows_a_commit_of_what_is_staged() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    let gikli = sandbox.gikli();
    gikli
        .set_show_push_dialog_for_commit_and_push(false)
        .expect("save the setting");
    gikli
        .set_staging_mode(true)
        .expect("switch to Staging mode");
    let root = opened(&gikli, &mine);
    mine.write("notes.txt", "notes\n");
    mine.git(&["add", "notes.txt"]);

    gikli
        .commit_staged(&root, "Add staged notes")
        .expect("commit");
    let after = gikli
        .push_after_commit(&root)
        .expect("push after the commit");

    assert_eq!(
        after,
        PushAfterCommit::Pushed {
            target: target("origin", "main")
        }
    );
    assert_eq!(last_subject_by_git(&remote, "main"), "Add staged notes");
}

#[test]
fn a_commit_in_detached_head_is_kept_and_has_no_branch_to_push() {
    let sandbox = Sandbox::new();
    let (remote, mine) = remote_and_clone(&sandbox);
    mine.git(&["switch", "--quiet", "--detach"]);
    let remote_main = remote.git(&["rev-parse", "main"]);
    let gikli = sandbox.gikli();
    gikli
        .set_show_push_dialog_for_commit_and_push(false)
        .expect("save the setting");
    let root = opened(&gikli, &mine);

    let after = commit_and_push(&gikli, &root, &mine);

    assert_eq!(after, Err(Failure::DetachedHeadNotPushed));
    assert_eq!(
        last_subject_by_git(&mine, "HEAD"),
        "Add notes",
        "git has the commit"
    );
    assert_eq!(remote.git(&["rev-parse", "main"]), remote_main);
}
