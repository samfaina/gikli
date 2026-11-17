// Tests of the smoke test's pointer actions on an element below the window's visible area (issue
// #137). The stand-in WebDriver server refuses them as WebKitGTK's does on Linux, which does not
// scroll an element into view before a pointer action.

import assert from "node:assert/strict";
import http from "node:http";
import { describe, it } from "node:test";
import { remote } from "webdriverio";

import { bringIntoViewBeforePointerActions } from "./into-view.js";

const ELEMENT_KEY = "element-6066-11e4-a52e-4f735466cecf";
const OUT_OF_BOUNDS = { error: "move target out of bounds", message: "move target out of bounds" };

async function readJson(request) {
  const chunks = [];
  for await (const chunk of request) {
    chunks.push(chunk);
  }
  return chunks.length === 0 ? {} : JSON.parse(Buffer.concat(chunks).toString());
}

function reply(response, value, status = 200) {
  response.writeHead(status, { "content-type": "application/json" }).end(JSON.stringify({ value }));
}

/**
 * A WebDriver server whose page holds an element for each selector, every one below the visible
 * area until a script scrolls it into view. A pointer action or a click on an element out of view
 * fails with "move target out of bounds".
 *
 * `received` holds, in order, each click and each pointer action that reached the page, as
 * `{ command, targets }`, the ids of the elements it moved to, and `refused` each one refused for
 * an element out of view.
 */
function pageBelowTheFoldServer() {
  const state = { inView: new Set(), received: [], refused: [] };
  const perform = (command, targets, response) => {
    if (!targets.every((id) => state.inView.has(id))) {
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
      reply(response, { [ELEMENT_KEY]: body.value });
    } else if (/^GET \/element\/[^/]+\/name$/.test(route)) {
      reply(response, "li");
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

/** Starts `server` and a WebdriverIO session on it, both ended when the test `t` ends. */
async function connect(t, server) {
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => {
    server.closeAllConnections();
    server.close();
  });
  return remote({
    hostname: "127.0.0.1",
    port: server.address().port,
    path: "/",
    logLevel: "silent",
    connectionRetryCount: 0,
    capabilities: { browserName: "mock", "wdio:enforceWebDriverClassic": true },
  });
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
});
