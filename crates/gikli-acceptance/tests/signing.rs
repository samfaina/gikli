//! Signing commits as the user configured it: a signing program that fails gets a message of its
//! own, with what the program wrote, whichever format the user signs with.

use gikli_acceptance::{Sandbox, TestRepository, WslRepository, as_git_writes_it, opened, paths};
use gikli_core::{ExplainedMessage, Explanation, Failure, NextStep};

/// A repository with one commit that holds `a.txt`, and a change to it.
fn repository_with_a_change(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.commit_all("Initial commit");
    repository.write("a.txt", "a changed\n");
    repository
}

/// What gpg writes when it has no secret key for the key it is asked to sign with: its status lines
/// on the descriptor git gives it, which is the error output, and its own sentence.
const NO_SECRET_KEY: &str = "echo '[GNUPG:] KEY_CONSIDERED 0123456789ABCDEF 2' >&2\n\
                             echo 'gpg: signing failed: No secret key' >&2\n\
                             exit 2";

/// Makes every commit signed in the OpenPGP format, by `script` in place of gpg.
fn sign_with_openpgp_program(sandbox: &Sandbox, script: &str) {
    let program = sandbox.root().join("gpg-stand-in");
    sandbox.write_script(&program, script);
    sandbox.configure(&format!(
        "[commit]\n\tgpgSign = true\n[user]\n\tsigningKey = 0123456789ABCDEF\n[gpg]\n\tprogram = {}\n",
        as_git_writes_it(&program)
    ));
}

/// What the signing failure `result` explains shows of the run. It must be explained as one, with
/// its next step and no action.
fn explained_signing_output<T: std::fmt::Debug>(result: Result<T, Failure>) -> String {
    let Err(Failure::Explained {
        explanation:
            Explanation {
                message: ExplainedMessage::SigningFailed { output },
                next_step,
                actions,
            },
    }) = result
    else {
        panic!("the signing failure is explained: {result:?}");
    };
    assert_eq!(next_step, NextStep::CheckSigning);
    assert_eq!(actions, vec![]);
    output
}

#[test]
fn a_commit_the_openpgp_program_cannot_sign_shows_what_the_program_wrote() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change(&sandbox);
    sign_with_openpgp_program(&sandbox, NO_SECRET_KEY);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.commit_files(&root, &paths(&["a.txt"]), "Change a");

    let output = explained_signing_output(result);
    assert!(
        output.contains("gpg: signing failed: No secret key"),
        "{output}"
    );
    assert!(!output.contains("[GNUPG:]"), "{output}");
    assert_eq!(repository.commit_count(), "1\n");
    assert_eq!(repository.status_by_git(), vec![" M a.txt"]);
}

#[test]
fn a_commit_signed_with_an_ssh_key_that_cannot_be_loaded_shows_what_ssh_keygen_wrote() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change(&sandbox);
    sandbox.sign_commits_with_ssh_key(&sandbox.root().join("missing-signing-key"));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.commit_files(&root, &paths(&["a.txt"]), "Change a");

    let output = explained_signing_output(result);
    assert!(output.contains("missing-signing-key"), "{output}");
    assert_eq!(repository.commit_count(), "1\n");
}

/// The git inside WSL forwards only some of its trace, and the start of the signing program has to
/// be part of it.
#[test]
fn a_commit_inside_wsl_the_openpgp_program_cannot_sign_shows_what_the_program_wrote() {
    let Some(repository) = WslRepository::create() else {
        eprintln!("skipped: needs WSL with git");
        return;
    };
    let program = repository.linux_path_beside("gpg-stand-in");
    repository.write_script(&program, NO_SECRET_KEY);
    for (name, value) in [
        ("user.name", "Signer"),
        ("user.email", "signer@example.com"),
        ("commit.gpgSign", "true"),
        ("user.signingKey", "0123456789ABCDEF"),
        ("gpg.program", &program),
    ] {
        repository.git(&["config", name, value]);
    }
    repository.write("a.txt", "a\n");
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    let project = gikli
        .open_project(&repository.root())
        .expect("open project");

    let result = gikli.commit_files(&project.repositories[0], &paths(&["a.txt"]), "Add a");

    let output = explained_signing_output(result);
    assert!(
        output.contains("gpg: signing failed: No secret key"),
        "{output}"
    );
}

/// A pre-commit hook fails before git starts the signing program, and no hook of a commit that
/// runs after signing can fail it, so this is the case a user with signing configured meets.
#[test]
fn a_commit_whose_hook_fails_with_signing_configured_is_explained_as_the_failed_hook() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change(&sandbox);
    sign_with_openpgp_program(&sandbox, NO_SECRET_KEY);
    sandbox.hook("pre-commit", "echo 'Lint found 2 problems' >&2\nexit 1");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.commit_files(&root, &paths(&["a.txt"]), "Change a");

    let Err(Failure::Explained { explanation }) = result else {
        panic!("the hook's failure is explained: {result:?}");
    };
    assert_eq!(
        explanation.message,
        ExplainedMessage::HookFailed {
            hook: "pre-commit".to_owned(),
            output: "Lint found 2 problems\n".to_owned(),
        }
    );
    assert_eq!(explanation.next_step, NextStep::FixHook);
}

/// Makes every commit signed in the SSH format with a new key without a passphrase.
fn sign_with_a_new_ssh_key(sandbox: &Sandbox) {
    let key = sandbox.root().join("signing-key");
    sandbox.create_ssh_key(&key, "");
    sandbox.sign_commits_with_ssh_key(&key);
}

/// Locks the current branch of `repository`, as a git operation that holds it does, so a commit
/// fails after it was signed, in a way gikli does not explain.
fn lock_branch(repository: &TestRepository) {
    std::fs::write(repository.root().join(".git/refs/heads/main.lock"), "")
        .expect("lock the branch");
}

/// A signing program that signed, in a run that failed for another reason, says nothing about it.
#[test]
fn a_commit_that_was_signed_and_failed_after_keeps_its_own_failure() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change(&sandbox);
    sign_with_a_new_ssh_key(&sandbox);
    lock_branch(&repository);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.commit_files(&root, &paths(&["a.txt"]), "Change a");

    assert_eq!(
        result,
        Err(Failure::CommitFailed {
            folder: root.root.clone()
        })
    );
    let commit = gikli
        .operation_log()
        .into_iter()
        .find(|entry| entry.args.contains(&"commit".to_owned()))
        .expect("the commit is logged");
    assert!(commit.stderr.contains("main.lock"), "{}", commit.stderr);
}

#[test]
fn a_commit_without_signing_keeps_its_own_failure() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change(&sandbox);
    lock_branch(&repository);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let result = gikli.commit_files(&root, &paths(&["a.txt"]), "Change a");

    assert_eq!(
        result,
        Err(Failure::CommitFailed {
            folder: root.root.clone()
        })
    );
}

#[test]
fn a_commit_signed_with_an_ssh_key_is_committed_signed() {
    let sandbox = Sandbox::new();
    let repository = repository_with_a_change(&sandbox);
    sign_with_a_new_ssh_key(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    gikli
        .commit_files(&root, &paths(&["a.txt"]), "Change a")
        .expect("commit, signed");

    let signed = repository.git(&["cat-file", "commit", "HEAD"]);
    assert!(signed.contains("BEGIN SSH SIGNATURE"), "{signed}");
}
