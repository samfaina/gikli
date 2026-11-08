//! Failures caused by the environment: gikli says in plain words what went wrong and what to do
//! next, and offers what it can do about it. git's raw output stays in the operation log.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use gikli_acceptance::{
    Sandbox, TestRepository, WslRepository, as_git_writes_it, file_url, speaking,
};
use gikli_core::{ExplainedMessage, Explanation, Failure, FailureAction, NextStep, Repository};

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

#[test]
fn a_failing_hook_shows_its_output_and_leaves_gits_output_in_the_operation_log() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.hook(
        "post-checkout",
        "echo 'Lint found 2 problems'\necho 'Run the formatter' >&2\nexit 3",
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(&file_url(remote.root()), &target);

    assert_eq!(
        result,
        Err(Failure::Explained {
            explanation: Explanation {
                message: ExplainedMessage::HookFailed {
                    hook: "post-checkout".to_owned(),
                    output: "Lint found 2 problems\nRun the formatter\n".to_owned(),
                },
                next_step: NextStep::FixHook,
                actions: vec![],
            }
        })
    );
    let clone = gikli
        .operation_log()
        .into_iter()
        .find(|entry| entry.args.contains(&"clone".to_owned()))
        .expect("the clone is logged");
    assert_eq!(clone.exit_code, Some(3));
    assert!(clone.stderr.contains("% ("), "{}", clone.stderr);
    assert!(
        clone.stderr.contains("Run the formatter"),
        "{}",
        clone.stderr
    );
    assert_eq!(
        sandbox
            .existing_repository(&target)
            .git(&["rev-parse", "HEAD"]),
        remote.git(&["rev-parse", "main"]),
        "the hook failed after git had cloned"
    );
}

/// A hook that asks the terminal for input, which would wait forever for a terminal nobody sees.
const ASKS_THE_TERMINAL: &str = "echo 'Enter the release code:'\nread code < /dev/tty || exit 1\necho \"Thanks: $code\"\nexit 1";

#[test]
fn a_hook_that_needs_a_terminal_fails_with_a_message_instead_of_hanging() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.hook("post-checkout", ASKS_THE_TERMINAL);
    let url = file_url(remote.root());
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let (finished, result) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = finished.send(gikli.clone_repository(&url, &target));
    });
    let result = result
        .recv_timeout(Duration::from_secs(60))
        .expect("the hook fails instead of hanging");

    let Err(Failure::Explained { explanation }) = result else {
        panic!("got {result:?}");
    };
    let ExplainedMessage::HookFailed { hook, output } = explanation.message else {
        panic!("got {:?}", explanation.message);
    };
    assert_eq!(hook, "post-checkout");
    assert!(output.contains("Enter the release code:"), "{output}");
    assert!(!output.contains("Thanks"), "{output}");
    assert_eq!(explanation.next_step, NextStep::FixHook);
    assert_eq!(
        sandbox
            .existing_repository(&sandbox.root().join("clone"))
            .git(&["rev-parse", "HEAD"]),
        remote.git(&["rev-parse", "main"])
    );
}

/// Whether plain git refuses `repository` as someone else's. git 2.34.0 predates the check, so the
/// tests of dubious ownership skip there.
fn git_checks_ownership(repository: &TestRepository) -> bool {
    let refused = repository.refused_for_ownership_by_git();
    if !refused {
        eprintln!("skipped: this git does not check who owns a repository");
    }
    refused
}

#[test]
fn dubious_ownership_is_explained_and_the_folder_is_trusted_only_when_the_user_takes_the_offer() {
    let sandbox = Sandbox::new();
    // With a space, which git quotes in the command it recommends.
    let repository = sandbox.repository("shared drive");
    repository.write("notes.txt", "draft\n");
    if !git_checks_ownership(&repository) {
        return;
    }
    let root = repository_of(&repository);
    let folder = as_git_writes_it(&root.root);
    let gikli = sandbox.gikli_in(sandbox.host_where_repositories_belong_to_someone_else());

    let result = gikli.read_changes(&root);

    let trust = FailureAction::TrustFolder {
        repository: root.clone(),
        folder: folder.clone(),
    };
    assert_eq!(
        result,
        Err(Failure::Explained {
            explanation: Explanation {
                message: ExplainedMessage::DubiousOwnership {
                    folder: folder.clone()
                },
                next_step: NextStep::TrustFolder,
                actions: vec![trust.clone()],
            }
        })
    );
    assert!(
        !repository.git_succeeds(&["config", "--global", "--get-all", "safe.directory"]),
        "explaining trusts nothing"
    );
    let refused = gikli.operation_log().pop().expect("the status is logged");
    assert!(refused.stderr.contains("safe.directory"), "{refused:?}");

    gikli.take_failure_action(&trust).expect("trust the folder");

    assert_eq!(
        repository
            .git(&["config", "--global", "--get-all", "safe.directory"])
            .trim_end(),
        folder
    );
    assert!(!repository.refused_for_ownership_by_git());
    let changes = gikli
        .read_changes(&root)
        .expect("read changes once trusted");
    assert_eq!(changes.changes.len(), 1);
    assert_eq!(repository.status_by_git(), ["?? notes.txt"]);
}

#[test]
fn a_folder_of_a_repository_refused_for_its_owner_is_explained_and_opens_once_trusted() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("shared drive");
    if !git_checks_ownership(&repository) {
        return;
    }
    let root = repository_of(&repository);
    let folder = as_git_writes_it(&root.root);
    let subfolder = repository.folder("src");
    let gikli = sandbox.gikli_in(sandbox.host_where_repositories_belong_to_someone_else());

    let opened = gikli.open_project(&subfolder);

    let trust = FailureAction::TrustFolder {
        repository: Repository {
            root: subfolder.clone(),
        },
        folder: folder.clone(),
    };
    assert_eq!(
        opened,
        Err(Failure::Explained {
            explanation: Explanation {
                message: ExplainedMessage::DubiousOwnership {
                    folder: folder.clone()
                },
                next_step: NextStep::TrustFolder,
                actions: vec![trust.clone()],
            }
        })
    );
    assert!(
        !repository.git_succeeds(&["config", "--global", "--get-all", "safe.directory"]),
        "explaining trusts nothing"
    );

    gikli.take_failure_action(&trust).expect("trust the folder");

    let project = gikli
        .open_project(&subfolder)
        .expect("open project once trusted");
    assert_eq!(project.repositories, vec![root]);
    assert_eq!(
        repository
            .git(&["config", "--global", "--get-all", "safe.directory"])
            .trim_end(),
        folder
    );
}

#[test]
fn a_project_git_refuses_after_it_was_opened_is_explained_instead_of_shown_as_broken() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    if !git_checks_ownership(&repository) {
        return;
    }
    sandbox
        .gikli()
        .open_project(repository.root())
        .expect("open project");
    let gikli = sandbox.gikli_in(sandbox.host_where_repositories_belong_to_someone_else());

    let opened = gikli.open_project(repository.root());

    let Err(Failure::Explained { explanation }) = opened else {
        panic!("got {opened:?}");
    };
    assert_eq!(explanation.next_step, NextStep::TrustFolder);
    assert_eq!(gikli.notifications(), vec![]);

    gikli
        .take_failure_action(&explanation.actions[0])
        .expect("trust the folder");

    let project = gikli
        .open_project(repository.root())
        .expect("open project once trusted");
    assert_eq!(project.repositories, vec![repository_of(&repository)]);
    assert_eq!(project.invalid_repositories, vec![]);
    assert!(repository.git_succeeds(&["config", "--global", "--get-all", "safe.directory"]));
}

#[test]
fn opening_a_refused_repository_is_explained_the_same_when_git_speaks_another_language() {
    let sandbox = Sandbox::new();
    let shared = sandbox.repository("shared");
    if !git_checks_ownership(&shared) {
        return;
    }
    let gikli = sandbox.gikli_in(speaking(
        sandbox.host_where_repositories_belong_to_someone_else(),
        "de",
    ));

    let opened = gikli.open_project(shared.root());

    let Err(Failure::Explained { explanation }) = opened else {
        panic!("got {opened:?}");
    };
    assert_eq!(
        explanation.message,
        ExplainedMessage::DubiousOwnership {
            folder: as_git_writes_it(&repository_of(&shared).root)
        }
    );
    assert!(
        !shared.git_succeeds(&["config", "--global", "--get-all", "safe.directory"]),
        "explaining trusts nothing"
    );
}

/// A repository with one commit and a new file, whose index is locked by a file some git left
/// behind. Returns the lock, as plain git's own path to it.
fn repository_with_a_stale_lock(sandbox: &Sandbox, name: &str) -> (TestRepository, PathBuf) {
    let repository = sandbox.repository(name);
    repository.write("notes.txt", "draft\n");
    repository.commit_all("First commit");
    repository.write("new.txt", "new\n");
    let lock = lock_of(&repository);
    fs::write(&lock, "").expect("leave a lock behind");
    (repository, lock)
}

/// Where the lock of the index of `repository` goes, as plain git says.
fn lock_of(repository: &TestRepository) -> PathBuf {
    let index = repository.git(&["rev-parse", "--path-format=absolute", "--git-path", "index"]);
    let index = dunce::canonicalize(index.trim_end()).expect("the repository has an index");
    index.with_file_name("index.lock")
}

fn repository_of(repository: &TestRepository) -> Repository {
    Repository {
        root: repository.toplevel_by_git(),
    }
}

#[test]
fn a_stale_index_lock_is_explained_and_removed_only_when_the_user_takes_the_offer() {
    let sandbox = Sandbox::new();
    let (repository, lock) = repository_with_a_stale_lock(&sandbox, "app");
    let root = repository_of(&repository);
    let gikli = sandbox.gikli();
    let new_file = ["new.txt".to_owned()];

    let result = gikli.add_files(&root, &new_file);

    let remove = FailureAction::RemoveIndexLock {
        repository: root.clone(),
    };
    assert_eq!(
        result,
        Err(Failure::Explained {
            explanation: Explanation {
                message: ExplainedMessage::IndexLocked { lock: lock.clone() },
                next_step: NextStep::RemoveIndexLock,
                actions: vec![remove.clone()],
            }
        })
    );
    assert!(lock.exists(), "explaining removes nothing");
    assert_eq!(repository.status_by_git(), ["?? new.txt"]);
    let refused = gikli
        .operation_log()
        .into_iter()
        .find(|entry| entry.args.contains(&"add".to_owned()))
        .expect("the add is logged");
    assert!(refused.stderr.contains("index.lock"), "{refused:?}");

    gikli
        .take_failure_action(&remove)
        .expect("remove the stale lock");

    assert!(!lock.exists());
    gikli
        .add_files(&root, &new_file)
        .expect("add the file once the lock is gone");
    assert_eq!(repository.status_by_git(), ["A  new.txt"]);
}

#[test]
fn the_lock_of_a_linked_worktree_is_the_one_in_its_own_git_directory() {
    let sandbox = Sandbox::new();
    let (main, main_lock) = repository_with_a_stale_lock(&sandbox, "app");
    fs::remove_file(&main_lock).expect("the main worktree is not locked");
    let worktree = main.linked_worktree("feature");
    worktree.write("new.txt", "new\n");
    let lock = lock_of(&worktree);
    fs::write(&lock, "").expect("leave a lock behind");
    let root = repository_of(&worktree);
    let gikli = sandbox.gikli();

    let result = gikli.add_files(&root, &["new.txt".to_owned()]);

    let Err(Failure::Explained { explanation }) = result else {
        panic!("got {result:?}");
    };
    assert_eq!(
        explanation.message,
        ExplainedMessage::IndexLocked { lock: lock.clone() }
    );
    gikli
        .take_failure_action(&explanation.actions[0])
        .expect("remove the stale lock");
    assert!(!lock.exists());
    assert!(worktree.git_succeeds(&["add", "--", "new.txt"]));
}

#[test]
fn removing_the_lock_is_not_offered_while_another_git_operation_runs_in_the_repository() {
    let sandbox = Sandbox::new();
    let (repository, lock) = repository_with_a_stale_lock(&sandbox, "app");
    let root = repository_of(&repository);
    let gikli = sandbox.gikli();
    let new_file = ["new.txt".to_owned()];
    let operation = repository.running_git();

    let while_running = gikli.add_files(&root, &new_file);
    let forced = gikli.take_failure_action(&FailureAction::RemoveIndexLock {
        repository: root.clone(),
    });

    let locked = Err(Failure::Explained {
        explanation: Explanation {
            message: ExplainedMessage::IndexLocked { lock: lock.clone() },
            next_step: NextStep::WaitForOtherGitOperation,
            actions: vec![],
        },
    });
    assert_eq!(while_running, locked);
    assert_eq!(forced, locked, "gikli checks again when the user acts");
    assert!(lock.exists(), "the running operation may own the lock");

    drop(operation);

    let Err(Failure::Explained { explanation }) = gikli.add_files(&root, &new_file) else {
        panic!("the index is still locked");
    };
    assert_eq!(explanation.next_step, NextStep::RemoveIndexLock);
    assert_eq!(repository.status_by_git(), ["?? new.txt"]);
}

/// A git sent to the repository from elsewhere runs there just the same.
#[test]
fn removing_the_lock_is_not_offered_while_a_git_from_another_folder_works_on_the_repository() {
    let sandbox = Sandbox::new();
    let (repository, lock) = repository_with_a_stale_lock(&sandbox, "app");
    let elsewhere = sandbox.folder("elsewhere");
    let root = repository_of(&repository);
    let gikli = sandbox.gikli();
    let new_file = ["new.txt".to_owned()];
    let locked = Err(Failure::Explained {
        explanation: Explanation {
            message: ExplainedMessage::IndexLocked { lock: lock.clone() },
            next_step: NextStep::WaitForOtherGitOperation,
            actions: vec![],
        },
    });

    let with_option = repository.running_git_with_option_from(&elsewhere);
    assert_eq!(gikli.add_files(&root, &new_file), locked);
    drop(with_option);

    let with_variable = repository.running_git_with_variable_from(&elsewhere);
    assert_eq!(gikli.add_files(&root, &new_file), locked);
    drop(with_variable);

    assert!(lock.exists());
    assert_eq!(repository.status_by_git(), ["?? new.txt"]);
}

/// git recommends the command only when it refuses a repository for its owner.
#[test]
fn a_failure_that_only_mentions_the_setting_is_not_taken_for_dubious_ownership() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let root = repository_of(&repository);
    let gikli = sandbox.gikli();

    let result = gikli.add_files(&root, &["safe.directory x".to_owned()]);

    assert_eq!(
        result,
        Err(Failure::FilesNotAdded {
            folder: root.root.clone()
        })
    );
    assert!(!repository.git_succeeds(&["config", "--global", "--get-all", "safe.directory"]));
}

#[test]
fn a_git_operation_in_another_repository_does_not_hold_back_the_offer() {
    let sandbox = Sandbox::new();
    let (repository, _lock) = repository_with_a_stale_lock(&sandbox, "app");
    let other = sandbox.repository("other");
    let _operation = other.running_git();
    let gikli = sandbox.gikli();

    let result = gikli.add_files(&repository_of(&repository), &["new.txt".to_owned()]);

    let Err(Failure::Explained { explanation }) = result else {
        panic!("got {result:?}");
    };
    assert_eq!(explanation.next_step, NextStep::RemoveIndexLock);
    assert_eq!(repository.status_by_git(), ["?? new.txt"]);
}

/// A bare remote holding a file whose path is longer than Windows allows by default. The commit is
/// put together without the file ever being on disk.
#[cfg(windows)]
fn remote_with_a_long_path(sandbox: &Sandbox) -> TestRepository {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.repository("work");
    work.write("notes.txt", "draft\n");
    let blob = work.git(&["hash-object", "-w", "notes.txt"]);
    let long_path = vec!["a-rather-long-folder-name"; 15].join("/") + "/notes.txt";
    work.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{},{long_path}", blob.trim_end()),
    ]);
    work.git(&["commit", "--quiet", "--message", "A very long path"]);
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main"]);
    remote
}

#[cfg(windows)]
#[test]
fn long_paths_on_windows_are_reported_recommending_core_longpaths_without_setting_it() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_long_path(&sandbox);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(&file_url(remote.root()), &target);

    assert_eq!(
        result,
        Err(Failure::Explained {
            explanation: Explanation {
                message: ExplainedMessage::LongPaths,
                next_step: NextStep::EnableLongPaths,
                actions: vec![],
            }
        })
    );
    let clone = sandbox.existing_repository(&target);
    assert!(
        !clone.git_succeeds(&["config", "core.longpaths"]),
        "gikli recommends the setting and leaves it to the user"
    );
    let entry = gikli
        .operation_log()
        .into_iter()
        .find(|entry| entry.args.contains(&"clone".to_owned()))
        .expect("the clone is logged");
    assert!(entry.stderr.contains("too long"), "{entry:?}");
}

/// What gikli says does not depend on the language git speaks. git translates its messages only
/// where its translations are installed, as on Linux with the latest git in CI.
#[test]
fn a_stale_lock_and_a_failing_hook_are_classified_the_same_when_git_speaks_another_language() {
    let sandbox = Sandbox::new();
    let (locked, lock) = repository_with_a_stale_lock(&sandbox, "locked");
    let gikli = sandbox.gikli_in(speaking(sandbox.host_environment(), "de"));

    let result = gikli.add_files(&repository_of(&locked), &["new.txt".to_owned()]);

    let Err(Failure::Explained { explanation }) = result else {
        panic!("got {result:?}");
    };
    assert_eq!(
        explanation.message,
        ExplainedMessage::IndexLocked { lock: lock.clone() }
    );
    assert!(lock.exists());
    assert_eq!(locked.status_by_git(), ["?? new.txt"]);

    let remote = remote_with_a_commit(&sandbox);
    sandbox.hook("post-checkout", "echo 'Prüfung fehlgeschlagen'\nexit 1");
    let target = sandbox.root().join("clone");
    let result = gikli.clone_repository(&file_url(remote.root()), &target);

    assert_eq!(
        result,
        Err(Failure::Explained {
            explanation: Explanation {
                message: ExplainedMessage::HookFailed {
                    hook: "post-checkout".to_owned(),
                    output: "Prüfung fehlgeschlagen\n".to_owned(),
                },
                next_step: NextStep::FixHook,
                actions: vec![],
            }
        })
    );
    assert_eq!(
        sandbox
            .existing_repository(&target)
            .git(&["rev-parse", "HEAD"]),
        remote.git(&["rev-parse", "main"])
    );
}

#[test]
fn dubious_ownership_is_classified_the_same_when_git_speaks_another_language() {
    let sandbox = Sandbox::new();
    let shared = sandbox.repository("shared");
    if !git_checks_ownership(&shared) {
        return;
    }
    let root = repository_of(&shared);
    let gikli = sandbox.gikli_in(speaking(
        sandbox.host_where_repositories_belong_to_someone_else(),
        "de",
    ));

    let result = gikli.read_changes(&root);

    let Err(Failure::Explained { explanation }) = result else {
        panic!("got {result:?}");
    };
    assert_eq!(
        explanation.message,
        ExplainedMessage::DubiousOwnership {
            folder: as_git_writes_it(&root.root)
        }
    );
    assert!(
        !shared.git_succeeds(&["config", "--global", "--get-all", "safe.directory"]),
        "explaining trusts nothing"
    );
}

/// The one classification that reads a sentence: the text of a system error, which on Windows does
/// not come from git's translations.
#[cfg(windows)]
#[test]
fn long_paths_are_reported_the_same_when_git_speaks_another_language() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_long_path(&sandbox);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_in(speaking(sandbox.host_environment(), "de"));

    let result = gikli.clone_repository(&file_url(remote.root()), &target);

    let Err(Failure::Explained { explanation }) = result else {
        panic!("got {result:?}");
    };
    assert_eq!(explanation.message, ExplainedMessage::LongPaths);
    assert!(
        !sandbox
            .existing_repository(&target)
            .git_succeeds(&["config", "core.longpaths"])
    );
}

#[test]
fn a_failure_gikli_does_not_know_keeps_its_own_failure_and_gits_output_stays_in_the_log() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let root = repository_of(&repository);
    let gikli = sandbox.gikli();

    let result = gikli.add_files(&root, &["no-such-file.txt".to_owned()]);

    assert_eq!(
        result,
        Err(Failure::FilesNotAdded {
            folder: root.root.clone()
        })
    );
    let refused = gikli.operation_log().pop().expect("the add is logged");
    assert!(refused.stderr.contains("no-such-file.txt"), "{refused:?}");
    assert_eq!(repository.status_by_git(), Vec::<String>::new());
}

#[test]
fn the_literal_paths_given_are_the_ones_added() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    repository.write("notes [draft].txt", "draft\n");
    repository.write("notes d.txt", "other\n");
    let gikli = sandbox.gikli();

    gikli
        .add_files(
            &repository_of(&repository),
            &["notes [draft].txt".to_owned()],
        )
        .expect("add the file");

    assert_eq!(
        repository.status_by_git(),
        ["A  notes [draft].txt", "?? notes d.txt"]
    );
}

/// gikli cannot see the processes inside WSL, so it cannot tell that a lock there is stale.
#[test]
fn removing_the_lock_of_a_repository_inside_wsl_is_not_offered() {
    let Some(repository) = WslRepository::create() else {
        eprintln!("skipped: needs WSL with git");
        return;
    };
    repository.write("new.txt", "new\n");
    let lock = repository.root().join(".git").join("index.lock");
    fs::write(&lock, "").expect("leave a lock behind");
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();

    let result = gikli.add_files(
        &Repository {
            root: repository.root(),
        },
        &["new.txt".to_owned()],
    );

    assert_eq!(
        result,
        Err(Failure::Explained {
            explanation: Explanation {
                message: ExplainedMessage::IndexLocked { lock: lock.clone() },
                next_step: NextStep::WaitForOtherGitOperation,
                actions: vec![],
            }
        })
    );
    assert!(lock.exists());
    assert_eq!(repository.status_by_git(), ["?? new.txt"]);
}
