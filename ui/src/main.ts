import { open } from "@tauri-apps/plugin-dialog";

import {
  checkGit,
  formatVersion,
  isFailure,
  onCoreEvent,
  openProject,
  readChanges,
  startupFolder,
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

document.title = t("appTitle");
openButton.textContent = t("openFolder");
openButton.disabled = true;
gitStatus.textContent = t("checkingGit");
showMessage(t("noProject"));

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
  const git = await checkGit();
  showGit(git);
  if (git.status !== "ready") {
    return;
  }
  openButton.disabled = false;
  const folder = await startupFolder();
  if (folder !== null) {
    await showProject(folder);
  }
}

function showGit(git: GitCheck): void {
  gitStatus.classList.toggle("warning", git.status !== "ready");
  switch (git.status) {
    case "ready":
      gitStatus.textContent = t("gitReady", { version: formatVersion(git.version) });
      break;
    case "tooOld":
      gitStatus.textContent = t("gitTooOld", {
        version: formatVersion(git.version),
        minimum: formatVersion(git.minimum),
        executable: git.executable,
      });
      break;
    case "unrecognized":
      gitStatus.textContent = t("gitUnrecognized", { executable: git.executable });
      break;
    case "notFound":
      gitStatus.textContent = t("gitNotFound", { minimum: formatVersion(git.minimum) });
      break;
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
  title.textContent = t("changesTitle");
  const refresh = document.createElement("button");
  refresh.type = "button";
  refresh.dataset.testid = "refresh-changes";
  refresh.textContent = t("refreshChanges");
  header.append(title, refresh);
  const changes = document.createElement("div");
  changes.dataset.testid = "changes";
  section.append(line("repository-root", t("repositoryRoot", { root: repository.root })), header, changes);

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
    case "folderNotFound":
      return t("failureFolderNotFound", { folder: failure.folder });
    case "notInRepository":
      return t("failureNotInRepository", { folder: failure.folder });
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
