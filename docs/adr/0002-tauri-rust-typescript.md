# Tauri 2 with a Rust core and a TypeScript UI

gikli is a Tauri 2 application: the core is in Rust and runs and parses git, and the UI is in TypeScript and uses CodeMirror 6 for the diff and merge editors. It targets Windows, macOS and Linux, with Windows as the main development platform. This gives a small, fast-starting binary, a mature editor component for the editable diff and the 3-pane merge, and fast parsing of large git output for the log.

## Considered Options

- **Electron + TypeScript**: the most mature option, but heavier in memory and binary size, with no gain for a client that mostly runs git and renders text.
- **Compose Multiplatform Desktop (Kotlin)**: fits existing Compose experience, but has no mature code editor component, so the diff and merge editors would have to be built from scratch.
- **.NET + Avalonia**: viable, with the same missing editor component.
