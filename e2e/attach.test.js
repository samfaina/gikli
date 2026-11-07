// Tests of the smoke test's attach mode, which runs on the Windows CI jobs (issue #54).

import assert from "node:assert/strict";
import http from "node:http";
import { describe, it } from "node:test";

import { attachPort, waitForDevTools } from "./attach.js";

/** A port nothing listens on: one the system handed out and took back. */
async function freePort() {
  const server = http.createServer();
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  await new Promise((resolve) => server.close(resolve));
  return port;
}

describe("attachPort", () => {
  it("is the port in GIKLI_E2E_WEBVIEW2_DEBUG_PORT", () => {
    assert.equal(attachPort({ GIKLI_E2E_WEBVIEW2_DEBUG_PORT: "9222" }), 9222);
  });

  it("is absent without GIKLI_E2E_WEBVIEW2_DEBUG_PORT, on CI too, so the run goes through tauri-driver", () => {
    assert.equal(attachPort({ CI: "true" }), undefined);
    assert.equal(attachPort({ GIKLI_E2E_WEBVIEW2_DEBUG_PORT: "" }), undefined);
  });

  it("rejects a value that is not a port", () => {
    for (const value of ["abc", "0", "65536", "92.22"]) {
      assert.throws(() => attachPort({ GIKLI_E2E_WEBVIEW2_DEBUG_PORT: value }), /GIKLI_E2E_WEBVIEW2_DEBUG_PORT/);
    }
  });
});

describe("waitForDevTools", () => {
  it("fails, naming the port and the policy, when nothing answers in time", async () => {
    const port = await freePort();
    const started = Date.now();

    await assert.rejects(waitForDevTools(port, { timeout: 1000 }), (error) => {
      assert.match(error.message, new RegExp(`\\b${port}\\b`));
      assert.match(error.message, /AdditionalBrowserArguments/);
      assert.match(error.message, /gikli\.exe/);
      return true;
    });
    assert.ok(Date.now() - started < 5000, "expected it to give up soon after the timeout");
  });

  it("resolves once DevTools answer, even when they start listening after the wait began", async (t) => {
    const port = await freePort();
    const devTools = http.createServer((request, response) => {
      response.writeHead(request.url === "/json/version" ? 200 : 404).end("{}");
    });
    t.after(() => devTools.close());
    setTimeout(() => devTools.listen(port, "127.0.0.1"), 500);

    await waitForDevTools(port, { timeout: 10000 });
  });
});
