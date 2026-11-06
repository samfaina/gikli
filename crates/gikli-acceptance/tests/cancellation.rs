//! Cancelling a long operation: it ends git and every process git started, and gikli deals with
//! what the run left behind. gikli never does this by itself: a hook runs for as long as it takes
//! until the user says otherwise.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::thread::ScopedJoinHandle;
use std::time::{Duration, Instant};

use gikli_acceptance::{Sandbox, TestRepository, as_git_writes_it, file_url};
use gikli_core::{
    Event, ExplainedMessage, Explanation, Failure, FailureAction, Gikli, LongOperation, NextStep,
    NotificationKind, Project, Repository, RunningOperation,
};

/// How long anything here may take. Generous, for slow CI machines, and far below what a process
/// left alive would take to end: never.
const DEADLINE: Duration = Duration::from_secs(60);

/// What a hook writes before it blocks, to find in the operation log.
const WRITTEN_BEFORE_BLOCKING: &str = "Waiting for the release code";

/// What blocks for good in a hook. On Windows, this is a native program waiting on the hidden
/// console the hook's shell gets, which nothing but killing it ends.
#[cfg(windows)]
const BLOCKS: &str = "cmd.exe //c \"set /p x=<CON\"";
#[cfg(unix)]
const BLOCKS: &str = "sleep 3600";

/// The program that does the blocking, as the list of processes names it.
#[cfg(windows)]
const BLOCKING_PROGRAM: &str = "cmd.exe";
#[cfg(unix)]
const BLOCKING_PROGRAM: &str = "sleep";

fn blocking_hook() -> String {
    format!("echo '{WRITTEN_BEFORE_BLOCKING}'\n{BLOCKS}")
}

/// A bare remote holding one commit, pushed from a working repository.
fn remote_with_a_commit(sandbox: &Sandbox) -> TestRepository {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.repository("work");
    work.write("notes.txt", "draft\n");
    work.commit_all("First commit");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main"]);
    remote
}

/// Waits for the hook `hook` of a running operation to be reported, and returns the operation's id.
fn wait_for_hook(events: &Receiver<Event>, hook: &str) -> u64 {
    let give_up = Instant::now() + DEADLINE;
    loop {
        let left = give_up.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(Event::OperationHook {
                id,
                hook: Some(running),
            }) if running == hook => return id,
            Ok(_) => {}
            Err(error) => panic!("{hook} was not reported: {error}"),
        }
    }
}

/// Waits for `path` to exist: something the test started has got as far as creating it.
fn wait_for_file(path: &Path) {
    let give_up = Instant::now() + DEADLINE;
    while !path.exists() {
        assert!(Instant::now() < give_up, "{} never came", path.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The only long operation running.
fn the_running_operation(gikli: &Gikli) -> RunningOperation {
    let running = gikli.running_operations();
    assert_eq!(running.len(), 1, "{running:?}");
    running.into_iter().next().expect("one operation")
}

/// Waits for what `thread` returns, for a limited time, as a cancelled run must end by itself.
fn result_of<T>(thread: ScopedJoinHandle<'_, T>) -> T {
    let give_up = Instant::now() + DEADLINE;
    while !thread.is_finished() {
        assert!(Instant::now() < give_up, "the cancelled run did not end");
        std::thread::sleep(Duration::from_millis(20));
    }
    thread.join().expect("the run does not panic")
}

/// Makes every clone from the sandbox's remotes hold on the transfer of its objects, before any
/// hook runs, and create the returned file once it holds.
fn hold_clones_before_any_hook(sandbox: &Sandbox) -> PathBuf {
    let holding = sandbox.root().join("clone-is-held");
    let script = sandbox.root().join("hold-clone.sh");
    fs::write(
        &script,
        format!(
            ": > '{}'\nwhile :; do sleep 1; done\n",
            as_git_writes_it(&holding)
        ),
    )
    .expect("write the script");
    // Read by git's upload-pack only from protected configuration, such as the global one.
    sandbox.configure(&format!(
        "[uploadpack]\n\tpackObjectsHook = sh '{}'\n",
        as_git_writes_it(&script)
    ));
    holding
}

#[test]
fn a_clone_waiting_on_a_hook_can_be_cancelled_and_stays_a_repository_that_opens() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.hook("post-checkout", &blocking_hook());
    let target = sandbox.root().join("clone");
    let url = file_url(remote.root());
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    let result: Result<Project, Failure> = std::thread::scope(|scope| {
        let clone = scope.spawn(|| gikli.clone_repository(&url, &target));
        let id = wait_for_hook(&events, "post-checkout");

        let running = the_running_operation(&gikli);
        assert_eq!(running.id, id);
        assert_eq!(running.hook.as_deref(), Some("post-checkout"));
        assert!(running.cancellable);
        assert_eventually_runs(&sandbox, BLOCKING_PROGRAM);

        gikli.cancel_operation(id);

        result_of(clone)
    });

    assert_eq!(sandbox.processes_still_running(), Vec::<String>::new());
    let project = result.expect("the clone is complete, so it opens");
    let clone = sandbox.existing_repository(&target);
    let repository = Repository {
        root: clone.toplevel_by_git(),
    };
    assert_eq!(project.repositories, vec![repository.clone()]);
    assert_eq!(
        clone.git(&["rev-parse", "HEAD"]),
        remote.git(&["rev-parse", "main"])
    );
    assert_eq!(clone.status_by_git(), Vec::<String>::new());
    assert!(gikli.running_operations().is_empty());
    let told: Vec<NotificationKind> = gikli
        .notifications()
        .into_iter()
        .filter(|notification| notification.project == project.folder)
        .map(|notification| notification.kind)
        .collect();
    assert!(
        told.contains(&NotificationKind::HookNotFinished {
            repository,
            hook: "post-checkout".to_owned()
        }),
        "{told:?}"
    );
    let logged = gikli
        .operation_log()
        .into_iter()
        .find(|entry| entry.args.contains(&"clone".to_owned()))
        .expect("the cancelled clone is logged");
    assert!(logged.cancelled);
    assert!(
        logged.stderr.contains(WRITTEN_BEFORE_BLOCKING),
        "{}",
        logged.stderr
    );
    assert_ne!(logged.exit_code, Some(0));
}

/// Fails unless a process called `program`, running for this sandbox, shows up.
fn assert_eventually_runs(sandbox: &Sandbox, program: &str) {
    let give_up = Instant::now() + DEADLINE;
    while !sandbox
        .processes_still_running()
        .iter()
        .any(|name| name == program)
    {
        assert!(
            Instant::now() < give_up,
            "{program} never ran: {:?}",
            sandbox.processes_still_running()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Clones with the transfer held, cancels, and returns what the clone returned.
fn cancel_a_held_clone(
    sandbox: &Sandbox,
    gikli: &Gikli,
    target: &Path,
) -> Result<Project, Failure> {
    let remote = remote_with_a_commit(sandbox);
    let holding = hold_clones_before_any_hook(sandbox);
    let url = file_url(remote.root());
    std::thread::scope(|scope| {
        let clone = scope.spawn(|| gikli.clone_repository(&url, target));
        wait_for_file(&holding);
        gikli.cancel_operation(the_running_operation(gikli).id);
        result_of(clone)
    })
}

#[test]
fn a_clone_cancelled_before_any_hook_runs_leaves_nothing_behind() {
    let sandbox = Sandbox::new();
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = cancel_a_held_clone(&sandbox, &gikli, &target);

    assert_eq!(
        result,
        Err(Failure::OperationCancelled { left_behind: None })
    );
    assert!(!target.exists(), "only the clone had created the folder");
    assert_eq!(sandbox.processes_still_running(), Vec::<String>::new());
    assert!(
        gikli
            .operation_log()
            .iter()
            .any(|entry| entry.cancelled && entry.args.contains(&"clone".to_owned()))
    );
}

#[test]
fn a_folder_that_existed_before_a_cancelled_clone_keeps_existing_empty() {
    let sandbox = Sandbox::new();
    let target = sandbox.folder("clone");
    let gikli = sandbox.gikli();

    let result = cancel_a_held_clone(&sandbox, &gikli, &target);

    assert_eq!(
        result,
        Err(Failure::OperationCancelled { left_behind: None })
    );
    assert_eq!(
        fs::read_dir(&target)
            .expect("the folder still exists")
            .count(),
        0
    );
}

/// git updates the references of a clone before it checks anything out, so a clone stopped in
/// that hook has no files yet.
#[test]
fn a_clone_cancelled_in_a_hook_that_runs_before_the_checkout_leaves_nothing_behind() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.hook("reference-transaction", &blocking_hook());
    let target = sandbox.root().join("clone");
    let url = file_url(remote.root());
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    let result = std::thread::scope(|scope| {
        let clone = scope.spawn(|| gikli.clone_repository(&url, &target));
        let id = wait_for_hook(&events, "reference-transaction");
        gikli.cancel_operation(id);
        result_of(clone)
    });

    assert_eq!(
        result,
        Err(Failure::OperationCancelled { left_behind: None })
    );
    assert!(!target.exists());
    assert_eq!(sandbox.processes_still_running(), Vec::<String>::new());
    assert!(gikli.notifications().is_empty());
}

#[test]
fn a_hook_that_is_only_slow_runs_to_completion_with_nobody_cancelling_it() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let finished = sandbox.root().join("hook-finished");
    sandbox.hook(
        "post-checkout",
        &format!("sleep 3\n: > '{}'", as_git_writes_it(&finished)),
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    let project = gikli
        .clone_repository(&file_url(remote.root()), &target)
        .expect("the clone succeeds");

    assert!(finished.exists(), "gikli let the hook finish");
    assert_eq!(project.repositories.len(), 1);
    assert!(gikli.notifications().is_empty());
    let hooks: Vec<Option<String>> = events
        .try_iter()
        .filter_map(|event| match event {
            Event::OperationHook { hook, .. } => Some(hook),
            _ => None,
        })
        .collect();
    assert_eq!(hooks, [Some("post-checkout".to_owned()), None]);
    let logged = gikli
        .operation_log()
        .into_iter()
        .find(|entry| entry.args.contains(&"clone".to_owned()))
        .expect("the clone is logged");
    assert!(!logged.cancelled);
    assert_eq!(logged.exit_code, Some(0));
}

/// A git that a hook starts traces to the same file, and may run hooks of its own. What the
/// operation waits on is still the hook of the git gikli started.
#[test]
fn a_hook_run_by_a_git_inside_a_hook_is_not_the_one_reported() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let nested_runs = sandbox.root().join("nested-hook-runs");
    sandbox.hook(
        "reference-transaction",
        &format!(
            "if [ -n \"$GIKLI_TEST_NESTED\" ]; then : > '{}'; {BLOCKS}; fi",
            as_git_writes_it(&nested_runs)
        ),
    );
    sandbox.hook(
        "post-checkout",
        "GIKLI_TEST_NESTED=1 git update-ref refs/heads/nested HEAD",
    );
    let target = sandbox.root().join("clone");
    let url = file_url(remote.root());
    let gikli = sandbox.gikli();

    let result = std::thread::scope(|scope| {
        let clone = scope.spawn(|| gikli.clone_repository(&url, &target));
        wait_for_file(&nested_runs);
        // Time for gikli to read the trace, in which it could take the nested hook for the one
        // that runs.
        std::thread::sleep(Duration::from_millis(500));

        let running = the_running_operation(&gikli);
        assert_eq!(running.hook.as_deref(), Some("post-checkout"));

        gikli.cancel_operation(running.id);
        result_of(clone)
    });

    result.expect("the clone is complete, so it opens");
    assert_eq!(sandbox.processes_still_running(), Vec::<String>::new());
}

/// A repository where adding `held.txt` blocks, with the index locked, in a filter that creates
/// the returned file once it holds.
fn repository_where_adding_blocks(sandbox: &Sandbox) -> (TestRepository, PathBuf) {
    let holding = sandbox.root().join("add-is-held");
    let script = sandbox.root().join("hold-add.sh");
    fs::write(
        &script,
        format!(
            ": > '{}'\nwhile :; do sleep 1; done\n",
            as_git_writes_it(&holding)
        ),
    )
    .expect("write the script");
    sandbox.configure(&format!(
        "[filter \"hold\"]\n\tclean = sh '{}'\n",
        as_git_writes_it(&script)
    ));
    let repository = sandbox.repository("app");
    repository.write(".gitattributes", "held.txt filter=hold\n");
    repository.write("notes.txt", "draft\n");
    repository.commit_all("First commit");
    repository.write("held.txt", "held\n");
    repository.write("new.txt", "new\n");
    (repository, holding)
}

/// Where a file of the git directory of `repository` goes, as plain git says.
fn in_git_directory(repository: &TestRepository, name: &str) -> PathBuf {
    let path = repository.git(&["rev-parse", "--path-format=absolute", "--git-path", name]);
    let index = PathBuf::from(path.trim_end());
    let folder = dunce::canonicalize(index.parent().expect("a file of the git directory"))
        .expect("the repository has a git directory");
    folder.join(name)
}

#[test]
fn a_run_cancelled_while_it_holds_the_index_lock_ends_with_the_offer_to_remove_it() {
    let sandbox = Sandbox::new();
    let (repository, holding) = repository_where_adding_blocks(&sandbox);
    let root = Repository {
        root: repository.toplevel_by_git(),
    };
    let lock = in_git_directory(&repository, "index.lock");
    // `git commit --only` leaves this one too, and gikli does not commit yet.
    let next_index_lock = in_git_directory(&repository, "next-index-4242.lock");
    let gikli = sandbox.gikli();

    let result = std::thread::scope(|scope| {
        let add = scope.spawn(|| gikli.add_files(&root, &["held.txt".to_owned()]));
        wait_for_file(&holding);
        let running = the_running_operation(&gikli);
        assert_eq!(
            running.operation,
            LongOperation::AddFiles {
                repository: root.clone()
            }
        );
        assert!(lock.exists(), "git holds the lock while the filter runs");
        fs::write(&next_index_lock, "").expect("leave the other lock behind");

        gikli.cancel_operation(running.id);

        result_of(add)
    });

    assert_eq!(sandbox.processes_still_running(), Vec::<String>::new());
    let remove = FailureAction::RemoveIndexLock {
        repository: root.clone(),
    };
    assert_eq!(
        result,
        Err(Failure::OperationCancelled {
            left_behind: Some(Explanation {
                message: ExplainedMessage::IndexLocked { lock: lock.clone() },
                next_step: NextStep::RemoveIndexLock,
                actions: vec![remove.clone()],
            })
        })
    );
    assert!(lock.exists(), "explaining removes nothing");
    assert!(next_index_lock.exists());
    assert!(
        !repository.git_succeeds(&["add", "--", "new.txt"]),
        "the lock stands in the way of plain git"
    );

    gikli
        .take_failure_action(&remove)
        .expect("remove the locks");

    assert!(!lock.exists());
    assert!(!next_index_lock.exists());
    repository.git(&["add", "--", "new.txt"]);
    assert!(
        repository
            .status_by_git()
            .contains(&"A  new.txt".to_owned())
    );
    assert!(
        gikli
            .operation_log()
            .iter()
            .any(|entry| entry.cancelled && entry.args.contains(&"add".to_owned()))
    );
}

#[test]
fn cancelling_an_operation_that_already_ended_or_never_ran_changes_nothing() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    gikli
        .clone_repository(&file_url(remote.root()), &target)
        .expect("clone the remote");
    let id = events
        .try_iter()
        .find_map(|event| match event {
            Event::OperationFinished { id } => Some(id),
            _ => None,
        })
        .expect("the clone finished");

    gikli.cancel_operation(id);
    gikli.cancel_operation(id + 100);

    assert!(gikli.running_operations().is_empty());
    assert_eq!(
        sandbox
            .existing_repository(&target)
            .git(&["rev-parse", "HEAD"]),
        remote.git(&["rev-parse", "main"])
    );
}

/// A cancelled clone takes the askpass program with it, and with it the question nobody needs
/// answered any more.
#[test]
fn cancelling_a_clone_waiting_for_a_prompt_closes_the_prompt() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let http = sandbox.http_remote(&remote, "ada", "s3cret");
    let target = sandbox.root().join("clone");
    let url = http.url();
    let gikli = sandbox.gikli_with_prompts(gikli_acceptance::askpass!());
    let events = gikli.subscribe();

    let (result, prompt) = std::thread::scope(|scope| {
        let clone = scope.spawn(|| gikli.clone_repository(&url, &target));
        let (operation, prompt) = wait_for_prompt(&events, &gikli);
        gikli.cancel_operation(operation);
        (result_of(clone), prompt)
    });

    assert_eq!(
        result,
        Err(Failure::OperationCancelled { left_behind: None })
    );
    let give_up = Instant::now() + DEADLINE;
    while !gikli.pending_prompts().is_empty() {
        assert!(Instant::now() < give_up, "the prompt stayed open");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        events
            .try_iter()
            .any(|event| event == Event::PromptClosed { id: prompt })
    );
    assert!(!target.exists());
}

/// Waits for a prompt to rise, and returns the id of the running operation and of the prompt.
fn wait_for_prompt(events: &Receiver<Event>, gikli: &Gikli) -> (u64, u64) {
    let give_up = Instant::now() + DEADLINE;
    loop {
        let left = give_up.saturating_duration_since(Instant::now());
        match events.recv_timeout(left) {
            Ok(Event::PromptRaised(prompt)) => {
                return (the_running_operation(gikli).id, prompt.id);
            }
            Ok(_) => {}
            Err(error) => panic!("no prompt rose: {error}"),
        }
    }
}

/// Cancelling git inside WSL may not reach the processes on the Linux side, so gikli does not offer
/// it. The stand-in `wsl.exe` fails at once, which is all this needs: the operation still starts.
#[cfg(windows)]
#[test]
fn an_operation_of_git_inside_wsl_does_not_offer_to_be_cancelled() {
    let sandbox = Sandbox::new();
    let (host, _wsl) = sandbox.host_with_fake_wsl(gikli_acceptance::fake_git!(), "2.43.0");
    let gikli = sandbox.gikli_in(host);
    let events = gikli.subscribe();

    let result = gikli.clone_repository("file:///nowhere", &sandbox.root().join("clone"));

    assert!(result.is_err());
    let started = events
        .try_iter()
        .find_map(|event| match event {
            Event::OperationStarted { cancellable, .. } => Some(cancellable),
            _ => None,
        })
        .expect("the clone announces it started");
    assert!(!started, "gikli offers no cancel that may not work");
}
