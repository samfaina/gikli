// The branches as the branch selector and the branches pane of the log draw them: each group a
// list, with its directories open, and each local branch with its incoming and outgoing commits.
// The core lists, groups and searches the branches; this only draws them.

import type { Branch, BranchNode, RefLabel, Upstream } from "./core";
import { t } from "./strings";

/** What a drawn branch does when the user acts on it. */
export interface BranchActions {
  /** The user clicked the branch, or pressed Enter on it. */
  pick(branch: Branch): void;
  /** The user double-clicked the branch. */
  open?(branch: Branch): void;
  /** The branch shown as chosen, if any. */
  chosen?: RefLabel | null;
}

/** The list of `nodes`, with each directory open and each branch named within its directory. */
export function branchList(nodes: BranchNode[], actions: BranchActions, directory = ""): HTMLUListElement {
  const list = document.createElement("ul");
  list.className = "branch-list";
  list.append(...nodes.map((node) => branchNodeItem(node, actions, directory)));
  return list;
}

/** The list of `branches`, each by its whole name, as Recent shows them. */
export function flatBranchList(branches: Branch[], actions: BranchActions): HTMLUListElement {
  return branchList(
    branches.map((branch) => ({ kind: "branch", ...branch })),
    actions,
  );
}

function branchNodeItem(node: BranchNode, actions: BranchActions, directory: string): HTMLLIElement {
  const item = document.createElement("li");
  if (node.kind === "directory") {
    const details = document.createElement("details");
    details.open = true;
    details.dataset.testid = "branch-directory";
    details.dataset.path = node.path;
    const summary = document.createElement("summary");
    summary.className = "branch-directory-name";
    summary.textContent = withinDirectory(node.path, directory);
    details.append(summary, branchList(node.children, actions, node.path));
    item.append(details);
  } else {
    item.append(branchButton(node, withinDirectory(node.label.name, directory), actions));
  }
  return item;
}

/** A button for `branch`, named `shownName`, with its incoming and outgoing commits. */
function branchButton(branch: Branch, shownName: string, actions: BranchActions): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "branch-item";
  button.dataset.testid = "branch";
  button.dataset.name = branch.label.name;
  button.dataset.kind = branch.label.kind;
  button.title = branch.label.name;
  const chosen = actions.chosen;
  const isChosen = chosen !== undefined && chosen !== null && chosen.name === branch.label.name && chosen.kind === branch.label.kind;
  button.classList.toggle("chosen", isChosen);
  button.setAttribute("aria-pressed", String(isChosen));
  const name = document.createElement("span");
  name.className = "branch-name";
  name.textContent = shownName;
  button.append(name);
  const mark = upstreamMark(branch.upstream);
  if (mark !== null) {
    button.append(mark);
  }
  button.addEventListener("click", () => actions.pick(branch));
  const open = actions.open;
  if (open !== undefined) {
    button.addEventListener("dblclick", () => open(branch));
  }
  return button;
}

/**
 * The incoming and outgoing commits of a branch against `upstream`, only those there are, or that
 * it is gone. A branch without an upstream, or level with it, has no mark.
 */
export function upstreamMark(upstream: Upstream | null): HTMLElement | null {
  if (upstream === null) {
    return null;
  }
  const mark = document.createElement("span");
  mark.className = "branch-marks";
  mark.dataset.testid = "branch-marks";
  if (upstream.commits === null) {
    mark.textContent = t("branchUpstreamGone");
    mark.title = t("statusUpstreamGone", { upstream: upstream.name });
    return mark;
  }
  const { incoming, outgoing } = upstream.commits;
  if (incoming === 0 && outgoing === 0) {
    return null;
  }
  const parts: string[] = [];
  if (incoming > 0) {
    parts.push(t("branchIncoming", { count: String(incoming) }));
  }
  if (outgoing > 0) {
    parts.push(t("branchOutgoing", { count: String(outgoing) }));
  }
  mark.textContent = parts.join(" ");
  mark.dataset.incoming = String(incoming);
  mark.dataset.outgoing = String(outgoing);
  mark.title = t("statusCountsTitle", { incoming: String(incoming), outgoing: String(outgoing), upstream: upstream.name });
  return mark;
}

/** `path` without the `directory` it is in, as a branch or directory is named within it. */
function withinDirectory(path: string, directory: string): string {
  return directory === "" ? path : path.slice(directory.length + 1);
}
