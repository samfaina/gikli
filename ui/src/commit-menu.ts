// The menu of a commit in the log. Each action is an entry of COMMIT_ACTIONS, or, for one that acts
// on the Repository the log shows, one the log gives the menu, so later actions on commits add theirs
// there.

import { t } from "./strings";

/** An action of the commit menu: on the one commit selected, and disabled while several are, unless it acts on several. */
export type CommitAction = {
  /** The `data-testid` of its item. */
  testId: string;
  /** The text of its item. */
  label(): string;
  /** Whether it can act on the selected commits `hashes`. Its item is disabled when it cannot. */
  enabled?(hashes: string[]): boolean;
} & (
  | {
      several?: false;
      /** Acts on the commit `hash`. A rejection shows as an error in the log. */
      run(hash: string): Promise<void>;
    }
  | {
      several: true;
      /** Acts on every selected commit, `hashes`, at once. A rejection shows as an error in the log. */
      run(hashes: string[]): Promise<void>;
    }
);

/** The actions of the commit menu that need nothing but the commit, in the order they show. */
export const COMMIT_ACTIONS: CommitAction[] = [
  {
    testId: "commit-copy-hash",
    label: () => t("logCommitMenuCopyHash"),
    run: (hash) => navigator.clipboard.writeText(hash),
  },
];

/** The menu of the selected commits, which one log has at a time. */
export interface CommitMenu {
  /** Opens the menu of the commits `hashes` at the point `x`, `y`; the focus goes back to `from` on Escape. */
  open(hashes: string[], x: number, y: number, from: HTMLElement): void;
}

/** The commit menu, added to `container`, with `actions` after {@link COMMIT_ACTIONS}. An action that fails tells `failed`. */
export function createCommitMenu(container: HTMLElement, failed: (error: unknown) => void, actions: CommitAction[] = []): CommitMenu {
  const menu = document.createElement("div");
  menu.className = "context-menu";
  menu.dataset.testid = "commit-menu";
  menu.setAttribute("role", "menu");
  // So that the menu takes the focus when every item is disabled, and still closes as it goes.
  menu.tabIndex = -1;
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
      const items = [...menu.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
      const from = items.indexOf(document.activeElement as HTMLButtonElement);
      items[(from + (event.key === "ArrowDown" ? 1 : items.length - 1)) % items.length]?.focus();
    }
  });

  function open(hashes: string[], x: number, y: number, from: HTMLElement): void {
    returnFocus = from;
    const items = [...COMMIT_ACTIONS, ...actions].map((action) => {
      const item = document.createElement("button");
      item.type = "button";
      item.dataset.testid = action.testId;
      item.setAttribute("role", "menuitem");
      item.textContent = action.label();
      item.disabled = (hashes.length > 1 && action.several !== true) || action.enabled?.(hashes) === false;
      item.addEventListener("click", () => {
        close();
        returnFocus?.focus();
        const [first] = hashes;
        const run = action.several === true ? action.run(hashes) : first === undefined ? null : action.run(first);
        run?.catch(failed);
      });
      return item;
    });
    menu.replaceChildren(...items);
    menu.hidden = false;
    menu.style.left = `${Math.min(x, window.innerWidth - menu.offsetWidth)}px`;
    menu.style.top = `${Math.min(y, window.innerHeight - menu.offsetHeight)}px`;
    (items.find((item) => !item.disabled) ?? menu).focus();
  }

  return { open };
}
