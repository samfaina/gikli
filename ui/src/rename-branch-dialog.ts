// The rename dialog: a new name for a local branch, and whether it lets go of its upstream. The core
// cleans the name as it is typed, and checks and renames the branch; this only draws the dialog.

import { cleanBranchName, localBranches, renameBranch, type Repository } from "./core";
import { element, folderName } from "./dom";
import { t } from "./strings";

/** What the rename dialog needs from the window around it. */
export interface RenameBranchDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** A branch of `repository` was renamed, maybe the one HEAD is on. */
  branchesMoved(repository: Repository): void;
}

export interface RenameBranchDialog {
  /** Opens the dialog on the local branch `name` of `repository`. */
  open(repository: Repository, name: string): void;
  /** Closes the dialog. */
  close(): void;
}

export function createRenameBranchDialog(host: RenameBranchDialogHost): RenameBranchDialog {
  const view = element<HTMLElement>("rename-branch");
  const title = element<HTMLElement>("rename-branch-title");
  const nameInput = element<HTMLInputElement>("rename-branch-name");
  const unsetOption = element<HTMLElement>("rename-branch-unset-upstream-option");
  const unsetInput = element<HTMLInputElement>("rename-branch-unset-upstream");
  const unsetLabel = element<HTMLElement>("rename-branch-unset-upstream-label");
  const message = element<HTMLElement>("rename-branch-message");
  const renameButton = element<HTMLButtonElement>("rename-branch-rename");
  const cancelButton = element<HTMLButtonElement>("rename-branch-cancel");

  let shown: { repository: Repository; name: string } | null = null;

  element<HTMLElement>("rename-branch-name-label").textContent = t("renameBranchName");
  renameButton.textContent = t("renameBranchRename");
  cancelButton.textContent = t("renameBranchCancel");
  renameButton.addEventListener("click", () => void rename());
  cancelButton.addEventListener("click", close);
  nameInput.addEventListener("input", () => void clean());
  nameInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void rename();
    }
  });
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(repository: Repository, name: string): void {
    const current = { repository, name };
    shown = current;
    title.textContent = t("renameBranchTitle", { branch: name, name: folderName(repository.root) });
    title.title = repository.root;
    nameInput.value = name;
    unsetInput.checked = false;
    unsetOption.hidden = true;
    message.replaceChildren();
    renameButton.disabled = false;
    view.hidden = false;
    nameInput.select();
    nameInput.focus();
    void showUpstream(current);
  }

  /** Offers to unset the upstream only when the branch has one. */
  async function showUpstream(current: { repository: Repository; name: string }): Promise<void> {
    let upstream: string | null = null;
    try {
      upstream = (await localBranches(current.repository)).find((branch) => branch.name === current.name)?.upstream?.name ?? null;
    } catch {
      // Without the upstream the option stays hidden, and the rename keeps it.
    }
    if (shown !== current || upstream === null) {
      return;
    }
    unsetLabel.textContent = t("renameBranchUnsetUpstream", { upstream });
    unsetOption.hidden = false;
  }

  function close(): void {
    shown = null;
    view.hidden = true;
  }

  /** Puts `-` in place of each character git takes in no branch name, as the user types, as the new branch dialog does. */
  async function clean(): Promise<void> {
    const typed = nameInput.value;
    const cleaned = await cleanBranchName(typed);
    if (nameInput.value !== typed || cleaned === typed) {
      return;
    }
    const { selectionStart, selectionEnd } = nameInput;
    nameInput.value = cleaned;
    nameInput.setSelectionRange(selectionStart, selectionEnd);
  }

  async function rename(): Promise<void> {
    const current = shown;
    if (current === null || renameButton.disabled) {
      return;
    }
    renameButton.disabled = true;
    message.textContent = t("renameBranchRenaming");
    try {
      // Cleaned here too, as a name pasted and renamed at once may not have been yet.
      const newName = await cleanBranchName(nameInput.value);
      if (shown === current) {
        nameInput.value = newName;
      }
      await renameBranch(current.repository, current.name, newName, !unsetOption.hidden && unsetInput.checked);
      host.branchesMoved(current.repository);
      if (shown === current) {
        close();
      }
    } catch (error) {
      if (shown === current) {
        message.replaceChildren(host.errorView(error));
        renameButton.disabled = false;
        nameInput.focus();
      }
    }
  }

  return { open, close };
}
