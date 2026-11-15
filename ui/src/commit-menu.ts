// The menu of a commit in the log. Each action is an entry of COMMIT_ACTIONS, so later actions on
// commits add theirs there.

import { t } from "./strings";

/** An action of the commit menu. */
export interface CommitAction {
  testId: string;
  label(): string;
  /** Acts on the commit `hash`. A rejection shows as an error in the log. */
  run(hash: string): Promise<void>;
}

/** The actions of the commit menu, in the order they show. */
export const COMMIT_ACTIONS: CommitAction[] = [
  {
    testId: "commit-copy-hash",
    label: () => t("logCommitMenuCopyHash"),
    run: (hash) => navigator.clipboard.writeText(hash),
  },
];

export interface CommitMenu {
  /** Opens the menu of the commit `hash` at the point `x`, `y`; the focus goes back to `from` on Escape. */
  open(hash: string, x: number, y: number, from: HTMLElement): void;
}

/** The commit menu, added to `container`. An action that fails tells `failed`. */
export function createCommitMenu(container: HTMLElement, failed: (error: unknown) => void): CommitMenu {
  const menu = document.createElement("div");
  menu.className = "context-menu";
  menu.dataset.testid = "commit-menu";
  menu.setAttribute("role", "menu");
  menu.hidden = true;
  container.append(menu);
  let returnFocus: HTMLElement | null = null;

  const close = (): void => {
    menu.hidden = true;
  };
  // So that clicking an item does not take the focus from it, which would close the menu first.
  menu.addEventListener("mousedown", (event) => event.preventDefault());
  menu.addEventListener("focusout", (event) => {
    if (!(event.relatedTarget instanceof Node && menu.contains(event.relatedTarget))) {
      close();
    }
  });
  menu.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      close();
      returnFocus?.focus();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const items = [...menu.children] as HTMLElement[];
      const from = items.indexOf(document.activeElement as HTMLElement);
      items[(from + (event.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
    }
  });

  function open(hash: string, x: number, y: number, from: HTMLElement): void {
    returnFocus = from;
    const items = COMMIT_ACTIONS.map((action) => {
      const item = document.createElement("button");
      item.type = "button";
      item.dataset.testid = action.testId;
      item.setAttribute("role", "menuitem");
      item.textContent = action.label();
      item.addEventListener("click", () => {
        close();
        returnFocus?.focus();
        action.run(hash).catch(failed);
      });
      return item;
    });
    menu.replaceChildren(...items);
    menu.hidden = false;
    menu.style.left = `${Math.min(x, window.innerWidth - menu.offsetWidth)}px`;
    menu.style.top = `${Math.min(y, window.innerHeight - menu.offsetHeight)}px`;
    items[0]?.focus();
  }

  return { open };
}
