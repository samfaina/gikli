// File history: the revisions of one file, newest first and followed through its renames, with a
// preview of what the selected one changed in the file and every file its commit changed. From
// there the user gets the file back as it was in a revision. The core reads all of it; this draws it.

import { confirm } from "@tauri-apps/plugin-dialog";
import type { MergeView } from "@codemirror/merge";

import {
  commitChanges,
  fileHistory,
  getFileVersion,
  logBranches,
  revisionFileDiff,
  setFileHistoryRowLimit,
  settings,
  type ChangeNode,
  type CommitChanges,
  type FileHistory,
  type FileRevision,
  type RefLabel,
  type Repository,
} from "./core";
import { createDiffPreview } from "./diff-view";
import { element } from "./dom";
import { SHORT_HASH_LENGTH } from "./log-details";
import { t } from "./strings";

/** What the file history needs from the window around it. */
export interface FileHistoryViewHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The file history took the place of the view the user opened it from. */
  opened(): void;
  /** The view the user opened it from is back. */
  closed(): void;
}

export interface FileHistoryView {
  /** Shows the history of the file at `path` in `repository`, on the current branch. */
  open(repository: Repository, path: string): void;
  /** Goes back to the view the user opened it from. */
  close(): void;
}

const dates = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });

export function createFileHistoryView(host: FileHistoryViewHost): FileHistoryView {
  const view = element<HTMLElement>("file-history");
  const title = element<HTMLElement>("file-history-title");
  const message = element<HTMLElement>("file-history-message");
  const list = element<HTMLElement>("file-history-list");
  const branchSelect = element<HTMLSelectElement>("file-history-branch");
  const limitInput = element<HTMLInputElement>("file-history-limit");
  const refreshButton = element<HTMLButtonElement>("file-history-refresh");
  const closeButton = element<HTMLButtonElement>("file-history-close");
  const getButton = element<HTMLButtonElement>("file-history-get");
  const previewTitle = element<HTMLElement>("file-history-preview-title");
  const preview = element<HTMLElement>("file-history-preview");
  const filesTitle = element<HTMLElement>("file-history-files-title");
  const files = element<HTMLElement>("file-history-files");

  let repository: Repository | null = null;
  let path = "";
  let history: FileHistory | null = null;
  let selected: FileRevision | null = null;
  /** The branches the history can be filtered by, as the branch filter lists them. */
  let branches: RefLabel[] = [];
  let merge: MergeView | null = null;
  /** Tells the answer to the latest read from one that a later one overtook. */
  let latestRead = 0;
  let latestSelection = 0;

  element<HTMLElement>("file-history-branch-label").textContent = t("fileHistoryBranch");
  element<HTMLElement>("file-history-limit-label").textContent = t("fileHistoryRowLimit");
  refreshButton.textContent = t("fileHistoryRefresh");
  closeButton.textContent = t("fileHistoryClose");
  getButton.textContent = t("fileHistoryGet");
  getButton.title = t("fileHistoryGetHint");
  previewTitle.textContent = t("fileHistoryPreview");
  filesTitle.textContent = t("fileHistoryAffectedFiles");
  refreshButton.addEventListener("click", () => void refresh());
  closeButton.addEventListener("click", close);
  getButton.addEventListener("click", () => void getVersion());
  branchSelect.addEventListener("change", () => void read());
  limitInput.addEventListener("change", () => void changeLimit());
  list.addEventListener("keydown", onKey);
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && event.target === list) {
      event.preventDefault();
      close();
    }
  });

  function open(toShow: Repository, file: string): void {
    repository = toShow;
    path = file;
    history = null;
    branches = [];
    branchSelect.replaceChildren(new Option(t("fileHistoryCurrentBranch"), ""));
    title.textContent = t("fileHistoryTitle", { path: file });
    view.hidden = false;
    host.opened();
    void settings().then((current) => {
      limitInput.value = String(current.fileHistoryRowLimit);
    });
    void listBranches(toShow);
    void read();
    list.focus();
  }

  function close(): void {
    repository = null;
    latestRead += 1;
    latestSelection += 1;
    select(null);
    list.replaceChildren();
    view.hidden = true;
    host.closed();
  }

  async function refresh(): Promise<void> {
    if (repository !== null) {
      await listBranches(repository);
    }
    await read();
  }

  /** Lists the branches of `shown` in the branch filter, after the current branch, keeping the one chosen. */
  async function listBranches(shown: Repository): Promise<void> {
    const chosen = branchSelect.value === "" ? undefined : branches[Number(branchSelect.value)];
    let listed: RefLabel[] = [];
    try {
      listed = await logBranches(shown);
    } catch {
      // The filter then offers the current branch only; the history says why it cannot be read.
    }
    if (repository !== shown) {
      return;
    }
    branches = listed;
    const current = new Option(t("fileHistoryCurrentBranch"), "");
    branchSelect.replaceChildren(current, ...branches.map((branch, index) => new Option(branch.name, String(index))));
    const still = branches.findIndex((branch) => branch.name === chosen?.name && branch.kind === chosen.kind);
    branchSelect.value = still < 0 ? "" : String(still);
  }

  /** Reads the history again, keeping the revision selected when it is still listed. */
  async function read(): Promise<void> {
    if (repository === null) {
      return;
    }
    const shown = repository;
    const thisRead = ++latestRead;
    const chosen = branchSelect.value === "" ? [] : [branches[Number(branchSelect.value)]].filter((branch) => branch !== undefined);
    message.textContent = t("fileHistoryReading");
    try {
      const read = await fileHistory(shown, path, chosen);
      if (thisRead !== latestRead) {
        return;
      }
      history = read;
      message.textContent =
        read.revisions.length === 0
          ? t("fileHistoryEmpty")
          : read.more
            ? t("fileHistoryMore", { count: String(read.revisions.length) })
            : "";
      drawRows(read);
      const still = read.revisions.find((revision) => revision.hash === selected?.hash) ?? read.revisions[0] ?? null;
      select(still);
    } catch (error) {
      if (thisRead === latestRead) {
        message.replaceChildren(host.errorView(error));
        list.replaceChildren();
        select(null);
      }
    }
  }

  async function changeLimit(): Promise<void> {
    const limit = Number.parseInt(limitInput.value, 10);
    if (Number.isNaN(limit)) {
      return;
    }
    try {
      const saved = await setFileHistoryRowLimit(limit);
      limitInput.value = String(saved.fileHistoryRowLimit);
      await read();
    } catch (error) {
      message.replaceChildren(host.errorView(error));
    }
  }

  /** The rows of the revisions, with the path each one has the file at: the renames column. */
  function drawRows(read: FileHistory): void {
    const rows = read.revisions.map((revision, index) => {
      const row = document.createElement("div");
      row.className = "file-history-row";
      row.dataset.testid = "file-history-row";
      row.dataset.index = String(index);
      row.setAttribute("role", "option");
      const cell = (className: string, text: string, full = text): HTMLElement => {
        const made = document.createElement("span");
        made.className = className;
        made.textContent = text;
        made.title = full;
        return made;
      };
      const pathText = revision.originalPath === null ? revision.path : t("logRenamedFrom", { path: revision.path, from: revision.originalPath });
      const pathCell = cell("file-history-path", pathText);
      pathCell.classList.toggle("file-history-path-other", revision.path !== read.path);
      row.append(
        cell("log-hash", revision.hash.slice(0, SHORT_HASH_LENGTH), revision.hash),
        cell("log-subject", revision.subject),
        pathCell,
        cell("log-author", revision.author),
        cell("log-date", dates.format(new Date(revision.committedAt * 1000))),
      );
      row.addEventListener("click", () => {
        select(revision);
        list.focus();
      });
      return row;
    });
    list.replaceChildren(...rows);
  }

  function onKey(event: KeyboardEvent): void {
    if (history === null || history.revisions.length === 0 || (event.key !== "ArrowDown" && event.key !== "ArrowUp")) {
      return;
    }
    event.preventDefault();
    const from = selected === null ? -1 : history.revisions.indexOf(selected);
    const to = Math.min(Math.max(from + (event.key === "ArrowDown" ? 1 : -1), 0), history.revisions.length - 1);
    select(history.revisions[to] ?? null);
  }

  /** Selects `revision`, previewing what it changed in the file and listing every file its commit changed. */
  function select(revision: FileRevision | null): void {
    selected = revision;
    const thisSelection = ++latestSelection;
    const index = revision === null || history === null ? -1 : history.revisions.indexOf(revision);
    for (const row of list.children) {
      const isSelected = (row as HTMLElement).dataset.index === String(index);
      row.classList.toggle("selected", isSelected);
      row.setAttribute("aria-selected", String(isSelected));
      if (isSelected) {
        row.scrollIntoView({ block: "nearest" });
      }
    }
    merge?.destroy();
    merge = null;
    preview.replaceChildren();
    files.replaceChildren();
    getButton.disabled = revision === null;
    if (revision !== null && repository !== null) {
      void showRevision(repository, revision, thisSelection);
    }
  }

  async function showRevision(shown: Repository, revision: FileRevision, thisSelection: number): Promise<void> {
    const [diff, changes] = await Promise.allSettled([
      revisionFileDiff(shown, revision.hash, revision.path),
      commitChanges(shown, revision.hash),
    ]);
    if (thisSelection !== latestSelection) {
      return;
    }
    if (diff.status === "rejected") {
      preview.replaceChildren(host.errorView(diff.reason));
    } else if (diff.value.content.kind === "binary") {
      preview.replaceChildren(note(t("diffBinaryFile")));
    } else if (diff.value.content.kind === "folder") {
      preview.replaceChildren(note(t("diffFolder")));
    } else {
      merge = createDiffPreview(preview, diff.value.content);
    }
    files.replaceChildren(changes.status === "fulfilled" ? filesView(changes.value[0], revision.path) : host.errorView(changes.reason));
  }

  /** Brings the file back to its version in the selected revision, once the user confirmed it. */
  async function getVersion(): Promise<void> {
    const revision = selected;
    const shown = repository;
    if (revision === null || shown === null) {
      return;
    }
    const hash = revision.hash.slice(0, SHORT_HASH_LENGTH);
    const sure = await confirm(t("fileHistoryGetConfirm", { path, hash }), { title: t("fileHistoryGet"), kind: "warning" });
    if (!sure) {
      return;
    }
    try {
      await getFileVersion(shown, revision.hash, revision.path, path);
      message.textContent = t("fileHistoryGot", { path, hash });
    } catch (error) {
      message.replaceChildren(host.errorView(error));
    }
  }

  return { open, close };
}

/** Every file the commit changed against its first parent, `path` marked as the file of the history. */
function filesView(changes: CommitChanges | undefined, path: string): HTMLElement {
  if (changes === undefined || changes.group.nodes.length === 0) {
    return note(t("logNoChangedFiles"));
  }
  const listed = document.createElement("ul");
  const add = (nodes: ChangeNode[]): void => {
    for (const node of nodes) {
      if (node.kind === "folder") {
        add(node.children);
        continue;
      }
      const item = document.createElement("li");
      item.className = "change";
      item.dataset.status = node.status;
      item.dataset.testid = "file-history-affected-file";
      item.classList.toggle("file-history-this-file", node.path === path);
      item.textContent = node.originalPath === null ? node.path : t("logRenamedFrom", { path: node.path, from: node.originalPath });
      listed.append(item);
    }
  };
  add(changes.group.nodes);
  return listed;
}

/** A line of `text` that stands in a pane for what it has nothing of. */
function note(text: string): HTMLElement {
  const view = document.createElement("p");
  view.className = "log-side-note";
  view.textContent = text;
  return view;
}
