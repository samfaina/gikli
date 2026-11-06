//! gikli reads git's trace of a run as git writes it, and keeps it in memory: git writes every
//! command line to the trace, and those can hold a password in a URL. git hides the password of a
//! URL it can read only from 2.44 on, and never one that reads as a port.
//!
//! A hook sees where the trace goes in the variable a git it starts inherits, so these tests look
//! there from inside the run and again after it.

use std::fs;
use std::path::PathBuf;
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
    _server: HttpRemote,
    url: String,
}

/// git reads the password of this URL, and from 2.44 on hides it in the trace.
fn url_git_can_read(sandbox: &Sandbox) -> RemoteWithPasswordInUrl {
    let remote = remote_named_with_a_commit(sandbox, "remote.git");
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let url = server
        .url()
        .replacen("http://", &format!("http://{USERNAME}:{PASSWORD}@"), 1);
    RemoteWithPasswordInUrl {
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

fn assert_clone_leaves_the_password_in_no_file(url_of: fn(&Sandbox) -> RemoteWithPasswordInUrl) {
    let sandbox = Sandbox::new();
    let remote = url_of(&sandbox);
    let watch = TraceWatch::new(&sandbox);
    sandbox.hook("post-checkout", &watch.script());
    let gikli = sandbox.gikli();

    gikli
        .clone_repository(&remote.url, &sandbox.root().join("clone"))
        .expect("clone");

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

/// gikli has no fetch of its own yet, so a hook fetches inside a run gikli started. The git of the
/// hook writes to the trace of that run just as a fetch gikli starts would.
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
    let gikli = sandbox.gikli();

    gikli
        .clone_repository(&remote.url, &sandbox.root().join("clone"))
        .expect("clone");

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

    let Err(Failure::Explained { explanation }) = &result else {
        panic!("got {result:?}");
    };
    let ExplainedMessage::HookFailed { hook, output } = &explanation.message else {
        panic!("got {:?}", explanation.message);
    };
    assert_eq!(hook, "post-checkout");
    assert!(output.contains("Lint found 2 problems"), "{output}");
    let events: Vec<Event> = events.try_iter().collect();
    assert_holds_no_trace(&format!(
        "{result:?}{events:?}{:?}{:?}",
        gikli.operation_log(),
        gikli.notifications(),
    ));
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
    let merged = sandbox.root().join("merged-output");
    let errors = sandbox.root().join("error-output");
    sandbox.hook(
        "post-checkout",
        &format!(
            "git ls-remote '{url}' > '{merged}' 2>&1\ngit ls-remote '{url}' 2> '{errors}' > /dev/null",
            url = remote.url,
            merged = as_git_writes_it(&merged),
            errors = as_git_writes_it(&errors),
        ),
    );
    let gikli = sandbox.gikli();

    gikli
        .clone_repository(&remote.url, &sandbox.root().join("clone"))
        .expect("clone");

    let merged = fs::read_to_string(merged).expect("the hook ran");
    assert!(merged.contains("refs/heads/main"), "{merged}");
    assert_holds_no_trace(&merged);
    let errors = fs::read_to_string(errors).expect("the hook ran");
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

    assert!(target.join(".git").exists());
    assert_eq!(watch.targets(), ["0"]);
    watch.assert_no_file_holds_the_password();
    assert!(!configured_trace.exists());
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

    /// A repository inside WSL with a commit, to clone with hooks that live next to it.
    struct WslSetup {
        sandbox: Sandbox,
        source: WslRepository,
        /// `source`, in a folder whose name reads as the password.
        url: String,
        /// The folder of `source` as git inside WSL sees it.
        linux_root: String,
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
            let linux_root = source
                .git(&["rev-parse", "--show-toplevel"])
                .trim()
                .to_owned();
            source.link_next_to_it(&format!("{PASSWORD}@v2"));
            let parent = linux_root.trim_end_matches("/app");
            let url = format!("file://{parent}/{PASSWORD}@v2");
            Some(Self {
                sandbox: Sandbox::new(),
                source,
                url,
                linux_root,
            })
        }

        /// The folder hooks live in, inside WSL.
        fn hooks_folder(&self) -> String {
            format!("{}/hooks", self.linux_root.trim_end_matches("/app"))
        }

        fn hook(&self, name: &str, script: &str) {
            self.source
                .write_script(&format!("{}/{name}", self.hooks_folder()), script);
        }

        /// Where a clone goes, as Windows sees it.
        fn target(&self) -> PathBuf {
            self.source.root().parent().expect("a parent").join("clone")
        }

        /// A file next to the repository, as git inside WSL sees it.
        fn linux_file(&self, name: &str) -> String {
            format!("{}/{name}", self.linux_root.trim_end_matches("/app"))
        }

        /// The same file, as Windows sees it.
        fn windows_file(&self, name: &str) -> PathBuf {
            self.source.root().parent().expect("a parent").join(name)
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
            host.set("GIT_CONFIG_VALUE_0", self.hooks_folder());
            for (name, value) in also_shared {
                shared.push(":");
                shared.push(name);
                host.set(*name, *value);
            }
            host.set("WSLENV", shared);
            host
        }

        /// gikli with `host`, cloning `url` to the target.
        fn clone(&self, host: HostEnvironment) -> (Result<gikli_core::Project, Failure>, Gikli) {
            let gikli = self.sandbox.gikli_in(host);
            let result = gikli.clone_repository(&self.url, &self.target());
            (result, gikli)
        }
    }

    /// What a hook writes down of the trace, inside WSL.
    fn watch_script(setup: &WslSetup) -> String {
        format!(
            "printf '%s\\n' \"$GIT_TRACE2_EVENT\" >> '{targets}'\n\
             if [ -f \"$GIT_TRACE2_EVENT\" ]; then cat \"$GIT_TRACE2_EVENT\" >> '{contents}'; fi",
            targets = setup.linux_file("trace-targets"),
            contents = setup.linux_file("trace-contents"),
        )
    }

    fn assert_no_file_holds_the_password(setup: &WslSetup) {
        let targets =
            fs::read_to_string(setup.windows_file("trace-targets")).expect("the hook ran");
        let during = fs::read_to_string(setup.windows_file("trace-contents")).unwrap_or_default();
        assert!(!during.contains(PASSWORD), "during the run: {during}");
        for target in targets.lines() {
            assert!(
                !setup.source.file_exists(target),
                "the trace went to the file {target}"
            );
        }
    }

    #[test]
    fn a_clone_inside_wsl_leaves_the_password_in_no_file_and_names_the_failed_hook() {
        let Some(setup) = WslSetup::new() else {
            eprintln!("skipped: no WSL with git on this machine");
            return;
        };
        setup.hook(
            "post-checkout",
            &format!(
                "{watch}\n\
                 git --version > '{merged}' 2>&1\n\
                 printf 'half a line' >&2\n\
                 exit 3",
                watch = watch_script(&setup),
                merged = setup.linux_file("merged-output"),
            ),
        );

        let (result, gikli) = setup.clone(setup.host(&[]));

        let Err(Failure::Explained { explanation }) = &result else {
            panic!("got {result:?}");
        };
        let ExplainedMessage::HookFailed { hook, output } = &explanation.message else {
            panic!("got {:?}", explanation.message);
        };
        assert_eq!(hook, "post-checkout");
        assert!(output.contains("half a line"), "{output}");
        assert_holds_no_trace(&format!("{result:?}{:?}", gikli.operation_log()));
        assert_no_file_holds_the_password(&setup);
        let merged = fs::read_to_string(setup.windows_file("merged-output")).expect("the hook ran");
        assert!(merged.starts_with("git version"), "{merged}");
        assert_holds_no_trace(&merged);
    }

    /// The program holds on to all it inherited from git, the FIFO included. WSL ends it once
    /// `wsl.exe` has ended, so it can only say whether the run waited for it.
    #[test]
    fn a_program_a_hook_leaves_behind_inside_wsl_is_not_waited_for() {
        let Some(setup) = WslSetup::new() else {
            eprintln!("skipped: no WSL with git on this machine");
            return;
        };
        let finished = setup.linux_file("finished");
        setup.hook(
            "post-checkout",
            &format!(
                "{watch}\n\
                 (\n\
                   sleep 20\n\
                   : > '{finished}'\n\
                 ) > /dev/null 2>&1 < /dev/null &",
                watch = watch_script(&setup),
            ),
        );

        let (result, _gikli) = setup.clone(setup.host(&[]));

        result.expect("clone");
        assert!(!setup.windows_file("finished").exists(), "the run waited");
        assert_no_file_holds_the_password(&setup);
    }

    /// `mktemp` has nowhere to go, as real a way as any for the forwarder to fail.
    #[test]
    fn without_a_fifo_inside_wsl_git_still_runs_with_no_trace() {
        let Some(setup) = WslSetup::new() else {
            eprintln!("skipped: no WSL with git on this machine");
            return;
        };
        setup.hook("post-checkout", &watch_script(&setup));

        let (result, gikli) = setup.clone(setup.host(&[("TMPDIR", "/nowhere/at/all")]));

        result.expect("clone");
        let targets = fs::read_to_string(setup.windows_file("trace-targets")).expect("hook ran");
        assert_eq!(targets, "0\n");
        assert_holds_no_trace(&format!("{:?}", gikli.operation_log()));
    }

    #[test]
    fn without_a_reader_git_inside_wsl_still_runs_with_no_trace() {
        let Some(setup) = WslSetup::new() else {
            eprintln!("skipped: no WSL with git on this machine");
            return;
        };
        setup.hook("post-checkout", &watch_script(&setup));

        let (result, _gikli) = setup.clone(setup.host(&[("GIKLI_NO_TRACE_READER", "1")]));

        result.expect("clone");
        let targets = fs::read_to_string(setup.windows_file("trace-targets")).expect("hook ran");
        assert_eq!(targets, "0\n");
    }
}
