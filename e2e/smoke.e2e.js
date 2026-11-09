// Checks the wiring between the UI and the core, not behavior: the core's tests cover that.

import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { Key } from "webdriverio";

import { doubleClickUntilDisplayed } from "./double-click.js";
import { commitStaged, runGit, sandboxGitconfig } from "./sandbox-git.js";

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

/** Opens the diff of the changed file at `file` with a double-click, and waits for its local side, which takes the focus. */
async function openDiff(file) {
  const row = `[data-testid="change"][data-path="${file}"]`;
  await $(row).waitForDisplayed({ timeout: 30000 });
  await doubleClickUntilDisplayed(browser, row, '[data-testid="diff"]');
  await expectDiffOf(file);
  await $(".cm-merge-b .cm-content").waitForDisplayed({ timeout: 30000 });
}

/** Waits for the diff view to show the file at `file`. */
async function expectDiffOf(file) {
  const title = await $('[data-testid="diff-title"]');
  await browser.waitUntil(async () => (await title.getText()) === file, {
    timeout: 30000,
    timeoutMsg: `the diff of ${file} did not show`,
  });
}

/**
 * Checks that the diff view still shows the file at `file` a moment later. Nothing on screen says
 * that what could still take it away, a late read or a closing window, will not happen.
 */
async function expectDiffStaysOn(file) {
  await browser.pause(1000);
  assert.equal(await $('[data-testid="diff-title"]').getText(), file);
}

/** Closes the diff, which saves what is edited, and waits for the changes view. */
async function closeDiff() {
  await $('[data-testid="diff-close"]').click();
  await $('[data-testid="diff"]').waitForDisplayed({ timeout: 30000, reverse: true });
  await $('[data-testid="changes"]').waitForDisplayed({ timeout: 30000 });
}

/** Types "x" at the cursor of the local side, and waits for the view to count it as an unsaved edit. */
async function typeEdit() {
  await browser.keys(["x"]);
  const diff = await $('[data-testid="diff"]');
  await browser.waitUntil(async () => (await diff.getAttribute("data-edited")) === "true", {
    timeout: 30000,
    timeoutMsg: "the edit did not reach the local side",
  });
}

/** Waits for the cursor to be on the line of the local side that reads `text`. */
async function expectCursorOn(text) {
  const cursorLine = () =>
    browser.execute(() => {
      const node = document.getSelection()?.focusNode;
      return (node instanceof Element ? node : node?.parentElement)?.closest(".cm-line")?.textContent ?? null;
    });
  await browser.waitUntil(async () => (await cursorLine()) === text, {
    timeout: 30000,
    timeoutMsg: `the cursor did not reach "${text}"`,
  });
}

/** Waits for the diff view to say that a save failed because the file changed on disk, and to offer to discard the edits. */
async function expectSaveRefused() {
  const message = await $('[data-testid="diff-message"]');
  await browser.waitUntil(async () => /changed on disk/.test(await message.getText()), {
    timeout: 30000,
    timeoutMsg: "the reason the save failed did not show",
  });
  await $('[data-testid="diff-discard-edits"]').waitForDisplayed({ timeout: 30000 });
}

/**
 * Asks the current window to close with the command `getCurrentWindow().close()` sends, so the close
 * goes through the Rust side to `onCloseRequested`. It does not wait for the answer, which may never
 * come from a window that is going away.
 */
async function requestWindowClose() {
  await browser.execute(() => {
    const { invoke, metadata } = window.__TAURI_INTERNALS__;
    void invoke("plugin:window|close", { label: metadata.currentWindow.label });
  });
}

/** Clones the smoke test's remote into `name` in the sandbox, and returns the clone with the windows open before it. */
async function cloneForNewWindow(name) {
  const clone = path.join(process.env.GIKLI_E2E_SANDBOX, name);
  const windowsBefore = await browser.getWindowHandles();
  await $('[data-testid="toggle-clone"]').click();
  await startClone(clone);
  await $('[data-testid="clone"]').waitForDisplayed({ timeout: 30000, reverse: true });
  return { clone, windowsBefore };
}

/** The file every clone of the smoke test's remote holds. */
const REMOTE_FILE = process.env.GIKLI_E2E_REMOTE_FILE;

/** Changes {@link REMOTE_FILE} in the window of `clone`, opens its diff and types an edit in it. */
async function editRemoteFile(clone) {
  await expectRepositoryRoot(clone);
  const file = path.join(clone, REMOTE_FILE);
  fs.writeFileSync(file, "changed\n");
  await (await idleRefresh()).click();
  await openDiff(REMOTE_FILE);
  await typeEdit();
  return file;
}

/** A tracked file with two differences, followed by {@link SECOND_FILE} in the changes view. */
const FIRST_FILE = "review first.txt";
/** A tracked file with one difference, which follows {@link FIRST_FILE} in the changes view. */
const SECOND_FILE = "review second.txt";

/** Twenty numbered lines, with the lines in `changed` marked as changed. */
function numberedLines(...changed) {
  return Array.from({ length: 20 }, (_, index) => `line ${index + 1}${changed.includes(index + 1) ? " changed" : ""}\n`).join("");
}

/** The full path of `file` in the smoke test's repository. */
function inRepository(file) {
  return path.join(process.env.GIKLI_E2E_REPOSITORY, file);
}

/** Gives {@link FIRST_FILE} two differences and {@link SECOND_FILE} one, as they come next to each other in the changes view. */
function changeReviewFiles() {
  fs.writeFileSync(inRepository(FIRST_FILE), numberedLines(2, 15));
  fs.writeFileSync(inRepository(SECOND_FILE), numberedLines(3));
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

  // These come after the cases that assert the exact list of changes, because they add to it.
  describe("in the diff of tracked files", () => {
    before(async () => {
      const repository = process.env.GIKLI_E2E_REPOSITORY;
      const gitconfig = sandboxGitconfig(process.env.GIKLI_E2E_SANDBOX);
      fs.writeFileSync(inRepository(FIRST_FILE), numberedLines());
      fs.writeFileSync(inRepository(SECOND_FILE), numberedLines());
      runGit(repository, gitconfig, "add", "--", FIRST_FILE, SECOND_FILE);
      commitStaged(repository, gitconfig, "Files to review");
      changeReviewFiles();
      await (await idleRefresh()).click();
    });

    beforeEach(changeReviewFiles);

    it("goes to the next difference with F7 and to the previous one with Shift+F7", async () => {
      await openDiff(FIRST_FILE);
      await expectCursorOn("line 2 changed");

      await browser.keys([Key.F7]);
      await expectCursorOn("line 15 changed");

      await browser.keys([Key.Shift, Key.F7]);
      await expectCursorOn("line 2 changed");

      await closeDiff();
    });

    it("goes to the next file with F7 after the last difference", async () => {
      await openDiff(FIRST_FILE);
      await browser.keys([Key.F7]);
      await expectCursorOn("line 15 changed");

      await browser.keys([Key.F7]);

      await expectDiffOf(SECOND_FILE);
      await closeDiff();
    });

    it("goes to the next file with Ctrl+Alt+Right and to the previous one with Ctrl+Alt+Left", async () => {
      await openDiff(FIRST_FILE);

      await browser.keys([Key.Ctrl, Key.Alt, Key.ArrowRight]);
      await expectDiffOf(SECOND_FILE);

      await browser.keys([Key.Ctrl, Key.Alt, Key.ArrowLeft]);
      await expectDiffOf(FIRST_FILE);

      await closeDiff();
    });

    it("saves what the user edited on moving to the next file", async () => {
      await openDiff(FIRST_FILE);
      await typeEdit();

      await browser.keys([Key.Ctrl, Key.Alt, Key.ArrowRight]);

      await expectDiffOf(SECOND_FILE);
      assert.ok(fs.readFileSync(inRepository(FIRST_FILE), "utf8").includes("xline 2 changed"), "the edit is not on disk");
      await closeDiff();
    });

    it("stays in the file with the reason and the way to discard the edits when the save on moving fails", async () => {
      const file = inRepository(FIRST_FILE);
      const contentBefore = fs.readFileSync(file);
      await openDiff(FIRST_FILE);
      await typeEdit();
      // After the edit: before it, the view would just read the file again.
      fs.writeFileSync(file, "changed elsewhere\n");

      await browser.keys([Key.Ctrl, Key.Alt, Key.ArrowRight]);

      await expectSaveRefused();
      assert.equal(await $('[data-testid="diff-title"]').getText(), FIRST_FILE);

      // The button to discard asks through a native dialog WebDriver cannot answer. With the file
      // back as the user saw it, the save goes through, and the view closes for the next case.
      fs.writeFileSync(file, contentBefore);
      await closeDiff();
    });

    it("shows the next file when a file changes on disk during its read", async () => {
      const sandbox = process.env.GIKLI_E2E_SANDBOX;
      const hold = path.join(sandbox, "hold-reads");
      const heldLog = path.join(sandbox, "held-reads");
      const heldReads = () => (fs.existsSync(heldLog) ? fs.readFileSync(heldLog, "utf8").trim().split("\n").length : 0);
      await openDiff(FIRST_FILE);
      // A read under way would count as one of the reads held below, and so would one started by
      // what the watcher has yet to announce of the files written before this case.
      await browser.pause(1000);
      await idleRefresh();
      fs.writeFileSync(hold, "");
      try {
        await browser.keys([Key.Ctrl, Key.Alt, Key.ArrowRight]);
        await browser.waitUntil(() => heldReads() >= 1, { timeout: 30000, timeoutMsg: "the read of the next file did not start" });

        fs.writeFileSync(inRepository("during the read.txt"), "new\n");
        // The read of the next file, and the two the change starts: the changes view and the diff.
        await browser.waitUntil(() => heldReads() >= 3, { timeout: 30000, timeoutMsg: "the change on disk started no read" });
      } finally {
        fs.rmSync(hold, { force: true });
      }

      await expectDiffOf(SECOND_FILE);
      // Whichever read ends last, the view stays on the file the user moved to.
      await idleRefresh();
      await expectDiffStaysOn(SECOND_FILE);
      await closeDiff();
    });
  });

  // In a window of a clone: closing the main window would end the WebDriver session.
  describe("on closing a window with unsaved edits", () => {
    it("saves them, and the window goes away", async () => {
      const { clone, windowsBefore } = await cloneForNewWindow("closed clone");
      let file;

      await inNewWindow(windowsBefore, async () => {
        file = await editRemoteFile(clone);
        await requestWindowClose();
      });

      await browser.waitUntil(async () => (await browser.getWindowHandles()).length === windowsBefore.length, {
        timeout: 30000,
        timeoutMsg: "the window stayed open",
      });
      assert.match(fs.readFileSync(file, "utf8"), /^xchanged/);
    });

    it("keeps the window open with the reason when the save fails", async () => {
      const { clone, windowsBefore } = await cloneForNewWindow("kept clone");

      await inNewWindow(windowsBefore, async () => {
        const file = await editRemoteFile(clone);
        fs.writeFileSync(file, "changed elsewhere\n");

        await requestWindowClose();

        await expectSaveRefused();
        await expectDiffStaysOn(REMOTE_FILE);
      });
    });
  });
});
