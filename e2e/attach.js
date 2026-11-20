// The smoke test's attach mode, which the Windows CI jobs use instead of tauri-driver (issue #54).
// See the comment where e2e/wdio.conf.js chooses the mode.

/**
 * The port WebView2 opens its DevTools on, from GIKLI_E2E_WEBVIEW2_DEBUG_PORT, or undefined when
 * the variable is not set and the smoke test goes through tauri-driver.
 */
export function attachPort(env = process.env) {
  const value = env.GIKLI_E2E_WEBVIEW2_DEBUG_PORT;
  if (!value) {
    return undefined;
  }
  const port = Number(value);
  if (!/^\d+$/.test(value) || port < 1 || port > 65535) {
    throw new Error(`GIKLI_E2E_WEBVIEW2_DEBUG_PORT is ${JSON.stringify(value)}, not a port number.`);
  }
  return port;
}

/**
 * Whether `url` answers with a success before `deadline` (a time in ms), polling until it does or
 * until `stopped()` says to give up.
 */
export async function respondsBefore(url, deadline, stopped = () => false) {
  while (Date.now() < deadline && !stopped()) {
    try {
      const response = await fetch(url, { signal: AbortSignal.timeout(Math.max(1, deadline - Date.now())) });
      if (response.ok) {
        return true;
      }
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  return false;
}

/** Whether `child`, a child process, has exited. */
export function hasExited(child) {
  return child.exitCode !== null || child.signalCode !== null;
}

/**
 * Resolves once WebView2's DevTools answer on `port`. Fails after `timeout` ms, or as soon as
 * `app`, the gikli process expected to open them, exits, with an error that says how long the wait
 * lasted and what became of `app`.
 */
export async function waitForDevTools(port, { timeout = 30000, app } = {}) {
  const started = Date.now();
  const exited = () => app !== undefined && hasExited(app);
  if (await respondsBefore(`http://127.0.0.1:${port}/json/version`, started + timeout, exited)) {
    return;
  }
  const waited = `${Number(((Date.now() - started) / 1000).toFixed(1))} s`;
  const policy =
    String.raw`One possible cause: the HKLM\SOFTWARE\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments ` +
    `policy does not set --remote-debugging-port=${port} for gikli.exe.`;
  if (app === undefined) {
    throw new Error(`WebView2's DevTools did not answer on port ${port} within ${waited} of launching gikli. ${policy}`);
  }
  if (exited()) {
    const how = app.signalCode === null ? `with code ${app.exitCode}` : `by signal ${app.signalCode}`;
    throw new Error(
      `WebView2's DevTools did not answer on port ${port}: gikli had exited ${how} ${waited} after its launch.`,
    );
  }
  throw new Error(
    `WebView2's DevTools did not answer on port ${port} within ${waited} of launching gikli, and gikli was ` +
      `still running (process ${app.pid}). ${policy}`,
  );
}

/**
 * Launches gikli with `launch()` until its WebView2's DevTools answer on `port`, at most
 * `attempts` times, each waiting up to `timeout` ms (see {@link waitForDevTools}), and resolves with
 * the gikli whose DevTools answered. Each launch that fails is stopped with `stop(app)` before the
 * next. Fails, with what each launch saw, once the last one has failed too.
 */
export async function launchWithDevTools(port, { launch, stop, attempts, timeout }) {
  const failures = [];
  for (let attempt = 1; attempt <= attempts; attempt++) {
    const app = await launch();
    try {
      await waitForDevTools(port, { timeout, app });
      return app;
    } catch (error) {
      await stop(app);
      failures.push(`Launch ${attempt} of ${attempts}: ${error.message}`);
      if (attempt < attempts) {
        console.warn(`${failures.at(-1)} Launching gikli again.`);
      }
    }
  }
  throw new Error(failures.join("\n"));
}
