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

/** The options that give git who commits for the smoke test's own fixtures. */
export const FIXTURE_IDENTITY = ["-c", "user.name=gikli", "-c", "user.email=gikli@example.com"];

/** Commits what is staged in `cwd`. */
export function commitStaged(cwd, gitconfig, message) {
  runGit(cwd, gitconfig, ...FIXTURE_IDENTITY, "commit", "--quiet", "--message", message);
}
