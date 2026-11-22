//! Shortcuts for the steps many tests take through gikli's application interface before they get
//! to what they are about, and for reading what gikli answers.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;

use gikli_core::{
    ChangeSet, Changelist, Event, Gikli, IncomingOutgoing, LogRow, Notification, Project,
    PushTarget, Repository, Upstream,
};

use crate::TestRepository;

/// Opens the repository's folder as a Project and returns its Repository.
pub fn opened(gikli: &Gikli, repository: &TestRepository) -> Repository {
    opened_project(gikli, repository).1
}

/// Opens the repository's folder as a Project and returns it with its Repository.
pub fn opened_project(gikli: &Gikli, repository: &TestRepository) -> (Project, Repository) {
    let project = gikli.open_project(repository.root()).expect("open project");
    let opened = project.repositories[0].clone();
    (project, opened)
}

/// Opens the repository's folder as a Project and reads the changes of its Repository.
pub fn read_changes(gikli: &Gikli, repository: &TestRepository) -> ChangeSet {
    gikli
        .read_changes(&opened(gikli, repository))
        .expect("read changes")
}

/// The Repositories the events received so far said an operation left with files in conflict.
pub fn conflicts_left(events: &Receiver<Event>) -> Vec<Repository> {
    events
        .try_iter()
        .filter_map(|event| match event {
            Event::ConflictsLeft { repository } => Some(repository),
            _ => None,
        })
        .collect()
}

/// `paths`, relative to the Repository root, as the core names files in what it is asked to do.
pub fn paths(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

/// `paths`, relative to the Repository root, as the core names files in what it reports.
pub fn path_bufs(paths: &[&str]) -> Vec<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

/// The Changelist called `name` in `change_set`. Panics if there is none.
pub fn changelist<'a>(change_set: &'a ChangeSet, name: &str) -> &'a Changelist {
    change_set
        .changelists
        .iter()
        .find(|list| list.name == name)
        .expect("the Changelist exists")
}

/// The paths of the changes in the Changelist called `name`.
pub fn paths_in(change_set: &ChangeSet, name: &str) -> Vec<String> {
    changelist(change_set, name)
        .group
        .changes
        .iter()
        .map(|change| change.path.to_string_lossy().into_owned())
        .collect()
}

/// The subjects of the log's `rows`, in order.
pub fn log_subjects(rows: &[LogRow]) -> Vec<&str> {
    rows.iter().map(|row| row.subject.as_str()).collect()
}

/// The one notification gikli raised. Panics if it raised none, or more than one.
pub fn the_notification(gikli: &Gikli) -> Notification {
    let notifications = gikli.notifications();
    assert_eq!(
        notifications.len(),
        1,
        "one notification: {notifications:?}"
    );
    notifications[0].clone()
}

/// The branch `branch` of the remote `remote`, as a push targets it.
pub fn push_target(remote: &str, branch: &str) -> PushTarget {
    PushTarget {
        remote: remote.to_owned(),
        branch: branch.to_owned(),
    }
}

/// The upstream `name` of a branch, with `incoming` commits the branch does not have and
/// `outgoing` commits the upstream does not have.
pub fn upstream(name: &str, incoming: usize, outgoing: usize) -> Option<Upstream> {
    Some(Upstream {
        name: name.to_owned(),
        commits: Some(IncomingOutgoing { incoming, outgoing }),
    })
}
