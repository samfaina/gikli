//! Cloning a repository from a URL into a folder: its submodules, the SSH and HTTPS forms of its
//! URL, and how it fails. Its progress and its cancelling have test files of their own.

use std::fs;
use std::net::TcpListener;
use std::path::Path;

use gikli_acceptance::{Sandbox, TestRepository, as_git_writes_it, file_url};
use gikli_core::{ExplainedMessage, Explanation, Failure, Gikli, NextStep, Project, Repository};

/// A bare remote named `name`, holding a file named after it and, when given, a submodule.
fn remote_holding(
    sandbox: &Sandbox,
    name: &str,
    submodule: Option<(&str, &TestRepository)>,
) -> TestRepository {
    let remote = sandbox.bare_remote(&format!("{name}.git"));
    let work = sandbox.repository(name);
    work.write(&format!("{name}.txt"), "content\n");
    if let Some((path, submodule)) = submodule {
        let url = file_url(submodule.root());
        work.git(&["submodule", "add", "--quiet", &url, path]);
    }
    work.commit_all("First commit");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "origin", "main"]);
    remote
}

/// A bare remote with a submodule `middle`, which has a submodule `leaf`.
fn remote_with_nested_submodules(sandbox: &Sandbox) -> TestRepository {
    sandbox.allow_local_submodules();
    let leaf = remote_holding(sandbox, "leaf", None);
    let middle = remote_holding(sandbox, "middle", Some(("leaf", &leaf)));
    remote_holding(sandbox, "top", Some(("middle", &middle)))
}

fn assert_holds(folder: &Path, file: &str) {
    assert!(folder.join(file).is_file(), "{file} in {folder:?}");
}

#[test]
fn a_clone_holds_its_submodules_and_theirs_by_default() {
    let sandbox = Sandbox::new();
    let remote = remote_with_nested_submodules(&sandbox);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let project = gikli
        .clone_repository(&file_url(remote.root()), &target)
        .expect("clone the remote");

    assert_holds(&target, "top.txt");
    assert_holds(&target, "middle/middle.txt");
    assert_holds(&target, "middle/leaf/leaf.txt");
    let status = sandbox
        .existing_repository(&target)
        .git(&["submodule", "status", "--recursive"]);
    assert_eq!(status.lines().count(), 2, "{status}");
    assert!(
        status.lines().all(|line| line.starts_with(' ')),
        "git finds every submodule at the commit recorded: {status}"
    );
    assert_eq!(
        project.repositories,
        vec![Repository {
            root: sandbox.existing_repository(&target).toplevel_by_git()
        }]
    );
}

#[test]
fn with_the_setting_off_a_clone_leaves_its_submodules_out_and_the_setting_stays_off() {
    let sandbox = Sandbox::new();
    let remote = remote_with_nested_submodules(&sandbox);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();
    assert!(gikli.settings().clone_submodules_recursively);

    let changed = gikli
        .set_clone_submodules_recursively(false)
        .expect("change the setting");

    assert!(!changed.clone_submodules_recursively);
    assert!(
        !sandbox.gikli().settings().clone_submodules_recursively,
        "the setting survives a restart"
    );
    gikli
        .clone_repository(&file_url(remote.root()), &target)
        .expect("clone the remote");
    assert_holds(&target, "top.txt");
    assert!(!target.join("middle/middle.txt").exists());
    assert!(
        sandbox
            .existing_repository(&target)
            .git(&["submodule", "status"])
            .starts_with('-'),
        "git finds the submodule not cloned"
    );
}

/// The forms gikli offers for `url`, as (HTTPS, SSH), when it offers them.
fn forms_of(gikli: &Gikli, url: &str) -> Option<(String, String)> {
    gikli
        .clone_url_forms(url)
        .map(|forms| (forms.https, forms.ssh))
}

#[test]
fn a_url_comes_in_an_https_and_an_ssh_form_and_keeps_the_form_it_was_typed_in() {
    const HTTPS: &str = "https://example.com/owner/repo.git";
    const SSH: &str = "git@example.com:owner/repo.git";
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    for (url, https, ssh) in [
        (HTTPS, HTTPS, SSH),
        (SSH, HTTPS, SSH),
        (
            "example.com:owner/repo.git",
            HTTPS,
            "example.com:owner/repo.git",
        ),
        (
            "ada@example.com:owner/repo.git",
            HTTPS,
            "ada@example.com:owner/repo.git",
        ),
        (
            "ssh://git@example.com/owner/repo.git",
            HTTPS,
            "ssh://git@example.com/owner/repo.git",
        ),
        // A port belongs to the protocol it was typed for.
        (
            "ssh://git@example.com:2222/owner/repo.git",
            HTTPS,
            "ssh://git@example.com:2222/owner/repo.git",
        ),
        (
            "https://example.com:8443/owner/repo.git",
            "https://example.com:8443/owner/repo.git",
            SSH,
        ),
        // A server on plain HTTP may have no HTTPS to move to.
        (
            "http://example.com/owner/repo.git",
            "http://example.com/owner/repo.git",
            SSH,
        ),
        (
            "https://ada@example.com/owner/repo.git",
            "https://ada@example.com/owner/repo.git",
            SSH,
        ),
        (
            "git@example.com:/srv/repo.git",
            "https://example.com/srv/repo.git",
            "git@example.com:/srv/repo.git",
        ),
        (
            "git@[::1]:repo.git",
            "https://[::1]/repo.git",
            "git@[::1]:repo.git",
        ),
        (
            "https://[::1]:8443/repo.git",
            "https://[::1]:8443/repo.git",
            "git@[::1]:repo.git",
        ),
    ] {
        assert_eq!(
            forms_of(&gikli, url),
            Some((https.to_owned(), ssh.to_owned())),
            "{url}"
        );
    }
}

#[test]
fn a_url_that_is_neither_https_nor_ssh_has_no_forms() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    for url in [
        "",
        "file:///srv/repo.git",
        "git://example.com/repo.git",
        "/srv/repo.git",
        "./repo.git",
        "../repo:v1.git",
        r"C:\repo.git",
        "C:/repo.git",
        r"\server\share:repo.git",
        "https://example.com",
        "https://example.com/",
        "https://example.com:port/repo.git",
        "https://example.com/repo.git?token=1",
        "https://example.com/repo.git#main",
        "https:///repo.git",
        "git@example.com:",
        "@example.com:repo.git",
        "two words:repo.git",
        "-oProxyCommand=evil:repo.git",
    ] {
        assert_eq!(forms_of(&gikli, url), None, "{url:?}");
    }
}

#[test]
fn a_password_in_a_url_never_reaches_the_other_form() {
    const SSH: &str = "git@example.com:repo.git";
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    for url in [
        "https://ada:s3cret@example.com/repo.git",
        // One that reads as a port and holds a slash, and one that holds an at sign.
        "https://ada:1234/s3cret@example.com/repo.git",
        "https://ada:s3@cret@example.com/repo.git",
    ] {
        assert_eq!(
            forms_of(&gikli, url),
            Some((url.to_owned(), SSH.to_owned())),
            "{url}"
        );
    }
    let url = "ssh://ada:s3cret@example.com/repo.git";
    assert_eq!(
        forms_of(&gikli, url),
        Some(("https://example.com/repo.git".to_owned(), url.to_owned()))
    );
}

#[test]
fn the_ssh_form_of_a_url_clones_through_ssh() {
    let sandbox = Sandbox::new();
    let remote = remote_holding(&sandbox, "remote", None);
    let asked = sandbox.root().join("ssh-was-asked");
    sandbox.ssh_stand_in(&format!(
        "echo \"$@\" > '{}'\nexec git upload-pack '{}'",
        as_git_writes_it(&asked),
        as_git_writes_it(remote.root()),
    ));
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();
    let forms = gikli
        .clone_url_forms("https://gikli.test/owner/remote.git")
        .expect("forms of the URL");

    gikli
        .clone_repository(&forms.ssh, &target)
        .expect("clone through ssh");

    assert_holds(&target, "remote.txt");
    assert_eq!(
        sandbox
            .existing_repository(&target)
            .git(&["remote", "get-url", "origin"])
            .trim_end(),
        forms.ssh
    );
    let asked = fs::read_to_string(asked).expect("ssh was started");
    assert!(asked.contains("git@gikli.test"), "{asked}");
    assert!(asked.contains("owner/remote.git"), "{asked}");
}

fn assert_explained(result: Result<Project, Failure>, explanation: Explanation) {
    match result {
        Err(Failure::Explained { explanation: got }) => assert_eq!(got, explanation),
        other => panic!("got {other:?}"),
    }
}

fn explanation_of_unreachable_server() -> Explanation {
    Explanation {
        message: ExplainedMessage::ServerUnreachable,
        next_step: NextStep::CheckConnection,
        actions: vec![],
    }
}

fn explanation_of_a_folder_in_use(folder: &Path) -> Explanation {
    Explanation {
        message: ExplainedMessage::CloneTargetNotEmpty {
            folder: folder.to_path_buf(),
        },
        next_step: NextStep::ChooseEmptyFolder,
        actions: vec![],
    }
}

#[test]
fn a_folder_that_holds_something_is_left_alone_and_the_failure_says_so() {
    let sandbox = Sandbox::new();
    let remote = remote_holding(&sandbox, "remote", None);
    let target = sandbox.folder("clone");
    fs::write(target.join("keep.txt"), "mine\n").expect("write a file");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(&file_url(remote.root()), &target);

    assert_explained(result, explanation_of_a_folder_in_use(&target));
    assert_eq!(
        fs::read_to_string(target.join("keep.txt")).expect("the file is still there"),
        "mine\n"
    );
    assert!(!sandbox.is_inside_repository_by_git(&target));
}

#[test]
fn a_server_that_refuses_the_connection_is_explained() {
    let sandbox = Sandbox::new();
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("find a free port");
        listener.local_addr().expect("its address").port()
    };
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(&format!("http://127.0.0.1:{port}/repo.git"), &target);

    assert_explained(result, explanation_of_unreachable_server());
    assert!(!target.exists());
}

/// Passes on `line` as what the transport reports, then fails as ssh does.
fn ssh_that_reports(sandbox: &Sandbox, line: &str) {
    sandbox.ssh_stand_in(&format!("echo \"{line}\" >&2\nexit 255"));
}

#[test]
fn what_ssh_and_curl_say_about_a_server_they_cannot_reach_is_explained() {
    for line in [
        "ssh: Could not resolve hostname gikli.test: Name or service not known",
        "ssh: connect to host gikli.test port 22: Connection refused",
        "ssh: connect to host gikli.test port 22: Connection timed out",
        // The words of curl, after a sentence git translates.
        "fatal: Zugriff auf 'https://gikli.test/' nicht möglich: Could not resolve host: gikli.test",
        "fatal: Zugriff auf 'https://gikli.test/' nicht möglich: Failed to connect to gikli.test port 443 after 21 ms: Timed out",
    ] {
        let sandbox = Sandbox::new();
        ssh_that_reports(&sandbox, line);
        let target = sandbox.root().join("clone");

        let result = sandbox
            .gikli()
            .clone_repository("git@gikli.test:repo.git", &target);

        assert_explained(result, explanation_of_unreachable_server());
    }
}

#[test]
fn what_a_server_says_about_other_servers_does_not_read_as_unreachable() {
    let sandbox = Sandbox::new();
    ssh_that_reports(
        &sandbox,
        "remote: Could not resolve host: mirror.gikli.test",
    );
    let target = sandbox.root().join("clone");

    let result = sandbox
        .gikli()
        .clone_repository("git@gikli.test:repo.git", &target);

    assert!(
        matches!(result, Err(Failure::CloneFailed { .. })),
        "got {result:?}"
    );
}

/// A bare remote with a submodule `missing` whose remote is gone.
fn remote_with_a_submodule_that_is_gone(sandbox: &Sandbox) -> TestRepository {
    sandbox.allow_local_submodules();
    let gone = remote_holding(sandbox, "gone", None);
    let remote = remote_holding(sandbox, "top", Some(("missing", &gone)));
    fs::remove_dir_all(gone.root()).expect("remove the remote of the submodule");
    remote
}

#[test]
fn a_submodule_that_cannot_be_cloned_is_explained_and_the_clone_stays_in_its_folder() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_submodule_that_is_gone(&sandbox);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();

    let result = gikli.clone_repository(&file_url(remote.root()), &target);

    assert_explained(
        result,
        Explanation {
            message: ExplainedMessage::SubmodulesNotCloned {
                folder: target.clone(),
                cause: None,
            },
            next_step: NextStep::UseOrDeleteClone,
            actions: vec![],
        },
    );
    assert_holds(&target, "top.txt");
    let clone = sandbox.existing_repository(&target);
    assert_eq!(
        clone.git(&["rev-parse", "HEAD"]),
        remote.git(&["rev-parse", "main"])
    );
    assert!(
        clone.git(&["submodule", "status"]).starts_with('-'),
        "the submodule is not there"
    );
    gikli.open_project(&target).expect("the clone opens");
}

/// What keeps a submodule from cloning is about its own server, while the URL the user gave was
/// fine, so the explanation says it is about the submodules, and where the clone is.
#[test]
fn a_submodule_on_a_server_out_of_reach_is_explained_as_a_submodule_with_that_cause() {
    let sandbox = Sandbox::new();
    sandbox.allow_local_submodules();
    let submodule = remote_holding(&sandbox, "submodule", None);
    let remote = remote_holding(&sandbox, "top", Some(("elsewhere", &submodule)));
    let work = sandbox.existing_repository(&sandbox.root().join("top"));
    work.git(&[
        "config",
        "--file",
        ".gitmodules",
        "submodule.elsewhere.url",
        "git@gikli.test:submodule.git",
    ]);
    work.commit_all("Move the submodule");
    work.git(&["push", "--quiet", "origin", "main"]);
    ssh_that_reports(
        &sandbox,
        "ssh: Could not resolve hostname gikli.test: Name or service not known",
    );
    let target = sandbox.root().join("clone");

    let result = sandbox
        .gikli()
        .clone_repository(&file_url(remote.root()), &target);

    assert_explained(
        result,
        Explanation {
            message: ExplainedMessage::SubmodulesNotCloned {
                folder: target.clone(),
                cause: Some(Box::new(ExplainedMessage::ServerUnreachable)),
            },
            next_step: NextStep::UseOrDeleteClone,
            actions: vec![],
        },
    );
    assert_holds(&target, "top.txt");
}

#[test]
fn a_second_clone_into_what_a_failed_one_left_is_explained_as_a_folder_in_use() {
    let sandbox = Sandbox::new();
    let remote = remote_with_a_submodule_that_is_gone(&sandbox);
    let target = sandbox.root().join("clone");
    let gikli = sandbox.gikli();
    let url = file_url(remote.root());
    gikli
        .clone_repository(&url, &target)
        .expect_err("the submodule is gone");

    let result = gikli.clone_repository(&url, &target);

    assert_explained(result, explanation_of_a_folder_in_use(&target));
}
