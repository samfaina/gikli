// The new branch dialog: a name for a branch that starts from HEAD, a branch or a commit, and
// whether to check it out. The core cleans the name as it is typed, suggests the prefixes of
// existing branches, and checks and creates the branch; this only draws the dialog.

import { cleanBranchName, createBranch, newBranchPrefixes, type NewBranchBase, type Repository } from "./core";
import { element, folderName } from "./dom";
import { t } from "./strings";

/** What the new branch dialog needs from the window around it. */
export interface NewBranchDialogHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** A branch was created in `repository`, and HEAD may have moved to it. */
  branchesMoved(repository: Repository): void;
}

export interface NewBranchDialog {
  /** Opens the dialog on a new branch of `repository` that starts from `base`, which the user knows as `baseName`. */
  open(repository: Repository, base: NewBranchBase, baseName: string): void;
  /** Closes the dialog. */
  close(): void;
}

/** How long the user stops typing before the prefixes are looked up, in milliseconds. */
const SUGGESTION_DELAY = 150;

/**
 * Puts `-` in place of each character git takes in no branch name in `input`, as the user types. The name keeps its length, so
 * the cursor stays where it was. An answer that later typing overtook is dropped.
 */
export async function cleanAsTyped(input: HTMLInputElement): Promise<void> {
  const typed = input.value;
  const cleaned = await cleanBranchName(typed);
  if (input.value !== typed || cleaned === typed) {
    return;
  }
  const { selectionStart, selectionEnd } = input;
  input.value = cleaned;
  input.setSelectionRange(selectionStart, selectionEnd);
}

export function createNewBranchDialog(host: NewBranchDialogHost): NewBranchDialog {
  const view = element<HTMLElement>("new-branch");
  const title = element<HTMLElement>("new-branch-title");
  const nameInput = element<HTMLInputElement>("new-branch-name");
  const prefixes = element<HTMLDataListElement>("new-branch-prefixes");
  const checkOutInput = element<HTMLInputElement>("new-branch-check-out");
  const message = element<HTMLElement>("new-branch-message");
  const createButton = element<HTMLButtonElement>("new-branch-create");
  const cancelButton = element<HTMLButtonElement>("new-branch-cancel");

  let shown: { repository: Repository; base: NewBranchBase } | null = null;
  /** Tells the answer to the latest lookup of prefixes from one that later typing overtook. */
  let latestLookup = 0;
  let suggestionTimer: number | undefined;

  element<HTMLElement>("new-branch-name-label").textContent = t("newBranchName");
  element<HTMLElement>("new-branch-check-out-label").textContent = t("newBranchCheckOut");
  createButton.textContent = t("newBranchCreate");
  cancelButton.textContent = t("newBranchCancel");
  createButton.addEventListener("click", () => void create());
  cancelButton.addEventListener("click", close);
  nameInput.addEventListener("input", () => void clean());
  nameInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void create();
    }
  });
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(repository: Repository, base: NewBranchBase, baseName: string): void {
    shown = { repository, base };
    title.textContent = t("newBranchTitle", { base: baseName, name: folderName(repository.root) });
    title.title = repository.root;
    nameInput.value = "";
    checkOutInput.checked = true;
    message.replaceChildren();
    createButton.disabled = false;
    view.hidden = false;
    scheduleSuggestions();
    nameInput.focus();
  }

  function close(): void {
    shown = null;
    latestLookup += 1;
    window.clearTimeout(suggestionTimer);
    prefixes.replaceChildren();
    view.hidden = true;
  }

  async function clean(): Promise<void> {
    scheduleSuggestions();
    await cleanAsTyped(nameInput);
  }

  function scheduleSuggestions(): void {
    window.clearTimeout(suggestionTimer);
    suggestionTimer = window.setTimeout(() => void suggest(), SUGGESTION_DELAY);
  }

  /** Suggests the prefixes of existing branches that start with what was typed. */
  async function suggest(): Promise<void> {
    const current = shown;
    if (current === null) {
      return;
    }
    const lookup = ++latestLookup;
    let found: string[];
    try {
      found = await newBranchPrefixes(current.repository, nameInput.value);
    } catch {
      // Without suggestions the user can still type the whole name.
      found = [];
    }
    if (lookup !== latestLookup || shown !== current) {
      return;
    }
    prefixes.replaceChildren(
      ...found.map((prefix) => {
        const option = document.createElement("option");
        option.value = prefix;
        return option;
      }),
    );
  }

  async function create(): Promise<void> {
    const current = shown;
    if (current === null || createButton.disabled) {
      return;
    }
    createButton.disabled = true;
    message.textContent = t("newBranchCreating");
    try {
      // Cleaned here too, as a name pasted and created at once may not have been yet.
      const name = await cleanBranchName(nameInput.value);
      if (shown === current) {
        nameInput.value = name;
      }
      await createBranch(current.repository, name, current.base, checkOutInput.checked);
      host.branchesMoved(current.repository);
      if (shown === current) {
        close();
      }
    } catch (error) {
      if (shown === current) {
        message.replaceChildren(host.errorView(error));
        createButton.disabled = false;
        nameInput.focus();
      }
    }
  }

  return { open, close };
}
