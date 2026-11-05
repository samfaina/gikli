import { open } from "@tauri-apps/plugin-dialog";

import {
  actOnNotification,
  addRepository,
  capabilities,
  checkGit,
  clearOperationLog,
  dismissNotification,
  createRepository,
  formatVersion,
  ignoreRepository,
  isFailure,
  notifications,
  onCoreEvent,
  openProject,
  operationLog,
  readChanges,
  removeRepository,
  setChangeGrouping,
  setGitPath,
  setOperationLogSoftWrap,
  setShowIgnoredFiles,
  settings,
  startupFolder,
  testGit,
  unwatchChanges,
  watchChanges,
  windowForProject,
  type Capability,
  type Change,
  type ChangeNode,
  type ChangeSet,
  type ChangeStatus,
  type ConflictKind,
  type Failure,
  type GitCheck,
  type LongOperation,
  type Notification,
  type NotificationAction,
  type OperationLogEntry,
  type Progress,
  type Project,
  type Repository,
} from "./core";
import { t, type StringKey } from "./strings";

const openButton = element<HTMLButtonElement>("open-folder");
const newRepositoryButton = element<HTMLButtonElement>("new-repository");
const gitStatus = element<HTMLElement>("git-status");
const projectView = element<HTMLElement>("project");
const settingsButton = element<HTMLButtonElement>("toggle-settings");
const settingsPanel = element<HTMLElement>("settings");
const gitPathInput = element<HTMLInputElement>("git-path");
const testGitButton = element<HTMLButtonElement>("test-git");
const saveGitPathButton = element<HTMLButtonElement>("save-git-path");
const gitTestResult = element<HTMLElement>("git-test-result");
const operationLogButton = element<HTMLButtonElement>("toggle-operation-log");
const operationLogPanel = element<HTMLElement>("operation-log");
const operationLogEntries = element<HTMLElement>("operation-log-entries");
const softWrapInput = element<HTMLInputElement>("operation-log-soft-wrap");
const clearOperationLogButton = element<HTMLButtonElement>("clear-operation-log");
const notificationsView = element<HTMLElement>("notifications");
const operationsView = element<HTMLElement>("operations");

document.title = t("appTitle");
openButton.textContent = t("openFolder");
openButton.disabled = true;
newRepositoryButton.textContent = t("newRepository");
newRepositoryButton.disabled = true;
gitStatus.textContent = t("checkingGit");
showMessage(t("noProject"));
settingsButton.textContent = t("settings");
element<HTMLElement>("git-path-label").textContent = t("gitPathLabel");
element<HTMLElement>("git-path-hint").textContent = t("gitPathHint");
gitPathInput.placeholder = t("gitPathPlaceholder");
testGitButton.textContent = t("testGit");
saveGitPathButton.textContent = t("saveGitPath");
operationLogButton.textContent = t("operationLog");
element<HTMLElement>("operation-log-title").textContent = t("operationLogTitle");
element<HTMLElement>("operation-log-soft-wrap-label").textContent = t("softWrap");
clearOperationLogButton.textContent = t("clearOperationLog");
notificationsView.setAttribute("aria-label", t("notificationsTitle"));

operationLogButton.addEventListener("click", () => {
  const opening = operationLogPanel.hidden;
  operationLogPanel.hidden = !opening;
  operationLogButton.setAttribute("aria-expanded", String(opening));
  if (opening) {
    operationLogEntries.scrollTop = operationLogEntries.scrollHeight;
  }
});

softWrapInput.addEventListener("change", async () => {
  softWrapInput.disabled = true;
  try {
    const saved = await setOperationLogSoftWrap(softWrapInput.checked);
    showSoftWrap(saved.operationLogSoftWrap);
  } catch (error) {
    softWrapInput.checked = !softWrapInput.checked;
    report(describeError(error));
  } finally {
    softWrapInput.disabled = false;
  }
});

clearOperationLogButton.addEventListener("click", () => void clearOperationLog());

settingsButton.addEventListener("click", async () => {
  const opening = settingsPanel.hidden;
  if (opening) {
    gitPathInput.value = (await settings()).gitPath ?? "";
    gitTestResult.textContent = "";
  }
  settingsPanel.hidden = !opening;
  settingsButton.setAttribute("aria-expanded", String(opening));
});

testGitButton.addEventListener("click", async () => {
  const path = gitPathInput.value.trim();
  if (path === "") {
    gitTestResult.textContent = t("gitPathEmpty");
    return;
  }
  testGitButton.disabled = true;
  gitTestResult.textContent = t("testingGit");
  try {
    gitTestResult.textContent = describeGit(await testGit(path));
  } finally {
    testGitButton.disabled = false;
  }
});

saveGitPathButton.addEventListener("click", async () => {
  const path = gitPathInput.value.trim();
  saveGitPathButton.disabled = true;
  try {
    const git = await setGitPath(path === "" ? null : path);
    showGit(git);
    gitTestResult.textContent = t("gitPathSaved", { result: describeGit(git) });
  } catch (error) {
    gitTestResult.textContent = describeError(error);
  } finally {
    saveGitPathButton.disabled = false;
  }
});

openButton.addEventListener("click", async () => {
  const folder = await open({ directory: true, title: t("openFolderDialogTitle") });
  if (typeof folder === "string") {
    await openFolder(folder);
  }
});

// In a window that shows a Project, a new Repository inside it joins it; any other opens as a Project.
newRepositoryButton.addEventListener("click", async () => {
  const folder = await open({ directory: true, title: t("newRepositoryDialogTitle") });
  if (typeof folder !== "string") {
    return;
  }
  newRepositoryButton.disabled = true;
  try {
    const repository = await createRepository(folder);
    if (shownFolder === null) {
      await openFolder(repository.root);
      return;
    }
    try {
      showProject(await addRepository(shownFolder, repository));
    } catch (error) {
      if (!isFailure(error) || error.kind !== "repositoryOutsideProject") {
        throw error;
      }
      await openFolder(repository.root);
    }
  } catch (error) {
    report(describeError(error));
  } finally {
    newRepositoryButton.disabled = false;
  }
});

void start();

/** The folder of the Project this window shows, once it shows one. */
let shownFolder: string | null = null;

/** The Repositories shown, by root, each with the function that reads its changes again. */
const shownRepositories = new Map<string, { repository: Repository; refresh: () => void }>();

void onCoreEvent((event) => {
  switch (event.event) {
    case "operationLogged":
      appendLogEntries([event.payload]);
      break;
    case "operationLogCleared":
      shownLogEntries.clear();
      showLogEntries();
      break;
    case "notificationRaised":
      shownNotifications.set(event.payload.id, event.payload);
      showNotifications();
      break;
    case "notificationRemoved":
      shownNotifications.delete(event.payload.id);
      showNotifications();
      break;
    case "operationStarted":
      runningOperations.set(event.payload.id, { operation: event.payload.operation, progress: null });
      showOperations();
      break;
    case "operationProgress": {
      const running = runningOperations.get(event.payload.id);
      if (running !== undefined) {
        running.progress = event.payload.progress;
        showOperations();
      }
      break;
    }
    case "operationFinished":
      runningOperations.delete(event.payload.id);
      showOperations();
      break;
    case "filesChanged":
      shownRepositories.get(event.payload.repository.root)?.refresh();
      break;
  }
});

async function start(): Promise<void> {
  // Read before opening a Project, which raises notifications and logs commands as events.
  const [current, logged, raised] = await Promise.all([settings(), operationLog(), notifications()]);
  showSoftWrap(current.operationLogSoftWrap);
  appendLogEntries(logged);
  for (const notification of raised) {
    shownNotifications.set(notification.id, notification);
  }
  showNotifications();
  showGit(await checkGit());
  // Even without a usable git, a folder under \\wsl$ opens with the git of its distribution;
  // any other folder reports why it cannot open.
  openButton.disabled = false;
  newRepositoryButton.disabled = false;
  const folder = await startupFolder();
  if (folder !== null) {
    await openFolder(folder);
  }
}

/** Shows the git gikli uses. */
function showGit(git: GitCheck): void {
  gitStatus.classList.toggle("warning", git.status !== "ready");
  gitStatus.textContent = describeGit(git);
}

/** The git gikli found or tested: its version, or what is wrong with it. */
function describeGit(git: GitCheck): string {
  switch (git.status) {
    case "ready":
      return t(git.source === "wsl" ? "gitReadyWsl" : "gitReady", { version: formatVersion(git.version) });
    case "tooOld":
      return t("gitTooOld", {
        version: formatVersion(git.version),
        minimum: formatVersion(git.minimum),
        executable: git.executable,
      });
    case "unrecognized":
      return t("gitUnrecognized", { executable: git.executable });
    case "missing":
      return t("gitMissing", { executable: git.executable });
    case "notFound":
      return t("gitNotFound", { minimum: formatVersion(git.minimum) });
  }
}

/** Opens `folder` as a Project, in this window or in a window of its own. */
async function openFolder(folder: string): Promise<void> {
  let project: Project;
  try {
    project = await openProject(folder);
  } catch (error) {
    report(describeError(error));
    return;
  }
  try {
    const shown = await windowForProject(project.folder);
    if (shown.kind === "here") {
      showProject(project);
    }
  } catch (error) {
    report(describeError(error));
  }
}

/** Shows `project` in this window: the Repositories gikli offers to add, the ones that broke, the ignored ones, and the changes of the others. */
function showProject(project: Project): void {
  shownFolder = project.folder;
  showNotifications();
  const name = project.folder.split(/[\\/]/).filter((part) => part !== "").pop() ?? project.folder;
  document.title = t("windowTitle", { name });
  // A Repository that leaves the view, removed or ignored, is no longer watched.
  const shownRoots = new Set(project.repositories.map((repository) => repository.root));
  for (const { repository } of shownRepositories.values()) {
    if (!shownRoots.has(repository.root)) {
      void unwatchChanges(repository);
    }
  }
  shownRepositories.clear();
  const options = document.createElement("div");
  void viewOptions().then((view) => options.replaceWith(view));
  const choose = (choice: typeof addRepository, repository: Repository) => choice(project.folder, repository);
  projectView.replaceChildren(
    line("project-folder", t("projectFolder", { folder: project.folder })),
    ...repositoryChoices("unregistered-repositories", t("unregisteredRepositoriesTitle"), project.unregisteredRepositories, (repository) => [
      { testId: "add-repository", text: t("addRepository"), act: () => choose(addRepository, repository) },
      { testId: "ignore-repository", text: t("ignoreRepository"), act: () => choose(ignoreRepository, repository) },
    ]),
    ...repositoryChoices("invalid-repositories", t("invalidRepositoriesTitle"), project.invalidRepositories, (repository) => [
      { testId: "remove-repository", text: t("removeRepository"), act: () => choose(removeRepository, repository) },
    ]),
    options,
    ...project.repositories.map(repositoryView),
    ...repositoryChoices("ignored-repositories", t("ignoredRepositoriesTitle"), project.ignoredRepositories, (repository) => [
      { testId: "add-repository", text: t("addRepository"), act: () => choose(addRepository, repository) },
    ]),
  );
}

/** A button that changes which Repositories the Project holds, and returns the Project after the change. */
interface RepositoryChoice {
  testId: string;
  text: string;
  act: () => Promise<Project>;
}

/**
 * A titled list of `repositories`, each with the buttons `choices` gives it; nothing when the
 * list is empty. A choice shows the Project again once made.
 */
function repositoryChoices(
  testId: string,
  title: string,
  repositories: Repository[],
  choices: (repository: Repository) => RepositoryChoice[],
): HTMLElement[] {
  if (repositories.length === 0) {
    return [];
  }
  const section = document.createElement("section");
  section.className = "repository-choices";
  section.dataset.testid = testId;
  const heading = document.createElement("h2");
  heading.textContent = title;
  const list = document.createElement("ul");
  for (const repository of repositories) {
    const item = document.createElement("li");
    const root = document.createElement("span");
    root.textContent = repository.root;
    const buttons = choices(repository).map((choice) => {
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.testid = choice.testId;
      button.textContent = choice.text;
      button.addEventListener("click", async () => {
        buttons.forEach((other) => (other.disabled = true));
        try {
          showProject(await choice.act());
        } catch (error) {
          report(describeError(error));
          buttons.forEach((other) => (other.disabled = false));
        }
      });
      return button;
    });
    item.append(root, ...buttons);
    list.append(item);
  }
  section.append(heading, list);
  return [section];
}

/** The options of the changes view: grouping by folder and showing ignored files. */
async function viewOptions(): Promise<HTMLElement> {
  const current = await settings();
  const options = document.createElement("div");
  options.className = "view-options";
  options.append(
    settingCheckbox("group-by-folder", t("groupByFolder"), current.changeGrouping === "folder", (checked) =>
      setChangeGrouping(checked ? "folder" : "flat"),
    ),
    settingCheckbox("show-ignored-files", t("showIgnoredFiles"), current.showIgnoredFiles, (checked) =>
      setShowIgnoredFiles(checked),
    ),
  );
  return options;
}

/** A labelled checkbox for a setting of the changes view: it saves the setting with `save`, then reads every shown Repository's changes again. */
function settingCheckbox(
  testId: string,
  text: string,
  checked: boolean,
  save: (checked: boolean) => Promise<unknown>,
): HTMLLabelElement {
  const label = document.createElement("label");
  const input = document.createElement("input");
  input.type = "checkbox";
  input.dataset.testid = testId;
  input.checked = checked;
  input.addEventListener("change", async () => {
    input.disabled = true;
    try {
      await save(input.checked);
      for (const { refresh } of shownRepositories.values()) {
        refresh();
      }
    } catch (error) {
      input.checked = !input.checked;
      projectView.prepend(line("message", describeError(error)));
    } finally {
      input.disabled = false;
    }
  });
  label.append(input, document.createTextNode(text));
  return label;
}

/** A Repository's root and its uncommitted changes, with a button that reads them again. */
function repositoryView(repository: Repository): HTMLElement {
  const section = document.createElement("section");
  section.className = "repository";
  const header = document.createElement("div");
  header.className = "changes-header";
  const title = document.createElement("h2");
  title.textContent = t("localChangesTitle");
  const refresh = document.createElement("button");
  refresh.type = "button";
  refresh.dataset.testid = "refresh-changes";
  refresh.textContent = t("refreshChanges");
  header.append(title, refresh);
  const changes = document.createElement("div");
  changes.dataset.testid = "changes";
  const watching = document.createElement("div");
  const features = document.createElement("div");
  section.append(
    line("repository-root", t("repositoryRoot", { root: repository.root })),
    header,
    watching,
    changes,
    features,
  );
  void showUnavailableFeatures(repository, features);

  // One read at a time: a request while one runs reads once more when it ends, so requests from
  // the watcher and the button never pile up.
  let reading = false;
  let readAgain = false;
  const refreshChanges = async (): Promise<void> => {
    if (reading) {
      readAgain = true;
      return;
    }
    reading = true;
    refresh.disabled = true;
    try {
      do {
        readAgain = false;
        try {
          changes.replaceChildren(changeList(await readChanges(repository)));
        } catch (error) {
          changes.replaceChildren(line("message", describeError(error)));
        }
      } while (readAgain);
    } finally {
      reading = false;
      refresh.disabled = false;
    }
  };
  shownRepositories.set(repository.root, { repository, refresh: () => void refreshChanges() });
  refresh.addEventListener("click", () => void refreshChanges());
  changes.replaceChildren(line("message", t("readingChanges")));
  void refreshChanges();
  // Without a watcher the view still works, refreshed with the button.
  watchChanges(repository).catch((error: unknown) => {
    watching.replaceChildren(line("message", describeError(error)));
  });
  return section;
}

/** The features the Repository's git is too old for, shown disabled with the version each needs. */
async function showUnavailableFeatures(repository: Repository, container: HTMLElement): Promise<void> {
  let states;
  try {
    states = await capabilities(repository);
  } catch {
    // Without a usable git, the changes view already says what is wrong.
    return;
  }
  const unavailable = states.filter((state) => !state.available);
  if (unavailable.length === 0) {
    return;
  }
  container.className = "unavailable-features";
  container.dataset.testid = "unavailable-features";
  const title = document.createElement("h2");
  title.textContent = t("unavailableFeaturesTitle");
  const list = document.createElement("ul");
  for (const state of unavailable) {
    const item = document.createElement("li");
    item.dataset.capability = state.capability;
    item.setAttribute("aria-disabled", "true");
    item.textContent = t("unavailableFeature", {
      feature: t(CAPABILITY_LABELS[state.capability]),
      required: formatVersion(state.required),
    });
    list.append(item);
  }
  container.replaceChildren(title, list);
}

const CAPABILITY_LABELS: Record<Capability, StringKey> = {
  rebaseUpdateRefs: "capabilityRebaseUpdateRefs",
};

function changeList(changeSet: ChangeSet): HTMLElement {
  if (changeSet.changes.length === 0) {
    return line("message", t("noChanges"));
  }
  return nodeList(changeSet.nodes, "");
}

/** The rows of one folder, whose path is `folder`, or of the whole view when `folder` is "". */
function nodeList(nodes: ChangeNode[], folder: string): HTMLUListElement {
  const list = document.createElement("ul");
  list.className = "changes";
  for (const node of nodes) {
    list.append(node.kind === "folder" ? folderItem(node) : changeItem(node, folder));
  }
  return list;
}

/** A folder and its rows. A folder that holds changes, not only ignored files, is highlighted. */
function folderItem(folder: Extract<ChangeNode, { kind: "folder" }>): HTMLLIElement {
  const item = document.createElement("li");
  item.className = "change-folder";
  item.dataset.testid = "change-folder";
  item.classList.toggle("has-changes", folder.hasChanges);
  const name = document.createElement("span");
  name.className = "folder-name";
  name.textContent = folder.path.slice(folder.path.lastIndexOf("/") + 1);
  item.append(name, nodeList(folder.children, folder.path));
  return item;
}

/** A changed file, shown by its path relative to `folder`. */
function changeItem(change: Change, folder: string): HTMLLIElement {
  const item = document.createElement("li");
  item.className = "change";
  item.dataset.testid = "change";
  item.dataset.status = change.status;
  const status = document.createElement("span");
  status.className = "change-status";
  status.textContent = t(STATUS_LABELS[change.status]);
  const path = document.createElement("span");
  path.className = "change-path";
  path.dataset.testid = "change-path";
  path.title = change.path;
  path.textContent = folder === "" ? change.path : change.path.slice(folder.length + 1);
  item.append(status, path);
  const details = changeDetails(change);
  if (details.length > 0) {
    const detail = document.createElement("span");
    detail.className = "change-detail";
    detail.textContent = details.join(" · ");
    item.append(detail);
  }
  return item;
}

/** What else the view says about a change: where it came from, the conflict, the submodule. */
function changeDetails(change: Change): string[] {
  const details: string[] = [];
  if (change.originalPath !== null) {
    details.push(t(change.status === "copied" ? "copiedFrom" : "renamedFrom", { path: change.originalPath }));
  }
  if (change.conflict !== null) {
    details.push(t(CONFLICT_LABELS[change.conflict]));
  }
  if (change.submodule !== null) {
    const state: string[] = [];
    if (change.submodule.commitChanged) {
      state.push(t("submoduleCommitChanged"));
    }
    if (change.submodule.hasTrackedChanges) {
      state.push(t("submoduleTrackedChanges"));
    }
    if (change.submodule.hasUntrackedFiles) {
      state.push(t("submoduleUntrackedFiles"));
    }
    details.push(state.length === 0 ? t("submodule") : t("submoduleWithState", { state: state.join(", ") }));
  }
  return details;
}

const STATUS_LABELS: Record<ChangeStatus, StringKey> = {
  modified: "statusModified",
  added: "statusAdded",
  deleted: "statusDeleted",
  renamed: "statusRenamed",
  copied: "statusCopied",
  untracked: "statusUntracked",
  ignored: "statusIgnored",
  conflicted: "statusConflicted",
};

const CONFLICT_LABELS: Record<ConflictKind, StringKey> = {
  bothModified: "conflictBothModified",
  bothAdded: "conflictBothAdded",
  bothDeleted: "conflictBothDeleted",
  addedByUs: "conflictAddedByUs",
  addedByThem: "conflictAddedByThem",
  deletedByUs: "conflictDeletedByUs",
  deletedByThem: "conflictDeletedByThem",
};

/** The operation log entries this window shows, by id: every entry since the log was last cleared. */
const shownLogEntries = new Map<number, OperationLogEntry>();

/** Adds `entries` to the operation log view, skipping the ones it already shows. */
function appendLogEntries(entries: OperationLogEntry[]): void {
  const atEnd =
    operationLogEntries.scrollTop + operationLogEntries.clientHeight >= operationLogEntries.scrollHeight - 4;
  for (const entry of entries) {
    shownLogEntries.set(entry.id, entry);
  }
  showLogEntries();
  // Follows new commands, unless the user scrolled up to read older ones.
  if (atEnd) {
    operationLogEntries.scrollTop = operationLogEntries.scrollHeight;
  }
}

/** Shows the operation log entries, oldest first. The view is read-only: text can be selected, not edited. */
function showLogEntries(): void {
  if (shownLogEntries.size === 0) {
    operationLogEntries.replaceChildren(line("operation-log-empty", t("operationLogEmpty")));
    return;
  }
  const entries = [...shownLogEntries.values()].sort((first, second) => first.id - second.id);
  operationLogEntries.replaceChildren(...entries.map(logEntryView));
}

function logEntryView(entry: OperationLogEntry): HTMLElement {
  const view = document.createElement("div");
  view.className = "log-entry";
  view.dataset.testid = "log-entry";
  const failed = entry.error !== null || entry.exitCode !== 0;
  view.classList.toggle("failed", failed);
  const command = document.createElement("pre");
  command.className = "log-command";
  command.textContent = t("operationLogCommand", { folder: entry.folder, args: entry.args.join(" ") });
  view.append(command);
  const output = entry.stdout + entry.stderr;
  if (output !== "") {
    const text = document.createElement("pre");
    text.className = "log-output";
    text.textContent = output;
    view.append(text);
  }
  if (failed) {
    const result = document.createElement("pre");
    result.className = "log-result";
    result.textContent =
      entry.error !== null
        ? t("operationLogNotStarted", { error: entry.error })
        : entry.exitCode === null
          ? t("operationLogEndedBySignal")
          : t("operationLogExitCode", { code: String(entry.exitCode) });
    view.append(result);
  }
  return view;
}

/** Wraps long lines of the operation log to its width, or lets them scroll sideways. */
function showSoftWrap(softWrap: boolean): void {
  softWrapInput.checked = softWrap;
  operationLogEntries.classList.toggle("soft-wrap", softWrap);
}

/** The notifications raised, by id, for every Project: this window shows the ones about its own. */
const shownNotifications = new Map<number, Notification>();

/** Shows the notifications about the Project this window shows, each with its actions and a way to dismiss it. */
function showNotifications(): void {
  const mine = [...shownNotifications.values()]
    .filter((notification) => notification.project === shownFolder)
    .sort((first, second) => first.id - second.id);
  notificationsView.hidden = mine.length === 0;
  notificationsView.replaceChildren(...mine.map(notificationView));
}

function notificationView(notification: Notification): HTMLElement {
  const view = document.createElement("div");
  view.className = "notification";
  view.dataset.testid = "notification";
  view.dataset.kind = notification.kind.kind;
  const message = document.createElement("p");
  message.textContent = describeNotification(notification);
  const buttons = notification.actions.map((action) => {
    const button = document.createElement("button");
    button.type = "button";
    button.dataset.testid = "notification-action";
    button.dataset.action = action.action;
    button.textContent = t(ACTION_LABELS[action.action]);
    button.addEventListener("click", async () => {
      buttons.forEach((other) => (other.disabled = true));
      try {
        const outcome = await actOnNotification(notification.id, action);
        showProject(outcome.payload);
      } catch (error) {
        report(describeError(error));
        buttons.forEach((other) => (other.disabled = false));
      }
    });
    return button;
  });
  const dismiss = document.createElement("button");
  dismiss.type = "button";
  dismiss.dataset.testid = "dismiss-notification";
  dismiss.textContent = t("dismissNotification");
  dismiss.addEventListener("click", () => void dismissNotification(notification.id));
  const actions = document.createElement("div");
  actions.className = "notification-actions";
  actions.append(...buttons, dismiss);
  view.append(message, actions);
  return view;
}

function describeNotification(notification: Notification): string {
  switch (notification.kind.kind) {
    case "unregisteredRepository":
      return t("notificationUnregisteredRepository", { root: notification.kind.repository.root });
    case "invalidRepository":
      return t("notificationInvalidRepository", { root: notification.kind.repository.root });
  }
}

const ACTION_LABELS: Record<NotificationAction["action"], StringKey> = {
  addRepository: "notificationAddRepository",
  ignoreRepository: "notificationIgnoreRepository",
  removeRepository: "notificationRemoveRepository",
};

/** The long operations running, by id, with the last progress each reported. Every window shows them all. */
const runningOperations = new Map<number, { operation: LongOperation; progress: Progress | null }>();

/** Shows each running long operation with its progress, in the status area, without blocking the views. */
function showOperations(): void {
  operationsView.hidden = runningOperations.size === 0;
  operationsView.replaceChildren(
    ...[...runningOperations.values()].map(({ operation, progress }) => {
      const view = document.createElement("div");
      view.className = "operation";
      view.dataset.testid = "operation";
      const name = describeOperation(operation);
      const text = document.createElement("span");
      text.textContent =
        progress === null
          ? t("operationStarting", { operation: name })
          : t("operationProgress", { operation: name, phase: progress.phase, percent: String(progress.percent) });
      const bar = document.createElement("progress");
      bar.max = 100;
      if (progress !== null) {
        bar.value = progress.percent;
      }
      view.append(text, bar);
      return view;
    }),
  );
}

function describeOperation(operation: LongOperation): string {
  switch (operation.kind) {
    case "clone":
      return t("operationClone", { url: operation.url });
  }
}

/** The message for an error from the core: a {@link Failure}, or anything unexpected. */
function describeError(error: unknown): string {
  return isFailure(error) ? describeFailure(error) : t("unexpectedError", { message: String(error) });
}

function describeFailure(failure: Failure): string {
  switch (failure.kind) {
    case "gitNotFound":
      return t("failureGitNotFound");
    case "gitTooOld":
      return t("failureGitTooOld", {
        version: formatVersion(failure.version),
        minimum: formatVersion(failure.minimum),
      });
    case "gitUnrecognized":
      return t("failureGitUnrecognized");
    case "gitFailedToStart":
      return t("failureGitFailedToStart", { message: failure.message });
    case "settingsNotSaved":
      return t("failureSettingsNotSaved", { message: failure.message });
    case "projectNotSaved":
      return t("failureProjectNotSaved", { message: failure.message });
    case "folderNotCreated":
      return t("failureFolderNotCreated", { folder: failure.folder, message: failure.message });
    case "repositoryExists":
      return t("failureRepositoryExists", { folder: failure.folder });
    case "repositoryNotCreated":
      return t("failureRepositoryNotCreated", { folder: failure.folder });
    case "repositoryOutsideProject":
      return t("failureRepositoryOutsideProject", { folder: failure.folder });
    case "notARepositoryRoot":
      return t("failureNotARepositoryRoot", { folder: failure.folder });
    case "folderNotFound":
      return t("failureFolderNotFound", { folder: failure.folder });
    case "notInRepository":
      return t("failureNotInRepository", { folder: failure.folder });
    case "changesUnreadable":
      return t("failureChangesUnreadable", { folder: failure.folder });
    case "changesNotWatched":
      return t("failureChangesNotWatched", { folder: failure.folder, message: failure.message });
    case "cloneFailed":
      return t("failureCloneFailed", { url: failure.url, folder: failure.folder });
    case "notificationGone":
      return t("failureNotificationGone");
    case "notificationActionNotOffered":
      return t("failureNotificationActionNotOffered");
  }
}

function showMessage(text: string): void {
  projectView.replaceChildren(line("message", text));
}

/** Reports `text`: in place of the start message, or above the Project this window shows. */
function report(text: string): void {
  if (shownFolder === null) {
    showMessage(text);
  } else {
    projectView.prepend(line("message", text));
  }
}

function line(testId: string, text: string): HTMLParagraphElement {
  const paragraph = document.createElement("p");
  paragraph.dataset.testid = testId;
  paragraph.textContent = text;
  return paragraph;
}

function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (found === null) {
    throw new Error(`missing #${id} in index.html`);
  }
  return found as T;
}
