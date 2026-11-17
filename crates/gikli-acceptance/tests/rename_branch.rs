//! Rename: a local branch takes a new name, and can let go of its upstream on the way.

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{CurrentBranch, Failure, Gikli, Repository};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// The local branches of `repository`, as git lists them.
fn branches_by_git(repository: &TestRepository) -> Vec<String> {
    repository
        .git(&["for-each-ref", "--format=%(refname:short)", "refs/heads/"])
        .lines()
        .map(str::to_owned)
        .collect()
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
fn a_branch_that_is_not_checked_out_takes_its_new_name() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "feature/logn"]);
    let commit = work.git(&["rev-parse", "feature/logn"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .rename_branch(&root, "feature/logn", "feature/login", false)
        .expect("rename the branch");

    assert_eq!(
        head,
        CurrentBranch::Branch {
            name: "main".to_owned(),
            upstream: None
        }
    );
    assert_eq!(branches_by_git(&work), vec!["feature/login", "main"]);
    assert_eq!(work.git(&["rev-parse", "feature/login"]), commit);
}

/// A clone whose `main` follows `origin/main` of a local bare remote.
fn clone_following_origin(sandbox: &Sandbox) -> TestRepository {
    let remote = sandbox.bare_remote("remote.git");
    let seed = sandbox.clone_of(&remote, "seed");
    seed.commit_at(1, "First");
    seed.git(&["push", "--quiet", "origin", "main"]);
    sandbox.clone_of(&remote, "work")
}

#[test]
fn renaming_the_current_branch_moves_head_along_and_keeps_its_upstream() {
    let sandbox = Sandbox::new();
    let work = clone_following_origin(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .rename_branch(&root, "main", "trunk", false)
        .expect("rename the branch");

    assert!(
        matches!(&head, CurrentBranch::Branch { name, upstream: Some(_) } if name == "trunk"),
        "HEAD is on the renamed branch, which still has its upstream: {head:?}"
    );
    assert_eq!(
        work.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "trunk"
    );
    assert_eq!(branches_by_git(&work), vec!["trunk"]);
    assert_eq!(
        upstream_by_git(&work, "trunk").as_deref(),
        Some("origin/main")
    );
}

#[test]
fn a_renamed_branch_can_let_go_of_its_upstream() {
    let sandbox = Sandbox::new();
    let work = clone_following_origin(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .rename_branch(&root, "main", "mine", true)
        .expect("rename the branch");

    assert_eq!(
        head,
        CurrentBranch::Branch {
            name: "mine".to_owned(),
            upstream: None
        }
    );
    assert_eq!(upstream_by_git(&work, "mine"), None);
}

#[test]
fn letting_go_of_an_upstream_a_branch_does_not_have_renames_it_all_the_same() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .rename_branch(&root, "main", "trunk", true)
        .expect("rename the branch");

    assert_eq!(branches_by_git(&work), vec!["trunk"]);
}

#[test]
fn a_new_name_git_does_not_take_or_that_another_branch_has_is_refused() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["branch", "feature"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let taken = gikli.rename_branch(&root, "feature", "main", false);
    let invalid = gikli.rename_branch(&root, "feature", "@{-1}", false);

    assert_eq!(
        taken,
        Err(Failure::BranchNameTaken {
            name: "main".to_owned()
        })
    );
    assert_eq!(
        invalid,
        Err(Failure::BranchNameInvalid {
            name: "@{-1}".to_owned()
        })
    );
    assert_eq!(branches_by_git(&work), vec!["feature", "main"]);
}

#[test]
fn a_branch_deleted_since_is_not_renamed() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let renamed = gikli.rename_branch(&root, "gone", "back", false);

    assert_eq!(
        renamed,
        Err(Failure::RevisionNotFound {
            revision: "gone".to_owned()
        })
    );
    assert_eq!(branches_by_git(&work), vec!["main"]);
}

/// The dialog opens on the branch's own name, so keeping it and unsetting the upstream is a rename
/// too, one that only lets go of the upstream.
#[test]
fn keeping_the_name_lets_go_of_the_upstream_alone() {
    let sandbox = Sandbox::new();
    let work = clone_following_origin(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    gikli
        .rename_branch(&root, "main", "main", true)
        .expect("unset the upstream");

    assert_eq!(branches_by_git(&work), vec!["main"]);
    assert_eq!(upstream_by_git(&work, "main"), None);
}
