// The panel next to the changes where the user commits a Changelist or selected files. It blocks nothing: the changes stay in reach while it is open.

import { commitFiles, isFailure, setCommitIdentity, type Change, type ChangeSet, type Repository } from "./core";
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

  const message = document.createElement("textarea");
  message.dataset.testid = "commit-message";
  message.rows = 4;
  message.spellcheck = false;
  const messageLabel = document.createElement("label");
  messageLabel.append(document.createTextNode(t("commitMessage")), message);

  const result = document.createElement("div");
  result.dataset.testid = "commit-result";
  result.setAttribute("role", "status");
  const submit = document.createElement("button");
  submit.type = "submit";
  submit.dataset.testid = "commit-submit";
  submit.textContent = t("commitSubmit");

  const identity = identityForm(host.repository, host.errorView, () => void commit());
  view.append(title, files, messageLabel, identity.view, result, submit);

  let ticked = new Set<string>();
  /** The files of the Active changelist the last time, so that one the user unticked is told from one that just came in. */
  let shownActivePaths = new Set<string>();
  /** The Active changelist the ticks started from. */
  let activeId: string | null = null;
  let shownChanges: ChangeSet | null = null;
  let committing = false;

  const updateSubmit = (): void => {
    submit.disabled = committing || ticked.size === 0 || message.value.trim() === "";
  };
  message.addEventListener("input", updateSubmit);

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
  }

  async function commit(): Promise<void> {
    if (committing) {
      return;
    }
    committing = true;
    updateSubmit();
    result.replaceChildren();
    try {
      await commitFiles(host.repository, [...ticked], message.value);
      message.value = "";
      identity.close();
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
    void commit();
  });

  updateSubmit();
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
