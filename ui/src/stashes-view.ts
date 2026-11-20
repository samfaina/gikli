// The stashes tab: the stashes of a Repository, newest first, the files the selected one holds, and
// the diff of the selected file against the local version or the parent commit. From there the user
// brings a stash back through the unstash dialog, and drops one stash or clears them all, after
// confirming. The core reads and changes all of it; this
// draws it.

import { confirm } from "@tauri-apps/plugin-dialog";
import type { MergeView } from "@codemirror/merge";

import {
  clearStashes,
  dropStash,
  setStashDiffAgainst,
  settings,
  stashes,
  stashFileDiff,
  stashFiles,
  type ChangeGroup,
  type ChangeNode,
  type Repository,
  type Stash,
  type StashDiffBase,
  type Stashes,
} from "./core";
import { createDiffPreview } from "./diff-view";
import { element, folderName } from "./dom";
import { cell } from "./listed-commit";
import { note } from "./log-details";
import { STASH_UNAVAILABLE_LABELS } from "./stash-dialog";
import { t } from "./strings";

/** What the stashes tab needs from the window around it. */
export interface StashesViewHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The stashes tab took the place of the view the user opened it from. */
  opened(): void;
  /** The view the user opened it from is back. */
  closed(): void;
  /** Opens the stash dialog on `repository`. */
  newStash(repository: Repository): void;
  /** Opens the unstash dialog on `stash` of `repository`. */
  unstash(repository: Repository, stash: Stash): void;
}

export interface StashesView {
  /** Shows the stashes of `repository`. */
  open(repository: Repository): void;
  /** Reads the stashes of `repository` again, when the tab shows them. */
  refreshIfShown(repository: Repository): void;
  /** Goes back to the view the user opened it from. */
  close(): void;
}

const dates = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });

/** Whether `a` and `b` are the same stashes, by the same names, in the same order. */
function sameStashes(a: Stash[], b: Stash[]): boolean {
  return a.length === b.length && a.every((stash, index) => stash.hash === b[index]?.hash && stash.name === b[index]?.name);
}

export function createStashesView(host: StashesViewHost): StashesView {
  const view = element<HTMLElement>("stashes");
  const title = element<HTMLElement>("stashes-title");
  const againstSelect = element<HTMLSelectElement>("stashes-against");
  const newButton = element<HTMLButtonElement>("stashes-new");
  const unstashButton = element<HTMLButtonElement>("stashes-unstash");
  const dropButton = element<HTMLButtonElement>("stashes-drop");
  const clearButton = element<HTMLButtonElement>("stashes-clear");
  const refreshButton = element<HTMLButtonElement>("stashes-refresh");
  const closeButton = element<HTMLButtonElement>("stashes-close");
  const message = element<HTMLElement>("stashes-message");
  const list = element<HTMLElement>("stashes-list");
  const files = element<HTMLElement>("stashes-files");
  const previewTitle = element<HTMLElement>("stashes-preview-title");
  const preview = element<HTMLElement>("stashes-preview");

  let repository: Repository | null = null;
  let listed: Stashes | null = null;
  let selected: Stash | null = null;
  let selectedPath: string | null = null;
  let merge: MergeView | null = null;
  /** Tell the answer to the latest read from one that a later one overtook. */
  let latestRead = 0;
  let latestSelection = 0;
  let latestFile = 0;

  element<HTMLElement>("stashes-against-label").textContent = t("stashesAgainst");
  againstSelect.replaceChildren(new Option(t("stashesAgainstLocal"), "local"), new Option(t("stashesAgainstParent"), "parent"));
  newButton.textContent = t("stashOpen");
  unstashButton.textContent = t("stashesUnstash");
  dropButton.textContent = t("stashesDrop");
  clearButton.textContent = t("stashesClear");
  refreshButton.textContent = t("stashesRefresh");
  closeButton.textContent = t("stashesClose");
  element<HTMLElement>("stashes-files-title").textContent = t("stashesFiles");
  previewTitle.textContent = t("stashesPreview");
  newButton.addEventListener("click", () => {
    if (repository !== null) {
      host.newStash(repository);
    }
  });
  unstashButton.addEventListener("click", unstash);
  dropButton.addEventListener("click", () => void drop());
  clearButton.addEventListener("click", () => void clear());
  refreshButton.addEventListener("click", () => void read());
  closeButton.addEventListener("click", close);
  againstSelect.addEventListener("change", () => void changeAgainst());
  list.addEventListener("keydown", onKey);
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && event.target === list) {
      event.preventDefault();
      close();
    }
  });

  function open(toShow: Repository): void {
    if (repository?.root !== toShow.root) {
      selected = null;
      selectedPath = null;
    }
    repository = toShow;
    title.textContent = t("stashesTitle", { name: folderName(toShow.root) });
    title.title = toShow.root;
    if (view.hidden) {
      view.hidden = false;
      host.opened();
    }
    void settings().then((current) => {
      againstSelect.value = current.stashDiffAgainst;
    });
    void read();
    list.focus();
  }

  function refreshIfShown(changed: Repository): void {
    if (repository?.root === changed.root && !view.hidden) {
      void read();
    }
  }

  function close(): void {
    if (view.hidden) {
      return;
    }
    repository = null;
    listed = null;
    latestRead += 1;
    select(null);
    list.replaceChildren();
    view.hidden = true;
    host.closed();
  }

  /** Reads the stashes again, keeping the one selected when it is still there. */
  async function read(): Promise<void> {
    if (repository === null) {
      return;
    }
    const shown = repository;
    const thisRead = ++latestRead;
    let read: Stashes;
    try {
      read = await stashes(shown);
    } catch (error) {
      if (thisRead === latestRead) {
        listed = null;
        message.replaceChildren(host.errorView(error));
        list.replaceChildren();
        select(null);
      }
      return;
    }
    if (thisRead !== latestRead) {
      return;
    }
    const same = listed !== null && listed.unavailable === read.unavailable && sameStashes(listed.entries, read.entries);
    message.textContent =
      read.unavailable !== null
        ? t(STASH_UNAVAILABLE_LABELS[read.unavailable])
        : read.entries.length === 0
          ? t("stashesEmpty")
          : "";
    if (same && selected !== null) {
      // The stashes are as they were: the rows stay, and only the diff, which may compare with a
      // local file that changed, is read again.
      enableActions();
      showFile(selectedPath);
      return;
    }
    listed = read;
    drawRows(read);
    select(read.entries.find((stash) => stash.hash === selected?.hash) ?? read.entries[0] ?? null);
  }

  /** The stash actions are there only when the Repository can stash and has something to act on. */
  function enableActions(): void {
    const available = listed !== null && listed.unavailable === null;
    newButton.disabled = !available;
    unstashButton.disabled = !available || selected === null;
    dropButton.disabled = !available || selected === null;
    clearButton.disabled = !available || (listed?.entries.length ?? 0) === 0;
    const reason = listed?.unavailable ?? null;
    const why = reason === null ? "" : t(STASH_UNAVAILABLE_LABELS[reason]);
    for (const button of [newButton, unstashButton, dropButton, clearButton]) {
      button.title = why;
    }
  }

  function drawRows(read: Stashes): void {
    const rows = read.entries.map((stash, index) => {
      const row = document.createElement("div");
      row.className = "file-history-row stashes-row";
      row.dataset.testid = "stashes-row";
      row.dataset.index = String(index);
      row.setAttribute("role", "option");
      row.append(
        cell("log-hash", stash.name, stash.hash),
        cell("log-subject", stash.message),
        cell("log-date", dates.format(new Date(stash.createdAt * 1000))),
      );
      row.addEventListener("click", () => {
        select(stash);
        list.focus();
      });
      return row;
    });
    list.replaceChildren(...rows);
  }

  function onKey(event: KeyboardEvent): void {
    const entries = listed?.entries ?? [];
    if (entries.length === 0) {
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      unstash();
      return;
    }
    if (event.key === "Delete") {
      event.preventDefault();
      void drop();
      return;
    }
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") {
      return;
    }
    event.preventDefault();
    const from = selected === null ? -1 : entries.indexOf(selected);
    const to = Math.min(Math.max(from + (event.key === "ArrowDown" ? 1 : -1), 0), entries.length - 1);
    select(entries[to] ?? null);
  }

  /** Selects `stash`, listing the files it holds and showing the diff of the file selected in it. */
  function select(stash: Stash | null): void {
    if (stash?.hash !== selected?.hash) {
      selectedPath = null;
    }
    selected = stash;
    const thisSelection = ++latestSelection;
    const index = stash === null || listed === null ? -1 : listed.entries.indexOf(stash);
    for (const row of list.children) {
      const isSelected = (row as HTMLElement).dataset.index === String(index);
      row.classList.toggle("selected", isSelected);
      row.setAttribute("aria-selected", String(isSelected));
      if (isSelected) {
        row.scrollIntoView({ block: "nearest" });
      }
    }
    files.replaceChildren();
    showFile(null);
    enableActions();
    if (stash !== null && repository !== null) {
      void listFiles(repository, stash, thisSelection);
    }
  }

  async function listFiles(shown: Repository, stash: Stash, thisSelection: number): Promise<void> {
    let group: ChangeGroup;
    try {
      group = await stashFiles(shown, stash.hash);
    } catch (error) {
      if (thisSelection === latestSelection) {
        files.replaceChildren(host.errorView(error));
      }
      return;
    }
    if (thisSelection !== latestSelection) {
      return;
    }
    files.replaceChildren(filesView(group));
    const path = group.changes.find((change) => change.path === selectedPath)?.path ?? group.changes[0]?.path ?? null;
    showFile(path);
  }

  /** The files of the stash, each of which shows its diff when clicked. */
  function filesView(group: ChangeGroup): HTMLElement {
    if (group.changes.length === 0) {
      return note(t("stashesNoFiles"));
    }
    const listedFiles = document.createElement("ul");
    const add = (nodes: ChangeNode[]): void => {
      for (const node of nodes) {
        if (node.kind === "folder") {
          add(node.children);
          continue;
        }
        const item = document.createElement("li");
        item.className = "change";
        item.dataset.status = node.status;
        item.dataset.testid = "stashes-file";
        item.dataset.path = node.path;
        item.textContent = node.originalPath === null ? node.path : t("logRenamedFrom", { path: node.path, from: node.originalPath });
        item.addEventListener("click", () => showFile(node.path));
        listedFiles.append(item);
      }
    };
    add(group.nodes);
    return listedFiles;
  }

  /** Shows the diff of the file at `path` in the selected stash, or nothing. */
  function showFile(path: string | null): void {
    selectedPath = path;
    const thisFile = ++latestFile;
    for (const item of files.querySelectorAll<HTMLElement>("[data-testid=stashes-file]")) {
      item.classList.toggle("file-history-this-file", item.dataset.path === path);
    }
    merge?.destroy();
    merge = null;
    preview.replaceChildren();
    previewTitle.textContent = t("stashesPreview");
    if (path !== null && selected !== null && repository !== null) {
      void showDiff(repository, selected, path, thisFile);
    }
  }

  async function showDiff(shown: Repository, stash: Stash, path: string, thisFile: number): Promise<void> {
    try {
      const diff = await stashFileDiff(shown, stash.hash, path);
      if (thisFile !== latestFile) {
        return;
      }
      previewTitle.textContent = t(diff.against === "local" ? "stashesPreviewAgainstLocal" : "stashesPreviewAgainstParent", { path });
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

  async function changeAgainst(): Promise<void> {
    const against = againstSelect.value as StashDiffBase;
    try {
      await setStashDiffAgainst(against);
      showFile(selectedPath);
    } catch (error) {
      message.replaceChildren(host.errorView(error));
    }
  }

  /** Opens the unstash dialog on the selected stash. */
  function unstash(): void {
    if (selected !== null && repository !== null && !unstashButton.disabled) {
      host.unstash(repository, selected);
    }
  }

  /** Drops the selected stash, once the user confirmed it. */
  async function drop(): Promise<void> {
    const stash = selected;
    const shown = repository;
    if (stash === null || shown === null || dropButton.disabled) {
      return;
    }
    const sure = await confirm(t("stashesDropConfirm", { name: stash.name, message: stash.message }), {
      title: t("stashesDrop"),
      kind: "warning",
    });
    if (!sure || repository !== shown) {
      return;
    }
    await act(() => dropStash(shown, stash.hash));
  }

  /** Drops every stash the tab shows, once the user confirmed it. */
  async function clear(): Promise<void> {
    const shown = repository;
    const seen = listed?.entries ?? [];
    if (shown === null || seen.length === 0 || clearButton.disabled) {
      return;
    }
    const sure = await confirm(t("stashesClearConfirm", { count: String(seen.length) }), {
      title: t("stashesClear"),
      kind: "warning",
    });
    if (!sure || repository !== shown) {
      return;
    }
    await act(() => clearStashes(shown, seen.map((stash) => stash.hash)));
  }

  /** Runs `change`, a drop or a clear, and reads the stashes again, whatever came of it. */
  async function act(change: () => Promise<Stashes>): Promise<void> {
    dropButton.disabled = true;
    clearButton.disabled = true;
    try {
      await change();
      await read();
    } catch (error) {
      await read();
      message.replaceChildren(host.errorView(error));
    }
  }

  return { open, refreshIfShown, close };
}
