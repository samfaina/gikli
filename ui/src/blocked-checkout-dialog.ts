// The blocked checkout dialog: files keep git from checking out, and the dialog lists them. Local changes the checkout would
// overwrite can be discarded, once the user confirmed the list, or saved, checked out over and restored in their Changelists;
// files git does not track are for the user to move or commit. The core does all of it; this only draws the dialog.

import { confirm } from "@tauri-apps/plugin-dialog";

import { checkOutOverLocalChanges, checkoutBlocked, type CheckoutTarget, type OverLocalChanges, type Repository } from "./core";
import { element, folderName } from "./dom";
import { t } from "./strings";

/** The files that block a checkout, as the core tells them. */
export type BlockedCheckout = NonNullable<ReturnType<typeof checkoutBlocked>>;

/** What the blocked checkout dialog needs from the window around it. */
export interface BlockedCheckoutDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The folder of the Project the window shows, or null when it shows none. */
  project(): string | null;
  /** HEAD of `repository` may have moved, and its files changed, even when the checkout failed. */
  headMoved(repository: Repository): void;
}

export interface BlockedCheckoutDialog {
  /** Opens the dialog on `blocked`, the files that keep `target` of `repository` from being checked out. */
  open(repository: Repository, target: CheckoutTarget, blocked: BlockedCheckout): void;
  /** Closes the dialog. */
  close(): void;
}

export function createBlockedCheckoutDialog(host: BlockedCheckoutDialogHost): BlockedCheckoutDialog {
  const view = element<HTMLElement>("blocked-checkout-dialog");
  const title = element<HTMLElement>("blocked-checkout-title");
  const localGroup = element<HTMLElement>("blocked-checkout-local");
  const localExplanation = element<HTMLElement>("blocked-checkout-local-explanation");
  const localFiles = element<HTMLElement>("blocked-checkout-local-files");
  const untrackedGroup = element<HTMLElement>("blocked-checkout-untracked");
  const untrackedExplanation = element<HTMLElement>("blocked-checkout-untracked-explanation");
  const untrackedFiles = element<HTMLElement>("blocked-checkout-untracked-files");
  const actions = element<HTMLElement>("blocked-checkout-actions");
  const saveButton = element<HTMLButtonElement>("blocked-checkout-save");
  const discardButton = element<HTMLButtonElement>("blocked-checkout-discard");
  const cancelButton = element<HTMLButtonElement>("blocked-checkout-cancel");
  const message = element<HTMLElement>("blocked-checkout-message");

  /** The checkout the dialog offers to go on with, while it is open. */
  let shown: { repository: Repository; target: CheckoutTarget; blocked: BlockedCheckout } | null = null;
  let busy = false;

  saveButton.textContent = t("blockedCheckoutSave");
  discardButton.textContent = t("blockedCheckoutDiscard");
  cancelButton.textContent = t("blockedCheckoutCancel");
  saveButton.addEventListener("click", () => void checkOut({ kind: "saveAndRestore" }));
  discardButton.addEventListener("click", () => void discard());
  cancelButton.addEventListener("click", close);
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(repository: Repository, target: CheckoutTarget, blocked: BlockedCheckout): void {
    shown = { repository, target, blocked };
    const name = folderName(repository.root);
    title.textContent = t("blockedCheckoutTitle", { target: label(target) });
    title.title = repository.root;
    localExplanation.textContent = t("blockedCheckoutLocalChanges", { name });
    untrackedExplanation.textContent = t("blockedCheckoutUntrackedFiles", { name });
    list(localFiles, blocked.localChanges);
    list(untrackedFiles, blocked.untrackedFiles);
    localGroup.hidden = blocked.localChanges.length === 0;
    untrackedGroup.hidden = blocked.untrackedFiles.length === 0;
    // Saving or discarding local changes leaves the files git does not track in the way, so only moving them unblocks it.
    actions.hidden = blocked.localChanges.length === 0 || blocked.untrackedFiles.length > 0;
    message.replaceChildren();
    allowChoice(true);
    view.hidden = false;
    (actions.hidden ? cancelButton : saveButton).focus();
  }

  function close(): void {
    if (busy) {
      return;
    }
    shown = null;
    view.hidden = true;
  }

  function allowChoice(allowed: boolean): void {
    saveButton.disabled = !allowed;
    discardButton.disabled = !allowed;
  }

  async function discard(): Promise<void> {
    const current = shown;
    if (current === null || busy) {
      return;
    }
    const files = current.blocked.localChanges;
    const sure = await confirm(t("blockedCheckoutDiscardConfirm", { target: label(current.target), files: files.join("\n") }), {
      title: t("blockedCheckoutDiscard"),
      kind: "warning",
    });
    if (sure && shown === current) {
      await checkOut({ kind: "discard", files });
    }
  }

  async function checkOut(over: OverLocalChanges): Promise<void> {
    const current = shown;
    const project = host.project();
    if (current === null || project === null || busy) {
      return;
    }
    busy = true;
    allowChoice(false);
    const target = label(current.target);
    message.textContent = over.kind === "discard" ? t("blockedCheckoutDiscarding", { target }) : t("blockedCheckoutSaving", { target });
    try {
      // A restore that conflicts opens the conflicts dialog, and HEAD is on the target by then.
      await checkOutOverLocalChanges(project, current.repository, current.target, over);
      busy = false;
      close();
    } catch (error) {
      busy = false;
      const stillBlocked = checkoutBlocked(error);
      if (stillBlocked !== null) {
        // The files that block it changed since the dialog opened: it shows the ones that block it now.
        open(current.repository, current.target, stillBlocked);
      } else {
        message.replaceChildren(host.errorView(error));
        allowChoice(true);
      }
    } finally {
      host.headMoved(current.repository);
    }
  }

  return { open, close };
}

/** Shows `paths` as the items of `files`. */
function list(files: HTMLElement, paths: string[]): void {
  files.replaceChildren(
    ...paths.map((path) => {
      const item = document.createElement("li");
      item.dataset.testid = "blocked-checkout-file";
      item.textContent = path;
      return item;
    }),
  );
}

/** How the dialog names `target`: the branch, tag or new branch by its name, a commit by its revision. */
function label(target: CheckoutTarget): string {
  return target.kind === "revision" ? target.revision : target.name;
}
