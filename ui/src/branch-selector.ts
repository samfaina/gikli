// The branch selector: opened from the current branch in the status area, it shows what HEAD is
// on, and every branch and tag in the groups Recent, Local, Remote and Tags, with a search across
// them. The core lists, groups and searches them; this draws them and keeps the options.

import { flatBranchList, branchList, upstreamMark, type BranchActions } from "./branch-tree";
import {
  branches as readBranches,
  setGroupBranchesByDirectory,
  setShowRecentBranches,
  setShowTagsInBranchSelector,
  settings,
  type Branch,
  type BranchNode,
  type Branches,
  type CurrentBranch,
  type Repository,
  type Settings,
} from "./core";
import { element, folderName } from "./dom";
import { SHORT_HASH_LENGTH } from "./log-details";
import { t } from "./strings";

/** What the branch selector needs from the window around it. */
export interface BranchSelectorHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** Shows the latest commit of `branch` of `repository` in the log. */
  showBranch(repository: Repository, branch: Branch): void;
}

export interface BranchSelector {
  /** Opens the selector on the branches of `repository`, or closes it when it is open on them. */
  toggle(repository: Repository): void;
  /** Closes the selector. */
  close(): void;
}

/** How long the user stops typing before the branches are searched, in milliseconds. */
const SEARCH_DELAY = 150;

export function createBranchSelector(host: BranchSelectorHost): BranchSelector {
  const view = element<HTMLElement>("branch-selector");
  const title = element<HTMLElement>("branch-selector-title");
  const current = element<HTMLElement>("branch-selector-current");
  const searchInput = element<HTMLInputElement>("branch-selector-search");
  const groups = element<HTMLElement>("branch-selector-groups");
  const closeButton = element<HTMLButtonElement>("branch-selector-close");
  const options: { input: HTMLInputElement; save: (on: boolean) => Promise<Settings>; read: (saved: Settings) => boolean }[] = [
    {
      input: element<HTMLInputElement>("branch-selector-show-recent"),
      save: setShowRecentBranches,
      read: (saved) => saved.showRecentBranches,
    },
    {
      input: element<HTMLInputElement>("branch-selector-show-tags"),
      save: setShowTagsInBranchSelector,
      read: (saved) => saved.showTagsInBranchSelector,
    },
    {
      input: element<HTMLInputElement>("branch-selector-group-by-directory"),
      save: setGroupBranchesByDirectory,
      read: (saved) => saved.groupBranchesByDirectory,
    },
  ];

  let repository: Repository | null = null;
  /** Tells the answer to the latest read from one that a later search overtook. */
  let latestRead = 0;
  let searchTimer: number | undefined;

  element<HTMLElement>("branch-selector-show-recent-label").textContent = t("branchSelectorShowRecent");
  element<HTMLElement>("branch-selector-show-tags-label").textContent = t("branchSelectorShowTags");
  element<HTMLElement>("branch-selector-group-by-directory-label").textContent = t("branchesGroupByDirectory");
  closeButton.textContent = t("branchSelectorClose");
  searchInput.placeholder = t("branchSelectorSearch");
  searchInput.setAttribute("aria-label", t("branchSelectorSearch"));
  closeButton.addEventListener("click", close);
  searchInput.addEventListener("input", () => {
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => void read(), SEARCH_DELAY);
  });
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });
  for (const option of options) {
    option.input.addEventListener("change", () => void saveOption(option));
  }

  function toggle(toShow: Repository): void {
    if (repository?.root === toShow.root && !view.hidden) {
      close();
      return;
    }
    repository = toShow;
    title.textContent = t("branchSelectorTitle", { name: folderName(toShow.root) });
    title.title = toShow.root;
    searchInput.value = "";
    current.replaceChildren();
    groups.replaceChildren();
    view.hidden = false;
    void settings().then((saved) => {
      for (const option of options) {
        option.input.checked = option.read(saved);
      }
    });
    void read();
    searchInput.focus();
  }

  function close(): void {
    repository = null;
    latestRead += 1;
    window.clearTimeout(searchTimer);
    view.hidden = true;
  }

  /** Saves one of the options of the selector, and lists the branches again as it says. */
  async function saveOption(option: (typeof options)[number]): Promise<void> {
    option.input.disabled = true;
    try {
      option.input.checked = option.read(await option.save(option.input.checked));
      await read();
    } catch (error) {
      option.input.checked = !option.input.checked;
      groups.replaceChildren(host.errorView(error));
    } finally {
      option.input.disabled = false;
    }
  }

  /** Lists the branches the search finds; an answer an earlier read gives late is dropped. */
  async function read(): Promise<void> {
    const shown = repository;
    if (shown === null) {
      return;
    }
    const reading = ++latestRead;
    const search = searchInput.value;
    try {
      const listed = await readBranches(shown, search);
      if (reading === latestRead) {
        draw(shown, listed, search);
      }
    } catch (error) {
      if (reading === latestRead) {
        groups.replaceChildren(host.errorView(error));
      }
    }
  }

  function draw(shown: Repository, listed: Branches, search: string): void {
    current.replaceChildren(...describeCurrent(listed.current));
    const actions: BranchActions = {
      pick: (branch) => {
        close();
        host.showBranch(shown, branch);
      },
    };
    const drawn = [
      group("recent", t("branchesRecent"), listed.recent.length === 0 ? null : flatBranchList(listed.recent, actions)),
      group("local", t("branchesLocal"), nodesList(listed.local, actions)),
      group("remote", t("branchesRemote"), nodesList(listed.remote, actions)),
      group("tags", t("branchesTags"), nodesList(listed.tags, actions)),
    ].filter((section): section is HTMLElement => section !== null);
    if (drawn.length === 0) {
      const empty = document.createElement("p");
      empty.className = "branch-selector-empty";
      empty.textContent = search.trim() === "" ? t("branchesNone") : t("branchesNoMatches");
      groups.replaceChildren(empty);
      return;
    }
    groups.replaceChildren(...drawn);
  }

  return { toggle, close };
}

function nodesList(nodes: BranchNode[], actions: BranchActions): HTMLElement | null {
  return nodes.length === 0 ? null : branchList(nodes, actions);
}

/** A group of the selector, with its heading, or null when it holds nothing. */
function group(id: string, heading: string, list: HTMLElement | null): HTMLElement | null {
  if (list === null) {
    return null;
  }
  const section = document.createElement("section");
  section.className = "branch-group";
  section.dataset.testid = `branch-group-${id}`;
  const title = document.createElement("h3");
  title.textContent = heading;
  section.append(title, list);
  return section;
}

/** What HEAD is on, and the incoming and outgoing commits of the current branch. */
function describeCurrent(head: CurrentBranch): HTMLElement[] {
  const name = document.createElement("span");
  name.className = "branch-selector-current-name";
  if (head.kind === "detached") {
    name.textContent = t("statusDetached", { hash: head.hash.slice(0, SHORT_HASH_LENGTH) });
    return [name];
  }
  name.textContent = t("branchSelectorCurrent", { branch: head.name });
  const mark = upstreamMark(head.upstream);
  return mark === null ? [name] : [name, mark];
}
