// The delete dialog: the local branches to delete, one or several, merged or not. The current branch
// is not on offer, as git keeps it. The core deletes them and raises the notification that can bring
// them back; this only draws the dialog.

import { currentBranch, deleteBranches, localBranches, type Repository } from "./core";
import { element, folderName } from "./dom";
import { t } from "./strings";

/** What the delete dialog needs from the window around it. */
export interface DeleteBranchesDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The folder of the Project shown, which the notification of the deletion is about. */
  project(): string | null;
  /** Branches of `repository` were deleted. */
  branchesMoved(repository: Repository): void;
}

export interface DeleteBranchesDialog {
  /** Opens the dialog on the local branches of `repository`, with `ticked` ticked when it is one of them. */
  open(repository: Repository, ticked: string | null): void;
  /** Closes the dialog. */
  close(): void;
}

export function createDeleteBranchesDialog(host: DeleteBranchesDialogHost): DeleteBranchesDialog {
  const view = element<HTMLElement>("delete-branches");
  const title = element<HTMLElement>("delete-branches-title");
  const list = element<HTMLUListElement>("delete-branches-list");
  const message = element<HTMLElement>("delete-branches-message");
  const deleteButton = element<HTMLButtonElement>("delete-branches-delete");
  const cancelButton = element<HTMLButtonElement>("delete-branches-cancel");

  let shown: { repository: Repository } | null = null;

  deleteButton.textContent = t("deleteBranchesDelete");
  cancelButton.textContent = t("deleteBranchesCancel");
  deleteButton.addEventListener("click", () => void remove());
  cancelButton.addEventListener("click", close);
  list.addEventListener("change", showTicked);
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(repository: Repository, ticked: string | null): void {
    const current = { repository };
    shown = current;
    title.textContent = t("deleteBranchesTitle", { name: folderName(repository.root) });
    title.title = repository.root;
    list.replaceChildren();
    message.replaceChildren();
    deleteButton.disabled = true;
    view.hidden = false;
    void listBranches(current, ticked);
  }

  /** Lists the local branches other than the current one, each with a box to tick. */
  async function listBranches(current: { repository: Repository }, ticked: string | null): Promise<void> {
    let names: string[];
    try {
      const [branches, head] = await Promise.all([localBranches(current.repository), currentBranch(current.repository)]);
      const onBranch = head.kind === "branch" ? head.name : null;
      names = branches.map((branch) => branch.name).filter((name) => name !== onBranch);
    } catch (error) {
      if (shown === current) {
        message.replaceChildren(host.errorView(error));
      }
      return;
    }
    if (shown !== current) {
      return;
    }
    if (names.length === 0) {
      message.textContent = t("deleteBranchesNone");
    }
    list.replaceChildren(
      ...names.map((name) => {
        const item = document.createElement("li");
        const label = document.createElement("label");
        const box = document.createElement("input");
        box.type = "checkbox";
        box.value = name;
        box.checked = name === ticked;
        box.dataset.testid = "delete-branches-branch";
        box.dataset.name = name;
        const text = document.createElement("span");
        text.textContent = name;
        label.append(box, text);
        item.append(label);
        return item;
      }),
    );
    showTicked();
  }

  /** The names of the branches ticked, in the order listed. */
  function tickedNames(): string[] {
    return [...list.querySelectorAll<HTMLInputElement>("input[type=checkbox]")].filter((box) => box.checked).map((box) => box.value);
  }

  function showTicked(): void {
    deleteButton.disabled = tickedNames().length === 0;
  }

  function close(): void {
    shown = null;
    view.hidden = true;
  }

  async function remove(): Promise<void> {
    const current = shown;
    const project = host.project();
    const names = tickedNames();
    if (current === null || project === null || names.length === 0 || deleteButton.disabled) {
      return;
    }
    deleteButton.disabled = true;
    message.textContent = t("deleteBranchesDeleting");
    try {
      await deleteBranches(project, current.repository, names);
      host.branchesMoved(current.repository);
      if (shown === current) {
        close();
      }
    } catch (error) {
      // Some branches may have gone before the one git could not delete.
      host.branchesMoved(current.repository);
      if (shown === current) {
        message.replaceChildren(host.errorView(error));
        void listBranches(current, null);
      }
    }
  }

  return { open, close };
}
