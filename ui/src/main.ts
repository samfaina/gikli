import { getCurrentWindow } from "@tauri-apps/api/window";
import { confirm, open } from "@tauri-apps/plugin-dialog";

import {
  actOnNotification,
  addRepository,
  answerPrompt,
  cancelOperation,
  cancelPrompt,
  capabilities,
  checkGit,
  clearOperationLog,
  cloneRepository,
  cloneUrlForms,
  createChangelist,
  deleteChangelist,
  dismissNotification,
  createRepository,
  formatVersion,
  ignoreRepository,
  isFailure,
  notifications,
  onCoreEvent,
  openProject,
  operationLog,
  pendingPrompts,
  readChanges,
  removeRepository,
  renameChangelist,
  runningOperations as runningOperationsOfCore,
  setActiveChangelist,
  setChangelistComment,
  setChangeGrouping,
  setCloneSubmodulesRecursively,
  setDiffLocalSideEditable,
  setGitPath,
  setGoToNextFileAfterLastChange,
  setOperationLogSoftWrap,
  setShowIgnoredFiles,
  setUseCredentialHelper,
  settings,
  startupFolder,
  takeFailureAction,
  testGit,
  unwatchChanges,
  watchChanges,
  windowForProject,
  type Capability,
  type Change,
  type ChangeNode,
  type ChangeSet,
  type ChangeStatus,
  type Changelist,
  type CloneUrlForms,
  type ConflictKind,
  type Failure,
  type GitCheck,
  type CoreEvent,
  type ExplainedMessage,
  type Explanation,
  type FailureAction,
  type LongOperation,
  type NextStep,
  type Notification,
  type NotificationAction,
  type OperationLogEntry,
  type Project,
  type Prompt,
  type PromptKind,
  type Repository,
  type RunningOperation,
} from "./core";
import { createDiffView } from "./diff-view";
import { element, hasShortcutKey } from "./dom";
import { createChangeMoves } from "./change-moves";
import { t, type StringKey } from "./strings";

const openButton = element<HTMLButtonElement>("open-folder");
const newRepositoryButton = element<HTMLButtonElement>("new-repository");
const cloneButton = element<HTMLButtonElement>("toggle-clone");
const clonePanel = element<HTMLFormElement>("clone");
const cloneUrlInput = element<HTMLInputElement>("clone-url");
const cloneUrlHttpsInput = element<HTMLInputElement>("clone-url-https");
const cloneUrlSshInput = element<HTMLInputElement>("clone-url-ssh");
const cloneFolderInput = element<HTMLInputElement>("clone-folder");
const browseCloneFolderButton = element<HTMLButtonElement>("browse-clone-folder");
const startCloneButton = element<HTMLButtonElement>("start-clone");
const cloneResult = element<HTMLElement>("clone-result");
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
const promptsView = element<HTMLElement>("prompts");
const useCredentialHelperInput = element<HTMLInputElement>("use-credential-helper");
const cloneSubmodulesInput = element<HTMLInputElement>("clone-submodules-recursively");
const diffLocalSideEditableInput = element<HTMLInputElement>("diff-local-side-editable");
const goToNextFileInput = element<HTMLInputElement>("go-to-next-file-after-last-change");

document.title = t("appTitle");
openButton.textContent = t("openFolder");
openButton.disabled = true;
newRepositoryButton.textContent = t("newRepository");
newRepositoryButton.disabled = true;
cloneButton.textContent = t("cloneRepository");
cloneButton.disabled = true;
clonePanel.setAttribute("aria-label", t("cloneTitle"));
element<HTMLElement>("clone-url-label").textContent = t("cloneUrlLabel");
element<HTMLElement>("clone-url-form-label").textContent = t("cloneUrlFormLabel");
element<HTMLElement>("clone-url-https-label").textContent = t("cloneUrlFormHttps");
element<HTMLElement>("clone-url-ssh-label").textContent = t("cloneUrlFormSsh");
element<HTMLElement>("clone-folder-label").textContent = t("cloneFolderLabel");
element<HTMLElement>("clone-folder-hint").textContent = t("cloneFolderHint");
browseCloneFolderButton.textContent = t("cloneFolderBrowse");
startCloneButton.textContent = t("cloneStart");
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
promptsView.setAttribute("aria-label", t("promptsTitle"));
element<HTMLElement>("use-credential-helper-label").textContent = t("useCredentialHelper");
element<HTMLElement>("use-credential-helper-hint").textContent = t("useCredentialHelperHint");
element<HTMLElement>("clone-submodules-recursively-label").textContent = t("cloneSubmodulesRecursively");
element<HTMLElement>("diff-local-side-editable-label").textContent = t("diffLocalSideEditable");
element<HTMLElement>("go-to-next-file-after-last-change-label").textContent = t("goToNextFileAfterLastChange");

const diffView = createDiffView({
  errorView,
  opened: () => {
    projectView.hidden = true;
  },
  closed: (path) => {
    projectView.hidden = false;
    focusChangeRow(projectView, path);
  },
});

// What the user edited in a diff is saved when the window closes. When it cannot be, the window
// stays, with the reason on screen.
void getCurrentWindow().onCloseRequested(async (event) => {
  if (!(await diffView.saveEdits())) {
    event.preventDefault();
  }
});

saveOnChange(useCredentialHelperInput, async (checked) => (await setUseCredentialHelper(checked)).useCredentialHelper);
saveOnChange(cloneSubmodulesInput, async (checked) => (await setCloneSubmodulesRecursively(checked)).cloneSubmodulesRecursively);
saveOnChange(diffLocalSideEditableInput, async (checked) => {
  const saved = await setDiffLocalSideEditable(checked);
  void diffView.refresh();
  return saved.diffLocalSideEditable;
});
saveOnChange(goToNextFileInput, async (checked) => {
  const saved = await setGoToNextFileAfterLastChange(checked);
  void diffView.refresh();
  return saved.goToNextFileAfterLastChange;
});

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
    report(error);
  } finally {
    softWrapInput.disabled = false;
  }
});

clearOperationLogButton.addEventListener("click", () => void clearOperationLog());

settingsButton.addEventListener("click", async () => {
  const opening = settingsPanel.hidden;
  if (opening) {
    const current = await settings();
    gitPathInput.value = current.gitPath ?? "";
    useCredentialHelperInput.checked = current.useCredentialHelper;
    cloneSubmodulesInput.checked = current.cloneSubmodulesRecursively;
    diffLocalSideEditableInput.checked = current.diffLocalSideEditable;
    goToNextFileInput.checked = current.goToNextFileAfterLastChange;
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
    report(error);
  } finally {
    newRepositoryButton.disabled = false;
  }
});

cloneButton.addEventListener("click", () => {
  const opening = clonePanel.hidden;
  clonePanel.hidden = !opening;
  cloneButton.setAttribute("aria-expanded", String(opening));
  if (opening) {
    cloneUrlInput.focus();
  }
});

cloneUrlInput.addEventListener("input", () => void updateCloneUrlChoice());
cloneFolderInput.addEventListener("input", updateStartClone);
cloneUrlHttpsInput.addEventListener("change", () => chooseCloneUrlForm("https"));
cloneUrlSshInput.addEventListener("change", () => chooseCloneUrlForm("ssh"));

browseCloneFolderButton.addEventListener("click", async () => {
  const folder = await open({ directory: true, title: t("cloneFolderDialogTitle") });
  if (typeof folder === "string") {
    cloneFolderInput.value = folder;
    updateStartClone();
  }
});

clonePanel.addEventListener("submit", async (event) => {
  event.preventDefault();
  cloning = true;
  updateStartClone();
  cloneResult.replaceChildren();
  try {
    const project = await cloneRepository(cloneUrlInput.value.trim(), cloneFolderInput.value.trim());
    clonePanel.reset();
    clonePanel.hidden = true;
    cloneButton.setAttribute("aria-expanded", "false");
    await updateCloneUrlChoice();
    await showInItsWindow(project);
  } catch (error) {
    cloneResult.replaceChildren(errorView(error));
  } finally {
    cloning = false;
    updateStartClone();
  }
});

void start();

/** Saves a setting when the user changes `input`, with what `save` says was saved. When it fails, `input` goes back. */
function saveOnChange(input: HTMLInputElement, save: (checked: boolean) => Promise<boolean>): void {
  input.addEventListener("change", async () => {
    input.disabled = true;
    try {
      input.checked = await save(input.checked);
    } catch (error) {
      input.checked = !input.checked;
      report(error);
    } finally {
      input.disabled = false;
    }
  });
}

/** Whether this window is cloning, which it does one at a time. */
let cloning = false;

/** The forms of the URL in the URL field, once the core has given them. */
let shownCloneUrlForms: CloneUrlForms | null = null;

/** Offers the choice between HTTPS and SSH when the URL has those forms, with the one it is in chosen. */
async function updateCloneUrlChoice(): Promise<void> {
  updateStartClone();
  const url = cloneUrlInput.value.trim();
  const forms = url === "" ? null : await cloneUrlForms(url);
  // The user typed on while the core answered: the answer to what they typed last counts.
  if (url !== cloneUrlInput.value.trim()) {
    return;
  }
  shownCloneUrlForms = forms;
  cloneUrlHttpsInput.disabled = cloneUrlSshInput.disabled = forms === null;
  cloneUrlHttpsInput.checked = forms?.https === url;
  cloneUrlSshInput.checked = forms?.ssh === url;
}

/** Puts the chosen form of the URL in the URL field. */
function chooseCloneUrlForm(form: keyof CloneUrlForms): void {
  if (shownCloneUrlForms !== null) {
    cloneUrlInput.value = shownCloneUrlForms[form];
  }
}

/** A clone takes a URL and a folder, and can start once nothing else is cloning here. */
function updateStartClone(): void {
  startCloneButton.disabled = cloning || cloneUrlInput.value.trim() === "" || cloneFolderInput.value.trim() === "";
}

/** The folder of the Project this window shows, once it shows one. */
let shownFolder: string | null = null;

/** The Repositories shown, by root, each with the function that reads its changes again. */
const shownRepositories = new Map<string, { repository: Repository; refresh: () => void }>();

/** Applies one event from the core to this window. */
function handleCoreEvent(event: CoreEvent): void {
  switch (event.event) {
    case "operationLogged":
      appendLogEntry(event.payload);
      break;
    case "operationLogCleared":
      clearLogEntries(event.payload.before);
      break;
    case "notificationRaised":
      shownNotifications.set(event.payload.id, event.payload);
      showNotifications();
      break;
    case "notificationRemoved":
      removedNotifications.add(event.payload.id);
      shownNotifications.delete(event.payload.id);
      showNotifications();
      break;
    case "operationStarted":
      runningOperations.set(event.payload.id, {
        id: event.payload.id,
        operation: event.payload.operation,
        progress: null,
        hook: null,
        cancellable: event.payload.cancellable,
      });
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
    case "operationHook": {
      const running = runningOperations.get(event.payload.id);
      if (running !== undefined) {
        running.hook = event.payload.hook;
        showOperations();
      }
      break;
    }
    case "operationFinished":
      finishedOperations.add(event.payload.id);
      cancellingOperations.delete(event.payload.id);
      runningOperations.delete(event.payload.id);
      showOperations();
      break;
    case "filesChanged":
      shownRepositories.get(event.payload.repository.root)?.refresh();
      if (diffView.shownRoot() === event.payload.repository.root) {
        void diffView.refresh();
      }
      break;
    case "promptRaised":
      showPrompt(event.payload);
      break;
    case "promptClosed":
      closePrompt(event.payload.id);
      break;
  }
}

async function start(): Promise<void> {
  // Listen first, then read what the core already holds: nothing happens between the two unseen.
  // What an event removed while the reads ran stays removed.
  await onCoreEvent(handleCoreEvent);
  const [current, logged, raised, running, waiting] = await Promise.all([
    settings(),
    operationLog(),
    notifications(),
    runningOperationsOfCore(),
    pendingPrompts(),
  ]);
  waiting.forEach(showPrompt);
  showSoftWrap(current.operationLogSoftWrap);
  mergeLogEntries(logged);
  for (const notification of raised) {
    if (!removedNotifications.has(notification.id) && !shownNotifications.has(notification.id)) {
      shownNotifications.set(notification.id, notification);
    }
  }
  showNotifications();
  for (const operation of running) {
    if (!finishedOperations.has(operation.id) && !runningOperations.has(operation.id)) {
      runningOperations.set(operation.id, { ...operation });
    }
  }
  showOperations();
  showGit(await checkGit());
  // Even without a usable git, a folder under \\wsl$ opens with the git of its distribution;
  // any other folder reports why it cannot open.
  openButton.disabled = false;
  newRepositoryButton.disabled = false;
  cloneButton.disabled = false;
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
    report(error, () => void openFolder(folder));
    return;
  }
  await showInItsWindow(project);
}

/** Shows `project` in this window or in a window of its own. */
async function showInItsWindow(project: Project): Promise<void> {
  try {
    const shown = await windowForProject(project.folder);
    if (shown.kind === "here") {
      showProject(project);
    }
  } catch (error) {
    report(error);
  }
}

/** Shows `project` in this window: the Repositories gikli offers to add, the ones that broke, the ignored ones, and the changes of the others. */
function showProject(project: Project): void {
  // The file of another Project is no longer in view: its edits are saved, as when the user closes it.
  if (project.folder !== shownFolder) {
    void diffView.close();
  }
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
          report(error);
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
      projectView.prepend(errorView(error));
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
  // Outside the rows, which are drawn again on every read: what the user is typing stays.
  const form = changelistForm(() => void refreshChanges());
  const changes = document.createElement("div");
  changes.dataset.testid = "changes";
  const runChangelistAction = async (action: () => Promise<unknown>): Promise<void> => {
    try {
      await action();
      void refreshChanges();
    } catch (error) {
      changes.prepend(errorView(error));
    }
  };
  const moves = createChangeMoves({
    repository,
    changes,
    run: (action) => void runChangelistAction(action),
    moved: () => void refreshChanges(),
    describeError,
  });
  const newChangelist = changelistButton("new-changelist", t("newChangelist"), () =>
    form.open({
      title: t("changelistFormNewTitle"),
      submit: t("changelistFormCreate"),
      name: "",
      description: "",
      comment: null,
      save: ({ name, description }) => createChangelist(repository, name, description),
    }),
  );
  header.append(title, refresh, newChangelist);
  const watching = document.createElement("div");
  const features = document.createElement("div");
  section.append(
    line("repository-root", t("repositoryRoot", { root: repository.root })),
    header,
    form.view,
    moves.form,
    moves.menu,
    watching,
    changes,
    features,
  );
  void showUnavailableFeatures(repository, features);

  const actions: ChangelistActions = {
    setActive: (list) => void runChangelistAction(() => setActiveChangelist(repository, list.id)),
    delete: (list) => void runChangelistAction(() => deleteChangelist(repository, list.id)),
    rename: (list) =>
      form.open({
        title: t("changelistFormRenameTitle", { name: list.name }),
        submit: t("changelistFormRename"),
        name: list.name,
        description: null,
        comment: null,
        save: ({ name }) => renameChangelist(repository, list.id, name),
      }),
    comment: (list) =>
      form.open({
        title: t("changelistFormCommentTitle", { name: list.name }),
        submit: t("changelistFormSaveComment"),
        name: null,
        description: null,
        comment: list.comment ?? "",
        save: ({ comment }) => setChangelistComment(repository, list.id, comment),
      }),
  };
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
          const changeSet = await readChanges(repository);
          // The row the user was on stays theirs when the rows are drawn again.
          const focused = changes.contains(document.activeElement) ? (document.activeElement as HTMLElement).dataset.path : undefined;
          changes.replaceChildren(changeList(changeSet, actions));
          moves.drawn(changeSet);
          if (focused !== undefined) {
            focusChangeRow(changes, focused);
          }
        } catch (error) {
          changes.replaceChildren(errorView(error, () => void refreshChanges()));
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

/** Puts the focus on the row in `container` of the file at `path`, if it has one. */
function focusChangeRow(container: HTMLElement, path: string): void {
  for (const row of container.querySelectorAll<HTMLElement>('[data-testid="change"]')) {
    if (row.dataset.path === path) {
      row.focus();
    }
  }
}

/** The Changelists of a Repository, each with its changes, then Unversioned Files and the ignored files when there are any. */
function changeList(changeSet: ChangeSet, actions: ChangelistActions): HTMLElement {
  const view = document.createElement("div");
  const { repository, unversionedFiles, ignoredFiles } = changeSet;
  for (const list of changeSet.changelists) {
    view.append(changelistView(repository, list, actions));
  }
  if (changeSet.changes.length === 0) {
    view.append(line("message", t("noChanges")));
  }
  if (unversionedFiles.nodes.length > 0) {
    view.append(changeGroupView(repository, "unversioned-files", [groupName(t("unversionedFiles"))], unversionedFiles.nodes));
  }
  if (ignoredFiles.nodes.length > 0) {
    view.append(changeGroupView(repository, "ignored-files", [groupName(t("ignoredFiles"))], ignoredFiles.nodes));
  }
  return view;
}

/** What the user can do to a Changelist in the changes view. */
interface ChangelistActions {
  setActive: (list: Changelist) => void;
  rename: (list: Changelist) => void;
  comment: (list: Changelist) => void;
  delete: (list: Changelist) => void;
}

/** A button that manages Changelists. */
function changelistButton(testId: string, text: string, act: () => void): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.dataset.testid = testId;
  button.textContent = text;
  button.addEventListener("click", act);
  return button;
}

/** A Changelist and its changes. Any other than the Active changelist offers to become it, and to be deleted. */
function changelistView(repository: Repository, list: Changelist, actions: ChangelistActions): HTMLElement {
  const name = groupName(list.name);
  name.dataset.testid = "changelist-name";
  const header: HTMLElement[] = [name];
  if (list.description !== null) {
    const description = document.createElement("span");
    description.className = "changelist-description";
    description.dataset.testid = "changelist-description";
    description.textContent = list.description;
    header.push(description);
  }
  if (list.comment !== null) {
    const comment = document.createElement("span");
    comment.className = "changelist-description changelist-comment";
    comment.dataset.testid = "changelist-comment";
    comment.textContent = list.comment;
    header.push(comment);
  }
  if (list.active) {
    const active = document.createElement("span");
    active.className = "changelist-active";
    active.textContent = t("activeChangelist");
    header.push(active);
  } else {
    header.push(changelistButton("set-active-changelist", t("setActiveChangelist"), () => actions.setActive(list)));
  }
  header.push(changelistButton("rename-changelist", t("renameChangelist"), () => actions.rename(list)));
  header.push(changelistButton("comment-changelist", t("commentChangelist"), () => actions.comment(list)));
  if (!list.active) {
    header.push(changelistButton("delete-changelist", t("deleteChangelist"), () => actions.delete(list)));
  }
  const view = changeGroupView(repository, "changelist", header, list.group.nodes);
  view.dataset.active = String(list.active);
  view.dataset.name = list.name;
  view.dataset.id = list.id;
  return view;
}

/** What the user wrote in the Changelist form. */
interface ChangelistFormAnswers {
  name: string;
  description: string;
  comment: string;
}

/** What the Changelist form asks, and what saves the answer. A field that is null is not asked. */
interface ChangelistFormRequest {
  title: string;
  submit: string;
  name: string | null;
  description: string | null;
  comment: string | null;
  save: (answers: ChangelistFormAnswers) => Promise<unknown>;
}

/** A form about a Changelist, hidden until something opens it. Once saved it closes and calls `saved`; when it cannot save, it shows why. */
function changelistForm(saved: () => void): { view: HTMLFormElement; open: (request: ChangelistFormRequest) => void } {
  const view = document.createElement("form");
  view.className = "changelist-form";
  view.dataset.testid = "changelist-form";
  view.hidden = true;
  const title = document.createElement("h3");
  const field = <T extends HTMLInputElement | HTMLTextAreaElement>(input: T, testId: string, text: string): { label: HTMLLabelElement; input: T } => {
    const label = document.createElement("label");
    input.dataset.testid = testId;
    input.autocomplete = "off";
    label.append(document.createTextNode(text), input);
    return { label, input };
  };
  const name = field(document.createElement("input"), "changelist-form-name", t("changelistFormName"));
  const description = field(document.createElement("input"), "changelist-form-description", t("changelistFormDescription"));
  const comment = field(document.createElement("textarea"), "changelist-form-comment", t("changelistFormComment"));
  const message = document.createElement("p");
  message.dataset.testid = "changelist-form-message";
  const submit = document.createElement("button");
  submit.type = "submit";
  submit.dataset.testid = "changelist-form-submit";
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.dataset.testid = "changelist-form-cancel";
  cancel.textContent = t("changelistFormCancel");
  const buttons = document.createElement("div");
  buttons.className = "notification-actions";
  buttons.append(submit, cancel);
  view.append(title, name.label, description.label, comment.label, message, buttons);

  let request: ChangelistFormRequest | null = null;
  const close = (): void => {
    request = null;
    view.hidden = true;
  };
  cancel.addEventListener("click", close);
  view.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (request === null) {
      return;
    }
    submit.disabled = true;
    message.textContent = "";
    try {
      await request.save({ name: name.input.value, description: description.input.value, comment: comment.input.value });
      close();
      saved();
    } catch (error) {
      message.textContent = describeError(error);
    } finally {
      submit.disabled = false;
    }
  });
  return {
    view,
    open: (opened) => {
      request = opened;
      title.textContent = opened.title;
      submit.textContent = opened.submit;
      const asked = [
        { field: name, value: opened.name },
        { field: description, value: opened.description },
        { field: comment, value: opened.comment },
      ];
      for (const { field: { label, input }, value } of asked) {
        label.hidden = value === null;
        input.value = value ?? "";
      }
      message.textContent = "";
      view.hidden = false;
      const first = asked.find(({ value }) => value !== null)?.field.input;
      first?.select();
      first?.focus();
    },
  };
}

function groupName(text: string): HTMLSpanElement {
  const name = document.createElement("span");
  name.className = "change-group-name";
  name.textContent = text;
  return name;
}

/** Changes the view shows together, under a `header`. */
function changeGroupView(repository: Repository, testId: string, header: HTMLElement[], nodes: ChangeNode[]): HTMLElement {
  const view = document.createElement("section");
  view.className = "change-group";
  view.dataset.testid = testId;
  const title = document.createElement("div");
  title.className = "change-group-header";
  title.append(...header);
  view.append(title, nodeList(repository, nodes, ""));
  return view;
}

/** The rows of one folder of `repository`, whose path is `folder`, or of a whole group when `folder` is "". */
function nodeList(repository: Repository, nodes: ChangeNode[], folder: string): HTMLUListElement {
  const list = document.createElement("ul");
  list.className = "changes";
  for (const node of nodes) {
    list.append(node.kind === "folder" ? folderItem(repository, node) : changeItem(repository, node, folder));
  }
  return list;
}

/** A folder and its rows. A folder that holds changes, not only ignored files, is highlighted. */
function folderItem(repository: Repository, folder: Extract<ChangeNode, { kind: "folder" }>): HTMLLIElement {
  const item = document.createElement("li");
  item.className = "change-folder";
  item.dataset.testid = "change-folder";
  item.classList.toggle("has-changes", folder.hasChanges);
  const name = document.createElement("span");
  name.className = "folder-name";
  name.textContent = folder.path.slice(folder.path.lastIndexOf("/") + 1);
  item.append(name, nodeList(repository, folder.children, folder.path));
  return item;
}

/**
 * A changed file of `repository`, shown by its path relative to `folder`. A double-click opens its
 * diff, as do Enter and Ctrl+D, or Cmd+D on a Mac, on the row. An ignored file is no change to review.
 */
function changeItem(repository: Repository, change: Change, folder: string): HTMLLIElement {
  const item = document.createElement("li");
  item.className = "change";
  item.dataset.testid = "change";
  item.dataset.status = change.status;
  item.dataset.path = change.path;
  if (change.status !== "ignored") {
    item.tabIndex = 0;
    item.addEventListener("dblclick", () => void diffView.open(repository, change.path));
    item.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || (hasShortcutKey(event) && event.key.toLowerCase() === "d")) {
        event.preventDefault();
        void diffView.open(repository, change.path);
      }
    });
  }
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

/** How many operation log entries a window shows, as many as the core keeps. */
const SHOWN_LOG_ENTRIES = 1000;

/** The ids of the operation log entries this window shows, oldest first, matching the views in {@link operationLogEntries}. */
const shownLogIds: number[] = [];

/** Entries with a lower id were cleared: a late read of the log must not bring them back. */
let logClearedBefore = 0;

/** Adds a new entry at the end of the operation log view, unless the view already shows it. */
function appendLogEntry(entry: OperationLogEntry): void {
  const last = shownLogIds[shownLogIds.length - 1];
  if (entry.id < logClearedBefore || (last !== undefined && entry.id <= last)) {
    return;
  }
  const atEnd =
    operationLogEntries.scrollTop + operationLogEntries.clientHeight >= operationLogEntries.scrollHeight - 4;
  if (shownLogIds.length === 0) {
    operationLogEntries.replaceChildren();
  }
  shownLogIds.push(entry.id);
  operationLogEntries.append(logEntryView(entry));
  while (shownLogIds.length > SHOWN_LOG_ENTRIES) {
    shownLogIds.shift();
    operationLogEntries.firstElementChild?.remove();
  }
  // Follows new commands, unless the user scrolled up to read older ones.
  if (atEnd) {
    operationLogEntries.scrollTop = operationLogEntries.scrollHeight;
  }
}

/** Puts `entries`, read from the core, before the entries that arrived as events meanwhile. */
function mergeLogEntries(entries: OperationLogEntry[]): void {
  const first = shownLogIds[0];
  const older = entries.filter((entry) => entry.id >= logClearedBefore && (first === undefined || entry.id < first));
  const room = Math.max(0, SHOWN_LOG_ENTRIES - shownLogIds.length);
  const kept = older.slice(Math.max(0, older.length - room));
  if (kept.length > 0) {
    if (shownLogIds.length === 0) {
      operationLogEntries.replaceChildren();
    }
    shownLogIds.unshift(...kept.map((entry) => entry.id));
    operationLogEntries.prepend(...kept.map(logEntryView));
    operationLogEntries.scrollTop = operationLogEntries.scrollHeight;
  }
  if (shownLogIds.length === 0) {
    showLogEmpty();
  }
}

/** Drops every entry whose id is below `before`, as the user cleared the log. */
function clearLogEntries(before: number): void {
  logClearedBefore = Math.max(logClearedBefore, before);
  while (shownLogIds.length > 0 && shownLogIds[0]! < logClearedBefore) {
    shownLogIds.shift();
    operationLogEntries.firstElementChild?.remove();
  }
  if (shownLogIds.length === 0) {
    showLogEmpty();
  }
}

function showLogEmpty(): void {
  operationLogEntries.replaceChildren(line("operation-log-empty", t("operationLogEmpty")));
}

function logEntryView(entry: OperationLogEntry): HTMLElement {
  const view = document.createElement("div");
  view.className = "log-entry";
  view.dataset.testid = "log-entry";
  const failed = !entry.cancelled && (entry.error !== null || entry.exitCode !== 0);
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
  if (entry.cancelled) {
    const result = document.createElement("pre");
    result.className = "log-result";
    result.textContent = t("operationLogCancelled");
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

/** The notifications removed since this window started listening, so a late read does not bring them back. */
const removedNotifications = new Set<number>();

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
        switch (outcome.outcome) {
          case "projectChanged":
            showProject(outcome.payload);
            break;
        }
      } catch (error) {
        report(error);
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
    case "hookNotFinished":
      return t("notificationHookNotFinished", { root: notification.kind.repository.root, hook: notification.kind.hook });
  }
}

const ACTION_LABELS: Record<NotificationAction["action"], StringKey> = {
  addRepository: "addRepository",
  ignoreRepository: "ignoreRepository",
  removeRepository: "removeRepository",
};

/** The prompts this window shows, by id. A view stays in place while others come and go, so what the user is typing survives. */
const shownPrompts = new Map<number, HTMLElement>();

/** The prompts closed since this window started listening, so a late read does not bring them back. */
const closedPrompts = new Set<number>();

function showPrompt(prompt: Prompt): void {
  if (closedPrompts.has(prompt.id) || shownPrompts.has(prompt.id)) {
    return;
  }
  const view = promptView(prompt);
  shownPrompts.set(prompt.id, view);
  promptsView.append(view);
  promptsView.hidden = false;
  view.querySelector<HTMLElement>("input, button")?.focus();
}

function closePrompt(id: number): void {
  closedPrompts.add(id);
  shownPrompts.get(id)?.remove();
  shownPrompts.delete(id);
  promptsView.hidden = shownPrompts.size === 0;
}

const PROMPT_TITLES: Record<PromptKind, StringKey> = {
  username: "promptUsername",
  password: "promptPassword",
  passphrase: "promptPassphrase",
  hostKey: "promptHostKey",
  other: "promptOther",
};

/** A prompt with what it takes to answer it. The answer goes to the core and stays nowhere here. */
function promptView(prompt: Prompt): HTMLElement {
  const view = document.createElement("form");
  view.className = "prompt";
  view.dataset.testid = "prompt";
  view.dataset.kind = prompt.kind;
  const title = document.createElement("h2");
  title.textContent = t(PROMPT_TITLES[prompt.kind]);
  const question = document.createElement("p");
  question.className = "prompt-text";
  question.dataset.testid = "prompt-text";
  question.textContent = prompt.text;
  const respond = async (response: () => Promise<void>): Promise<void> => {
    view.querySelectorAll<HTMLButtonElement | HTMLInputElement>("button, input").forEach((control) => {
      control.disabled = true;
    });
    try {
      await response();
    } catch (error) {
      closePrompt(prompt.id);
      report(error);
    }
  };
  const button = (testId: string, text: StringKey, response: () => Promise<void>): HTMLButtonElement => {
    const made = document.createElement("button");
    made.type = "button";
    made.dataset.testid = testId;
    made.textContent = t(text);
    made.addEventListener("click", () => void respond(response));
    return made;
  };
  const actions = document.createElement("div");
  actions.className = "notification-actions";
  if (prompt.kind === "hostKey") {
    // Not trusting comes first, so it has the focus: a stray Enter must not trust a key.
    actions.append(
      button("prompt-reject", "promptRejectHostKey", () => answerPrompt(prompt.id, "no")),
      button("prompt-accept", "promptAcceptHostKey", () => answerPrompt(prompt.id, "yes")),
    );
    view.append(title, question, actions);
    return view;
  }
  const input = document.createElement("input");
  input.type = prompt.kind === "username" ? "text" : "password";
  input.dataset.testid = "prompt-answer";
  input.autocomplete = "off";
  input.spellcheck = false;
  input.setAttribute("aria-label", prompt.text);
  view.addEventListener("submit", (event) => {
    event.preventDefault();
    void respond(() => answerPrompt(prompt.id, input.value));
  });
  const submit = document.createElement("button");
  submit.type = "submit";
  submit.dataset.testid = "prompt-submit";
  submit.textContent = t("promptAnswer");
  actions.append(submit, button("prompt-cancel", "promptCancel", () => cancelPrompt(prompt.id)));
  const notKept = document.createElement("p");
  notKept.className = "hint";
  notKept.textContent = t("promptNotKept");
  view.append(title, question, input, notKept, actions);
  return view;
}

/** The long operations running, by id, with the last progress each reported. Every window shows them all. */
const runningOperations = new Map<number, RunningOperation>();

/** The long operations finished since this window started listening, so a late read does not show them again. */
const finishedOperations = new Set<number>();

/** The long operations this window asked to cancel and that have not finished yet. */
const cancellingOperations = new Set<number>();

/** Shows each running long operation with its progress, the hook it waits on and the way to cancel it, in the status area, without blocking the views. */
function showOperations(): void {
  operationsView.hidden = runningOperations.size === 0;
  operationsView.replaceChildren(
    ...[...runningOperations.values()].map(({ id, operation, progress, hook, cancellable }) => {
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
      if (hook !== null) {
        const waiting = document.createElement("span");
        waiting.dataset.testid = "operation-hook";
        waiting.textContent = t("operationWaitingOnHook", { hook });
        view.append(waiting);
      }
      if (cancellable) {
        const cancel = document.createElement("button");
        cancel.type = "button";
        cancel.dataset.testid = "cancel-operation";
        cancel.textContent = t("operationCancel");
        cancel.disabled = cancellingOperations.has(id);
        cancel.addEventListener("click", () => {
          cancellingOperations.add(id);
          showOperations();
          void cancelOperation(id).catch(report);
        });
        view.append(cancel);
      }
      return view;
    }),
  );
}

function describeOperation(operation: LongOperation): string {
  switch (operation.kind) {
    case "clone":
      return t("operationClone", { url: operation.url });
    case "addFiles":
      return t("operationAddFiles", { root: operation.repository.root });
  }
}

/**
 * What to show for an error from the core. An explained failure offers what gikli can do about it;
 * `retry`, if given, runs once it is done.
 */
function errorView(error: unknown, retry?: () => void): HTMLElement {
  if (isFailure(error) && error.kind === "explained") {
    return explanationView(error.explanation, retry);
  }
  if (isFailure(error) && error.kind === "operationCancelled" && error.leftBehind !== null) {
    const view = document.createElement("div");
    view.append(line("message", t("failureOperationCancelled")), explanationView(error.leftBehind, retry));
    return view;
  }
  return line("message", describeError(error));
}

function explanationView(explanation: Explanation, retry?: () => void): HTMLElement {
  const view = document.createElement("div");
  view.className = "failure-explanation";
  view.dataset.testid = "failure-explanation";
  view.dataset.message = explanation.message.kind;
  const message = document.createElement("p");
  message.textContent = describeExplainedMessage(explanation.message);
  view.append(message);
  if (explanation.message.kind === "hookFailed") {
    const output = document.createElement("pre");
    output.dataset.testid = "hook-output";
    output.textContent = explanation.message.output;
    view.append(output);
  }
  const nextStep = document.createElement("p");
  nextStep.textContent = t(NEXT_STEPS[explanation.nextStep]);
  view.append(nextStep);
  const buttons = explanation.actions.map((action) => {
    const button = document.createElement("button");
    button.type = "button";
    button.dataset.testid = "failure-action";
    button.dataset.action = action.action;
    button.textContent = t(FAILURE_ACTION_LABELS[action.action]);
    button.addEventListener("click", async () => {
      if (!(await confirm(describeConfirmation(action), { kind: "warning" }))) {
        return;
      }
      buttons.forEach((other) => (other.disabled = true));
      try {
        await takeFailureAction(action);
        view.replaceWith(line("message", t("failureActionDone")));
        retry?.();
      } catch (error) {
        view.replaceWith(errorView(error, retry));
      }
    });
    return button;
  });
  if (buttons.length > 0) {
    const actions = document.createElement("div");
    actions.className = "notification-actions";
    actions.append(...buttons);
    view.append(actions);
  }
  return view;
}

function describeExplainedMessage(message: ExplainedMessage): string {
  switch (message.kind) {
    case "hookFailed":
      return t("failureHookFailed", { hook: message.hook });
    case "dubiousOwnership":
      return t("failureDubiousOwnership", { folder: message.folder });
    case "indexLocked":
      return t("failureIndexLocked", { lock: message.lock });
    case "longPaths":
      return t("failureLongPaths");
    case "authenticationFailed":
      return t("failureAuthenticationFailed");
    case "hostKeyUnknown":
      return t("failureHostKeyUnknown");
    case "hostKeyChanged":
      return t("failureHostKeyChanged");
    case "serverUnreachable":
      return t("failureServerUnreachable");
    case "cloneTargetNotEmpty":
      return t("failureCloneTargetNotEmpty", { folder: message.folder });
    case "submodulesNotCloned": {
      const notCloned = t("failureSubmodulesNotCloned", { folder: message.folder });
      return message.cause === null
        ? notCloned
        : t("failureSubmodulesNotClonedCause", { message: notCloned, cause: describeExplainedMessage(message.cause) });
    }
  }
}

const NEXT_STEPS: Record<NextStep, StringKey> = {
  fixHook: "nextStepFixHook",
  trustFolder: "nextStepTrustFolder",
  removeIndexLock: "nextStepRemoveIndexLock",
  waitForOtherGitOperation: "nextStepWaitForOtherGitOperation",
  enableLongPaths: "nextStepEnableLongPaths",
  checkCredentials: "nextStepCheckCredentials",
  verifyHostKey: "nextStepVerifyHostKey",
  checkChangedHostKey: "nextStepCheckChangedHostKey",
  checkConnection: "nextStepCheckConnection",
  chooseEmptyFolder: "nextStepChooseEmptyFolder",
  useOrDeleteClone: "nextStepUseOrDeleteClone",
};

const FAILURE_ACTION_LABELS: Record<FailureAction["action"], StringKey> = {
  trustFolder: "trustFolder",
  removeIndexLock: "removeIndexLock",
};

function describeConfirmation(action: FailureAction): string {
  switch (action.action) {
    case "trustFolder":
      return t("confirmTrustFolder", { folder: action.folder });
    case "removeIndexLock":
      return t("confirmRemoveIndexLock", { repository: action.repository.root });
  }
}

/** The message for an error from the core: a {@link Failure}, or anything unexpected. */
function describeError(error: unknown): string {
  return isFailure(error) ? describeFailure(error) : t("unexpectedError", { message: String(error) });
}

function describeFailure(failure: Failure): string {
  switch (failure.kind) {
    case "explained":
      return t("failureExplained", {
        message: describeExplainedMessage(failure.explanation.message),
        nextStep: t(NEXT_STEPS[failure.explanation.nextStep]),
      });
    case "operationCancelled":
      return t("failureOperationCancelled");
    case "cancelledCloneNotRemoved":
      return t("failureCancelledCloneNotRemoved", { folder: failure.folder, message: failure.message });
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
    case "notAWorkingTree":
      return t("failureNotAWorkingTree", { folder: failure.folder });
    case "projectNotOpened":
      return t("failureProjectNotOpened", { folder: failure.folder });
    case "changesUnreadable":
      return t("failureChangesUnreadable", { folder: failure.folder });
    case "diffUnreadable":
      return t("failureDiffUnreadable", { path: failure.path });
    case "localFileUnreadable":
      return t("failureLocalFileUnreadable", { path: failure.path, message: failure.message });
    case "localSideNotEditable":
      return t("failureLocalSideNotEditable", { path: failure.path });
    case "localFileChanged":
      return t("failureLocalFileChanged", { path: failure.path });
    case "localFileNotSaved":
      return t("failureLocalFileNotSaved", { path: failure.path, message: failure.message });
    case "changelistGone":
      return t("failureChangelistGone");
    case "changelistNameEmpty":
      return t("failureChangelistNameEmpty");
    case "changelistNameTaken":
      return t("failureChangelistNameTaken", { name: failure.name });
    case "activeChangelistNotDeletable":
      return t("failureActiveChangelistNotDeletable");
    case "changelistsUnreadable":
      return t("failureChangelistsUnreadable", { file: failure.file, message: failure.message });
    case "activeChangelistMissing":
      return t("failureActiveChangelistMissing", { file: failure.file });
    case "changelistsFromNewerGikli":
      return t("failureChangelistsFromNewerGikli", { file: failure.file });
    case "changelistsNotSaved":
      return t("failureChangelistsNotSaved", { file: failure.file, message: failure.message });
    case "changesNotWatched":
      return t("failureChangesNotWatched", { folder: failure.folder, message: failure.message });
    case "cloneFailed":
      return t("failureCloneFailed", { url: failure.url, folder: failure.folder });
    case "filesNotAdded":
      return t("failureFilesNotAdded", { folder: failure.folder });
    case "indexLockNotRemoved":
      return t("failureIndexLockNotRemoved", { lock: failure.lock, message: failure.message });
    case "folderNotTrusted":
      return t("failureFolderNotTrusted", { folder: failure.folder });
    case "notificationGone":
      return t("failureNotificationGone");
    case "notificationActionNotOffered":
      return t("failureNotificationActionNotOffered");
    case "promptGone":
      return t("failurePromptGone");
  }
}

function showMessage(text: string): void {
  projectView.replaceChildren(line("message", text));
}

/**
 * Reports an error from the core: in place of the start message, or above the Project this window shows.
 * `retry`, if given, runs once the user has done what an explained failure offers.
 */
function report(error: unknown, retry?: () => void): void {
  if (shownFolder === null) {
    projectView.replaceChildren(errorView(error, retry));
  } else {
    projectView.prepend(errorView(error, retry));
  }
}

function line(testId: string, text: string): HTMLParagraphElement {
  const paragraph = document.createElement("p");
  paragraph.dataset.testid = testId;
  paragraph.textContent = text;
  return paragraph;
}

