// The core's application interface, as the UI sees it. Types mirror crates/gikli-core.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface GitVersion {
  major: number;
  minor: number;
  patch: number;
}

/** Where gikli found git: the path in the settings, PATH, or WSL (then `executable` is `wsl.exe`). */
export type GitSource = "configured" | "path" | "wsl";

export type GitCheck =
  | { status: "ready"; executable: string; source: GitSource; version: GitVersion }
  | { status: "tooOld"; executable: string; source: GitSource; version: GitVersion; minimum: GitVersion }
  | { status: "unrecognized"; executable: string; source: GitSource }
  | { status: "missing"; executable: string }
  | { status: "notFound"; minimum: GitVersion };

export interface Settings {
  /** The git executable to use; null when gikli looks for git itself. */
  gitPath: string | null;
  /** Whether the changes view lists the files git ignores. */
  showIgnoredFiles: boolean;
  /** How the changes view arranges the changes. */
  changeGrouping: ChangeGrouping;
  /** Whether the operation log wraps long lines instead of scrolling sideways. */
  operationLogSoftWrap: boolean;
}

/** How the changes view arranges the changes: a tree of folders, or one list. */
export type ChangeGrouping = "folder" | "flat";

/** A git feature that needs a newer git than the minimum. */
export type Capability = "rebaseUpdateRefs";

export interface CapabilityState {
  capability: Capability;
  /** The first git version that has it. */
  required: GitVersion;
  available: boolean;
}

export interface Repository {
  root: string;
}

export interface Project {
  folder: string;
  /** The registered Repositories that are valid repositories, ordered by root. */
  repositories: Repository[];
  /** The registered Repositories that are no longer valid repositories (moved, deleted or broken). */
  invalidRepositories: Repository[];
  /** Repositories found at or inside the folder that are neither registered nor ignored: offer to add or ignore each. */
  unregisteredRepositories: Repository[];
  /** Repositories found at or inside the folder that the user ignored; they can still be added. */
  ignoredRepositories: Repository[];
}

/** Where a Project shows: in the window that asked, in the window that already shows it, or in a new window. */
export type ProjectWindow =
  | { kind: "here" }
  | { kind: "open"; window: string }
  | { kind: "new"; window: string };

export type ChangeStatus =
  | "modified"
  | "added"
  | "deleted"
  | "renamed"
  | "copied"
  | "untracked"
  | "ignored"
  | "conflicted";

/** What each side of a conflict did to the file: "us" is the branch the operation runs on. */
export type ConflictKind =
  | "bothModified"
  | "bothAdded"
  | "bothDeleted"
  | "addedByUs"
  | "addedByThem"
  | "deletedByUs"
  | "deletedByThem";

/** What changed inside a submodule. */
export interface SubmoduleState {
  /** Another commit is checked out than the one the Repository records. */
  commitChanged: boolean;
  /** Files the submodule tracks have uncommitted changes. */
  hasTrackedChanges: boolean;
  /** The submodule has new files it does not track. */
  hasUntrackedFiles: boolean;
}

export interface Change {
  /** Relative to the Repository root, with `/` between folders. An ignored folder ends in `/`. */
  path: string;
  status: ChangeStatus;
  /** The path it was moved or copied from, for a renamed or copied file. */
  originalPath: string | null;
  conflict: ConflictKind | null;
  /** Set when the file is a submodule. */
  submodule: SubmoduleState | null;
}

/** One row of the changes view: a folder, when grouped by folder, or a changed file. */
export type ChangeNode =
  | { kind: "folder"; path: string; hasChanges: boolean; children: ChangeNode[] }
  | ({ kind: "file" } & Change);

export interface ChangeSet {
  repository: Repository;
  /** Ordered by path. */
  changes: Change[];
  /** The changes arranged as the user chose. */
  nodes: ChangeNode[];
}

export type Failure =
  | { kind: "gitNotFound" }
  | { kind: "gitTooOld"; version: GitVersion; minimum: GitVersion }
  | { kind: "gitUnrecognized" }
  | { kind: "gitFailedToStart"; message: string }
  | { kind: "settingsNotSaved"; message: string }
  | { kind: "projectNotSaved"; message: string }
  | { kind: "folderNotCreated"; folder: string; message: string }
  | { kind: "repositoryExists"; folder: string }
  | { kind: "repositoryNotCreated"; folder: string }
  | { kind: "repositoryOutsideProject"; folder: string }
  | { kind: "notARepositoryRoot"; folder: string }
  | { kind: "folderNotFound"; folder: string }
  | { kind: "notInRepository"; folder: string }
  | { kind: "changesUnreadable"; folder: string }
  | { kind: "changesNotWatched"; folder: string; message: string }
  | { kind: "cloneFailed"; url: string; folder: string }
  | { kind: "notificationGone"; id: number }
  | { kind: "notificationActionNotOffered"; id: number };

/** One git command gikli ran, with its raw output. */
export interface OperationLogEntry {
  /** Grows with each command: an entry read with {@link operationLog} may arrive again as an event. */
  id: number;
  /** The folder git ran in. */
  folder: string;
  /** The full command line after `git`. */
  args: string[];
  /** null when git was ended by a signal or could not start. */
  exitCode: number | null;
  stdout: string;
  stderr: string;
  /** Why git could not start, when it did not. */
  error: string | null;
}

/** What a notification says. */
export type NotificationKind =
  | { kind: "unregisteredRepository"; repository: Repository }
  | { kind: "invalidRepository"; repository: Repository };

/** An action a notification offers. */
export type NotificationAction =
  | { action: "addRepository"; repository: Repository }
  | { action: "ignoreRepository"; repository: Repository }
  | { action: "removeRepository"; repository: Repository };

/** A long-lived message about a Project, with the actions the user can take on it. */
export interface Notification {
  id: number;
  /** The {@link Project.folder} it is about: the window showing that Project shows it. */
  project: string;
  kind: NotificationKind;
  actions: NotificationAction[];
}

/** What acting on a notification changed. */
export type NotificationOutcome = { outcome: "projectChanged"; payload: Project };

/** A git command that takes long enough to show its progress. */
export type LongOperation = { kind: "clone"; url: string; folder: string };

/** How far one phase of a long operation has come. */
export interface Progress {
  /** The phase as git names it, in the user's language. */
  phase: string;
  /** From 0 to 100. */
  percent: number;
}

export type CoreEvent =
  | { event: "operationLogged"; payload: OperationLogEntry }
  | { event: "operationLogCleared" }
  | { event: "notificationRaised"; payload: Notification }
  | { event: "notificationRemoved"; payload: { id: number } }
  | { event: "operationStarted"; payload: { id: number; operation: LongOperation } }
  | { event: "operationProgress"; payload: { id: number; progress: Progress } }
  | { event: "operationFinished"; payload: { id: number } }
  | { event: "filesChanged"; payload: { repository: Repository } };

export function checkGit(): Promise<GitCheck> {
  return invoke("check_git");
}

/** Checks the git at `executable` without using it. */
export function testGit(executable: string): Promise<GitCheck> {
  return invoke("test_git", { executable });
}

/** Sets the path to git, or clears it with null, and looks for git again. Rejects with a {@link Failure}. */
export function setGitPath(path: string | null): Promise<GitCheck> {
  return invoke("set_git_path", { path });
}

/** The application settings. */
export function settings(): Promise<Settings> {
  return invoke("settings");
}

/** Turns the listing of ignored files on or off. Rejects with a {@link Failure}. */
export function setShowIgnoredFiles(show: boolean): Promise<Settings> {
  return invoke("set_show_ignored_files", { show });
}

/** Groups the changes view by folder or shows a flat list. Rejects with a {@link Failure}. */
export function setChangeGrouping(grouping: ChangeGrouping): Promise<Settings> {
  return invoke("set_change_grouping", { grouping });
}

/** Turns soft wrap of the operation log on or off. Rejects with a {@link Failure}. */
export function setOperationLogSoftWrap(softWrap: boolean): Promise<Settings> {
  return invoke("set_operation_log_soft_wrap", { softWrap });
}

/** Every git command gikli ran, oldest first. New ones arrive as `operationLogged` events. */
export function operationLog(): Promise<OperationLogEntry[]> {
  return invoke("operation_log");
}

/** Empties the operation log in every window, through the `operationLogCleared` event. */
export function clearOperationLog(): Promise<void> {
  return invoke("clear_operation_log");
}

/** The notifications raised and not yet acted on or dismissed, oldest first. */
export function notifications(): Promise<Notification[]> {
  return invoke("notifications");
}

/** Takes one of the actions of the notification `id`, which then goes away. Rejects with a {@link Failure}. */
export function actOnNotification(id: number, action: NotificationAction): Promise<NotificationOutcome> {
  return invoke("act_on_notification", { id, action });
}

/** Removes the notification `id` without acting on it. */
export function dismissNotification(id: number): Promise<void> {
  return invoke("dismiss_notification", { id });
}

/**
 * Clones the repository at `url` into `folder` and opens it as a Project. Its progress arrives as
 * `operationStarted`, `operationProgress` and `operationFinished` events. Rejects with a {@link Failure}.
 */
export function cloneRepository(url: string, folder: string): Promise<Project> {
  return invoke("clone_repository", { url, folder });
}

/** What the git that runs in `repository` can do beyond the minimum. Rejects with a {@link Failure}. */
export function capabilities(repository: Repository): Promise<CapabilityState[]> {
  return invoke("capabilities", { repository });
}

/** Rejects with a {@link Failure}. */
export function openProject(folder: string): Promise<Project> {
  return invoke("open_project", { folder });
}

/** Registers `repository`, at or inside the Project's folder, in the Project at `folder`. Rejects with a {@link Failure}. */
export function addRepository(folder: string, repository: Repository): Promise<Project> {
  return invoke("add_repository", { folder, repository });
}

/** Stops offering `repository` to the Project at `folder`. Rejects with a {@link Failure}. */
export function ignoreRepository(folder: string, repository: Repository): Promise<Project> {
  return invoke("ignore_repository", { folder, repository });
}

/** Unregisters `repository`, valid or not, from the Project at `folder`. Rejects with a {@link Failure}. */
export function removeRepository(folder: string, repository: Repository): Promise<Project> {
  return invoke("remove_repository", { folder, repository });
}

/** Creates a new Repository in `folder`, creating the folder if needed. Rejects with a {@link Failure}. */
export function createRepository(folder: string): Promise<Repository> {
  return invoke("create_repository", { folder });
}

/**
 * Shows the Project at `folder` (a {@link Project.folder}) in its own window: this window when it
 * shows no other Project, the window that already shows it, brought forward, or a new window.
 */
export function windowForProject(folder: string): Promise<ProjectWindow> {
  return invoke("window_for_project", { folder });
}

/** Reads the uncommitted changes of `repository`; calling it again refreshes them. Rejects with a {@link Failure}. */
export function readChanges(repository: Repository): Promise<ChangeSet> {
  return invoke("read_changes", { repository });
}

/**
 * Watches `repository` for changes made outside gikli; each burst of them arrives as a
 * `filesChanged` event. Rejects with a {@link Failure}.
 */
export function watchChanges(repository: Repository): Promise<void> {
  return invoke("watch_changes", { repository });
}

/** Stops watching `repository` for changes made outside gikli. */
export function unwatchChanges(repository: Repository): Promise<void> {
  return invoke("unwatch_changes", { repository });
}

/** The folder this window opens at startup: the Project it was created for, or for the first window the folder passed on the command line. */
export function startupFolder(): Promise<string | null> {
  return invoke("startup_folder");
}

export function onCoreEvent(handler: (event: CoreEvent) => void): Promise<UnlistenFn> {
  return listen<CoreEvent>("gikli://event", (message) => handler(message.payload));
}

export function isFailure(value: unknown): value is Failure {
  return typeof value === "object" && value !== null && "kind" in value;
}

export function formatVersion(version: GitVersion): string {
  return `${version.major}.${version.minor}.${version.patch}`;
}
