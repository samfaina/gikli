//! A clone URL can carry a password. gikli hides it wherever it keeps or shows the URL, and git
//! still receives the URL as the user gave it.

use std::path::Path;
use std::time::Duration;

use gikli_acceptance::{Sandbox, TestRepository, askpass, fake_git, file_url};
use gikli_core::{
    Event, ExplainedMessage, Explanation, Failure, Gikli, LongOperation, NextStep,
    OperationLogEntry, Progress, PromptKind,
};

const USERNAME: &str = "ada-lovelace-1815";
const PASSWORD: &str = "tr0ub4dor-and-3";

/// A bare remote holding one commit, pushed from a working repository.
fn remote_with_a_commit(sandbox: &Sandbox) -> TestRepository {
    remote_named_with_a_commit(sandbox, "remote.git")
}

/// A bare remote in the folder `name` holding one commit, pushed from a working repository.
fn remote_named_with_a_commit(sandbox: &Sandbox, name: &str) -> TestRepository {
    let remote = sandbox.bare_remote(name);
    let work = sandbox.repository("work");
    work.write("notes.txt", "draft\n");
    work.commit_all("First commit");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main"]);
    remote
}

/// `url`, an `http://` URL without credentials, carrying these.
fn with_credentials(url: &str, username: &str, password: &str) -> String {
    url.replacen("http://", &format!("http://{username}:{password}@"), 1)
}

#[test]
fn a_clone_works_with_the_password_in_the_url_and_only_git_sees_it() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let url = with_credentials(&server.url(), USERNAME, PASSWORD);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    gikli
        .clone_repository(&url, &target)
        .expect("the remote takes the password in the URL");

    assert_cloned(&sandbox, &remote, &target);
    assert_git_received(&sandbox, &target, &url);

    let hidden = with_credentials(&server.url(), USERNAME, "***");
    let events: Vec<Event> = events.try_iter().collect();
    assert_clone_started_showing(&events, &hidden);
    let args = logged_run(&gikli, "clone").args;
    assert!(args.contains(&hidden), "{args:?}");
    let log = gikli.operation_log();
    assert!(!format!("{log:?}").contains(PASSWORD), "{log:?}");
    assert!(!format!("{events:?}").contains(PASSWORD), "{events:?}");
}

/// What `gikli` logged about the command with this argument.
fn logged_run(gikli: &Gikli, argument: &str) -> OperationLogEntry {
    gikli
        .operation_log()
        .into_iter()
        .find(|entry| entry.args.iter().any(|arg| arg == argument))
        .expect("the command is logged")
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

/// Checks with plain git that the clone in `target` has `url` as its remote.
fn assert_git_received(sandbox: &Sandbox, target: &Path, url: &str) {
    assert_eq!(
        sandbox
            .existing_repository(target)
            .git(&["config", "remote.origin.url"])
            .trim_end(),
        url,
        "git received the URL as the user gave it"
    );
}

fn assert_clone_started_showing(events: &[Event], shown: &str) {
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::OperationStarted { operation: LongOperation::Clone { url, .. }, .. }
                if url == shown
        )),
        "{events:?}"
    );
}

#[test]
fn what_a_hook_repeats_of_the_url_shows_without_the_password() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let url = with_credentials(&server.url(), USERNAME, PASSWORD);
    sandbox.hook("post-checkout", "git remote get-url origin >&2");
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    gikli.clone_repository(&url, &target).expect("clone");

    assert_cloned(&sandbox, &remote, &target);
    let stderr = logged_run(&gikli, "clone").stderr;
    assert!(!stderr.contains(PASSWORD), "{stderr}");
    assert!(
        stderr.contains(&with_credentials(&server.url(), USERNAME, "***")),
        "{stderr}"
    );
}

#[test]
fn a_failing_hook_that_repeats_the_url_is_explained_without_the_password() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let url = with_credentials(&server.url(), USERNAME, PASSWORD);
    sandbox.hook(
        "post-checkout",
        "echo \"Cannot reach $(git remote get-url origin)\"\nexit 3",
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(&url, &target);

    let Err(Failure::Explained { explanation }) = result else {
        panic!("got {result:?}");
    };
    let ExplainedMessage::HookFailed { hook, output } = explanation.message else {
        panic!("got {:?}", explanation.message);
    };
    assert_eq!(hook, "post-checkout");
    // git can add lines of its own that repeat the URL: git 2.34.0 warned of a redirect on CI.
    let hidden = with_credentials(&server.url(), USERNAME, "***");
    assert!(
        output
            .lines()
            .any(|line| line == format!("Cannot reach {hidden}")),
        "{output}"
    );
    assert!(!output.contains(PASSWORD), "{output}");
    assert_cloned(&sandbox, &remote, &target);
}

#[test]
fn only_the_password_of_a_url_goes_away() {
    let cases = [
        (
            "http://ada:s3cret@host/repo.git",
            "http://ada:***@host/repo.git",
        ),
        (
            "https://ada:pa%40ss%3Aw0rd@host:8443/repo.git?ref=main#top",
            "https://ada:***@host:8443/repo.git?ref=main#top",
        ),
        (
            "http://ada:p@ss:w0rd@[::1]:8080/repo",
            "http://ada:***@[::1]:8080/repo",
        ),
        ("https://ada:two words@host", "https://ada:***@host"),
        (
            "fatal: unable to access 'https://ada:s3cret@host/repo': denied",
            "fatal: unable to access 'https://ada:***@host/repo': denied",
        ),
        (
            "from https://ada:s3cret@one/x to ssh://bob:hunter2@two/y",
            "from https://ada:***@one/x to ssh://bob:***@two/y",
        ),
        (
            "https://ada:se/cret@host/repo.git",
            "https://ada:***@host/repo.git",
        ),
        (
            "https://ada:pa#ss@host/repo.git",
            "https://ada:***@host/repo.git",
        ),
        (
            "https://ada:pa?ss@host/repo.git",
            "https://ada:***@host/repo.git",
        ),
        ("https://ada@host/repo.git", "https://ada@host/repo.git"),
        ("ssh://git@host:org/repo@v1", "ssh://git@host:org/repo@v1"),
        ("http://[::1]/ada@home", "http://[::1]/ada@home"),
        ("http://[::1]:8080/ada@home", "http://[::1]:8080/ada@home"),
        ("ssh://git@host:22/repo.git", "ssh://git@host:22/repo.git"),
        ("http://[::1]:8080/repo", "http://[::1]:8080/repo"),
        (
            "https://user:123/abc@host/repo.git",
            "https://user:***@host/repo.git",
        ),
        (
            "https://user:/abc@host/repo.git",
            "https://user:***@host/repo.git",
        ),
        (
            "fatal: unable to access 'https://user:123/abc@127.0.0.1/repo.git/': Could not resolve host: user",
            "fatal: unable to access 'https://user:***@127.0.0.1/repo.git/': Could not resolve host: user",
        ),
        (
            "https://host:8080/team@v2/repo.git",
            "https://host:***@v2/repo.git",
        ),
        ("https://host:8443/ada:lace@home", "https://host:***@home"),
        (
            "https://host:8080/team/repo.git",
            "https://host:8080/team/repo.git",
        ),
        (
            "https://host:8080/repo.git failed for ada@example.com",
            "https://host:8080/repo.git failed for ada@example.com",
        ),
        (
            "'https://host:8080/repo.git','ada@example.com'",
            "'https://host:8080/repo.git','ada@example.com'",
        ),
        (
            "\\\"https://host:8080/repo.git\\\",\\\"ada@example.com\\\"",
            "\"https://host:8080/repo.git\",\"ada@example.com\"",
        ),
        (
            "https://host:8080/a.git and https://ada:s3cret@two/y",
            "https://host:8080/a.git and https://ada:***@two/y",
        ),
        (
            "https://user:123/a b@host/repo.git",
            "https://user:123/a b@host/repo.git",
        ),
        (
            "https://user:/a b@host/repo.git",
            "https://user:/a b@host/repo.git",
        ),
        ("git@host:org/repo.git", "git@host:org/repo.git"),
        ("file:///srv/ada:lace@home", "file:///srv/ada:lace@home"),
    ];
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let script: Vec<String> = cases
        .iter()
        .map(|(written, _)| format!("echo \"{written}\" >&2"))
        .collect();
    sandbox.hook("post-checkout", &script.join("\n"));
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    gikli
        .clone_repository(&file_url(remote.root()), &target)
        .expect("clone");

    assert_cloned(&sandbox, &remote, &target);
    let stderr = logged_run(&gikli, "clone").stderr;
    for (written, shown) in cases {
        assert!(
            stderr.lines().any(|line| line == shown),
            "{written} should show as {shown}: {stderr}"
        );
    }
}

#[test]
fn a_remote_that_refuses_the_password_in_the_url_is_explained_without_it() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let wrong = "not-the-password-1815";
    let url = with_credentials(&server.url(), USERNAME, wrong);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_with_prompts(askpass!());
    let events = gikli.subscribe();

    let result = gikli.clone_repository(&url, &target);

    assert_eq!(
        result,
        Err(Failure::Explained {
            explanation: Explanation {
                message: ExplainedMessage::AuthenticationFailed,
                next_step: NextStep::CheckCredentials,
                actions: vec![],
            }
        })
    );
    let events: Vec<Event> = events.try_iter().collect();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::PromptRaised(_))),
        "{events:?}"
    );
    let kept = format!("{:?}{events:?}", gikli.operation_log());
    assert!(!kept.contains(wrong), "{kept}");
    assert!(!target.join(".git").exists());
}

/// git cannot use a password like these unless it is encoded, and its messages can repeat the URL.
#[test]
fn a_password_that_should_have_been_encoded_stays_hidden_when_the_clone_fails() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let gikli = sandbox.gikli();
    for (folder, password) in [
        ("slash", "se/cr3t-1815"),
        ("hash", "se#cr3t-1815"),
        ("question-mark", "se?cr3t-1815"),
    ] {
        let url = with_credentials(&server.url(), USERNAME, password);
        let target = sandbox.root().join(folder);
        let events = gikli.subscribe();

        let result = gikli.clone_repository(&url, &target);

        let events: Vec<Event> = events.try_iter().collect();
        let shown = format!("{result:?}{:?}{events:?}", gikli.operation_log());
        assert!(result.is_err(), "{shown}");
        assert!(!shown.contains("cr3t-1815"), "{shown}");
        assert!(!target.join(".git").exists());
    }
}

#[test]
fn a_clone_that_fails_names_the_url_without_the_password() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote_answering(&remote, "500 Internal Server Error");
    let url = with_credentials(&server.url(), USERNAME, PASSWORD);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(&url, &target);

    assert_eq!(
        result,
        Err(Failure::CloneFailed {
            url: with_credentials(&server.url(), USERNAME, "***"),
            folder: target.clone(),
        })
    );
    let log = gikli.operation_log();
    assert!(!format!("{log:?}").contains(PASSWORD), "{log:?}");
    assert!(!target.join(".git").exists());
}

#[test]
fn progress_that_repeats_a_url_shows_without_the_password() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.hook(
        "post-checkout",
        "echo 'Fetching http://ada:s3cret@host: 50% (1/2)' >&2",
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    gikli
        .clone_repository(&file_url(remote.root()), &target)
        .expect("clone");

    assert_cloned(&sandbox, &remote, &target);
    let events: Vec<Event> = events.try_iter().collect();
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::OperationProgress { progress: Progress { phase, percent: 50 }, .. }
                if phase == "Fetching http://ada:***@host"
        )),
        "{events:?}"
    );
    assert!(!format!("{events:?}").contains("s3cret"), "{events:?}");
}

#[test]
fn a_prompt_that_repeats_a_url_reaches_the_user_without_the_password() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.hook(
        "post-checkout",
        "\"$GIT_ASKPASS\" 'Password for http://ada:s3cret@host/repo: ' > /dev/null\nexit 0",
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_with_prompts(askpass!());
    let events = gikli.subscribe();

    let prompt = std::thread::scope(|scope| {
        let clone = scope.spawn(|| gikli.clone_repository(&file_url(remote.root()), &target));
        let prompt = loop {
            let event = events
                .recv_timeout(Duration::from_secs(60))
                .expect("the hook asks");
            if let Event::PromptRaised(prompt) = event {
                break prompt;
            }
        };
        assert_eq!(gikli.pending_prompts(), std::slice::from_ref(&prompt));
        gikli.cancel_prompt(prompt.id).expect("cancel the prompt");
        clone.join().expect("the clone ends").expect("clone");
        prompt
    });

    assert_cloned(&sandbox, &remote, &target);
    assert_eq!(prompt.kind, PromptKind::Password);
    assert_eq!(prompt.text, "Password for http://ada:***@host/repo: ");
}

/// git fails on this URL and repeats it: it takes the `9` for a port and connects to nothing, with
/// no DNS lookup. The `9` goes along with the password, so the test looks for the password.
#[test]
fn a_password_that_reads_as_a_port_stays_hidden_when_the_clone_fails() {
    let sandbox = Sandbox::new();
    let url = format!("https://127.0.0.1:9/{PASSWORD}@host/repo.git");
    let hidden = "https://127.0.0.1:***@host/repo.git";
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    let result = gikli.clone_repository(&url, &target);

    let Err(Failure::Explained { explanation }) = result else {
        panic!("got {result:?}");
    };
    assert_eq!(explanation.message, ExplainedMessage::ServerUnreachable);
    let events: Vec<Event> = events.try_iter().collect();
    assert_clone_started_showing(&events, hidden);
    let logged = logged_run(&gikli, "clone");
    assert!(logged.args.iter().any(|arg| arg == hidden), "{logged:?}");
    assert!(
        logged
            .stderr
            .contains(&format!("unable to access '{hidden}/'")),
        "{logged:?}"
    );
    let kept = format!("{:?}{events:?}", gikli.operation_log());
    assert!(!kept.contains(PASSWORD), "{kept}");
    assert!(!target.join(".git").exists());
}

/// Text alone cannot tell this URL from one with a password, so what shows of it loses a part, and
/// git still receives all of it.
#[test]
fn a_url_with_a_port_and_an_at_sign_in_its_path_reaches_git_as_the_user_gave_it() {
    let sandbox = Sandbox::new();
    let remote = remote_named_with_a_commit(&sandbox, "team@v2.git");
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    sandbox.configure(&format!(
        "[credential]\n\thelper = \"!f() {{ echo username={USERNAME}; echo password={PASSWORD}; }}; f\"\n"
    ));
    let url = server.url();
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    gikli.clone_repository(&url, &target).expect("clone");

    assert_cloned(&sandbox, &remote, &target);
    assert_git_received(&sandbox, &target, &url);
    let args = logged_run(&gikli, "clone").args;
    assert!(
        args.iter().any(|arg| arg == "http://127.0.0.1:***@v2.git"),
        "{args:?}"
    );
}

/// The real git writes no URL on its standard output for any command gikli runs today, and a
/// stand-in can say what it likes in its version.
#[test]
fn what_git_writes_on_its_standard_output_shows_without_the_password() {
    let sandbox = Sandbox::new();
    let git = sandbox.fake_git_in(
        "git-with-a-url",
        fake_git!(),
        "2.50.0 http://ada:s3cret@host/repo.git",
    );
    let gikli = sandbox.gikli();

    gikli
        .set_git_path(Some(git.executable().to_path_buf()))
        .expect("save the path to git");

    assert_eq!(
        logged_run(&gikli, "--version").stdout,
        "git version 2.50.0 http://ada:***@host/repo.git\n"
    );
}
