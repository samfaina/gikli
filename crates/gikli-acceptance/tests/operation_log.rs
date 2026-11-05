//! The operation log: every git command gikli runs, with its raw output.

use std::sync::mpsc::Receiver;

use gikli_acceptance::Sandbox;
use gikli_core::{Event, OperationLogEntry};

/// The operation log entries received so far.
fn logged(events: &Receiver<Event>) -> Vec<OperationLogEntry> {
    events
        .try_iter()
        .map(|event| match event {
            Event::OperationLogged(entry) => entry,
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
