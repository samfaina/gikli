//! Opening a folder as a Project.

use std::fs;
use std::path::PathBuf;

use gikli_acceptance::{Sandbox, file_url, speaking};
use gikli_core::{Failure, Project, Repository};

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
fn a_bare_repository_is_not_opened_as_it_has_no_working_tree() {
    let sandbox = Sandbox::new();
    let bare = sandbox.bare_remote("store.git");

    let opened = sandbox.gikli().open_project(bare.root());

    assert_eq!(
        opened,
        Err(Failure::NotAWorkingTree {
            folder: bare.root().to_path_buf()
        })
    );
    assert_eq!(bare.git(&["rev-parse", "--is-bare-repository"]), "true\n");
}

#[test]
fn a_folder_inside_a_git_directory_is_not_opened_as_it_has_no_working_tree() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let hooks = repository.folder(".git/hooks");

    let opened = sandbox.gikli().open_project(&hooks);

    assert_eq!(
        opened,
        Err(Failure::NotAWorkingTree {
            folder: hooks.clone()
        })
    );
    assert_eq!(
        sandbox
            .existing_repository(&hooks)
            .git(&["rev-parse", "--is-inside-work-tree"]),
        "false\n"
    );
}

/// git knows the working tree of a submodule even from inside its git directory, so it gives a root
/// there instead of failing.
#[test]
fn the_git_directory_of_a_submodule_is_not_opened_as_it_has_no_working_tree() {
    let sandbox = Sandbox::new();
    let library = sandbox.repository("library");
    library.write("lib.txt", "code\n");
    library.commit_all("First commit");
    let app = sandbox.repository("app");
    app.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "--quiet",
        &file_url(library.root()),
        "library",
    ]);
    let git_directory = app.root().join(".git").join("modules").join("library");

    let opened = sandbox.gikli().open_project(&git_directory);

    assert_eq!(
        opened,
        Err(Failure::NotAWorkingTree {
            folder: git_directory.clone()
        })
    );
    assert_eq!(
        sandbox
            .existing_repository(&git_directory)
            .git(&["rev-parse", "--is-inside-work-tree"]),
        "false\n"
    );
}

/// A Project is a folder that holds Repositories (ADR-0004), and what a bare repository holds in
/// its folder are working trees, offered as those of a plain folder are.
#[test]
fn a_bare_repository_that_holds_a_linked_worktree_offers_the_worktree() {
    let sandbox = Sandbox::new();
    let bare = sandbox.bare_remote("store.git");
    let author = sandbox.repository("author");
    author.write("README.md", "hello\n");
    author.commit_all("Initial commit");
    author.add_remote("origin", &bare);
    author.git(&["push", "--quiet", "origin", "main"]);
    bare.git(&["worktree", "add", "--quiet", "inner", "main"]);
    let inner = sandbox.existing_repository(&bare.root().join("inner"));

    let project = sandbox
        .gikli()
        .open_project(bare.root())
        .expect("open project");

    assert_eq!(project.repositories, vec![]);
    assert_eq!(
        project.unregistered_repositories,
        vec![Repository {
            root: inner.toplevel_by_git()
        }]
    );
}

/// What gikli says does not depend on the language git speaks. git translates its messages only
/// where its translations are installed, as on Linux with the latest git in CI.
#[test]
fn opening_is_classified_the_same_when_git_speaks_another_language() {
    let sandbox = Sandbox::new();
    let plain = sandbox.folder("notes");
    let bare = sandbox.bare_remote("store.git");
    let broken = sandbox.repository("broken");
    fs::write(broken.root().join(".git/config"), "[broken").expect("break the configuration");
    let gikli = sandbox.gikli_in(speaking(sandbox.host_environment(), "de"));

    assert_eq!(
        gikli.open_project(&plain),
        Err(Failure::NotInRepository {
            folder: plain.clone()
        })
    );
    assert_eq!(
        gikli.open_project(bare.root()),
        Err(Failure::NotAWorkingTree {
            folder: bare.root().to_path_buf()
        })
    );
    assert_eq!(
        gikli.open_project(broken.root()),
        Err(Failure::ProjectNotOpened {
            folder: broken.root().to_path_buf()
        })
    );
    assert!(!sandbox.is_inside_repository_by_git(&plain));
    assert_eq!(bare.git(&["rev-parse", "--is-bare-repository"]), "true\n");
    assert!(!broken.git_succeeds(&["rev-parse", "--git-dir"]));
}

#[test]
fn a_repository_git_cannot_read_is_not_taken_for_no_repository() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    fs::write(repository.root().join(".git/config"), "[broken").expect("break the configuration");
    let gikli = sandbox.gikli();

    let opened = gikli.open_project(repository.root());

    assert_eq!(
        opened,
        Err(Failure::ProjectNotOpened {
            folder: repository.root().to_path_buf()
        })
    );
    let refused = gikli
        .operation_log()
        .into_iter()
        .find(|entry| entry.args.contains(&"rev-parse".to_owned()))
        .expect("the detection is logged");
    assert!(refused.stderr.contains("config"), "{refused:?}");
    assert!(!repository.git_succeeds(&["rev-parse", "--git-dir"]));
}

#[test]
fn a_folder_git_cannot_look_at_is_not_taken_for_no_repository() {
    let sandbox = Sandbox::new();
    let folder = sandbox.folder("notes");
    let gikli = sandbox.gikli();
    gikli.check_git();
    sandbox.configure("[broken");

    let opened = gikli.open_project(&folder);

    assert!(!sandbox.is_inside_repository_by_git(&folder));
    assert_eq!(opened, Err(Failure::ProjectNotOpened { folder }));
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

    let project = sandbox
        .gikli_in(host)
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
