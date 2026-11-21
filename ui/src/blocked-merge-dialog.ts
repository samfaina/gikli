// The blocked merge dialog: files keep git from merging a branch into the current one, and the dialog lists them. Local changes the
// merge would overwrite can be saved, merged over and restored in their Changelists through Saved local changes; files git does not
// track are for the user to move or commit. The core does all of it; this only draws the dialog.

import { mergeBlocked, mergeOverLocalChanges, type RefLabel, type Repository } from "./core";
import { element, folderName } from "./dom";
import { t } from "./strings";

/** The files that block a merge, as the core tells them. */
export type BlockedMerge = NonNullable<ReturnType<typeof mergeBlocked>>;

/** What the blocked merge dialog needs from the window around it. */
export interface BlockedMergeDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The folder of the Project the window shows, or null when it shows none. */
  project(): string | null;
  /** HEAD of `repository` may have moved, and its files changed, even when the merge failed. */
  headMoved(repository: Repository): void;
}

export interface BlockedMergeDialog {
  /** Opens the dialog on `blocked`, the files that keep `branch` from being merged into the current branch of `repository`. */
  open(repository: Repository, branch: RefLabel, blocked: BlockedMerge): void;
  /** Closes the dialog. */
  close(): void;
}

export function createBlockedMergeDialog(host: BlockedMergeDialogHost): BlockedMergeDialog {
  const view = element<HTMLElement>("blocked-merge-dialog");
  const title = element<HTMLElement>("blocked-merge-title");
  const localGroup = element<HTMLElement>("blocked-merge-local");
  const localExplanation = element<HTMLElement>("blocked-merge-local-explanation");
  const localFiles = element<HTMLElement>("blocked-merge-local-files");
  const untrackedGroup = element<HTMLElement>("blocked-merge-untracked");
  const untrackedExplanation = element<HTMLElement>("blocked-merge-untracked-explanation");
  const untrackedFiles = element<HTMLElement>("blocked-merge-untracked-files");
  const actions = element<HTMLElement>("blocked-merge-actions");
  const saveButton = element<HTMLButtonElement>("blocked-merge-save");
  const cancelButton = element<HTMLButtonElement>("blocked-merge-cancel");
  const message = element<HTMLElement>("blocked-merge-message");

  /** The merge the dialog offers to go on with, while it is open. */
  let shown: { repository: Repository; branch: RefLabel } | null = null;
  let busy = false;

  saveButton.textContent = t("blockedMergeSave");
  cancelButton.textContent = t("blockedMergeCancel");
  saveButton.addEventListener("click", () => void merge());
  cancelButton.addEventListener("click", close);
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(repository: Repository, branch: RefLabel, blocked: BlockedMerge): void {
    shown = { repository, branch };
    const name = folderName(repository.root);
    title.textContent = t("blockedMergeTitle", { branch: branch.name });
    title.title = repository.root;
    localExplanation.textContent = t("blockedMergeLocalChanges", { name });
    untrackedExplanation.textContent = t("blockedMergeUntrackedFiles", { name });
    list(localFiles, blocked.localChanges);
    list(untrackedFiles, blocked.untrackedFiles);
    localGroup.hidden = blocked.localChanges.length === 0;
    untrackedGroup.hidden = blocked.untrackedFiles.length === 0;
    // Saving local changes leaves the files git does not track in the way, so only moving them unblocks it.
    actions.hidden = blocked.localChanges.length === 0 || blocked.untrackedFiles.length > 0;
    message.replaceChildren();
    saveButton.disabled = false;
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

  async function merge(): Promise<void> {
    const current = shown;
    const project = host.project();
    if (current === null || project === null || busy) {
      return;
    }
    busy = true;
    saveButton.disabled = true;
    message.textContent = t("blockedMergeSaving", { branch: current.branch.name });
    try {
      // A merge that stops on a conflict opens the conflicts dialog, and keeps the changes saved until it is over.
      await mergeOverLocalChanges(project, current.repository, current.branch);
      busy = false;
      close();
    } catch (error) {
      busy = false;
      const stillBlocked = mergeBlocked(error);
      if (stillBlocked !== null) {
        // Files git does not track still block it: the dialog shows them.
        open(current.repository, current.branch, stillBlocked);
      } else {
        message.replaceChildren(host.errorView(error));
        saveButton.disabled = false;
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
      item.dataset.testid = "blocked-merge-file";
      item.textContent = path;
      return item;
    }),
  );
}
