// The status area: always in view, it shows what HEAD of each Repository shown is on, how far
// the current branch is from its upstream, and the operation half done there, with what the user can
// do about it. The core reads them; this only draws them. What HEAD is on opens the branch selector.

import { currentBranch, type CurrentBranch, type OperationInProgress, type Repository, type Upstream } from "./core";
import { element, folderName } from "./dom";
import { SHORT_HASH_LENGTH } from "./log-details";
import { operationInProgressView, type OperationsHost } from "./operation-in-progress";
import { t } from "./strings";

export interface StatusArea {
  /** Shows the status of `repositories`, in place of the ones shown before. */
  show(repositories: Repository[]): void;
  /** Reads again the status of the Repository at `root`, if it is shown, or of every one shown. */
  refresh(root?: string): void;
}

/** One Repository the status area shows, with the number of its latest read. */
interface ShownRepository {
  repository: Repository;
  view: HTMLElement;
  /**
   * What HEAD is on, which opens the branch selector. Each read keeps it in the page: a browser drops a click whose target left the page
   * between the press and the release, as a read the watcher starts just after a checkout would make it do (issue #154).
   */
  branch: HTMLButtonElement;
  latestRead: number;
}

export function createStatusArea(
  describeError: (error: unknown) => string,
  openBranches: (repository: Repository) => void,
  operations: OperationsHost,
): StatusArea {
  const view = element<HTMLElement>("status-area");
  /** Each Repository shown, by root. */
  let shown = new Map<string, ShownRepository>();

  function show(repositories: Repository[]): void {
    shown = new Map(
      repositories.map((repository) => {
        const item = document.createElement("span");
        item.className = "status-repository";
        item.dataset.testid = "current-branch";
        item.dataset.root = repository.root;
        item.title = repository.root;
        return [repository.root, { repository, view: item, branch: branchButton(() => openBranches(repository)), latestRead: 0 }];
      }),
    );
    view.replaceChildren(...[...shown.values()].map(({ view: item }) => item));
    view.hidden = shown.size === 0;
    refresh();
  }

  function refresh(root?: string): void {
    for (const entry of shown.values()) {
      if (root === undefined || entry.repository.root === root) {
        void read(entry);
      }
    }
  }

  /**
   * Reads what HEAD of the Repository of `entry` is on, and the operation half done there; an answer an earlier read gives late is
   * dropped.
   */
  async function read(entry: ShownRepository): Promise<void> {
    const reading = ++entry.latestRead;
    const { repository } = entry;
    let answer: { head: CurrentBranch; operation: OperationInProgress | null } | { error: unknown };
    try {
      const [head, operation] = await Promise.all([currentBranch(repository), operations.read(repository)]);
      answer = { head, operation };
    } catch (error) {
      answer = { error };
    }
    if (entry.latestRead !== reading || shown.get(entry.repository.root) !== entry) {
      return;
    }
    if ("error" in answer) {
      entry.view.replaceChildren(part("status-error", describeError(answer.error)));
      return;
    }
    // Several Repositories each say which one they are.
    const name = shown.size > 1 ? [part("status-name", t("statusRepository", { name: folderName(entry.repository.root) }))] : [];
    const after = describe(entry.branch, answer.head);
    if (answer.operation !== null) {
      after.push(operationInProgressView(answer.operation, (action) => operations.take(repository, action)));
    }
    redraw(entry, name, after);
  }

  return { show, refresh };
}

/** Shows `before`, the branch button of `entry` and `after` in its view, without taking the button out of the page if it is in. */
function redraw(entry: ShownRepository, before: HTMLElement[], after: HTMLElement[]): void {
  const { view, branch } = entry;
  if (branch.parentNode !== view) {
    view.replaceChildren(...before, branch, ...after);
    return;
  }
  for (const child of [...view.children]) {
    if (child !== branch) {
      child.remove();
    }
  }
  branch.before(...before);
  branch.after(...after);
}

/** The button that shows what HEAD is on, and opens the branch selector with `open`. */
function branchButton(open: () => void): HTMLButtonElement {
  const branch = document.createElement("button");
  branch.type = "button";
  branch.className = "status-branch";
  branch.dataset.testid = "status-branch";
  branch.title = t("statusOpenBranches");
  branch.setAttribute("aria-haspopup", "dialog");
  branch.setAttribute("aria-controls", "branch-selector");
  branch.addEventListener("click", open);
  return branch;
}

/**
 * Shows the current branch, or the commit of a detached HEAD, on `branch`, and returns how far the
 * branch is from its upstream.
 */
function describe(branch: HTMLButtonElement, head: CurrentBranch): HTMLElement[] {
  const name = head.kind === "detached" ? t("statusDetached", { hash: head.hash.slice(0, SHORT_HASH_LENGTH) }) : head.name;
  // Its text is only set when it changes, as the text the user presses on is part of the click too.
  if (branch.textContent !== name) {
    branch.textContent = name;
  }
  return head.kind === "detached" ? [] : [upstreamPart(head.upstream)];
}

function upstreamPart(upstream: Upstream | null): HTMLElement {
  if (upstream === null) {
    return part("status-upstream", t("statusNoUpstream"));
  }
  if (upstream.commits === null) {
    return part("status-upstream", t("statusUpstreamGone", { upstream: upstream.name }));
  }
  const { incoming, outgoing } = upstream.commits;
  const counts = part("status-counts", t("statusCounts", { incoming: String(incoming), outgoing: String(outgoing) }));
  counts.dataset.incoming = String(incoming);
  counts.dataset.outgoing = String(outgoing);
  counts.title = t("statusCountsTitle", { incoming: String(incoming), outgoing: String(outgoing), upstream: upstream.name });
  return counts;
}

function part(testId: string, text: string): HTMLElement {
  const span = document.createElement("span");
  span.dataset.testid = testId;
  span.textContent = text;
  return span;
}
