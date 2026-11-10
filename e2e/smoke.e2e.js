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

  // After the cases that count on which Changelist is active.
  describe("with a Changelist the user created", () => {
    /** A file git tracks, as a new file git does not track goes to Unversioned Files. */
    const STAGED_FILE = "for the new changelist.txt";

    /** The selector of the Changelist called `name`. */
    const changelist = (name) => `[data-testid="changelist"][data-name="${name}"]`;

    /** Waits for the Changelist called `name` to be the Active changelist. */
    async function expectActive(name) {
      await browser.waitUntil(async () => (await $(changelist(name)).getAttribute("data-active")) === "true", {
        timeout: 30000,
        timeoutMsg: `${name} did not become active`,
      });
    }

    /** Clicks the button `testId` of the Changelist called `name`, once no read can draw it again. */
    async function clickInChangelist(name, testId) {
      await idleRefresh();
      await $(`${changelist(name)} [data-testid="${testId}"]`).click();
    }

    /** Waits for the Changelist called `name` to show the change at `file`. */
    async function expectChangeIn(name, file) {
      await $(`${changelist(name)} [data-testid="change"][data-path="${file}"]`).waitForDisplayed({ timeout: 30000 });
    }

    /**
     * Opens the context menu of what `selector` finds with a right-click. On Linux it fires the event
     * the click would: after a right-click from WebKitWebDriver, whatever the element, every key it
     * types for the rest of the session loses Shift, so "Moved" reaches the field as "moved" and a
     * path to clone as "7tmp7...".
     */
    async function openContextMenu(selector) {
      const target = await $(selector);
      await target.waitForDisplayed({ timeout: 30000 });
      if (process.platform !== "linux") {
        await target.click({ button: "right" });
        return;
      }
      await browser.execute((targetSelector) => {
        const element = document.querySelector(targetSelector);
        const { left, top, width, height } = element.getBoundingClientRect();
        const center = { clientX: left + width / 2, clientY: top + height / 2 };
        element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, ...center }));
      }, selector);
    }

    it("creates a Changelist, sets it active, and a new change goes to it", async () => {
      await $('[data-testid="new-changelist"]').click();
      await $('[data-testid="changelist-form-name"]').setValue("Feature");
      await $('[data-testid="changelist-form-description"]').setValue("Work for the smoke test");
      await $('[data-testid="changelist-form-submit"]').click();
      await $(changelist("Feature")).waitForDisplayed({ timeout: 30000 });
      assert.equal(await $(`${changelist("Feature")} [data-testid="changelist-description"]`).getText(), "Work for the smoke test");

      await clickInChangelist("Feature", "set-active-changelist");
      await expectActive("Feature");

      fs.writeFileSync(inRepository(STAGED_FILE), "new\n");
      runGit(process.env.GIKLI_E2E_REPOSITORY, sandboxGitconfig(process.env.GIKLI_E2E_SANDBOX), "add", "--", STAGED_FILE);
      await (await idleRefresh()).click();
      await expectChangeIn("Feature", STAGED_FILE);
    });

    it("renames a Changelist, and deleting it moves its changes to the Active changelist", async () => {
      assert.equal(await $(`${changelist("Feature")} [data-testid="delete-changelist"]`).isExisting(), false);

      await clickInChangelist("Feature", "rename-changelist");
      await $('[data-testid="changelist-form-name"]').setValue("Renamed");
      await $('[data-testid="changelist-form-submit"]').click();
      await $(changelist("Renamed")).waitForDisplayed({ timeout: 30000 });

      await clickInChangelist("Changes", "set-active-changelist");
      await expectActive("Changes");
      await clickInChangelist("Renamed", "delete-changelist");

      await $(changelist("Renamed")).waitForDisplayed({ timeout: 30000, reverse: true });
      await expectChangeIn("Changes", STAGED_FILE);
    });

    it("moves a selected change to a new Changelist from the menu, which becomes active", async () => {
      await idleRefresh();
      await openContextMenu(`${changelist("Changes")} [data-testid="change"][data-path="${STAGED_FILE}"]`);
      await $('[data-testid="move-to-another-changelist"]').click();
      // With one Changelist, the only place to go is a new one.
      await $('[data-testid="move-form-name"]').setValue("Moved");
      await $('[data-testid="move-form-make-active"]').click();
      await $('[data-testid="move-form-submit"]').click();

      await expectChangeIn("Moved", STAGED_FILE);
      await expectActive("Moved");
    });

    it("moves a change to another Changelist by dragging it", async () => {
      await idleRefresh();
      const row = `${changelist("Moved")} [data-testid="change"][data-path="${STAGED_FILE}"]`;
      assert.equal(await $(row).getAttribute("draggable"), "true");
      // tauri-driver speaks classic WebDriver, whose mouse does not start an HTML drag.
      await browser.execute(
        (rowSelector, listSelector) => {
          const dataTransfer = new DataTransfer();
          const fire = (target, type) => target.dispatchEvent(new DragEvent(type, { bubbles: true, cancelable: true, dataTransfer }));
          const row = document.querySelector(rowSelector);
          const list = document.querySelector(listSelector);
          fire(row, "dragstart");
          fire(list, "dragover");
          fire(list, "drop");
          fire(row, "dragend");
        },
        row,
        changelist("Changes"),
      );

      await expectChangeIn("Changes", STAGED_FILE);
    });

    it("keeps a comment on a Changelist", async () => {
      await clickInChangelist("Moved", "comment-changelist");
      await $('[data-testid="changelist-form-comment"]').setValue("Add the feature");
      await $('[data-testid="changelist-form-submit"]').click();

      const comment = $(`${changelist("Moved")} [data-testid="changelist-comment"]`);
      await comment.waitForDisplayed({ timeout: 30000 });
      assert.equal(await comment.getText(), "Add the feature");
    });

    it("adds an Unversioned File from the menu, and rolls it back only after the list is confirmed", async () => {
      const file = "added and rolled back.txt";
      fs.writeFileSync(inRepository(file), "new\n");
      await (await idleRefresh()).click();

      await openContextMenu(`[data-testid="unversioned-files"] [data-testid="change"][data-path="${file}"]`);
      await $('[data-testid="add-to-git"]').click();
      await expectChangeIn("Moved", file);

      await idleRefresh();
      await openContextMenu(`${changelist("Moved")} [data-testid="change"][data-path="${file}"]`);
      await $('[data-testid="rollback-changes"]').click();
      await $(`[data-testid="rollback-file"][data-path="${file}"]`).waitForDisplayed({ timeout: 30000 });
      assert.ok(fs.existsSync(inRepository(file)), "nothing goes before the user confirms");

      await $('[data-testid="rollback-delete-added"]').click();
      await $('[data-testid="rollback-confirm"]').click();

      await $(`[data-testid="change"][data-path="${file}"]`).waitForDisplayed({ timeout: 30000, reverse: true });
      assert.ok(!fs.existsSync(inRepository(file)), "its local copy goes with it");
    });

    it("commits the Active changelist from the panel beside the changes, once it knows who is committing", async () => {
      const repository = process.env.GIKLI_E2E_REPOSITORY;
      const gitconfig = sandboxGitconfig(process.env.GIKLI_E2E_SANDBOX);
      const file = "committed from the panel.txt";
      fs.writeFileSync(inRepository(file), "new\n");
      runGit(repository, gitconfig, "add", "--", file);
      await (await idleRefresh()).click();
      await expectChangeIn("Moved", file);

      const tick = (path) => $(`[data-testid="commit-file"][data-path="${path}"] input`);
      await browser.waitUntil(async () => (await tick(file).isExisting()) && tick(file).isSelected(), {
        timeout: 30000,
        timeoutMsg: "the file of the Active changelist was not ticked",
      });
      assert.equal(await tick(STAGED_FILE).isSelected(), false);
      await browser.waitUntil(async () => (await $('[data-testid="commit-message"]').getValue()) === "Add the feature", {
        timeout: 30000,
        timeoutMsg: "the message was not pre-filled from the comment of the Changelist",
      });

      await $('[data-testid="commit-message"]').setValue("Commit from the panel");
      await $('[data-testid="commit-submit"]').click();
      await $('[data-testid="commit-identity-name"]').waitForDisplayed({ timeout: 30000 });
      assert.equal(runGit(repository, gitconfig, "rev-list", "--count", "HEAD"), "1\n", "nothing is committed before the answer");

      await $('[data-testid="commit-identity-name"]').setValue("Smoke");
      await $('[data-testid="commit-identity-email"]').setValue("smoke@example.com");
      await $('[data-testid="commit-identity-save"]').click();

      await $(`[data-testid="change"][data-path="${file}"]`).waitForDisplayed({ timeout: 30000, reverse: true });
      assert.equal(runGit(repository, gitconfig, "log", "-1", "--format=%s|%an"), "Commit from the panel|Smoke\n");
      assert.equal(await $('[data-testid="commit-message"]').getValue(), "");
    });

    it("keeps the message after a commit when the option to clear it is off", async () => {
      const repository = process.env.GIKLI_E2E_REPOSITORY;
      const gitconfig = sandboxGitconfig(process.env.GIKLI_E2E_SANDBOX);
      const setting = $('[data-testid="clear-commit-message-after-commit"]');
      if (!(await setting.isDisplayed())) {
        await $('[data-testid="toggle-settings"]').click();
      }
      await setting.waitForDisplayed({ timeout: 30000 });
      assert.equal(await setting.isSelected(), true);
      await setting.click();

      const file = "committed with the message kept.txt";
      fs.writeFileSync(inRepository(file), "new\n");
      runGit(repository, gitconfig, "add", "--", file);
      await (await idleRefresh()).click();
      await expectChangeIn("Moved", file);
      const message = $('[data-testid="commit-message"]');
      await browser.waitUntil(async () => (await message.getValue()) === "Add the feature", {
        timeout: 30000,
        timeoutMsg: "the message was not pre-filled from the comment of the Changelist",
      });

      await $('[data-testid="commit-submit"]').click();

      await $(`[data-testid="change"][data-path="${file}"]`).waitForDisplayed({ timeout: 30000, reverse: true });
      assert.equal(runGit(repository, gitconfig, "log", "-1", "--format=%s"), "Add the feature\n");
      assert.equal(await message.getValue(), "Add the feature");
      await setting.click();
    });

    it("amends the last commit from the panel, starting from its message", async () => {
      const repository = process.env.GIKLI_E2E_REPOSITORY;
      const gitconfig = sandboxGitconfig(process.env.GIKLI_E2E_SANDBOX);
      const message = $('[data-testid="commit-message"]');
      const amend = $('[data-testid="commit-amend"]');
      const commitNew = async (file) => {
        fs.writeFileSync(inRepository(file), "new\n");
        runGit(repository, gitconfig, "add", "--", file);
        await (await idleRefresh()).click();
        await expectChangeIn("Moved", file);
      };

      const first = "committed before the amend.txt";
      await commitNew(first);
      await message.setValue("To be amended");
      await $('[data-testid="commit-submit"]').click();
      await $(`[data-testid="change"][data-path="${first}"]`).waitForDisplayed({ timeout: 30000, reverse: true });
      const commits = runGit(repository, gitconfig, "rev-list", "--count", "HEAD");

      const second = "amended from the panel.txt";
      await commitNew(second);
      await browser.waitUntil(async () => (await message.getValue()) === "Add the feature", {
        timeout: 30000,
        timeoutMsg: "the message was not pre-filled from the comment of the Changelist",
      });
      await amend.click();
      await browser.waitUntil(async () => (await message.getValue()) === "To be amended", {
        timeout: 30000,
        timeoutMsg: "the message was not pre-filled from the last commit",
      });
      await amend.click();
      await browser.waitUntil(async () => (await message.getValue()) === "Add the feature", {
        timeout: 30000,
        timeoutMsg: "unticking did not bring back the message from the comment",
      });
      await amend.click();
      await browser.waitUntil(async () => (await message.getValue()) === "To be amended", {
        timeout: 30000,
        timeoutMsg: "ticking again did not bring back the message of the last commit",
      });

      await message.setValue("Amended from the panel");
      await amend.click();
      await amend.click();
      await $('[data-testid="commit-submit"]').click();

      await $(`[data-testid="change"][data-path="${second}"]`).waitForDisplayed({ timeout: 30000, reverse: true });
      assert.equal(runGit(repository, gitconfig, "rev-list", "--count", "HEAD"), commits, "the last commit is replaced");
      assert.equal(runGit(repository, gitconfig, "log", "-1", "--format=%s"), "Amended from the panel\n");
      assert.deepEqual(runGit(repository, gitconfig, "show", "--format=", "--name-only", "HEAD").trim().split("\n").sort(), [first, second].sort());
      assert.equal(await amend.isSelected(), false, "the next commit is a new one");
    });

    it("shows the commits in the log, and goes back to the changes", async () => {
      await $('[data-testid="open-log"]').click();
      const row = $('[data-testid="log-row"] .log-subject');
      await row.waitForDisplayed({ timeout: 30000 });
      assert.equal(await row.getText(), "Amended from the panel");

      await $('[data-testid="log-close"]').click();
      await $('[data-testid="changes"]').waitForDisplayed({ timeout: 30000 });
    });

    it("works with the index in Staging mode, and brings the Changelists back", async () => {
      const repository = process.env.GIKLI_E2E_REPOSITORY;
      const gitconfig = sandboxGitconfig(process.env.GIKLI_E2E_SANDBOX);
      const setting = $('[data-testid="staging-mode"]');
      if (!(await setting.isDisplayed())) {
        await $('[data-testid="toggle-settings"]').click();
      }
      await setting.waitForDisplayed({ timeout: 30000 });
      assert.equal(await setting.isSelected(), false, "Changelist mode is the default");

      const kept = "kept in its changelist.txt";
      fs.writeFileSync(inRepository(kept), "committed\n");
      runGit(repository, gitconfig, "add", "--", kept);
      runGit(repository, gitconfig, "commit", "--quiet", "--message=Tracked", "--", kept);
      fs.writeFileSync(inRepository(kept), "changed\n");
      await (await idleRefresh()).click();
      await expectChangeIn("Moved", kept);

      const row = (group, file) => `[data-testid="${group}"] [data-testid="change"][data-path="${file}"]`;
      const expectRow = (group, file) => $(row(group, file)).waitForDisplayed({ timeout: 30000 });
      const chooseFromMenu = async (group, file, item) => {
        await idleRefresh();
        await openContextMenu(row(group, file));
        await $(`[data-testid="${item}"]`).click();
      };

      await setting.click();
      await expectRow("unstaged-changes", kept);
      await browser.waitUntil(async () => !(await $('[data-testid="new-changelist"]').isDisplayed()), {
        timeout: 30000,
        timeoutMsg: "Staging mode still offers to create a Changelist",
      });

      await chooseFromMenu("unstaged-changes", kept, "stage-files");
      await expectRow("staged-changes", kept);
      assert.equal(await $(row("staged-changes", kept)).getAttribute("data-side"), "staged");

      await doubleClickUntilDisplayed(browser, row("staged-changes", kept), '[data-testid="diff"]');
      await expectDiffOf(kept);
      await $(".cm-merge-b .cm-content").waitForDisplayed({ timeout: 30000 });
      assert.equal(await $('[data-testid="diff-local-label"]').getText(), "Index (read-only)");
      await closeDiff();

      await chooseFromMenu("staged-changes", kept, "unstage-files");
      await expectRow("unstaged-changes", kept);

      const committed = "committed in staging mode.txt";
      fs.writeFileSync(inRepository(committed), "new\n");
      await (await idleRefresh()).click();
      await expectRow("unversioned-files", committed);
      await chooseFromMenu("unversioned-files", committed, "add-to-git");
      await expectRow("staged-changes", committed);
      await $('[data-testid="commit-message"]').setValue("From the index");
      await $('[data-testid="commit-submit"]').click();
      await $(row("staged-changes", committed)).waitForDisplayed({ timeout: 30000, reverse: true });
      assert.equal(runGit(repository, gitconfig, "log", "-1", "--format=%s"), "From the index\n");
      const inCommit = runGit(repository, gitconfig, "show", "--format=", "--name-only", "HEAD").trim().split("\n");
      assert.ok(inCommit.includes(committed), "the commit holds what was staged");
      assert.ok(!inCommit.includes(kept), "the commit leaves out what was not staged");

      await idleRefresh();
      await setting.click();
      await expectChangeIn("Moved", kept);
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
