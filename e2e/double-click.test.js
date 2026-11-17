// Tests of the smoke test's double-click on a change row (issue #81). They run the real WebdriverIO
// against a stand-in WebDriver server, because the failure lies in what WebdriverIO sends.

import assert from "node:assert/strict";
import http from "node:http";
import { describe, it } from "node:test";

import { doubleClickUntilDisplayed } from "./double-click.js";
import { bringIntoViewBeforePointerActions } from "./into-view.js";
import { connect, ELEMENT_KEY, readJson, reply } from "./webdriver-stand-in.js";

const ROW = '[data-testid="change"][data-path="smoke file.txt"]';
const DIFF = '[data-testid="diff"]';
/** Quick enough for a test: the double-click that opens the diff shows it at once here. */
const QUICK_RETRIES = { attempts: 3, timeout: 200 };
const STALE = { error: "stale element reference", message: "stale element reference" };

/**
 * A WebDriver server showing one change row and a diff view, closed until a double-click on the
 * row's current element reaches it.
 *
 * - `rebuildsAfterFind`: how many times finding the row is followed by a rebuild of the changes
 *   view, which gives the row a new element id and leaves the one just handed out stale.
 * - `lostDoubleClicks`: how many double-clicks on the current element leave the diff closed.
 * - `failure`: the WebDriver error every double-click gets, as `{ error, message }`.
 * - `opensAfter`: how many ms the diff takes to open after the double-click that opens it.
 *
 * `doubleClicks` holds the element id of each double-click received, in order, and `targeted` that
 * of each double-click and each scroll into view before one.
 */
function changesViewServer({ rebuildsAfterFind = 0, lostDoubleClicks = 0, failure, opensAfter = 0 } = {}) {
  const state = { doubleClicks: [], targeted: [], rebuilds: rebuildsAfterFind, lost: lostDoubleClicks, row: 1, diffOpen: false };

  const find = (selector) => {
    const id = selector === ROW ? `row-${state.row}` : "diff";
    if (selector === ROW && state.rebuilds > 0) {
      state.rebuilds -= 1;
      state.row += 1;
    }
    return { [ELEMENT_KEY]: id };
  };

  const server = http.createServer(async (request, response) => {
    const body = await readJson(request);
    const route = `${request.method} ${request.url.replace("/session/mock", "")}`;
    if (route === "POST /session") {
      reply(response, { sessionId: "mock", capabilities: { browserName: "mock" } });
    } else if (route === "GET /window") {
      reply(response, "window");
    } else if (route === "POST /element") {
      reply(response, find(body.value));
    } else if (route === "POST /elements") {
      reply(response, [find(body.value)]);
    } else if (/^GET \/element\/[^/]+\/name$/.test(route)) {
      // WebdriverIO asks for the tag to learn whether an element it holds is still there.
      const id = route.split("/")[2];
      if (id === "diff" || id === `row-${state.row}`) {
        reply(response, "li");
      } else {
        reply(response, STALE, 404);
      }
    } else if (route === "POST /actions") {
      const id = body.actions[0].actions.find((action) => action.type === "pointerMove").origin[ELEMENT_KEY];
      state.doubleClicks.push(id);
      state.targeted.push(id);
      if (failure !== undefined) {
        reply(response, failure, 500);
      } else if (id !== `row-${state.row}`) {
        reply(response, STALE, 404);
      } else {
        if (state.lost > 0) {
          state.lost -= 1;
        } else {
          setTimeout(() => (state.diffOpen = true), opensAfter);
        }
        reply(response, null);
      }
    } else if (route === "POST /execute/sync" && body.script.includes("scrollIntoView")) {
      // The scroll into view before a double-click, which meets a stale row as the double-click would.
      const id = body.args[0][ELEMENT_KEY];
      state.targeted.push(id);
      if (id === `row-${state.row}`) {
        reply(response, null);
      } else {
        reply(response, STALE, 404);
      }
    } else if (route === "POST /execute/sync") {
      // WebdriverIO's isDisplayed runs its checks as scripts.
      reply(response, state.diffOpen);
    } else if (request.method === "DELETE") {
      reply(response, null);
    } else {
      reply(response, { error: "unknown command", message: route }, 404);
    }
  });

  return { server, state };
}

/**
 * A session on `server` that brings an element into view before a pointer action on it, as the
 * smoke test's does.
 */
async function connectAsTheSmokeTest(t, server) {
  const browser = await connect(t, server);
  bringIntoViewBeforePointerActions(browser);
  return browser;
}

describe("doubleClickUntilDisplayed", () => {
  it("double-clicks a row found afresh when the changes view is rebuilt after the row was found", async (t) => {
    const { server, state } = changesViewServer({ rebuildsAfterFind: 1 });
    const browser = await connectAsTheSmokeTest(t, server);

    const diff = await doubleClickUntilDisplayed(browser, ROW, DIFF, QUICK_RETRIES);

    assert.equal(diff.elementId, "diff");
    assert.equal(state.diffOpen, true);
    assert.equal(state.targeted.at(0), "row-1");
    assert.equal(state.doubleClicks.at(-1), "row-2");
  });

  it("stops with the stale element error when the row goes stale on every attempt", async (t) => {
    const { server, state } = changesViewServer({ rebuildsAfterFind: Infinity });
    const browser = await connectAsTheSmokeTest(t, server);

    await assert.rejects(doubleClickUntilDisplayed(browser, ROW, DIFF, QUICK_RETRIES), { name: "stale element reference" });

    // WebdriverIO repeats each attempt with the same id, so an attempt is a distinct id.
    assert.equal(new Set(state.targeted).size, QUICK_RETRIES.attempts);
    assert.equal(state.diffOpen, false);
  });

  it("does not retry an error other than a stale element", async (t) => {
    const failure = { error: "move target out of bounds", message: "the row is off screen" };
    const { server, state } = changesViewServer({ failure });
    const browser = await connectAsTheSmokeTest(t, server);

    await assert.rejects(doubleClickUntilDisplayed(browser, ROW, DIFF, QUICK_RETRIES), { name: failure.error });

    assert.equal(state.doubleClicks.length, 1);
  });

  it("double-clicks again when a double-click left the diff closed, and only then", async (t) => {
    const { server, state } = changesViewServer({ lostDoubleClicks: 1 });
    const browser = await connectAsTheSmokeTest(t, server);

    await doubleClickUntilDisplayed(browser, ROW, DIFF, QUICK_RETRIES);

    assert.equal(state.diffOpen, true);
    assert.equal(state.doubleClicks.length, 2);
  });

  it("waits for a diff that is slow to open without double-clicking again", async (t) => {
    const { server, state } = changesViewServer({ opensAfter: QUICK_RETRIES.timeout / 2 });
    const browser = await connectAsTheSmokeTest(t, server);

    await doubleClickUntilDisplayed(browser, ROW, DIFF, QUICK_RETRIES);

    assert.equal(state.doubleClicks.length, 1);
  });

  it("fails, naming the diff view, when no double-click opens it", async (t) => {
    const { server, state } = changesViewServer({ lostDoubleClicks: Infinity });
    const browser = await connectAsTheSmokeTest(t, server);

    await assert.rejects(doubleClickUntilDisplayed(browser, ROW, DIFF, QUICK_RETRIES), (error) => {
      assert.ok(error.message.includes(DIFF), error.message);
      return true;
    });

    assert.equal(state.doubleClicks.length, QUICK_RETRIES.attempts);
  });
});
