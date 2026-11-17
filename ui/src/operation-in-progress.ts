// The operation in progress: a merge, rebase, revert or cherry-pick that is half done, as the status area and the branch selector
// show it, with a button for each thing the user can do about it. The core tells what is half done and what it offers, and takes
// the action; this only draws them.

import type { OperationAction, OperationInProgress, OperationKind, Repository } from "./core";
import type { StringKey } from "./strings";
import { t } from "./strings";

/** What the views that show the operations in progress need from the window around them. */
export interface OperationsHost {
  /** The operation half done in `repository`, a Repository of the Project the window shows, or null. */
  read(repository: Repository): Promise<OperationInProgress | null>;
  /** Takes `action` on the operation half done in `repository`. */
  take(repository: Repository, action: OperationAction): Promise<void>;
}

const KIND_LABELS: Record<OperationKind, StringKey> = {
  merge: "operationInProgressMerge",
  rebase: "operationInProgressRebase",
  revert: "operationInProgressRevert",
  cherryPick: "operationInProgressCherryPick",
};

const ACTION_LABELS: Record<OperationAction, StringKey> = {
  continue: "operationActionContinue",
  skip: "operationActionSkip",
  commitMerge: "operationActionCommitMerge",
  abort: "operationActionAbort",
};

/** What the user knows the operation of `kind` as, such as "Rebasing". */
export function describeOperationKind(kind: OperationKind): string {
  return t(KIND_LABELS[kind]);
}

/** The label of the button that takes `action`, such as "Commit Merge". */
export function describeOperationAction(action: OperationAction): string {
  return t(ACTION_LABELS[action]);
}

/**
 * `operation`, with a button for each action it offers that hands it to `take`. Every button waits while one action is taken.
 */
export function operationInProgressView(
  operation: OperationInProgress,
  take: (action: OperationAction) => Promise<void>,
): HTMLElement {
  const view = document.createElement("span");
  view.className = "operation-in-progress";
  view.dataset.testid = "operation-in-progress";
  view.dataset.kind = operation.kind;
  const name = document.createElement("span");
  name.className = "operation-in-progress-kind";
  name.textContent = describeOperationKind(operation.kind);
  const buttons = operation.actions.map((action) => {
    const button = document.createElement("button");
    button.type = "button";
    button.dataset.testid = "operation-action";
    button.dataset.action = action;
    button.textContent = describeOperationAction(action);
    button.addEventListener("click", async () => {
      buttons.forEach((other) => (other.disabled = true));
      try {
        await take(action);
      } finally {
        buttons.forEach((other) => (other.disabled = false));
      }
    });
    return button;
  });
  view.append(name, ...buttons);
  return view;
}
