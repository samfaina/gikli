// The conflicts dialog: the files an operation left in conflict, each with a button to accept yours, to accept theirs, and to mark
// it resolved once edited by hand. The core tells what each side did, from the user's side even during a rebase, and resolves the
// files; closing the dialog leaves them in conflict, with a notification to come back. This only draws the dialog.

import {
  acceptSide,
  closeConflicts,
  conflicts,
  markResolved,
  type ConflictedFile,
  type ConflictSide,
  type Repository,
  type SideChange,
} from "./core";
import { element, folderName } from "./dom";
import type { StringKey } from "./strings";
import { t } from "./strings";

/** What the conflicts dialog needs from the window around it. */
export interface ConflictsDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The folder of the Project shown, which the notification left on closing is about. */
  project(): string | null;
  /** The conflicts of `repository` may have changed, as after resolving files: its changes and the operation in progress are read again. */
  conflictsChanged(repository: Repository): void;
}

/** The Repository the dialog shows, with the Project it is in. */
interface Shown {
  project: string;
  repository: Repository;
}

export interface ConflictsDialog {
  /** Opens the dialog on the files in conflict in `repository`. */
  open(repository: Repository): void;
  /** Closes the dialog. Files still in conflict stay so, and the core raises the notification that brings it back. */
  close(): void;
}

const SIDE_CHANGES: Record<SideChange, StringKey> = {
  modified: "conflictSideModified",
  added: "conflictSideAdded",
  deleted: "conflictSideDeleted",
  absent: "conflictSideAbsent",
};

export function createConflictsDialog(host: ConflictsDialogHost): ConflictsDialog {
  const view = element<HTMLElement>("conflicts");
  const title = element<HTMLElement>("conflicts-title");
  const list = element<HTMLUListElement>("conflicts-list");
  const message = element<HTMLElement>("conflicts-message");
  const closeButton = element<HTMLButtonElement>("conflicts-close");

  let shown: Shown | null = null;

  closeButton.textContent = t("conflictsClose");
  closeButton.addEventListener("click", close);
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(repository: Repository): void {
    const project = host.project();
    if (project === null) {
      return;
    }
    if (shown !== null && shown.repository.root === repository.root) {
      void read(shown);
      return;
    }
    const current: Shown = { project, repository };
    shown = current;
    title.textContent = t("conflictsTitle", { name: folderName(repository.root) });
    title.title = repository.root;
    list.replaceChildren();
    message.replaceChildren();
    view.hidden = false;
    closeButton.focus();
    void read(current);
  }

  function close(): void {
    const current = shown;
    if (current === null) {
      return;
    }
    shown = null;
    view.hidden = true;
    closeConflicts(current.project, current.repository).catch(() => {
      // Without the notification the conflicts still show as a group in the changes view, with the way to resolve them.
    });
  }

  async function read(current: Shown): Promise<void> {
    try {
      show(current, await conflicts(current.project, current.repository));
    } catch (error) {
      showError(current, error);
    }
  }

  function showError(current: Shown, error: unknown): void {
    if (shown === current) {
      message.replaceChildren(host.errorView(error));
    }
  }

  /** Lists `files`, the files still in conflict, or says that none is. */
  function show(current: Shown, files: ConflictedFile[]): void {
    if (shown !== current) {
      return;
    }
    list.replaceChildren(...files.map((file) => row(current, file)));
    message.textContent = files.length === 0 ? t("conflictsAllResolved") : "";
  }

  function row(current: Shown, file: ConflictedFile): HTMLLIElement {
    const item = document.createElement("li");
    item.className = "conflict";
    item.dataset.testid = "conflict";
    item.dataset.path = file.path;
    const path = document.createElement("span");
    path.className = "conflict-path";
    path.textContent = file.path;
    const sides = document.createElement("span");
    sides.className = "conflict-sides";
    sides.textContent = t("conflictSides", { yours: t(SIDE_CHANGES[file.yours]), theirs: t(SIDE_CHANGES[file.theirs]) });
    const buttons = [
      button("accept-yours", acceptLabel("yours", file.yours), () => acceptSide(current.project, current.repository, [file.path], "yours")),
      button("accept-theirs", acceptLabel("theirs", file.theirs), () => acceptSide(current.project, current.repository, [file.path], "theirs")),
      button("mark-resolved", t("conflictMarkResolved"), () => markResolved(current.project, current.repository, [file.path])),
    ];
    function button(testId: string, text: string, resolve: () => Promise<ConflictedFile[]>): HTMLButtonElement {
      const control = document.createElement("button");
      control.type = "button";
      control.dataset.testid = testId;
      control.textContent = text;
      control.addEventListener("click", async () => {
        buttons.forEach((other) => (other.disabled = true));
        try {
          const left = await resolve();
          show(current, left);
        } catch (error) {
          buttons.forEach((other) => (other.disabled = false));
          showError(current, error);
        } finally {
          // A command that failed may still have resolved some files.
          host.conflictsChanged(current.repository);
        }
      });
      return control;
    }
    const actions = document.createElement("span");
    actions.className = "conflict-actions";
    actions.append(...buttons);
    item.append(path, sides, actions);
    return item;
  }

  return { open, close };
}

/** The label of the button that accepts `side`, which deletes the file when that side does not have it. */
function acceptLabel(side: ConflictSide, change: SideChange): string {
  if (change === "deleted" || change === "absent") {
    return t(side === "yours" ? "conflictAcceptYoursDeletion" : "conflictAcceptTheirsDeletion");
  }
  return t(side === "yours" ? "conflictAcceptYours" : "conflictAcceptTheirs");
}
