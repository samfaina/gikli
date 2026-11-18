// Where the smoke test leaves what it saw when a case failed, for CI to keep (issue #152): the
// WebDriver logs of the run and a screenshot of each case that failed.

import path from "node:path";

/** The longest name a screenshot gets, before its extension. */
const MAX_NAME_LENGTH = 150;

/**
 * The folder to write the WebDriver logs and the screenshots of failed cases to, from
 * GIKLI_E2E_OUTPUT, or undefined when it is not set, as on a developer's run.
 */
export function failureOutputFolder(env) {
  const folder = env.GIKLI_E2E_OUTPUT;
  return folder === undefined || folder === "" ? undefined : folder;
}

/** The file in `folder` for the screenshot of the case titled `title`. */
export function screenshotPath(folder, title) {
  const name = title
    .replace(/[^A-Za-z0-9.]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, MAX_NAME_LENGTH);
  return path.join(folder, `${name === "" ? "case" : name}.png`);
}
