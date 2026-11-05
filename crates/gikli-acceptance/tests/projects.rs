//! Projects and their Repositories: the Repositories a Project registers, the ones gikli finds
//! inside its folder and offers to add or ignore, and the ones that broke (ADR-0004).

use std::fs;
use std::path::PathBuf;

use gikli_acceptance::Sandbox;
use gikli_core::{Failure, Project, Repository};

fn roots(repositories: &[Repository]) -> Vec<PathBuf> {
    repositories
        .iter()
        .map(|repository| repository.root.clone())
        .collect()
}

fn registered(project: &Project) -> Vec<PathBuf> {
    roots(&project.repositories)
}

fn offered(project: &Project) -> Vec<PathBuf> {
    roots(&project.unregistered_repositories)
}

fn invalid(project: &Project) -> Vec<PathBuf> {
    roots(&project.invalid_repositories)
}

fn ignored(project: &Project) -> Vec<PathBuf> {
    roots(&project.ignored_repositories)
}

#[test]
fn nested_repositories_that_are_not_registered_are_offered() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let docs = sandbox.repository("app/docs");

    let project = sandbox
        .gikli()
        .open_project(app.root())
        .expect("open project");

    assert_eq!(registered(&project), vec![app.toplevel_by_git()]);
    assert_eq!(
        offered(&project),
        vec![docs.toplevel_by_git(), engine.toplevel_by_git()]
    );
    assert_eq!(app.status_by_git(), vec!["?? docs/", "?? libs/engine/"]);
}

#[test]
fn an_offered_repository_that_is_added_stays_registered_after_a_restart() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let docs = sandbox.repository("app/docs");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");

    let added = gikli
        .add_repository(&project.folder, &project.unregistered_repositories[1])
        .expect("add repository");

    let expected_registered = vec![app.toplevel_by_git(), engine.toplevel_by_git()];
    assert_eq!(registered(&added), expected_registered);
    assert_eq!(offered(&added), vec![docs.toplevel_by_git()]);
    let reopened = sandbox
        .gikli()
        .open_project(app.root())
        .expect("reopen project");
    assert_eq!(reopened, added);
}

#[test]
fn an_offered_repository_that_is_ignored_is_not_offered_again_after_a_restart() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");

    let after = gikli
        .ignore_repository(&project.folder, &project.unregistered_repositories[0])
        .expect("ignore repository");

    assert_eq!(registered(&after), vec![app.toplevel_by_git()]);
    assert_eq!(offered(&after), Vec::<PathBuf>::new());
    let reopened = sandbox
        .gikli()
        .open_project(app.root())
        .expect("reopen project");
    assert_eq!(reopened, after);
    assert_eq!(ignored(&reopened), vec![engine.toplevel_by_git()]);
    // Ignoring leaves the repository alone: git still finds it, and its parent still sees it.
    assert_eq!(engine.toplevel_by_git(), engine.root());
    assert_eq!(app.status_by_git(), vec!["?? libs/engine/"]);
}

#[test]
fn a_folder_holding_only_nested_repositories_opens_with_each_of_them_offered() {
    let sandbox = Sandbox::new();
    let folder = sandbox.folder("workspace");
    let api = sandbox.repository("workspace/api");
    let web = sandbox.repository("workspace/web");

    let project = sandbox.gikli().open_project(&folder).expect("open project");

    assert_eq!(registered(&project), Vec::<PathBuf>::new());
    assert_eq!(
        offered(&project),
        vec![api.toplevel_by_git(), web.toplevel_by_git()]
    );
    assert!(!sandbox.is_inside_repository_by_git(&folder));
}

#[test]
fn a_registered_repository_whose_git_directory_is_gone_is_marked_invalid() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");
    let engine_root = engine.toplevel_by_git();
    gikli
        .add_repository(&project.folder, &project.unregistered_repositories[0])
        .expect("add repository");

    fs::remove_dir_all(engine.root().join(".git")).expect("delete git directory");
    let reopened = sandbox
        .gikli()
        .open_project(app.root())
        .expect("reopen project");

    assert_eq!(registered(&reopened), vec![app.toplevel_by_git()]);
    assert_eq!(invalid(&reopened), vec![engine_root]);
    assert_eq!(offered(&reopened), Vec::<PathBuf>::new());
    assert_eq!(engine.toplevel_by_git(), app.toplevel_by_git());
}

#[test]
fn a_registered_repository_whose_folder_is_gone_is_marked_invalid() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");
    let engine_root = engine.toplevel_by_git();
    gikli
        .add_repository(&project.folder, &project.unregistered_repositories[0])
        .expect("add repository");

    fs::remove_dir_all(app.root().join("libs")).expect("delete repository folder");
    let reopened = gikli.open_project(app.root()).expect("reopen project");

    assert_eq!(registered(&reopened), vec![app.toplevel_by_git()]);
    assert_eq!(invalid(&reopened), vec![engine_root.clone()]);
    assert!(!engine_root.exists());
}

#[test]
fn a_project_whose_only_repository_is_gone_still_opens_with_it_marked_invalid() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let root = app.toplevel_by_git();
    sandbox
        .gikli()
        .open_project(app.root())
        .expect("open project");

    fs::remove_dir_all(app.root().join(".git")).expect("delete git directory");
    let reopened = sandbox
        .gikli()
        .open_project(app.root())
        .expect("reopen project");

    assert_eq!(registered(&reopened), Vec::<PathBuf>::new());
    assert_eq!(invalid(&reopened), vec![root.clone()]);
    assert!(!sandbox.is_inside_repository_by_git(&root));
}

#[test]
fn a_folder_that_is_not_a_repository_root_cannot_be_added() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let src = app.folder("src");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");

    let added = gikli.add_repository(&project.folder, &Repository { root: src.clone() });

    assert_eq!(
        added,
        Err(Failure::NotARepositoryRoot {
            folder: src.clone()
        })
    );
    assert_eq!(gikli.open_project(app.root()), Ok(project));
    assert_eq!(
        sandbox.existing_repository(&src).toplevel_by_git(),
        app.toplevel_by_git()
    );
}

#[test]
fn a_new_repository_is_created_in_an_empty_folder() {
    let sandbox = Sandbox::new();
    let folder = sandbox.folder("new app");

    let repository = sandbox
        .gikli()
        .create_repository(&folder)
        .expect("create repository");

    let created = sandbox.existing_repository(&folder);
    assert_eq!(
        repository,
        Repository {
            root: created.toplevel_by_git()
        }
    );
    assert_eq!(
        created.git(&["symbolic-ref", "HEAD"]),
        "refs/heads/main
"
    );
}

#[test]
fn a_new_repository_is_created_in_a_folder_that_does_not_exist_yet() {
    let sandbox = Sandbox::new();
    let folder = sandbox.root().join("projects").join("año nuevo");

    let repository = sandbox
        .gikli()
        .create_repository(&folder)
        .expect("create repository");

    let created = sandbox.existing_repository(&folder);
    assert_eq!(
        repository,
        Repository {
            root: created.toplevel_by_git()
        }
    );
    assert_eq!(repository.root, folder);
}

#[test]
fn a_repository_is_not_created_over_an_existing_one() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    app.write(
        "README.md",
        "hello
",
    );
    app.commit_all("Initial commit");

    let created = sandbox.gikli().create_repository(app.root());

    assert_eq!(
        created,
        Err(Failure::RepositoryExists {
            folder: app.toplevel_by_git()
        })
    );
    assert_eq!(
        app.git(&["log", "--format=%s"]),
        "Initial commit
"
    );
}

#[test]
fn a_repository_created_inside_a_project_is_offered_and_can_be_added() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");

    let plugin = gikli
        .create_repository(&app.root().join("plugins").join("lint"))
        .expect("create repository");
    let added = gikli
        .add_repository(&project.folder, &plugin)
        .expect("add repository");

    assert_eq!(
        roots(&added.repositories),
        vec![app.toplevel_by_git(), plugin.root.clone()]
    );
    assert_eq!(app.status_by_git(), vec!["?? plugins/lint/"]);
}

#[test]
fn the_changes_of_each_repository_in_a_project_carry_their_repository() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    app.write("README.md", "app\n");
    engine.write("engine.rs", "fn main() {}\n");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");
    let project = gikli
        .add_repository(&project.folder, &project.unregistered_repositories[0])
        .expect("add repository");

    let change_sets: Vec<_> = project
        .repositories
        .iter()
        .map(|repository| gikli.read_changes(repository).expect("read changes"))
        .collect();

    let read: Vec<_> = change_sets
        .iter()
        .map(|set| {
            let paths: Vec<_> = set
                .changes
                .iter()
                .map(|change| change.path.clone())
                .collect();
            (set.repository.root.clone(), paths)
        })
        .collect();
    assert_eq!(
        read,
        vec![
            (
                app.toplevel_by_git(),
                vec![PathBuf::from("README.md"), PathBuf::from("libs/engine/")]
            ),
            (engine.toplevel_by_git(), vec![PathBuf::from("engine.rs")]),
        ]
    );
    assert_eq!(app.status_by_git(), vec!["?? README.md", "?? libs/engine/"]);
    assert_eq!(engine.status_by_git(), vec!["?? engine.rs"]);
}

#[test]
fn an_ignored_repository_can_be_added_after_all() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");
    let project = gikli
        .ignore_repository(&project.folder, &project.unregistered_repositories[0])
        .expect("ignore repository");

    let added = gikli
        .add_repository(&project.folder, &project.ignored_repositories[0])
        .expect("add repository");

    assert_eq!(
        registered(&added),
        vec![app.toplevel_by_git(), engine.toplevel_by_git()]
    );
    assert_eq!(ignored(&added), Vec::<PathBuf>::new());
    assert_eq!(engine.toplevel_by_git(), engine.root());
}

#[test]
fn a_folder_whose_nested_repositories_are_all_ignored_still_opens() {
    let sandbox = Sandbox::new();
    let folder = sandbox.folder("workspace");
    let api = sandbox.repository("workspace/api");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(&folder).expect("open project");
    gikli
        .ignore_repository(&project.folder, &project.unregistered_repositories[0])
        .expect("ignore repository");

    let reopened = sandbox
        .gikli()
        .open_project(&folder)
        .expect("reopen project");

    assert_eq!(registered(&reopened), Vec::<PathBuf>::new());
    assert_eq!(offered(&reopened), Vec::<PathBuf>::new());
    assert_eq!(ignored(&reopened), vec![api.toplevel_by_git()]);
    assert!(!sandbox.is_inside_repository_by_git(&folder));
}

#[test]
fn an_invalid_repository_can_be_removed_from_the_project() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");
    gikli
        .add_repository(&project.folder, &project.unregistered_repositories[0])
        .expect("add repository");
    fs::remove_dir_all(engine.root().join(".git")).expect("delete git directory");
    let broken = gikli.open_project(app.root()).expect("reopen project");

    let removed = gikli
        .remove_repository(&broken.folder, &broken.invalid_repositories[0])
        .expect("remove repository");

    assert_eq!(registered(&removed), vec![app.toplevel_by_git()]);
    assert_eq!(invalid(&removed), Vec::<PathBuf>::new());
    let reopened = sandbox
        .gikli()
        .open_project(app.root())
        .expect("reopen project");
    assert_eq!(reopened, removed);
    assert_eq!(engine.toplevel_by_git(), app.toplevel_by_git());
}

#[test]
fn a_repository_outside_the_project_folder_cannot_be_added() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let site = sandbox.repository("site");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");

    let added = gikli.add_repository(
        &project.folder,
        &Repository {
            root: site.toplevel_by_git(),
        },
    );

    assert_eq!(
        added,
        Err(Failure::RepositoryOutsideProject {
            folder: site.toplevel_by_git()
        })
    );
    assert_eq!(gikli.open_project(app.root()), Ok(project));
    assert_eq!(site.toplevel_by_git(), site.root());
}

#[test]
fn the_repository_above_a_project_subfolder_stays_registered_when_another_is_added() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let libs = app.root().join("libs");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(&libs).expect("open project");

    let added = gikli
        .add_repository(&project.folder, &project.unregistered_repositories[0])
        .expect("add repository");

    assert_eq!(
        registered(&added),
        vec![app.toplevel_by_git(), engine.toplevel_by_git()]
    );
    assert_eq!(engine.toplevel_by_git(), engine.root());
}

#[test]
fn a_choice_that_cannot_be_saved_is_reported_and_changes_nothing() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    // A folder where the registry file belongs: it cannot be written.
    fs::create_dir_all(sandbox.config_folder().join("projects.json")).expect("block registry");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");

    let added = gikli.add_repository(&project.folder, &project.unregistered_repositories[0]);

    assert!(
        matches!(added, Err(Failure::ProjectNotSaved { .. })),
        "{added:?}"
    );
    assert_eq!(gikli.open_project(app.root()), Ok(project));
    assert_eq!(app.status_by_git(), vec!["?? libs/engine/"]);
    assert_eq!(engine.toplevel_by_git(), engine.root());
}
