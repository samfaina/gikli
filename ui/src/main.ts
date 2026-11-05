import { open } from "@tauri-apps/plugin-dialog";

import {
  checkGit,
  formatVersion,
  isFailure,
  openProject,
  startupFolder,
  type Failure,
  type GitCheck,
  type Project,
} from "./core";
import { t } from "./strings";

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
    ...(project.repositories.length === 0
      ? [line("not-a-repository", t("notARepository"))]
      : project.repositories.map((repository) =>
          line("repository-root", t("repositoryRoot", { root: repository.root })),
        )),
  );
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
    case "folderNotFound":
      return t("failureFolderNotFound", { folder: failure.folder });
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
