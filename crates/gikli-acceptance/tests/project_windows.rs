//! One window per Project, with several open at once. The shell creates and focuses the windows;
//! the core decides which window shows each Project.

use gikli_acceptance::Sandbox;
use gikli_core::ProjectWindow;

#[test]
fn the_first_project_opens_in_the_window_that_asked() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");

    let shown = gikli.window_for_project("main", &project.folder);

    assert_eq!(shown, ProjectWindow::Here);
    assert_eq!(gikli.project_in_window("main"), Some(project.folder));
}

#[test]
fn another_project_opens_in_a_new_window_and_both_stay_open() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let site = sandbox.repository("site");
    let gikli = sandbox.gikli();
    let app_project = gikli.open_project(app.root()).expect("open app");
    let site_project = gikli.open_project(site.root()).expect("open site");
    gikli.window_for_project("main", &app_project.folder);

    let shown = gikli.window_for_project("main", &site_project.folder);

    let ProjectWindow::New { window } = shown else {
        panic!("expected a new window, got {shown:?}");
    };
    assert_ne!(window, "main");
    assert_eq!(gikli.project_in_window(&window), Some(site_project.folder));
    assert_eq!(gikli.project_in_window("main"), Some(app_project.folder));
}

#[test]
fn a_project_already_open_brings_its_window_forward() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let site = sandbox.repository("site");
    let gikli = sandbox.gikli();
    let app_project = gikli.open_project(app.root()).expect("open app");
    let site_project = gikli.open_project(site.root()).expect("open site");
    gikli.window_for_project("main", &app_project.folder);
    let ProjectWindow::New {
        window: site_window,
    } = gikli.window_for_project("main", &site_project.folder)
    else {
        panic!("expected a new window");
    };

    let shown = gikli.window_for_project(&site_window, &app_project.folder);

    assert_eq!(
        shown,
        ProjectWindow::Open {
            window: "main".to_owned()
        }
    );
}

#[test]
fn each_new_window_gets_a_window_of_its_own() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let folders: Vec<_> = ["one", "two", "three"]
        .into_iter()
        .map(|name| {
            let repository = sandbox.repository(name);
            gikli
                .open_project(repository.root())
                .expect("open project")
                .folder
        })
        .collect();
    gikli.window_for_project("main", &folders[0]);

    let windows: Vec<_> = folders[1..]
        .iter()
        .map(|folder| match gikli.window_for_project("main", folder) {
            ProjectWindow::New { window } => window,
            other => panic!("expected a new window, got {other:?}"),
        })
        .collect();

    assert_ne!(windows[0], windows[1]);
    assert_eq!(
        gikli.project_in_window(&windows[0]),
        Some(folders[1].clone())
    );
    assert_eq!(
        gikli.project_in_window(&windows[1]),
        Some(folders[2].clone())
    );
}

#[test]
fn a_project_whose_window_closed_opens_again_in_a_new_window() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let site = sandbox.repository("site");
    let gikli = sandbox.gikli();
    let app_project = gikli.open_project(app.root()).expect("open app");
    let site_project = gikli.open_project(site.root()).expect("open site");
    gikli.window_for_project("main", &app_project.folder);
    let ProjectWindow::New {
        window: site_window,
    } = gikli.window_for_project("main", &site_project.folder)
    else {
        panic!("expected a new window");
    };

    gikli.window_closed(&site_window);
    let shown = gikli.window_for_project("main", &site_project.folder);

    assert_eq!(gikli.project_in_window(&site_window), None);
    let ProjectWindow::New { window } = shown else {
        panic!("expected a new window, got {shown:?}");
    };
    assert_ne!(window, site_window);
}
