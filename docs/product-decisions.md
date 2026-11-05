# Product decisions

Decisions that complete or settle the [product reference](reference/jetbrains-git.md). Architectural decisions with real trade-offs live in [adr/](adr/), and terms live in [GLOSSARY.md](../GLOSSARY.md).

## Settled

### Platform and language

- Windows, macOS and Linux from the first release, WSL repositories included. Windows is the main development platform ([ADR-0002](adr/0002-tauri-rust-typescript.md)).
- English interface at launch. Every string is externalized so other languages can be added without code changes.
- Minimum git version 2.34. Below it, gikli warns and does not run operations. Features that need a newer git (for example `rebase --update-refs`, 2.38) are disabled with an explanation.
- Light and dark themes that follow the operating system. gikli's own design and keyboard shortcuts, following each platform's conventions (Ctrl on Windows and Linux, Cmd on macOS), without copying the JetBrains layout beyond what a feature needs.
- One window per Project, with several windows open at once.
- No telemetry and no automatic error reports. The user exports a report by hand.
- License: Apache-2.0.
- Distribution through GitHub Releases: `.msi`/`.exe` on Windows, `.dmg` on macOS, AppImage and `.deb` on Linux, with the Tauri updater. Code signing on Windows and macOS before the first public release; development builds are unsigned.

### Saved local changes and the Shelf

- In Scope 1, Saved local changes use a stash. When the Shelf ships in Scope 2, the default method becomes Shelve, as in JetBrains.
- When gikli saves local changes, it also records which Changelist each change belonged to and restores those assignments when it puts the changes back.
- The Shelf lives in `.git/gikli/shelf/` of each Repository by default and can be moved ([ADR-0005](adr/0005-changelists-live-in-the-git-directory.md)).

### Changelists

- Changelist mode is the default; Staging mode is a setting. Changelists survive a switch to Staging mode and back.
- The Active changelist cannot be deleted; another one must be made active first. Deleting any other Changelist moves its changes to the Active changelist. At least one Changelist always exists.
- "Changes" is only the initial Changelist. It can be renamed and deleted like any other.
- A file belongs to a Changelist only while it has changes. A file committed outside gikli leaves its Changelist, and a later change to it goes to the Active changelist. gikli does not remember earlier assignments.
- In Scope 1 a changed file belongs to exactly one Changelist, and later edits stay in that Changelist.
- A commit in Changelist mode takes the working tree content of the selected files (`git commit --only`). Index entries for files that are not selected keep their state; gikli does not touch or clear them.
- New files appear in Unversioned Files without a prompt and are added when they are selected in a commit. Deleted files appear as deleted, and committing them records the removal. The "add new files" setting (ask / always / never) exists and defaults to never.

### History

- Undo commit works on the root commit: gikli saves a Backup first, and the changes stay in the index. In Changelist mode they appear as added files in the Active changelist.

### Defaults for settings the reference leaves "not stated"

| Setting | Default |
| --- | --- |
| Fetch remote changes automatically | on, every 20 minutes |
| Check remote for incoming changes | on |
| Load branch protection rules from GitHub | off |
| Combine stashes and shelves in one tab | off |
| Diff context lines | 4 |
| Go to next file after the last change | on |
| Automatically apply non-conflicting changes | off |
| Create changelists automatically | off |
| Allow changes within one file in different changelists | on |
| Highlight files from inactive changelists | on |
| Dialog on editing a file from an inactive changelist | off |
| Highlight files with changelist conflicts | on |
| When an empty changelist becomes inactive | do nothing |
| Shelve base revisions | on |
| Add new files to git | never |

## Deferred

Settled when the feature's scope tier comes up.

| Decision | Scope | Source |
| --- | --- | --- |
| Whether Update Project integrates only the current branch of each Repository or all tracked branches. The reference's current reading is the current branch only. | 2 | Kickoff brief; reference section 4 |
| Whether gikli adds and removes submodules | 3 | Kickoff brief; reference section 11 |
| Whether any JetBrains internal action from section 14 becomes a gikli feature | 4 | Kickoff brief; reference section 14 |
| `[unconfirmed]` items of Scope 2 and above: worktrees version, history for a method or class | 2+ | Reference |
