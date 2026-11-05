// End-to-end smoke test: launches the real gikli app through tauri-driver (WebDriver), with a
// throwaway repository passed on the command line, and checks that the window shows it.
//
// Needs `tauri-driver` (`cargo install tauri-driver --locked`). On Windows, the Microsoft Edge
// WebDriver matching the WebView2 Runtime is downloaded on first run; on Linux, install
// `webkit2gtk-driver` (`webkitgtk-webdriver` from Ubuntu 26.04 on).
// tauri-driver does not support macOS.

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.platform === "win32" ? ".exe" : "";

/** A temporary folder with a git repository, isolated from the developer's git configuration. */
function createRepository() {
  const sandbox = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), "gikli-e2e-")));
  const gitconfig = path.join(sandbox, ".gitconfig");
  fs.writeFileSync(gitconfig, "[init]\n\tdefaultBranch = main\n");
  const repository = path.join(sandbox, "smoke repo");
  fs.mkdirSync(repository);
  const git = spawnSync("git", ["init", "--quiet"], {
    cwd: repository,
    env: { ...process.env, GIT_CONFIG_GLOBAL: gitconfig, GIT_CONFIG_NOSYSTEM: "1" },
  });
  if (git.status !== 0) {
    throw new Error(`git init failed: ${git.stderr}`);
  }
  return { sandbox, repository };
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

const application = path.join(targetDirectory(), "debug", `gikli${exe}`);
let tauriDriver;

export const config = {
  runner: "local",
  specs: ["./smoke.e2e.js"],
  maxInstances: 1,
  hostname: "127.0.0.1",
  port: 4444,
  capabilities: [
    {
      maxInstances: 1,
      // tauri-driver speaks classic WebDriver only.
      "wdio:enforceWebDriverClassic": true,
      // The repository reaches the app through GIKLI_PROJECT_FOLDER (see beforeSession):
      // WebView2's driver hands `args` to the webview, not to the app.
      "tauri:options": { application },
    },
  ],
  logLevel: "warn",
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { ui: "bdd", timeout: 60000 },

  onPrepare: () => {
    if (process.platform === "darwin") {
      throw new Error("tauri-driver does not support macOS; run the smoke test on Windows or Linux.");
    }
    const build = spawnSync("npx", ["tauri", "build", "--debug", "--no-bundle"], {
      cwd: root,
      stdio: "inherit",
      shell: true,
    });
    if (build.status !== 0) {
      throw new Error("building gikli failed");
    }
  },

  beforeSession: async () => {
    const args = [];
    if (process.platform === "win32") {
      const { download } = await import("edgedriver");
      const version = webView2Version();
      console.log(`WebView2 Runtime ${version}; using the Edge WebDriver for it.`);
      // One folder per version: edgedriver reuses whatever driver its folder holds.
      const cache = path.join(os.tmpdir(), "gikli-msedgedriver", version);
      args.push("--native-driver", await download(version, cache));
    }
    const driver = path.join(os.homedir(), ".cargo", "bin", `tauri-driver${exe}`);
    tauriDriver = spawn(fs.existsSync(driver) ? driver : "tauri-driver", args, {
      stdio: ["ignore", process.stdout, process.stderr],
      env: { ...process.env, GIKLI_PROJECT_FOLDER: process.env.GIKLI_E2E_REPOSITORY },
    });
    await new Promise((resolve) => setTimeout(resolve, 1000));
  },

  afterSession: () => {
    tauriDriver?.kill();
  },

  onComplete: () => {
    fs.rmSync(process.env.GIKLI_E2E_SANDBOX, { recursive: true, force: true });
  },
};
