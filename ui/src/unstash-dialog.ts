// The unstash dialog: the stash to bring back, a warning when tracked files have uncommitted
// changes, whether to restore what was staged, and a name for a new branch. Apply keeps the stash,
// Pop drops it once it applied, and Branch creates the branch from it. The core does all of it and
// keeps a stash that conflicts; this only draws the dialog.

import { applyStash, cleanBranchName, stashBranch, unstashPlan, type Repository, type Stash, type Unstashed } from "./core";
import { element } from "./dom";
import { cleanAsTyped } from "./new-branch-dialog";
import { t } from "./strings";

/** What the unstash dialog needs from the window around it. */
export interface UnstashDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The folder of the Project the window shows, or null when it shows none. */
  project(): string | null;
  /** The stash came back into `repository`, which may now have files in conflict or be on a new branch. */
  unstashed(repository: Repository): void;
}

export interface UnstashDialog {
  /** Opens the dialog on `stash` of `repository`. */
  open(repository: Repository, stash: Stash): void;
  /** Closes the dialog. */
  close(): void;
}

export function createUnstashDialog(host: UnstashDialogHost): UnstashDialog {
  const view = element<HTMLElement>("unstash-dialog");
  const title = element<HTMLElement>("unstash-dialog-title");
  const stashLine = element<HTMLElement>("unstash-dialog-stash");
  const warning = element<HTMLElement>("unstash-dialog-warning");
  const restoreIndexInput = element<HTMLInputElement>("unstash-dialog-restore-index");
  const branchInput = element<HTMLInputElement>("unstash-dialog-branch");
  const applyButton = element<HTMLButtonElement>("unstash-dialog-apply");
  const popButton = element<HTMLButtonElement>("unstash-dialog-pop");
  const branchButton = element<HTMLButtonElement>("unstash-dialog-create-branch");
  const cancelButton = element<HTMLButtonElement>("unstash-dialog-cancel");
  const status = element<HTMLElement>("unstash-dialog-status");

  let shown: { repository: Repository; stash: Stash } | null = null;
  /** Tells the answer about the stash opened last from one about a stash opened before. */
  let latestOpen = 0;
  let busy = false;

  title.textContent = t("unstashDialogTitle");
  element<HTMLElement>("unstash-dialog-restore-index-label").textContent = t("unstashDialogRestoreIndex");
  element<HTMLElement>("unstash-dialog-branch-label").textContent = t("unstashDialogBranch");
  applyButton.textContent = t("unstashDialogApply");
  popButton.textContent = t("unstashDialogPop");
  branchButton.textContent = t("unstashDialogCreateBranch");
  cancelButton.textContent = t("unstashDialogCancel");
  applyButton.addEventListener("click", () => void unstash(false));
  popButton.addEventListener("click", () => void unstash(true));
  branchButton.addEventListener("click", () => void branch());
  cancelButton.addEventListener("click", close);
  branchInput.addEventListener("input", () => {
    enable();
    void cleanAsTyped(branchInput);
  });
  branchInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void branch();
    }
  });
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(repository: Repository, stash: Stash): void {
    shown = { repository, stash };
    const thisOpen = ++latestOpen;
    stashLine.textContent = t("unstashDialogStash", { name: stash.name, message: stash.message });
    warning.textContent = "";
    warning.hidden = true;
    status.replaceChildren();
    restoreIndexInput.checked = false;
    branchInput.value = "";
    enable();
    view.hidden = false;
    popButton.focus();
    void unstashPlan(repository, stash.hash).then(
      (plan) => {
        if (thisOpen === latestOpen && plan.localChanges) {
          warning.textContent = t("unstashDialogLocalChanges");
          warning.hidden = false;
        }
      },
      (error: unknown) => {
        if (thisOpen === latestOpen) {
          status.replaceChildren(host.errorView(error));
        }
      },
    );
  }

  function close(): void {
    latestOpen += 1;
    shown = null;
    view.hidden = true;
  }

  function enable(): void {
    applyButton.disabled = busy;
    popButton.disabled = busy;
    branchButton.disabled = busy || branchInput.value.trim() === "";
  }

  async function unstash(pop: boolean): Promise<void> {
    await run((project, repository, stash) =>
      applyStash(project, repository, stash.hash, { pop, restoreIndex: restoreIndexInput.checked }),
    );
  }

  async function branch(): Promise<void> {
    if (branchButton.disabled) {
      return;
    }
    const name = await cleanBranchName(branchInput.value);
    await run((project, repository, stash) => stashBranch(project, repository, stash.hash, name));
  }

  /** Runs `change` on the stash shown, and closes the dialog once it came back, conflicts or not. */
  async function run(change: (project: string, repository: Repository, stash: Stash) => Promise<Unstashed>): Promise<void> {
    const project = host.project();
    const current = shown;
    if (project === null || current === null || busy) {
      return;
    }
    const thisOpen = latestOpen;
    busy = true;
    enable();
    status.textContent = t("unstashDialogApplying");
    try {
      await change(project, current.repository, current.stash);
      host.unstashed(current.repository);
      if (thisOpen === latestOpen) {
        close();
      }
    } catch (error) {
      host.unstashed(current.repository);
      if (thisOpen === latestOpen) {
        status.replaceChildren(host.errorView(error));
      }
    } finally {
      busy = false;
      enable();
    }
  }

  return { open, close };
}
