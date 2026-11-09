const onMac = navigator.platform.startsWith("Mac");

/** Whether this is a Mac, where shortcuts take Cmd instead of Ctrl. */
export function isMac(): boolean {
  return onMac;
}

/** Whether `event` has the key shortcuts take on this platform: Cmd on a Mac, and Ctrl elsewhere. */
export function hasShortcutKey(event: KeyboardEvent | MouseEvent): boolean {
  return onMac ? event.metaKey : event.ctrlKey;
}

/** The element of index.html with this id. */
export function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (found === null) {
    throw new Error(`missing #${id} in index.html`);
  }
  return found as T;
}
