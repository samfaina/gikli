//! A remote whose URL holds no repository: gikli names the remote and its URL, or the URL a clone
//! got, and points to it, instead of the general failure.

use std::fs;

use gikli_acceptance::{Sandbox, TestRepository, as_git_writes_it, file_url};
use gikli_core::{
    ExplainedMessage, Explanation, Failure, Gikli, NextStep, RefKind, RefLabel, Repository,
};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A working repository with one commit and the remote `origin`, a bare remote it pushed to.
fn work_with_origin(sandbox: &Sandbox) -> (TestRepository, TestRepository) {
    let remote = sandbox.bare_remote("remote.git");
    let work = sandbox.repository("work");
    work.write("notes.txt", "draft\n");
    work.commit_all("First commit");
    work.add_remote("origin", &remote);
    work.git(&["push", "--quiet", "--set-upstream", "origin", "main"]);
    (work, remote)
}

/// What the failure explainer says of `remote`, when there is one, whose `url` holds no repository.
fn no_repository(remote: Option<&str>, url: &str) -> Result<(), Failure> {
    Err(Failure::Explained {
        explanation: Explanation {
            message: ExplainedMessage::RemoteWithoutRepository {
                remote: remote.map(str::to_owned),
                url: url.to_owned(),
            },
            next_step: NextStep::CheckRemoteUrl,
            actions: vec![],
        },
    })
}

#[test]
fn a_fetch_from_a_folder_that_is_gone_or_empty_names_the_remote_and_its_url() {
    let sandbox = Sandbox::new();
    let (work, remote) = work_with_origin(&sandbox);
    let url = work
        .git(&["remote", "get-url", "origin"])
        .trim_end()
        .to_owned();
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    fs::remove_dir_all(remote.root()).expect("remove the remote");
    let gone = gikli.fetch_all(&root);
    fs::create_dir(remote.root()).expect("an empty folder in its place");
    let empty = gikli.fetch_all(&root);

    assert_eq!(gone, no_repository(Some("origin"), &url));
    assert_eq!(empty, no_repository(Some("origin"), &url));
}

#[test]
fn a_fetch_of_one_branch_from_a_file_url_that_holds_nothing_names_the_remote_and_its_url() {
    let sandbox = Sandbox::new();
    let (work, remote) = work_with_origin(&sandbox);
    let url = file_url(remote.root());
    work.git(&["remote", "set-url", "origin", &url]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let main = RefLabel {
        name: "main".to_owned(),
        kind: RefKind::Local,
    };

    fs::remove_dir_all(remote.root()).expect("remove the remote");
    let gone = gikli.fetch_branch(&root, &main);
    fs::create_dir(remote.root()).expect("an empty folder in its place");
    let empty = gikli.fetch_branch(&root, &main);

    assert_eq!(gone, no_repository(Some("origin"), &url));
    assert_eq!(empty, no_repository(Some("origin"), &url));
}

#[test]
fn a_fetch_of_every_remote_names_the_one_that_holds_no_repository() {
    let sandbox = Sandbox::new();
    let (work, _origin) = work_with_origin(&sandbox);
    let mirror = sandbox.bare_remote("mirror.git");
    work.add_remote("mirror", &mirror);
    work.git(&["push", "--quiet", "mirror", "main"]);
    let backup = sandbox.bare_remote("backup.git");
    work.add_remote("backup", &backup);
    let url = work
        .git(&["remote", "get-url", "mirror"])
        .trim_end()
        .to_owned();
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    fs::remove_dir_all(mirror.root()).expect("remove the mirror");
    let fetched = gikli.fetch_all(&root);

    assert_eq!(fetched, no_repository(Some("mirror"), &url));
}

#[test]
fn a_clone_from_a_path_or_file_url_that_holds_no_repository_names_the_url() {
    let sandbox = Sandbox::new();
    let gone = sandbox.root().join("gone");
    let empty = sandbox.folder("empty");
    let gikli = sandbox.gikli();
    let urls = [
        as_git_writes_it(&gone),
        file_url(&gone),
        as_git_writes_it(&empty),
        file_url(&empty),
    ];

    for (number, url) in urls.iter().enumerate() {
        let target = sandbox.root().join(format!("clone-{number}"));

        let cloned = gikli.clone_repository(url, &target).map(|_| ());

        assert_eq!(cloned, no_repository(None, url), "{url}");
        assert!(!target.exists(), "{url}");
    }
}

/// What GitHub's SSH server says of a repository it does not have, or that the user may not see,
/// as ssh passes it on before git's own sentence.
const GITHUB_OVER_SSH: &str = "echo 'ERROR: Repository not found.' >&2; exit 1";

const SSH_URL: &str = "git@gikli.test:ada/gone.git";

#[test]
fn what_github_says_over_ssh_of_a_repository_it_does_not_have_names_the_remote_or_the_url() {
    let sandbox = Sandbox::new();
    let (work, _remote) = work_with_origin(&sandbox);
    work.git(&["remote", "set-url", "origin", SSH_URL]);
    sandbox.ssh_stand_in(GITHUB_OVER_SSH);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let target = sandbox.root().join("clone");

    let fetched = gikli.fetch_all(&root);
    let cloned = gikli.clone_repository(SSH_URL, &target).map(|_| ());

    assert_eq!(fetched, no_repository(Some("origin"), SSH_URL));
    assert_eq!(cloned, no_repository(None, SSH_URL));
}

/// What GitHub answers over HTTPS for a repository it does not have, or that the user may not see.
const GITHUB_OVER_HTTPS: &str = "Repository not found.";

#[test]
fn what_github_says_over_https_of_a_repository_it_does_not_have_names_the_remote_or_the_url() {
    let sandbox = Sandbox::new();
    let (work, remote) = work_with_origin(&sandbox);
    let server = sandbox.http_remote_saying(&remote, "404 Not Found", GITHUB_OVER_HTTPS);
    work.git(&["remote", "set-url", "origin", &server.url()]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let target = sandbox.root().join("clone");

    let fetched = gikli.fetch_all(&root);
    let cloned = gikli.clone_repository(&server.url(), &target).map(|_| ());

    assert_eq!(fetched, no_repository(Some("origin"), &server.url()));
    assert_eq!(cloned, no_repository(None, &server.url()));
}

/// `git http-backend` answers a repository it does not have with a 404 and nothing more, and git
/// says so only in the user's language.
#[test]
fn a_server_that_answers_not_found_without_words_of_its_own_keeps_the_general_failure() {
    let sandbox = Sandbox::new();
    let (work, remote) = work_with_origin(&sandbox);
    let server = sandbox.http_remote_answering(&remote, "404 Not Found");
    work.git(&["remote", "set-url", "origin", &server.url()]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    let target = sandbox.root().join("clone");

    let fetched = gikli.fetch_all(&root);
    let cloned = gikli.clone_repository(&server.url(), &target).map(|_| ());

    assert_eq!(
        fetched,
        Err(Failure::FetchFailed {
            folder: root.root.clone()
        })
    );
    assert_eq!(
        cloned,
        Err(Failure::CloneFailed {
            url: server.url(),
            folder: target
        })
    );
}

/// A server can say it has no repository to someone it does not let in, and what gikli says is
/// that it did not let them in.
#[test]
fn a_server_that_refuses_access_and_says_it_has_no_repository_is_an_authentication_failure() {
    let sandbox = Sandbox::new();
    let (work, remote) = work_with_origin(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);
    for status in ["403 Forbidden", "407 Proxy Auth"] {
        let server = sandbox.http_remote_saying(&remote, status, GITHUB_OVER_HTTPS);
        work.git(&["remote", "set-url", "origin", &server.url()]);

        let fetched = gikli.fetch_all(&root);

        assert_eq!(
            fetched,
            Err(Failure::Explained {
                explanation: Explanation {
                    message: ExplainedMessage::AuthenticationFailed,
                    next_step: NextStep::CheckCredentials,
                    actions: vec![],
                }
            }),
            "{status}"
        );
    }
}

/// The stand-in for ssh hands the connection to this, as a server that has `remote`.
fn serves(remote: &TestRepository) -> String {
    format!("exec git upload-pack '{}'", as_git_writes_it(remote.root()))
}

#[test]
fn a_fetch_of_every_remote_on_servers_names_the_one_whose_server_has_no_repository() {
    let sandbox = Sandbox::new();
    let (work, remote) = work_with_origin(&sandbox);
    work.git(&["remote", "set-url", "origin", SSH_URL]);
    sandbox.ssh_stand_in(&serves(&remote));
    let server = sandbox.http_remote_saying(&remote, "404 Not Found", GITHUB_OVER_HTTPS);
    work.git(&["remote", "add", "mirror", &server.url()]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &work);

    let fetched = gikli.fetch_all(&root);

    assert_eq!(fetched, no_repository(Some("mirror"), &server.url()));
}

/// The clone got its files from its URL, so what a server says afterwards is about a submodule.
#[test]
fn a_submodule_whose_server_has_no_repository_does_not_read_as_the_clone_without_one() {
    let sandbox = Sandbox::new();
    sandbox.allow_local_submodules();
    let library = sandbox.bare_remote("library.git");
    let library_work = sandbox.repository("library");
    library_work.write("lib.txt", "lib\n");
    library_work.commit_all("Library");
    library_work.add_remote("origin", &library);
    library_work.git(&["push", "--quiet", "origin", "main"]);
    let server = sandbox.http_remote_saying(&library, "404 Not Found", GITHUB_OVER_HTTPS);
    let (work, remote) = work_with_origin(&sandbox);
    work.git(&[
        "submodule",
        "add",
        "--quiet",
        &file_url(library.root()),
        "lib",
    ]);
    work.git(&[
        "config",
        "-f",
        ".gitmodules",
        "submodule.lib.url",
        &server.url(),
    ]);
    work.commit_all("Add the library");
    work.git(&["push", "--quiet", "origin", "main"]);
    sandbox.ssh_stand_in(&serves(&remote));
    let target = sandbox.root().join("clone");

    let cloned = sandbox
        .gikli()
        .clone_repository(SSH_URL, &target)
        .map(|_| ());

    assert_eq!(
        cloned,
        Err(Failure::Explained {
            explanation: Explanation {
                message: ExplainedMessage::SubmodulesNotCloned {
                    folder: target.clone(),
                    cause: None,
                },
                next_step: NextStep::UseOrDeleteClone,
                actions: vec![],
            }
        })
    );
}

/// What GitLab says of a project it does not have, or that the user may not see, framed as it
/// frames it.
#[test]
fn what_gitlab_says_of_a_project_it_does_not_have_names_the_url() {
    let sandbox = Sandbox::new();
    sandbox.ssh_stand_in(
        "echo 'remote: ' >&2\n\
         echo \"remote: ERROR: The project you were looking for could not be found or you don't have permission to view it.\" >&2\n\
         echo 'remote: ' >&2\n\
         exit 1",
    );
    let target = sandbox.root().join("clone");

    let cloned = sandbox
        .gikli()
        .clone_repository(SSH_URL, &target)
        .map(|_| ());

    assert_eq!(cloned, no_repository(None, SSH_URL));
}
