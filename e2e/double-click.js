// The smoke test's double-click on a change row, which has to survive a rebuild of the changes view
// (issue #81). WebdriverIO's own retry of a stale element sends the old element id again, so only
// a row found afresh helps.

/** Whether `element` is displayed before `deadline` (a time in ms), polling until it is. */
async function displayedBefore(element, deadline) {
  while (!(await element.isDisplayed())) {
    if (Date.now() >= deadline) {
      return false;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  return true;
}

/**
 * Double-clicks what `rowSelector` finds until `targetSelector` is displayed, finding the row
 * afresh on each of up to `attempts` attempts, and waiting up to `timeout` ms after each
 * double-click. Resolves with the displayed element.
 *
 * A stale element means the view was rebuilt before the double-click, and a double-click with no
 * effect that it was rebuilt between the two clicks, so either leads to another attempt. Any other
 * error is thrown at once, and so is the stale element of the last attempt.
 *
 * Another double-click cancels the read of the diff the last one started, so `timeout` has to be
 * longer than that read takes. The defaults keep every attempt within the 60 s a smoke case may take.
 */
export async function doubleClickUntilDisplayed(browser, rowSelector, targetSelector, { attempts = 4, timeout = 12000 } = {}) {
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
    if (await displayedBefore(target, Date.now() + timeout)) {
      return target;
    }
    console.warn(`Double-click ${attempt} of ${attempts} on ${rowSelector} did not display ${targetSelector}.`);
  }
  throw new Error(`${targetSelector} was not displayed after ${attempts} double-clicks on ${rowSelector}.`);
}
