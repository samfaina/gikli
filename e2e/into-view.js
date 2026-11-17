// The smoke test's pointer actions on an element below the window's visible area (issue #137). On
// Linux, WebKitGTK's WebDriver does not scroll an element into view before a pointer action, and
// fails it with "move target out of bounds"; the drivers on Windows and macOS do scroll.

/**
 * The element commands that move the pointer to an element, each with the elements it moves to:
 * the one it runs on, and for a drag the one it drops on, unless that is a point. A drag's target
 * may come as the query that finds it, which dragAndDrop awaits too.
 *
 * A drag needs both in view at once, which scrolling each into view in turn gives only when the
 * window can show both.
 */
const POINTER_COMMANDS = {
  click: async (element) => [element],
  doubleClick: async (element) => [element],
  dragAndDrop: async (element, [target]) => {
    const dropTarget = await target;
    return typeof dropTarget?.elementId === "string" ? [element, dropTarget] : [element];
  },
};

/** Scrolls each scrolling container holding `element` just enough to show it. */
async function bringIntoView(browser, element) {
  await browser.execute((target) => target.scrollIntoView({ block: "nearest", inline: "nearest" }), element);
}

/**
 * Makes every pointer action on an element of `browser` bring the elements it moves to into view
 * first, in the order it moves to them.
 */
export function bringIntoViewBeforePointerActions(browser) {
  for (const [command, targets] of Object.entries(POINTER_COMMANDS)) {
    browser.overwriteCommand(
      command,
      async function (original, ...args) {
        for (const element of await targets(this, args)) {
          await bringIntoView(browser, element);
        }
        return original(...args);
      },
      true,
    );
  }
}
