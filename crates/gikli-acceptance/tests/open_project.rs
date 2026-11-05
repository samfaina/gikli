//! Opening a folder as a Project.

use gikli_acceptance::Sandbox;
use gikli_core::{Failure, Gikli};

#[test]
fn opening_a_repository_folder_shows_its_repository_root() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");

    let project = sandbox
        .gikli()
        .open_project(repository.root())
        .expect("open project");

    let roots: Vec<_> = project
        .repositories
        .iter()
        .map(|r| r.root.clone())
        .collect();
    assert_eq!(roots, vec![repository.toplevel_by_git()]);
}

#[test]
fn opening_a_folder_outside_any_repository_finds_no_repository() {
    let sandbox = Sandbox::new();
    let folder = sandbox.folder("notes");

    let project = sandbox.gikli().open_project(&folder).expect("open project");

    assert_eq!(project.repositories, vec![]);
    assert!(!sandbox.is_inside_repository_by_git(&folder));
}

#[test]
fn a_subfolder_with_unicode_and_spaces_opens_at_its_repository_root() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("año único ✓");
    let subfolder = repository.folder("src/deep folder");

    let project = sandbox
        .gikli()
        .open_project(&subfolder)
        .expect("open project");

    let roots: Vec<_> = project
        .repositories
        .iter()
        .map(|r| r.root.clone())
        .collect();
    assert_eq!(roots, vec![repository.toplevel_by_git()]);
    assert_eq!(roots[0], sandbox.root().join("año único ✓"));
}

#[test]
fn git_variables_in_the_users_environment_do_not_redirect_detection() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let elsewhere = sandbox.repository("elsewhere");
    let mut host = sandbox.host_environment();
    host.set("GIT_DIR", elsewhere.root().join(".git"));
    host.set("GIT_WORK_TREE", elsewhere.root());

    let project = Gikli::new(host)
        .open_project(repository.root())
        .expect("open project");

    let roots: Vec<_> = project
        .repositories
        .iter()
        .map(|r| r.root.clone())
        .collect();
    assert_eq!(roots, vec![repository.toplevel_by_git()]);
}

#[test]
fn opening_a_folder_that_does_not_exist_is_reported() {
    let sandbox = Sandbox::new();
    let missing = sandbox.root().join("gone");

    let opened = sandbox.gikli().open_project(&missing);

    assert_eq!(opened, Err(Failure::FolderNotFound { folder: missing }));
}

#[test]
fn opening_a_clone_shows_the_clone_and_not_its_remote() {
    let sandbox = Sandbox::new();
    let remote = sandbox.bare_remote("origin.git");
    let author = sandbox.clone_of(&remote, "author");
    author.write("README.md", "hello\n");
    author.commit_all("Initial commit");
    author.git(&["push", "--quiet", "origin", "main"]);
    let clone = sandbox.clone_of(&remote, "reader");

    let project = sandbox
        .gikli()
        .open_project(clone.root())
        .expect("open project");

    let roots: Vec<_> = project
        .repositories
        .iter()
        .map(|r| r.root.clone())
        .collect();
    assert_eq!(roots, vec![clone.toplevel_by_git()]);
    assert_eq!(clone.git(&["log", "--format=%s"]), "Initial commit\n");
}
