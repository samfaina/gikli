// A row of a list of commits, as the push dialog lists those a push would send and the unmerged
// commits view those a deleted branch took away. Each list makes its own row; this fills it.

import type { ListedCommit } from "./core";
import { SHORT_HASH_LENGTH } from "./log-details";

const dates = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });

/**
 * Fills `row` with the hash, subject, author and date cells of `commit`, and returns the subject
 * cell for the caller to mark.
 */
export function fillCommitRow(row: HTMLElement, commit: ListedCommit): HTMLElement {
  const subject = cell("log-subject", commit.subject);
  row.append(
    cell("log-hash", commit.hash.slice(0, SHORT_HASH_LENGTH), commit.hash),
    subject,
    cell("log-author", commit.author),
    cell("log-date", dates.format(new Date(commit.committedAt * 1000))),
  );
  return subject;
}

/** A cell showing `text`, and all of `full` as its tooltip. */
function cell(className: string, text: string, full = text): HTMLElement {
  const made = document.createElement("span");
  made.className = className;
  made.textContent = text;
  made.title = full;
  return made;
}
