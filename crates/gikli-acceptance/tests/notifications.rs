//! Notifications: long-lived messages that carry actions. They stay until the user acts on them,
//! dismisses them or restarts gikli, so the user can act on them later.

use std::fs;
use std::sync::mpsc::Receiver;

use gikli_acceptance::Sandbox;
use gikli_core::{
    Event, Failure, Notification, NotificationAction, NotificationKind, NotificationOutcome,
    Repository,
};

/// The notifications raised so far.
fn raised(events: &Receiver<Event>) -> Vec<Notification> {
    events
        .try_iter()
        .filter_map(|event| match event {
            Event::NotificationRaised(notification) => Some(notification),
            _ => None,
        })
        .collect()
}

#[test]
fn a_nested_repository_that_is_not_registered_raises_a_notification_to_add_or_ignore_it() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let gikli = sandbox.gikli();
    let events = gikli.subscribe();

    let project = gikli.open_project(app.root()).expect("open project");

    let engine = Repository {
        root: engine.toplevel_by_git(),
    };
    let notifications = gikli.notifications();
    assert_eq!(notifications.len(), 1);
    let notification = &notifications[0];
    assert_eq!(notification.project, project.folder);
    assert_eq!(
        notification.kind,
        NotificationKind::UnregisteredRepository {
            repository: engine.clone()
        }
    );
    assert_eq!(
        notification.actions,
        vec![
            NotificationAction::AddRepository {
                repository: engine.clone()
            },
            NotificationAction::IgnoreRepository { repository: engine }
        ]
    );
    assert_eq!(raised(&events), notifications);
    assert_eq!(app.status_by_git(), vec!["?? libs/engine/"]);
}

#[test]
fn acting_on_add_later_registers_the_repository_and_removes_the_notification() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let gikli = sandbox.gikli();
    gikli.open_project(app.root()).expect("open project");
    gikli
        .read_changes(&Repository {
            root: app.toplevel_by_git(),
        })
        .expect("keep working before acting");
    let notification = gikli.notifications().remove(0);
    let events = gikli.subscribe();

    let outcome = gikli
        .act_on_notification(notification.id, &notification.actions[0])
        .expect("add the repository");

    let NotificationOutcome::ProjectChanged(project) = outcome;
    let engine_root = engine.toplevel_by_git();
    assert_eq!(
        project.repositories,
        vec![
            Repository {
                root: app.toplevel_by_git()
            },
            Repository {
                root: engine_root.clone()
            }
        ]
    );
    assert!(gikli.notifications().is_empty());
    assert!(events.try_iter().any(|event| event
        == Event::NotificationRemoved {
            id: notification.id
        }));
    let reopened = sandbox
        .gikli()
        .open_project(app.root())
        .expect("reopen project");
    assert_eq!(reopened.repositories, project.repositories);
    assert_eq!(engine_root, engine.root());
}

#[test]
fn a_registered_repository_that_is_gone_raises_a_notification_to_remove_it() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let engine = Repository {
        root: engine.toplevel_by_git(),
    };
    let first = sandbox.gikli();
    let project = first.open_project(app.root()).expect("open project");
    first
        .add_repository(&project.folder, &engine)
        .expect("add repository");
    fs::remove_dir_all(app.root().join("libs")).expect("delete repository folder");
    let gikli = sandbox.gikli();

    gikli.open_project(app.root()).expect("reopen project");

    let notification = gikli.notifications().remove(0);
    assert_eq!(
        notification.kind,
        NotificationKind::InvalidRepository {
            repository: engine.clone()
        }
    );
    assert_eq!(
        notification.actions,
        vec![NotificationAction::RemoveRepository {
            repository: engine.clone()
        }]
    );

    let NotificationOutcome::ProjectChanged(project) = gikli
        .act_on_notification(notification.id, &notification.actions[0])
        .expect("remove the repository");

    assert!(project.invalid_repositories.is_empty());
    assert!(gikli.notifications().is_empty());
    assert_eq!(
        project.repositories,
        vec![Repository {
            root: app.toplevel_by_git()
        }]
    );
    assert!(!engine.root.exists());
}

#[test]
fn a_dismissed_notification_is_gone_and_can_no_longer_be_acted_on() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let gikli = sandbox.gikli();
    gikli.open_project(app.root()).expect("open project");
    let notification = gikli.notifications().remove(0);
    let events = gikli.subscribe();

    gikli.dismiss_notification(notification.id);

    assert!(gikli.notifications().is_empty());
    assert!(events.try_iter().any(|event| event
        == Event::NotificationRemoved {
            id: notification.id
        }));
    assert_eq!(
        gikli.act_on_notification(notification.id, &notification.actions[0]),
        Err(Failure::NotificationGone {
            id: notification.id
        })
    );
    let reopened = sandbox
        .gikli()
        .open_project(app.root())
        .expect("reopen project");
    assert_eq!(
        reopened.unregistered_repositories,
        vec![Repository {
            root: engine.toplevel_by_git()
        }],
        "dismissing decides nothing"
    );
}

#[test]
fn a_notification_takes_only_the_actions_it_offers() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let other = sandbox.repository("app/docs");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");
    let engine = Repository {
        root: engine.toplevel_by_git(),
    };
    let notification = gikli
        .notifications()
        .into_iter()
        .find(|notification| {
            notification.kind
                == NotificationKind::UnregisteredRepository {
                    repository: engine.clone(),
                }
        })
        .expect("a notification about the engine");
    let other = Repository {
        root: other.toplevel_by_git(),
    };

    let result = gikli.act_on_notification(
        notification.id,
        &NotificationAction::AddRepository {
            repository: other.clone(),
        },
    );

    assert_eq!(
        result,
        Err(Failure::NotificationActionNotOffered {
            id: notification.id
        })
    );
    assert_eq!(gikli.notifications().len(), 2);
    let reopened = sandbox
        .gikli()
        .open_project(app.root())
        .expect("reopen project");
    assert_eq!(reopened.repositories, project.repositories);
    assert!(reopened.unregistered_repositories.contains(&other));
}

#[test]
fn reopening_a_project_does_not_raise_the_same_notification_twice() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    sandbox.repository("app/libs/engine");
    let gikli = sandbox.gikli();
    gikli.open_project(app.root()).expect("open project");
    let first = gikli.notifications();

    gikli.open_project(app.root()).expect("open project again");

    assert_eq!(gikli.notifications(), first);
}

#[test]
fn a_notification_goes_away_when_the_user_decides_from_the_project_view() {
    let sandbox = Sandbox::new();
    let app = sandbox.repository("app");
    let engine = sandbox.repository("app/libs/engine");
    let docs = sandbox.repository("app/docs");
    let gikli = sandbox.gikli();
    let project = gikli.open_project(app.root()).expect("open project");
    let docs = Repository {
        root: docs.toplevel_by_git(),
    };

    gikli
        .ignore_repository(&project.folder, &docs)
        .expect("ignore from the view");

    let kinds: Vec<_> = gikli
        .notifications()
        .into_iter()
        .map(|notification| notification.kind)
        .collect();
    assert_eq!(
        kinds,
        vec![NotificationKind::UnregisteredRepository {
            repository: Repository {
                root: engine.toplevel_by_git()
            }
        }]
    );
    assert_eq!(app.status_by_git(), vec!["?? docs/", "?? libs/engine/"]);
}
