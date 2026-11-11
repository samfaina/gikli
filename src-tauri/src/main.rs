//! The gikli desktop app.
//!
//! This shell only connects the UI to the core's application interface: each Tauri command
//! forwards to one [`Gikli`] command, and every core event is re-emitted to the UI as
//! `gikli://event`. Behavior lives in the core.

// No console window next to the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::OsString;
use std::path::PathBuf;

use gikli_core::{
    CapabilityState, ChangeGrouping, ChangeSet, CloneUrlForms, CommitWarning, Failure,
    FailureAction, FileDiff, Gikli, GitCheck, HostEnvironment, LogCursor, LogPage, MoveTarget,
    Notification, NotificationAction, NotificationOutcome, OperationLogEntry, Project,
    ProjectWindow, Prompt, Repository, RollbackPlan, RunningOperation, Settings, StagingSide,
    answer_askpass_request,
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

/// The folder gikli keeps its settings and its Projects in: the one in `GIKLI_CONFIG_FOLDER`
/// (`variable`), or else the app's config folder. The variable lets the smoke test keep gikli away
/// from the developer's own settings and Projects.
fn config_folder(
    variable: Option<OsString>,
    app_config_dir: impl FnOnce() -> tauri::Result<PathBuf>,
) -> tauri::Result<PathBuf> {
    match variable.filter(|folder| !folder.is_empty()) {
        Some(folder) => Ok(std::path::absolute(&folder)?),
        None => app_config_dir(),
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
fn set_clone_submodules_recursively(
    gikli: State<'_, Gikli>,
    recursively: bool,
) -> Result<Settings, Failure> {
    gikli.set_clone_submodules_recursively(recursively)
}

#[tauri::command]
fn set_diff_local_side_editable(
    gikli: State<'_, Gikli>,
    editable: bool,
) -> Result<Settings, Failure> {
    gikli.set_diff_local_side_editable(editable)
}

#[tauri::command]
fn set_warn_on_unusual_commit_state(
    gikli: State<'_, Gikli>,
    warn: bool,
) -> Result<Settings, Failure> {
    gikli.set_warn_on_unusual_commit_state(warn)
}

#[tauri::command]
fn set_clear_commit_message_after_commit(
    gikli: State<'_, Gikli>,
    clear: bool,
) -> Result<Settings, Failure> {
    gikli.set_clear_commit_message_after_commit(clear)
}

#[tauri::command]
fn set_staging_mode(gikli: State<'_, Gikli>, enabled: bool) -> Result<Settings, Failure> {
    gikli.set_staging_mode(enabled)
}

#[tauri::command]
fn set_go_to_next_file_after_last_change(
    gikli: State<'_, Gikli>,
    go_to_next_file: bool,
) -> Result<Settings, Failure> {
    gikli.set_go_to_next_file_after_last_change(go_to_next_file)
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

#[tauri::command]
fn clone_url_forms(gikli: State<'_, Gikli>, url: String) -> Option<CloneUrlForms> {
    gikli.clone_url_forms(&url)
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
                // Tauri's own handler would stop the HTML drag and drop that moves changes.
                .disable_drag_drop_handler()
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
fn set_active_changelist(
    gikli: State<'_, Gikli>,
    repository: Repository,
    id: String,
) -> Result<ChangeSet, Failure> {
    gikli.set_active_changelist(&repository, &id)
}

#[tauri::command(async)]
fn create_changelist(
    gikli: State<'_, Gikli>,
    repository: Repository,
    name: String,
    description: Option<String>,
) -> Result<ChangeSet, Failure> {
    gikli.create_changelist(&repository, &name, description.as_deref())
}

#[tauri::command(async)]
fn rename_changelist(
    gikli: State<'_, Gikli>,
    repository: Repository,
    id: String,
    name: String,
) -> Result<ChangeSet, Failure> {
    gikli.rename_changelist(&repository, &id, &name)
}

#[tauri::command(async)]
fn delete_changelist(
    gikli: State<'_, Gikli>,
    repository: Repository,
    id: String,
) -> Result<ChangeSet, Failure> {
    gikli.delete_changelist(&repository, &id)
}

#[tauri::command(async)]
fn set_changelist_comment(
    gikli: State<'_, Gikli>,
    repository: Repository,
    id: String,
    comment: Option<String>,
) -> Result<ChangeSet, Failure> {
    gikli.set_changelist_comment(&repository, &id, comment.as_deref())
}

#[tauri::command(async)]
fn move_changes(
    gikli: State<'_, Gikli>,
    repository: Repository,
    paths: Vec<String>,
    target: MoveTarget,
    make_active: bool,
) -> Result<ChangeSet, Failure> {
    gikli.move_changes(&repository, &paths, &target, make_active)
}

#[tauri::command(async)]
fn read_diff(
    gikli: State<'_, Gikli>,
    repository: Repository,
    path: String,
) -> Result<FileDiff, Failure> {
    gikli.read_diff(&repository, &path)
}

#[tauri::command(async)]
fn read_staging_diff(
    gikli: State<'_, Gikli>,
    repository: Repository,
    path: String,
    side: StagingSide,
) -> Result<FileDiff, Failure> {
    gikli.read_staging_diff(&repository, &path, side)
}

#[tauri::command(async)]
fn save_unstaged_file(
    gikli: State<'_, Gikli>,
    repository: Repository,
    path: String,
    text: String,
    version: String,
) -> Result<FileDiff, Failure> {
    gikli.save_unstaged_file(&repository, &path, &text, &version)
}

#[tauri::command(async)]
fn save_local_file(
    gikli: State<'_, Gikli>,
    repository: Repository,
    path: String,
    text: String,
    version: String,
) -> Result<FileDiff, Failure> {
    gikli.save_local_file(&repository, &path, &text, &version)
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
fn force_add_files(
    gikli: State<'_, Gikli>,
    repository: Repository,
    paths: Vec<String>,
) -> Result<(), Failure> {
    gikli.force_add_files(&repository, &paths)
}

#[tauri::command(async)]
fn stage_all(gikli: State<'_, Gikli>, repository: Repository) -> Result<(), Failure> {
    gikli.stage_all(&repository)
}

#[tauri::command(async)]
fn stage_tracked(gikli: State<'_, Gikli>, repository: Repository) -> Result<(), Failure> {
    gikli.stage_tracked(&repository)
}

#[tauri::command(async)]
fn unstage_files(
    gikli: State<'_, Gikli>,
    repository: Repository,
    paths: Vec<String>,
) -> Result<(), Failure> {
    gikli.unstage_files(&repository, &paths)
}

#[tauri::command(async)]
fn rollback_plan(
    gikli: State<'_, Gikli>,
    repository: Repository,
    paths: Vec<String>,
) -> Result<RollbackPlan, Failure> {
    gikli.rollback_plan(&repository, &paths)
}

#[tauri::command(async)]
fn staging_rollback_plan(
    gikli: State<'_, Gikli>,
    repository: Repository,
    paths: Vec<String>,
    side: StagingSide,
) -> Result<RollbackPlan, Failure> {
    gikli.staging_rollback_plan(&repository, &paths, side)
}

#[tauri::command(async)]
fn rollback_files(
    gikli: State<'_, Gikli>,
    repository: Repository,
    plan: RollbackPlan,
    delete_added_files: bool,
) -> Result<ChangeSet, Failure> {
    gikli.rollback_files(&repository, &plan, delete_added_files)
}

#[tauri::command(async)]
fn previous_commit_messages(
    gikli: State<'_, Gikli>,
    repository: Repository,
) -> Result<Vec<String>, Failure> {
    gikli.previous_commit_messages(&repository)
}

#[tauri::command(async)]
fn prefilled_commit_message(
    gikli: State<'_, Gikli>,
    repository: Repository,
    paths: Vec<String>,
) -> Result<Option<String>, Failure> {
    gikli.prefilled_commit_message(&repository, &paths)
}

#[tauri::command(async)]
fn commit_warning(
    gikli: State<'_, Gikli>,
    repository: Repository,
) -> Result<Option<CommitWarning>, Failure> {
    gikli.commit_warning(&repository)
}

#[tauri::command(async)]
fn commit_files(
    gikli: State<'_, Gikli>,
    repository: Repository,
    paths: Vec<String>,
    message: String,
) -> Result<ChangeSet, Failure> {
    gikli.commit_files(&repository, &paths, &message)
}

#[tauri::command(async)]
fn amend_last_commit(
    gikli: State<'_, Gikli>,
    repository: Repository,
    paths: Vec<String>,
    message: String,
) -> Result<ChangeSet, Failure> {
    gikli.amend_last_commit(&repository, &paths, &message)
}

#[tauri::command(async)]
fn commit_staged(
    gikli: State<'_, Gikli>,
    repository: Repository,
    message: String,
) -> Result<ChangeSet, Failure> {
    gikli.commit_staged(&repository, &message)
}

#[tauri::command(async)]
fn amend_with_staged(
    gikli: State<'_, Gikli>,
    repository: Repository,
    message: String,
) -> Result<ChangeSet, Failure> {
    gikli.amend_with_staged(&repository, &message)
}

#[tauri::command(async)]
fn last_commit_message(
    gikli: State<'_, Gikli>,
    repository: Repository,
) -> Result<Option<String>, Failure> {
    gikli.last_commit_message(&repository)
}

#[tauri::command(async)]
fn set_commit_identity(
    gikli: State<'_, Gikli>,
    repository: Repository,
    name: Option<String>,
    email: Option<String>,
    everywhere: bool,
) -> Result<(), Failure> {
    gikli.set_commit_identity(&repository, name.as_deref(), email.as_deref(), everywhere)
}

#[tauri::command(async)]
fn read_log(
    gikli: State<'_, Gikli>,
    repository: Repository,
    after: Option<LogCursor>,
    count: usize,
) -> Result<LogPage, Failure> {
    gikli.read_log(&repository, after.as_ref(), count)
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
            let config_folder = config_folder(std::env::var_os("GIKLI_CONFIG_FOLDER"), || {
                app.path().app_config_dir()
            })?;
            let mut gikli = Gikli::new(HostEnvironment::current(), &config_folder);
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
            set_clone_submodules_recursively,
            set_diff_local_side_editable,
            set_go_to_next_file_after_last_change,
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
            clone_url_forms,
            clone_repository,
            running_operations,
            cancel_operation,
            window_for_project,
            read_changes,
            set_active_changelist,
            create_changelist,
            rename_changelist,
            delete_changelist,
            set_changelist_comment,
            move_changes,
            read_diff,
            read_staging_diff,
            save_local_file,
            save_unstaged_file,
            add_files,
            force_add_files,
            stage_all,
            stage_tracked,
            unstage_files,
            rollback_plan,
            staging_rollback_plan,
            rollback_files,
            previous_commit_messages,
            prefilled_commit_message,
            commit_warning,
            commit_files,
            amend_last_commit,
            commit_staged,
            amend_with_staged,
            last_commit_message,
            set_commit_identity,
            read_log,
            set_warn_on_unusual_commit_state,
            set_clear_commit_message_after_commit,
            set_staging_mode,
            watch_changes,
            unwatch_changes,
            startup_folder
        ])
        .run(tauri::generate_context!())
        .expect("gikli failed to start");
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::config_folder;

    fn app_config_dir() -> tauri::Result<PathBuf> {
        Ok(PathBuf::from("app config dir"))
    }

    #[test]
    fn the_config_folder_is_the_one_in_gikli_config_folder() {
        let folder = std::env::temp_dir().join("gikli settings");

        let chosen =
            config_folder(Some(folder.clone().into()), app_config_dir).expect("config folder");

        assert_eq!(chosen, folder);
    }

    #[test]
    fn a_relative_config_folder_resolves_against_the_current_directory() {
        let chosen = config_folder(Some(OsString::from("gikli settings")), app_config_dir)
            .expect("config folder");

        let current = std::env::current_dir().expect("current directory");
        assert_eq!(chosen, current.join("gikli settings"));
    }

    #[test]
    fn without_gikli_config_folder_the_config_folder_is_the_app_config_dir() {
        for variable in [None, Some(OsString::new())] {
            let chosen = config_folder(variable, app_config_dir).expect("config folder");

            assert_eq!(chosen, PathBuf::from("app config dir"));
        }
    }
}
