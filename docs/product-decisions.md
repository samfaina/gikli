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
- A password in a URL shows as `***` wherever gikli keeps or shows the URL: the operation log, events, prompts and failures. git still receives the URL as the user gave it, so `git clone` writes it to `.git/config`, as it does in a terminal. gikli leaves that file to git.
- git writes every command line to its trace of a run, the only place that names the hook that runs or failed, and git does not always hide a password there. So no file holds the trace: gikli reads it as git writes it and keeps only the names and results of the hooks of the git it started, in memory. On Windows git writes to a named pipe, on macOS and Linux to a socket, and inside WSL to a FIFO that a small shell script reads, which sends gikli only those hook events, on the error output, and gikli takes them out before anything else reads it. A git that a hook starts inherits the target, gets no trace of use to gikli, and its output stays as it was. When gikli cannot set up the reader, git runs without a trace and gikli cannot name a failed hook.
- A password can read as a port, as in `https://user:123/abc@host/repo.git`, and text alone cannot tell that from a valid URL with a port and an `@` in its path. gikli hides both, up to the last `@` before the URL ends at whitespace, a quote or the end of the line, and accepts two costs. A valid URL such as `https://host:8080/team@v2/repo.git` shows as `https://host:***@v2/repo.git`, while git still receives it whole. A password with zero or more digits, a `/` and then whitespace or a quote, as in `https://user:123/a b@host/repo.git`, stays visible, as hiding it would also hide ordinary text in git's messages.
- gikli hides nothing in a `file` URL, because it has no password to hide. `file://C:/Users/me@x/repo.git` shows whole, and so does `file://ada:s3cret@host/repo.git`, where the scheme settles what text alone cannot. The exemption covers no other URL on the line, so one that follows is masked as usual, and so is a `://` inside the path, which reads as another URL. A URL of another scheme earlier on the line can still run up to an `@` in the `file` URL, as its masking does not change. Both err on the side of hiding.

### Clone

- Submodules are cloned with `git clone --recurse-submodules`, the command the reference names, under the setting "Clone submodules recursively", on by default. A clone whose submodules fail keeps what git left in its folder, as git does, and says so, with the cause when gikli knows it. The URL the user gave was fine, so the failure is about the submodules first. The user can open the clone without them or delete it.
- The URL field holds what git gets. Choosing HTTPS or SSH rewrites the URL there: the form it is already in stays as typed, and the other is built from the host and the path alone. A port stays with the form it was typed for, and a password never moves to the other form. The SSH form uses the user `git`, as the servers gikli knows of expect. A URL on plain HTTP keeps its HTTP as its HTTPS form, as gikli cannot know that the server has HTTPS.
- Only HTTP, HTTPS and SSH URLs have these forms. A folder on this machine, a `file://` URL and a `git://` URL offer no choice, and clone as they are.
- gikli tells a server it cannot reach only from what ssh and curl write, as neither translates it. A repository the server does not have is reported by git in the user's language, with nothing else to tell it by, so it stays the general failure that points to the operation log.

### Diff

- git decides what differs. gikli has it compare the version in HEAD with the file on disk, so what `core.autocrlf` or a clean filter converts is no difference. Without a conversion, a changed line ending is a difference, even though the two sides show the same text.
- The diff shows what git shows as text. A file git takes as binary shows without a text diff, and so does one that holds a zero byte, whatever git says: with Git LFS, git compares pointers while the file on disk is binary. A submodule or a nested Repository shows without a text diff too.
- The local side is editable, unless the setting "Edit the local side of a diff" is off. A file that is not UTF-8 and a symbolic link are read-only, as saving what the view shows of them would write something else.
- Edits are saved with Save, and also when the user moves to another file, closes the diff or closes the window, so moving on never drops them. When the save fails, the user stays where they are, with the reason and a way to discard the edits after confirming.
- A file that changed on disk after gikli read it is never overwritten. The save fails, and the user can copy what they want to keep and discard the edits.
- While the user edits, the view follows their edits with CodeMirror's own diff, also after a save, as drawing the editors again would lose the cursor and the undo history. The diff comes from git again the next time the file is read.
- CodeMirror draws two differences as one when a line of at most two characters, such as `}` or an empty line, is all that separates them, and it has no option to stop that. The next difference stops once for the two, and the line is drawn as changed.
- A file without a text diff has one place to look, so next difference moves on from it as from a last difference.
- A saved line keeps the line ending it has in the file. Unchanged lines at the top and bottom of the file keep theirs, and the lines between get the line ending most of the file has. In a file that mixes line endings, two edits far apart in one save can therefore change the line endings between them.
- Next and previous file follow the changes view as the user arranged it, leaving out ignored files. A file that stops being a change while the user looks at it keeps a place in that order, as a change in the Active changelist, the one it would go to if it changed again.
- Shortcuts: double-click, Enter or Ctrl+D (Cmd+D on macOS) on a row opens its diff. In the diff, F7 and Shift+F7 go to the next and previous difference, Ctrl+Alt+Right and Ctrl+Alt+Left (Cmd on macOS) to the next and previous file, Ctrl+S (Cmd+S) saves and Esc closes.
- A single unchanged line past the context stays visible, as the marker that says it is hidden would take its place.

### Long operations and hooks

- The user can cancel a running long operation. That ends git and every process it started. gikli never kills a hook by itself and has no timeout, as it cannot tell a hook waiting on the console from a slow one. While a hook runs, the view of the operation names it next to Cancel.
- Cancelling is not offered for git inside WSL, as ending the process on the Windows side may not reach the ones on the Linux side.
- A clone cancelled during `post-checkout` or `post-index-change` is complete: gikli keeps it, opens it and says the hook did not finish. A clone cancelled earlier, including during a hook such as `reference-transaction`, has no checkout yet: gikli removes what it created, and a folder that existed before stays, empty. A clone with submodules left to clone is not complete either, even in those two hooks, as git goes on to the submodules after them: it goes the same way.
- A run cancelled while it holds `index.lock` ends with the usual offer to remove the lock, which also removes the `next-index-*.lock` files of `git commit --only`.

### Saved local changes and the Shelf

- In Scope 1, Saved local changes use a stash. When the Shelf ships in Scope 2, the default method becomes Shelve, as in JetBrains.
- When gikli saves local changes, it also records which Changelist each change belonged to and restores those assignments when it puts the changes back.
- The Shelf lives in `.git/gikli/shelf/` of each Repository by default and can be moved ([ADR-0005](adr/0005-changelists-live-in-the-git-directory.md)).

### Changelists

- Changelist mode is the default; Staging mode is a setting. Changelists survive a switch to Staging mode and back.
- Changelists keep following the changes while Staging mode is on, so a change made meanwhile is in the Active changelist on the way back and a file committed meanwhile has left its Changelist. Staging and unstaging move no file between Changelists.
- In Staging mode a commit takes the index as it is, and an amend adds the index to the last commit. Either one needs something staged, except a commit that concludes a merge, which can stage nothing when the merge kept what HEAD has. New files stay in Unversioned Files until they are staged, and a conflicted file is an Unstaged change, whose diff compares with HEAD as the index holds no one version of it.
- Staging a conflicted file marks it resolved, as in git, whatever it holds. So Stage All and Stage Tracked are not offered while a file is in conflict, and each one is staged on its own.
- Rollback on a Staged or an Unstaged change asks for the same confirmation as in Changelist mode. A file on one side goes back to HEAD, as in Changelist mode. A file on both sides keeps the other side, version by version: rolling back its Unstaged change brings the file on disk back to what is staged, and the index stays. Rolling back its Staged change brings the index back to HEAD and the file on disk stays as it is, so what the user staged goes only from the index, and the Unstaged change now compares with HEAD. An added file on both sides leaves git and stays on disk as an Unversioned File, without the choice to delete its local copy, and a move on both sides leaves the new path as an Unversioned File and the old one as deleted. A conflicted file is on the Unstaged side alone, so it goes back to HEAD. A file that starts or stops being on both sides after the list was shown makes the list outdated, as the rollback would discard something else.
- Unstaging a move takes both of its paths out of the index, as a move is one row and two changes to git.
- In Staging mode next and previous file stay on the side of the diff the user opened: Staged, or Unstaged followed by Unversioned Files. A file with changes on both sides would otherwise be its own next file.
- The index side of a Staged diff is read-only. Editing the index is part of the hunk and line staging of Scope 2.
- The Active changelist cannot be deleted; another one must be made active first. Deleting any other Changelist moves its changes to the Active changelist. At least one Changelist always exists.
- "Changes" is only the initial Changelist. It can be renamed and deleted like any other.
- A Changelist needs a name, and no two Changelists of a Repository share one, so the user can tell them apart. The description is optional.
- A comment is not the description. The description says what a Changelist is for. The comment is what the commit message is pre-filled from when the Changelist is committed, so it can span several lines. Either can be left out.
- Moving changes only changes which Changelist they are in: the working tree and the git index stay as they are. An Unversioned File is in no Changelist, so it has nowhere to move from.
- Moving changes to a new Changelist is one step: when the name cannot be used, nothing moves.
- A file belongs to a Changelist only while it has changes. A file committed outside gikli leaves its Changelist, and a later change to it goes to the Active changelist. gikli does not remember earlier assignments.
- In Scope 1 a changed file belongs to exactly one Changelist, and later edits stay in that Changelist. So do moves git reports as renames: the moved file takes the place of the file it was.
- gikli notices a commit made while it was closed: a file in a commit HEAD reached since the last read leaves its Changelist, even when it has changed again. Going back in history or rewording a commit commits nothing, so a file still changed stays where it is.
- A change made before the user sets another Changelist active stays in the Active changelist of its time, even when gikli had not read it yet.
- The changes view shows the Changelists in order, then Unversioned Files, then ignored files in a group of their own. Next and previous file follow that order.
- Changelists that gikli cannot read, or that a newer gikli saved, stop the changes view with a message and stay as they are on disk: gikli does not write over what it does not understand. Assignments that cannot be saved do not stop the view, as the next read works them out again, but setting the Active changelist then fails, as that choice would be lost.
- A commit in Changelist mode takes the working tree content of the selected files (`git commit --only`). Index entries for files that are not selected keep their state; gikli does not touch or clear them.
- The commit message reaches git as written, with a line end added when its last line has none. `commit.cleanup`, `commit.verbose` and `core.commentChar` do not change it.
- The previous commit messages on offer are read from git, so there is nothing else to keep: the last 20 commits on HEAD and the local branches with the user's current `user.email`, each message once. Merges are left out, as git wrote their messages.
- The message starts from the comment of the Changelist when the selected changes are in exactly one Changelist and it has a comment, and otherwise from `commit.template`. Unversioned Files are in no Changelist, so they do not count. The template appears as its file has it, comment lines included, as what is in the box is what gets committed. What the user has written is never replaced.
- Amend replaces the last commit with one that also holds the selected files, as `git commit --only --amend`, so what is staged elsewhere stays out of it. It needs at least one selected file, as changing only the message is Edit message, in Scope 2. git refuses an amend that would leave the commit empty, and nothing changes. The box goes back to unticked after each amend, so the next commit is a new one.
- Ticking Amend starts the message from the last commit, on the same terms as the other pre-filled messages: what the user has written is never replaced, and unticking brings back what would have been there.
- Amend saves no Backup. [ADR-0003](adr/0003-backups-as-private-refs.md) covers the rewrites of section 8 of the reference and the Backup engine of [#2](https://github.com/samfaina/gikli/issues/2) names undo commit and dropping local commits, while amend is in section 3. git's reflog still holds the commit it replaced.
- Committing in detached HEAD or during a rebase asks first, and the user can commit anyway. The "Warn when committing in detached HEAD or during a rebase" setting turns this off.
- Selected Unversioned Files are added just before the commit, and go back to being unversioned when it fails. A cancelled commit that left the lock of the index behind is the exception: nothing is committed and they stay added, as git cannot touch the index until the user removes the lock.
- A rename whose old path holds a new file git does not track commits only the new path, unless that file is selected too. `git commit --only` would otherwise commit the unselected file, so the removal of the old path stays staged.
- gikli asks for `user.name` and `user.email` when git's configuration has none, even where git could make them up from the name of the machine, as a commit would carry those for good. The answer goes to the global configuration, or to the Repository's own when the user unticks "Use for every Repository".
- Rollback discards changes only after the user confirms a list of the files. The list holds every file that goes, including a new or ignored file the rollback would write over. When what a rollback would discard is no longer what the list says, nothing goes and the list is shown again. Selecting an Unversioned File rolls nothing back. A file the last commit does not hold leaves git and, unless the user also chooses to delete its local copy, stays on disk as an Unversioned File. Rolling back a rename puts the file back and leaves nothing at the new path, as JetBrains does. A submodule is rolled back only when it is new: git only moves what the Repository records, so a changed submodule would stay changed and a moved one would lose its files.
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
| Clear the commit message after committing | on |
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
