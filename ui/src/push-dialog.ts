// The push dialog: the Repository, its current branch and the remote branch it goes to, as a tree,
// with the commits the push would send and what each one changed. The user can change or create the
// target, choose which tags go along, and define a remote when there is none. The core reads and
// pushes; this draws it.

import type { MergeView } from "@codemirror/merge";

import {
  addRemote,
  commitChanges,
  push,
  pushPlan,
  revisionFileDiff,
  type ChangeNode,
  type PushCommit,
  type PushPlan,
  type PushTags,
  type PushTarget,
  type Repository,
} from "./core";
import { createDiffPreview } from "./diff-view";
import { element, folderName } from "./dom";
import { note, SHORT_HASH_LENGTH } from "./log-details";
import { t } from "./strings";

/** What the push dialog needs from the window around it. */
export interface PushDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The dialog took the place of the view the user opened it from. */
  opened(): void;
  /** The view the user opened it from is back. */
  closed(): void;
  /** The push or the new remote changed the branches of `repository`. */
  branchesMoved(repository: Repository): void;
}

export interface PushDialog {
  /** Shows what a push of the current branch of `repository` would send. */
  open(repository: Repository): void;
  /** Goes back to the view the user opened it from. */
  close(): void;
}

/** How long the target waits for the user to stop typing before the commits are read again. */
const TARGET_TYPING_PAUSE_MS = 300;

const dates = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });

export function createPushDialog(host: PushDialogHost): PushDialog {
  const view = element<HTMLElement>("push-dialog");
  const title = element<HTMLElement>("push-title");
  const closeButton = element<HTMLButtonElement>("push-close");
  const repositoryName = element<HTMLElement>("push-repository");
  const branchName = element<HTMLElement>("push-branch");
  const targetView = element<HTMLElement>("push-target");
  const remoteSelect = element<HTMLSelectElement>("push-remote");
  const targetBranch = element<HTMLInputElement>("push-target-branch");
  const newBadge = element<HTMLElement>("push-new-branch");
  const defineView = element<HTMLElement>("push-define-remote");
  const defineName = element<HTMLInputElement>("push-define-name");
  const defineUrl = element<HTMLInputElement>("push-define-url");
  const defineButton = element<HTMLButtonElement>("push-define");
  const message = element<HTMLElement>("push-message");
  const commitList = element<HTMLElement>("push-commits");
  const filesTitle = element<HTMLElement>("push-files-title");
  const files = element<HTMLElement>("push-files");
  const previewTitle = element<HTMLElement>("push-preview-title");
  const preview = element<HTMLElement>("push-preview");
  const tagsCheck = element<HTMLInputElement>("push-tags");
  const tagsChoice = element<HTMLSelectElement>("push-tags-choice");
  const pushButton = element<HTMLButtonElement>("push-start");
  const cancelButton = element<HTMLButtonElement>("push-cancel");

  let repository: Repository | null = null;
  let plan: PushPlan | null = null;
  let selected: PushCommit | null = null;
  let merge: MergeView | null = null;
  let typingPause: number | undefined;
  /** Tell the answer to the latest read from one that a later one overtook. */
  let latestRead = 0;
  let latestSelection = 0;
  let latestFile = 0;

  title.textContent = t("pushTitle");
  closeButton.textContent = t("pushClose");
  cancelButton.textContent = t("pushCancel");
  pushButton.textContent = t("pushStart");
  newBadge.textContent = t("pushNewBranch");
  newBadge.title = t("pushNewBranchHint");
  remoteSelect.setAttribute("aria-label", t("pushRemote"));
  remoteSelect.title = t("pushRemote");
  targetBranch.setAttribute("aria-label", t("pushTargetBranch"));
  targetBranch.title = t("pushTargetBranch");
  element<HTMLElement>("push-define-text").textContent = t("pushNoRemote");
  element<HTMLElement>("push-define-name-label").textContent = t("pushDefineName");
  element<HTMLElement>("push-define-url-label").textContent = t("pushDefineUrl");
  defineButton.textContent = t("pushDefine");
  element<HTMLElement>("push-tags-label").textContent = t("pushTags");
  tagsChoice.replaceChildren(new Option(t("pushTagsAll"), "all"), new Option(t("pushTagsCurrentBranch"), "currentBranch"));
  filesTitle.textContent = t("pushChangedFiles");
  previewTitle.textContent = t("pushFileChanges");

  closeButton.addEventListener("click", close);
  cancelButton.addEventListener("click", close);
  pushButton.addEventListener("click", () => void startPush());
  defineButton.addEventListener("click", () => void defineRemote());
  remoteSelect.addEventListener("change", () => void read(chosenTarget()));
  targetBranch.addEventListener("input", () => {
    // Until the commits of the typed target are read, Push would still send to the one before.
    latestRead += 1;
    pushButton.disabled = true;
    window.clearTimeout(typingPause);
    typingPause = window.setTimeout(() => void read(chosenTarget()), TARGET_TYPING_PAUSE_MS);
  });
  // "Push tags" is off by default; what it sends is chosen once it is on.
  tagsCheck.addEventListener("change", () => {
    tagsChoice.disabled = !tagsCheck.checked;
  });
  commitList.addEventListener("keydown", onKey);
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(toShow: Repository): void {
    repository = toShow;
    plan = null;
    tagsCheck.checked = false;
    tagsChoice.value = "all";
    tagsChoice.disabled = true;
    repositoryName.textContent = folderName(toShow.root);
    repositoryName.title = toShow.root;
    branchName.textContent = "";
    defineName.value = "origin";
    defineUrl.value = "";
    view.hidden = false;
    host.opened();
    void read(null);
    commitList.focus();
  }

  function close(): void {
    window.clearTimeout(typingPause);
    repository = null;
    plan = null;
    latestRead += 1;
    select(null);
    commitList.replaceChildren();
    view.hidden = true;
    host.closed();
  }

  /** The target the user set in the tree. */
  function chosenTarget(): PushTarget {
    return { remote: remoteSelect.value, branch: targetBranch.value.trim() };
  }

  /** Reads what a push to `target` would send, or to the branch the core picks when it is null. */
  async function read(target: PushTarget | null): Promise<void> {
    window.clearTimeout(typingPause);
    if (repository === null) {
      return;
    }
    const shown = repository;
    const thisRead = ++latestRead;
    message.textContent = t("pushReading");
    pushButton.disabled = true;
    try {
      const read = await pushPlan(shown, target);
      if (thisRead !== latestRead) {
        return;
      }
      plan = read;
      draw(read, target === null);
    } catch (error) {
      if (thisRead === latestRead) {
        plan = null;
        message.replaceChildren(host.errorView(error));
        commitList.replaceChildren();
        newBadge.hidden = true;
        select(null);
      }
    }
  }

  /** Shows `read`, and the target it goes to in the tree when the core picked it: `fromCore`. */
  function draw(read: PushPlan, fromCore: boolean): void {
    branchName.textContent = read.branch;
    const target = read.target;
    targetView.hidden = target === null;
    defineView.hidden = target !== null;
    if (fromCore && target !== null) {
      remoteSelect.replaceChildren(...read.remotes.map((remote) => new Option(remote, remote)));
      remoteSelect.value = target.remote;
      targetBranch.value = target.branch;
    }
    newBadge.hidden = !read.newBranch;
    pushButton.disabled = target === null;
    message.textContent =
      target === null
        ? ""
        : read.commits.length === 0
          ? t("pushNothingToPush")
          : read.commits.length === 1
            ? t("pushOneCommit", { target: `${target.remote}/${target.branch}` })
            : t("pushCommitCount", { count: String(read.commits.length), target: `${target.remote}/${target.branch}` });
    drawCommits(read.commits);
    select(read.commits.find((commit) => commit.hash === selected?.hash) ?? read.commits[0] ?? null);
  }

  function drawCommits(commits: PushCommit[]): void {
    commitList.replaceChildren(
      ...commits.map((commit, index) => {
        const row = document.createElement("div");
        row.className = "push-commit";
        row.dataset.testid = "push-commit";
        row.dataset.index = String(index);
        row.dataset.hash = commit.hash;
        row.setAttribute("role", "option");
        const cell = (className: string, text: string, full = text): HTMLElement => {
          const made = document.createElement("span");
          made.className = className;
          made.textContent = text;
          made.title = full;
          return made;
        };
        // A commit someone else wrote carries `*`, so the user notices they publish another's work.
        const subject = cell("log-subject", commit.byOtherAuthor ? `* ${commit.subject}` : commit.subject);
        if (commit.byOtherAuthor) {
          subject.dataset.testid = "push-commit-other-author";
          subject.title = t("pushOtherAuthor", { author: commit.author });
        }
        row.append(
          cell("log-hash", commit.hash.slice(0, SHORT_HASH_LENGTH), commit.hash),
          subject,
          cell("log-author", commit.author),
          cell("log-date", dates.format(new Date(commit.committedAt * 1000))),
        );
        row.addEventListener("click", () => {
          select(commit);
          commitList.focus();
        });
        return row;
      }),
    );
  }

  function onKey(event: KeyboardEvent): void {
    const commits = plan?.commits ?? [];
    if (commits.length === 0 || (event.key !== "ArrowDown" && event.key !== "ArrowUp")) {
      return;
    }
    event.preventDefault();
    const from = selected === null ? -1 : commits.indexOf(selected);
    const to = Math.min(Math.max(from + (event.key === "ArrowDown" ? 1 : -1), 0), commits.length - 1);
    select(commits[to] ?? null);
  }

  /** Selects `commit`, listing the files it changed and showing the changes of the first. */
  function select(commit: PushCommit | null): void {
    selected = commit;
    const thisSelection = ++latestSelection;
    for (const row of commitList.children) {
      const isSelected = (row as HTMLElement).dataset.hash === commit?.hash;
      row.classList.toggle("selected", isSelected);
      row.setAttribute("aria-selected", String(isSelected));
      if (isSelected) {
        row.scrollIntoView({ block: "nearest" });
      }
    }
    showFile(null, null);
    files.replaceChildren();
    if (commit !== null && repository !== null) {
      void listFiles(repository, commit, thisSelection);
    }
  }

  async function listFiles(shown: Repository, commit: PushCommit, thisSelection: number): Promise<void> {
    let paths: string[];
    try {
      // The core lists the changes against the first parent first.
      const changes = (await commitChanges(shown, commit.hash))[0];
      paths = changes === undefined ? [] : filePaths(changes.group.nodes);
    } catch (error) {
      if (thisSelection === latestSelection) {
        files.replaceChildren(host.errorView(error));
      }
      return;
    }
    if (thisSelection !== latestSelection) {
      return;
    }
    if (paths.length === 0) {
      files.replaceChildren(note(t("logNoChangedFiles")));
      return;
    }
    const listed = document.createElement("ul");
    for (const path of paths) {
      const item = document.createElement("li");
      item.className = "change push-file";
      item.dataset.testid = "push-file";
      item.dataset.path = path;
      item.textContent = path;
      item.addEventListener("click", () => showFile(commit, path));
      listed.append(item);
    }
    files.replaceChildren(listed);
    showFile(commit, paths[0] ?? null);
  }

  /** Shows what `commit` changed in the file at `path`, or nothing when either is null. */
  function showFile(commit: PushCommit | null, path: string | null): void {
    const thisFile = ++latestFile;
    merge?.destroy();
    merge = null;
    preview.replaceChildren();
    for (const item of files.querySelectorAll<HTMLElement>(".push-file")) {
      item.classList.toggle("selected", item.dataset.path === path);
    }
    if (commit !== null && path !== null && repository !== null) {
      void readFile(repository, commit, path, thisFile);
    }
  }

  async function readFile(shown: Repository, commit: PushCommit, path: string, thisFile: number): Promise<void> {
    try {
      const diff = await revisionFileDiff(shown, commit.hash, path);
      if (thisFile !== latestFile) {
        return;
      }
      if (diff.content.kind === "binary") {
        preview.replaceChildren(note(t("diffBinaryFile")));
      } else if (diff.content.kind === "folder") {
        preview.replaceChildren(note(t("diffFolder")));
      } else {
        merge = createDiffPreview(preview, diff.content);
      }
    } catch (error) {
      if (thisFile === latestFile) {
        preview.replaceChildren(host.errorView(error));
      }
    }
  }

  async function startPush(): Promise<void> {
    const shown = repository;
    const target = plan?.target ?? null;
    if (shown === null || target === null) {
      return;
    }
    const tags: PushTags = tagsCheck.checked ? (tagsChoice.value as PushTags) : "none";
    pushButton.disabled = true;
    message.textContent = t("pushPushing", { target: `${target.remote}/${target.branch}` });
    try {
      await push(shown, target, tags);
      host.branchesMoved(shown);
      if (repository === shown) {
        close();
      }
    } catch (error) {
      host.branchesMoved(shown);
      if (repository === shown) {
        message.replaceChildren(host.errorView(error));
        pushButton.disabled = false;
      }
    }
  }

  /** Defines the remote the user named, fetches it, and reads the push again with it. */
  async function defineRemote(): Promise<void> {
    const shown = repository;
    if (shown === null) {
      return;
    }
    defineButton.disabled = true;
    message.textContent = t("pushDefining");
    try {
      await addRemote(shown, defineName.value.trim(), defineUrl.value);
      host.branchesMoved(shown);
      if (repository === shown) {
        await read(null);
      }
    } catch (error) {
      if (repository === shown) {
        message.replaceChildren(host.errorView(error));
      }
    } finally {
      defineButton.disabled = false;
    }
  }

  return { open, close };
}

/** The paths of the files among `nodes`, folders opened, in their order. */
function filePaths(nodes: ChangeNode[]): string[] {
  return nodes.flatMap((node) => (node.kind === "folder" ? filePaths(node.children) : [node.path]));
}
