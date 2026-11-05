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
});
