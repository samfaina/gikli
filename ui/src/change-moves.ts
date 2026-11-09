// Moving changes between Changelists: the selection of changes in the changes view, and the menu,
// the form and the dragging that move them.

import { moveChanges, type ChangeSet, type MoveTarget, type Repository } from "./core";
import { hasShortcutKey } from "./dom";
import { t } from "./strings";

const CHANGE = '[data-testid="change"]';
const CHANGELIST = '[data-testid="changelist"]';

/** The type of what a drag of changes carries: a {@link DraggedChanges} as JSON. */
const DRAGGED_CHANGES = "application/x-gikli-changes";

interface DraggedChanges {
  root: string;
  paths: string[];
}

/** What the moves need from the view of the Repository around them. */
export interface ChangeMovesHost {
  repository: Repository;
  /** Where the rows of the changes are drawn, again on every read. */
  changes: HTMLElement;
  /** Runs what moves changes, then reads them again, and shows why when it fails. */
  run(action: () => Promise<unknown>): void;
  /** The form moved changes, so they need reading again. */
  moved(): void;
  /** What to say about an error from the core. */
  describeError(error: unknown): string;
}

export interface ChangeMoves {
  /** The menu of the selected changes, to place outside the rows. */
  menu: HTMLElement;
  /** The form that asks where the selected changes go, to place outside the rows. */
  form: HTMLFormElement;
  /** The rows were drawn again for `changeSet`. */
  drawn(changeSet: ChangeSet): void;
}

/** Lets the user select the changes drawn in `host.changes` and move them to another Changelist. */
export function createChangeMoves(host: ChangeMovesHost): ChangeMoves {
  const { repository, changes } = host;
  let changeSet: ChangeSet | null = null;
  let selected = new Set<string>();
  /** The row a Shift+click extends the selection from. */
  let anchor: string | null = null;

  /** The row of a change in a Changelist at `target`. Other rows have nowhere to move from. */
  const movableRow = (target: EventTarget | null): HTMLElement | null => {
    const row = target instanceof Element ? target.closest<HTMLElement>(CHANGE) : null;
    return row?.closest(CHANGELIST) ? row : null;
  };
  const movableRows = (): HTMLElement[] => [...changes.querySelectorAll<HTMLElement>(`${CHANGELIST} ${CHANGE}`)];
  const pathOf = (row: HTMLElement): string => row.dataset.path as string;

  const showSelection = (): void => {
    for (const row of movableRows()) {
      row.draggable = true;
      row.classList.toggle("selected", selected.has(pathOf(row)));
    }
  };
  const selectOnly = (path: string): void => {
    selected = new Set([path]);
    anchor = path;
    showSelection();
  };
  /** A menu or a drag acts on the selection, or on the row alone when it is outside the selection. */
  const includeInSelection = (path: string): void => {
    if (!selected.has(path)) {
      selectOnly(path);
    }
  };

  changes.addEventListener("click", (event) => {
    const row = movableRow(event.target);
    if (row === null) {
      return;
    }
    const path = pathOf(row);
    if (event.shiftKey && anchor !== null) {
      const paths = movableRows().map(pathOf);
      const from = paths.indexOf(anchor);
      const to = paths.indexOf(path);
      selected = new Set(from === -1 ? [path] : paths.slice(Math.min(from, to), Math.max(from, to) + 1));
      showSelection();
    } else if (hasShortcutKey(event)) {
      if (!selected.delete(path)) {
        selected.add(path);
      }
      anchor = path;
      showSelection();
    } else {
      selectOnly(path);
    }
  });

  const form = moveForm(host);

  const menu = document.createElement("div");
  menu.className = "context-menu";
  menu.dataset.testid = "change-menu";
  menu.setAttribute("role", "menu");
  menu.hidden = true;
  const moveItem = document.createElement("button");
  moveItem.type = "button";
  moveItem.dataset.testid = "move-to-another-changelist";
  moveItem.setAttribute("role", "menuitem");
  moveItem.textContent = t("moveToAnotherChangelist");
  menu.append(moveItem);
  const closeMenu = (): void => {
    menu.hidden = true;
  };
  // So that clicking the item does not take the focus from it, which would close the menu first.
  menu.addEventListener("mousedown", (event) => event.preventDefault());
  menu.addEventListener("focusout", closeMenu);
  menu.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      closeMenu();
    }
  });
  moveItem.addEventListener("click", () => {
    closeMenu();
    if (changeSet !== null) {
      form.open(changeSet, [...selected]);
    }
  });
  changes.addEventListener("contextmenu", (event) => {
    const row = movableRow(event.target);
    if (row === null) {
      return;
    }
    event.preventDefault();
    includeInSelection(pathOf(row));
    menu.hidden = false;
    menu.style.left = `${Math.min(event.clientX, window.innerWidth - menu.offsetWidth)}px`;
    menu.style.top = `${Math.min(event.clientY, window.innerHeight - menu.offsetHeight)}px`;
    moveItem.focus();
  });

  changes.addEventListener("dragstart", (event) => {
    const row = movableRow(event.target);
    if (row === null || event.dataTransfer === null) {
      return;
    }
    includeInSelection(pathOf(row));
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
    event.dataTransfer?.types.includes(DRAGGED_CHANGES) && event.target instanceof Element
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
    // Changelists belong to one Repository.
    if (dragged.root === repository.root) {
      const target: MoveTarget = { kind: "changelist", id: list.dataset.id as string };
      host.run(() => moveChanges(repository, dragged.paths, target, false));
    }
  });

  return {
    menu,
    form: form.view,
    drawn: (drawnSet) => {
      changeSet = drawnSet;
      const paths = new Set(drawnSet.changelists.flatMap((list) => list.group.changes.map((change) => change.path)));
      selected = new Set([...selected].filter((path) => paths.has(path)));
      showSelection();
    },
  };
}

/** A form that asks where changes go, hidden until something opens it. When it cannot move them, it shows why. */
function moveForm(host: ChangeMovesHost): { view: HTMLFormElement; open: (changeSet: ChangeSet, paths: string[]) => void } {
  const view = document.createElement("form");
  view.className = "changelist-form";
  view.dataset.testid = "move-form";
  view.hidden = true;
  const title = document.createElement("h3");
  title.textContent = t("moveToAnotherChangelist");

  const target = document.createElement("select");
  target.dataset.testid = "move-form-target";
  const targetLabel = document.createElement("label");
  targetLabel.append(document.createTextNode(t("moveFormTarget")), target);

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

  /** The value of the option that creates a Changelist. No Changelist has an empty identifier. */
  const NEW_CHANGELIST = "";
  const showName = (): void => {
    nameLabel.hidden = target.value !== NEW_CHANGELIST;
  };
  target.addEventListener("change", showName);

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
    const destination: MoveTarget =
      target.value === NEW_CHANGELIST ? { kind: "newChangelist", name: name.value } : { kind: "changelist", id: target.value };
    submit.disabled = true;
    message.textContent = "";
    try {
      await moveChanges(host.repository, moving, destination, makeActive.checked);
      close();
      host.moved();
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
      target.replaceChildren(...options, new Option(t("moveFormNewChangelist"), NEW_CHANGELIST));
      // Most moves leave the Changelist the changes are in.
      const source = changeSet.changelists.find((list) => list.group.changes.some((change) => change.path === paths[0]));
      target.value = changeSet.changelists.find((list) => list !== source)?.id ?? NEW_CHANGELIST;
      name.value = "";
      makeActive.checked = false;
      message.textContent = "";
      showName();
      view.hidden = false;
      target.focus();
    },
  };
}
