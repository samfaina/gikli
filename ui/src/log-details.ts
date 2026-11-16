// Beside the log's commit list: the files the selected commit changed, as a tree or a list and
// against each parent of a merge, and its details. The core reads both; this draws them.

import {
  commitChanges,
  commitDetails,
  setCommitChangesGrouping,
  settings,
  type ChangeGrouping,
  type ChangeNode,
  type CommitChanges,
  type CommitDetails,
  type Repository,
} from "./core";
import { element } from "./dom";
import { t } from "./strings";

/** What the panes need from the log around them. */
export interface CommitPanesHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** Selects the commit `hash` in the log. */
  goToCommit(hash: string): void;
  /** Opens the history of the file at `path` in `repository`. */
  showFileHistory(repository: Repository, path: string): void;
}

/** The panes of the selected commit. */
export interface CommitPanes {
  /** Shows what the commit `hash` of `repository` changed and its details, or nothing when null. */
  show(repository: Repository | null, hash: string | null): void;
}

/** The branches containing a commit that show before "Show all". */
const BRANCHES_SHOWN = 6;
/** How many characters of a hash the log shows where the whole one does not fit. */
export const SHORT_HASH_LENGTH = 8;

const dates = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

/** The panes beside the commit list, in the elements of index.html that hold them. */
export function createCommitPanes(host: CommitPanesHost): CommitPanes {
  const changesTitle = element<HTMLElement>("log-changes-title");
  const groupingButton = element<HTMLButtonElement>("log-changes-grouping");
  const changesView = element<HTMLElement>("log-changes-content");
  const detailsView = element<HTMLElement>("log-details");

  let shown: { repository: Repository; hash: string } | null = null;
  let grouping: ChangeGrouping = "folder";
  /** Tells the answer to the latest read from one that a later selection overtook. */
  let latestRead = 0;

  changesTitle.textContent = t("logChangedFiles");
  groupingButton.addEventListener("click", () => void changeGrouping());
  void settings().then((current) => {
    grouping = current.commitChangesGrouping;
    labelGroupingButton();
  });
  labelGroupingButton();
  clear();

  function labelGroupingButton(): void {
    // The button offers the other arrangement.
    groupingButton.textContent = grouping === "folder" ? t("logShowAsList") : t("logShowAsTree");
  }

  function clear(): void {
    changesView.replaceChildren(note(t("logSelectCommit")));
    detailsView.replaceChildren();
    groupingButton.disabled = true;
  }

  function show(repository: Repository | null, hash: string | null): void {
    latestRead += 1;
    shown = repository === null || hash === null ? null : { repository, hash };
    if (shown === null) {
      clear();
      return;
    }
    void read(shown.repository, shown.hash, latestRead);
  }

  async function read(repository: Repository, hash: string, thisRead: number): Promise<void> {
    groupingButton.disabled = false;
    const [changes, details] = await Promise.allSettled([
      commitChanges(repository, hash),
      commitDetails(repository, hash),
    ]);
    if (thisRead !== latestRead) {
      return;
    }
    changesView.replaceChildren(
      changes.status === "fulfilled" ? changesContent(repository, changes.value) : host.errorView(changes.reason),
    );
    detailsView.replaceChildren(
      details.status === "fulfilled" ? detailsContent(details.value) : host.errorView(details.reason),
    );
  }

  /** Saves the other arrangement of the changed files, and shows them so. */
  async function changeGrouping(): Promise<void> {
    groupingButton.disabled = true;
    try {
      const saved = await setCommitChangesGrouping(grouping === "folder" ? "flat" : "folder");
      grouping = saved.commitChangesGrouping;
      labelGroupingButton();
      if (shown !== null) {
        latestRead += 1;
        await read(shown.repository, shown.hash, latestRead);
      }
    } catch (error) {
      changesView.replaceChildren(host.errorView(error));
    } finally {
      groupingButton.disabled = shown === null;
    }
  }

  /** The changed files, under a title for each parent when there are several. */
  function changesContent(repository: Repository, perParent: CommitChanges[]): HTMLElement {
    const showHistory = (path: string): void => host.showFileHistory(repository, path);
    const content = document.createElement("div");
    for (const against of perParent) {
      if (perParent.length > 1 && against.parent !== null) {
        const title = document.createElement("h4");
        title.className = "log-parent-title";
        title.textContent = t("logAgainstParent", { parent: against.parent.slice(0, SHORT_HASH_LENGTH) });
        content.append(title);
      }
      content.append(
        against.group.nodes.length === 0 ? note(t("logNoChangedFiles")) : nodesView(against.group.nodes, showHistory),
      );
    }
    return content;
  }

  function detailsContent(details: CommitDetails): HTMLElement {
    const content = document.createElement("div");
    const message = document.createElement("pre");
    message.className = "log-message-text";
    message.dataset.testid = "log-details-message";
    for (const part of details.message) {
      message.append(part.commit === null ? part.text : commitLink(part.text, part.commit));
    }
    const fields = document.createElement("dl");
    const hash = document.createElement("span");
    hash.className = "log-hash-value";
    hash.textContent = details.hash;
    field(fields, t("logDetailsHash"), hash);
    field(fields, t("logDetailsAuthor"), t("logDetailsPerson", { name: details.author, email: details.authorEmail }));
    field(fields, t("logDetailsDate"), dates.format(new Date(details.authoredAt * 1000)));
    // An amended, rebased or cherry-picked commit was committed later than it was written.
    if (details.committerDiffers || details.committedAt !== details.authoredAt) {
      const person = t("logDetailsPerson", { name: details.committer, email: details.committerEmail });
      const date = dates.format(new Date(details.committedAt * 1000));
      field(fields, t("logDetailsCommitter"), t("logDetailsCommitted", { person, date }));
    }
    field(fields, t("logDetailsBranches"), branchesView(details));
    content.append(message, fields);
    return content;
  }

  /** The branches that contain the commit: the first few, and a button that shows them all. */
  function branchesView(details: CommitDetails): HTMLElement {
    const list = document.createElement("span");
    list.className = "log-branch-list";
    list.dataset.testid = "log-details-branches";
    if (details.branches.length === 0) {
      list.textContent = t("logNoBranches");
      return list;
    }
    const chips = details.branches.map((branch) => {
      const chip = document.createElement("span");
      chip.className = `log-label log-label-${branch.kind}`;
      chip.textContent = branch.name;
      chip.title = branch.name;
      return chip;
    });
    list.append(...chips.slice(0, BRANCHES_SHOWN));
    if (chips.length > BRANCHES_SHOWN) {
      const showAll = document.createElement("button");
      showAll.type = "button";
      showAll.className = "log-commit-link";
      showAll.dataset.testid = "log-details-show-all-branches";
      showAll.textContent = t("logShowAllBranches", { count: String(chips.length) });
      showAll.addEventListener("click", () => {
        showAll.remove();
        list.append(...chips.slice(BRANCHES_SHOWN));
      });
      list.append(showAll);
    }
    return list;
  }

  /** `text`, a hash in a message, as a link to the commit `hash`. */
  function commitLink(text: string, hash: string): HTMLElement {
    const link = document.createElement("button");
    link.type = "button";
    link.className = "log-commit-link";
    link.textContent = text;
    link.title = hash;
    link.addEventListener("click", () => host.goToCommit(hash));
    return link;
  }

  return { show };
}

/** A button that shows the history of the file at `path`. */
export function historyButton(path: string, showHistory: (path: string) => void): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "file-history-button";
  button.dataset.testid = "show-file-history";
  button.textContent = t("logShowFileHistory");
  button.title = t("logShowFileHistoryOf", { path });
  button.addEventListener("click", (event) => {
    event.stopPropagation();
    showHistory(path);
  });
  button.addEventListener("dblclick", (event) => event.stopPropagation());
  return button;
}

/** Adds the field `name` with its `value` to the description `list`. */
function field(list: HTMLElement, name: string, value: string | Node): void {
  const term = document.createElement("dt");
  term.textContent = name;
  const description = document.createElement("dd");
  description.append(value);
  list.append(term, description);
}

/** A line of `text` that stands in a pane for what it has nothing of. */
function note(text: string): HTMLElement {
  const view = document.createElement("p");
  view.className = "log-side-note";
  view.textContent = text;
  return view;
}

/**
 * The changed files as a tree of folders, or a list when there are no folders among `nodes`. A file
 * `inFolder` shows by its name, under its folder, and each one offers `showHistory` of its path.
 */
function nodesView(nodes: ChangeNode[], showHistory: (path: string) => void, inFolder = false): HTMLElement {
  const list = document.createElement("ul");
  for (const node of nodes) {
    const item = document.createElement("li");
    if (node.kind === "folder") {
      const name = document.createElement("span");
      name.className = "folder-name";
      name.textContent = node.path.slice(node.path.lastIndexOf("/") + 1);
      name.title = node.path;
      item.append(name, nodesView(node.children, showHistory, true));
    } else {
      item.className = "change";
      item.dataset.status = node.status;
      const path = document.createElement("span");
      path.className = "change-path";
      const shownPath = inFolder ? node.path.slice(node.path.lastIndexOf("/") + 1) : node.path;
      path.textContent =
        node.originalPath === null ? shownPath : t("logRenamedFrom", { path: shownPath, from: node.originalPath });
      path.title = node.path;
      item.append(path, historyButton(node.path, showHistory));
    }
    list.append(item);
  }
  return list;
}
