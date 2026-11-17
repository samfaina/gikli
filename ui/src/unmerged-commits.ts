// The commits a deleted branch took away, as the link of its notification shows them: those that
// HEAD did not have when the branch went. The core finds them; this only lists them.

import type { ListedCommit } from "./core";
import { element } from "./dom";
import { fillCommitRow } from "./listed-commit";
import { t } from "./strings";

export interface UnmergedCommitsView {
  /** Shows `commits`, newest first, the ones the deleted branch `branch` took away. */
  open(branch: string, commits: ListedCommit[]): void;
  /** Closes the view. */
  close(): void;
}

export function createUnmergedCommitsView(): UnmergedCommitsView {
  const view = element<HTMLElement>("unmerged-commits");
  const title = element<HTMLElement>("unmerged-commits-title");
  const list = element<HTMLUListElement>("unmerged-commits-list");
  const closeButton = element<HTMLButtonElement>("unmerged-commits-close");

  closeButton.textContent = t("unmergedCommitsClose");
  closeButton.addEventListener("click", close);
  view.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  });

  function open(branch: string, commits: ListedCommit[]): void {
    title.textContent = t("unmergedCommitsTitle", { branch });
    list.replaceChildren(
      ...commits.map((commit) => {
        const item = document.createElement("li");
        item.className = "push-commit";
        item.dataset.testid = "unmerged-commit";
        item.dataset.hash = commit.hash;
        fillCommitRow(item, commit);
        return item;
      }),
    );
    view.hidden = false;
    closeButton.focus();
  }

  function close(): void {
    view.hidden = true;
  }

  return { open, close };
}
