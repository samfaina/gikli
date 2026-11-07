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
 * Whether `url` answers with a success before `deadline` (a time in ms), polling until it does.
 */
export async function respondsBefore(url, deadline) {
  while (Date.now() < deadline) {
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

/**
 * Resolves once WebView2's DevTools answer on `port`, and fails after `timeout` ms with an error
 * that points at the policy that opens the port.
 */
export async function waitForDevTools(port, { timeout = 30000 } = {}) {
  if (!(await respondsBefore(`http://127.0.0.1:${port}/json/version`, Date.now() + timeout))) {
    throw new Error(
      `WebView2's DevTools did not answer on port ${port} within ${timeout / 1000} s of launching gikli. ` +
        String.raw`Is the HKLM\SOFTWARE\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments policy ` +
        `set for gikli.exe to --remote-debugging-port=${port}?`,
    );
  }
}
