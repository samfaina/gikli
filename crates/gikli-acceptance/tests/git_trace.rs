//! gikli reads git's trace of a run as git writes it, and keeps it in memory: git writes every
//! command line to the trace, and those can hold a password in a URL. An older git hides no
//! password there, and no git hides one that reads as a port.
//!
//! A hook sees where the trace goes in the variable a git it starts inherits, so these tests look
//! there from inside the run and again after it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gikli_acceptance::{HttpRemote, Sandbox, TestRepository, as_git_writes_it, file_url};
use gikli_core::{Event, ExplainedMessage, Failure, Repository};

const USERNAME: &str = "ada-lovelace-1815";
const PASSWORD: &str = "tr0ub4dor-and-3";

/// How long anything here may take. Generous, for slow CI machines.
const DEADLINE: Duration = Duration::from_secs(60);

/// Words of the trace, which nothing a test's hooks and git say holds.
const TRACE_WORDS: [&str; 3] = ["\"event\"", "\"sid\"", "gikli-trace-"];

/// A bare remote in the folder `name` holding one commit.
fn remote_named_with_a_commit(sandbox: &Sandbox, name: &str) -> TestRepository {
    let remote = sandbox.bare_remote(name);
    let work = sandbox.repository("work");
    work.write("notes.txt", "draft\n");
    work.commit_all("First commit");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main"]);
    remote
}

/// A remote over HTTP, and a URL of it with the password in it.
struct RemoteWithPasswordInUrl {
    remote: TestRepository,
    _server: HttpRemote,
    url: String,
}

/// git reads the password of this URL, and a recent one hides it in the trace.
fn url_git_can_read(sandbox: &Sandbox) -> RemoteWithPasswordInUrl {
    let remote = remote_named_with_a_commit(sandbox, "remote.git");
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let url = server
        .url()
        .replacen("http://", &format!("http://{USERNAME}:{PASSWORD}@"), 1);
    RemoteWithPasswordInUrl {
        remote,
        _server: server,
        url,
    }
}

/// An unencoded password holding a `/`. git takes what comes before it for a port, and the rest for
/// a path, so it serves this remote and writes the URL in full, whatever its version.
fn url_git_takes_for_a_path(sandbox: &Sandbox) -> RemoteWithPasswordInUrl {
    let remote = remote_named_with_a_commit(sandbox, &format!("{PASSWORD}@v2.git"));
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    sandbox.configure(&format!(
        "[credential]\n\thelper = \"!f() {{ echo username={USERNAME}; echo password={PASSWORD}; }}; f\"\n"
    ));
    let url = server.url();
    RemoteWithPasswordInUrl {
        remote,
        _server: server,
        url,
    }
}

/// What a hook found of the trace while the run went on.
struct TraceWatch {
    targets: PathBuf,
    contents: PathBuf,
}

impl TraceWatch {
    fn new(sandbox: &Sandbox) -> Self {
        Self {
            targets: sandbox.root().join("trace-targets"),
            contents: sandbox.root().join("trace-contents"),
        }
    }

    /// The lines of a hook that note where the trace goes and what a file there holds. Only a pipe
    /// on Windows is left unread, as its name takes the shell seconds to look up, and is no file.
    fn script(&self) -> String {
        format!(
            "printf '%s\\n' \"$GIT_TRACE2_EVENT\" >> '{targets}'\n\
             case \"$GIT_TRACE2_EVENT\" in\n\
             //./pipe/*) ;;\n\
             *) if [ -f \"$GIT_TRACE2_EVENT\" ]; then cat \"$GIT_TRACE2_EVENT\" >> '{contents}'; fi ;;\n\
             esac",
            targets = as_git_writes_it(&self.targets),
            contents = as_git_writes_it(&self.contents),
        )
    }

    /// Where the trace went, as the hook saw it.
    fn targets(&self) -> Vec<String> {
        fs::read_to_string(&self.targets)
            .expect("the hook ran")
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Checks that no file holding the password was where the trace goes, during the run and now.
    fn assert_no_file_holds_the_password(&self) {
        let during = fs::read_to_string(&self.contents).unwrap_or_default();
        assert!(!during.contains(PASSWORD), "during the run: {during}");
        for target in self.targets() {
            let after = fs::read_to_string(target).unwrap_or_default();
            assert!(!after.contains(PASSWORD), "after the run: {after}");
        }
    }
}

/// Checks with plain git that `target` holds what `remote` holds.
fn assert_cloned(sandbox: &Sandbox, remote: &TestRepository, target: &Path) {
    assert_eq!(
        sandbox
            .existing_repository(target)
            .git(&["rev-parse", "HEAD"]),
        remote.git(&["rev-parse", "main"])
    );
}

/// The name of the hook `result` says failed, and what it wrote.
fn failed_hook_of<T: std::fmt::Debug>(result: &Result<T, Failure>) -> (&str, &str) {
    let Err(Failure::Explained { explanation }) = result else {
        panic!("got {result:?}");
    };
    let ExplainedMessage::HookFailed { hook, output } = &explanation.message else {
        panic!("got {:?}", explanation.message);
    };
    (hook, output)
}

fn assert_clone_leaves_the_password_in_no_file(url_of: fn(&Sandbox) -> RemoteWithPasswordInUrl) {
    let sandbox = Sandbox::new();
    let remote = url_of(&sandbox);
    let watch = TraceWatch::new(&sandbox);
    sandbox.hook("post-checkout", &watch.script());
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    gikli.clone_repository(&remote.url, &target).expect("clone");

    assert_cloned(&sandbox, &remote.remote, &target);
    watch.assert_no_file_holds_the_password();
}

#[test]
fn a_clone_of_a_url_git_can_read_leaves_the_password_in_no_file() {
    assert_clone_leaves_the_password_in_no_file(url_git_can_read);
}

#[test]
fn a_clone_of_a_password_holding_a_slash_leaves_it_in_no_file() {
    assert_clone_leaves_the_password_in_no_file(url_git_takes_for_a_path);
}

/// gikli has no fetch of its own yet, so a hook fetches inside a run gikli started. That git
/// inherits the target of the trace, and writes its command lines there if it reaches it.
fn assert_fetch_leaves_the_password_in_no_file(url_of: fn(&Sandbox) -> RemoteWithPasswordInUrl) {
    let sandbox = Sandbox::new();
    let remote = url_of(&sandbox);
    let local = sandbox.repository("local");
    local.git(&["remote", "add", "origin", &remote.url]);
    local.write("new.txt", "new\n");
    let watch = TraceWatch::new(&sandbox);
    sandbox.hook(
        "post-index-change",
        &format!("git fetch --quiet origin\n{}", watch.script()),
    );
    let gikli = sandbox.gikli();
    let repository = Repository {
        root: local.root().to_path_buf(),
    };

    gikli
        .add_files(&repository, &["new.txt".to_owned()])
        .expect("add");

    assert!(
        local.git_succeeds(&["rev-parse", "--verify", "refs/remotes/origin/main"]),
        "the hook fetched"
    );
    watch.assert_no_file_holds_the_password();
}

#[test]
fn a_fetch_of_a_url_git_can_read_in_the_configuration_leaves_the_password_in_no_file() {
    assert_fetch_leaves_the_password_in_no_file(url_git_can_read);
}

#[test]
fn a_fetch_of_a_password_holding_a_slash_in_the_configuration_leaves_it_in_no_file() {
    assert_fetch_leaves_the_password_in_no_file(url_git_takes_for_a_path);
}

fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let give_up = Instant::now() + DEADLINE;
    while !condition() {
        assert!(Instant::now() < give_up, "waited in vain for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The program outlives the run and holds on to all it inherited from git. It waits for the test to
/// say so, or a minute, so a run that waits for it ends with `finished` already there.
#[test]
fn a_program_that_outlives_the_run_is_not_waited_for_and_leaves_no_file() {
    let sandbox = Sandbox::new();
    let remote = url_git_takes_for_a_path(&sandbox);
    let release = sandbox.root().join("release");
    let finished = sandbox.root().join("finished");
    let watch = TraceWatch::new(&sandbox);
    sandbox.hook(
        "post-checkout",
        &format!(
            "{watch}\n\
             (\n\
               tries=0\n\
               while [ ! -e '{release}' ] && [ $tries -lt 600 ]; do\n\
                 sleep 0.1\n\
                 tries=$((tries + 1))\n\
               done\n\
               git ls-remote '{url}'\n\
               : > '{finished}'\n\
             ) > /dev/null 2>&1 < /dev/null &",
            watch = watch.script(),
            release = as_git_writes_it(&release),
            finished = as_git_writes_it(&finished),
            url = remote.url,
        ),
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    gikli.clone_repository(&remote.url, &target).expect("clone");

    assert_cloned(&sandbox, &remote.remote, &target);
    assert!(!finished.exists(), "the run waited for the program");
    fs::write(&release, "").expect("release the program");
    wait_until("the program to finish", || finished.exists());
    watch.assert_no_file_holds_the_password();
}

/// `reference-transaction` of the git gikli started runs first and succeeds, as child 0. The one of
/// a git inside `post-checkout` fails, also as child 0 of its own git.
#[test]
fn a_failing_hook_of_a_git_inside_a_hook_is_not_the_one_reported() {
    let sandbox = Sandbox::new();
    let remote = remote_named_with_a_commit(&sandbox, "remote.git");
    sandbox.hook(
        "reference-transaction",
        "if [ -n \"$GIKLI_TEST_NESTED\" ]; then exit 5; fi",
    );
    sandbox.hook(
        "post-checkout",
        "GIKLI_TEST_NESTED=1 git update-ref refs/heads/nested HEAD\necho 'Lint found 2 problems'\nexit 3",
    );
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    let result = gikli.clone_repository(&file_url(remote.root()), &sandbox.root().join("clone"));

    let (hook, output) = failed_hook_of(&result);
    assert_eq!(hook, "post-checkout");
    assert!(output.contains("Lint found 2 problems"), "{output}");
    let events: Vec<Event> = events.try_iter().collect();
    assert_holds_no_trace(&format!(
        "{result:?}{events:?}{:?}{:?}",
        gikli.operation_log(),
        gikli.notifications(),
    ));
}

/// git writes one event for each setting `trace2.configParams` names, here far more than a pipe
/// holds, before it starts anything. A reader that stopped draining would hold git up for good, and
/// one that lost events would not name the hook.
#[test]
fn a_trace_longer_than_a_pipe_holds_is_read_to_its_end() {
    let sandbox = Sandbox::new();
    let remote = remote_named_with_a_commit(&sandbox, "remote.git");
    let settings: String = (0..1000)
        .map(|number| format!("\tkey{number} = long-enough-for-1000-of-these-to-fill-a-pipe\n"))
        .collect();
    sandbox.configure(&format!(
        "[trace2]\n\tconfigParams = gikli.*\n[gikli]\n{settings}"
    ));
    sandbox.hook("post-checkout", "exit 3");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(&file_url(remote.root()), &sandbox.root().join("clone"));

    assert_eq!(failed_hook_of(&result).0, "post-checkout");
}

fn assert_holds_no_trace(text: &str) {
    for word in TRACE_WORDS {
        assert!(!text.contains(word), "{word} in {text}");
    }
}

/// A hook that keeps what a git writes to its error output gets that, and no trace.
#[test]
fn what_a_hook_captures_of_a_git_it_starts_holds_no_trace_and_no_password() {
    let sandbox = Sandbox::new();
    let remote = url_git_takes_for_a_path(&sandbox);
    let merged_file = sandbox.root().join("merged-output");
    let errors_file = sandbox.root().join("error-output");
    sandbox.hook(
        "post-checkout",
        &format!(
            "git ls-remote '{url}' > '{merged}' 2>&1\ngit ls-remote '{url}' 2> '{errors}' > /dev/null",
            url = remote.url,
            merged = as_git_writes_it(&merged_file),
            errors = as_git_writes_it(&errors_file),
        ),
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    gikli.clone_repository(&remote.url, &target).expect("clone");

    assert_cloned(&sandbox, &remote.remote, &target);
    let merged = fs::read_to_string(merged_file).expect("the hook ran");
    assert!(merged.contains("refs/heads/main"), "{merged}");
    assert_holds_no_trace(&merged);
    let errors = fs::read_to_string(errors_file).expect("the hook ran");
    assert_eq!(errors, "");
}

/// A host in which gikli cannot set up the reader of the trace.
fn host_without_trace_reader(sandbox: &Sandbox) -> gikli_core::HostEnvironment {
    let mut host = sandbox.host_environment();
    host.set("GIKLI_NO_TRACE_READER", "1");
    host
}

/// The configuration names a file, as a user can, and git must not use it for gikli's runs: when
/// the reader is not there, gikli asks for no trace, not for a trace elsewhere.
#[test]
fn without_a_reader_git_still_runs_and_no_trace_file_appears() {
    let sandbox = Sandbox::new();
    let remote = remote_named_with_a_commit(&sandbox, &format!("{PASSWORD}@v2.git"));
    let configured_trace = sandbox.root().join("configured-trace.json");
    sandbox.configure(&format!(
        "[trace2]\n\teventTarget = {}\n",
        as_git_writes_it(&configured_trace)
    ));
    let watch = TraceWatch::new(&sandbox);
    sandbox.hook("post-checkout", &watch.script());
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_in(host_without_trace_reader(&sandbox));

    gikli
        .clone_repository(&file_url(remote.root()), &target)
        .expect("clone");

    assert!(!configured_trace.exists());
    assert_eq!(watch.targets(), ["0"]);
    watch.assert_no_file_holds_the_password();
    // The plain git of this check does write that trace.
    assert_cloned(&sandbox, &remote, &target);
}

/// Without a trace, gikli cannot name the hook, and what a failed clone reports says what happened.
#[test]
fn without_a_reader_a_failing_hook_is_a_plain_failure() {
    let sandbox = Sandbox::new();
    let remote = remote_named_with_a_commit(&sandbox, "remote.git");
    sandbox.hook("post-checkout", "exit 3");
    let target = sandbox.root().join("clone");
    let url = file_url(remote.root());
    let gikli = sandbox.gikli_in(host_without_trace_reader(&sandbox));

    let result = gikli.clone_repository(&url, &target);

    assert_eq!(
        result,
        Err(Failure::CloneFailed {
            url,
            folder: target,
        })
    );
}

#[cfg(windows)]
mod wsl {
    use super::*;

    use std::ffi::OsString;

    use gikli_acceptance::WslRepository;
    use gikli_core::{Gikli, HostEnvironment};

    /// A repository inside WSL with a commit, to clone with hooks that live beside it.
    struct WslSetup {
        sandbox: Sandbox,
        source: WslRepository,
        /// `source`, in a folder whose name reads as the password.
        url: String,
    }

    impl WslSetup {
        fn new() -> Option<Self> {
            let source = WslRepository::create()?;
            source.write("notes.txt", "draft\n");
            source.git(&["add", "."]);
            source.git(&[
                "-c",
                "user.name=Test User",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--quiet",
                "-m",
                "First commit",
            ]);
            let alias = format!("{PASSWORD}@v2");
            source.link_beside(&alias);
            let url = format!("file://{}", source.linux_path_beside(&alias));
            Some(Self {
                sandbox: Sandbox::new(),
                source,
                url,
            })
        }

        fn hook(&self, name: &str, script: &str) {
            let path = self.source.linux_path_beside(&format!("hooks/{name}"));
            self.source.write_script(&path, script);
        }

        /// An environment with no native git, whose git inside WSL uses the hooks of this setup.
        fn host(&self, also_shared: &[(&str, &str)]) -> HostEnvironment {
            let mut host = self
                .sandbox
                .host_with_real_wsl_only()
                .expect("WSL is installed");
            let mut shared = OsString::from("GIT_CONFIG_COUNT:GIT_CONFIG_KEY_0:GIT_CONFIG_VALUE_0");
            host.set("GIT_CONFIG_COUNT", "1");
            host.set("GIT_CONFIG_KEY_0", "core.hooksPath");
            host.set("GIT_CONFIG_VALUE_0", self.source.linux_path_beside("hooks"));
            for (name, value) in also_shared {
                shared.push(":");
                shared.push(name);
                host.set(*name, *value);
            }
            host.set("WSLENV", shared);
            host
        }

        /// gikli with `host`, cloning `url` beside the repository.
        fn clone(&self, host: HostEnvironment) -> (Result<gikli_core::Project, Failure>, Gikli) {
            let gikli = self.sandbox.gikli_in(host);
            let target = self.source.windows_path_beside("clone");
            let result = gikli.clone_repository(&self.url, &target);
            (result, gikli)
        }

        /// Checks with plain git that the clone holds what the repository holds.
        fn assert_cloned(&self) {
            let clone = self.source.linux_path_beside("clone");
            assert_eq!(
                self.source.git(&["-C", &clone, "rev-parse", "HEAD"]),
                self.source.git(&["rev-parse", "HEAD"])
            );
        }

        /// The lines of a hook that note where the trace goes and what a file there holds.
        fn watch_script(&self) -> String {
            format!(
                "printf '%s\\n' \"$GIT_TRACE2_EVENT\" >> '{targets}'\n\
                 if [ -f \"$GIT_TRACE2_EVENT\" ]; then cat \"$GIT_TRACE2_EVENT\" >> '{contents}'; fi",
                targets = self.source.linux_path_beside("trace-targets"),
                contents = self.source.linux_path_beside("trace-contents"),
            )
        }

        fn output_of_hook(&self, name: &str) -> String {
            fs::read_to_string(self.source.windows_path_beside(name)).expect("the hook ran")
        }

        /// Checks that no file holding the password was where the trace goes, then or now.
        fn assert_no_file_holds_the_password(&self) {
            let contents = self.source.windows_path_beside("trace-contents");
            let during = fs::read_to_string(contents).unwrap_or_default();
            assert!(!during.contains(PASSWORD), "during the run: {during}");
            for target in self.output_of_hook("trace-targets").lines() {
                assert!(
                    !self.source.file_exists(target),
                    "the trace went to the file {target}"
                );
            }
        }

        /// Checks that the clone worked and that git got no target for a trace.
        fn assert_cloned_without_a_trace(&self) {
            self.assert_cloned();
            assert_eq!(self.output_of_hook("trace-targets"), "0\n");
        }
    }

    #[test]
    fn a_clone_inside_wsl_leaves_the_password_in_no_file_and_names_the_failed_hook() {
        let Some(setup) = WslSetup::new() else {
            eprintln!("skipped: no WSL with git on this machine");
            return;
        };
        setup.hook(
            "reference-transaction",
            "if [ -n \"$GIKLI_TEST_NESTED\" ]; then exit 5; fi",
        );
        setup.hook(
            "post-checkout",
            &format!(
                "git fetch --quiet origin\n\
                 {watch}\n\
                 git ls-remote '{url}' > '{merged}' 2>&1\n\
                 git ls-remote '{url}' 2> '{errors}' > /dev/null\n\
                 GIKLI_TEST_NESTED=1 git update-ref refs/heads/nested HEAD 2> /dev/null\n\
                 printf 'half a line' >&2\n\
                 exit 3",
                watch = setup.watch_script(),
                url = setup.url,
                merged = setup.source.linux_path_beside("merged-output"),
                errors = setup.source.linux_path_beside("error-output"),
            ),
        );

        let (result, gikli) = setup.clone(setup.host(&[]));

        let (hook, output) = failed_hook_of(&result);
        assert_eq!(hook, "post-checkout");
        assert!(output.contains("half a line"), "{output}");
        assert_holds_no_trace(&format!("{result:?}{:?}", gikli.operation_log()));
        setup.assert_cloned();
        setup.assert_no_file_holds_the_password();
        let merged = setup.output_of_hook("merged-output");
        assert!(merged.contains("HEAD"), "{merged}");
        assert_holds_no_trace(&merged);
        assert_eq!(setup.output_of_hook("error-output"), "");
    }

    /// The program holds on to all it inherited from git, the FIFO included. WSL ends it once
    /// `wsl.exe` has ended, so it can only say whether the run waited for it.
    #[test]
    fn a_program_a_hook_leaves_behind_inside_wsl_is_not_waited_for() {
        let Some(setup) = WslSetup::new() else {
            eprintln!("skipped: no WSL with git on this machine");
            return;
        };
        setup.hook(
            "post-checkout",
            &format!(
                "{watch}\n\
                 (\n\
                   sleep 20\n\
                   : > '{finished}'\n\
                 ) > /dev/null 2>&1 < /dev/null &",
                watch = setup.watch_script(),
                finished = setup.source.linux_path_beside("finished"),
            ),
        );

        let (result, _gikli) = setup.clone(setup.host(&[]));

        result.expect("clone");
        setup.assert_cloned();
        assert!(
            !setup.source.windows_path_beside("finished").exists(),
            "the run waited"
        );
        setup.assert_no_file_holds_the_password();
    }

    /// `mktemp` has nowhere to go, as real a way as any for the forwarder to fail.
    #[test]
    fn without_a_fifo_inside_wsl_git_still_runs_with_no_trace() {
        let Some(setup) = WslSetup::new() else {
            eprintln!("skipped: no WSL with git on this machine");
            return;
        };
        setup.hook("post-checkout", &setup.watch_script());

        let (result, gikli) = setup.clone(setup.host(&[("TMPDIR", "/nowhere/at/all")]));

        result.expect("clone");
        setup.assert_cloned_without_a_trace();
        assert_holds_no_trace(&format!("{:?}", gikli.operation_log()));
    }

    #[test]
    fn without_a_reader_git_inside_wsl_still_runs_with_no_trace() {
        let Some(setup) = WslSetup::new() else {
            eprintln!("skipped: no WSL with git on this machine");
            return;
        };
        setup.hook("post-checkout", &setup.watch_script());

        let (result, _gikli) = setup.clone(setup.host(&[("GIKLI_NO_TRACE_READER", "1")]));

        result.expect("clone");
        setup.assert_cloned_without_a_trace();
    }
}
