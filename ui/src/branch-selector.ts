// The branch selector: opened from the current branch in the status area, it shows what HEAD is
// on with the operation half done there, and every branch and tag in the groups Recent, Local,
// Remote and Tags, with a search across them. Picking a local branch, a remote branch or a tag
// checks it out, and a new branch starts from HEAD. The menu of a branch checks it out too, or
// merges it into the current branch. The core lists, groups and searches them, checks them out
// and merges them; this draws them and keeps the options.

import type { BlockedCheckout } from "./blocked-checkout-dialog";
import type { BlockedMerge } from "./blocked-merge-dialog";
import type { LostLocalCommits } from "./local-commits-dialog";
import { branchGroup, branchList, flatBranchList, upstreamMark, type BranchActions } from "./branch-tree";
import {
  branches as readBranches,
  checkOutBranch,
  checkOutRemoteBranch,
  checkOutTag,
  checkoutBlocked,
  localCommitsWouldBeLost,
  mergeBlocked,
  mergeIntoCurrent,
  type CheckoutTarget,
  setGroupBranchesByDirectory,
  setShowRecentBranches,
  setShowTagsInBranchSelector,
  settings,
  type Branch,
  type BranchNode,
  type Branches,
  type CurrentBranch,
  type OperationAction,
  type OperationInProgress,
  type RefLabel,
  type Repository,
  type Settings,
} from "./core";
import { element, folderName } from "./dom";
import { SHORT_HASH_LENGTH } from "./log-details";
import { operationInProgressView, type OperationsHost } from "./operation-in-progress";
import { t } from "./strings";

/** What the branch selector needs from the window around it. */
export interface BranchSelectorHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The folder of the Project the window shows, or null when it shows none. */
  project(): string | null;
  /** Opens the new branch dialog on a branch of `repository` that starts from HEAD. */
  newBranch(repository: Repository): void;
  /** HEAD of `repository` moved to another branch or commit. */
  headMoved(repository: Repository): void;
  /** Files block the checkout of `target` in `repository`, as `blocked` lists them, for the user to choose how to go on. */
  checkoutBlocked(repository: Repository, target: CheckoutTarget, blocked: BlockedCheckout): void;
  /** Files block the merge of `branch` into the current branch of `repository`, as `blocked` lists them, for the user to choose how to go on. */
  mergeBlocked(repository: Repository, branch: RefLabel, blocked: BlockedMerge): void;
  /** Checking out a remote branch of `repository` would lose the local commits `lost` tells of, for the user to choose what happens to them. */
  localCommitsWouldBeLost(repository: Repository, lost: LostLocalCommits): void;
  /** What shows the operation half done in the Repository, and takes the actions on it. */
  operations: OperationsHost;
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
  const newBranchButton = element<HTMLButtonElement>("branch-selector-new-branch");
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
  /** Whether a branch or tag the user picked is being checked out, or a branch merged. */
  let busy = false;
  /** The branch HEAD is on, as the selector last listed it, or null on a detached HEAD. */
  let currentName: string | null = null;
  const menu = createBranchMenu(view);

  element<HTMLElement>("branch-selector-show-recent-label").textContent = t("branchSelectorShowRecent");
  element<HTMLElement>("branch-selector-show-tags-label").textContent = t("branchSelectorShowTags");
  element<HTMLElement>("branch-selector-group-by-directory-label").textContent = t("branchesGroupByDirectory");
  closeButton.textContent = t("branchSelectorClose");
  newBranchButton.textContent = t("branchSelectorNewBranch");
  newBranchButton.addEventListener("click", () => {
    const shown = repository;
    if (shown !== null) {
      close();
      host.newBranch(shown);
    }
  });
  searchInput.placeholder = t("branchSelectorSearch");
  searchInput.setAttribute("aria-label", t("branchSelectorSearch"));
  closeButton.addEventListener("click", close);
  searchInput.addEventListener("input", () => {
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => void listBranches(), SEARCH_DELAY);
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
    void listBranches();
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
      await listBranches();
    } catch (error) {
      option.input.checked = !option.input.checked;
      groups.replaceChildren(host.errorView(error));
    } finally {
      option.input.disabled = false;
    }
  }

  /** Lists the branches the search finds; an answer an earlier read gives late is dropped. */
  async function listBranches(): Promise<void> {
    const shown = repository;
    if (shown === null || busy) {
      return;
    }
    const reading = ++latestRead;
    const search = searchInput.value;
    try {
      const [listed, operation] = await Promise.all([readBranches(shown, search), host.operations.read(shown)]);
      if (reading === latestRead) {
        draw(shown, listed, operation, search);
      }
    } catch (error) {
      if (reading === latestRead) {
        groups.replaceChildren(host.errorView(error));
      }
    }
  }

  groups.addEventListener("contextmenu", (event) => {
    const shown = repository;
    const button = event.target instanceof Element ? event.target.closest<HTMLButtonElement>('[data-testid="branch"]') : null;
    const { name, kind } = button?.dataset ?? {};
    if (shown === null || button === null || name === undefined || !isRefKind(kind)) {
      return;
    }
    event.preventDefault();
    const label: RefLabel = { name, kind };
    const items: BranchMenuItem[] = [{ testId: "branch-menu-check-out", label: t("branchSelectorMenuCheckOut"), run: () => void pick(shown, { label }) }];
    // A branch merges into the current one, which is not itself.
    if (kind === "remote" || (kind === "local" && name !== currentName)) {
      items.push({ testId: "branch-menu-merge", label: t("branchSelectorMenuMerge"), run: () => void merge(shown, label) });
    }
    // From the keyboard the menu opens on no point, so it opens at the branch.
    const from = button.getBoundingClientRect();
    const atPoint = event.clientX !== 0 || event.clientY !== 0;
    menu.open(items, atPoint ? event.clientX : from.left, atPoint ? event.clientY : from.bottom, button);
  });

  function draw(shown: Repository, listed: Branches, operation: OperationInProgress | null, search: string): void {
    currentName = listed.current.kind === "branch" ? listed.current.name : null;
    current.replaceChildren(...describeCurrent(listed.current));
    if (operation !== null) {
      current.append(operationInProgressView(operation, (action) => takeOperationAction(shown, action)));
    }
    const actions: BranchActions = {
      pick: (branch) => void pick(shown, branch),
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

  /**
   * Checks out a local branch, a remote branch or a tag the user picked, and closes the selector once HEAD is on it; a failure shows
   * in the selector.
   */
  async function pick(shown: Repository, branch: Pick<Branch, "label">): Promise<void> {
    const { name } = branch.label;
    const project = host.project();
    if (busy || project === null) {
      return;
    }
    const picked = checkoutOf(project, shown, branch);
    if (picked === null) {
      return;
    }
    const { target, run } = picked;
    // The lists are not read again while the checkout runs, so they do not take the place of what it says.
    window.clearTimeout(searchTimer);
    latestRead += 1;
    busy = true;
    const progress = document.createElement("p");
    progress.className = "branch-selector-empty";
    progress.textContent = t("branchSelectorCheckingOut", { target: name });
    groups.replaceChildren(progress);
    let failure: { error: unknown } | null = null;
    try {
      await run();
    } catch (error) {
      failure = { error };
    } finally {
      busy = false;
      // A checkout that failed may still have moved HEAD, as when a hook after it fails.
      host.headMoved(shown);
    }
    if (repository !== shown) {
      return;
    }
    const blocked = failure === null ? null : checkoutBlocked(failure.error);
    const lost = failure === null ? null : localCommitsWouldBeLost(failure.error);
    if (failure === null) {
      close();
    } else if (blocked !== null) {
      close();
      host.checkoutBlocked(shown, target, blocked);
    } else if (lost !== null) {
      close();
      host.localCommitsWouldBeLost(shown, lost);
    } else {
      groups.replaceChildren(host.errorView(failure.error));
    }
  }

  /**
   * Merges `branch` into the current branch of `shown`, and closes the selector once it is merged or stopped on a conflict, which opens
   * the conflicts dialog; files that block it open the blocked merge dialog, and another failure shows in the selector.
   */
  async function merge(shown: Repository, branch: RefLabel): Promise<void> {
    const project = host.project();
    if (busy || project === null) {
      return;
    }
    // The lists are not read again while the merge runs, so they do not take the place of what it says.
    window.clearTimeout(searchTimer);
    latestRead += 1;
    busy = true;
    const progress = document.createElement("p");
    progress.className = "branch-selector-empty";
    progress.textContent = t("branchSelectorMerging", { branch: branch.name });
    groups.replaceChildren(progress);
    let failure: { error: unknown } | null = null;
    try {
      await mergeIntoCurrent(project, shown, branch);
    } catch (error) {
      failure = { error };
    } finally {
      busy = false;
      host.headMoved(shown);
    }
    if (repository !== shown) {
      return;
    }
    const blocked = failure === null ? null : mergeBlocked(failure.error);
    if (failure === null) {
      close();
    } else if (blocked !== null) {
      close();
      host.mergeBlocked(shown, branch, blocked);
    } else {
      groups.replaceChildren(host.errorView(failure.error));
    }
  }

  /** Takes `action` on the operation half done in `shown`, and reads the branches again, as HEAD may have moved. */
  async function takeOperationAction(shown: Repository, action: OperationAction): Promise<void> {
    await host.operations.take(shown, action);
    if (repository === shown) {
      await listBranches();
    }
  }

  return { toggle, close };
}

/**
 * The checkout picking `branch` of `repository`, a Repository of the Project at `project`, runs: of a local branch, a remote branch
 * or a tag, with the target it takes HEAD to. Null for HEAD itself.
 */
function checkoutOf(
  project: string,
  repository: Repository,
  branch: Pick<Branch, "label">,
): { target: CheckoutTarget; run: () => Promise<CurrentBranch> } | null {
  const { name, kind } = branch.label;
  switch (kind) {
    case "local":
      return { target: { kind: "branch", name }, run: () => checkOutBranch(repository, name) };
    case "remote":
      return { target: { kind: "remoteBranch", name, localCommits: null }, run: () => checkOutRemoteBranch(project, repository, name, null) };
    case "tag":
      return { target: { kind: "tag", name }, run: () => checkOutTag(repository, name) };
    case "head":
      return null;
  }
}

/** An item of the menu of a branch. */
interface BranchMenuItem {
  /** The `data-testid` of its item. */
  testId: string;
  /** The text of its item. */
  label: string;
  /** Acts on the branch the menu is of. */
  run(): void;
}

/** The menu of a branch in the selector, added to `container`. */
function createBranchMenu(container: HTMLElement): { open(items: BranchMenuItem[], x: number, y: number, from: HTMLElement): void } {
  const menu = document.createElement("div");
  menu.className = "context-menu";
  menu.dataset.testid = "branch-menu";
  menu.setAttribute("role", "menu");
  menu.hidden = true;
  container.append(menu);
  let returnFocus: HTMLElement | null = null;

  const close = (): void => {
    menu.hidden = true;
  };
  // So that clicking an item does not take the focus from it, which would close the menu first.
  menu.addEventListener("mousedown", (event) => event.preventDefault());
  menu.addEventListener("focusout", (event) => {
    if (!(event.relatedTarget instanceof Node && menu.contains(event.relatedTarget))) {
      close();
    }
  });
  menu.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      // The selector stays open: Escape closes only the menu.
      event.stopPropagation();
      close();
      returnFocus?.focus();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const items = [...menu.children] as HTMLElement[];
      const from = items.indexOf(document.activeElement as HTMLElement);
      items[(from + (event.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
    }
  });

  function open(items: BranchMenuItem[], x: number, y: number, from: HTMLElement): void {
    returnFocus = from;
    const buttons = items.map((entry) => {
      const item = document.createElement("button");
      item.type = "button";
      item.dataset.testid = entry.testId;
      item.setAttribute("role", "menuitem");
      item.textContent = entry.label;
      item.addEventListener("click", () => {
        close();
        entry.run();
      });
      return item;
    });
    menu.replaceChildren(...buttons);
    menu.hidden = false;
    menu.style.left = `${Math.min(x, window.innerWidth - menu.offsetWidth)}px`;
    menu.style.top = `${Math.min(y, window.innerHeight - menu.offsetHeight)}px`;
    buttons[0]?.focus();
  }

  return { open };
}

/** Whether `kind`, as a branch button of the selector names it, is the kind of a reference. */
function isRefKind(kind: string | undefined): kind is RefLabel["kind"] {
  return kind === "local" || kind === "remote" || kind === "tag" || kind === "head";
}

function nodesList(nodes: BranchNode[], actions: BranchActions): HTMLElement | null {
  return nodes.length === 0 ? null : branchList(nodes, actions);
}

/** A group of the selector, with its heading, or null when it holds nothing. */
function group(id: string, heading: string, list: HTMLElement | null): HTMLElement | null {
  if (list === null) {
    return null;
  }
  const title = document.createElement("h3");
  title.textContent = heading;
  return branchGroup(`branch-group-${id}`, title, list);
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
