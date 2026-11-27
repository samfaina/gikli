# UI Redesign

## Intent

The app's functionality is complete, but its interface doesn't match the quality of what's behind it. This work redesigns the UI so the client looks and feels like a professional, polished desktop Git tool.

## Direction

Two existing clients capture what I want:

- **GitKraken**: repositories open as tabs. Every open repository is a tab, and switching repositories means switching tabs. ([gitkraken.com](https://www.gitkraken.com))
- **GitButler**: a clean interface with rounded shapes and a menu on the left. ([gitbutler.com](https://gitbutler.com) · [GitHub](https://github.com/gitbutlerapp/gitbutler))

Other references, as loose inspiration only, never as rules:

- **Fork**: the look of the log graph: its lines, nodes, branch and tag labels and row density (`refs/fork-history*.png`, `.gif`). Collapsing branches in the graph, which the `.gif` shows, is not part of this work.
- **Sublime Merge**: the diff and resolving Conflicts (`refs/sublime-merge-*.png`).
- **SourceGit**: how the light and the dark theme balance each other (`refs/sourcegit-*.png`).

Screenshots, if present: references in `docs/design/refs/`, the current UI in `docs/design/current/`.

## Scope

- **In:** the UI under `ui/`.
- **Out:** Git functionality and the Rust backend. No new features.
- **Exceptions, and only these:**
  - What the shell and the backend need so that one window shows several Projects as tabs, and nothing more. Today each open Project has a window of its own (ADR-0011).
  - Closing a Project tab while an operation runs in its Project asks first, offering to cancel the operation and close the tab or to keep the tab. Before tabs, closing the last window ended gikli and its git with it; a closed tab would leave the operation running out of sight.
  - A Prompt says which Repository it comes from, or for a clone which folder, so its modal can name the Project over whatever tab is in view. Today a Prompt carries only its kind and text.
  - The keyboard shortcuts of the tabs: Ctrl+Tab, Ctrl+Shift+Tab and Ctrl+W (Cmd+W on macOS).

## Constraints

- Frontend stack stays as is: TypeScript with no framework (direct DOM modules, one file per dialog or view, a single `styles.css`), Vite, CodeMirror 6 for diff and merge, `@tauri-apps/api` for the backend.
- Only free tools that need no account or subscription.
- GitButler is source-available (FSL): visual inspiration only, no copying of code or assets.

## Decisions

Settled while grilling this brief. Terms as in `GLOSSARY.md`; lines marked [visual] go to `DESIGN.md`.

### Window and navigation

- Every open Project is a Project tab of one window (ADR-0011).
- gikli has one window. Opening a Project that is already open brings its tab forward. Tabs do not detach into windows of their own.
- The title bar stays native, and the Project tabs sit right below it.
- The + at the end of the tabs offers Open Folder…, Clone… and New Repository…, and each opens its Project in a new tab.
- With no tab open, the window shows those same three actions as its welcome screen.
- A Project tab shows the name of the Project's folder, and a marker when its Project has uncommitted changes, an Operation in progress or a notification.
- The Navigation bar holds Changes, Log and Stashes at the top, and Operation Log and Settings at the foot. It only switches views.
- The actions of a view (Refresh, Fetch, Pull…, Push…, New Changelist…) sit in the header of the view, beside its title.
- In the Log, the Branches pane sits beside the Navigation bar, at the left of the graph.
- Each Project tab remembers its view and its selection while gikli runs. Nothing of it is saved to disk.
- The Status area shows the Repository in view.
- The Repository selector shows only in the tab of a Project with more than one Repository, or with Repositories found in its folder and not added. It sits left of the view title.
- The Repository selector lists the Project's Repositories, those found and not added (Add, Ignore) and those no longer valid (Remove), and they no longer come as notifications. A count on it tells how many were found.
- Closing a Project tab saves its diff edits first. When they cannot be saved, the tab stays, with the reason on screen.
- Closing a Project tab while an operation runs in its Project asks whether to cancel the operation and close the tab, or keep the tab.
- Closing the window does what closing each of its tabs does.
- Ctrl+Tab and Ctrl+Shift+Tab switch tabs, and Ctrl+W (Cmd+W on macOS) closes the current one.
- The window keeps its current minimum size. Below about 900 px wide, the side columns of a view go below its main column.
- Columns are neither resizable nor collapsible.

### Forms, notifications and operations

- Clone, New, Move and Rename Changelist, Rollback, Commit identity, Prompt, the branch dialogs, Pull and Push open as centred modal dialogs.
- Settings and Operation Log are views of the Navigation bar.
- A notification shows as a banner inside the tab of its Project, under the view header, until the user dismisses it.
- A long operation shows its progress and Cancel in a strip inside the tab of its Project, above the Status area.
- While a long operation runs, its Project tab shows a spinner.
- Clone opens a new tab at once, with its progress there. The tab becomes the cloned Project when the clone ends, or shows the error when it fails.
- A Prompt opens as a modal over whatever tab is in view, with the Project's name in its title.

### Look

- [visual] Light and dark themes, following the system, with no setting to choose.
- [visual] The Navigation bar is a narrow column of icons, about 48 px wide, each with a tooltip.
- [visual] Notification banners are compact and rounded, with an icon.
- [visual] Icons from Lucide, as inline SVG of only the icons used.
- [visual] Inter, bundled with the app, for the interface; the system monospace for code and diffs. Provisional: to be revisited.
- [visual] The accent stays as it is: `#2f6f5e` in light, `#5fb89f` in dark.
- [visual] From Fork, only the look of the log graph.

### Delivery

- The work goes in issues, in this order: (1) tokens and base components, which complete the inventory of tokens and components in `DESIGN.md`; (2) the shell: Project tabs, Navigation bar, Status area, and the exceptions in Scope; (3) the views, one by one: Changes, Log, Diff, Stashes; (4) dialogs and forms.
- After each issue the app stays usable.
- The e2e tests keep every `data-testid` and every assertion. Only the way to reach an element may change.
- The work is done when every screenshot of `docs/design/current/` is taken again in `docs/design/after/`, in both themes and with the same sample data, and the pairs are reviewed side by side.
- The script that takes the screenshots is committed, and `@playwright/test` moves to `devDependencies` with a pinned version.
- Out of this work, each with an issue of its own: a list of recent Projects, reopening the tabs open at the last close, and collapsing branches in the log graph.
