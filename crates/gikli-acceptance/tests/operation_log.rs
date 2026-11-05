//! The operation log: every git command gikli runs, with its raw output.

use std::sync::mpsc::Receiver;

use gikli_acceptance::Sandbox;
use gikli_core::{Event, GitCheck, OperationLogEntry};

/// The operation log entries received so far.
fn logged(events: &Receiver<Event>) -> Vec<OperationLogEntry> {
    events
        .try_iter()
        .filter_map(|event| match event {
            Event::OperationLogged(entry) => Some(entry),
            _ => None,
        })
        .collect()
}

#[test]
fn every_git_command_reaches_the_operation_log_with_its_output() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    gikli.open_project(repository.root()).expect("open project");

    let entries = logged(&events);
    let version_check = entries
        .iter()
        .find(|entry| entry.args.contains(&"--version".to_owned()))
        .expect("version check logged");
    assert!(version_check.stdout.starts_with("git version "));
    let detection = entries
        .iter()
        .find(|entry| entry.args.contains(&"rev-parse".to_owned()))
        .expect("repository detection logged");
    assert_eq!(detection.exit_code, Some(0));
    let reported_root = dunce::canonicalize(detection.stdout.trim_end()).expect("a path");
    assert_eq!(reported_root, repository.toplevel_by_git());
}

#[test]
fn a_failing_git_command_is_logged_with_its_error_output() {
    let sandbox = Sandbox::new();
    let folder = sandbox.folder("notes");
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    gikli
        .open_project(&folder)
        .expect_err("a folder outside any repository");

    let failed = logged(&events)
        .into_iter()
        .find(|entry| entry.exit_code != Some(0))
        .expect("failed detection logged");
    assert!(!failed.stderr.is_empty());
    assert!(!sandbox.is_inside_repository_by_git(&folder));
}

#[test]
fn the_operation_log_keeps_every_git_command_in_the_order_gikli_ran_them() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes.txt", "draft\n");
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    let project = gikli.open_project(repository.root()).expect("open project");
    gikli
        .read_changes(&project.repositories[0])
        .expect("read changes");

    let log = gikli.operation_log();
    assert_eq!(log, logged(&events));
    let status = log
        .iter()
        .rev()
        .find(|entry| entry.args.contains(&"status".to_owned()))
        .expect("status read logged");
    assert!(status.stdout.contains("notes.txt"));
    assert_eq!(repository.status_by_git(), ["?? notes.txt"]);
}

#[test]
fn clearing_the_operation_log_empties_it_and_later_commands_are_logged_again() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(repository.root()).expect("open project");
    let before = gikli.operation_log();
    let events = gikli.subscribe();

    gikli.clear_operation_log();

    assert!(gikli.operation_log().is_empty());
    let cleared_before = events
        .try_iter()
        .find_map(|event| match event {
            Event::OperationLogCleared { before } => Some(before),
            _ => None,
        })
        .expect("every window learns the log was cleared");
    gikli
        .read_changes(&project.repositories[0])
        .expect("read changes");
    let after = gikli.operation_log();
    assert_eq!(after.len(), 1);
    assert!(after[0].args.contains(&"status".to_owned()));
    assert!(
        before.iter().all(|entry| entry.id < cleared_before) && after[0].id >= cleared_before,
        "a window can tell the cleared entries from the new ones"
    );
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn the_operation_log_does_not_wrap_long_lines_until_the_user_turns_soft_wrap_on() {
    let sandbox = Sandbox::new();
    assert!(!sandbox.gikli().settings().operation_log_soft_wrap);

    let settings = sandbox
        .gikli()
        .set_operation_log_soft_wrap(true)
        .expect("turn soft wrap on");

    assert!(settings.operation_log_soft_wrap);
    assert!(
        sandbox.gikli().settings().operation_log_soft_wrap,
        "the choice survives a restart"
    );
}

#[test]
fn a_git_that_cannot_start_is_logged_with_the_reason() {
    let sandbox = Sandbox::new();
    let broken = sandbox
        .folder("broken-git")
        .join(format!("git{}", std::env::consts::EXE_SUFFIX));
    // Not a program: Windows refuses to start it, and elsewhere it is not executable.
    std::fs::write(&broken, "not a program\n").expect("write broken git");
    let gikli = sandbox.gikli();

    let check = gikli
        .set_git_path(Some(broken.clone()))
        .expect("save the path to git");

    assert!(matches!(check, GitCheck::Unrecognized { .. }));
    let entry = gikli
        .operation_log()
        .into_iter()
        .find(|entry| entry.args.contains(&"--version".to_owned()))
        .expect("the attempt is logged");
    assert_eq!(entry.exit_code, None);
    assert!(
        entry
            .error
            .as_deref()
            .is_some_and(|reason| !reason.is_empty()),
        "the log says why git did not start"
    );
}
