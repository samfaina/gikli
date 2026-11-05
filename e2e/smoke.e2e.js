// Checks the wiring between the UI and the core, not behavior: the core's tests cover that.

import assert from "node:assert/strict";

describe("gikli", () => {
  it("opens the startup Project and shows its Repository root and the git version", async () => {
    const repository = process.env.GIKLI_E2E_REPOSITORY;

    const root = await $('[data-testid="repository-root"]');
    await root.waitForDisplayed({ timeout: 30000 });

    assert.ok((await root.getText()).includes(repository), `expected ${repository} on screen`);
    assert.match(await $('[data-testid="git-status"]').getText(), /^git \d+\.\d+\.\d+$/);
  });

  it("shows the Repository's changes and reads them again on refresh", async () => {
    const untracked = process.env.GIKLI_E2E_UNTRACKED_FILE;
    const change = await $('[data-testid="change-path"]');
    await change.waitForDisplayed({ timeout: 30000 });
    assert.equal(await change.getText(), untracked);

    await $('[data-testid="refresh-changes"]').click();

    await browser.waitUntil(async () => await $('[data-testid="change-path"]').isDisplayed(), { timeout: 30000 });
    assert.equal(await $('[data-testid="change-path"]').getText(), untracked);
  });
});
