//! Opening a folder as a Project.

use std::path::PathBuf;

use gikli_acceptance::Sandbox;
use gikli_core::{Failure, Gikli, Project};

fn repository_roots(project: &Project) -> Vec<PathBuf> {
    project
        .repositories
        .iter()
        .map(|repository| repository.root.clone())
        .collect()
}

#[test]
fn opening_a_repository_folder_shows_its_repository_root() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");

    let project = sandbox
        .gikli()
        .open_project(repository.root())
        .expect("open project");

    assert_eq!(
        repository_roots(&project),
        vec![repository.toplevel_by_git()]
    );
}

#[test]
fn a_folder_outside_any_repository_is_not_opened() {
    let sandbox = Sandbox::new();
    let folder = sandbox.folder("notes");

    let opened = sandbox.gikli().open_project(&folder);

    assert_eq!(
        opened,
        Err(Failure::NotInRepository {
            folder: folder.clone()
        })
    );
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

    assert_eq!(
        repository_roots(&project),
        vec![repository.toplevel_by_git()]
    );
    assert_eq!(
        repository_roots(&project),
        vec![sandbox.root().join("año único ✓")]
    );
}

#[test]
fn presentation_settings_in_the_users_configuration_do_not_change_detection() {
    let sandbox = Sandbox::new();
    sandbox.configure(
        "[color]\n\tui = always\n\tstatus = always\n\tdiff = always\n\
         [core]\n\tquotePath = true\n\
         [status]\n\trelativePaths = true\n",
    );
    let repository = sandbox.repository("año único ✓");
    let subfolder = repository.folder("src");

    let project = sandbox
        .gikli()
        .open_project(&subfolder)
        .expect("open project");

    assert_eq!(
        repository_roots(&project),
        vec![repository.toplevel_by_git()]
    );
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

    assert_eq!(
        repository_roots(&project),
        vec![repository.toplevel_by_git()]
    );
}

/// No plain-git counterpart: git cannot be asked about a folder that is not there.
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
    let author = sandbox.repository("author");
    author.add_remote("origin", &remote);
    author.write("README.md", "hello\n");
    author.commit_all("Initial commit");
    author.git(&["push", "--quiet", "origin", "main"]);
    let clone = sandbox.clone_of(&remote, "reader");

    let project = sandbox
        .gikli()
        .open_project(clone.root())
        .expect("open project");

    assert_eq!(repository_roots(&project), vec![clone.toplevel_by_git()]);
    assert_eq!(clone.git(&["log", "--format=%s"]), "Initial commit\n");
}
