# Product decisions

Decisions that complete or settle the [product reference](reference/jetbrains-git.md). Architectural decisions with real trade-offs live in [adr/](adr/), and terms live in [GLOSSARY.md](../GLOSSARY.md).

## Settled

### Platform and language

- Windows, macOS and Linux from the first release, WSL repositories included. Windows is the main development platform ([ADR-0002](adr/0002-tauri-rust-typescript.md)).
- English interface at launch. Every string is externalized so other languages can be added without code changes.
- Minimum git version 2.34. Below it, gikli warns and does not run operations. Features that need a newer git (for example `rebase --update-refs`, 2.38) are disabled with an explanation.
- gikli looks for git in this order: the path in the settings, then PATH, then on Windows the git inside WSL. A path in the settings is used as is: when there is no git there, or it is too old or unreadable, gikli says so and does not fall back to another git.
- A folder under `\\wsl$` or `\\wsl.localhost` always uses the git of its WSL distribution, whatever git gikli found for other folders.
- Light and dark themes that follow the operating system. gikli's own design and keyboard shortcuts, following each platform's conventions (Ctrl on Windows and Linux, Cmd on macOS), without copying the JetBrains layout beyond what a feature needs.
- One window per Project, with several windows open at once.
- No telemetry and no automatic error reports. The user exports a report by hand.
- License: Apache-2.0.
- Distribution through GitHub Releases: `.msi`/`.exe` on Windows, `.dmg` on macOS, AppImage and `.deb` on Linux, with the Tauri updater. Code signing on Windows and macOS before the first public release; development builds are unsigned.

### Credentials and prompts

- Prompts from git and ssh reach the user through gikli's askpass program, which replaces any askpass program the user configured. It covers usernames, passwords, SSH passphrases, unknown host keys and signing with an SSH key. gikli keeps no answer. This departs from "work as configured" in [ADR-0001](adr/0001-drive-the-users-git-executable.md) for the askpass program only, as gikli runs git without a terminal.
- "Use credential helper" is the other departure: when the user turns it off, git uses none of their helpers. It is on by default, and gikli also adds a helper of its own after theirs, which only notes that a remote refused the credentials.
- Passphrases for GPG keys stay with `gpg-agent` and its own pinentry window. The embedded GPG prompt is Scope 3 in the reference.
- gikli never edits `known_hosts`: ssh records a key the user accepted, and a changed key is the user's to resolve.
- A password in a URL shows as `***` wherever gikli keeps or shows the URL: the operation log, events, prompts and failures. git still receives the URL as the user gave it, so `git clone` writes it to `.git/config`, as it does in a terminal. gikli leaves that file to git. git also writes the command line to the trace file gikli reads and removes after each run: git 2.51 hides the password of a URL it can read there, and gikli cannot change what a given version of git writes.
- A password can read as a port, as in `https://user:123/abc@host/repo.git`, and text alone cannot tell that from a valid URL with a port and an `@` in its path. gikli hides both, up to the last `@` before the URL ends at whitespace, a quote or the end of the line, and accepts two costs. A valid URL such as `https://host:8080/team@v2/repo.git` shows as `https://host:***@v2/repo.git`, while git still receives it whole. A password with digits, a `/` and then whitespace or a quote, as in `https://user:123/a b@host/repo.git`, stays visible, as hiding it would also hide ordinary text in git's messages.

### Long operations and hooks

- The user can cancel a running long operation. That ends git and every process it started. gikli never kills a hook by itself and has no timeout, as it cannot tell a hook waiting on the console from a slow one. While a hook runs, the view of the operation names it next to Cancel.
- Cancelling is not offered for git inside WSL, as ending the process on the Windows side may not reach the ones on the Linux side.
- A clone cancelled during `post-checkout` or `post-index-change` is complete: gikli keeps it, opens it and says the hook did not finish. A clone cancelled earlier, including during a hook such as `reference-transaction`, has no checkout yet: gikli removes what it created, and a folder that existed before stays, empty.
- A run cancelled while it holds `index.lock` ends with the usual offer to remove the lock, which also removes the `next-index-*.lock` files of `git commit --only`.

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

## Roadmap

gikli ships in the four scope tiers of the [product reference](reference/jetbrains-git.md). A tier starts when the previous one meets "Done means" from the kickoff brief: every feature behaves as its row says, the repository conditions hold, and Appendix B holds for every failure the tier can reach.

| Tier | Contents | Spec |
| --- | --- | --- |
| 1 | MVP | [#2](https://github.com/samfaina/gikli/issues/2) |
| 2 | Complete daily use | not started |
| 3 | Advanced | not started |
| 4 | Optional | not started |

### Starting a tier

1. **Grill.** Run `grill-with-docs` over the reference rows marked with the tier and the Deferred decisions below that belong to it. Record new terms in the glossary and new architecture in ADRs, and move settled items from Deferred to Settled.
2. **Spec.** Run `to-spec` and publish the spec with `ready-for-agent`. Add its number to the table above.
3. **Tickets.** Run `to-tickets` and publish the tickets as sub-issues of the spec. The last ticket verifies the repository conditions and Appendix B for the tier.

### Known inputs per tier

- **Tier 2:** Update Project's branch coverage. Saved local changes default to Shelve once the Shelf ships. The rest of Smart Checkout and the merge dialog build on the Tier 1 save, switch and restore (see the Further Notes of [#2](https://github.com/samfaina/gikli/issues/2)).
- **Tier 3:** adding and removing submodules. The choice between classic rebase and fast rewrite for history editing. Protected branch rules per Repository and from GitHub.
- **Tier 4:** JetBrains internal actions. GitHub and GitLab integration.
- **Before the first public release, whatever the tier:** code signing on Windows and macOS, installers and the updater, and the `LICENSE` file.

## Deferred

Settled when the feature's scope tier comes up.

| Decision | Scope | Source |
| --- | --- | --- |
| Whether Update Project integrates only the current branch of each Repository or all tracked branches. The reference's current reading is the current branch only. | 2 | Kickoff brief; reference section 4 |
| Whether gikli adds and removes submodules | 3 | Kickoff brief; reference section 11 |
| Whether any JetBrains internal action from section 14 becomes a gikli feature | 4 | Kickoff brief; reference section 14 |
| `[unconfirmed]` items of Scope 2 and above: worktrees version, history for a method or class | 2+ | Reference |
