//! Long operations: they report progress while they run, and the rest of gikli keeps answering
//! meanwhile. Clone is the first of them.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gikli_acceptance::{Sandbox, TestRepository};
use gikli_core::{Event, Failure, LongOperation, Progress, Repository};

/// How long the tests wait for the clone to start before failing. Generous, for slow CI machines.
const DEADLINE: Duration = Duration::from_secs(30);

/// A `file://` URL for `folder`, so git clones through its transport and reports progress as it
/// does from a server, instead of copying the folder.
fn file_url(folder: &Path) -> String {
    let path = folder.to_string_lossy().replace('\\', "/");
    format!("file:///{}", path.trim_start_matches('/'))
}

/// A bare remote holding a few commits, pushed from a working repository.
fn remote_with_commits(sandbox: &Sandbox) -> TestRepository {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.repository("work");
    for index in 0..3 {
        work.write(&format!("file-{index}.txt"), &format!("content {index}\n"));
        work.commit_all(&format!("Commit {index}"));
    }
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main"]);
    remote
}

/// The progress the operation `id` reported, in order.
fn progress_of(events: &[Event], id: u64) -> Vec<Progress> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::OperationProgress {
                id: reporter,
                progress,
            } if *reporter == id => Some(progress.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_clone_reports_its_progress_and_opens_as_a_project() {
    let sandbox = Sandbox::new();
    let remote = remote_with_commits(&sandbox);
    let target = sandbox.root().join("clone");
    let url = file_url(remote.root());
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    let project = gikli
        .clone_repository(&url, &target)
        .expect("clone the remote");

    let received: Vec<Event> = events.try_iter().collect();
    let started = received
        .iter()
        .find_map(|event| match event {
            Event::OperationStarted { id, operation } => Some((*id, operation.clone())),
            _ => None,
        })
        .expect("the clone announces it started");
    assert_eq!(
        started.1,
        LongOperation::Clone {
            url: url.clone(),
            folder: target.clone()
        }
    );
    let reported = progress_of(&received, started.0);
    assert!(
        reported.iter().any(|progress| progress.percent == 100),
        "the clone reports progress up to the end: {reported:?}"
    );
    assert!(
        reported.iter().all(|progress| !progress.phase.is_empty()),
        "each report names its phase"
    );
    assert!(received.contains(&Event::OperationFinished { id: started.0 }));
    let clone = sandbox.existing_repository(&target);
    assert_eq!(
        project.repositories,
        vec![Repository {
            root: clone.toplevel_by_git()
        }]
    );
    assert_eq!(
        clone.git(&["rev-parse", "HEAD"]),
        remote.git(&["rev-parse", "main"])
    );
}

/// Makes every clone from the sandbox's remotes wait, before the remote sends anything, until
/// the returned file exists. Holds the clone while the test checks what gikli does meanwhile.
fn hold_clones(sandbox: &Sandbox) -> PathBuf {
    let release = sandbox.root().join("release-clone");
    let hook = sandbox.root().join("hold-clone.sh");
    let forward = |path: &Path| path.to_string_lossy().replace('\\', "/");
    fs::write(
        &hook,
        format!(
            "while [ ! -f '{}' ]; do sleep 0.05; done\nexec \"$@\"\n",
            forward(&release)
        ),
    )
    .expect("write hook");
    // Read by git's upload-pack only from protected configuration, such as the global one.
    sandbox.configure(&format!(
        "[uploadpack]\n\tpackObjectsHook = sh '{}'\n",
        forward(&hook)
    ));
    release
}

#[test]
fn the_rest_of_gikli_keeps_answering_while_a_clone_runs() {
    let sandbox = Sandbox::new();
    let remote = remote_with_commits(&sandbox);
    let other = sandbox.repository("other");
    other.write("notes.txt", "draft\n");
    let release = hold_clones(&sandbox);
    let target = sandbox.root().join("clone");
    let url = file_url(remote.root());
    let gikli = sandbox.gikli();
    let project = gikli.open_project(other.root()).expect("open project");
    let events = gikli.subscribe();

    std::thread::scope(|scope| {
        let clone = scope.spawn(|| gikli.clone_repository(&url, &target));
        loop {
            match events.recv_timeout(DEADLINE) {
                Ok(Event::OperationStarted { .. }) => break,
                Ok(_) => {}
                Err(error) => panic!("the clone never started: {error}"),
            }
        }

        let changes = gikli
            .read_changes(&project.repositories[0])
            .expect("read changes while the clone runs");
        let log = gikli.operation_log();
        let still_running = !clone.is_finished();
        fs::write(&release, "").expect("release the clone");
        clone
            .join()
            .expect("clone thread")
            .expect("clone the remote");

        assert!(still_running, "the clone was held while gikli answered");
        assert_eq!(changes.changes.len(), 1);
        assert!(
            log.iter()
                .any(|entry| entry.args.contains(&"status".to_owned()))
        );
    });
    assert_eq!(other.status_by_git(), ["?? notes.txt"]);
    assert_eq!(
        sandbox
            .existing_repository(&target)
            .git(&["rev-parse", "HEAD"]),
        remote.git(&["rev-parse", "main"])
    );
}

#[test]
fn a_clone_that_fails_still_finishes_and_leaves_git_output_in_the_operation_log() {
    let sandbox = Sandbox::new();
    let missing = sandbox.root().join("no-such-remote.git");
    let url = file_url(&missing);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    let result = gikli.clone_repository(&url, &target);

    assert_eq!(
        result,
        Err(Failure::CloneFailed {
            url,
            folder: target.clone()
        })
    );
    let received: Vec<Event> = events.try_iter().collect();
    let id = received
        .iter()
        .find_map(|event| match event {
            Event::OperationStarted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("the clone announces it started");
    assert!(received.contains(&Event::OperationFinished { id }));
    let clone = gikli
        .operation_log()
        .into_iter()
        .find(|entry| entry.args.contains(&"clone".to_owned()))
        .expect("the clone is logged");
    assert_ne!(clone.exit_code, Some(0));
    assert!(!clone.stderr.is_empty());
    assert!(!sandbox.is_inside_repository_by_git(sandbox.root()));
    assert!(!target.join(".git").exists());
}
