import assert from "node:assert/strict";
import path from "node:path";
import { describe, it } from "node:test";

import { failureOutputFolder, screenshotPath } from "./failure-output.js";

describe("failureOutputFolder", () => {
  it("is the folder in GIKLI_E2E_OUTPUT", () => {
    assert.equal(failureOutputFolder({ GIKLI_E2E_OUTPUT: "/tmp/smoke output" }), "/tmp/smoke output");
  });

  it("is absent without GIKLI_E2E_OUTPUT, so a developer's run writes no files", () => {
    assert.equal(failureOutputFolder({}), undefined);
    assert.equal(failureOutputFolder({ GIKLI_E2E_OUTPUT: "" }), undefined);
  });
});

describe("screenshotPath", () => {
  it("names the screenshot after the case, in the folder", () => {
    assert.equal(
      screenshotPath("out", 'gikli in the log lists the commits a "deleted" branch took away'),
      path.join("out", "gikli-in-the-log-lists-the-commits-a-deleted-branch-took-away.png"),
    );
  });

  it("keeps the name of a long title short enough for any file system", () => {
    const name = path.basename(screenshotPath("out", "a".repeat(400)));
    assert.equal(name, `${"a".repeat(150)}.png`);
  });

  it("gives a title with nothing to keep a name still", () => {
    assert.equal(screenshotPath("out", '""'), path.join("out", "case.png"));
  });
});
