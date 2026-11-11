# Tests run the real git, except the installation checks

Every test builds and checks its repositories with the real git executable. The installation checks are the exception, along with the few tests named under Limits. They put a stand-in, `fake-git` (`crates/gikli-acceptance/src/bin/fake-git.rs`), where gikli looks for git, to play installations the real one cannot: a git older than 2.34, a git whose version gikli does not recognize, a version just below or at what a capability needs, a git the user installed off the PATH, and `wsl.exe` on a machine without WSL. An old or unrecognized git cannot be installed on every developer's machine and CI runner, and the real git only ever reports its own version.

## Limits

- `fake-git` answers a request for its version, meaning any command line that holds `--version` or `version`, with `git version` and the text the test gives it.
- It fails every other command with exit code 128, so a test that needs git to do anything else runs the real one.
- It records each invocation in `fake-git-invocations`, so a test can prove gikli ran nothing else. `git_older_than_2_34_runs_no_operations` does this.
- Five tests of Repositories under a WSL share, in `crates/gikli-acceptance/tests/wsl.rs`, install it as the native git beside the real `wsl.exe`. They are outside the installation checks. A native git that fails every command proves gikli ran the git inside WSL.
- One more test outside the installation checks uses it. `what_git_writes_on_its_standard_output_shows_without_the_password` needs a URL in git's standard output. The real git writes none there for any command gikli runs today, and the version text of `fake-git` can hold one.
- `a_burst_without_a_pause_is_announced_once`, in `crates/gikli-core/src/watch.rs`, runs no git at all. It builds no repository and checks only how the changes watcher groups the signals it is given, which real files and their events cannot show independently of the machine.
- `a_process_the_cancel_missed_is_ended_too`, in `crates/gikli-core/src/process_tree.rs`, runs no git at all. It plays a process that a cancel missed, which a real git and its hooks only do by chance.
- The tests of `git_may_run`, in `crates/gikli-core/src/processes.rs`, run no git at all. They play the looks at the processes, as a git that Windows refuses to open while it starts or ends only turns up by chance.
- Teaching `fake-git` another command, or using it in another kind of test, means changing this ADR first.

## Considered Options

- **Real gits of each version, installed only in CI**: developers could not run these checks, and CI would still need a stand-in for a git whose version gikli does not recognize.
- **Mocking the git runner inside the core**: the spec (#2) rules out mocking git, and a mock would skip the search for git and the version check, which are what these tests exercise.
