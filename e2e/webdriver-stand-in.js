// What the tests of the smoke test's helpers share to run the real WebdriverIO against a stand-in
// WebDriver server, which plays the part of a driver in the case under test.

import { remote } from "webdriverio";

/** The key WebDriver names an element by in what it sends and receives. */
export const ELEMENT_KEY = "element-6066-11e4-a52e-4f735466cecf";

/** The JSON body of `request`, or `{}` when it has none. */
export async function readJson(request) {
  const chunks = [];
  for await (const chunk of request) {
    chunks.push(chunk);
  }
  return chunks.length === 0 ? {} : JSON.parse(Buffer.concat(chunks).toString());
}

/** Answers a WebDriver command with `value`, an error when `status` says so. */
export function reply(response, value, status = 200) {
  response.writeHead(status, { "content-type": "application/json" }).end(JSON.stringify({ value }));
}

/** Starts `server` and a WebdriverIO session on it, both ended when the test `t` ends. */
export async function connect(t, server) {
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
