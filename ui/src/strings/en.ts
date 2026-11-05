// Every string the UI shows. Other languages add a file with the same keys.
// `{name}` marks a value filled in at runtime.
export const en = {
  appTitle: "gikli",
  openFolder: "Open Folder…",
  openFolderDialogTitle: "Open a folder as a Project",
  noProject: "Open a folder to start.",
  projectFolder: "Project: {folder}",
  repositoryRoot: "Repository: {root}",
  localChangesTitle: "Local changes",
  refreshChanges: "Refresh",
  readingChanges: "Reading changes…",
  noChanges: "No changes.",
  statusModified: "Modified",
  statusAdded: "Added",
  statusDeleted: "Deleted",
  statusUntracked: "Untracked",
  gitReady: "git {version}",
  gitTooOld:
    "gikli needs git {minimum} or newer, and found {version} at {executable}. Update git to use gikli; until then gikli runs no git operations.",
  gitUnrecognized:
    "gikli could not read the version of the git at {executable}. Check that it runs from a terminal.",
  gitNotFound:
    "gikli could not find git on the PATH. Install git {minimum} or newer and restart gikli.",
  checkingGit: "Looking for git…",
  failureGitNotFound: "git is not available, so the folder cannot be opened.",
  failureGitTooOld:
    "git {version} is older than {minimum}, so gikli runs no operations.",
  failureGitUnrecognized: "gikli cannot use this git, so the folder cannot be opened.",
  failureGitFailedToStart: "git could not be started: {message}",
  failureFolderNotFound: "There is no folder at {folder}.",
  failureNotInRepository: "{folder} is not inside a git repository.",
  failureChangesUnreadable:
    "git could not read the changes in {folder}. Check that it is still a git repository, then refresh.",
  unexpectedError: "Something went wrong: {message}",
} as const;

export type Strings = { readonly [K in keyof typeof en]: string };
