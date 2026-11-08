// The smoke test's double-click on a change row, which has to survive a rebuild of the change list
// (issue #81). WebdriverIO's own retry of a stale element sends the old element id again, so only
// a row found afresh helps.

/** Whether `element` is displayed before `deadline` (a time in ms), polling until it is. */
async function displayedBefore(element, deadline) {
  while (Date.now() < deadline) {
    if (await element.isDisplayed()) {
      return true;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return false;
}

/**
 * Double-clicks what `rowSelector` finds until `targetSelector` is displayed, finding the row
 * afresh on each of up to `attempts` attempts, and waiting up to `within` ms after each double-click.
 *
 * A stale element means the list was rebuilt before the double-click, and a double-click with no
 * effect that it was rebuilt between the two clicks, so either leads to another attempt. Any other
 * error is thrown at once, and so is the stale element of the last attempt. The defaults keep every
 * attempt within the 60 s a smoke case may take.
 */
export async function doubleClickUntilDisplayed(browser, rowSelector, targetSelector, { attempts = 5, within = 8000 } = {}) {
  const target = await browser.$(targetSelector);
  for (let attempt = 1; attempt <= attempts; attempt++) {
    try {
      const row = await browser.$(rowSelector);
      await row.doubleClick();
    } catch (error) {
      if (error.name !== "stale element reference" || attempt === attempts) {
        throw error;
      }
      console.warn(`Double-click ${attempt} of ${attempts} on ${rowSelector} met a stale element; finding the row again.`);
      continue;
    }
    if (await displayedBefore(target, Date.now() + within)) {
      return;
    }
    console.warn(`Double-click ${attempt} of ${attempts} on ${rowSelector} did not display ${targetSelector}.`);
  }
  throw new Error(`${targetSelector} was not displayed after ${attempts} double-clicks on ${rowSelector}.`);
}
