//! New branch: a branch created from HEAD, the selected branch or a commit, and checked out when
//! the user asks, with a name git takes as it is.

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{
    CurrentBranch, Failure, Gikli, IncomingOutgoing, NewBranchBase, RefKind, RefLabel, Repository,
    Upstream,
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

fn on_branch(name: &str) -> CurrentBranch {
    CurrentBranch::Branch {
        name: name.to_owned(),
        upstream: None,
    }
}

#[test]
fn a_new_branch_from_head_stays_unchecked_out_unless_asked() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .create_branch(&root, "feature/login", &NewBranchBase::Head, false)
        .expect("create the branch");

    assert_eq!(head, on_branch("main"), "HEAD stays where it was");
    assert_eq!(commit_of(&work, "feature/login"), commit_of(&work, "HEAD"));
}

/// The base goes by the whole reference, so a tag with the same name as the branch does not get in
/// the way.
#[test]
fn a_new_branch_from_the_selected_branch_is_checked_out_when_asked() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["tag", "release"]);
    work.git(&["checkout", "--quiet", "-b", "release"]);
    work.commit_at(2, "On release");
    work.git(&["checkout", "--quiet", "main"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let release = RefLabel {
        name: "release".to_owned(),
        kind: RefKind::Local,
    };

    let head = gikli
        .create_branch(
            &root,
            "hotfix",
            &NewBranchBase::Branch { label: release },
            true,
        )
        .expect("create the branch");

    assert_eq!(head, on_branch("hotfix"));
    assert_eq!(
        work.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "hotfix"
    );
    assert_eq!(
        commit_of(&work, "hotfix"),
        commit_of(&work, "refs/heads/release")
    );
}

#[test]
fn a_new_branch_from_a_commit_starts_at_that_commit() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.commit_at(2, "Second");
    let first = commit_of(&work, "HEAD~1");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .create_branch(
            &root,
            "from-first",
            &NewBranchBase::Commit {
                hash: first.clone(),
            },
            true,
        )
        .expect("create the branch");

    assert_eq!(head, on_branch("from-first"));
    assert_eq!(commit_of(&work, "from-first"), first);
}

#[test]
fn a_new_branch_from_a_branch_deleted_since_says_so() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "old"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    work.git(&["branch", "--quiet", "--delete", "old"]);
    let old = RefLabel {
        name: "old".to_owned(),
        kind: RefKind::Local,
    };

    let created = gikli.create_branch(&root, "new", &NewBranchBase::Branch { label: old }, false);

    assert_eq!(
        created,
        Err(Failure::RevisionNotFound {
            revision: "old".to_owned()
        })
    );
    assert!(!work.git_succeeds(&["rev-parse", "--verify", "--quiet", "refs/heads/new"]));
}

/// git takes a name for a new branch as it is: one it rejects, or one it would expand to another
/// branch, as `@{-1}` names the branch checked out before, is no name for a branch.
#[test]
fn names_git_would_not_take_as_they_are_are_rejected() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "other"]);
    work.git(&["checkout", "--quiet", "other"]);
    work.git(&["checkout", "--quiet", "main"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let branches_before = work.git(&["for-each-ref", "refs/heads/"]);

    for name in [
        "@{-1}", "HEAD", "-x", "a b", "a..b", "x.lock", "feature/", "",
    ] {
        for check_out in [false, true] {
            assert_eq!(
                gikli.create_branch(&root, name, &NewBranchBase::Head, check_out),
                Err(Failure::BranchNameInvalid {
                    name: name.to_owned()
                }),
                "{name:?}"
            );
        }
    }
    assert_eq!(work.git(&["for-each-ref", "refs/heads/"]), branches_before);
    assert_eq!(
        work.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "main"
    );
}

/// A branch git already has keeps its commit, whether or not the new one would be checked out.
#[test]
fn a_name_another_branch_has_is_taken() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "taken"]);
    work.commit_at(2, "Second");
    let taken = commit_of(&work, "taken");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    for check_out in [false, true] {
        assert_eq!(
            gikli.create_branch(&root, "taken", &NewBranchBase::Head, check_out),
            Err(Failure::BranchNameTaken {
                name: "taken".to_owned()
            })
        );
    }
    assert_eq!(commit_of(&work, "taken"), taken);
    assert_eq!(
        work.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "main"
    );
}

/// What the user types in the name of a new branch: each character git takes in no branch name
/// becomes `-`, as typed, so the name keeps its length.
#[test]
fn characters_git_takes_in_no_branch_name_become_dashes() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();

    assert_eq!(
        gikli.clean_branch_name("fix the ~bug^: why? *now* [x] a\\b\tc\u{7f}d"),
        "fix-the--bug---why---now---x]-a-b-c-d"
    );
    assert_eq!(
        gikli.clean_branch_name("feature/héllo-1.2"),
        "feature/héllo-1.2"
    );
    assert_eq!(
        gikli.clean_branch_name("a..b@{-1}"),
        "a..b@{-1}",
        "left to the check"
    );
}

/// The prefixes are the directories of the local branches, and of the remote branches within their
/// remote, each once, by name: those that start with what the user typed, ignoring case.
#[test]
fn the_name_field_suggests_prefixes_from_existing_branches() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    let remote = sandbox.bare_remote("remote.git");
    work.add_remote("team/origin", &remote);
    for name in ["feature/login", "feature/ui/menu", "bugfix/crash", "plain"] {
        work.git(&["branch", name]);
    }
    work.git(&[
        "push",
        "--quiet",
        "team/origin",
        "HEAD:release/1.0",
        "HEAD:feature/remote",
    ]);
    work.git(&["tag", "tags/v1"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let prefixes = |typed: &str| {
        gikli
            .new_branch_prefixes(&root, typed)
            .expect("suggest prefixes")
    };

    assert_eq!(
        prefixes(""),
        vec!["bugfix/", "feature/", "feature/ui/", "release/"]
    );
    assert_eq!(prefixes("FE"), vec!["feature/", "feature/ui/"]);
    assert_eq!(
        prefixes("feature/"),
        vec!["feature/ui/"],
        "not the one typed"
    );
    assert_eq!(prefixes("feature/login"), Vec::<String>::new());
}

/// git sets the remote branch as the upstream of a branch started from it, as `branch.autoSetupMerge`
/// says by default.
#[test]
fn a_new_branch_from_a_remote_branch_follows_it_as_git_is_configured() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    let remote = sandbox.bare_remote("remote.git");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "HEAD:feature/x"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let base = NewBranchBase::Branch {
        label: RefLabel {
            name: "origin/feature/x".to_owned(),
            kind: RefKind::Remote,
        },
    };

    let head = gikli
        .create_branch(&root, "feature/x", &base, true)
        .expect("create the branch");

    assert_eq!(
        head,
        CurrentBranch::Branch {
            name: "feature/x".to_owned(),
            upstream: Some(Upstream {
                name: "origin/feature/x".to_owned(),
                commits: Some(IncomingOutgoing {
                    incoming: 0,
                    outgoing: 0
                }),
            }),
        }
    );
}
