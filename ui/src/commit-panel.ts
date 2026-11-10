// The panel next to the changes where the user commits a Changelist or selected files. It blocks nothing: the changes stay in reach while it is open.

import {
  amendLastCommit,
  commitFiles,
  commitWarning,
  isFailure,
  lastCommitMessage,
  prefilledCommitMessage,
  previousCommitMessages,
  setCommitIdentity,
  settings,
  type Change,
  type ChangeSet,
  type CommitWarning,
  type Repository,
} from "./core";
import { t } from "./strings";

/** What the panel needs from the view of the Repository around it. */
export interface CommitPanelHost {
  repository: Repository;
  /** What to say about a change besides its path. */
  describeChange(change: Change): string;
  /** A commit went through, so the changes need reading again. */
  committed(): void;
  /** What to show for an error from the core. `retry` runs once the user has done what it offers. */
  errorView(error: unknown, retry?: () => void): HTMLElement;
}

export interface CommitPanel {
  view: HTMLElement;
  /** The changes were read again and are now `changeSet`. */
  drawn(changeSet: ChangeSet): void;
}

/** The files the panel shows under one heading. */
interface FileGroup {
  /** Tells this group's checkbox from the others, when the panel is drawn again. */
  key: string;
  testId: string;
  name: string;
  changes: Change[];
}

/** Lets the user tick files, with the Active changelist ticked to begin with, and commit them. */
export function createCommitPanel(host: CommitPanelHost): CommitPanel {
  const view = document.createElement("form");
  view.className = "changelist-form commit-panel";
  view.dataset.testid = "commit-panel";
  view.setAttribute("aria-label", t("commitTitle"));
  const title = document.createElement("h3");
  title.textContent = t("commitTitle");
  const files = document.createElement("div");
  files.className = "commit-files";

  const amend = document.createElement("input");
  amend.type = "checkbox";
  amend.dataset.testid = "commit-amend";
  const amendLabel = document.createElement("label");
  amendLabel.className = "changelist-form-check";
  amendLabel.append(amend, document.createTextNode(t("commitAmend")));

  const message = document.createElement("textarea");
  message.dataset.testid = "commit-message";
  message.rows = 4;
  message.spellcheck = false;
  const messageLabel = document.createElement("label");
  messageLabel.append(document.createTextNode(t("commitMessage")), message);

  const previousMessages = document.createElement("select");
  previousMessages.dataset.testid = "commit-previous-messages";
  previousMessages.setAttribute("aria-label", t("commitPreviousMessages"));
  previousMessages.hidden = true;

  const result = document.createElement("div");
  result.dataset.testid = "commit-result";
  result.setAttribute("role", "status");
  const submit = document.createElement("button");
  submit.type = "submit";
  submit.dataset.testid = "commit-submit";

  const identity = identityForm(host.repository, host.errorView, () => void commit());
  const warning = warningForm(() => void commit());
  view.append(title, files, amendLabel, messageLabel, previousMessages, identity.view, warning.view, result, submit);

  let ticked = new Set<string>();
  /** The files of the Active changelist the last time, so that one the user unticked is told from one that just came in. */
  let shownActivePaths = new Set<string>();
  /** The Active changelist the ticks started from. */
  let activeId: string | null = null;
  let shownChanges: ChangeSet | null = null;
  let committing = false;
  /** What gikli last put in the message: while the message is still this, the user has not written over it. Null when what is there has to stay. */
  let prefilled: string | null = "";
  /** The ticked files and comments the message was last pre-filled for, so that reading the changes again asks for nothing new. */
  let prefilledFor = "";
  /** Tells the last request for a pre-filled message from those it replaced. */
  let prefillRequests = 0;

  const updateSubmit = (): void => {
    // An amend needs no files: it can change only the message.
    submit.disabled = committing || (ticked.size === 0 && !amend.checked) || message.value.trim() === "";
    submit.textContent = t(amend.checked ? "commitSubmitAmend" : "commitSubmit");
  };
  message.addEventListener("input", () => {
    if (message.value === "") {
      prefilled = "";
    }
    updateSubmit();
  });

  previousMessages.addEventListener("change", () => {
    message.value = previousMessages.value;
    previousMessages.selectedIndex = 0;
    updateSubmit();
  });

  /** Offers the messages of the user's last commits, when there are any. */
  async function loadPreviousMessages(): Promise<void> {
    try {
      const messages = await previousCommitMessages(host.repository);
      const firstLine = (text: string): string => text.split("\n", 1)[0] ?? text;
      previousMessages.replaceChildren(new Option(t("commitPreviousMessages"), ""), ...messages.map((text) => new Option(firstLine(text), text)));
      previousMessages.hidden = messages.length === 0;
    } catch (error) {
      result.replaceChildren(host.errorView(error));
    }
  }

  amend.addEventListener("change", () => {
    updateSubmit();
    void prefillMessage();
  });

  /** Starts the message from the last commit when amending, or else from what the ticked files call for, unless the user has written one. */
  async function prefillMessage(): Promise<void> {
    if (shownChanges === null || message.value !== prefilled) {
      return;
    }
    const paths = [...ticked].sort();
    const askedFor = JSON.stringify([amend.checked, paths, shownChanges.changelists.map((list) => [list.id, list.comment])]);
    if (askedFor === prefilledFor) {
      return;
    }
    prefilledFor = askedFor;
    const request = ++prefillRequests;
    try {
      const text = await (amend.checked ? lastCommitMessage(host.repository) : prefilledCommitMessage(host.repository, paths));
      if (request === prefillRequests && message.value === prefilled) {
        message.value = text ?? "";
        // Read back, as the box gives its line ends its own way.
        prefilled = message.value;
        updateSubmit();
      }
    } catch (error) {
      if (request === prefillRequests) {
        result.replaceChildren(host.errorView(error));
      }
    }
  }

  const tickBox = (key: string, checked: boolean, toggled: (checked: boolean) => void): HTMLInputElement => {
    const box = document.createElement("input");
    box.type = "checkbox";
    box.dataset.tick = key;
    box.checked = checked;
    box.addEventListener("change", () => toggled(box.checked));
    return box;
  };

  const setTicked = (path: string, checked: boolean): void => {
    if (checked) {
      ticked.add(path);
    } else {
      ticked.delete(path);
    }
  };

  const groupView = (group: FileGroup): HTMLElement => {
    const section = document.createElement("div");
    section.dataset.testid = group.testId;
    section.dataset.name = group.name;
    const paths = group.changes.map((change) => change.path);
    const tickedPaths = paths.filter((path) => ticked.has(path));
    const box = tickBox(group.key, tickedPaths.length === paths.length, (checked) => {
      paths.forEach((path) => setTicked(path, checked));
      draw();
    });
    box.indeterminate = tickedPaths.length > 0 && tickedPaths.length < paths.length;
    const heading = document.createElement("label");
    heading.className = "changelist-form-check commit-group";
    heading.append(box, document.createTextNode(group.name));
    section.append(heading);
    for (const change of group.changes) {
      const fileBox = tickBox(`file:${change.path}`, ticked.has(change.path), (checked) => {
        setTicked(change.path, checked);
        draw();
      });
      const row = document.createElement("label");
      row.className = "changelist-form-check commit-file";
      row.dataset.testid = "commit-file";
      row.dataset.path = change.path;
      row.append(fileBox, document.createTextNode(t("commitFile", { path: change.path, description: host.describeChange(change) })));
      section.append(row);
    }
    return section;
  };

  /** Draws the files, keeping the focus on the checkbox that had it. */
  function draw(): void {
    if (shownChanges === null) {
      return;
    }
    const focused = files.contains(document.activeElement) ? (document.activeElement as HTMLElement).dataset.tick : undefined;
    const groups: FileGroup[] = shownChanges.changelists.map((list) => ({
      key: `changelist:${list.id}`,
      testId: "commit-changelist",
      name: list.name,
      changes: list.group.changes,
    }));
    groups.push({ key: "unversioned", testId: "commit-unversioned", name: t("unversionedFiles"), changes: shownChanges.unversionedFiles.changes });
    const shown = groups.filter((group) => group.changes.length > 0);
    if (shown.length === 0) {
      const none = document.createElement("p");
      none.textContent = t("commitNothing");
      files.replaceChildren(none);
    } else {
      files.replaceChildren(...shown.map(groupView));
    }
    if (focused !== undefined) {
      [...files.querySelectorAll<HTMLElement>("input")].find((box) => box.dataset.tick === focused)?.focus();
    }
    updateSubmit();
    void prefillMessage();
  }

  /** Starts a step of committing and clears what the last one showed. False while one is going on. */
  function startCommitting(): boolean {
    if (committing) {
      return false;
    }
    committing = true;
    updateSubmit();
    result.replaceChildren();
    warning.close();
    return true;
  }

  /** Commits, unless the Repository is somewhere the user should hear about first. */
  async function commitUnlessWarned(): Promise<void> {
    if (!startCommitting()) {
      return;
    }
    try {
      const found = await commitWarning(host.repository);
      if (found !== null) {
        warning.ask(found);
        return;
      }
    } catch (error) {
      result.replaceChildren(host.errorView(error, () => void commitUnlessWarned()));
      return;
    } finally {
      committing = false;
      updateSubmit();
    }
    await commit();
  }

  async function commit(): Promise<void> {
    if (!startCommitting()) {
      return;
    }
    try {
      await (amend.checked ? amendLastCommit : commitFiles)(host.repository, [...ticked], message.value);
      // The next commit is a new one unless the user says otherwise.
      amend.checked = false;
      if ((await settings()).clearCommitMessageAfterCommit) {
        message.value = "";
        prefilled = "";
        prefilledFor = "";
      } else {
        prefilled = null;
      }
      identity.close();
      void loadPreviousMessages();
      host.committed();
    } catch (error) {
      if (isFailure(error) && error.kind === "commitIdentityMissing") {
        identity.ask(error.nameMissing, error.emailMissing);
      } else {
        result.replaceChildren(host.errorView(error, () => void commit()));
      }
    } finally {
      committing = false;
      updateSubmit();
    }
  }

  view.addEventListener("submit", (event) => {
    event.preventDefault();
    void commitUnlessWarned();
  });

  updateSubmit();
  void loadPreviousMessages();
  return {
    view,
    drawn: (changeSet) => {
      const active = changeSet.changelists.find((list) => list.active);
      const activePaths = new Set((active?.group.changes ?? []).map((change) => change.path));
      const listed = new Set(
        [...changeSet.changelists.flatMap((list) => list.group.changes), ...changeSet.unversionedFiles.changes].map((change) => change.path),
      );
      if (active !== undefined && active.id !== activeId) {
        activeId = active.id;
        ticked = new Set(activePaths);
      } else {
        // What comes into the Active changelist is ticked, and what leaves it is not.
        [...activePaths].filter((path) => !shownActivePaths.has(path)).forEach((path) => setTicked(path, true));
        [...shownActivePaths].filter((path) => !activePaths.has(path)).forEach((path) => setTicked(path, false));
        ticked = new Set([...ticked].filter((path) => listed.has(path)));
      }
      shownActivePaths = activePaths;
      shownChanges = changeSet;
      draw();
    },
  };
}

/** Says where a commit would go and lets the user go on with it or leave it. */
function warningForm(proceed: () => void): { view: HTMLElement; ask: (found: CommitWarning) => void; close: () => void } {
  const view = document.createElement("div");
  view.className = "changelist-form";
  view.dataset.testid = "commit-warning";
  view.setAttribute("role", "alert");
  view.hidden = true;
  const text = document.createElement("p");
  text.dataset.testid = "commit-warning-text";

  const close = (): void => {
    view.hidden = true;
  };
  const button = (testId: string, label: string, clicked: () => void): HTMLButtonElement => {
    const element = document.createElement("button");
    element.type = "button";
    element.dataset.testid = testId;
    element.textContent = label;
    element.addEventListener("click", clicked);
    return element;
  };
  view.append(text, button("commit-warning-proceed", t("commitWarningProceed"), proceed), button("commit-warning-cancel", t("commitWarningCancel"), close));

  return {
    view,
    close,
    ask: (found) => {
      text.textContent = t(found === "rebase" ? "commitWarningRebase" : "commitWarningDetachedHead");
      view.hidden = false;
    },
  };
}

/** A question of the form that asks for the name or email git is missing. */
interface IdentityField {
  label: HTMLLabelElement;
  input: HTMLInputElement;
}

/** A form that asks for the name or email git is missing, saves them and goes on with the commit. */
function identityForm(
  repository: Repository,
  errorView: CommitPanelHost["errorView"],
  saved: () => void,
): { view: HTMLElement; ask: (nameMissing: boolean, emailMissing: boolean) => void; close: () => void } {
  const view = document.createElement("div");
  view.className = "changelist-form";
  view.dataset.testid = "commit-identity";
  view.hidden = true;
  const explanation = document.createElement("p");
  explanation.textContent = t("commitIdentityExplanation");

  const field = (testId: string, text: string): IdentityField => {
    const input = document.createElement("input");
    input.type = "text";
    input.autocomplete = "off";
    input.spellcheck = false;
    input.dataset.testid = testId;
    const label = document.createElement("label");
    label.append(document.createTextNode(text), input);
    return { label, input };
  };
  const name = field("commit-identity-name", t("commitIdentityName"));
  const email = field("commit-identity-email", t("commitIdentityEmail"));

  const everywhere = document.createElement("input");
  everywhere.type = "checkbox";
  everywhere.dataset.testid = "commit-identity-everywhere";
  const everywhereLabel = document.createElement("label");
  everywhereLabel.className = "changelist-form-check";
  everywhereLabel.append(everywhere, document.createTextNode(t("commitIdentityEverywhere")));

  const message = document.createElement("div");
  message.dataset.testid = "commit-identity-message";
  const save = document.createElement("button");
  save.type = "button";
  save.dataset.testid = "commit-identity-save";
  save.textContent = t("commitIdentitySave");
  view.append(explanation, name.label, email.label, everywhereLabel, message, save);

  const close = (): void => {
    view.hidden = true;
  };
  save.addEventListener("click", async () => {
    const answerTo = ({ label, input }: IdentityField): string | null => (label.hidden ? null : input.value);
    save.disabled = true;
    message.replaceChildren();
    try {
      await setCommitIdentity(repository, answerTo(name), answerTo(email), everywhere.checked);
      close();
      saved();
    } catch (error) {
      message.replaceChildren(errorView(error));
    } finally {
      save.disabled = false;
    }
  });

  // The fields are inside the form of the panel, where Enter would try the commit again.
  view.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      save.click();
    }
  });

  return {
    view,
    close,
    ask: (nameMissing, emailMissing) => {
      name.label.hidden = !nameMissing;
      email.label.hidden = !emailMissing;
      everywhere.checked = true;
      message.textContent = "";
      view.hidden = false;
      (nameMissing ? name.input : email.input).focus();
    },
  };
}
