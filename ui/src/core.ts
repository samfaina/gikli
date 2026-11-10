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
  /** Whether the user's credential helpers answer for credentials and keep them. */
  useCredentialHelper: boolean;
  /** Whether a clone also clones the submodules of the repository, and theirs. */
  cloneSubmodulesRecursively: boolean;
  /** Whether the user can edit and save the local side of a diff. */
  diffLocalSideEditable: boolean;
  /** Whether moving on from the last difference of a file goes to the next file. */
  goToNextFileAfterLastChange: boolean;
  /** Whether the user is warned before committing in detached HEAD or during a rebase. */
  warnOnUnusualCommitState: boolean;
  /** Whether the commit message is emptied once the commit went through. */
  clearCommitMessageAfterCommit: boolean;
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

/** Changes the view shows together. */
export interface ChangeGroup {
  /** Ordered by path. */
  changes: Change[];
  /** The changes arranged as the user chose. */
  nodes: ChangeNode[];
}

/** A named group of uncommitted changes that belongs to gikli and that git does not know about. */
export interface Changelist {
  id: string;
  name: string;
  /** What the user wrote about it, if anything. */
  description: string | null;
  /** What will become the commit message when it is committed, if anything. */
  comment: string | null;
  /** Whether this is the Active changelist, the one new changes go into. */
  active: boolean;
  group: ChangeGroup;
}

/** Where the user moves changes to. */
export type MoveTarget =
  | { kind: "changelist"; id: string }
  /** Created after the others, in the same step as the move. */
  | { kind: "newChangelist"; name: string };

export interface ChangeSet {
  repository: Repository;
  /** Ordered by path. */
  changes: Change[];
  /** Exactly one is active. */
  changelists: Changelist[];
  /** The new files git does not track yet. */
  unversionedFiles: ChangeGroup;
  /** The files git ignores, when the user turned ignored files on. */
  ignoredFiles: ChangeGroup;
}

/** The lines from `start` up to, and not including, `end`, counted from 0. */
export interface LineRange {
  start: number;
  end: number;
}

/** Lines that differ between the two sides of a diff. A side without lines here shows where they would go. */
export interface Difference {
  head: LineRange;
  local: LineRange;
}

/** Unchanged lines the view hides: the same on both sides, and in the same number. */
export interface UnchangedRegion {
  head: LineRange;
  local: LineRange;
}

/** Why the local side of a diff cannot be edited. */
export type LocalReadOnly = "turnedOff" | "noLocalFile" | "notUtf8" | "symbolicLink";

/** The two versions of a text file and where they differ, with `\n` between lines. */
export interface TextDiff {
  /** null when HEAD does not have the file. */
  head: string | null;
  /** null when there is no file on disk. */
  local: string | null;
  /** Identifies what is on disk: {@link saveLocalFile} takes it back. */
  localVersion: string;
  localReadOnly: LocalReadOnly | null;
  /** From the top of the file down. */
  differences: Difference[];
  /** From the top of the file down. */
  collapsed: UnchangedRegion[];
}

/** What the diff view can show of a file: a text diff, or nothing of a binary file, a submodule or a nested Repository. */
export type DiffContent = ({ kind: "text" } & TextDiff) | { kind: "binary" } | { kind: "folder" };

/** A changed file, compared with its version in HEAD. */
export interface FileDiff {
  repository: Repository;
  /** Relative to the Repository root, with `/` between folders. */
  path: string;
  /** The path in HEAD it was moved or copied from, which the head side shows. */
  originalPath: string | null;
  /** The file before this one in the changes view. */
  previousFile: string | null;
  /** The file after this one in the changes view. */
  nextFile: string | null;
  /** The file the review moves on to from the last difference of this one, when it moves on. */
  afterLastDifference: string | null;
  content: DiffContent;
}

/** What went wrong, in a failure the core explains. */
export type ExplainedMessage =
  /** `output` is what the hook wrote. */
  | { kind: "hookFailed"; hook: string; output: string }
  /** `folder` is written as git writes it. */
  | { kind: "dubiousOwnership"; folder: string }
  | { kind: "indexLocked"; lock: string }
  | { kind: "longPaths" }
  | { kind: "authenticationFailed" }
  | { kind: "hostKeyUnknown" }
  | { kind: "hostKeyChanged" }
  | { kind: "serverUnreachable" }
  | { kind: "cloneTargetNotEmpty"; folder: string }
  /** `cause` is what kept the submodules from cloning, when the core knows it. */
  | { kind: "submodulesNotCloned"; folder: string; cause: ExplainedMessage | null };

/** What the user can do next, in a failure the core explains. */
export type NextStep =
  | "fixHook"
  | "trustFolder"
  | "removeIndexLock"
  | "waitForOtherGitOperation"
  | "enableLongPaths"
  | "checkCredentials"
  | "verifyHostKey"
  | "checkChangedHostKey"
  | "checkConnection"
  | "chooseEmptyFolder"
  | "useOrDeleteClone";

/** Something gikli can do about a failure, once the user has confirmed. */
export type FailureAction =
  | { action: "trustFolder"; repository: Repository; folder: string }
  | { action: "removeIndexLock"; repository: Repository };

/** A failure explained in plain words. git's raw output stays in the operation log. */
export interface Explanation {
  message: ExplainedMessage;
  nextStep: NextStep;
  /** What gikli can do about it, in the order to offer them. */
  actions: FailureAction[];
}

export type Failure =
  | { kind: "explained"; explanation: Explanation }
  /** The user cancelled the operation. `leftBehind` is what the run left in their way, when it left anything. */
  | { kind: "operationCancelled"; leftBehind: Explanation | null }
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
  /** A bare repository or the inside of a git directory: gikli opens working trees. */
  | { kind: "notAWorkingTree"; folder: string }
  | { kind: "projectNotOpened"; folder: string }
  | { kind: "filesNotAdded"; folder: string }
  | { kind: "filesNotRolledBack"; folder: string }
  | { kind: "rollbackListOutdated" }
  | { kind: "commitMessageEmpty" }
  /** Nothing the user selected is a change any more. */
  | { kind: "nothingToCommit" }
  | { kind: "commitFailed"; folder: string }
  | { kind: "commitInputNotWritten"; folder: string; message: string }
  | { kind: "commitMessagesUnreadable"; folder: string }
  /** The user has to say who they are: see {@link setCommitIdentity}. */
  | { kind: "commitIdentityMissing"; nameMissing: boolean; emailMissing: boolean }
  /** A name or email has to have some text. */
  | { kind: "commitIdentityEmpty" }
  | { kind: "commitIdentityNotSaved"; folder: string }
  | { kind: "indexLockNotRemoved"; lock: string; message: string }
  | { kind: "folderNotTrusted"; folder: string }
  | { kind: "changesUnreadable"; folder: string }
  | { kind: "diffUnreadable"; path: string }
  | { kind: "localFileUnreadable"; path: string; message: string }
  | { kind: "localSideNotEditable"; path: string }
  /** The file is no longer what the user saw when they started editing it, so it was not written. */
  | { kind: "localFileChanged"; path: string }
  | { kind: "localFileNotSaved"; path: string; message: string }
  | { kind: "changelistGone"; id: string }
  | { kind: "changelistNameEmpty" }
  | { kind: "changelistNameTaken"; name: string }
  /** Another Changelist has to become active first. */
  | { kind: "activeChangelistNotDeletable" }
  | { kind: "changelistsUnreadable"; file: string; message: string }
  | { kind: "activeChangelistMissing"; file: string }
  | { kind: "changelistsFromNewerGikli"; file: string; version: number }
  | { kind: "changelistsNotSaved"; file: string; message: string }
  | { kind: "changesNotWatched"; folder: string; message: string }
  | { kind: "cloneFailed"; url: string; folder: string }
  | { kind: "cancelledCloneNotRemoved"; folder: string; message: string }
  | { kind: "notificationGone"; id: number }
  | { kind: "notificationActionNotOffered"; id: number }
  | { kind: "promptGone"; id: number };

/** One git command gikli ran, with its raw output. A password in a URL shows as `***`. */
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
  /** Whether the user cancelled this run, which ended it. */
  cancelled: boolean;
}

/** What a notification says. */
export type NotificationKind =
  | { kind: "unregisteredRepository"; repository: Repository }
  | { kind: "invalidRepository"; repository: Repository }
  /** The user cancelled the clone while this hook ran: the clone is complete, and the hook did not finish. */
  | { kind: "hookNotFinished"; repository: Repository; hook: string };

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

/** The two forms of the URL of a repository. The form it was typed in stays as it was typed. */
export interface CloneUrlForms {
  https: string;
  ssh: string;
}

/** A git command that takes long enough to show its progress. */
export type LongOperation =
  | { kind: "clone"; url: string; folder: string }
  | { kind: "addFiles"; repository: Repository }
  | { kind: "rollbackFiles"; repository: Repository }
  | { kind: "commitFiles"; repository: Repository };

/** How far one phase of a long operation has come. */
export interface Progress {
  /** The phase as git names it, in the user's language. */
  phase: string;
  /** From 0 to 100. */
  percent: number;
}

/** A long operation that has started and not finished. */
export interface RunningOperation {
  id: number;
  operation: LongOperation;
  /** null until git reports any. */
  progress: Progress | null;
  /** The hook the operation waits on, as git names it, while one runs. */
  hook: string | null;
  /** Whether the user can cancel it. They cannot when git runs inside WSL. */
  cancellable: boolean;
}

/** What a prompt asks for. Only a username and the answer about a host key show as they are typed. */
export type PromptKind = "username" | "password" | "passphrase" | "hostKey" | "other";

/** Something git or ssh asks the user. The operation waits until it closes. */
export interface Prompt {
  id: number;
  kind: PromptKind;
  /** The question, as git or ssh words it. */
  text: string;
}

export type CoreEvent =
  | { event: "operationLogged"; payload: OperationLogEntry }
  | { event: "promptRaised"; payload: Prompt }
  /** The user answered it in some window, or what asked went away: every window stops showing it. */
  | { event: "promptClosed"; payload: { id: number } }
  /** Drop the entries whose id is below `before`. */
  | { event: "operationLogCleared"; payload: { before: number } }
  | { event: "notificationRaised"; payload: Notification }
  | { event: "notificationRemoved"; payload: { id: number } }
  | { event: "operationStarted"; payload: { id: number; operation: LongOperation; cancellable: boolean } }
  | { event: "operationProgress"; payload: { id: number; progress: Progress } }
  | { event: "operationHook"; payload: { id: number; hook: string | null } }
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

/** Leaves credentials to the user's credential helpers, or makes git ask every time. Rejects with a {@link Failure}. */
export function setUseCredentialHelper(useHelper: boolean): Promise<Settings> {
  return invoke("set_use_credential_helper", { useHelper });
}

/** Makes a clone also clone submodules, and theirs, or leave them out. Rejects with a {@link Failure}. */
export function setCloneSubmodulesRecursively(recursively: boolean): Promise<Settings> {
  return invoke("set_clone_submodules_recursively", { recursively });
}

/** Lets the user edit and save the local side of a diff, or turns that off. Rejects with a {@link Failure}. */
export function setDiffLocalSideEditable(editable: boolean): Promise<Settings> {
  return invoke("set_diff_local_side_editable", { editable });
}

/** Makes a commit in detached HEAD or during a rebase warn the user, or stops that. Rejects with a {@link Failure}. */
export function setWarnOnUnusualCommitState(warn: boolean): Promise<Settings> {
  return invoke("set_warn_on_unusual_commit_state", { warn });
}

/** Makes the commit message empty once a commit went through, or keeps it. Rejects with a {@link Failure}. */
export function setClearCommitMessageAfterCommit(clear: boolean): Promise<Settings> {
  return invoke("set_clear_commit_message_after_commit", { clear });
}

/** Makes moving on from the last difference of a file go to the next file, or stay. Rejects with a {@link Failure}. */
export function setGoToNextFileAfterLastChange(goToNextFile: boolean): Promise<Settings> {
  return invoke("set_go_to_next_file_after_last_change", { goToNextFile });
}

/** The prompts waiting for the user, oldest first. New ones arrive as `promptRaised` events. */
export function pendingPrompts(): Promise<Prompt[]> {
  return invoke("pending_prompts");
}

/** Gives what asked the prompt `id` the user's answer, which gikli does not keep. Rejects with a {@link Failure}. */
export function answerPrompt(id: number, answer: string): Promise<void> {
  return invoke("answer_prompt", { id, answer });
}

/** Tells what asked the prompt `id` that the user declined to answer. Rejects with a {@link Failure}. */
export function cancelPrompt(id: number): Promise<void> {
  return invoke("cancel_prompt", { id });
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

/** Takes one of the actions of an {@link Explanation}, after asking the user to confirm. Rejects with a {@link Failure}. */
export function takeFailureAction(action: FailureAction): Promise<void> {
  return invoke("take_failure_action", { action });
}

/** The long operations running, with their last progress, for a window that opens while they run. */
export function runningOperations(): Promise<RunningOperation[]> {
  return invoke("running_operations");
}

/**
 * Cancels the long operation `id`: git and every process it started end, and the operation finishes.
 * Nothing happens when it already ended.
 */
export function cancelOperation(id: number): Promise<void> {
  return invoke("cancel_operation", { id });
}

/** The HTTPS and SSH forms of `url`, when it has them: a folder on this machine, for one, has none. */
export function cloneUrlForms(url: string): Promise<CloneUrlForms | null> {
  return invoke("clone_url_forms", { url });
}

/**
 * Clones the repository at `url` into `folder` and opens it as a Project. Its progress arrives as
 * `operationStarted`, `operationProgress`, `operationHook` and `operationFinished` events. Rejects with a
 * {@link Failure}.
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

/** Reads the uncommitted changes of `repository`, sorted into its Changelists; calling it again refreshes them. Rejects with a {@link Failure}. */
export function readChanges(repository: Repository): Promise<ChangeSet> {
  return invoke("read_changes", { repository });
}

/** Makes the Changelist `id` the Active changelist of `repository`. Rejects with a {@link Failure}. */
export function setActiveChangelist(repository: Repository, id: string): Promise<ChangeSet> {
  return invoke("set_active_changelist", { repository, id });
}

/** Adds a Changelist called `name`, with an optional `description`, to `repository`. Rejects with a {@link Failure}. */
export function createChangelist(repository: Repository, name: string, description: string | null): Promise<ChangeSet> {
  return invoke("create_changelist", { repository, name, description });
}

/** Calls the Changelist `id` of `repository` `name` from here on. Rejects with a {@link Failure}. */
export function renameChangelist(repository: Repository, id: string, name: string): Promise<ChangeSet> {
  return invoke("rename_changelist", { repository, id, name });
}

/** Deletes the Changelist `id` of `repository`, whose changes go to the Active changelist. Rejects with a {@link Failure}. */
export function deleteChangelist(repository: Repository, id: string): Promise<ChangeSet> {
  return invoke("delete_changelist", { repository, id });
}

/** Gives the Changelist `id` of `repository` a comment, or takes it away when there is no text. Rejects with a {@link Failure}. */
export function setChangelistComment(repository: Repository, id: string, comment: string | null): Promise<ChangeSet> {
  return invoke("set_changelist_comment", { repository, id, comment });
}

/**
 * Moves the changes at `paths`, relative to the Repository root, to `target`, which becomes the Active
 * changelist if `makeActive`, all in one step. Rejects with a {@link Failure}.
 */
export function moveChanges(repository: Repository, paths: string[], target: MoveTarget, makeActive: boolean): Promise<ChangeSet> {
  return invoke("move_changes", { repository, paths, target, makeActive });
}

/** The diff of the file at `path`, relative to the Repository root, against HEAD. Rejects with a {@link Failure}. */
export function readDiff(repository: Repository, path: string): Promise<FileDiff> {
  return invoke("read_diff", { repository, path });
}

/**
 * Saves `text`, with `\n` between lines, as the local side of the diff of the file at `path`; the lines
 * keep the line endings they have in the file. `version` is the {@link TextDiff.localVersion} of the diff
 * the user edited. Rejects with a {@link Failure}.
 */
export function saveLocalFile(repository: Repository, path: string, text: string, version: string): Promise<FileDiff> {
  return invoke("save_local_file", { repository, path, text, version });
}

/** Adds `paths`, relative to the Repository root and taken literally, to the git index. Rejects with a {@link Failure}. */
export function addFiles(repository: Repository, paths: string[]): Promise<void> {
  return invoke("add_files", { repository, paths });
}

/** Adds `paths`, relative to the Repository root and taken literally, to the git index, even when git ignores them. Rejects with a {@link Failure}. */
export function forceAddFiles(repository: Repository, paths: string[]): Promise<void> {
  return invoke("force_add_files", { repository, paths });
}

/** What rolling back some changes would discard, for the user to confirm first. */
export interface RollbackPlan {
  /** The files whose changes would be discarded, and the new or ignored files it would write over, ordered by path. */
  files: Change[];
  /** Whether any is a file the rollback takes out of git, whose local copy the user can delete as well. */
  hasAddedFiles: boolean;
}

/** What rolling back the changes at `paths`, relative to the Repository root, would discard. It changes nothing. Rejects with a {@link Failure}. */
export function rollbackPlan(repository: Repository, paths: string[]): Promise<RollbackPlan> {
  return invoke("rollback_plan", { repository, paths });
}

/**
 * Discards the changes on `plan`, which the user confirmed. Added files leave git, and their local
 * copies go too if `deleteAddedFiles`. Rejects with a {@link Failure}, without discarding anything
 * when the plan no longer holds.
 */
export function rollbackFiles(repository: Repository, plan: RollbackPlan, deleteAddedFiles: boolean): Promise<ChangeSet> {
  return invoke("rollback_files", { repository, plan, deleteAddedFiles });
}

/**
 * Commits the changes at `paths`, relative to the Repository root, as they are in the working tree,
 * with `message` as written. Unversioned Files among them are added in the same step, and what is
 * staged at any other path stays staged. Rejects with a {@link Failure}.
 */
export function commitFiles(repository: Repository, paths: string[], message: string): Promise<ChangeSet> {
  return invoke("commit_files", { repository, paths, message });
}

/** Where a commit may not be meant to go. */
export type CommitWarning = "detachedHead" | "rebase";

/** The messages the user can reuse for a commit, newest first. Rejects with a {@link Failure}. */
export function previousCommitMessages(repository: Repository): Promise<string[]> {
  return invoke("previous_commit_messages", { repository });
}

/** What the message of a commit of the changes at `paths` starts as, if anything. Rejects with a {@link Failure}. */
export function prefilledCommitMessage(repository: Repository, paths: string[]): Promise<string | null> {
  return invoke("prefilled_commit_message", { repository, paths });
}

/** What the user should hear before committing in `repository`, if anything. Rejects with a {@link Failure}. */
export function commitWarning(repository: Repository): Promise<CommitWarning | null> {
  return invoke("commit_warning", { repository });
}

/** Saves the name and email git writes in commits, for every Repository if `everywhere`. A null value stays as it is. Rejects with a {@link Failure}. */
export function setCommitIdentity(repository: Repository, name: string | null, email: string | null, everywhere: boolean): Promise<void> {
  return invoke("set_commit_identity", { repository, name, email, everywhere });
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
