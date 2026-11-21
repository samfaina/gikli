// The local commits dialog: checking out a remote branch would move the local branch of its name to it and lose commits of that
// branch, and the user chooses to drop them, with a Backup to undo it, or to rebase them onto the remote branch. The core checks out,
// saves the Backup and rebases; this draws the dialog.

import type { BlockedCheckout } from "./blocked-checkout-dialog";
import { checkOutRemoteBranch, checkoutBlocked, localCommitsWouldBeLost, type CheckoutTarget, type LocalCommits, type Repository } from "./core";
import { element, folderName } from "./dom";
import { t } from "./strings";

/** The local commits a checkout of a remote branch would lose, as the core tells them. */
export type LostLocalCommits = NonNullable<ReturnType<typeof localCommitsWouldBeLost>>;

/** What the local commits dialog needs from the window around it. */
export interface LocalCommitsDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The folder of the Project the window shows, or null when it shows none. */
  project(): string | null;
  /** HEAD of `repository` may have moved, and its branches changed, even when the checkout failed. */
  headMoved(repository: Repository): void;
  /** Files block the checkout of `target` in `repository`, as `blocked` lists them, for the user to choose how to go on. */
  checkoutBlocked(repository: Repository, target: CheckoutTarget, blocked: BlockedCheckout): void;
}

export interface LocalCommitsDialog {
  /** Opens the dialog on `lost`, the local commits of `repository` a checkout of a remote branch would lose. */
  open(repository: Repository, lost: LostLocalCommits): void;
  /** Closes the dialog. */
  close(): void;
}

export function createLocalCommitsDialog(host: LocalCommitsDialogHost): LocalCommitsDialog {
  const view = element<HTMLElement>("local-commits-dialog");
  const title = element<HTMLElement>("local-commits-title");
  const explanation = element<HTMLElement>("local-commits-explanation");
  const dropButton = element<HTMLButtonElement>("local-commits-drop");
  const rebaseButton = element<HTMLButtonElement>("local-commits-rebase");
  const cancelButton = element<HTMLButtonElement>("local-commits-cancel");
  const message = element<HTMLElement>("local-commits-message");

  /** The checkout the dialog offers to go on with, while it is open. */
  let shown: { repository: Repository; lost: LostLocalCommits } | null = null;
  let busy = false;

  dropButton.textContent = t("localCommitsDrop");
  rebaseButton.textContent = t("localCommitsRebase");
  cancelButton.textContent = t("localCommitsCancel");
  dropButton.addEventListener("click", () => void checkOut("drop"));
  rebaseButton.addEventListener("click", () => void checkOut("rebase"));
  cancelButton.addEventListener("click", close);
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(repository: Repository, lost: LostLocalCommits): void {
    shown = { repository, lost };
    title.textContent = t("localCommitsTitle", { remoteBranch: lost.remoteBranch });
    title.title = repository.root;
    explanation.textContent = t("localCommitsExplanation", {
      branch: lost.branch,
      remoteBranch: lost.remoteBranch,
      commits: String(lost.commits),
      name: folderName(repository.root),
    });
    message.replaceChildren();
    allowChoice(true);
    view.hidden = false;
    rebaseButton.focus();
  }

  function close(): void {
    if (busy) {
      return;
    }
    shown = null;
    view.hidden = true;
  }

  function allowChoice(allowed: boolean): void {
    dropButton.disabled = !allowed;
    rebaseButton.disabled = !allowed;
  }

  async function checkOut(localCommits: LocalCommits): Promise<void> {
    const current = shown;
    const project = host.project();
    if (current === null || project === null || busy) {
      return;
    }
    busy = true;
    allowChoice(false);
    const { remoteBranch } = current.lost;
    message.textContent = t(localCommits === "drop" ? "localCommitsDropping" : "localCommitsRebasing", { remoteBranch });
    try {
      // A rebase that stops on a conflict opens the conflicts dialog, and is in progress.
      await checkOutRemoteBranch(project, current.repository, remoteBranch, localCommits);
      busy = false;
      close();
    } catch (error) {
      busy = false;
      const blocked = checkoutBlocked(error);
      if (blocked !== null) {
        close();
        host.checkoutBlocked(current.repository, { kind: "remoteBranch", name: remoteBranch, localCommits }, blocked);
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
