// The status area: always in view, it shows what HEAD of each Repository shown is on, and how far
// the current branch is from its upstream. The core reads both; this only draws them.

import { currentBranch, type CurrentBranch, type Repository, type Upstream } from "./core";
import { element } from "./dom";
import { SHORT_HASH_LENGTH } from "./log-details";
import { t } from "./strings";

export interface StatusArea {
  /** Shows the status of `repositories`, in place of the ones shown before. */
  show(repositories: Repository[]): void;
  /** Reads again the status of the Repository at `root`, if it is shown, or of every one shown. */
  refresh(root?: string): void;
}

export function createStatusArea(describeError: (error: unknown) => string): StatusArea {
  const view = element<HTMLElement>("status-area");
  /** The view of each Repository shown, by root, with the number of its latest read. */
  let shown = new Map<string, { repository: Repository; view: HTMLElement; latestRead: number }>();

  function show(repositories: Repository[]): void {
    shown = new Map(
      repositories.map((repository) => {
        const item = document.createElement("span");
        item.className = "status-repository";
        item.dataset.testid = "current-branch";
        item.dataset.root = repository.root;
        item.title = repository.root;
        return [repository.root, { repository, view: item, latestRead: 0 }];
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

  /** Reads what HEAD of the Repository of `entry` is on; an answer an earlier read gives late is dropped. */
  async function read(entry: { repository: Repository; view: HTMLElement; latestRead: number }): Promise<void> {
    const reading = ++entry.latestRead;
    let parts: HTMLElement[];
    try {
      parts = describe(await currentBranch(entry.repository));
    } catch (error) {
      parts = [part("status-error", describeError(error))];
    }
    if (entry.latestRead !== reading || shown.get(entry.repository.root) !== entry) {
      return;
    }
    // Several Repositories each say which one they are.
    const name = shown.size > 1 ? [part("status-name", t("statusRepository", { name: folderName(entry.repository.root) }))] : [];
    entry.view.replaceChildren(...name, ...parts);
  }

  return { show, refresh };
}

/** The current branch, or the commit of a detached HEAD, and how far the branch is from its upstream. */
function describe(head: CurrentBranch): HTMLElement[] {
  if (head.kind === "detached") {
    return [part("status-branch", t("statusDetached", { hash: head.hash.slice(0, SHORT_HASH_LENGTH) }))];
  }
  return [part("status-branch", head.name), upstreamPart(head.upstream)];
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

function folderName(root: string): string {
  return root.split(/[\\/]/).filter((piece) => piece !== "").pop() ?? root;
}
