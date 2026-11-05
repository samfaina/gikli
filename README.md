# gikli

A standalone desktop Git client for Windows, macOS and Linux. See `GLOSSARY.md` for the domain
terms, `docs/adr/` for the architecture and `docs/product-decisions.md` for settled behavior.

## Layout

- `crates/gikli-core`: the core. It runs the user's git (ADR-0001) and exposes the application
  interface, `Gikli`: the commands the UI sends and the events it listens to.
- `crates/gikli-acceptance`: the test harness and the tests. Tests drive `Gikli` and check every
  outcome a second way with plain git, in throwaway repositories and local bare remotes. The
  real git runs in every test except the installation checks, which put a stand-in git
  (`fake-git`) on the PATH to play a version the real one cannot.
- `src-tauri`: the Tauri 2 shell. Each core command is a Tauri command that forwards to `Gikli`,
  and core events reach the UI as `gikli://event`. The shell adds only `startup_folder`, the
  folder to open at launch.
- `ui`: the TypeScript UI. Every string it shows is in `ui/src/strings/en.ts`, and the theme
  follows the operating system's light or dark setting.
- `e2e`: the end-to-end smoke test, which launches the app through WebDriver.

## Requirements

- Rust (stable) and Node.js 20 or newer.
- git 2.34 or newer on the PATH.
- The Tauri prerequisites for your platform (<https://v2.tauri.app/start/prerequisites/>):
  WebView2 on Windows (preinstalled on Windows 11), Xcode Command Line Tools on macOS, and on
  Linux `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev` and `libayatana-appindicator3-dev`
  or their equivalents.

## Commands

```sh
npm install
npm run dev            # run the app with the UI hot-reloading
npm run build          # build the app for this platform
cargo test --workspace # core tests through the application interface
npm run typecheck      # typecheck the UI
npm run test:e2e       # smoke test: builds the app, launches it, opens a Project
npm test               # all of the above checks
```

The smoke test needs `tauri-driver` (`cargo install tauri-driver --locked`). On Windows it
downloads the Microsoft Edge WebDriver that matches the installed WebView2 Runtime on first run. On Linux it
needs `webkit2gtk-driver` (WebKitWebDriver). tauri-driver does not support macOS, so there the
smoke test does not run.

CI (`.github/workflows/ci.yml`) builds the app and runs these checks on every pull request
against `main` and every push to `main`, on Windows, macOS and Linux, and once more on Linux
against git 2.34, the oldest supported version. The smoke test runs only on Linux:
the macOS and Windows jobs report it as skipped (on Windows because of #54).

To open a folder at startup, pass it as the first argument (`gikli <folder>`) or set
`GIKLI_PROJECT_FOLDER`.
