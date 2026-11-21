// git for the smoke test's own fixtures, isolated from the developer's git configuration.

import { spawnSync } from "node:child_process";
import path from "node:path";

/** The global git configuration of `sandbox`. */
export function sandboxGitconfig(sandbox) {
  return path.join(sandbox, ".gitconfig");
}

/** Runs git in `cwd`, isolated from the developer's git configuration, and returns what it wrote. */
export function runGit(cwd, gitconfig, ...args) {
  const git = spawnSync("git", args, {
    cwd,
    env: { ...process.env, GIT_CONFIG_GLOBAL: gitconfig, GIT_CONFIG_NOSYSTEM: "1" },
  });
  if (git.status !== 0) {
    throw new Error(`git ${args.join(" ")} failed: ${git.stderr}`);
  }
  return String(git.stdout);
}

/** How long {@link runGitPastIndexLock} waits for the index to be unlocked, in milliseconds. */
const INDEX_LOCK_PATIENCE = 30000;

/**
 * Runs git as {@link runGit} does, and again while another git holds the lock of the index, as the
 * app's own git does while it writes the index: during a cherry-pick or a revert that a case failed
 * in the middle of, when its cleanup starts at once.
 */
export async function runGitPastIndexLock(cwd, gitconfig, ...args) {
  const until = Date.now() + INDEX_LOCK_PATIENCE;
  for (;;) {
    try {
      return runGit(cwd, gitconfig, ...args);
    } catch (error) {
      if (!error.message.includes("index.lock") || Date.now() > until) {
        throw error;
      }
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
  }
}

/** The options that give git who commits for the smoke test's own fixtures. */
export const FIXTURE_IDENTITY = ["-c", "user.name=gikli", "-c", "user.email=gikli@example.com"];

/** Commits what is staged in `cwd`. */
export function commitStaged(cwd, gitconfig, message) {
  runGit(cwd, gitconfig, ...FIXTURE_IDENTITY, "commit", "--quiet", "--message", message);
}
