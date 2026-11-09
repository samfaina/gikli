// End-to-end smoke test: launches the real gikli app through tauri-driver (WebDriver), with a
// throwaway repository passed on the command line, and checks that the window shows it.
//
// Needs `tauri-driver` (`cargo install tauri-driver --locked`). On Windows, the Microsoft Edge
// WebDriver matching the WebView2 Runtime is downloaded on first run; on Linux, install
// `webkit2gtk-driver` (`webkitgtk-webdriver` from Ubuntu 26.04 on).
// tauri-driver does not support macOS.
//
// With GIKLI_E2E_WEBVIEW2_DEBUG_PORT set, the smoke test runs without tauri-driver; see where the
// mode is chosen below.

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { SevereServiceError } from "webdriverio";

import { attachPort, respondsBefore, waitForDevTools } from "./attach.js";
import { commitStaged, runGit, sandboxGitconfig } from "./sandbox-git.js";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.platform === "win32" ? ".exe" : "";

/** The one uncommitted change in the smoke test's repository, a file git does not track yet. */
const UNTRACKED_FILE = "smoke file.txt";

/**
 * A temporary folder with a git repository holding {@link UNTRACKED_FILE}, isolated from the
 * developer's git configuration.
 */
function createRepository() {
  // The native realpath, as the app canonicalizes the folder it opens: on Windows it expands 8.3
  // short names, such as RUNNER~1 in the runner's temporary folder, which the JavaScript one keeps.
  const sandbox = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "gikli-e2e-")));
  const gitconfig = sandboxGitconfig(sandbox);
  fs.writeFileSync(gitconfig, "[init]\n\tdefaultBranch = main\n");
  const repository = path.join(sandbox, "smoke repo");
  fs.mkdirSync(repository);
  runGit(repository, gitconfig, "init", "--quiet");
  fs.writeFileSync(path.join(repository, UNTRACKED_FILE), "smoke\n");
  installFsmonitor(sandbox, repository, gitconfig);
  installBlockingPostCheckout(sandbox, gitconfig);
  return { sandbox, repository, remote: createRemote(sandbox, gitconfig) };
}

/** The one file in the smoke test's remote. */
const REMOTE_FILE = "remote file.txt";

/** A bare remote in `sandbox` holding {@link REMOTE_FILE} in one commit. */
function createRemote(sandbox, gitconfig) {
  const remote = path.join(sandbox, "remote.git");
  const work = path.join(sandbox, "remote work");
  const git = (cwd, ...args) => runGit(cwd, gitconfig, ...args);
  fs.mkdirSync(remote);
  fs.mkdirSync(work);
  git(remote, "init", "--quiet", "--bare");
  git(work, "init", "--quiet");
  fs.writeFileSync(path.join(work, REMOTE_FILE), "smoke\n");
  git(work, "add", "--all");
  commitStaged(work, gitconfig, "First commit");
  git(work, "push", "--quiet", remote, "main");
  return remote;
}

/**
 * The file system monitor git runs in `git status`, which lets the smoke test step into a read of
 * the changes, the first thing a read of a diff does.
 *
 * Every read holds while `hold-reads` exists in `sandbox`, and adds a line to `held-reads` when it
 * starts to. The hold ends by itself, so a case that fails before it removes the marker leaves no
 * process behind.
 *
 * The next read after `ask-for-prompt` appears in `sandbox` raises a prompt, and leaves the answer
 * in `prompt-answer`. It asks through gikli's askpass, so the test needs no server that wants
 * credentials.
 */
function installFsmonitor(sandbox, repository, gitconfig) {
  const monitor = path.join(sandbox, "fsmonitor");
  const folder = sandbox.replaceAll("\\", "/");
  fs.writeFileSync(
    monitor,
    `#!/bin/sh
if [ -e '${folder}/hold-reads' ]; then
  echo held >> '${folder}/held-reads'
  waited=0
  while [ -e '${folder}/hold-reads' ] && [ "$waited" -lt 600 ]; do
    sleep 0.1
    waited=$((waited + 1))
  done
fi
if [ -e '${folder}/ask-for-prompt' ]; then
  rm '${folder}/ask-for-prompt'
  "$GIT_ASKPASS" 'Username for smoke: ' > '${folder}/prompt-answer'
fi
exit 1
`,
    { mode: 0o755 },
  );
  runGit(repository, gitconfig, "config", "core.fsmonitor", monitor.replaceAll("\\", "/"));
}

/**
 * Makes the next `post-checkout` after `block-post-checkout` appears in `sandbox` block until gikli
 * ends it. Every other run of the hook exits at once. The hook comes from the global configuration
 * because a clone has no repository of its own to hold one before it starts. The block outlasts any
 * case and still ends by itself, so a case that fails before it cancels leaves no process for good.
 */
function installBlockingPostCheckout(sandbox, gitconfig) {
  const hooks = path.join(sandbox, "hooks");
  const folder = sandbox.replaceAll("\\", "/");
  fs.mkdirSync(hooks);
  fs.writeFileSync(
    path.join(hooks, "post-checkout"),
    `#!/bin/sh
if [ -e '${folder}/block-post-checkout' ]; then
  rm '${folder}/block-post-checkout'
  sleep 300
fi
`,
    { mode: 0o755 },
  );
  runGit(sandbox, gitconfig, "config", "--global", "core.hooksPath", hooks.replaceAll("\\", "/"));
}

function targetDirectory() {
  const metadata = spawnSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], {
    cwd: root,
    encoding: "utf8",
  });
  if (metadata.status !== 0) {
    throw new Error(`cargo metadata failed: ${metadata.stderr}`);
  }
  return JSON.parse(metadata.stdout).target_directory;
}

// The launcher and each worker load this file; only the launcher creates the repository, and
// the workers inherit its location through the environment.
if (!process.env.GIKLI_E2E_REPOSITORY) {
  const created = createRepository();
  process.env.GIKLI_E2E_SANDBOX = created.sandbox;
  process.env.GIKLI_E2E_REPOSITORY = created.repository;
  process.env.GIKLI_E2E_UNTRACKED_FILE = UNTRACKED_FILE;
  process.env.GIKLI_E2E_REMOTE = created.remote;
  process.env.GIKLI_E2E_REMOTE_FILE = REMOTE_FILE;
}

/**
 * The installed WebView2 Runtime's version, such as 141.0.3537.71. The app runs on this runtime,
 * not on the Edge browser, and the two can be at different versions, so the Edge WebDriver must
 * match this one.
 */
function webView2Version() {
  const client = String.raw`Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}`;
  const keys = [String.raw`HKLM\SOFTWARE\WOW6432Node`, String.raw`HKLM\SOFTWARE`, String.raw`HKCU\Software`].map(
    (hive) => `${hive}\\${client}`,
  );
  for (const key of keys) {
    const query = spawnSync("reg", ["query", key, "/v", "pv"], { encoding: "utf8" });
    const version = query.stdout?.match(/\bpv\s+REG_SZ\s+(\d+\.\d+\.\d+\.\d+)/)?.[1];
    if (query.status === 0 && version && version !== "0.0.0.0") {
      return version;
    }
  }
  throw new Error("The WebView2 Runtime is not installed; gikli needs it to run on Windows.");
}

/** The Edge WebDriver matching the WebView2 Runtime, downloaded on first use. */
async function edgeDriver() {
  const { download } = await import("edgedriver");
  const version = webView2Version();
  console.log(`WebView2 Runtime ${version}; using the Edge WebDriver for it.`);
  // One folder per version: edgedriver reuses whatever driver its folder holds.
  const cache = path.join(os.tmpdir(), "gikli-msedgedriver", version);
  return download(version, cache);
}

/** Spawn options that send a child's output where the smoke test's goes. */
const inheritOutput = { stdio: ["ignore", process.stdout, process.stderr] };

/** Starts `command`, and resolves with it once it runs; rejects if it cannot start. */
async function start(command, args, options) {
  const child = spawn(command, args, { ...inheritOutput, ...options });
  await new Promise((resolve, reject) => {
    child.once("spawn", resolve);
    child.once("error", reject);
  });
  return child;
}

/** Stops `child`, if it runs, and resolves once it has exited. */
async function stop(child) {
  if (child?.pid !== undefined && child.exitCode === null && child.signalCode === null) {
    const exited = new Promise((resolve) => child.once("exit", resolve));
    child.kill();
    await exited;
  }
}

const application = path.join(targetDirectory(), "debug", `gikli${exe}`);
/** Where the WebDriver the session talks to listens: tauri-driver, or msedgedriver in attach mode. */
const driverPort = 4444;
/**
 * The environment gikli runs in: it opens the smoke test's repository on startup, and the git it
 * runs reads the sandbox's global configuration.
 */
function appEnvironment() {
  return {
    ...process.env,
    GIKLI_PROJECT_FOLDER: process.env.GIKLI_E2E_REPOSITORY,
    GIT_CONFIG_GLOBAL: sandboxGitconfig(process.env.GIKLI_E2E_SANDBOX),
  };
}

// The mode, chosen by GIKLI_E2E_WEBVIEW2_DEBUG_PORT alone. Without it, the smoke test goes through
// tauri-driver, as a developer runs it on Windows and Linux. With it, the config launches gikli
// itself and attaches msedgedriver to the DevTools port WebView2 opened ("attach mode").
//
// Attach mode exists for the GitHub-hosted Windows runner, where WebView2 ignores the WEBVIEW2_*
// variables, so it never gets msedgedriver's --remote-debugging-port and tauri-driver's session
// fails after minutes with "DevToolsActivePort file doesn't exist"; tauri-driver rewrites
// ms:edgeOptions, so no debuggerAddress can go through it. There, a CI step opens a fixed port
// with the AdditionalBrowserArguments policy for gikli.exe and names it in the variable. Nothing
// here writes that policy: it is machine-wide and would leave the port open on a developer's
// machine. See https://github.com/samfaina/gikli/issues/54.
const debugPort = attachPort();
let tauriDriver;
let attached = { app: undefined, driver: undefined };

/**
 * Launches gikli, waits for WebView2's DevTools on {@link debugPort}, and starts msedgedriver on
 * {@link driverPort}. Throws, having stopped what it started, if either does not come up.
 */
async function launchAttached() {
  if (process.platform !== "win32") {
    throw new SevereServiceError("GIKLI_E2E_WEBVIEW2_DEBUG_PORT is for Windows, where gikli runs on WebView2.");
  }
  try {
    const driver = await edgeDriver();
    attached.app = await start(application, [], { env: appEnvironment() });
    await waitForDevTools(debugPort);
    attached.driver = await start(driver, [`--port=${driverPort}`]);
    if (!(await respondsBefore(`http://127.0.0.1:${driverPort}/status`, Date.now() + 30000))) {
      throw new Error(`msedgedriver did not start on port ${driverPort} within 30 s.`);
    }
  } catch (error) {
    await stopAttached();
    throw new SevereServiceError(error.message);
  }
}

/** Stops what {@link launchAttached} started, if anything. */
async function stopAttached() {
  await Promise.all([stop(attached.driver), stop(attached.app)]);
  attached = { app: undefined, driver: undefined };
}

export const config = {
  runner: "local",
  specs: ["./smoke.e2e.js"],
  maxInstances: 1,
  hostname: "127.0.0.1",
  port: driverPort,
  capabilities: [
    {
      maxInstances: 1,
      // tauri-driver speaks classic WebDriver only, and attach mode does too, so the specs run
      // the same way in both modes.
      "wdio:enforceWebDriverClassic": true,
      ...(debugPort === undefined
        ? // The repository reaches the app through GIKLI_PROJECT_FOLDER (see appEnvironment):
          // WebView2's driver hands `args` to the webview, not to the app.
          { "tauri:options": { application } }
        : { browserName: "webview2", "ms:edgeOptions": { debuggerAddress: `127.0.0.1:${debugPort}` } }),
    },
  ],
  logLevel: "warn",
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { ui: "bdd", timeout: 60000 },

  // In attach mode the app and msedgedriver run here, in the launcher, because only an error
  // thrown from onPrepare stops the run: one from beforeSession is logged and the session is
  // attempted anyway.
  onPrepare: async () => {
    if (process.platform === "darwin") {
      throw new Error("tauri-driver does not support macOS; run the smoke test on Windows or Linux.");
    }
    const build = spawnSync("npx", ["tauri", "build", "--debug", "--no-bundle"], {
      cwd: root,
      stdio: "inherit",
      shell: true,
    });
    if (build.status !== 0) {
      // In attach mode the run must stop here, or it would launch an app this build did not make.
      throw new (debugPort === undefined ? Error : SevereServiceError)("building gikli failed");
    }
    if (debugPort !== undefined) {
      await launchAttached();
    }
  },

  beforeSession: async () => {
    if (debugPort !== undefined) {
      return;
    }
    const args = [];
    if (process.platform === "win32") {
      args.push("--native-driver", await edgeDriver());
    }
    const driver = path.join(os.homedir(), ".cargo", "bin", `tauri-driver${exe}`);
    tauriDriver = spawn(fs.existsSync(driver) ? driver : "tauri-driver", args, {
      ...inheritOutput,
      env: appEnvironment(),
    });
    await new Promise((resolve) => setTimeout(resolve, 1000));
  },

  afterSession: () => {
    tauriDriver?.kill();
  },

  onComplete: async () => {
    await stopAttached();
    fs.rmSync(process.env.GIKLI_E2E_SANDBOX, { recursive: true, force: true });
  },
};
