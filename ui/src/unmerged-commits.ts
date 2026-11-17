// The commits a deleted branch took away, as the link of its notification shows them: those that
// HEAD did not have when the branch went. The core finds them; this only lists them.

import type { ListedCommit } from "./core";
import { element } from "./dom";
import { SHORT_HASH_LENGTH } from "./log-details";
import { t } from "./strings";

export interface UnmergedCommitsView {
  /** Shows `commits`, newest first, the ones the deleted branch `branch` took away. */
  open(branch: string, commits: ListedCommit[]): void;
  /** Closes the view. */
  close(): void;
}

const dates = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });

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
        const cell = (className: string, text: string, full = text): HTMLElement => {
          const made = document.createElement("span");
          made.className = className;
          made.textContent = text;
          made.title = full;
          return made;
        };
        item.append(
          cell("log-hash", commit.hash.slice(0, SHORT_HASH_LENGTH), commit.hash),
          cell("log-subject", commit.subject),
          cell("log-author", commit.author),
          cell("log-date", dates.format(new Date(commit.committedAt * 1000))),
        );
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
