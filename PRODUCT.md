# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Any developer who wants a desktop Git client for the repositories on their own machine. The behavior of the Git integration in JetBrains IDEs is what sets gikli apart, not who it is for: a user does not need to know JetBrains to use it.

## Product Purpose

gikli is a standalone desktop Git client for Windows, macOS and Linux that reproduces the behavior of the Git integration in JetBrains IDEs, with Changelists, the Shelf and partial commits at its core. Its functionality is complete for the current tier; the interface is being redesigned so it looks and feels like a professional, polished desktop Git tool (`docs/design/BRIEF.md`).

## Positioning

The JetBrains way of organizing uncommitted work (Changelists that git does not know about, the Shelf, committing a selection of changes) in a client of its own, outside any IDE, driving the user's own git executable (ADR-0001).

## Operating Context

- A desktop app: a Tauri 2 shell with a web UI in the system webview (WebView2, WKWebView, WebKitGTK). Windows is the main development platform. The design language is gikli's own on every OS, not each platform's native one.
- The user opens a Project, a folder that holds one or more Repositories, usually one. Every open Project is a tab of one window (ADR-0011).
- Daily work: reviewing local changes and diffs, grouping them into Changelists, committing, pulling and pushing, reading the log, handling branches, Conflicts and Operations in progress.
- git, ssh and the programs they start ask the user things (passwords, passphrases, host keys) through gikli while an operation waits.
- Terms are defined in `GLOSSARY.md`; settled behavior in `docs/product-decisions.md`; the behavior reference in `docs/reference/jetbrains-git.md`.

## Capabilities and Constraints

- English interface. Every string lives in `ui/src/strings/en.ts`, so other languages can come later without code changes.
- Light and dark themes that follow the operating system.
- Keyboard shortcuts follow each platform's convention: Ctrl on Windows and Linux, Cmd on macOS.
- Minimum git 2.34. gikli finds git in the settings, then on the PATH, then on Windows inside WSL.
- No telemetry and no automatic error reports.
- Frontend stack fixed: TypeScript with no framework, Vite, CodeMirror 6 for diff and merge, `@tauri-apps/api`. Only free tools that need no account.
- The redesign adds no features beyond the exceptions listed in `docs/design/BRIEF.md`.
- The e2e smoke test relies on every `data-testid` in the UI.

## Brand Commitments

- The name is **gikli**, written in lowercase.
- The current app icon (`src-tauri/icons/icon.svg`) is not binding and may be redesigned with the UI.
- License Apache-2.0.

## Evidence on Hand

- Screenshots of every screen of the current UI, light and dark, in `docs/design/current/` (index in its `README.md`).
- Reference screenshots of other clients in `docs/design/refs/`: GitButler, Fork, SourceGit, Sublime Merge. Inspiration only; GitButler is source-available (FSL), so nothing of its code or assets is copied.
- Sample repositories generated for the screenshots (`acme-web`, `conflict-demo`, a `workspace` Project with three Repositories).
- There are no users, testimonials, metrics or press to show. Nothing of that kind may be invented.

## Product Principles

1. **JetBrains behavior, gikli's own look.** Features behave as the reference says; the layout and design are gikli's, not a copy of the IDE's.
2. **The user's work is never lost by surprise.** Destructive actions show what goes before it goes, and gikli keeps a way back (Backups, Saved local changes, Restore).
3. **Say what git did, in the user's words.** gikli shows the real state of git and explains failures plainly, in the vocabulary of the glossary rather than git's jargon.
4. **Nothing waits out of sight.** What git asks, what is running and what went wrong reach the user in the Project it belongs to.

## Accessibility & Inclusion

- Everything can be done with the keyboard, with a visible focus.
- Text meets WCAG AA contrast in both the light and the dark theme.
- No formal audit is required.
