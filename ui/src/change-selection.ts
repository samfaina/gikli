// The selection of changes in the changes view, and the menu, the forms and the dragging that act on it.

import { addFiles, forceAddFiles, moveChanges, type Change, type ChangeSet, type MoveTarget, type Repository } from "./core";
import { hasShortcutKey } from "./dom";
import { createRollbackForm } from "./rollback-form";
import { t } from "./strings";

const CHANGE = '[data-testid="change"]';
const CHANGELIST = '[data-testid="changelist"]';
const UNVERSIONED_FILES = '[data-testid="unversioned-files"]';
const IGNORED_FILES = '[data-testid="ignored-files"]';

/** The groups of rows the user can select in: rows of different groups have different actions, so they are never selected together. */
type RowGroup = "changelist" | "unversioned" | "ignored";

const ROWS_OF: Record<RowGroup, string> = {
  changelist: `${CHANGELIST} ${CHANGE}`,
  unversioned: `${UNVERSIONED_FILES} ${CHANGE}`,
  ignored: `${IGNORED_FILES} ${CHANGE}`,
};

/** The type of what a drag of changes carries: a {@link DraggedChanges} as JSON. */
const DRAGGED_CHANGES = "application/x-gikli-changes";

interface DraggedChanges {
  root: string;
  paths: string[];
}

/** The value of the option of the move form that creates a Changelist. No Changelist has an empty identifier. */
const NEW_CHANGELIST = "";

/**
 * The root of the Repository of the last drag of changes started in this window. Only a drop can
 * read what a drag carries, and a Changelist has to know earlier whether it can take the changes.
 */
let draggedFrom: string | null = null;

/** What the selection needs from the view of the Repository around it. */
export interface ChangeSelectionHost {
  repository: Repository;
  /** Where the rows of the changes are drawn, again on every read. */
  changes: HTMLElement;
  /** Runs what acts on changes, then reads them again, and shows why when it fails. */
  run(action: () => Promise<unknown>): void;
  /** A form acted on changes, so they need reading again. */
  changed(): void;
  /** What to say about an error from the core. */
  describeError(error: unknown): string;
  /** What to say about a change besides its path. */
  describeChange(change: Change): string;
}

export interface ChangeSelection {
  /** The menu of the selected changes, to place outside the rows. */
  menu: HTMLElement;
  /** The form that asks where the selected changes go, to place outside the rows. */
  form: HTMLFormElement;
  /** The form that confirms a rollback of the selected changes, to place outside the rows. */
  rollbackForm: HTMLElement;
  /** The rows were drawn again for `changeSet`. */
  drawn(changeSet: ChangeSet): void;
}

/** Lets the user select the changes drawn in `host.changes` and act on them. */
export function createChangeSelection(host: ChangeSelectionHost): ChangeSelection {
  const { repository, changes } = host;
  let changeSet: ChangeSet | null = null;
  let selected = new Set<string>();
  /** The group the selected rows are in. */
  let selectedGroup: RowGroup = "changelist";
  /** The row a Shift+click extends the selection from. */
  let anchor: string | null = null;

  const rowAt = (target: EventTarget | null): HTMLElement | null => (target instanceof Element ? target.closest<HTMLElement>(CHANGE) : null);
  const groupOf = (row: HTMLElement): RowGroup => (row.closest(CHANGELIST) ? "changelist" : row.closest(UNVERSIONED_FILES) ? "unversioned" : "ignored");
  const rowsOf = (group: RowGroup): HTMLElement[] => [...changes.querySelectorAll<HTMLElement>(ROWS_OF[group])];
  const pathOf = (row: HTMLElement): string => row.dataset.path as string;

  const showSelection = (): void => {
    for (const row of changes.querySelectorAll<HTMLElement>(CHANGE)) {
      const group = groupOf(row);
      row.draggable = group === "changelist";
      row.classList.toggle("selected", group === selectedGroup && selected.has(pathOf(row)));
    }
  };
  const selectOnly = (row: HTMLElement): void => {
    selected = new Set([pathOf(row)]);
    selectedGroup = groupOf(row);
    anchor = pathOf(row);
    showSelection();
  };
  /** A menu or a drag acts on the selection, or on the row alone when it is outside the selection. */
  const includeInSelection = (row: HTMLElement): void => {
    if (groupOf(row) !== selectedGroup || !selected.has(pathOf(row))) {
      selectOnly(row);
    }
  };

  changes.addEventListener("click", (event) => {
    const row = rowAt(event.target);
    if (row === null) {
      return;
    }
    if (groupOf(row) !== selectedGroup) {
      selectOnly(row);
      return;
    }
    const path = pathOf(row);
    if (event.shiftKey && anchor !== null) {
      const paths = rowsOf(selectedGroup).map(pathOf);
      const from = paths.indexOf(anchor);
      const to = paths.indexOf(path);
      selected = new Set(from === -1 ? [path] : paths.slice(Math.min(from, to), Math.max(from, to) + 1));
      anchor = from === -1 ? path : anchor;
      showSelection();
    } else if (hasShortcutKey(event)) {
      if (!selected.delete(path)) {
        selected.add(path);
      }
      anchor = path;
      showSelection();
    } else {
      selectOnly(row);
    }
  });

  const form = moveForm(host);
  const rollback = createRollbackForm({
    repository,
    describeChange: host.describeChange,
    rolledBack: host.changed,
    describeError: host.describeError,
  });

  const menu = document.createElement("div");
  menu.className = "context-menu";
  menu.dataset.testid = "change-menu";
  menu.setAttribute("role", "menu");
  menu.hidden = true;
  /** The row the menu was opened on, which gets the focus back. */
  let menuRow: HTMLElement | null = null;
  const closeMenu = (): void => {
    menu.hidden = true;
  };
  // So that clicking the item does not take the focus from it, which would close the menu first.
  menu.addEventListener("mousedown", (event) => event.preventDefault());
  menu.addEventListener("focusout", (event) => {
    if (!(event.relatedTarget instanceof Node && menu.contains(event.relatedTarget))) {
      closeMenu();
    }
  });
  menu.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      closeMenu();
      menuRow?.focus();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const items = [...menu.children] as HTMLElement[];
      const from = items.indexOf(document.activeElement as HTMLElement);
      items[(from + (event.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
    }
  });
  /** An item of the menu, which runs `act` on the selected paths. */
  const menuItem = (testId: string, text: string, act: (paths: string[]) => void): HTMLButtonElement => {
    const item = document.createElement("button");
    item.type = "button";
    item.dataset.testid = testId;
    item.setAttribute("role", "menuitem");
    item.textContent = text;
    item.addEventListener("click", () => {
      closeMenu();
      // A read since the menu opened can have found that nothing selected is a change any more.
      if (selected.size > 0) {
        act([...selected]);
      }
    });
    return item;
  };
  const itemsOf: Record<RowGroup, HTMLButtonElement[]> = {
    changelist: [
      menuItem("move-to-another-changelist", t("moveToAnotherChangelist"), (paths) => {
        if (changeSet !== null) {
          form.open(changeSet, paths);
        }
      }),
      menuItem("rollback-changes", t("rollbackMenu"), (paths) => rollback.open(paths)),
    ],
    unversioned: [menuItem("add-to-git", t("addToGit"), (paths) => host.run(() => addFiles(repository, paths)))],
    ignored: [menuItem("force-add-to-git", t("forceAddToGit"), (paths) => host.run(() => forceAddFiles(repository, paths)))],
  };
  changes.addEventListener("contextmenu", (event) => {
    const row = rowAt(event.target);
    if (row === null) {
      return;
    }
    event.preventDefault();
    includeInSelection(row);
    menuRow = row;
    const items = itemsOf[selectedGroup];
    menu.replaceChildren(...items);
    menu.hidden = false;
    menu.style.left = `${Math.min(event.clientX, window.innerWidth - menu.offsetWidth)}px`;
    menu.style.top = `${Math.min(event.clientY, window.innerHeight - menu.offsetHeight)}px`;
    items[0]?.focus();
  });

  changes.addEventListener("dragstart", (event) => {
    const row = rowAt(event.target);
    // Only a change in a Changelist has somewhere to move from.
    if (row === null || groupOf(row) !== "changelist" || event.dataTransfer === null) {
      return;
    }
    includeInSelection(row);
    draggedFrom = repository.root;
    const dragged: DraggedChanges = { root: repository.root, paths: [...selected] };
    event.dataTransfer.setData(DRAGGED_CHANGES, JSON.stringify(dragged));
    event.dataTransfer.effectAllowed = "move";
  });

  let dropTarget: HTMLElement | null = null;
  const markDropTarget = (list: HTMLElement | null): void => {
    dropTarget?.classList.remove("drop-target");
    list?.classList.add("drop-target");
    dropTarget = list;
  };
  /** The Changelist under a drag of changes, if that is what `event` is. */
  const changelistUnderDrag = (event: DragEvent): HTMLElement | null =>
    event.dataTransfer?.types.includes(DRAGGED_CHANGES) && draggedFrom === repository.root && event.target instanceof Element
      ? event.target.closest<HTMLElement>(CHANGELIST)
      : null;

  changes.addEventListener("dragover", (event) => {
    const list = changelistUnderDrag(event);
    markDropTarget(list);
    if (list !== null && event.dataTransfer !== null) {
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
    }
  });
  changes.addEventListener("dragleave", (event) => {
    if (!(event.relatedTarget instanceof Node && changes.contains(event.relatedTarget))) {
      markDropTarget(null);
    }
  });
  changes.addEventListener("dragend", () => markDropTarget(null));
  changes.addEventListener("drop", (event) => {
    markDropTarget(null);
    const list = changelistUnderDrag(event);
    if (list === null || event.dataTransfer === null) {
      return;
    }
    event.preventDefault();
    const dragged = JSON.parse(event.dataTransfer.getData(DRAGGED_CHANGES)) as DraggedChanges;
    // From a window of another Project, as draggedFrom only knows about this window.
    if (dragged.root === repository.root) {
      const target: MoveTarget = { kind: "changelist", id: list.dataset.id as string };
      host.run(() => moveChanges(repository, dragged.paths, target, false));
    }
  });

  return {
    menu,
    form: form.view,
    rollbackForm: rollback.view,
    drawn: (drawnSet) => {
      changeSet = drawnSet;
      const paths = new Set(rowsOf(selectedGroup).map(pathOf));
      selected = new Set([...selected].filter((path) => paths.has(path)));
      showSelection();
    },
  };
}

/** A form that asks where changes go, hidden until something opens it. When it cannot move them, it shows why. */
function moveForm(host: ChangeSelectionHost): { view: HTMLFormElement; open: (changeSet: ChangeSet, paths: string[]) => void } {
  const view = document.createElement("form");
  view.className = "changelist-form";
  view.dataset.testid = "move-form";
  view.hidden = true;
  const title = document.createElement("h3");
  title.textContent = t("moveToAnotherChangelist");

  const targetSelect = document.createElement("select");
  targetSelect.dataset.testid = "move-form-target";
  const targetLabel = document.createElement("label");
  targetLabel.append(document.createTextNode(t("moveFormTarget")), targetSelect);

  const name = document.createElement("input");
  name.type = "text";
  name.dataset.testid = "move-form-name";
  name.autocomplete = "off";
  const nameLabel = document.createElement("label");
  nameLabel.append(document.createTextNode(t("moveFormName")), name);

  const makeActive = document.createElement("input");
  makeActive.type = "checkbox";
  makeActive.dataset.testid = "move-form-make-active";
  const makeActiveLabel = document.createElement("label");
  makeActiveLabel.className = "changelist-form-check";
  makeActiveLabel.append(makeActive, document.createTextNode(t("moveFormMakeActive")));

  const message = document.createElement("p");
  message.dataset.testid = "move-form-message";
  const submit = document.createElement("button");
  submit.type = "submit";
  submit.dataset.testid = "move-form-submit";
  submit.textContent = t("moveFormMove");
  const cancel = document.createElement("button");
  cancel.type = "button";
  cancel.dataset.testid = "move-form-cancel";
  cancel.textContent = t("changelistFormCancel");
  const buttons = document.createElement("div");
  buttons.className = "notification-actions";
  buttons.append(submit, cancel);
  view.append(title, targetLabel, nameLabel, makeActiveLabel, message, buttons);

  const showName = (): void => {
    nameLabel.hidden = targetSelect.value !== NEW_CHANGELIST;
  };
  targetSelect.addEventListener("change", showName);

  let moving: string[] | null = null;
  const close = (): void => {
    moving = null;
    view.hidden = true;
  };
  cancel.addEventListener("click", close);
  view.addEventListener("submit", async (event) => {
    event.preventDefault();
    if (moving === null) {
      return;
    }
    const target: MoveTarget =
      targetSelect.value === NEW_CHANGELIST ? { kind: "newChangelist", name: name.value } : { kind: "changelist", id: targetSelect.value };
    submit.disabled = true;
    message.textContent = "";
    try {
      await moveChanges(host.repository, moving, target, makeActive.checked);
      close();
      host.changed();
    } catch (error) {
      message.textContent = host.describeError(error);
    } finally {
      submit.disabled = false;
    }
  });
  return {
    view,
    open: (changeSet, paths) => {
      moving = paths;
      const options = changeSet.changelists.map((list) => new Option(list.name, list.id));
      targetSelect.replaceChildren(...options, new Option(t("moveFormNewChangelist"), NEW_CHANGELIST));
      // A move is away from the Changelists the changes are in.
      const elsewhere = changeSet.changelists.find((list) => !list.group.changes.some((change) => paths.includes(change.path)));
      targetSelect.value = elsewhere?.id ?? NEW_CHANGELIST;
      name.value = "";
      makeActive.checked = false;
      message.textContent = "";
      showName();
      view.hidden = false;
      targetSelect.focus();
    },
  };
}
