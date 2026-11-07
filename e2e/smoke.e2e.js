// Checks the wiring between the UI and the core, not behavior: the core's tests cover that.

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

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

    const added = "after refresh.txt";
    fs.writeFileSync(path.join(process.env.GIKLI_E2E_REPOSITORY, added), "later\n");
    await $('[data-testid="refresh-changes"]').click();

    const paths = async () => Promise.all(await $$('[data-testid="change-path"]').map((item) => item.getText()));
    await browser.waitUntil(async () => (await paths()).length === 2, { timeout: 30000 });
    assert.deepEqual(await paths(), [added, untracked]);
  });

  it("opens the diff of a changed file on a double-click, and saves the local side", async () => {
    const untracked = process.env.GIKLI_E2E_UNTRACKED_FILE;
    const row = await $(`[data-testid="change"][data-path="${untracked}"]`);
    await row.doubleClick();

    const diff = await $('[data-testid="diff"]');
    await diff.waitForDisplayed({ timeout: 30000 });
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
    fs.writeFileSync(path.join(sandbox, "ask-for-prompt"), "");
    await $('[data-testid="refresh-changes"]').click();

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

  // The clone opens in a window of its own, which not every WebDriver can reach, so this stops at
  // the form closing, which it does once the core has opened the clone as a Project.
  it("clones a repository from a URL", async () => {
    const remote = process.env.GIKLI_E2E_REMOTE;
    const target = path.join(process.env.GIKLI_E2E_SANDBOX, "clone");

    await $('[data-testid="clone-url"]').setValue(pathToFileURL(remote).href);
    await $('[data-testid="clone-folder"]').setValue(target);
    await $('[data-testid="start-clone"]').click();

    await $('[data-testid="clone"]').waitForDisplayed({ timeout: 30000, reverse: true });
  });
});
