// Tests of the smoke test's pointer actions on an element below the window's visible area (issue
// #137). The stand-in WebDriver server refuses them as WebKitGTK's does on Linux, which does not
// scroll an element into view before a pointer action.

import assert from "node:assert/strict";
import http from "node:http";
import { describe, it } from "node:test";

import { bringIntoViewBeforePointerActions } from "./into-view.js";
import { connect, ELEMENT_KEY, readJson, reply } from "./webdriver-stand-in.js";

const OUT_OF_BOUNDS = { error: "move target out of bounds", message: "move target out of bounds" };
const STALE = { error: "stale element reference", message: "stale element reference" };

/**
 * A WebDriver server whose page holds an element for each selector, every one below the visible
 * area until a script scrolls it into view. A pointer action or a click on an element out of view
 * fails with "move target out of bounds".
 *
 * `rebuildsAfterFind` is how many times finding an element is followed by a rebuild of the page,
 * which gives every element a new id, the selector and the number of the rebuild, and leaves the
 * ids handed out before it stale.
 *
 * `received` holds, in order, each click and each pointer action that reached the page, as
 * `{ command, targets }`, the ids of the elements it moved to, and `refused` each one refused for
 * an element out of view.
 */
function pageBelowTheFoldServer({ rebuildsAfterFind = 0 } = {}) {
  const state = { inView: new Set(), received: [], refused: [], rebuilds: 0 };
  const idOf = (selector) => (state.rebuilds === 0 ? selector : `${selector}-${state.rebuilds}`);
  const isCurrent = (id) => id === idOf(id.replace(/-\d+$/, ""));
  const find = (selector) => {
    const id = idOf(selector);
    if (state.rebuilds < rebuildsAfterFind) {
      state.rebuilds += 1;
    }
    return { [ELEMENT_KEY]: id };
  };
  const perform = (command, targets, response) => {
    if (!targets.every(isCurrent)) {
      reply(response, STALE, 404);
    } else if (!targets.every((id) => state.inView.has(id))) {
      state.refused.push({ command, targets });
      reply(response, OUT_OF_BOUNDS, 500);
      return;
    }
    state.received.push({ command, targets });
    reply(response, null);
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
      if (isCurrent(route.split("/")[2])) {
        reply(response, "li");
      } else {
        reply(response, STALE, 404);
      }
    } else if (/^POST \/element\/[^/]+\/click$/.test(route)) {
      perform("click", [route.split("/")[2]], response);
    } else if (route === "POST /actions") {
      const { actions } = body.actions[0];
      const targets = actions.map((action) => action.origin?.[ELEMENT_KEY]).filter((id) => id !== undefined);
      perform(actions.map((action) => action.type).join(","), targets, response);
    } else if (route === "DELETE /actions") {
      reply(response, null);
    } else if (route === "POST /execute/sync") {
      const element = body.args[0]?.[ELEMENT_KEY];
      if (element !== undefined && !isCurrent(element)) {
        reply(response, STALE, 404);
        return;
      }
      if (element !== undefined && body.script.includes("scrollIntoView")) {
        state.inView.add(element);
      }
      reply(response, null);
    } else if (request.method === "DELETE") {
      reply(response, null);
    } else {
      reply(response, { error: "unknown command", message: route }, 404);
    }
  });

  return { server, state };
}

describe("bringIntoViewBeforePointerActions", () => {
  it("double-clicks an element that starts below the visible area", async (t) => {
    const { server, state } = pageBelowTheFoldServer();
    const browser = await connect(t, server);
    bringIntoViewBeforePointerActions(browser);

    await (await browser.$("row")).doubleClick();

    assert.deepEqual(state.refused, []);
    assert.equal(state.received.length, 1);
    assert.deepEqual(state.received[0].targets, ["row"]);
  });

  it("clicks an element that starts below the visible area", async (t) => {
    const { server, state } = pageBelowTheFoldServer();
    const browser = await connect(t, server);
    bringIntoViewBeforePointerActions(browser);

    await (await browser.$("button")).click();

    assert.deepEqual(state.refused, []);
    assert.deepEqual(state.received, [{ command: "click", targets: ["button"] }]);
  });

  it("clicks an element that starts below the visible area of a page rebuilt after it was found", async (t) => {
    const { server, state } = pageBelowTheFoldServer({ rebuildsAfterFind: 1 });
    const browser = await connect(t, server);
    bringIntoViewBeforePointerActions(browser);

    await (await browser.$("button")).click();

    assert.deepEqual(state.refused, []);
    assert.deepEqual(state.received, [{ command: "click", targets: ["button-1"] }]);
  });

  it("right-clicks an element that starts below the visible area", async (t) => {
    const { server, state } = pageBelowTheFoldServer();
    const browser = await connect(t, server);
    bringIntoViewBeforePointerActions(browser);

    await (await browser.$("row")).click({ button: "right" });

    assert.deepEqual(state.refused, []);
    assert.equal(state.received.length, 1);
    assert.deepEqual(state.received[0].targets, ["row"]);
  });

  it("drags an element to another, both starting below the visible area", async (t) => {
    const { server, state } = pageBelowTheFoldServer();
    const browser = await connect(t, server);
    bringIntoViewBeforePointerActions(browser);

    await (await browser.$("row")).dragAndDrop(await browser.$("list"));

    assert.deepEqual(state.refused, []);
    assert.equal(state.received.length, 1);
    assert.deepEqual(state.received[0].targets, ["row", "list"]);
  });

  it("drags an element to another given as the query that finds it", async (t) => {
    const { server, state } = pageBelowTheFoldServer();
    const browser = await connect(t, server);
    bringIntoViewBeforePointerActions(browser);

    await (await browser.$("row")).dragAndDrop(browser.$("list"));

    assert.deepEqual(state.refused, []);
    assert.deepEqual(state.received[0].targets, ["row", "list"]);
  });
});
