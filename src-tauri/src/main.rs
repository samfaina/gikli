//! The gikli desktop app.
//!
//! This shell only connects the UI to the core's application interface: each Tauri command
//! forwards to one [`Gikli`] command, and every core event is re-emitted to the UI as
//! `gikli://event`. Behavior lives in the core.

// No console window next to the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use gikli_core::{ChangeSet, Failure, Gikli, GitCheck, HostEnvironment, Project, Repository};
use tauri::{Emitter, Manager, State};

/// The folder opened as a Project at startup: the one given on the command line
/// (`gikli <folder>`), or else the one in `GIKLI_PROJECT_FOLDER`. The variable exists for
/// launchers that cannot pass arguments, such as WebDriver with WebView2 on Windows.
struct StartupFolder(Option<PathBuf>);

impl StartupFolder {
    fn from_process() -> Self {
        let folder = std::env::args_os()
            .nth(1)
            .or_else(|| std::env::var_os("GIKLI_PROJECT_FOLDER"))
            .filter(|folder| !folder.is_empty())
            .map(PathBuf::from)
            .map(|folder| std::path::absolute(&folder).unwrap_or(folder));
        Self(folder)
    }
}

#[tauri::command(async)]
fn check_git(gikli: State<'_, Gikli>) -> GitCheck {
    gikli.check_git()
}

#[tauri::command(async)]
fn open_project(gikli: State<'_, Gikli>, folder: PathBuf) -> Result<Project, Failure> {
    gikli.open_project(&folder)
}

#[tauri::command(async)]
fn read_changes(gikli: State<'_, Gikli>, repository: Repository) -> Result<ChangeSet, Failure> {
    gikli.read_changes(&repository)
}

#[tauri::command]
fn startup_folder(startup: State<'_, StartupFolder>) -> Option<PathBuf> {
    startup.0.clone()
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(StartupFolder::from_process())
        .setup(|app| {
            let gikli = Gikli::new(HostEnvironment::current());
            let events = gikli.subscribe();
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                for event in events {
                    // The window may be closing; a lost event is harmless then.
                    let _ = handle.emit("gikli://event", &event);
                }
            });
            app.manage(gikli);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            check_git,
            open_project,
            read_changes,
            startup_folder
        ])
        .run(tauri::generate_context!())
        .expect("gikli failed to start");
}
