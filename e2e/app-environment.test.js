// Tests of the environment the smoke test runs gikli in, which keeps the run inside its sandbox
// (issue #107).

import assert from "node:assert/strict";
import path from "node:path";
import { describe, it } from "node:test";

import { appEnvironment } from "./app-environment.js";

const sandbox = path.join("tmp", "gikli-e2e-sandbox");
const repository = path.join(sandbox, "smoke repo");
const smokeTest = { PATH: "the path", GIKLI_E2E_SANDBOX: sandbox, GIKLI_E2E_REPOSITORY: repository };

describe("appEnvironment", () => {
  it("opens the smoke test's repository on startup", () => {
    assert.equal(appEnvironment(smokeTest).GIKLI_PROJECT_FOLDER, repository);
  });

  it("keeps gikli's settings and Projects in the sandbox, away from the developer's", () => {
    const folder = appEnvironment(smokeTest).GIKLI_CONFIG_FOLDER;

    assert.equal(path.dirname(folder), sandbox);
  });

  it("runs git with the sandbox's global configuration", () => {
    assert.equal(path.dirname(appEnvironment(smokeTest).GIT_CONFIG_GLOBAL), sandbox);
  });

  it("keeps the rest of the smoke test's environment", () => {
    assert.equal(appEnvironment(smokeTest).PATH, "the path");
  });

  it("puts the sandbox's config folder in place of one the developer set", () => {
    const environment = appEnvironment({ ...smokeTest, GIKLI_CONFIG_FOLDER: "the developer's" });

    assert.equal(path.dirname(environment.GIKLI_CONFIG_FOLDER), sandbox);
  });
});
