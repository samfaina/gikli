//! What helps the user write a commit message, and what warns them before committing somewhere
//! unexpected.

use gikli_acceptance::{Sandbox, TestRepository, as_git_writes_it};
use gikli_core::{ChangeSet, CommitWarning, Gikli, MoveTarget, Repository};

/// Opens the repository's folder as a Project and returns its Repository.
fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    gikli
        .open_project(repository.root())
        .expect("open project")
        .repositories[0]
        .clone()
}

/// A repository with one commit that holds `a.txt` and `b.txt`.
fn committed_repository(sandbox: &Sandbox) -> TestRepository {
    let repository = sandbox.repository("app");
    repository.write("a.txt", "a\n");
    repository.write("b.txt", "b\n");
    repository.commit_all("Initial commit");
    repository
}

#[test]
fn previous_messages_come_newest_first() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("a.txt", "changed\n");
    repository.commit_all("Second\n\nWith a body");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let messages = gikli
        .previous_commit_messages(&root)
        .expect("read messages");

    assert_eq!(messages, vec!["Second\n\nWith a body", "Initial commit"]);
}

/// Makes an empty commit with `message`, written by someone else.
fn commit_by_someone_else(repository: &TestRepository, message: &str) {
    repository.git(&[
        "-c",
        "user.name=Someone Else",
        "-c",
        "user.email=someone.else@example.com",
        "commit",
        "--quiet",
        "--allow-empty",
        "--message",
        message,
    ]);
}

#[test]
fn the_messages_of_other_people_are_not_offered() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    commit_by_someone_else(&repository, "Theirs");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let messages = gikli
        .previous_commit_messages(&root)
        .expect("read messages");

    assert_eq!(messages, vec!["Initial commit"]);
}

/// Commits a change to `a.txt` on a new branch `topic`, goes back to the branch it was on, and
/// returns the name of that branch.
fn commit_on_a_topic_branch(repository: &TestRepository) -> String {
    let main = repository.git(&["branch", "--show-current"]);
    let main = main.trim().to_owned();
    repository.git(&["switch", "--quiet", "-c", "topic"]);
    repository.write("a.txt", "on the topic\n");
    repository.commit_all("On the topic");
    repository.git(&["switch", "--quiet", &main]);
    main
}

#[test]
fn a_repository_without_commits_has_no_previous_messages() {
    let sandbox = Sandbox::new();
    let repository = sandbox.repository("app");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let messages = gikli
        .previous_commit_messages(&root)
        .expect("read messages");

    assert_eq!(messages, Vec::<String>::new());
}

#[test]
fn a_message_used_twice_is_offered_once_where_it_was_last_used() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.write("a.txt", "one\n");
    repository.commit_all("Fix the build");
    repository.write("a.txt", "two\n");
    repository.commit_all("Something else");
    repository.write("a.txt", "three\n");
    repository.commit_all("Fix the build");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let messages = gikli
        .previous_commit_messages(&root)
        .expect("read messages");

    assert_eq!(
        messages,
        vec!["Fix the build", "Something else", "Initial commit"]
    );
}

/// git wrote a merge's message, not the user.
#[test]
fn the_messages_of_merges_are_not_offered() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    commit_on_a_topic_branch(&repository);
    repository.write("b.txt", "on main\n");
    repository.commit_all("On main");
    repository.git(&["merge", "--quiet", "--no-edit", "topic"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let messages = gikli
        .previous_commit_messages(&root)
        .expect("read messages");

    assert_eq!(messages.len(), 3, "{messages:?}");
    assert!(messages.iter().all(|message| !message.starts_with("Merge")));
}

#[test]
fn messages_written_on_another_branch_are_offered_too() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    commit_on_a_topic_branch(&repository);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let messages = gikli
        .previous_commit_messages(&root)
        .expect("read messages");

    assert!(
        messages.contains(&"On the topic".to_owned()),
        "{messages:?}"
    );
}

#[test]
fn only_the_last_twenty_messages_are_offered() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    for number in 1..=25 {
        repository.git(&[
            "commit",
            "--quiet",
            "--allow-empty",
            "--message",
            &format!("Message {number}"),
        ]);
    }
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let messages = gikli
        .previous_commit_messages(&root)
        .expect("read messages");

    assert_eq!(messages.len(), 20);
    assert_eq!(messages[0], "Message 25");
    assert_eq!(messages[19], "Message 6");
}

/// A commit carries the encoding it was written in, and `i18n.logOutputEncoding` would change it.
#[test]
fn a_message_reads_the_same_whatever_encoding_git_is_set_to_show() {
    let sandbox = Sandbox::new();
    sandbox.configure("[i18n]\n\tlogOutputEncoding = ISO-8859-1\n");
    let repository = committed_repository(&sandbox);
    repository.write("a.txt", "changed\n");
    repository.commit_all("Añadir la opción");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let messages = gikli
        .previous_commit_messages(&root)
        .expect("read messages");

    assert_eq!(messages[0], "Añadir la opción");
}

/// A `.mailmap` shows a commit under another address, but the commit is still the user's.
#[test]
fn a_mailmap_does_not_hide_the_users_own_messages() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    let email = repository.git(&["config", "user.email"]);
    repository.write(
        ".mailmap",
        &format!("Canonical <canonical@example.com> <{}>\n", email.trim()),
    );
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let messages = gikli
        .previous_commit_messages(&root)
        .expect("read messages");

    assert_eq!(messages, vec!["Initial commit"]);
}

fn paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

/// A repository where `a.txt` is a change in the Changelist Feature and `b.txt` one in Changes, the
/// Active changelist, with gikli and the Repository.
fn repository_with_two_changelists(
    sandbox: &Sandbox,
) -> (TestRepository, Gikli, Repository, ChangeSet) {
    let repository = committed_repository(sandbox);
    repository.write("a.txt", "a changed\n");
    repository.write("b.txt", "b changed\n");
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);
    let changes = gikli
        .move_changes(
            &root,
            &paths(&["a.txt"]),
            &MoveTarget::NewChangelist {
                name: "Feature".to_owned(),
            },
            false,
        )
        .expect("move");
    (repository, gikli, root, changes)
}

/// Gives the Changelist called `name` the comment `comment`.
fn comment_on(gikli: &Gikli, root: &Repository, changes: &ChangeSet, name: &str, comment: &str) {
    let list = changes
        .changelists
        .iter()
        .find(|list| list.name == name)
        .expect("the Changelist exists");
    gikli
        .set_changelist_comment(root, &list.id, Some(comment))
        .expect("save the comment");
}

#[test]
fn the_message_is_prefilled_from_the_comment_of_the_changelist_being_committed() {
    let sandbox = Sandbox::new();
    let (_repository, gikli, root, changes) = repository_with_two_changelists(&sandbox);
    comment_on(
        &gikli,
        &root,
        &changes,
        "Feature",
        "Add the feature\n\nWith a body",
    );

    let message = gikli
        .prefilled_commit_message(&root, &paths(&["a.txt"]))
        .expect("read the message");

    assert_eq!(message.as_deref(), Some("Add the feature\n\nWith a body"));
}

#[test]
fn an_unversioned_file_beside_a_changelist_keeps_its_comment() {
    let sandbox = Sandbox::new();
    let (repository, gikli, root, changes) = repository_with_two_changelists(&sandbox);
    repository.write("new.txt", "new\n");
    comment_on(&gikli, &root, &changes, "Feature", "Add the feature");

    let message = gikli
        .prefilled_commit_message(&root, &paths(&["a.txt", "new.txt"]))
        .expect("read the message");

    assert_eq!(message.as_deref(), Some("Add the feature"));
}

#[test]
fn a_selection_across_changelists_takes_no_comment() {
    let sandbox = Sandbox::new();
    let (_repository, gikli, root, changes) = repository_with_two_changelists(&sandbox);
    comment_on(&gikli, &root, &changes, "Feature", "Add the feature");

    let message = gikli
        .prefilled_commit_message(&root, &paths(&["a.txt", "b.txt"]))
        .expect("read the message");

    assert_eq!(message, None);
}

/// Points `commit.template` at a new file outside the repository that holds `text`.
fn use_template(sandbox: &Sandbox, repository: &TestRepository, text: &str) {
    let template = sandbox.folder("templates").join("message template.txt");
    std::fs::write(&template, text).expect("write the template");
    repository.git(&["config", "commit.template", &as_git_writes_it(&template)]);
}

#[test]
fn without_a_comment_the_message_is_prefilled_from_the_template() {
    let sandbox = Sandbox::new();
    let (repository, gikli, root, _) = repository_with_two_changelists(&sandbox);
    use_template(&sandbox, &repository, "Ticket: \n\nWhy:\n");

    let message = gikli
        .prefilled_commit_message(&root, &paths(&["a.txt"]))
        .expect("read the message");

    assert_eq!(message.as_deref(), Some("Ticket: \n\nWhy:\n"));
}

#[test]
fn the_comment_comes_before_the_template() {
    let sandbox = Sandbox::new();
    let (repository, gikli, root, changes) = repository_with_two_changelists(&sandbox);
    use_template(&sandbox, &repository, "From the template\n");
    comment_on(&gikli, &root, &changes, "Feature", "From the comment");

    let message = gikli
        .prefilled_commit_message(&root, &paths(&["a.txt"]))
        .expect("read the message");

    assert_eq!(message.as_deref(), Some("From the comment"));
}

#[test]
fn a_selection_across_changelists_takes_the_template() {
    let sandbox = Sandbox::new();
    let (repository, gikli, root, changes) = repository_with_two_changelists(&sandbox);
    use_template(&sandbox, &repository, "From the template\n");
    comment_on(&gikli, &root, &changes, "Feature", "From the comment");

    let message = gikli
        .prefilled_commit_message(&root, &paths(&["a.txt", "b.txt"]))
        .expect("read the message");

    assert_eq!(message.as_deref(), Some("From the template\n"));
}

/// git reads a relative `commit.template` from the top of the working tree.
#[test]
fn a_relative_template_is_read_from_the_root_of_the_repository() {
    let sandbox = Sandbox::new();
    let (repository, gikli, root, _) = repository_with_two_changelists(&sandbox);
    repository.write("conventions/message.txt", "Relative\n");
    repository.git(&["config", "commit.template", "conventions/message.txt"]);

    let message = gikli
        .prefilled_commit_message(&root, &paths(&["a.txt"]))
        .expect("read the message");

    assert_eq!(message.as_deref(), Some("Relative\n"));
}

#[test]
fn a_template_that_cannot_be_read_prefills_nothing() {
    let sandbox = Sandbox::new();
    let (repository, gikli, root, _) = repository_with_two_changelists(&sandbox);
    repository.git(&["config", "commit.template", "no such file.txt"]);

    let message = gikli
        .prefilled_commit_message(&root, &paths(&["a.txt"]))
        .expect("read the message");

    assert_eq!(message, None);
}

#[test]
fn a_template_with_nothing_in_it_prefills_nothing() {
    let sandbox = Sandbox::new();
    let (repository, gikli, root, _) = repository_with_two_changelists(&sandbox);
    use_template(&sandbox, &repository, " \n\n");

    let message = gikli
        .prefilled_commit_message(&root, &paths(&["a.txt"]))
        .expect("read the message");

    assert_eq!(message, None);
}

#[test]
fn committing_on_a_branch_warns_of_nothing() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    assert_eq!(gikli.commit_warning(&root), Ok(None));
}

#[test]
fn committing_in_detached_head_warns() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["switch", "--quiet", "--detach"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    assert_eq!(
        gikli.commit_warning(&root),
        Ok(Some(CommitWarning::DetachedHead))
    );
}

/// A repository whose branch `topic` conflicts with its main branch at `a.txt`.
fn repository_with_a_conflicting_topic(sandbox: &Sandbox) -> (TestRepository, String) {
    let repository = committed_repository(sandbox);
    let main = commit_on_a_topic_branch(&repository);
    repository.write("a.txt", "on main\n");
    repository.commit_all("On main");
    (repository, main)
}

/// HEAD is detached in a rebase, and the rebase is what the user needs to hear about.
#[test]
fn committing_during_a_rebase_warns_of_the_rebase() {
    let sandbox = Sandbox::new();
    let (repository, main) = repository_with_a_conflicting_topic(&sandbox);
    repository.git(&["switch", "--quiet", "topic"]);
    assert!(!repository.git_succeeds(&["rebase", "--merge", &main]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    assert_eq!(gikli.commit_warning(&root), Ok(Some(CommitWarning::Rebase)));
}

#[test]
fn committing_during_a_rebase_by_applying_patches_warns_of_the_rebase() {
    let sandbox = Sandbox::new();
    let (repository, main) = repository_with_a_conflicting_topic(&sandbox);
    repository.git(&["switch", "--quiet", "topic"]);
    assert!(!repository.git_succeeds(&["rebase", "--apply", &main]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    assert_eq!(gikli.commit_warning(&root), Ok(Some(CommitWarning::Rebase)));
}

/// `git am` keeps its state in the same folder as a rebase by applying patches, on a branch.
#[test]
fn applying_mailed_patches_on_a_branch_is_not_a_rebase() {
    let sandbox = Sandbox::new();
    let (repository, main) = repository_with_a_conflicting_topic(&sandbox);
    let patch = repository.git(&["format-patch", "--stdout", "-1", "topic"]);
    let patch_file = sandbox.folder("patches").join("topic.patch");
    std::fs::write(&patch_file, patch).expect("write the patch");
    repository.git(&["switch", "--quiet", &main]);
    assert!(!repository.git_succeeds(&["am", &as_git_writes_it(&patch_file)]));
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    assert_eq!(gikli.commit_warning(&root), Ok(None));
}

/// Each linked worktree has a HEAD and a rebase of its own.
#[test]
fn a_rebase_in_a_linked_worktree_warns_there_and_nowhere_else() {
    let sandbox = Sandbox::new();
    let (repository, main) = repository_with_a_conflicting_topic(&sandbox);
    let worktree = repository.linked_worktree("linked");
    worktree.git(&["switch", "--quiet", "topic"]);
    assert!(!worktree.git_succeeds(&["rebase", "--merge", &main]));
    let gikli = sandbox.gikli();
    let in_worktree = opened(&gikli, &worktree);
    let in_main = opened(&gikli, &repository);

    assert_eq!(
        gikli.commit_warning(&in_worktree),
        Ok(Some(CommitWarning::Rebase))
    );
    assert_eq!(gikli.commit_warning(&in_main), Ok(None));
}

#[test]
fn the_warning_can_be_turned_off_and_stays_off() {
    let sandbox = Sandbox::new();
    let repository = committed_repository(&sandbox);
    repository.git(&["switch", "--quiet", "--detach"]);
    let gikli = sandbox.gikli();
    let root = opened(&gikli, &repository);

    let settings = gikli
        .set_warn_on_unusual_commit_state(false)
        .expect("save the setting");

    assert!(!settings.warn_on_unusual_commit_state);
    assert_eq!(gikli.commit_warning(&root), Ok(None));
    let restarted = sandbox.gikli();
    assert!(!restarted.settings().warn_on_unusual_commit_state);
}

#[test]
fn the_warning_is_on_by_default() {
    let sandbox = Sandbox::new();

    assert!(sandbox.gikli().settings().warn_on_unusual_commit_state);
}

#[test]
fn the_message_is_cleared_after_a_commit_unless_the_user_turns_that_off() {
    let sandbox = Sandbox::new();
    let gikli = sandbox.gikli();
    assert!(gikli.settings().clear_commit_message_after_commit);

    let settings = gikli
        .set_clear_commit_message_after_commit(false)
        .expect("save the setting");

    assert!(!settings.clear_commit_message_after_commit);
    assert!(!sandbox.gikli().settings().clear_commit_message_after_commit);
}
