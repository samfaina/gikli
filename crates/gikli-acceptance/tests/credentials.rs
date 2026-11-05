//! Authentication works as it does in the user's terminal: their credential helper stays in charge,
//! prompts reach the user inside gikli, and gikli stores none of the answers.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use gikli_acceptance::{
    Sandbox, TestRepository, WslRepository, as_git_writes_it, askpass, speaking,
};
use gikli_core::{
    Event, ExplainedMessage, Explanation, Failure, Gikli, NextStep, Prompt, PromptKind, Repository,
    Settings,
};

const USERNAME: &str = "ada-lovelace-1815";
const PASSWORD: &str = "correct horse battery staple";

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

/// Installs a credential helper as a user would, in `~/.gitconfig`. It records each action git asks
/// of it, and what git tells it to store, in the file it returns, and runs `on_get` to answer `get`.
fn credential_helper(sandbox: &Sandbox, on_get: &str) -> PathBuf {
    let folder = sandbox.folder("credential-helper");
    let record = folder.join("record");
    let helper = folder.join("helper");
    sandbox.write_script(
        &helper,
        &format!(
            "echo \"$1\" >> '{record}'\n\
             if [ \"$1\" = get ]; then :\n{on_get}\nelse grep -E '^(username|password)=' >> '{record}'; fi",
            record = as_git_writes_it(&record),
        ),
    );
    sandbox.configure(&format!(
        "[credential]\n\thelper = \"!\\\"{}\\\"\"\n",
        as_git_writes_it(&helper)
    ));
    record
}

fn knows_the_credentials() -> String {
    format!("echo username={USERNAME}; echo password={PASSWORD}")
}

fn assert_cloned(sandbox: &Sandbox, remote: &TestRepository, target: &Path) {
    assert_eq!(
        sandbox
            .existing_repository(target)
            .git(&["rev-parse", "HEAD"]),
        remote.git(&["rev-parse", "main"])
    );
}

#[test]
fn the_users_credential_helper_answers_without_a_prompt_by_default() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let record = credential_helper(&sandbox, &knows_the_credentials());
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    assert!(gikli.settings().use_credential_helper);
    assert!(Settings::default().use_credential_helper);
    gikli
        .clone_repository(&server.url(), &target)
        .expect("clone with the helper's credentials");

    assert_cloned(&sandbox, &remote, &target);
    let record = fs::read_to_string(record).expect("git asked the helper");
    assert!(record.starts_with("get\n"), "{record}");
}

/// Runs `operation` while answering every prompt that reaches the user with what `answer` says,
/// or cancelling it on None. Returns what the operation returned, the prompts in order, and every
/// event sent meanwhile.
fn answering_prompts<T: Send>(
    gikli: &Gikli,
    answer: impl Fn(&Prompt) -> Option<String>,
    operation: impl FnOnce() -> T + Send,
) -> (T, Vec<Prompt>, Vec<Event>) {
    let events = gikli.subscribe();
    std::thread::scope(|scope| {
        let running = scope.spawn(operation);
        let mut prompts = Vec::new();
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !running.is_finished() {
            assert!(
                Instant::now() < deadline,
                "the operation still runs, after these prompts: {prompts:?}"
            );
            let Ok(event) = events.recv_timeout(Duration::from_millis(20)) else {
                continue;
            };
            if let Event::PromptRaised(prompt) = &event {
                assert!(
                    gikli.pending_prompts().contains(prompt),
                    "a window that opens now shows it too"
                );
                match answer(prompt) {
                    Some(text) => gikli
                        .answer_prompt(prompt.id, &text)
                        .expect("answer the prompt"),
                    None => gikli.cancel_prompt(prompt.id).expect("cancel the prompt"),
                }
                prompts.push(prompt.clone());
            }
            seen.push(event);
        }
        seen.extend(events.try_iter());
        (running.join().expect("the operation ends"), prompts, seen)
    })
}

/// The answers the user gives when git asks for a username and a password.
fn the_right_credentials(prompt: &Prompt) -> Option<String> {
    match prompt.kind {
        PromptKind::Username => Some(USERNAME.to_owned()),
        PromptKind::Password => Some(PASSWORD.to_owned()),
        _ => panic!("unexpected prompt: {prompt:?}"),
    }
}

/// Everything in `folder` and below that is a file, read as text.
fn text_of_files_in(folder: &Path) -> String {
    let mut text = String::new();
    for entry in fs::read_dir(folder).expect("read folder").flatten() {
        if entry.path().is_dir() {
            text.push_str(&text_of_files_in(&entry.path()));
        } else {
            text.push_str(&String::from_utf8_lossy(
                &fs::read(entry.path()).expect("read file"),
            ));
        }
    }
    text
}

#[test]
fn username_and_password_prompts_reach_the_user_and_gikli_keeps_neither_answer() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_with_prompts(askpass!());

    let (result, prompts, events) = answering_prompts(&gikli, the_right_credentials, || {
        gikli.clone_repository(&server.url(), &target)
    });

    result.unwrap_or_else(|failure| panic!("{failure:?}: {:#?}", gikli.operation_log().pop()));
    assert_cloned(&sandbox, &remote, &target);
    assert_eq!(
        prompts.iter().map(|prompt| prompt.kind).collect::<Vec<_>>(),
        [PromptKind::Username, PromptKind::Password]
    );
    assert!(prompts[0].text.contains(&server.url()[..20]), "{prompts:?}");
    assert!(gikli.pending_prompts().is_empty());
    for prompt in &prompts {
        assert!(
            events.contains(&Event::PromptClosed { id: prompt.id }),
            "{events:?}"
        );
    }

    let kept = format!(
        "{}{:?}{:?}",
        text_of_files_in(&sandbox.config_folder()),
        gikli.operation_log(),
        gikli.settings(),
    );
    assert!(!kept.contains(PASSWORD), "{kept}");
    assert!(!kept.contains(USERNAME), "{kept}");
    // git names the user when it asks for the password, so only the password is checked here.
    assert!(!format!("{events:?}").contains(PASSWORD), "{events:?}");

    let second = sandbox.root().join("second");
    let (result, prompts, _) = answering_prompts(&gikli, the_right_credentials, || {
        gikli.clone_repository(&server.url(), &second)
    });
    result.expect("clone again, answering again");
    assert_cloned(&sandbox, &remote, &second);
    assert_eq!(prompts.len(), 2, "gikli remembers no answer: {prompts:?}");
}

#[test]
fn with_the_setting_off_git_asks_the_user_and_the_helper_stays_out_of_it() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let record = credential_helper(&sandbox, &knows_the_credentials());
    let gikli = sandbox.gikli_with_prompts(askpass!());

    let changed = gikli
        .set_use_credential_helper(false)
        .expect("turn the setting off");

    assert!(!changed.use_credential_helper);
    assert!(
        !sandbox.gikli().settings().use_credential_helper,
        "the next start remembers it"
    );
    let target = sandbox.root().join("clone");
    let (result, prompts, _) = answering_prompts(&gikli, the_right_credentials, || {
        gikli.clone_repository(&server.url(), &target)
    });
    result.expect("clone with the answers");
    assert_cloned(&sandbox, &remote, &target);
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    assert!(!record.exists(), "git never started the helper");

    gikli
        .set_use_credential_helper(true)
        .expect("turn the setting on");
    let again = sandbox.root().join("again");
    let (result, prompts, _) = answering_prompts(&gikli, the_right_credentials, || {
        gikli.clone_repository(&server.url(), &again)
    });
    result.expect("clone with the credentials of the helper");
    assert_cloned(&sandbox, &remote, &again);
    assert_eq!(prompts, []);
    assert!(record.exists());
}

#[test]
fn settings_saved_before_the_setting_existed_keep_the_helper_in_charge() {
    let sandbox = Sandbox::new();
    let config = sandbox.config_folder();
    fs::create_dir_all(&config).expect("create the configuration folder");
    fs::write(config.join("settings.json"), r#"{"showIgnoredFiles":true}"#)
        .expect("write older settings");

    let settings = sandbox.gikli().settings();

    assert!(settings.show_ignored_files);
    assert!(settings.use_credential_helper);
}

/// The answers of a user who remembers the username and not the password.
fn the_wrong_password(prompt: &Prompt) -> Option<String> {
    match prompt.kind {
        PromptKind::Username => Some(USERNAME.to_owned()),
        _ => Some("not the password".to_owned()),
    }
}

#[test]
fn the_users_helper_keeps_what_the_user_answered_and_forgets_what_the_remote_refused() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let record = credential_helper(&sandbox, "");
    let gikli = sandbox.gikli_with_prompts(askpass!());

    let target = sandbox.root().join("clone");
    let (result, _, _) = answering_prompts(&gikli, the_right_credentials, || {
        gikli.clone_repository(&server.url(), &target)
    });
    result.expect("clone with the answers");
    assert_cloned(&sandbox, &remote, &target);
    let told = fs::read_to_string(&record).expect("git told the helper");
    assert!(
        told.contains(&format!(
            "store\nusername={USERNAME}\npassword={PASSWORD}\n"
        )),
        "{told}"
    );

    let refused = sandbox.root().join("refused");
    let (result, _, _) = answering_prompts(&gikli, the_wrong_password, || {
        gikli.clone_repository(&server.url(), &refused)
    });
    assert!(result.is_err());
    let told = fs::read_to_string(&record).expect("git told the helper");
    assert!(
        told.contains(&format!(
            "erase\nusername={USERNAME}\npassword=not the password\n"
        )),
        "{told}"
    );
}

fn explanation_of_failed_authentication() -> Explanation {
    Explanation {
        message: ExplainedMessage::AuthenticationFailed,
        next_step: NextStep::CheckCredentials,
        actions: vec![],
    }
}

fn assert_explained<T: std::fmt::Debug>(result: Result<T, Failure>, explanation: Explanation) {
    match result {
        Err(Failure::Explained { explanation: got }) => assert_eq!(got, explanation),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn a_refused_password_gets_a_dedicated_message_and_stays_out_of_the_operation_log() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_with_prompts(askpass!());

    let (result, _, _) = answering_prompts(&gikli, the_wrong_password, || {
        gikli.clone_repository(&server.url(), &target)
    });

    assert_explained(result, explanation_of_failed_authentication());
    let log = format!("{:?}", gikli.operation_log());
    assert!(!log.contains("not the password"), "{log}");
    assert!(!target.join(".git").exists());
}

#[test]
fn credentials_a_helper_gave_and_the_remote_refused_get_the_same_message() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    credential_helper(
        &sandbox,
        &format!("echo username={USERNAME}; echo password=expired"),
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(&server.url(), &target);

    assert_explained(result, explanation_of_failed_authentication());
}

#[test]
fn declining_to_answer_and_having_nobody_to_ask_get_the_same_message() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);

    let gikli = sandbox.gikli_with_prompts(askpass!());
    let target = sandbox.root().join("declined");
    let (result, prompts, _) = answering_prompts(
        &gikli,
        |_| None,
        || gikli.clone_repository(&server.url(), &target),
    );
    assert_explained(result, explanation_of_failed_authentication());
    assert_eq!(prompts.len(), 1, "{prompts:?}");

    let gikli = sandbox.gikli();
    let target = sandbox.root().join("nobody");
    let result = gikli.clone_repository(&server.url(), &target);
    assert_explained(result, explanation_of_failed_authentication());
}

#[test]
fn a_refusal_is_explained_the_same_when_git_speaks_another_language() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    credential_helper(
        &sandbox,
        &format!("echo username={USERNAME}; echo password=expired"),
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_in(speaking(sandbox.host_environment(), "de"));

    let result = gikli.clone_repository(&server.url(), &target);

    assert_explained(result, explanation_of_failed_authentication());
}

#[test]
fn declining_to_answer_is_explained_the_same_when_git_speaks_another_language() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    let mut gikli = sandbox.gikli_in(speaking(sandbox.host_environment(), "de"));
    gikli
        .bring_prompts_through(askpass!().into())
        .expect("start the askpass bridge");
    let target = sandbox.root().join("declined");

    let (result, _, _) = answering_prompts(
        &gikli,
        |_| None,
        || gikli.clone_repository(&server.url(), &target),
    );

    assert_explained(result, explanation_of_failed_authentication());
}

#[test]
fn a_user_who_forbade_asking_for_credentials_gets_the_same_message() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote(&remote, USERNAME, PASSWORD);
    if !sandbox.git_knows_credential_interactive() {
        eprintln!("skipped: this git always asks");
        return;
    }
    sandbox.configure("[credential]\n\tinteractive = false\n");
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_with_prompts(askpass!());

    let (result, prompts, _) = answering_prompts(&gikli, the_right_credentials, || {
        gikli.clone_repository(&server.url(), &target)
    });

    assert_explained(result, explanation_of_failed_authentication());
    assert_eq!(prompts, []);
}

#[test]
fn a_remote_that_forbids_access_or_wants_proxy_credentials_is_an_authentication_failure() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let gikli = sandbox.gikli();
    for (status, folder) in [("403 Forbidden", "forbidden"), ("407 Proxy Auth", "proxy")] {
        let server = sandbox.http_remote_answering(&remote, status);
        let target = sandbox.root().join(folder);

        let result = gikli.clone_repository(&server.url(), &target);

        assert_explained(result, explanation_of_failed_authentication());
    }
}

#[test]
fn a_remote_that_breaks_is_not_taken_for_an_authentication_failure() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let server = sandbox.http_remote_answering(&remote, "500 Internal Server Error");
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(&server.url(), &target);

    assert_eq!(
        result,
        Err(Failure::CloneFailed {
            url: server.url(),
            folder: target.clone()
        })
    );
}

#[test]
fn answering_a_prompt_that_is_gone_says_so() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli_with_prompts(askpass!());

    assert_eq!(
        gikli.answer_prompt(7, "anything"),
        Err(Failure::PromptGone { id: 7 })
    );
    assert_eq!(gikli.cancel_prompt(7), Err(Failure::PromptGone { id: 7 }));
}

/// The stand-in for ssh hands the connection to this, once it has let the user in.
fn serves(remote: &TestRepository) -> String {
    format!("exec git upload-pack '{}'", as_git_writes_it(remote.root()))
}

const SSH_URL: &str = "ssh://git@gikli.test/remote.git";
const PASSPHRASE: &str = "my key, my castle";
const REFUSED_BY_SSH: &str = "echo 'git@gikli.test: Permission denied (publickey).' >&2; exit 255";

/// What ssh does for a key with a passphrase: it asks, then the server takes the key or not. The
/// stand-in asks only as the real ssh would, when told to use the askpass program.
fn asks_for_the_passphrase(remote: &TestRepository) -> String {
    format!(
        "[ \"$SSH_ASKPASS_REQUIRE\" = force ] || {{ echo 'no askpass' >&2; exit 1; }}\n\
         answer=$(\"$SSH_ASKPASS\" \"Enter passphrase for key '/home/ada/.ssh/id_ed25519': \")\n\
         if [ \"$answer\" != '{PASSPHRASE}' ]; then {REFUSED_BY_SSH}; fi\n\
         {}",
        serves(remote)
    )
}

#[test]
fn an_ssh_passphrase_prompt_reaches_the_user_and_gikli_keeps_no_answer() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.ssh_stand_in(&asks_for_the_passphrase(&remote));
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_with_prompts(askpass!());

    let (result, prompts, events) = answering_prompts(
        &gikli,
        |_| Some(PASSPHRASE.to_owned()),
        || gikli.clone_repository(SSH_URL, &target),
    );

    result.expect("clone with the passphrase");
    assert_cloned(&sandbox, &remote, &target);
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    assert_eq!(prompts[0].kind, PromptKind::Passphrase);
    assert!(prompts[0].text.contains("id_ed25519"), "{prompts:?}");
    let kept = format!(
        "{}{:?}{events:?}",
        text_of_files_in(&sandbox.config_folder()),
        gikli.operation_log(),
    );
    assert!(!kept.contains(PASSPHRASE), "{kept}");
}

#[test]
fn a_server_that_refuses_the_key_is_an_authentication_failure() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.ssh_stand_in(&asks_for_the_passphrase(&remote));
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_with_prompts(askpass!());

    let (result, _, _) = answering_prompts(
        &gikli,
        |_| Some("not the passphrase".to_owned()),
        || gikli.clone_repository(SSH_URL, &target),
    );

    assert_explained(result, explanation_of_failed_authentication());
}

/// A server gives up on a client that offers too many keys, in words of its own that ssh passes on.
#[test]
fn a_server_tired_of_being_offered_keys_is_an_authentication_failure() {
    let sandbox = Sandbox::new();
    sandbox.ssh_stand_in(
        "echo 'Received disconnect from 127.0.0.1 port 22:2: Too many authentication failures' >&2\n\
         echo 'Disconnected from 127.0.0.1 port 22' >&2\n\
         exit 255",
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(SSH_URL, &target);

    assert_explained(result, explanation_of_failed_authentication());
}

/// How ssh words the question about a host key it does not know.
const UNKNOWN_HOST_KEY_QUESTION: &str = "The authenticity of host 'gikli.test (127.0.0.1)' can't be established.\nED25519 key fingerprint is SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s.\nThis key is not known by any other names.\nAre you sure you want to continue connecting (yes/no/[fingerprint])? ";

/// What ssh does for a host it does not know: it asks, and goes on only with a `yes`.
fn asks_about_the_host_key(remote: &TestRepository) -> String {
    format!(
        "answer=$(\"$SSH_ASKPASS\" \"{}\")\n\
         if [ \"$answer\" != yes ]; then echo 'Host key verification failed.' >&2; exit 255; fi\n\
         {}",
        UNKNOWN_HOST_KEY_QUESTION,
        serves(remote)
    )
}

fn explanation_of_unknown_host_key() -> Explanation {
    Explanation {
        message: ExplainedMessage::HostKeyUnknown,
        next_step: NextStep::VerifyHostKey,
        actions: vec![],
    }
}

#[test]
fn the_user_decides_about_an_unknown_host_key_inside_gikli() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.ssh_stand_in(&asks_about_the_host_key(&remote));
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_with_prompts(askpass!());

    let (result, prompts, _) = answering_prompts(
        &gikli,
        |_| Some("yes".to_owned()),
        || gikli.clone_repository(SSH_URL, &target),
    );

    result.expect("clone from the host the user trusted");
    assert_cloned(&sandbox, &remote, &target);
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    assert_eq!(prompts[0].kind, PromptKind::HostKey);
    assert_eq!(prompts[0].text, UNKNOWN_HOST_KEY_QUESTION);
}

#[test]
fn an_unknown_host_key_the_user_does_not_accept_gets_a_dedicated_message() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.ssh_stand_in(&asks_about_the_host_key(&remote));
    let gikli = sandbox.gikli_with_prompts(askpass!());

    let target = sandbox.root().join("said-no");
    let (result, _, _) = answering_prompts(
        &gikli,
        |_| Some("no".to_owned()),
        || gikli.clone_repository(SSH_URL, &target),
    );
    assert_explained(result, explanation_of_unknown_host_key());

    let target = sandbox.root().join("declined");
    let (result, _, _) = answering_prompts(
        &gikli,
        |_| None,
        || gikli.clone_repository(SSH_URL, &target),
    );
    assert_explained(result, explanation_of_unknown_host_key());
}

#[test]
fn an_unknown_host_key_under_strict_checking_gets_the_same_message() {
    let sandbox = Sandbox::new();
    sandbox.ssh_stand_in(
        "echo 'No ED25519 host key is known for gikli.test and you have requested strict checking.' >&2\n\
         echo 'Host key verification failed.' >&2\n\
         exit 255",
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(SSH_URL, &target);

    assert_explained(result, explanation_of_unknown_host_key());
}

/// How ssh reports that a host answers with another key than the one it knows, as far as that is
/// the same for every key.
const CHANGED_HOST_KEY_REPORT: &str = "echo '@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@' >&2\n\
    echo '@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @' >&2\n\
    echo '@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@' >&2\n\
    echo 'IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!' >&2\n\
    echo 'Offending ED25519 key in /home/ada/.ssh/known_hosts:3' >&2";

#[test]
fn a_changed_host_key_gets_a_dedicated_message() {
    let sandbox = Sandbox::new();
    sandbox.ssh_stand_in(&format!(
        "{CHANGED_HOST_KEY_REPORT}\n\
         echo 'Host key for gikli.test has changed and you have requested strict checking.' >&2\n\
         echo 'Host key verification failed.' >&2\n\
         exit 255"
    ));
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_in(speaking(sandbox.host_environment(), "de"));

    let result = gikli.clone_repository(SSH_URL, &target);

    assert_explained(
        result,
        Explanation {
            message: ExplainedMessage::HostKeyChanged,
            next_step: NextStep::CheckChangedHostKey,
            actions: vec![],
        },
    );
}

/// Without strict checking, ssh warns about the key and goes on to authenticate with what is left.
/// The changed key is what the user needs to hear about.
#[test]
fn a_changed_host_key_comes_before_the_refusal_it_leads_to() {
    let sandbox = Sandbox::new();
    sandbox.ssh_stand_in(&format!("{CHANGED_HOST_KEY_REPORT}\n{REFUSED_BY_SSH}"));
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(SSH_URL, &target);

    let Err(Failure::Explained { explanation }) = result else {
        panic!("got {result:?}");
    };
    assert_eq!(explanation.message, ExplainedMessage::HostKeyChanged);
}

/// ssh says these things on lines of their own. A file that happens to be named after one of them
/// is not a failure of ssh.
#[test]
fn a_failure_that_only_mentions_what_ssh_says_is_not_taken_for_it() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let root = Repository {
        root: repository.toplevel_by_git(),
    };
    let gikli = sandbox.gikli();

    for name in [
        "Host key verification failed.",
        "REMOTE HOST IDENTIFICATION HAS CHANGED!",
        "git@host: Permission denied (publickey).",
        "Too many authentication failures",
        "could not read Username for x: terminal prompts disabled",
        "unable to get password from user",
        "The requested URL returned error: 403",
    ] {
        assert_eq!(
            gikli.add_files(&root, &[name.to_owned()]),
            Err(Failure::FilesNotAdded {
                folder: root.root.clone()
            }),
            "{name}"
        );
    }
}

/// A file the user cannot write says "Permission denied" too, with no authentication in sight.
#[test]
fn a_permission_denied_by_the_file_system_is_not_an_authentication_failure() {
    let sandbox = Sandbox::new();
    sandbox.ssh_stand_in("echo 'fatal: cannot open key.txt: Permission denied' >&2; exit 1");
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(SSH_URL, &target);

    assert_eq!(
        result,
        Err(Failure::CloneFailed {
            url: SSH_URL.to_owned(),
            folder: target.clone()
        })
    );
}

/// A hook can say what it likes, and what failed is the hook.
#[test]
fn a_hook_that_mentions_host_keys_is_a_failing_hook() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.hook(
        "post-checkout",
        "echo 'Host key verification failed.' >&2; exit 1",
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(&gikli_acceptance::file_url(remote.root()), &target);

    let Err(Failure::Explained { explanation }) = result else {
        panic!("got {result:?}");
    };
    assert!(
        matches!(explanation.message, ExplainedMessage::HookFailed { .. }),
        "{explanation:?}"
    );
}

/// What a security key does: ssh asks for a touch without waiting for an answer, and ends the
/// askpass program once it got it.
#[test]
fn a_prompt_closes_by_itself_when_what_asked_no_longer_needs_the_answer() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let touched = sandbox.root().join("touched");
    sandbox.ssh_stand_in(&format!(
        "\"$SSH_ASKPASS\" 'Confirm user presence for key ED25519-SK' &\n\
         asking=$!\n\
         while [ ! -e '{touched}' ]; do sleep 0.1; done\n\
         kill $asking\n\
         {}",
        serves(&remote),
        touched = as_git_writes_it(&touched),
    ));
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_with_prompts(askpass!());
    let events = gikli.subscribe();

    let result = std::thread::scope(|scope| {
        let cloning = scope.spawn(|| gikli.clone_repository(SSH_URL, &target));
        let prompt = next_event(&events, |event| match event {
            Event::PromptRaised(prompt) => Some(prompt.clone()),
            _ => None,
        });
        assert_eq!(prompt.kind, PromptKind::Other, "secret until known better");
        fs::write(&touched, "").expect("touch the key");
        next_event(&events, |event| {
            (*event == Event::PromptClosed { id: prompt.id }).then_some(())
        });
        assert_eq!(gikli.pending_prompts(), []);
        assert_eq!(
            gikli.answer_prompt(prompt.id, "too late"),
            Err(Failure::PromptGone { id: prompt.id })
        );
        cloning.join().expect("the clone ends")
    });

    result.expect("clone once the key is touched");
    assert_cloned(&sandbox, &remote, &target);
}

/// The first event `wanted` picks.
fn next_event<T>(events: &Receiver<Event>, wanted: impl Fn(&Event) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = events.recv_timeout(left).expect("the event comes");
        if let Some(picked) = wanted(&event) {
            return picked;
        }
    }
}

/// Another program on this machine can find the port, and has no business asking the user.
#[test]
fn a_program_without_the_token_cannot_ask_the_user_anything() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    sandbox.ssh_stand_in(&format!(
        "if GIKLI_ASKPASS_TOKEN=guess \"$SSH_ASKPASS\" 'Enter your bank PIN'; then exit 9; fi\n{}",
        serves(&remote)
    ));
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_with_prompts(askpass!());

    let (result, prompts, _) = answering_prompts(
        &gikli,
        |_| Some("1234".to_owned()),
        || gikli.clone_repository(SSH_URL, &target),
    );

    result.expect("the clone goes on");
    assert_cloned(&sandbox, &remote, &target);
    assert_eq!(prompts, []);
}

/// gikli has no commit command yet, so a hook signs the commit as git will when gikli commits. It
/// signs with the real `ssh-keygen`, the program git uses to sign with an SSH key.
#[test]
fn a_signing_prompt_reaches_the_user_and_signs_with_the_answer() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_commit(&sandbox);
    let key = as_git_writes_it(&sandbox.root().join("signing-key"));
    sandbox.hook(
        "post-checkout",
        &format!(
            "ssh-keygen -q -t ed25519 -N '{PASSPHRASE}' -C signing -f '{key}' || exit 1\n\
             git -c gpg.format=ssh -c user.signingKey='{key}' commit --quiet --allow-empty -S -m Signed"
        ),
    );
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli_with_prompts(askpass!());

    let (result, prompts, events) = answering_prompts(
        &gikli,
        |_| Some(PASSPHRASE.to_owned()),
        || gikli.clone_repository(&gikli_acceptance::file_url(remote.root()), &target),
    );

    result.expect("clone, and sign in the hook");
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    // Only the kind: newer OpenSSH names the key in this prompt, and older OpenSSH does not.
    assert_eq!(prompts[0].kind, PromptKind::Passphrase);
    let signed = sandbox
        .existing_repository(&target)
        .git(&["cat-file", "commit", "HEAD"]);
    assert!(signed.contains("BEGIN SSH SIGNATURE"), "{signed}");
    let kept = format!(
        "{}{:?}{events:?}",
        text_of_files_in(&sandbox.config_folder()),
        gikli.operation_log(),
    );
    assert!(!kept.contains(PASSPHRASE), "{kept}");
}

/// git inside WSL reaches gikli's askpass program, a Windows program, through WSL's interop. A clean
/// filter asks, as it runs in the same environment as a credential prompt of that git would.
#[test]
fn a_prompt_from_the_git_inside_wsl_reaches_the_user() {
    let Some(repository) = WslRepository::create() else {
        eprintln!("skipped: needs WSL with git");
        return;
    };
    repository.git(&[
        "config",
        "filter.ask.clean",
        "\"$GIT_ASKPASS\" 'Username for inside WSL: '",
    ]);
    repository.write(".gitattributes", "notes.txt filter=ask\n");
    repository.write("notes.txt", "draft\n");
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli_with_prompts(askpass!());
    let root = Repository {
        root: repository.root(),
    };

    let (result, prompts, _) = answering_prompts(
        &gikli,
        |_| Some(USERNAME.to_owned()),
        || gikli.add_files(&root, &["notes.txt".to_owned()]),
    );

    result.expect("add the file through the filter");
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    assert_eq!(prompts[0].kind, PromptKind::Username);
    assert_eq!(
        repository.git(&["show", ":notes.txt"]).trim_end(),
        USERNAME,
        "the answer reached the git inside WSL"
    );
}

/// The note of refused credentials has to reach a file gikli can read from inside WSL too. A required
/// clean filter plays the credential helper, as a remote on the Windows side is out of reach of WSL.
#[test]
fn refused_credentials_inside_wsl_are_noted_where_gikli_finds_them() {
    let Some(repository) = WslRepository::create() else {
        eprintln!("skipped: needs WSL with git");
        return;
    };
    repository.git(&[
        "config",
        "filter.refuse.clean",
        ": > \"$GIKLI_CREDENTIALS_REFUSED\"; exit 1",
    ]);
    repository.git(&["config", "filter.refuse.required", "true"]);
    repository.write(".gitattributes", "notes.txt filter=refuse\n");
    repository.write("notes.txt", "draft\n");
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let root = Repository {
        root: repository.root(),
    };

    let result = gikli.add_files(&root, &["notes.txt".to_owned()]);

    assert_explained(result, explanation_of_failed_authentication());
    assert!(
        repository
            .status_by_git()
            .contains(&"?? notes.txt".to_owned())
    );
}
