// Tests of the git the smoke test runs for its own fixtures, on a real repository (ADR-0006).

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { describe, it } from "node:test";

import { runGit, runGitPastIndexLock, sandboxGitconfig } from "./sandbox-git.js";

/** A new repository in a temporary folder, removed when the test ends, with a file git does not track yet. */
function repository(t) {
  const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "gikli-sandbox-git-"));
  t.after(() => fs.rmSync(sandbox, { recursive: true, force: true }));
  const gitconfig = sandboxGitconfig(sandbox);
  fs.writeFileSync(gitconfig, "");
  const root = path.join(sandbox, "repository");
  fs.mkdirSync(root);
  runGit(root, gitconfig, "init", "--quiet");
  fs.writeFileSync(path.join(root, "file.txt"), "new");
  return { root, gitconfig, lock: path.join(root, ".git", "index.lock") };
}

describe("runGitPastIndexLock", () => {
  it("runs git once the index is no longer locked, as by the app's own git", async (t) => {
    const { root, gitconfig, lock } = repository(t);
    fs.writeFileSync(lock, "");
    setTimeout(() => fs.rmSync(lock), 500);

    await runGitPastIndexLock(root, gitconfig, "add", "--", "file.txt");

    assert.equal(runGit(root, gitconfig, "diff", "--cached", "--name-only"), "file.txt\n");
  });

  it("fails at once with git's reason when git fails for anything else", async (t) => {
    const { root, gitconfig } = repository(t);
    const started = Date.now();

    await assert.rejects(runGitPastIndexLock(root, gitconfig, "switch", "--quiet", "no-such-branch"), /no-such-branch/);

    assert.ok(Date.now() - started < 5000, "it does not wait");
  });
});
