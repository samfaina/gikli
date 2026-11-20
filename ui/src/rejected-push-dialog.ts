// The rejected push dialog: the upstream of the current branch rejected its push, as it has commits the branch lacks,
// and the user chooses to merge or rebase them before the push runs again, and whether to do so silently from then on.
// The core integrates, pushes again and remembers the choice; this draws the dialog.

import { updateAndPush, type PushTags, type PushTarget, type Repository, type UpdateMethod } from "./core";
import { element, folderName } from "./dom";
import { t } from "./strings";

/** What the rejected push dialog needs from the window around it. */
export interface RejectedPushDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The update or the push moved the branches of `repository`, even when it failed. */
  branchesMoved(repository: Repository): void;
}

export interface RejectedPushDialog {
  /**
   * Offers to integrate `target`, the upstream that rejected the push of the current branch of `repository`, and push
   * there again with `tags`. `pushed` runs once the push went through.
   */
  open(repository: Repository, target: PushTarget, tags: PushTags, pushed?: () => void): void;
  /** Closes the dialog. */
  close(): void;
}

export function createRejectedPushDialog(host: RejectedPushDialogHost): RejectedPushDialog {
  const view = element<HTMLElement>("rejected-push-dialog");
  const title = element<HTMLElement>("rejected-push-title");
  const explanation = element<HTMLElement>("rejected-push-explanation");
  const rememberCheck = element<HTMLInputElement>("rejected-push-remember");
  const mergeButton = element<HTMLButtonElement>("rejected-push-merge");
  const rebaseButton = element<HTMLButtonElement>("rejected-push-rebase");
  const cancelButton = element<HTMLButtonElement>("rejected-push-cancel");
  const message = element<HTMLElement>("rejected-push-message");

  /** The push the dialog offers to run again, while it is open. */
  let rejected: { repository: Repository; target: PushTarget; tags: PushTags; pushed?: () => void } | null = null;

  element<HTMLElement>("rejected-push-remember-label").textContent = t("rejectedPushRemember");
  mergeButton.textContent = t("rejectedPushMerge");
  rebaseButton.textContent = t("rejectedPushRebase");
  cancelButton.textContent = t("rejectedPushCancel");

  cancelButton.addEventListener("click", close);
  mergeButton.addEventListener("click", () => void update("merge"));
  rebaseButton.addEventListener("click", () => void update("rebase"));
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(repository: Repository, target: PushTarget, tags: PushTags, pushed?: () => void): void {
    rejected = { repository, target, tags, pushed };
    title.textContent = t("rejectedPushTitle", { target: `${target.remote}/${target.branch}` });
    title.title = repository.root;
    explanation.textContent = t("rejectedPushExplanation", {
      remote: target.remote,
      branch: target.branch,
      name: folderName(repository.root),
    });
    rememberCheck.checked = false;
    message.textContent = "";
    allowChoice(true);
    view.hidden = false;
    mergeButton.focus();
  }

  function close(): void {
    rejected = null;
    view.hidden = true;
  }

  function allowChoice(allowed: boolean): void {
    mergeButton.disabled = !allowed;
    rebaseButton.disabled = !allowed;
    rememberCheck.disabled = !allowed;
  }

  async function update(method: UpdateMethod): Promise<void> {
    const shown = rejected;
    if (shown === null) {
      return;
    }
    allowChoice(false);
    message.textContent = t("rejectedPushUpdating", { target: `${shown.target.remote}/${shown.target.branch}` });
    try {
      // An update that stops on a conflict opens the conflicts dialog, and pushes nothing.
      await updateAndPush(shown.repository, shown.target, shown.tags, method, rememberCheck.checked);
      host.branchesMoved(shown.repository);
      if (rejected === shown) {
        close();
      }
      shown.pushed?.();
    } catch (error) {
      host.branchesMoved(shown.repository);
      if (rejected === shown) {
        message.replaceChildren(host.errorView(error));
        allowChoice(true);
      }
    }
  }

  return { open, close };
}
