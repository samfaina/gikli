//! The gikli desktop app.
//!
//! This shell only connects the UI to the core's application interface: each Tauri command
//! forwards to one [`Gikli`] command, and every core event is re-emitted to the UI as
//! `gikli://event`. Behavior lives in the core.

// No console window next to the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use gikli_core::{
    CapabilityState, ChangeGrouping, ChangeSet, Failure, FailureAction, Gikli, GitCheck,
    HostEnvironment, Notification, NotificationAction, NotificationOutcome, OperationLogEntry,
    Project, ProjectWindow, Prompt, Repository, RunningOperation, Settings, answer_askpass_request,
};
use tauri::{
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent,
};

/// The folder the first window opens as a Project at startup: the one given on the command line
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
fn test_git(gikli: State<'_, Gikli>, executable: PathBuf) -> GitCheck {
    gikli.test_git(&executable)
}

#[tauri::command(async)]
fn set_git_path(gikli: State<'_, Gikli>, path: Option<PathBuf>) -> Result<GitCheck, Failure> {
    gikli.set_git_path(path)
}

#[tauri::command]
fn settings(gikli: State<'_, Gikli>) -> Settings {
    gikli.settings()
}

#[tauri::command]
fn set_show_ignored_files(gikli: State<'_, Gikli>, show: bool) -> Result<Settings, Failure> {
    gikli.set_show_ignored_files(show)
}

#[tauri::command]
fn set_change_grouping(
    gikli: State<'_, Gikli>,
    grouping: ChangeGrouping,
) -> Result<Settings, Failure> {
    gikli.set_change_grouping(grouping)
}

#[tauri::command]
fn set_operation_log_soft_wrap(
    gikli: State<'_, Gikli>,
    soft_wrap: bool,
) -> Result<Settings, Failure> {
    gikli.set_operation_log_soft_wrap(soft_wrap)
}

#[tauri::command]
fn set_use_credential_helper(
    gikli: State<'_, Gikli>,
    use_helper: bool,
) -> Result<Settings, Failure> {
    gikli.set_use_credential_helper(use_helper)
}

#[tauri::command]
fn pending_prompts(gikli: State<'_, Gikli>) -> Vec<Prompt> {
    gikli.pending_prompts()
}

#[tauri::command]
fn answer_prompt(gikli: State<'_, Gikli>, id: u64, answer: String) -> Result<(), Failure> {
    gikli.answer_prompt(id, &answer)
}

#[tauri::command]
fn cancel_prompt(gikli: State<'_, Gikli>, id: u64) -> Result<(), Failure> {
    gikli.cancel_prompt(id)
}

#[tauri::command]
fn operation_log(gikli: State<'_, Gikli>) -> Vec<OperationLogEntry> {
    gikli.operation_log()
}

#[tauri::command]
fn clear_operation_log(gikli: State<'_, Gikli>) {
    gikli.clear_operation_log();
}

#[tauri::command]
fn notifications(gikli: State<'_, Gikli>) -> Vec<Notification> {
    gikli.notifications()
}

#[tauri::command(async)]
fn act_on_notification(
    gikli: State<'_, Gikli>,
    id: u64,
    action: NotificationAction,
) -> Result<NotificationOutcome, Failure> {
    gikli.act_on_notification(id, &action)
}

#[tauri::command(async)]
fn take_failure_action(gikli: State<'_, Gikli>, action: FailureAction) -> Result<(), Failure> {
    gikli.take_failure_action(&action)
}

#[tauri::command]
fn dismiss_notification(gikli: State<'_, Gikli>, id: u64) {
    gikli.dismiss_notification(id);
}

#[tauri::command(async)]
fn capabilities(
    gikli: State<'_, Gikli>,
    repository: Repository,
) -> Result<Vec<CapabilityState>, Failure> {
    gikli.capabilities(&repository)
}

#[tauri::command(async)]
fn open_project(gikli: State<'_, Gikli>, folder: PathBuf) -> Result<Project, Failure> {
    gikli.open_project(&folder)
}

#[tauri::command(async)]
fn add_repository(
    gikli: State<'_, Gikli>,
    folder: PathBuf,
    repository: Repository,
) -> Result<Project, Failure> {
    gikli.add_repository(&folder, &repository)
}

#[tauri::command(async)]
fn ignore_repository(
    gikli: State<'_, Gikli>,
    folder: PathBuf,
    repository: Repository,
) -> Result<Project, Failure> {
    gikli.ignore_repository(&folder, &repository)
}

#[tauri::command(async)]
fn remove_repository(
    gikli: State<'_, Gikli>,
    folder: PathBuf,
    repository: Repository,
) -> Result<Project, Failure> {
    gikli.remove_repository(&folder, &repository)
}

#[tauri::command]
fn running_operations(gikli: State<'_, Gikli>) -> Vec<RunningOperation> {
    gikli.running_operations()
}

#[tauri::command]
fn cancel_operation(gikli: State<'_, Gikli>, id: u64) {
    gikli.cancel_operation(id);
}

#[tauri::command(async)]
fn clone_repository(
    gikli: State<'_, Gikli>,
    url: String,
    folder: PathBuf,
) -> Result<Project, Failure> {
    gikli.clone_repository(&url, &folder)
}

#[tauri::command(async)]
fn create_repository(gikli: State<'_, Gikli>, folder: PathBuf) -> Result<Repository, Failure> {
    gikli.create_repository(&folder)
}

/// Shows the Project at `folder` in its own window: the calling window when it shows no other
/// Project, the window that already shows it, brought to the front, or a new window. Async, as
/// creating a window from a synchronous command deadlocks on Windows.
#[tauri::command(async)]
fn window_for_project(
    app: AppHandle,
    window: Window,
    gikli: State<'_, Gikli>,
    folder: PathBuf,
) -> tauri::Result<ProjectWindow> {
    let shown = gikli.window_for_project(window.label(), &folder);
    match &shown {
        ProjectWindow::Here => {}
        ProjectWindow::Open { window } => {
            if let Some(open) = app.get_webview_window(window) {
                open.unminimize()?;
                open.set_focus()?;
            }
        }
        ProjectWindow::New { window } => {
            let created = WebviewWindowBuilder::new(&app, window, WebviewUrl::default())
                .title("gikli")
                .inner_size(960.0, 640.0)
                .min_inner_size(480.0, 320.0)
                .build();
            if let Err(error) = created {
                gikli.window_closed(window);
                return Err(error);
            }
        }
    }
    Ok(shown)
}

#[tauri::command(async)]
fn read_changes(gikli: State<'_, Gikli>, repository: Repository) -> Result<ChangeSet, Failure> {
    gikli.read_changes(&repository)
}

#[tauri::command(async)]
fn add_files(
    gikli: State<'_, Gikli>,
    repository: Repository,
    paths: Vec<String>,
) -> Result<(), Failure> {
    gikli.add_files(&repository, &paths)
}

#[tauri::command(async)]
fn watch_changes(gikli: State<'_, Gikli>, repository: Repository) -> Result<(), Failure> {
    gikli.watch_changes(&repository)
}

#[tauri::command]
fn unwatch_changes(gikli: State<'_, Gikli>, repository: Repository) {
    gikli.unwatch_changes(&repository);
}

/// The folder the calling window opens as a Project when it starts: the one it was created for,
/// or for the first window the one given at startup.
#[tauri::command]
fn startup_folder(
    window: Window,
    gikli: State<'_, Gikli>,
    startup: State<'_, StartupFolder>,
) -> Option<PathBuf> {
    gikli.project_in_window(window.label()).or_else(|| {
        (window.label() == "main")
            .then(|| startup.0.clone())
            .flatten()
    })
}

fn main() {
    // git and ssh start this same program to ask the user something.
    if let Some(code) = answer_askpass_request() {
        std::process::exit(code);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(StartupFolder::from_process())
        .setup(|app| {
            let mut gikli = Gikli::new(HostEnvironment::current(), &app.path().app_config_dir()?);
            // Without prompts gikli still runs, and what needs an answer fails with a message.
            if let Err(error) =
                std::env::current_exe().and_then(|askpass| gikli.bring_prompts_through(askpass))
            {
                eprintln!("gikli cannot bring prompts to the user: {error}");
            }
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
        .on_window_event(|window, event| {
            if let WindowEvent::Destroyed = event {
                window.state::<Gikli>().window_closed(window.label());
            }
        })
        .invoke_handler(tauri::generate_handler![
            check_git,
            test_git,
            set_git_path,
            settings,
            set_show_ignored_files,
            set_change_grouping,
            set_operation_log_soft_wrap,
            set_use_credential_helper,
            pending_prompts,
            answer_prompt,
            cancel_prompt,
            operation_log,
            clear_operation_log,
            notifications,
            act_on_notification,
            dismiss_notification,
            take_failure_action,
            capabilities,
            open_project,
            add_repository,
            ignore_repository,
            remove_repository,
            create_repository,
            clone_repository,
            running_operations,
            cancel_operation,
            window_for_project,
            read_changes,
            add_files,
            watch_changes,
            unwatch_changes,
            startup_folder
        ])
        .run(tauri::generate_context!())
        .expect("gikli failed to start");
}
