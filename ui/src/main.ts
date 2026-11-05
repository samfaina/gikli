import { open } from "@tauri-apps/plugin-dialog";

import {
  capabilities,
  checkGit,
  formatVersion,
  isFailure,
  onCoreEvent,
  openProject,
  readChanges,
  setGitPath,
  settings,
  startupFolder,
  testGit,
  type Capability,
  type ChangeSet,
  type ChangeStatus,
  type Failure,
  type GitCheck,
  type Project,
  type Repository,
} from "./core";
import { t, type StringKey } from "./strings";

const openButton = element<HTMLButtonElement>("open-folder");
const gitStatus = element<HTMLElement>("git-status");
const projectView = element<HTMLElement>("project");
const settingsButton = element<HTMLButtonElement>("toggle-settings");
const settingsPanel = element<HTMLElement>("settings");
const gitPathInput = element<HTMLInputElement>("git-path");
const testGitButton = element<HTMLButtonElement>("test-git");
const saveGitPathButton = element<HTMLButtonElement>("save-git-path");
const gitTestResult = element<HTMLElement>("git-test-result");

document.title = t("appTitle");
openButton.textContent = t("openFolder");
openButton.disabled = true;
gitStatus.textContent = t("checkingGit");
showMessage(t("noProject"));
settingsButton.textContent = t("settings");
element<HTMLElement>("git-path-label").textContent = t("gitPathLabel");
element<HTMLElement>("git-path-hint").textContent = t("gitPathHint");
gitPathInput.placeholder = t("gitPathPlaceholder");
testGitButton.textContent = t("testGit");
saveGitPathButton.textContent = t("saveGitPath");

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
    gitTestResult.textContent = isFailure(error) ? describeFailure(error) : t("unexpectedError", { message: String(error) });
  } finally {
    saveGitPathButton.disabled = false;
  }
});

openButton.addEventListener("click", async () => {
  const folder = await open({ directory: true, title: t("openFolderDialogTitle") });
  if (typeof folder === "string") {
    await showProject(folder);
  }
});

void start();

// The operation log view does not exist yet; until then, git commands show in the devtools console.
void onCoreEvent((event) => {
  if (event.event === "operationLogged") {
    console.debug("git", ...event.payload.args, "→", event.payload.exitCode);
  }
});

async function start(): Promise<void> {
  showGit(await checkGit());
  // Even without a usable git, a folder under \\wsl$ opens with the git of its distribution;
  // any other folder reports why it cannot open.
  openButton.disabled = false;
  const folder = await startupFolder();
  if (folder !== null) {
    await showProject(folder);
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

async function showProject(folder: string): Promise<void> {
  let project: Project;
  try {
    project = await openProject(folder);
  } catch (error) {
    showMessage(isFailure(error) ? describeFailure(error) : t("unexpectedError", { message: String(error) }));
    return;
  }
  projectView.replaceChildren(
    line("project-folder", t("projectFolder", { folder: project.folder })),
    ...project.repositories.map(repositoryView),
  );
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
  const features = document.createElement("div");
  section.append(line("repository-root", t("repositoryRoot", { root: repository.root })), header, changes, features);
  void showUnavailableFeatures(repository, features);

  const refreshChanges = async (): Promise<void> => {
    refresh.disabled = true;
    changes.replaceChildren(line("message", t("readingChanges")));
    try {
      changes.replaceChildren(changeList(await readChanges(repository)));
    } catch (error) {
      changes.replaceChildren(
        line("message", isFailure(error) ? describeFailure(error) : t("unexpectedError", { message: String(error) })),
      );
    } finally {
      refresh.disabled = false;
    }
  };
  refresh.addEventListener("click", () => void refreshChanges());
  void refreshChanges();
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
  const list = document.createElement("ul");
  list.className = "changes";
  for (const change of changeSet.changes) {
    const item = document.createElement("li");
    item.dataset.testid = "change";
    item.dataset.status = change.status;
    const status = document.createElement("span");
    status.className = "change-status";
    status.textContent = t(STATUS_LABELS[change.status]);
    const path = document.createElement("span");
    path.className = "change-path";
    path.dataset.testid = "change-path";
    path.textContent = change.path;
    item.append(status, path);
    list.append(item);
  }
  return list;
}

const STATUS_LABELS: Record<ChangeStatus, StringKey> = {
  modified: "statusModified",
  added: "statusAdded",
  deleted: "statusDeleted",
  untracked: "statusUntracked",
};

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
    case "folderNotFound":
      return t("failureFolderNotFound", { folder: failure.folder });
    case "notInRepository":
      return t("failureNotInRepository", { folder: failure.folder });
    case "changesUnreadable":
      return t("failureChangesUnreadable", { folder: failure.folder });
  }
}

function showMessage(text: string): void {
  projectView.replaceChildren(line("message", text));
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
