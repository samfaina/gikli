// Tests of the smoke test's attach mode, which runs on the Windows CI jobs (issue #54).

import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import http from "node:http";
import { describe, it } from "node:test";

import { attachPort, launchWithDevTools, waitForDevTools } from "./attach.js";

/** A process that stands in for gikli: Node running `code`, stopped when the test ends. */
async function standIn(t, code) {
  const child = spawn(process.execPath, ["-e", code], { stdio: "ignore" });
  await once(child, "spawn");
  t.after(() => child.kill());
  return child;
}

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
  it("fails, naming the port, how long it waited, and the policy as one possible cause, when nothing answers in time", async () => {
    const port = await freePort();
    const started = Date.now();

    await assert.rejects(waitForDevTools(port, { timeout: 1000 }), (error) => {
      assert.match(error.message, new RegExp(`\\b${port}\\b`));
      assert.match(error.message, /within [12](\.\d)? s\b/);
      assert.match(error.message, /AdditionalBrowserArguments/);
      assert.match(error.message, /gikli\.exe/);
      assert.match(error.message, /one possible cause/i);
      return true;
    });
    assert.ok(Date.now() - started < 5000, "expected it to give up soon after the timeout");
  });

  it("says gikli was still running when it gave up", async (t) => {
    const port = await freePort();
    const app = await standIn(t, "setInterval(() => {}, 1000)");

    await assert.rejects(waitForDevTools(port, { timeout: 1000, app }), (error) => {
      assert.match(error.message, new RegExp(`gikli was still running \\(process ${app.pid}\\)`));
      return true;
    });
  });

  it("says gikli had exited, and with which code, and gives up then rather than at the timeout", async (t) => {
    const port = await freePort();
    const app = await standIn(t, "setTimeout(() => process.exit(3), 300)");
    const started = Date.now();

    await assert.rejects(waitForDevTools(port, { timeout: 20000, app }), (error) => {
      assert.match(error.message, /gikli had exited with code 3/);
      assert.doesNotMatch(error.message, /still running/);
      return true;
    });
    assert.ok(Date.now() - started < 5000, "expected it to give up soon after gikli exited");
  });

  it("says by which signal gikli was ended", async (t) => {
    const port = await freePort();
    const app = await standIn(t, "setInterval(() => {}, 1000)");
    app.kill("SIGTERM");
    await once(app, "exit");

    await assert.rejects(waitForDevTools(port, { timeout: 20000, app }), (error) => {
      assert.match(error.message, /gikli had exited by signal SIGTERM/);
      return true;
    });
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

describe("launchWithDevTools", () => {
  /** A stand-in for a gikli whose WebView2 opens DevTools on `port`. */
  const answering = (port) =>
    "require('node:http').createServer((request, response) => response.end('{}'))" +
    `.listen(${port}, '127.0.0.1')`;
  const silent = "setInterval(() => {}, 1000)";

  it("launches gikli again when its DevTools do not answer, having stopped the launch that failed", async (t) => {
    const port = await freePort();
    const launched = [];
    const stopped = [];
    const launch = async () => {
      const app = await standIn(t, launched.length === 0 ? silent : answering(port));
      launched.push(app);
      return app;
    };
    const stop = async (app) => {
      stopped.push(app);
      app.kill();
      await once(app, "exit");
    };

    const app = await launchWithDevTools(port, { launch, stop, attempts: 3, timeout: 1000 });

    assert.equal(launched.length, 2);
    assert.equal(app, launched[1]);
    assert.deepEqual(stopped, [launched[0]]);
  });

  it("gives up after the last launch, having stopped every one, with what each saw", async (t) => {
    const port = await freePort();
    const launched = [];
    const stopped = [];
    const launch = async () => {
      const app = await standIn(t, silent);
      launched.push(app);
      return app;
    };
    const stop = async (app) => {
      stopped.push(app);
      app.kill();
      await once(app, "exit");
    };

    await assert.rejects(launchWithDevTools(port, { launch, stop, attempts: 2, timeout: 500 }), (error) => {
      assert.match(error.message, /launch 1 of 2/i);
      assert.match(error.message, /launch 2 of 2/i);
      assert.match(error.message, new RegExp(`still running \\(process ${launched[0].pid}\\)`));
      assert.match(error.message, new RegExp(`still running \\(process ${launched[1].pid}\\)`));
      return true;
    });
    assert.deepEqual(stopped, launched);
  });
});
