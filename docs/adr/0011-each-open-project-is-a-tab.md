# Each open Project is a tab of one window

gikli shows every open Project as a tab of its window, and switching Projects means switching tabs, as GitKraken does with repositories. A Project almost always holds one Repository, so in practice a tab is a repository; a Project that holds several shows one at a time, chosen in its tab. Before the redesign each open Project had a window of its own (`crates/gikli-core/src/window.rs`), so several open Projects are not new: the tabs bring them into one window. This is the one place where the UI redesign reaches into the shell and the backend, and only to let one window show several Projects.

## Considered Options

- **A tab for each Repository of the one Project a window shows**: keeps the Project of ADR-0004 as the frame of the window, but a Project with one Repository, the usual case, gets a window with a single tab, and switching Projects still means switching windows.
- **One window per Project, as before**: no change to the shell, and not what the redesign asked for.

## Consequences

- A Changelist that spans Repositories (ADR-0005) shows in parts, one for each Repository, as the UI already showed it.
- The Status area shows the Repository in view, not every Repository of the Project.
