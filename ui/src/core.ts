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
  /** Whether Commit and push opens the push dialog once the commit went through, instead of pushing at once. */
  showPushDialogForCommitAndPush: boolean;
  /** Whether Commit and push opens the push dialog only when the push goes to a protected branch. */
  showPushDialogOnlyForProtectedBranches: boolean;
  /** Whether a push of the current branch that its upstream rejects integrates the commits it lacks and pushes again without asking. */
  autoUpdateIfPushRejected: boolean;
  /** How such a push integrates them without asking: the method last remembered, merge until one is. */
  pushRejectedUpdateMethod: UpdateMethod;
  /** Whether the changes view shows the git index as Staged and Unstaged changes instead of Changelists. */
  stagingMode: boolean;
  /** Whether the log draws an edge that spans many rows whole, instead of as two arrows. */
  showLongEdges: boolean;
  /** How the log arranges the files a commit changed. */
  commitChangesGrouping: ChangeGrouping;
  /** The most revisions a file history lists, newest first. At least 1. */
  fileHistoryRowLimit: number;
  /** Whether the branch selector and the branches pane group branches such as `feature/login` under their directory. */
  groupBranchesByDirectory: boolean;
  /** Whether the branch selector shows Recent, the local branches checked out last. */
  showRecentBranches: boolean;
  /** Whether the branch selector shows the tags. */
  showTagsInBranchSelector: boolean;
  /** What the diff of a file in a stash compares the stashed file with. */
  stashDiffAgainst: StashDiffBase;
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
  /** The files in conflict, shown as a group of their own before the rest: they are in no Changelist and on neither side meanwhile. */
  conflicts: ChangeGroup;
  /** Exactly one is active. */
  changelists: Changelist[];
  /** The new files git does not track yet. */
  unversionedFiles: ChangeGroup;
  /** The files git ignores, when the user turned ignored files on. */
  ignoredFiles: ChangeGroup;
  /** The git index as Staged and Unstaged changes, in Staging mode. */
  staging: StagingArea | null;
}

/** The uncommitted changes as the git index divides them. A file with one version staged and another on disk is on both sides. */
export interface StagingArea {
  /** How the index differs from the last commit. */
  staged: ChangeGroup;
  /** How the working tree differs from the index, without the files git does not track yet. */
  unstaged: ChangeGroup;
  /** Whether a merge is in progress, which a commit concludes even with nothing staged. */
  merging: boolean;
}

/** One of the two comparisons of Staging mode: the index against HEAD, or the working tree against the index. */
export type StagingSide = "staged" | "unstaged";

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
export type LocalReadOnly = "turnedOff" | "noLocalFile" | "notUtf8" | "symbolicLink" | "index" | "revision" | "stash";

/** The two versions of a text file and where they differ, with `\n` between lines. {@link FileDiff.side} says what each side holds on a side of Staging mode. */
export interface TextDiff {
  /** The older version: null when HEAD does not have the file. */
  head: string | null;
  /** The newer version: null when there is no file on disk. */
  local: string | null;
  /** Identifies what is on disk: {@link saveLocalFile} takes it back. Empty when the local side is the index. */
  localVersion: string;
  localReadOnly: LocalReadOnly | null;
  /** From the top of the file down. */
  differences: Difference[];
  /** From the top of the file down. */
  collapsed: UnchangedRegion[];
}

/** What the diff view can show of a file: a text diff, or nothing of a binary file, a submodule or a nested Repository. */
export type DiffContent = ({ kind: "text" } & TextDiff) | { kind: "binary" } | { kind: "folder" };

/** A changed file, compared with its version in HEAD, or with the index on a side of Staging mode. */
export interface FileDiff {
  repository: Repository;
  /** Relative to the Repository root, with `/` between folders. */
  path: string;
  /** The path in HEAD it was moved or copied from, which the head side shows. */
  originalPath: string | null;
  /** Which comparison of Staging mode this is, or null when it compares HEAD with the file on disk. */
  side: StagingSide | null;
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
  /** `remote` is null for a clone, which has no remote yet. A password in `url` is hidden. */
  | { kind: "remoteWithoutRepository"; remote: string | null; url: string }
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
  | "checkRemoteUrl"
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
  | { kind: "filesNotUnstaged"; folder: string }
  | { kind: "filesNotRolledBack"; folder: string }
  | { kind: "rollbackListOutdated" }
  | { kind: "commitMessageEmpty" }
  /** Nothing the user selected is a change any more. */
  | { kind: "nothingToCommit" }
  /** The Repository has no commit yet. */
  | { kind: "nothingToAmend" }
  | { kind: "commitFailed"; folder: string }
  /** Undo commit takes only the last commit of the current branch, and `commit` is not that one, or HEAD is on no branch. */
  | { kind: "notLastCommit"; commit: string }
  /** `commit` is on `branch`, the remote branch of a protected branch, such as `origin/main`: shared history stays. */
  | { kind: "commitPushedToProtectedBranch"; commit: string; branch: string }
  /** git could not save a Backup, so the history was not rewritten. */
  | { kind: "backupNotSaved"; folder: string }
  /** git could not bring the branch back to its Backup. */
  | { kind: "backupNotRestored"; folder: string }
  /** Undo of a history rewrite found `branch`, or the detached HEAD when it is null, moved since the rewrite. */
  | { kind: "branchMovedSinceRewrite"; branch: string | null }
  /** git could not undo the last commit. */
  | { kind: "commitNotUndone"; folder: string }
  | { kind: "commitInputNotWritten"; folder: string; message: string }
  | { kind: "commitMessagesUnreadable"; folder: string }
  | { kind: "logUnreadable"; folder: string }
  /** The history changed since the page before: the log starts over from the first page. */
  | { kind: "logChanged" }
  /** The Repository has no remote to fetch from. */
  | { kind: "noRemoteToFetchFrom"; folder: string }
  /** The local `branch` has no upstream on a remote, so there is nothing to fetch for it alone. */
  | { kind: "noUpstream"; branch: string }
  /** git could not fetch: the operation log shows why. */
  | { kind: "fetchFailed"; folder: string }
  /** git could not read the branches or how far they are from their upstreams: the operation log shows why. */
  | { kind: "branchesUnreadable"; folder: string }
  /** The Repository has no remote called `remote`. */
  | { kind: "remoteNotFound"; remote: string }
  /** `name` is no name git takes for a branch. */
  | { kind: "branchNameInvalid"; name: string }
  /** Another branch is already called `name`, so no branch was created. */
  | { kind: "branchNameTaken"; name: string }
  /** git could not create the branch `name`: the operation log shows why. */
  | { kind: "branchNotCreated"; folder: string; name: string }
  /** git could not rename the branch `name`: the operation log shows why. */
  | { kind: "branchNotRenamed"; folder: string; name: string }
  /** git renamed the branch to `name` and could not unset its upstream: the operation log shows why. */
  | { kind: "upstreamNotUnset"; folder: string; name: string }
  /** HEAD is on the branch `name`, which git does not delete, so no branch was deleted. */
  | { kind: "currentBranchNotDeleted"; name: string }
  /** git could not delete the branch `name`: the operation log shows why. */
  | { kind: "branchNotDeleted"; folder: string; name: string }
  /** git could not delete the branch `branch` of `remote`: the operation log shows why. */
  | { kind: "remoteBranchNotDeleted"; folder: string; remote: string; branch: string }
  /** git could not check out a branch, a tag or a commit: the operation log shows why. */
  | { kind: "checkoutFailed"; folder: string }
  /**
   * Files block the checkout, and nothing changed. `localChanges` are files with uncommitted changes it would overwrite, which the user
   * can discard or save and restore around the checkout with {@link checkOutOverLocalChanges}; `untrackedFiles` are files git does not
   * track that it would overwrite or remove, which the user moves or commits. Both by path, relative to the Repository root.
   */
  | { kind: "checkoutBlocked"; folder: string; localChanges: string[]; untrackedFiles: string[] }
  /**
   * Checking out the remote branch `remoteBranch` would move the local branch `branch` to it and lose `commits` of its commits, and
   * nothing changed. The user drops them, with a Backup, or rebases them onto the remote branch, with {@link checkOutRemoteBranch}.
   */
  | { kind: "localCommitsWouldBeLost"; branch: string; remoteBranch: string; commits: number }
  /** HEAD is detached, so there is no current branch to push. */
  | { kind: "detachedHeadNotPushed" }
  /** The current branch has no commit yet, so there is nothing to push. */
  | { kind: "branchWithoutCommits"; branch: string }
  /** git could not push: the operation log shows why. */
  | { kind: "pushFailed"; folder: string }
  /** The upstream of the current branch, `branch` of `remote`, rejected the push as it has commits the branch lacks: the user chooses how to integrate them. */
  | { kind: "pushRejected"; folder: string; remote: string; branch: string }
  /** Integrating the upstream that rejected a push stopped on a conflict, so nothing was pushed. */
  | { kind: "updateStoppedOnConflicts"; folder: string }
  /** HEAD is detached, so there is no current branch to pull into. */
  | { kind: "detachedHeadNotPulled" }
  /** git could not pull: the operation log shows why. */
  | { kind: "pullFailed"; folder: string }
  /** The pull options `first` and `second`, as git names them, contradict each other, so the pull did not start. */
  | { kind: "pullOptionsIncompatible"; first: string; second: string }
  /** Another remote is already called `name`. */
  | { kind: "remoteNameTaken"; name: string }
  /** `name` is no name git takes for a remote. */
  | { kind: "remoteNameInvalid"; name: string }
  /** A remote needs a URL. */
  | { kind: "remoteUrlEmpty" }
  /** git could not define the remote `name`: the operation log shows why. */
  | { kind: "remoteNotAdded"; folder: string; name: string }
  /** git could not fetch the rest of the history from `remote`: the operation log shows why. */
  | { kind: "historyNotFetched"; folder: string; remote: string }
  /** No commit goes by `revision`: no hash, branch, tag or other name git knows. */
  | { kind: "revisionNotFound"; revision: string }
  | { kind: "fileVersionNotRestored"; path: string }
  /** The Repository cannot stash, for `reason`, so its stash actions are unavailable. */
  | { kind: "stashUnavailable"; folder: string; reason: StashUnavailable }
  /**
   * The local changes cannot be saved for `reason`, as they are saved in a stash, so an operation
   * that saves them first is unavailable.
   */
  | { kind: "localChangesNotSavable"; folder: string; reason: StashUnavailable }
  /** The Repository has no uncommitted change a stash would take. */
  | { kind: "nothingToStash"; folder: string }
  /** git could not stash: the operation log shows why. */
  | { kind: "notStashed"; folder: string }
  /** git could not list the stashes: the operation log shows why. */
  | { kind: "stashesUnreadable"; folder: string }
  /** No stash has the commit `hash`: it was dropped. */
  | { kind: "stashNotFound"; hash: string }
  /** git could not drop the stash: the operation log shows why. */
  | { kind: "stashNotDropped"; folder: string }
  /** The stashes are no longer the ones the user saw, so none was cleared. */
  | { kind: "stashesChanged"; folder: string }
  /** git could not clear the stashes: the operation log shows why. */
  | { kind: "stashesNotCleared"; folder: string }
  /** git could not apply all of the stash and left no file in conflict. What it applied stays, and the stash is kept. */
  | { kind: "stashNotApplied"; folder: string }
  /** The text of a search is no regular expression, as `message` explains. */
  | { kind: "searchPatternInvalid"; message: string }
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
  /** No merge, rebase, revert or cherry-pick is half done, so there is nothing to continue or abort. */
  | { kind: "noOperationInProgress" }
  /** The operation half done does not offer `action`. */
  | { kind: "operationActionNotOffered"; action: OperationAction }
  /** Files still have conflicts, so the operation in progress cannot go on. */
  | { kind: "conflictsNotResolved" }
  /** The file at `path` is not in conflict, so there is nothing to resolve. */
  | { kind: "fileNotInConflict"; path: string }
  /** git could not resolve the conflicts: the operation log shows why. */
  | { kind: "filesNotResolved"; folder: string }
  /** git could not take `action` on the `operation` half done: the operation log shows why. */
  | { kind: "operationActionFailed"; folder: string; operation: OperationKind; action: OperationAction }
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
  | { kind: "hookNotFinished"; repository: Repository; hook: string }
  /** gikli deleted these branches while HEAD was on the commit `head`, null on a branch with no commit yet. */
  | { kind: "branchesDeleted"; repository: Repository; branches: DeletedBranch[]; head: string | null }
  /** A rebase stopped, on a conflict or at a commit to edit, and waits for the user to continue or abort it. */
  | { kind: "rebaseStopped"; repository: Repository }
  /** The user closed the conflicts dialog while files were still in conflict: they stay so until resolved or aborted. */
  | { kind: "conflictsUnresolved"; repository: Repository }
  /** gikli rewrote the history with `rewrite` after saving `backup`, and left the branch on `head`, null for no commit. */
  | { kind: "historyRewritten"; repository: Repository; rewrite: Rewrite; backup: Backup; head: string | null }
  /** Popping `stash`, or creating a branch from it, left files in conflict, so gikli kept it until the user confirms dropping it. */
  | { kind: "stashKept"; repository: Repository; stash: Stash };

/** A history rewrite gikli saved a Backup before. */
export type Rewrite = "undoCommit" | "amend" | "dropLocalCommits";

/** The state of a branch gikli saved before a history rewrite, so the rewrite can be undone. */
export interface Backup {
  /** The ref that holds it, under `refs/gikli/backups/`. */
  reference: string;
  /** The local branch it saved, null for a detached HEAD. */
  branch: string | null;
  /** The full hash of the commit the branch was on before the rewrite. */
  commit: string;
}

/** A branch of a remote: `branch` is its name there. */
export interface RemoteBranch {
  remote: string;
  branch: string;
}

/** A local branch gikli deleted, with what it takes to bring it back. */
export interface DeletedBranch {
  name: string;
  /** The full hash of the commit it was on. */
  commit: string;
  /** The branch of a remote it followed, if any. */
  upstream: RemoteBranch | null;
  /** Whether it had commits that HEAD did not have when it went. */
  unmerged: boolean;
}

/** An action a notification offers. */
export type NotificationAction =
  | { action: "addRepository"; repository: Repository }
  | { action: "ignoreRepository"; repository: Repository }
  | { action: "removeRepository"; repository: Repository }
  /** Creates the deleted branches again, each on the commit it was on. */
  | { action: "restoreBranches" }
  /** Shows the commits of the deleted branch `branch` that HEAD did not have. */
  | { action: "showUnmergedCommits"; branch: string }
  /** Deletes `upstream`, which a deleted branch followed, on its remote too. */
  | { action: "deleteRemoteBranch"; upstream: RemoteBranch }
  /** Goes on with the stopped rebase. */
  | { action: "continueRebase" }
  /** Gives up the stopped rebase. */
  | { action: "abortRebase" }
  /** Opens the conflicts dialog again. */
  | { action: "resolveConflicts" }
  /** Gives up the operation in progress that left the conflicts. */
  | { action: "abortOperation" }
  /** Undoes the history rewrite: brings the branch back to the Backup saved before it. */
  | { action: "restoreBackup" }
  /** Drops the stash kept after it conflicted. */
  | { action: "dropStash" };

/** A long-lived message about a Project, with the actions the user can take on it. */
export interface Notification {
  id: number;
  /** The {@link Project.folder} it is about: the window showing that Project shows it. */
  project: string;
  kind: NotificationKind;
  actions: NotificationAction[];
}

/** What acting on a notification changed. */
export type NotificationOutcome =
  | { outcome: "projectChanged"; payload: Project }
  /** The branches of the Repository changed. */
  | { outcome: "branchesChanged"; payload: Repository }
  /** The commits a deleted branch took away, newest first. */
  | { outcome: "unmergedCommits"; payload: ListedCommit[] }
  /** HEAD of the Repository moved, as continuing or aborting a rebase moves it. */
  | { outcome: "headMoved"; payload: Repository }
  /** The conflicts dialog opens on the files of the Repository still in conflict. */
  | { outcome: "resolveConflicts"; payload: Repository }
  /** The stashes of the Repository changed. */
  | { outcome: "stashesChanged"; payload: Repository };

/** The two forms of the URL of a repository. The form it was typed in stays as it was typed. */
export interface CloneUrlForms {
  https: string;
  ssh: string;
}

/** A git command that takes long enough to show its progress. */
export type LongOperation =
  | { kind: "clone"; url: string; folder: string }
  | { kind: "addFiles"; repository: Repository }
  | { kind: "unstageFiles"; repository: Repository }
  | { kind: "rollbackFiles"; repository: Repository }
  | { kind: "commitFiles"; repository: Repository }
  | { kind: "fetchAll"; repository: Repository }
  | { kind: "fetchBranch"; repository: Repository; remote: string; branch: string }
  | { kind: "fetchRestOfHistory"; repository: Repository; remote: string }
  /** Fetching a remote the user just defined. */
  | { kind: "fetchRemote"; repository: Repository; remote: string }
  /** Pushing the current branch to the branch `branch` of `remote`. */
  | { kind: "push"; repository: Repository; remote: string; branch: string }
  /** Pulling the branch `branch` of `remote` into the current branch. */
  | { kind: "pull"; repository: Repository; remote: string; branch: string }
  /** Checking out `target`: a branch, a tag or a commit. */
  | { kind: "checkOut"; repository: Repository; target: string }
  /** Deleting the branch `branch` of `remote`, which a deleted branch followed. */
  | { kind: "deleteRemoteBranch"; repository: Repository; remote: string; branch: string }
  /** Taking `action` on the `operation` half done. */
  | { kind: "takeOperationAction"; repository: Repository; operation: OperationKind; action: OperationAction }
  /** Resolving files in conflict. */
  | { kind: "resolveConflicts"; repository: Repository }
  /** Applying a stash, or creating a branch from one. */
  | { kind: "applyStash"; repository: Repository };

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
  /** A notification still raised changed, as after an action that cannot be taken again: this is it now, with the same id. */
  | { event: "notificationChanged"; payload: Notification }
  | { event: "notificationRemoved"; payload: { id: number } }
  | { event: "operationStarted"; payload: { id: number; operation: LongOperation; cancellable: boolean } }
  | { event: "operationProgress"; payload: { id: number; progress: Progress } }
  | { event: "operationHook"; payload: { id: number; hook: string | null } }
  | { event: "operationFinished"; payload: { id: number } }
  | { event: "filesChanged"; payload: { repository: Repository } }
  /** An operation gikli ran left files of the Repository in conflict: the conflicts dialog opens. */
  | { event: "conflictsLeft"; payload: { repository: Repository } };

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

/** Arranges the files a commit changed as a tree of folders or a list. Rejects with a {@link Failure}. */
export function setCommitChangesGrouping(grouping: ChangeGrouping): Promise<Settings> {
  return invoke("set_commit_changes_grouping", { grouping });
}

/** Sets the most revisions a file history lists; below 1 it is 1. */
export function setFileHistoryRowLimit(limit: number): Promise<Settings> {
  return invoke("set_file_history_row_limit", { limit });
}

/** Draws long edges of the log whole, or as two arrows. Rejects with a {@link Failure}. */
export function setShowLongEdges(show: boolean): Promise<Settings> {
  return invoke("set_show_long_edges", { show });
}

/** Groups the branches the branch selector and the branches pane list under their directory, or lists them by their whole name. Rejects with a {@link Failure}. */
export function setGroupBranchesByDirectory(group: boolean): Promise<Settings> {
  return invoke("set_group_branches_by_directory", { group });
}

/** Shows or hides Recent in the branch selector. Rejects with a {@link Failure}. */
export function setShowRecentBranches(show: boolean): Promise<Settings> {
  return invoke("set_show_recent_branches", { show });
}

/** Shows or hides the tags in the branch selector. Rejects with a {@link Failure}. */
export function setShowTagsInBranchSelector(show: boolean): Promise<Settings> {
  return invoke("set_show_tags_in_branch_selector", { show });
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

/** Makes Commit and push open the push dialog once the commit went through, or push at once. Rejects with a {@link Failure}. */
export function setShowPushDialogForCommitAndPush(show: boolean): Promise<Settings> {
  return invoke("set_show_push_dialog_for_commit_and_push", { show });
}

/** Makes Commit and push open the push dialog only for a push to a protected branch, or for every push. Rejects with a {@link Failure}. */
export function setShowPushDialogOnlyForProtectedBranches(onlyProtected: boolean): Promise<Settings> {
  return invoke("set_show_push_dialog_only_for_protected_branches", { onlyProtected });
}

/** Makes a push its upstream rejects integrate the commits the branch lacks and push again without asking, or ask first. Rejects with a {@link Failure}. */
export function setAutoUpdateIfPushRejected(autoUpdate: boolean): Promise<Settings> {
  return invoke("set_auto_update_if_push_rejected", { autoUpdate });
}

/** Switches between Changelist mode and Staging mode. Rejects with a {@link Failure}. */
export function setStagingMode(enabled: boolean): Promise<Settings> {
  return invoke("set_staging_mode", { enabled });
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

/** The diff of the file at `path` on `side` of the index. Rejects with a {@link Failure}. */
export function readStagingDiff(repository: Repository, path: string, side: StagingSide): Promise<FileDiff> {
  return invoke("read_staging_diff", { repository, path, side });
}

/** Saves `text` as the file at `path`, as {@link saveLocalFile} does, and returns its diff against the index. Rejects with a {@link Failure}. */
export function saveUnstagedFile(repository: Repository, path: string, text: string, version: string): Promise<FileDiff> {
  return invoke("save_unstaged_file", { repository, path, text, version });
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

/** Stages every change, new files included. Rejects with a {@link Failure}. */
export function stageAll(repository: Repository): Promise<void> {
  return invoke("stage_all", { repository });
}

/** Stages every change to a file git tracks, leaving new files out. Rejects with a {@link Failure}. */
export function stageTracked(repository: Repository): Promise<void> {
  return invoke("stage_tracked", { repository });
}

/** Takes what is staged at `paths`, relative to the Repository root, out of the index. Rejects with a {@link Failure}. */
export function unstageFiles(repository: Repository, paths: string[]): Promise<void> {
  return invoke("unstage_files", { repository, paths });
}

/** What rolling back some changes would discard, for the user to confirm first. */
export interface RollbackPlan {
  /** The files whose changes would be discarded, and the new or ignored files it would write over, ordered by path. */
  files: Change[];
  /** Whether any is a file the rollback takes out of git, whose local copy the user can delete as well. */
  hasAddedFiles: boolean;
  /** The side of the index the plan is about in Staging mode, or null for changes in Changelists. */
  side: StagingSide | null;
  /** The files on both sides of the index, whose other side the rollback keeps. */
  onBothSides: string[];
}

/** What rolling back the changes at `paths`, relative to the Repository root, would discard. It changes nothing. Rejects with a {@link Failure}. */
export function rollbackPlan(repository: Repository, paths: string[]): Promise<RollbackPlan> {
  return invoke("rollback_plan", { repository, paths });
}

/**
 * What rolling back the changes at `paths`, relative to the Repository root, on `side` of the index
 * would discard. It changes nothing. Rejects with a {@link Failure}.
 */
export function stagingRollbackPlan(repository: Repository, paths: string[], side: StagingSide): Promise<RollbackPlan> {
  return invoke("staging_rollback_plan", { repository, paths, side });
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

/**
 * Replaces the last commit with one that also holds the changes at `paths`, as they are in the
 * working tree, and has `message` as written. It saves a Backup first, and a notification about
 * the Project at `project` offers to undo it. Rejects with a {@link Failure}.
 */
export function amendLastCommit(project: string, repository: Repository, paths: string[], message: string): Promise<ChangeSet> {
  return invoke("amend_last_commit", { project, repository, paths, message });
}

/** Commits what is staged, with `message` as written. Rejects with a {@link Failure}. */
export function commitStaged(repository: Repository, message: string): Promise<ChangeSet> {
  return invoke("commit_staged", { repository, message });
}

/**
 * Replaces the last commit with one that also holds what is staged, and has `message` as written. It
 * saves a Backup first, and a notification about the Project at `project` offers to undo it. Rejects
 * with a {@link Failure}.
 */
export function amendWithStaged(project: string, repository: Repository, message: string): Promise<ChangeSet> {
  return invoke("amend_with_staged", { project, repository, message });
}

/**
 * Undoes `commit`, the last commit of the current branch: its changes come back uncommitted, in the
 * Active changelist. It saves a Backup first, and a notification about the Project at `project`
 * offers to undo it. Rejects with a {@link Failure}.
 */
export function undoCommit(project: string, repository: Repository, commit: string): Promise<ChangeSet> {
  return invoke("undo_commit", { project, repository, commit });
}

/** A line of the graph, from a column at its start to a column at its end. */
export interface GraphLine {
  from: number;
  to: number;
}

/** The end of an edge too long to draw whole, which leads to the commit at its other end. */
export interface GraphArrow {
  /** The column the arrow ends in, at the top or the bottom of the row. */
  column: number;
  /** The hash of the commit at the other end of the edge. */
  target: string;
}

/** How one row of the graph is drawn. */
export interface GraphRow {
  /** The column of the commit's node. */
  column: number;
  /** The lines from the top of the row to the height of the node. */
  above: GraphLine[];
  /** The lines from the height of the node to the bottom of the row. */
  below: GraphLine[];
  /** Arrows from the top of the row to the node, each leading to a child far above. */
  arrowsUp: GraphArrow[];
  /** Arrows from the node to the bottom of the row, each leading to a parent far below. */
  arrowsDown: GraphArrow[];
}

/** What a reference on a commit names. Each kind shows in its own color. */
export type RefKind = "head" | "local" | "remote" | "tag";

/** A reference shown on the commit it points at. */
export interface RefLabel {
  /** Its name as the user knows it: `HEAD`, `main`, `origin/main`, `v1`. */
  name: string;
  kind: RefKind;
}

/** One commit of the log. */
export interface LogRow {
  hash: string;
  /** Its parents, the first parent first. The left arrow goes to the first. */
  parents: string[];
  /** Its children, the one on its line of the graph first. The right arrow goes to the first. */
  children: string[];
  subject: string;
  author: string;
  /** When it was committed, in seconds since the epoch. */
  committedAt: number;
  graph: GraphRow;
  /** The references that point at it, in the order they show. */
  labels: RefLabel[];
  /** Whether HEAD reaches it. */
  onCurrentBranch: boolean;
  /** Whether the user wrote it. */
  own: boolean;
  /** Whether its author and its committer are not the same. */
  committerDiffers: boolean;
  /** Whether the history is cut at it, as in a shallow clone: the Repository does not hold its parents. */
  historyCut: boolean;
}

/** Where a page of the log starts. Only the core reads it. */
export type LogCursor = unknown;

/** One page of the log, newest commit first. */
export interface LogPage {
  repository: Repository;
  rows: LogRow[];
  /** Where the next page starts, or null on the last page. */
  next: LogCursor | null;
  /** Whether the history of the Repository is cut anywhere, as in a shallow clone, so the log offers to fetch the rest. */
  historyCut: boolean;
}

/** A search of the log for the commits whose message, author or hash match a text. */
export interface TextSearch {
  text: string;
  /** Whether the text is a regular expression. Otherwise a message or author holds it, or a hash starts with it. */
  regex: boolean;
  /** Whether a match must have the same case as the text. */
  matchCase: boolean;
}

/** What the log shows of the history. */
export interface LogFilter {
  /** The branches whose commits the log shows; every branch, remote branch, tag and HEAD when empty. */
  branches: RefLabel[];
  /** The text the commits shown match, if any. A search shows them without their graph. */
  text: TextSearch | null;
}

/**
 * A page of the log, at most `count` of the commits `filter` shows, after `after` or from the newest
 * commit. A later page keeps to the filter of the first. Rejects with a {@link Failure}.
 */
export function readLog(
  repository: Repository,
  filter: LogFilter,
  after: LogCursor | null,
  count: number,
): Promise<LogPage> {
  return invoke("read_log", { repository, filter, after, count });
}

/**
 * Fetches the rest of the cut history of `repository` from its remote, every branch of the remote
 * included, as a long operation. Rejects with a {@link Failure}.
 */
export function fetchRestOfHistory(repository: Repository): Promise<void> {
  return invoke("fetch_rest_of_history", { repository });
}

/**
 * Fetches every remote of `repository`, pruning the remote branches gone from them, as a long
 * operation. Rejects with a {@link Failure}.
 */
export function fetchAll(repository: Repository): Promise<void> {
  return invoke("fetch_all", { repository });
}

/**
 * Fetches `branch` of `repository` alone: a local branch from its upstream, a remote branch from its
 * remote, as a long operation. Rejects with a {@link Failure}.
 */
export function fetchBranch(repository: Repository, branch: RefLabel): Promise<void> {
  return invoke("fetch_branch", { repository, branch });
}

/** How far a local branch is from its upstream. */
export interface IncomingOutgoing {
  /** The commits of the upstream the branch does not have. */
  incoming: number;
  /** The commits of the branch its upstream does not have. */
  outgoing: number;
}

/** The branch a local branch is compared with. */
export interface Upstream {
  /** Its name as the user knows it, such as `origin/main`. */
  name: string;
  /** null when it is gone, as when it was deleted on the remote and a fetch pruned it. */
  commits: IncomingOutgoing | null;
}

/** A local branch, with its upstream if it has one. */
export interface LocalBranch {
  name: string;
  upstream: Upstream | null;
}

/** What HEAD is on, as the status area shows it. */
export type CurrentBranch =
  | { kind: "branch"; name: string; upstream: Upstream | null }
  | { kind: "detached"; hash: string };

/** The local branches of `repository`, by name, each with how far it is from its upstream. Rejects with a {@link Failure}. */
export function localBranches(repository: Repository): Promise<LocalBranch[]> {
  return invoke("local_branches", { repository });
}

/** A branch or a tag in the branch selector and the branches pane. */
export interface Branch {
  /** Its whole name, such as `feature/login` or `origin/main`, and its kind. */
  label: RefLabel;
  /** The upstream of a local branch, with its incoming and outgoing commits; null for one without, and for remote branches and tags. */
  upstream: Upstream | null;
}

/** One entry of a group of branches: a directory, when they are grouped by directory, or a branch. */
export type BranchNode =
  | { kind: "directory"; path: string; children: BranchNode[] }
  | ({ kind: "branch" } & Branch);

/** The branches of a Repository, in the groups the branch selector shows. */
export interface Branches {
  /** The Repository the branches are of. */
  repository: Repository;
  /** What HEAD is on. */
  current: CurrentBranch;
  /** The local branches checked out last, newest first, other than the current one; empty when hidden. */
  recent: Branch[];
  /** The local branches, by name. */
  local: BranchNode[];
  /** The remote branches, by name. */
  remote: BranchNode[];
  /** The tags, by name; empty when hidden. */
  tags: BranchNode[];
}

/**
 * The branches of `repository` that `search` finds, in every group: those whose whole name holds it,
 * ignoring case. Grouped by directory, and with Recent and the tags, as the settings say. Rejects with
 * a {@link Failure}.
 */
export function branches(repository: Repository, search: string): Promise<Branches> {
  return invoke("branches", { repository, search });
}

/** The full hash of the latest commit of `branch`. Rejects with a {@link Failure}, a branch that no longer exists with `revisionNotFound`. */
export function goToBranch(repository: Repository, branch: RefLabel): Promise<string> {
  return invoke("go_to_branch", { repository, branch });
}

/** What a new branch starts from: the commit HEAD is on, the latest commit of a branch or a tag, or a commit. */
export type NewBranchBase = { kind: "head" } | { kind: "branch"; label: RefLabel } | { kind: "commit"; hash: string };

/** `name`, as the user types it for a new branch, with each character git takes in no branch name replaced by `-`, so it keeps its length. */
export function cleanBranchName(name: string): Promise<string> {
  return invoke("clean_branch_name", { name });
}

/** The prefixes of existing branches, such as `feature/`, that start with what the user `typed` for a new branch, by name. Rejects with a {@link Failure}. */
export function newBranchPrefixes(repository: Repository, typed: string): Promise<string[]> {
  return invoke("new_branch_prefixes", { repository, typed });
}

/**
 * Creates the branch `name` at `base`, checked out when `checkOut` says so, and returns what HEAD is on after that. Rejects with a
 * {@link Failure}: `branchNameInvalid` for a name git does not take as it is, `branchNameTaken` for one a branch has.
 */
export function createBranch(repository: Repository, name: string, base: NewBranchBase, checkOut: boolean): Promise<CurrentBranch> {
  return invoke("create_branch", { repository, name, base, checkOut });
}

/**
 * Renames the local branch `name` to `newName`, which is checked as a new branch's name is, and with `unsetUpstream` leaves it
 * following nothing. Returns what HEAD is on after that. Rejects with a {@link Failure}.
 */
export function renameBranch(repository: Repository, name: string, newName: string, unsetUpstream: boolean): Promise<CurrentBranch> {
  return invoke("rename_branch", { repository, name, newName, unsetUpstream });
}

/**
 * Deletes the local branches `names`, merged or not, and raises a notification about the Project at `project` that offers to
 * restore them. Rejects with a {@link Failure}: `currentBranchNotDeleted` when HEAD is on one of them, and nothing goes.
 */
export function deleteBranches(project: string, repository: Repository, names: string[]): Promise<void> {
  return invoke("delete_branches", { project, repository, names });
}

/** Checks out the local branch `name`, and returns what HEAD is on after that. Rejects with a {@link Failure}. */
export function checkOutBranch(repository: Repository, name: string): Promise<CurrentBranch> {
  return invoke("check_out_branch", { repository, name });
}

/** Checks out the tag `name` on a detached HEAD, and returns what HEAD is on after that. Rejects with a {@link Failure}. */
export function checkOutTag(repository: Repository, name: string): Promise<CurrentBranch> {
  return invoke("check_out_tag", { repository, name });
}

/** Checks out the commit `revision` names on a detached HEAD, and returns what HEAD is on after that. Rejects with a {@link Failure}. */
export function checkOutRevision(repository: Repository, revision: string): Promise<CurrentBranch> {
  return invoke("check_out_revision", { repository, revision });
}

/** What a checkout takes HEAD to. */
export type CheckoutTarget =
  | { kind: "branch"; name: string }
  | { kind: "tag"; name: string }
  | { kind: "revision"; revision: string }
  | { kind: "newBranch"; name: string; base: NewBranchBase }
  | { kind: "remoteBranch"; name: string; localCommits: LocalCommits | null };

/** What happens to the commits of a local branch that checking out the remote branch of its name would lose, as the user chose. */
export type LocalCommits = "drop" | "rebase";

/**
 * Checks out the remote branch `name`, such as `origin/main`, as the local branch of its name that follows it, and returns what HEAD
 * is on after that. A local branch of that name moves to it when none of its commits would be lost, and otherwise as `localCommits`
 * says: dropping them, once a Backup holds them, which raises a notification about the Project at `project` that offers Undo, or
 * rebasing them onto it, which can stop on a conflict and raise `conflictsLeft`. Rejects with a {@link Failure}:
 * `localCommitsWouldBeLost` when commits would be lost and `localCommits` is null, and `checkoutBlocked` when files block it.
 */
export function checkOutRemoteBranch(project: string, repository: Repository, name: string, localCommits: LocalCommits | null): Promise<CurrentBranch> {
  return invoke("check_out_remote_branch", { project, repository, name, localCommits });
}

/** The local commits a checkout of a remote branch would lose, as `error` tells of them, or null when it tells of another failure. */
export function localCommitsWouldBeLost(error: unknown): Extract<Failure, { kind: "localCommitsWouldBeLost" }> | null {
  return isFailure(error) && error.kind === "localCommitsWouldBeLost" ? error : null;
}

/**
 * What happens to the local changes that block a checkout: discarding the changes to `files`, the blocking files the user confirmed,
 * which the others stay beside, or saving them, checking out and restoring them in their Changelists.
 */
export type OverLocalChanges = { kind: "discard"; files: string[] } | { kind: "saveAndRestore" };

/**
 * Checks out `target` once the local changes that blocked it have gone as `over` says, and returns what HEAD is on after that. A
 * restore that conflicts keeps the saved changes and raises `conflictsLeft`. Rejects with a {@link Failure}: `checkoutBlocked` again
 * when the files that block it are no longer the ones the user confirmed, or files git does not track still block it.
 */
export function checkOutOverLocalChanges(project: string, repository: Repository, target: CheckoutTarget, over: OverLocalChanges): Promise<CurrentBranch> {
  return invoke("check_out_over_local_changes", { project, repository, target, over });
}

/** The blocked checkout `error` tells of, or null when it tells of another failure. */
export function checkoutBlocked(error: unknown): Extract<Failure, { kind: "checkoutBlocked" }> | null {
  return isFailure(error) && error.kind === "checkoutBlocked" ? error : null;
}

/** What HEAD of `repository` is on. Rejects with a {@link Failure}. */
export function currentBranch(repository: Repository): Promise<CurrentBranch> {
  return invoke("current_branch", { repository });
}

/** Which operation is half done. */
export type OperationKind = "merge" | "rebase" | "revert" | "cherryPick";

/**
 * What the user can do about an operation in progress: go on once the conflicts are resolved, leave out the commit a rebase
 * stopped on, finish a merge with a commit with the message git prepared, or give it up.
 */
export type OperationAction = "continue" | "skip" | "commitMerge" | "abort";

/** A merge, rebase, revert or cherry-pick half done in a Repository, with what the user can do about it, in the order to offer it. */
export interface OperationInProgress {
  kind: OperationKind;
  actions: OperationAction[];
}

/**
 * The operation half done in `repository`, a Repository of the Project at `project`, whoever started it, or null. A stopped rebase
 * also raises a notification that offers to continue or abort it. Rejects with a {@link Failure}.
 */
export function operationInProgress(project: string, repository: Repository): Promise<OperationInProgress | null> {
  return invoke("operation_in_progress", { project, repository });
}

/**
 * Takes `action` on the operation half done in `repository`, and returns the operation in progress after that, which a rebase,
 * revert or cherry-pick that stopped on the next conflict still is. Rejects with a {@link Failure}: `conflictsNotResolved` when
 * going on while files still have conflicts.
 */
export function takeOperationAction(
  project: string,
  repository: Repository,
  action: OperationAction,
): Promise<OperationInProgress | null> {
  return invoke("take_operation_action", { project, repository, action });
}

/** What one side of a conflict did to the file since the sides parted. `absent`: the side never had it, only the other added it. */
export type SideChange = "modified" | "added" | "deleted" | "absent";

/** One side of a conflict from the user's point of view: theirs is what the operation brings in. */
export type ConflictSide = "yours" | "theirs";

/** A file in conflict. "Yours" is the user's side, the branch they are on, even during a rebase, where git calls it `--theirs`. */
export interface ConflictedFile {
  path: string;
  yours: SideChange;
  theirs: SideChange;
}

/** The files in conflict in `repository`, ordered by path. Rejects with a {@link Failure}. */
export function conflicts(project: string, repository: Repository): Promise<ConflictedFile[]> {
  return invoke("conflicts", { project, repository });
}

/**
 * Resolves the files at `paths` with the version of `side`, or deletes them where that side does not have them, and returns the files
 * still in conflict. Rejects with a {@link Failure}: `fileNotInConflict` for a path that is not, and then nothing is resolved.
 */
export function acceptSide(project: string, repository: Repository, paths: string[], side: ConflictSide): Promise<ConflictedFile[]> {
  return invoke("accept_side", { project, repository, paths, side });
}

/** Marks the files at `paths` resolved as the user edited them by hand, and returns the files still in conflict. Rejects with a {@link Failure}. */
export function markResolved(project: string, repository: Repository, paths: string[]): Promise<ConflictedFile[]> {
  return invoke("mark_resolved", { project, repository, paths });
}

/**
 * Closes the conflicts dialog. Files still in conflict stay so, as a group in the changes view, and a notification offers to
 * resolve them and to abort the operation that left them. Rejects with a {@link Failure}.
 */
export function closeConflicts(project: string, repository: Repository): Promise<void> {
  return invoke("close_conflicts", { project, repository });
}

/** The searches of the log the user ran, newest first, each with its regex and match-case options. */
export function logSearchHistory(): Promise<TextSearch[]> {
  return invoke("log_search_history");
}

/** The local and remote branches the log can be filtered by. Rejects with a {@link Failure}. */
export function logBranches(repository: Repository): Promise<RefLabel[]> {
  return invoke("log_branches", { repository });
}

/** The full hash of the commit `revision` names: a hash of 40 or 64 characters or part of one, a branch or a tag. Rejects with a {@link Failure}. */
export function goTo(repository: Repository, revision: string): Promise<string> {
  return invoke("go_to", { repository, revision });
}

/**
 * Whether the log filtered by `filter` can show the commit `hash`: its search, if any, matches it, and
 * the branches it keeps to reach it. When it cannot, the log does not show it, which is known without
 * reading its pages. Rejects with a {@link Failure}, as the search does for a regular expression gikli
 * cannot read.
 */
export function filteredLogReaches(repository: Repository, filter: LogFilter, hash: string): Promise<boolean> {
  return invoke("filtered_log_reaches", { repository, filter, hash });
}

/** The branches and tags whose name holds `text`, those that start with it first. Rejects with a {@link Failure}. */
export function goToSuggestions(repository: Repository, text: string): Promise<RefLabel[]> {
  return invoke("go_to_suggestions", { repository, text });
}

/** A piece of a commit message. */
export interface MessagePart {
  text: string;
  /** The full hash of the commit the text names, when it is the hash of one. */
  commit: string | null;
}

/** The details of a commit. */
export interface CommitDetails {
  hash: string;
  parents: string[];
  /** Its whole message, in parts, so the hashes of commits it names can lead to them. */
  message: MessagePart[];
  author: string;
  authorEmail: string;
  /** When it was written, in seconds since the epoch. */
  authoredAt: number;
  committer: string;
  committerEmail: string;
  /** When it was committed, in seconds since the epoch. */
  committedAt: number;
  /** Whether its author and its committer are not the same. */
  committerDiffers: boolean;
  /** The local and remote branches that contain it, local ones first. */
  branches: RefLabel[];
}

/** The files a commit changed against one of its parents. */
export interface CommitChanges {
  /** The parent, or null for a commit with none. */
  parent: string | null;
  group: ChangeGroup;
}

/** Where a push sends the current branch: the branch `branch` of the remote `remote`. */
export interface PushTarget {
  remote: string;
  /** The name of the branch on the remote, such as `main`. */
  branch: string;
}

/** Which tags a push sends besides the branch: none unless the user asks, every one, or those of the current branch (`--follow-tags`). */
export type PushTags = "none" | "all" | "currentBranch";

/** A commit in a list of commits: one a push would send, or one a deleted branch took away. */
export interface ListedCommit {
  hash: string;
  subject: string;
  author: string;
  /** When it was committed, in seconds since the epoch. */
  committedAt: number;
  /** Whether someone other than the user wrote it. The dialog marks it with `*`. */
  byOtherAuthor: boolean;
}

/** What the push dialog shows: the Repository, its current branch, where it goes and the commits it would send. */
export interface PushPlan {
  repository: Repository;
  /** The current branch, the one that is pushed. */
  branch: string;
  /** The remotes the target can be on. */
  remotes: string[];
  /** null when the Repository has no remote, so the user has to define one first. */
  target: PushTarget | null;
  /** Whether the push creates the target. */
  newBranch: boolean;
  /** Newest first. */
  commits: ListedCommit[];
}

/**
 * What a push of the current branch of `repository` would send to `target`, or to its upstream or a
 * branch of the same name when `target` is null. Rejects with a {@link Failure}.
 */
export function pushPlan(repository: Repository, target: PushTarget | null): Promise<PushPlan> {
  return invoke("push_plan", { repository, target });
}

/**
 * Pushes the current branch of `repository` to `target` with `tags`, as a long operation. A target the
 * push creates becomes the upstream of a branch that has none. Rejects with a {@link Failure}.
 */
export function push(repository: Repository, target: PushTarget, tags: PushTags): Promise<void> {
  return invoke("push", { repository, target, tags });
}

/** How a push its upstream rejected integrates the commits the branch lacks: `git pull --no-rebase` or `git pull --rebase`. */
export type UpdateMethod = "merge" | "rebase";

/**
 * Integrates `target`, the upstream that rejected the push of the current branch of `repository`, with `method`, and
 * pushes again with `tags`. With `remember`, later rejected pushes do so without asking. Rejects with a {@link Failure}.
 */
export function updateAndPush(
  repository: Repository,
  target: PushTarget,
  tags: PushTags,
  method: UpdateMethod,
  remember: boolean,
): Promise<void> {
  return invoke("update_and_push", { repository, target, tags, method, remember });
}

/** A remote of the Repository, with the names of the branches it had when it was last fetched. */
export interface PullRemote {
  name: string;
  branches: string[];
}

/** What the pull dialog shows: the current branch, which the pull integrates into, the remotes, and the source it starts from. */
export interface PullPlan {
  repository: Repository;
  /** The current branch, which the pull integrates into. */
  branch: string;
  remotes: PullRemote[];
  /** The remote the dialog starts from: null when the Repository has none. */
  remote: string | null;
  /** The branch of `remote` the dialog starts from: null when there is none to start from. */
  sourceBranch: string | null;
}

/** Where a pull takes the work it integrates from: the branch `branch` of the remote `remote`. */
export interface PullSource {
  remote: string;
  branch: string;
}

/** How a pull integrates what it brings, each as the git option of that name. None ticked merges. */
export interface PullOptions {
  rebase: boolean;
  ffOnly: boolean;
  noFf: boolean;
  squash: boolean;
  noCommit: boolean;
  noVerify: boolean;
}

/**
 * What the pull dialog shows for `repository`: it starts from the source the current branch was last pulled from when that was
 * not the default, and otherwise from its upstream, or `origin`. Rejects with a {@link Failure}.
 */
export function pullPlan(repository: Repository): Promise<PullPlan> {
  return invoke("pull_plan", { repository });
}

/**
 * Pulls `source` into the current branch of `repository`, a Repository of the Project at `project`, with `options`, as a long
 * operation, and returns the operation in progress after that: a rebase or a merge that stopped. A pull that stops on a conflict
 * raises `conflictsLeft`. Rejects with a {@link Failure}.
 */
export function pull(
  project: string,
  repository: Repository,
  source: PullSource,
  options: PullOptions,
): Promise<OperationInProgress | null> {
  return invoke("pull", { project, repository, source, options });
}

/** What follows a commit made with Commit and push: the push dialog, or a push that already ran because the settings suppress the dialog. */
export type PushAfterCommit = { kind: "dialog"; plan: PushPlan } | { kind: "pushed"; target: PushTarget };

/**
 * The second step of Commit and push, once the commit of `repository` went through: opens the push
 * dialog, or pushes the current branch at once, without tags, when the settings suppress the dialog.
 * Rejects with a {@link Failure}.
 */
export function pushAfterCommit(repository: Repository): Promise<PushAfterCommit> {
  return invoke("push_after_commit", { repository });
}

/**
 * Defines the remote `name` of `repository` at `url` and fetches it at once, as a long operation. A
 * remote that cannot be fetched is not kept. Rejects with a {@link Failure}.
 */
export function addRemote(repository: Repository, name: string, url: string): Promise<void> {
  return invoke("add_remote", { repository, name, url });
}

/** The details of the commit `revision` names. Rejects with a {@link Failure}. */
export function commitDetails(repository: Repository, revision: string): Promise<CommitDetails> {
  return invoke("commit_details", { repository, revision });
}

/** The files the commit `revision` names changed, against each of its parents. Rejects with a {@link Failure}. */
export function commitChanges(repository: Repository, revision: string): Promise<CommitChanges[]> {
  return invoke("commit_changes", { repository, revision });
}

/** A commit that changed a file, and what it did to it. */
export interface FileRevision {
  hash: string;
  parents: string[];
  subject: string;
  author: string;
  /** When it was committed, in seconds since the epoch. */
  committedAt: number;
  /** The path the file has in this commit, which differs from {@link FileHistory.path} before a rename. */
  path: string;
  /** The path the file had before this commit, for a commit that renamed or copied it. */
  originalPath: string | null;
  status: ChangeStatus;
}

/** The history of one file: the commits that changed it, newest first, followed through its renames. */
export interface FileHistory {
  repository: Repository;
  /** Relative to the Repository root, with `/` between folders. */
  path: string;
  revisions: FileRevision[];
  /** Whether the file has older revisions than those listed, which the row limit left out. */
  more: boolean;
}

/** What a commit changed in one file, against its first parent. The local side is the file in the commit, read-only. */
export interface RevisionFileDiff {
  repository: Repository;
  revision: string;
  /** The first parent, or null for a commit with none. */
  parent: string | null;
  path: string;
  /** The path the file had in the parent, which the head side shows, for a rename or a copy. */
  originalPath: string | null;
  content: DiffContent;
}

/** The history of the file at `path`, of HEAD or of `branches` when the user filters by them. Rejects with a {@link Failure}. */
export function fileHistory(repository: Repository, path: string, branches: RefLabel[]): Promise<FileHistory> {
  return invoke("file_history", { repository, path, branches });
}

/** What the commit `revision` changed in the file at `path`, its path there. Rejects with a {@link Failure}. */
export function revisionFileDiff(repository: Repository, revision: string, path: string): Promise<RevisionFileDiff> {
  return invoke("revision_file_diff", { repository, revision, path });
}

/**
 * Brings the file at `path` back to its version in the commit `revision`, where it was at
 * `revisionPath`, on disk only. Rejects with a {@link Failure}.
 */
export function getFileVersion(repository: Repository, revision: string, revisionPath: string, path: string): Promise<void> {
  return invoke("get_file_version", { repository, revision, revisionPath, path });
}

/** What the user chose in the stash dialog. */
export interface StashOptions {
  /** Without text, git names the stash after the commit HEAD is on. */
  message: string;
  /** Whether what is staged also stays in the index and the working tree. */
  keepIndex: boolean;
  /** Whether the stash also takes the files git does not track. */
  includeUntracked: boolean;
}

/** Why a Repository cannot stash. */
export type StashUnavailable = "noCommits";

/** One stash. */
export interface Stash {
  /** How git names it now, such as `stash@{0}`. */
  name: string;
  /** The full hash of its commit, which stays the same whatever its place in the list. */
  hash: string;
  /** What git says of it, such as `On main: Half done`. */
  message: string;
  /** In seconds since the epoch. */
  createdAt: number;
}

/** The stashes of a Repository, newest first, with why it cannot stash, when it cannot. */
export interface Stashes {
  repository: Repository;
  unavailable: StashUnavailable | null;
  entries: Stash[];
}

/** What the diff of a file in a stash compares the stashed file with: the file on disk, or the file in the commit the stash was made on. */
export type StashDiffBase = "local" | "parent";

/**
 * The diff of a file in a stash. Against the local version the head side is the stashed file and
 * the local side the file on disk; against the parent the head side is the file in that commit and
 * the local side the stashed file. Neither side can be edited.
 */
export interface StashFileDiff {
  repository: Repository;
  stash: string;
  against: StashDiffBase;
  path: string;
  originalPath: string | null;
  content: DiffContent;
}

/** Puts the uncommitted changes aside in a stash, and returns the stashes after that. Rejects with a {@link Failure}. */
export function stash(repository: Repository, options: StashOptions): Promise<Stashes> {
  return invoke("stash", { repository, options });
}

/** The stashes of `repository`, newest first. Rejects with a {@link Failure}. */
export function stashes(repository: Repository): Promise<Stashes> {
  return invoke("stashes", { repository });
}

/** The files the stash `hash` holds, arranged as the log arranges the files of a commit. Rejects with a {@link Failure}. */
export function stashFiles(repository: Repository, hash: string): Promise<ChangeGroup> {
  return invoke("stash_files", { repository, hash });
}

/** The diff of the file at `path` in the stash `hash`, against what the settings say. Rejects with a {@link Failure}. */
export function stashFileDiff(repository: Repository, hash: string, path: string): Promise<StashFileDiff> {
  return invoke("stash_file_diff", { repository, hash, path });
}

/** Makes the diff of a file in a stash compare it with `against`. Rejects with a {@link Failure}. */
export function setStashDiffAgainst(against: StashDiffBase): Promise<Settings> {
  return invoke("set_stash_diff_against", { against });
}

/** Drops the stash `hash`, once the user confirmed it, and returns the stashes after that. Rejects with a {@link Failure}. */
export function dropStash(repository: Repository, hash: string): Promise<Stashes> {
  return invoke("drop_stash", { repository, hash });
}

/** Drops every stash, once the user confirmed it, when they are still `seen`, newest first. Rejects with a {@link Failure}. */
export function clearStashes(repository: Repository, seen: string[]): Promise<Stashes> {
  return invoke("clear_stashes", { repository, seen });
}

/** How the user brings a stash back. */
export interface UnstashOptions {
  /** Whether the stash is dropped once it applied. One that left files in conflict is kept. */
  pop: boolean;
  /** Whether what was staged when the stash was made is staged again. */
  restoreIndex: boolean;
}

/** What the unstash dialog shows before the user brings a stash back. */
export interface UnstashPlan {
  repository: Repository;
  stash: Stash;
  /** Whether tracked files have uncommitted changes. The dialog warns of them and does not block. */
  localChanges: boolean;
  /** The files the stash holds that git did not track and that are on disk already: git restores none of them. */
  untrackedInTheWay: string[];
}

/** What bringing a stash back came to. */
export interface Unstashed {
  stashes: Stashes;
  /** Whether applying the stash left files in conflict; the core then raises `conflictsLeft`. */
  conflictsLeft: boolean;
}

/** What the unstash dialog shows for the stash `hash`. Rejects with a {@link Failure}. */
export function unstashPlan(repository: Repository, hash: string): Promise<UnstashPlan> {
  return invoke("unstash_plan", { repository, hash });
}

/**
 * Applies the stash `hash`, restoring its index and dropping it after as `options` say. A stash
 * that conflicts raises `conflictsLeft` and is kept. Rejects with a {@link Failure}.
 */
export function applyStash(project: string, repository: Repository, hash: string, options: UnstashOptions): Promise<Unstashed> {
  return invoke("apply_stash", { project, repository, hash, options });
}

/**
 * Creates the branch `name` at the commit the stash `hash` was made on, checks it out, applies the
 * stash with its index and drops it. A stash that conflicts raises `conflictsLeft` and is kept.
 * Rejects with a {@link Failure}.
 */
export function stashBranch(project: string, repository: Repository, hash: string, name: string): Promise<Unstashed> {
  return invoke("stash_branch", { project, repository, hash, name });
}

/** Lets go of what the core holds to read the page at `cursor` quickly, once the log no longer needs it. */
export function endLogRead(cursor: LogCursor): Promise<void> {
  return invoke("end_log_read", { cursor });
}

/** The message of the last commit, or null when there is no commit yet. Rejects with a {@link Failure}. */
export function lastCommitMessage(repository: Repository): Promise<string | null> {
  return invoke("last_commit_message", { repository });
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
