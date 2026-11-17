//! Checkout of a local branch, which HEAD switches to, and of a tag or a revision, which leave a
//! detached HEAD.

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

/// The commit `revision` names in `repository`, as git resolves it.
fn commit_of(repository: &TestRepository, revision: &str) -> String {
    repository
        .git(&["rev-parse", &format!("{revision}^{{commit}}")])
        .trim()
        .to_owned()
}

#[test]
fn checking_out_a_local_branch_switches_to_it() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["checkout", "--quiet", "-b", "feature/login"]);
    work.write("login.txt", "login\n");
    work.commit_all("Login");
    work.git(&["checkout", "--quiet", "main"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .check_out_branch(&root, "feature/login")
        .expect("check out the branch");

    assert_eq!(
        head,
        CurrentBranch::Branch {
            name: "feature/login".to_owned(),
            upstream: None,
        }
    );
    assert_eq!(
        work.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "feature/login"
    );
    assert_eq!(work.status_by_git(), Vec::<String>::new());
    assert!(
        work.root().join("login.txt").exists(),
        "the working tree follows"
    );
    assert_eq!(commit_of(&work, "HEAD"), commit_of(&work, "feature/login"));
}

/// git would otherwise create a local branch that tracks a remote branch of that name, which
/// checking out a remote branch does.
#[test]
fn checking_out_a_local_branch_deleted_since_says_so() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    let remote = sandbox.bare_remote("remote.git");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "HEAD:old"]);
    work.git(&["branch", "old"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    work.git(&["branch", "--quiet", "--delete", "old"]);

    assert_eq!(
        gikli.check_out_branch(&root, "old"),
        Err(Failure::RevisionNotFound {
            revision: "old".to_owned()
        })
    );
    assert_eq!(
        work.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "main"
    );
    assert!(!work.git_succeeds(&["rev-parse", "--verify", "--quiet", "refs/heads/old"]));
}

fn detached_at(hash: &str) -> CurrentBranch {
    CurrentBranch::Detached {
        hash: hash.to_owned(),
    }
}

/// A tag goes by its whole reference, so a branch with the same name does not get in the way, and
/// an annotated tag leaves HEAD on the commit it points at.
#[test]
fn checking_out_a_tag_leaves_a_detached_head_on_its_commit() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["tag", "--annotate", "--message", "Release", "v1"]);
    work.git(&["tag", "light"]);
    work.git(&["branch", "v1"]);
    work.commit_at(2, "Second");
    work.git(&["branch", "--force", "v1"]);
    let first = commit_of(&work, "refs/tags/v1");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli.check_out_tag(&root, "v1").expect("check out the tag");

    assert_eq!(head, detached_at(&first));
    assert!(
        !work.git_succeeds(&["symbolic-ref", "--quiet", "HEAD"]),
        "detached"
    );
    assert_eq!(commit_of(&work, "HEAD"), first);
    assert_eq!(
        gikli.current_branch(&root),
        Ok(detached_at(&first)),
        "as the status area shows it"
    );

    gikli
        .check_out_branch(&root, "main")
        .expect("go back to main");
    let light = gikli
        .check_out_tag(&root, "light")
        .expect("check out the tag");
    assert_eq!(light, detached_at(&first));
}

#[test]
fn checking_out_a_revision_leaves_a_detached_head_on_it() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.commit_at(2, "Second");
    let first = commit_of(&work, "HEAD~1");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let head = gikli
        .check_out_revision(&root, &first[..7])
        .expect("check out the revision");

    assert_eq!(head, detached_at(&first));
    assert_eq!(commit_of(&work, "HEAD"), first);
    assert_eq!(
        work.git(&["log", "-1", "--format=%s", "main"]).trim(),
        "Second",
        "main stays"
    );
}

#[test]
fn checking_out_a_tag_or_revision_that_names_no_commit_says_so() {
    let sandbox = Sandbox::new();
    let work = sandbox.repository("work");
    work.commit_at(1, "First");
    work.git(&["tag", "gone"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    work.git(&["tag", "--delete", "gone"]);
    work.git(&["branch", "gone"]);

    assert_eq!(
        gikli.check_out_tag(&root, "gone"),
        Err(Failure::RevisionNotFound {
            revision: "gone".to_owned()
        }),
        "not the branch of that name"
    );
    assert_eq!(
        gikli.check_out_revision(&root, "nothing"),
        Err(Failure::RevisionNotFound {
            revision: "nothing".to_owned()
        })
    );
    assert_eq!(
        work.git(&["symbolic-ref", "--short", "HEAD"]).trim(),
        "main"
    );
}
