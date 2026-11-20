// The stash dialog: the Repository to stash in, with the branch HEAD is on, a message, and whether
// the stash keeps the index and includes the files git does not track. The core stashes and says
// when there is nothing to stash or the Repository cannot stash; this only draws the dialog.

import { currentBranch, stash, stashes, type Repository, type StashUnavailable } from "./core";
import { element } from "./dom";
import { SHORT_HASH_LENGTH } from "./log-details";
import { t, type StringKey } from "./strings";

/** What the stash dialog needs from the window around it. */
export interface StashDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The changes of `repository` went into a stash. */
  stashed(repository: Repository): void;
}

export interface StashDialog {
  /** Opens the dialog on `repository`, one of the `repositories` of the Project the user can choose from. */
  open(repository: Repository, repositories: Repository[]): void;
  /** Closes the dialog. */
  close(): void;
}

/** What the user reads when a Repository cannot stash. */
export const STASH_UNAVAILABLE_LABELS: Record<StashUnavailable, StringKey> = {
  noCommits: "stashUnavailableNoCommits",
};

export function createStashDialog(host: StashDialogHost): StashDialog {
  const view = element<HTMLElement>("stash-dialog");
  const title = element<HTMLElement>("stash-dialog-title");
  const repositorySelect = element<HTMLSelectElement>("stash-dialog-repository");
  const branch = element<HTMLElement>("stash-dialog-branch");
  const messageInput = element<HTMLInputElement>("stash-dialog-message");
  const keepIndexInput = element<HTMLInputElement>("stash-dialog-keep-index");
  const includeUntrackedInput = element<HTMLInputElement>("stash-dialog-include-untracked");
  const stashButton = element<HTMLButtonElement>("stash-dialog-stash");
  const cancelButton = element<HTMLButtonElement>("stash-dialog-cancel");
  const status = element<HTMLElement>("stash-dialog-status");

  let repositories: Repository[] = [];
  /** Tells the answer about the Repository chosen last from one about a Repository chosen before. */
  let latestChoice = 0;
  let stashing = false;

  title.textContent = t("stashDialogTitle");
  element<HTMLElement>("stash-dialog-repository-label").textContent = t("stashDialogRepository");
  element<HTMLElement>("stash-dialog-message-label").textContent = t("stashDialogMessage");
  element<HTMLElement>("stash-dialog-keep-index-label").textContent = t("stashDialogKeepIndex");
  element<HTMLElement>("stash-dialog-include-untracked-label").textContent = t("stashDialogIncludeUntracked");
  stashButton.textContent = t("stashDialogStash");
  cancelButton.textContent = t("stashDialogCancel");
  stashButton.addEventListener("click", () => void stashChanges());
  cancelButton.addEventListener("click", close);
  repositorySelect.addEventListener("change", () => void showChosen());
  messageInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void stashChanges();
    }
  });
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(repository: Repository, ofProject: Repository[]): void {
    repositories = ofProject.some((other) => other.root === repository.root) ? ofProject : [repository, ...ofProject];
    repositorySelect.replaceChildren(...repositories.map((other, index) => new Option(other.root, String(index))));
    repositorySelect.value = String(repositories.findIndex((other) => other.root === repository.root));
    messageInput.value = "";
    keepIndexInput.checked = false;
    includeUntrackedInput.checked = false;
    view.hidden = false;
    messageInput.focus();
    void showChosen();
  }

  function close(): void {
    latestChoice += 1;
    view.hidden = true;
  }

  function chosen(): Repository | null {
    return repositories[Number(repositorySelect.value)] ?? null;
  }

  /** Shows the branch HEAD of the chosen Repository is on, and whether it can stash at all. */
  async function showChosen(): Promise<void> {
    const repository = chosen();
    const thisChoice = ++latestChoice;
    status.replaceChildren();
    branch.textContent = "";
    stashButton.disabled = true;
    if (repository === null) {
      return;
    }
    const [head, listed] = await Promise.allSettled([currentBranch(repository), stashes(repository)]);
    if (thisChoice !== latestChoice) {
      return;
    }
    if (head.status === "fulfilled") {
      branch.textContent =
        head.value.kind === "branch"
          ? t("stashDialogBranch", { branch: head.value.name })
          : t("stashDialogDetached", { hash: head.value.hash.slice(0, SHORT_HASH_LENGTH) });
    }
    if (listed.status === "rejected") {
      status.replaceChildren(host.errorView(listed.reason));
    } else if (listed.value.unavailable !== null) {
      status.textContent = t(STASH_UNAVAILABLE_LABELS[listed.value.unavailable]);
      return;
    }
    stashButton.disabled = stashing;
  }

  async function stashChanges(): Promise<void> {
    const repository = chosen();
    if (repository === null || stashButton.disabled) {
      return;
    }
    const thisChoice = latestChoice;
    stashing = true;
    stashButton.disabled = true;
    status.textContent = t("stashDialogStashing");
    try {
      await stash(repository, {
        message: messageInput.value,
        keepIndex: keepIndexInput.checked,
        includeUntracked: includeUntrackedInput.checked,
      });
      host.stashed(repository);
      if (thisChoice === latestChoice) {
        close();
      }
    } catch (error) {
      if (thisChoice === latestChoice) {
        status.replaceChildren(host.errorView(error));
        messageInput.focus();
      }
    } finally {
      stashing = false;
      if (thisChoice === latestChoice) {
        stashButton.disabled = false;
      }
    }
  }

  return { open, close };
}
