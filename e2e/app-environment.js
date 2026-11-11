// The environment the smoke test runs gikli in, isolated from the developer's own gikli and git.

import path from "node:path";

import { sandboxGitconfig } from "./sandbox-git.js";

/**
 * The environment gikli runs in, from the smoke test's `env`: it opens the smoke test's repository
 * on startup, keeps its settings and Projects in the sandbox, and the git it runs reads the
 * sandbox's global configuration. A run then starts from default settings and no Projects, and
 * leaves nothing outside the sandbox.
 */
export function appEnvironment(env) {
  return {
    ...env,
    GIKLI_PROJECT_FOLDER: env.GIKLI_E2E_REPOSITORY,
    GIKLI_CONFIG_FOLDER: path.join(env.GIKLI_E2E_SANDBOX, "gikli-config"),
    GIT_CONFIG_GLOBAL: sandboxGitconfig(env.GIKLI_E2E_SANDBOX),
  };
}
