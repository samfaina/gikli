// Checks the wiring between the UI and the core, not behavior: the core's tests cover that.

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { doubleClickUntilDisplayed } from "./double-click.js";

/**
 * The Refresh button, once no read of the changes is under way. It is disabled during one, as when
 * the watcher saw a file change, and WebDriver drops a click on a disabled button silently.
 */
async function idleRefresh() {
  const refresh = await $('[data-testid="refresh-changes"]');
  await refresh.waitForEnabled({ timeout: 30000 });
  return refresh;
}

/** Waits for the current window to show a Repository, and checks that its root is `folder`. */
async function expectRepositoryRoot(folder) {
  const root = await $('[data-testid="repository-root"]');
  await root.waitForDisplayed({ timeout: 30000 });
  assert.ok((await root.getText()).includes(folder), `expected ${folder} on screen`);
}

/** Fills the open clone form to clone the smoke test's remote into `target`, and starts the clone. */
async function startClone(target) {
  await $('[data-testid="clone-url"]').setValue(pathToFileURL(process.env.GIKLI_E2E_REMOTE).href);
  await $('[data-testid="clone-folder"]').setValue(target);
  await $('[data-testid="start-clone"]').click();
}

/** Runs `check` in the window that opened since `windowsBefore`, then returns to the current window for the cases after it. */
async function inNewWindow(windowsBefore, check) {
  const current = await browser.getWindowHandle();
  const opened = await browser.waitUntil(
    async () => (await browser.getWindowHandles()).find((handle) => !windowsBefore.includes(handle)),
    { timeout: 30000, timeoutMsg: "no new window opened" },
  );
  try {
    await browser.switchToWindow(opened);
    await check();
  } finally {
    await browser.switchToWindow(current);
  }
}

describe("gikli", () => {
  it("opens the startup Project and shows its Repository root and the git version", async () => {
    await expectRepositoryRoot(process.env.GIKLI_E2E_REPOSITORY);
    assert.match(await $('[data-testid="git-status"]').getText(), /^git \d+\.\d+\.\d+$/);
  });

  it("shows the Repository's changes and reads them again on refresh", async () => {
    const untracked = process.env.GIKLI_E2E_UNTRACKED_FILE;
    const change = await $('[data-testid="change-path"]');
    await change.waitForDisplayed({ timeout: 30000 });
    assert.equal(await change.getText(), untracked);

    const added = "after refresh.txt";
    fs.writeFileSync(path.join(process.env.GIKLI_E2E_REPOSITORY, added), "later\n");
    await $('[data-testid="refresh-changes"]').click();

    const paths = async () => Promise.all(await $$('[data-testid="change-path"]').map((item) => item.getText()));
    await browser.waitUntil(async () => (await paths()).length === 2, { timeout: 30000 });
    assert.deepEqual(await paths(), [added, untracked]);
  });

  it("shows the active Changelist named Changes, and new files in Unversioned Files", async () => {
    const changelist = await $('[data-testid="changelist"]');
    await changelist.waitForDisplayed({ timeout: 30000 });
    assert.equal(await changelist.getAttribute("data-active"), "true");
    assert.equal(await $('[data-testid="changelist-name"]').getText(), "Changes");
    await $('[data-testid="unversioned-files"] [data-testid="change-path"]').waitForDisplayed({ timeout: 30000 });
  });

  it("opens the diff of a changed file on a double-click, and saves the local side", async () => {
    const untracked = process.env.GIKLI_E2E_UNTRACKED_FILE;
    const diff = await doubleClickUntilDisplayed(browser, `[data-testid="change"][data-path="${untracked}"]`, '[data-testid="diff"]');

    assert.equal(await $('[data-testid="diff-title"]').getText(), untracked);
    const local = await $(".cm-merge-b .cm-content");
    await local.waitForDisplayed({ timeout: 30000 });
    assert.match(await local.getText(), /^smoke/);

    await local.click();
    await browser.keys(["x"]);
    await $('[data-testid="diff-save"]').click();
    const file = path.join(process.env.GIKLI_E2E_REPOSITORY, untracked);
    await browser.waitUntil(() => fs.readFileSync(file, "utf8") !== "smoke\n", { timeout: 30000 });

    await $('[data-testid="diff-close"]').click();
    await diff.waitForDisplayed({ timeout: 30000, reverse: true });
    await $('[data-testid="changes"]').waitForDisplayed({ timeout: 30000 });
  });

  it("brings a prompt to the user and gives git the answer", async () => {
    const sandbox = process.env.GIKLI_E2E_SANDBOX;
    // A read already under way would take the marker, and its prompt would keep Refresh disabled.
    const refresh = await idleRefresh();
    fs.writeFileSync(path.join(sandbox, "ask-for-prompt"), "");
    await refresh.click();

    const prompt = await $('[data-testid="prompt"]');
    await prompt.waitForDisplayed({ timeout: 30000 });
    assert.equal(await prompt.getAttribute("data-kind"), "username");
    assert.equal((await $('[data-testid="prompt-text"]').getText()).trim(), "Username for smoke:");

    await $('[data-testid="prompt-answer"]').setValue("ada");
    await $('[data-testid="prompt-submit"]').click();

    await prompt.waitForDisplayed({ timeout: 30000, reverse: true });
    const answer = path.join(sandbox, "prompt-answer");
    await browser.waitUntil(() => fs.existsSync(answer) && fs.readFileSync(answer, "utf8") !== "", { timeout: 30000 });
    assert.equal(fs.readFileSync(answer, "utf8").trim(), "ada");
  });

  it("has the credential helper setting on by default", async () => {
    await $('[data-testid="toggle-settings"]').click();
    const setting = await $('[data-testid="use-credential-helper"]');
    await setting.waitForDisplayed({ timeout: 30000 });
    assert.equal(await setting.isSelected(), true);
  });

  it("has the setting to clone submodules on by default", async () => {
    const setting = await $('[data-testid="clone-submodules-recursively"]');
    await setting.waitForDisplayed({ timeout: 30000 });
    assert.equal(await setting.isSelected(), true);
  });

  it("offers the SSH form of a URL to clone", async () => {
    await $('[data-testid="toggle-clone"]').click();
    const url = await $('[data-testid="clone-url"]');
    await url.setValue("https://example.com/owner/repo.git");
    const ssh = await $('[data-testid="clone-url-ssh"]');
    await browser.waitUntil(() => ssh.isEnabled(), { timeout: 30000 });

    await ssh.click();

    assert.equal(await url.getValue(), "git@example.com:owner/repo.git");
  });

  it("clones a repository from a URL and opens the clone as a Project in a window of its own", async () => {
    const target = path.join(process.env.GIKLI_E2E_SANDBOX, "clone");
    const windowsBefore = await browser.getWindowHandles();

    await startClone(target);

    await $('[data-testid="clone"]').waitForDisplayed({ timeout: 30000, reverse: true });
    await inNewWindow(windowsBefore, () => expectRepositoryRoot(target));
  });

  it("keeps showing the startup Project in the main window after a clone", async () => {
    await expectRepositoryRoot(process.env.GIKLI_E2E_REPOSITORY);
  });

  it("cancels a clone from the view of running operations while its post-checkout hook blocks", async () => {
    const sandbox = process.env.GIKLI_E2E_SANDBOX;
    const target = path.join(sandbox, "cancelled clone");
    const windowsBefore = await browser.getWindowHandles();

    fs.writeFileSync(path.join(sandbox, "block-post-checkout"), "");
    await $('[data-testid="toggle-clone"]').click();
    await startClone(target);

    const hook = await $('[data-testid="operation"] [data-testid="operation-hook"]');
    await hook.waitForDisplayed({ timeout: 30000 });
    assert.match(await hook.getText(), /post-checkout/);
    const cancel = await $('[data-testid="operation"] [data-testid="cancel-operation"]');
    await cancel.waitForDisplayed({ timeout: 30000 });
    await cancel.click();

    // Queried afresh: the view is rebuilt on every event, and any rebuild would make a held element look gone.
    await browser.waitUntil(async () => (await $$('[data-testid="operation"]')).length === 0, {
      timeout: 30000,
      timeoutMsg: "the operation stayed in the view",
    });
    await inNewWindow(windowsBefore, async () => {
      await expectRepositoryRoot(target);
      const notification = await $('[data-testid="notification"][data-kind="hookNotFinished"]');
      await notification.waitForDisplayed({ timeout: 30000 });
      assert.match(await notification.getText(), /post-checkout/);
    });
  });
});
