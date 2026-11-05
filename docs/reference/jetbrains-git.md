# JetBrains Git feature reference

## What this reference is for

This catalog defines **gikli**, a standalone desktop Git client with the same functionality and behavior as the Git integration of the JetBrains IDEs (IntelliJ IDEA 2026.2 and its siblings). It describes, area by area, what each feature does and what the user gets. It sets no technology, architecture or way of working.

How to read it:

- Reproduce the **behavior**, not the interface: no identical icons, colors, text, shortcuts or product names.
- The **Git equivalent** column names the git operation whose result the feature matches. It explains the meaning of the feature and does not prescribe how gikli achieves it.
- The **Scope** column sets the product tier: **1** = MVP, **2** = complete daily use, **3** = advanced, **4** = optional or out of scope.
- Items marked **\[IDE\]** depend on concepts specific to the IDE (code analysis, project model, Local History); the section "IDE-dependent features" says how gikli adapts them.
- Items marked **\[unconfirmed\]** could not be settled from primary sources and need a product decision.
- Evidence tags: **\[doc\]** means the JetBrains help or blog says so, and **\[src\]** means the action definitions or defaults in JetBrains' open source (intellij-community, commit 94e95f9d1, 5 November 2026), and **\[git\]** means the behavior was observed in git itself on real repositories (see "Git behavior to account for"). Older catalog rows without a tag come from the JetBrains help of the first revision.
- **(gikli)** marks a product decision that goes beyond JetBrains or differs from it on purpose.

Baseline: the official IntelliJ IDEA 2026.2 help, read on 16 October and 4 November 2026 (some pages rebuilt on 3 November). IntelliJ IDEA 2026.3 is at Release Candidate with no help or What's New pages yet, so its changes are not included. Links in the final section.

## Feature map

The 14 catalog sections fall into four working areas; the MVP needs pieces from all four to be useful day to day.

| Working area | Catalog sections |
| --- | --- |
| A. Workspace and commit | 1 Repository, 2 Local changes, 3 Commit, 9 Stash and shelve, 10 Patches |
| B. Collaboration | 4 Synchronization, 5 Branches and tags, 11 Worktrees, submodules and forks |
| C. History and integration | 6 Log, history and blame, 7 Merge, rebase, cherry-pick and conflicts, 8 Rewriting history |
| D. Viewing and platform | 12 Diff viewer, 13 Local History, 14 Miscellaneous |

## Catalog by area

### 1. Repository: open, clone, initialize, remotes

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Git environment | Finds the user's git setup automatically, with a setting to give the path and a button to test it. On Windows it works with WSL git if there is no native git, and with projects opened through `\\wsl$` [doc] | `git --version` | 1 |
| Open repository | Opens a folder and detects its repository root; in folders with several repositories it detects nested "unregistered" roots and offers to add or ignore them; marks in red configured roots that are no longer valid repositories | `git rev-parse --show-toplevel` | 1 |
| Clone | URL + target folder; with a connected GitHub/GitLab account it lists the user's and their organizations' repositories; option for a shallow clone of N commits and later "unshallow"; all remote branches stay visible in a shallow clone [git] (gikli); recursive submodules, on by default [src]; choice between SSH and HTTPS URLs | `git clone [--depth N --no-single-branch] [--recurse-submodules]`; `git fetch --unshallow` | 1 (shallow: 3) |
| Create repository | Initializes git in a folder | `git init` | 1 |
| Add files | Add untracked files; ask when new files are created (ask / always add / never); force when the file is ignored; when a tracked file is deleted, ask whether to remove it from git | `git add`, `git add -f`, `git rm` | 1 |
| Ignore files | Two targets: `.gitignore` (shared) or `.git/info/exclude` (local only); toggle to show or hide ignored files | `.gitignore`, `.git/info/exclude`; `git status --ignored` | 2 |
| Manage remotes | Dialog to add, edit the URL of and remove remotes; if there are none when pushing, it offers to define one (name + URL + immediate fetch) | `git remote add/set-url/remove`, `git fetch` | 2 |
| Credentials | Setting "Use credential helper", on by default: the user's helpers stay in charge [doc]. Option not to save. gikli stores no passwords (gikli) | the system credential helper (Git Credential Manager on Windows) | 1 |
| Multi-repository | A project can hold several repositories; lists show which root each item belongs to, with one color per root | one repository per root | 3 |

### 2. Local changes, staging and partial commit

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Local changes view | Tree of modified (blue), added (green), deleted, renamed, untracked and conflicted files; group by folder or flat list; refresh; show ignored | `git status` | 1 |
| File diff | Double-click or a shortcut opens the local diff against HEAD (or against the index); option for a single diff with every file | `git diff [--cached] -- <path>` | 1 |
| Partial commit by hunk | Checkboxes per hunk inside the diff; what is not ticked stays in its changelist for another commit [doc]. In staging mode, anything else already staged stays staged [git] (gikli). If a selected path also has an overlapping staged edit, the staged version cannot be kept and gikli says so (gikli) | `git commit` of a selection | 2 |
| Partial commit by line | Checkboxes per line; "split hunks and include the selected lines" | same as by hunk | 2 |
| Staging area mode | Alternative to changelists, chosen in the settings and off by default [src]: shows Staged / Unstaged; stage a file, hunk or line; three-way diff HEAD / index / local with an editable index; staged changes are marked differently. The settings page says enabling it deletes existing changelists, while the commit page says they are saved [doc]. gikli keeps them and brings them back when the user switches (gikli) | `git add`, `git restore --staged` | 1 (file), 2 (hunk/line) |
| Rollback | Confirmation list of files to discard; option to also delete the local copies of added files. Also works in a repository with no commits [git] | `git restore --source=HEAD --staged --worktree -- <paths>`; for added files `git rm --cached` + delete | 1 |
| Changelists | Named groups of uncommitted changes that belong to gikli, so git does not know them. A default list named Changes holds new changes, and an Unversioned Files group holds new files. The user creates as many as needed, with a name and optional description, renames them, and commits each one independently [doc] | none | 1 |
| Active changelist | One list is active at a time, and new changes go into it. The user can set any list active [doc] | none | 1 |
| Move changes between changelists | Move selected changes to an existing or new list, from the menu or by dragging. Optional comment, reused as the commit message when that list is committed. Option to make the target active [doc] | none | 1 |
| Changes within one file in different changelists | Setting "Allow putting changes within one file into different changelists": a hunk or lines move to another list from the diff or the gutter, with Move Lines to Another Changelist [doc] | none | 2 |
| Delete a changelist | Removes the list [doc]. The help does not say what happens to its changes **\[unconfirmed\]**; gikli moves them to the active list and loses nothing (gikli) | none | 1 |
| Automatic changelists | Setting "Create changelists automatically", for example for cherry-pick and revert. The unshelve, apply patch and revert dialogs let the user pick a target list [doc] | none | 2 |
| Empty changelists | Setting for an empty list that becomes inactive: do nothing or remove silently [doc] | none | 2 |
| Changelist conflicts | Files from inactive lists are highlighted. Editing one can open a dialog with shelve, move to the active list, switch to their list or ignore. Each behavior has its own setting [doc]. In gikli this applies where files are edited in the app (gikli) | none | 3 |
| Stage all | Stage every change, or every change in tracked files [src] | `git add -A`; `git add -u` | 1 |
| Commit all when nothing is staged | Toggle in staging mode: with nothing staged, the commit takes all files [src] | `git commit -a` | 2 |
| Stash selected files | Stash the chosen files from the staging view [src], though the stash help says stashing takes all changes [doc] | `git stash push -- <paths>` | 2 |
| Line ending warning | Warns when CRLF line endings are about to be committed; offers "commit as is" or "fix and commit" | none | 2 |
| Show ignored files | View option in the changes list | `git status --ignored` | 2 |
| Commit side panel | Commit from a non-modal panel next to the changes. In JetBrains the modal dialog survives only as a bundled plugin since 2025.1 [doc] | none | 1 |
| Diff as a tab | Diff opens as a tab (default) or in a separate window [doc] | none | 2 |

### 3. Commit

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Commit | Message + selected files or a whole changelist, with the active list selected by default [doc]; history of previous messages; message pre-filled from the changelist comment or `commit.template`; option to clear the message after committing; asks for `user.name` if it is missing. Unversioned files can be selected and are added in the same step [doc]. The message is kept as written, so a line such as `# 123 is the ticket` survives [git] (gikli) | `git commit` | 1 |
| Message checks | Blank line between subject and body; maximum subject and body length with a visible margin; spell check; rewrap long lines | none | 2 |
| Amend | Checkbox to amend the last commit; a selector allows amending a chosen local commit, the root commit included [git] | last: `git commit --amend`; older: `git commit --fixup=<sha>` + `git rebase -i --autosquash` | 1 (last), 3 (older) |
| Author | Change the commit author; commits whose author differs from the committer are marked with `*` in the log | `git commit --author="Name <email>"` | 2 |
| Sign-off | Adds the Signed-off-by line | `git commit --signoff` | 2 |
| Run git hooks | Per-commit toggle to skip hooks; global setting | `git commit --no-verify` | 2 |
| GPG signing | Configure a key and sign commits; the signature state shows in the commit details and in an optional log column | `commit.gpgsign`, `user.signingkey`; `git log --format=%G?` | 3 |
| Commit and push | Commit followed by the push dialog, or a direct push. Two settings: show the dialog for commit and push, and show it only for protected branches. When suppressed, the push goes to the affected repository without a dialog [doc] | `git commit` + `git push` | 1 |
| Pre-commit checks | Reformat, optimize imports, analyze code, check TODOs, run tests or an external tool; option to run them after the commit | **\[IDE\]**; standalone: only "run an external command" | 4 |
| AI message | Generate or improve the message with AI; AI self-review of changes | out of scope for the base product | 4 |
| Unusual state warning | Warns when committing in detached HEAD or during a rebase | detached HEAD or rebase in progress | 1 |
| Commit during a merge | A partial commit is impossible during a merge [git]. gikli hides selection checkboxes while a merge is in progress (gikli) | `git commit --only` is refused | 2 |
| Inline commit | Commit one change from the gutter, with optional amend [doc] | same as partial commit | 3 |
| Large file warning | Warns when committing files above a size, on by default at 50 MB [src] | none | 2 |
| File name warning | Warns about names that may cause problems on other systems, on by default [src] | none | 2 |
| Fixup and squash completion | The message field completes `fixup!`, `squash!` and `amend!` prefixes from the 20 latest commits, the same limit as the Amend selector [src] | `git commit` message prefixes | 3 |
| SSH signing | Not found in the JetBrains pages read; GPG is documented. Sign with an SSH key (gikli) | `gpg.format=ssh` | 3 |

### 4. Synchronization: fetch, pull, update, push

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Fetch | Fetch every remote or the selected branch | `git fetch --all --prune` / `git fetch <remote> <branch>` | 1 |
| Automatic fetch | Background fetch of all remotes every 20 minutes when "Fetch remote changes automatically" is on. A separate setting, "Check remote for incoming changes", marks branches in the selector [doc]. The interval is a user setting in gikli (gikli) | `git fetch --all` | 2 |
| Incoming/outgoing indicators | Arrow for incoming commits (on the remote, not yet integrated) and outgoing ones (not yet pushed) per branch | commits ahead of and behind `<branch>@{u}` | 1 |
| Tag policy on fetch | Auto (git default), Sync (deletes local tags that no longer exist on the remote), Always, Never [doc]. Across several remotes, Sync would delete tags that exist on only one of them [git], so gikli runs Sync against the upstream remote of the current branch and warns when a repository has several (gikli) | Sync: `git fetch <remote> --prune --prune-tags`; Always: `--tags`; Never: `--no-tags` | 3 |
| Pull | Dialog: repository, remote (default `origin`), branch (the upstream by default; if the user picks another source branch it is remembered and shown next time [doc]); options `--rebase`, `--ff-only`, `--no-ff`, `--squash`, `--no-commit`, `--no-verify`. From a branch: "pull into the current one using rebase / using merge" | `git pull [options] <remote> <branch>` or `git fetch` + `git rebase/merge <remote>/<branch>` | 1 |
| Update a branch that is not checked out | Fast-forwards a local branch that is not checked out to its upstream (multi-select). A branch checked out in any worktree cannot be updated this way, so gikli reports it and continues with the rest [git] (gikli) | `git fetch <remote> <remote-branch>:<local-branch>` (fast-forward only) | 2 |
| Update project | Fetches all roots, then integrates with a chosen method: Merge (like `git pull --no-rebase`), Rebase (like `git pull --rebase`) or Reset to the Remote Branch, which drops local commits [doc]. Local changes are saved first with the method set in "Clean working tree using" (Shelve by default [src]) and restored afterwards [doc]. The update method is Merge by default [src]. Submodules on a branch are updated too; submodules in detached HEAD get `git submodule update` [doc]. When done it opens a tab with the new commits and changed files, with an optional path filter. The help says "tracked remote branches"; whether it integrates more than the current branch of each root is [unconfirmed], so gikli updates the current branch and leaves other branches to "Update a branch that is not checked out" | Merge: `git pull --no-rebase`; Rebase: `git pull --rebase`; Reset: `git fetch` + `git reset --hard @{u}`; submodules: `git submodule update` | 2 |
| Push | Dialog with a tree repository → local branch → target `remote/branch` and the commits about to be pushed (with their diff); the local branch is fixed to the current one; the target can be changed or created, and all targets edited at once; commits by other authors carry `*`; "Push tags" is off by default, with All or Current Branch [doc]. Current Branch means `--follow-tags`, so it sends annotated tags only | `git push [-u] <remote> <local>:<target>`; `--tags` / `--follow-tags` | 1 |
| Safe force push | Separate force push option that uses "with lease", controlled by the "Use Safe Force Push" setting, on by default [src]; optional warning; not available on protected branches [doc]. gikli also refuses to overwrite remote commits the user has not integrated, because automatic fetch would otherwise satisfy the lease (gikli) | `git push --force-with-lease --force-if-includes` | 2 |
| Protected branches | Semicolon-separated list of regular expressions, `master` and `main` by default [src], shared with the team, where force push and rewriting pushed commits are not allowed. Optional "Load branch protection rules from GitHub", synced on fetch; the history page also says a branch protected on GitHub is marked when checked out [doc]. The list is per repository (gikli) | none; GitHub branch protection | 3 |
| Rejected push | If "Auto-update if push of the current branch was rejected" is off, a dialog offers merge or rebase, and "remember and silently update in future" turns the setting on. If it is on and no method was chosen before, the IDE merges silently; otherwise it uses the remembered method [doc] | `git merge @{u}` or `git rebase @{u}`, then `git push` | 2 |
| Push up to here | From the log: pushes commits up to the selected one | `git push <remote> <sha>:refs/heads/<branch>` | 3 |
| Checkout and update | Checks out a branch and syncs it with its upstream in one step | `git switch <branch>` + `git fetch` + `git merge --ff-only @{u}` | 2 |
| Unshallow prompt | Detects a shallow repository and offers to convert it into a complete one [src] | `git fetch --unshallow` | 3 |
| Update info view | Lists the commits an update brought in, opens automatically (advanced setting, on) [doc] | `git log <old>..<new>` per root | 2 |
| Update info path filter | Restricts the update info to paths [doc] | `git log <old>..<new> -- <paths>` | 3 |
| Pull into current from a branch | Merge or rebase variant from the branch menu [doc] | `git pull <remote> <branch>` or with `--rebase` | 2 |

### 5. Branches and tags

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Branch selector | Shows the current branch; search; groups Recent (up to 5, fixed since 2025.2), Local, Remote, Tags, with Recent and Tags optional; group by directory (`feature/…`); incoming/outgoing marks; access to abort or continue the operation in progress; the new-branch field suggests prefixes from existing branches [doc] | `git for-each-ref`; recent ones from `git reflog` | 1 |
| Branches pane | Side list in the log with actions: new branch, update, delete, compare with the current one, "my branches only", fetch, favorites, go to the branch's latest commit | same | 1 |
| New branch | Name (prohibited characters become `-`, a configurable symbol [src]), checkout checkbox, base = HEAD, the selected branch or a commit. Invalid names are rejected, including `@{-1}`, which git would expand to another branch [git] | `git branch <name> <base>` / `git switch -c` | 1 |
| Checkout local and remote | Local: direct switch. Remote: creates a tracking local branch; if a local branch with that name already exists and no commits would be lost, it resets it to the remote; otherwise it offers to drop the local commits or rebase onto the remote | `git switch <branch>`, `git switch --track <remote>/<branch>` | 1 |
| Checkout tag or revision | Leaves a detached HEAD | `git switch --detach <ref>` | 1 |
| Smart checkout | If local changes would be overwritten, it lists the blocking files and offers Force Checkout (discard) or Smart Checkout: save, switch and restore, resolving conflicts if any. Saving uses the "Clean working tree using" setting, Shelve by default and Stash as the alternative [doc] [src] | `git switch`; force: `git switch -f`; `git stash` when that method is chosen | 2 |
| Untracked files block checkout | A notification says untracked files prevent the operation and to move or commit them, with a dialog listing the files [src]. A stash of tracked changes does not help, and including untracked files fails on restore if the target branch holds them [git] | none: no automatic save | 2 |
| Rename | Local branches only; option to unset the upstream | `git branch -m <old> <new>` \[+ `--unset-upstream`\] | 1 |
| Delete | Deletes even if not merged; notification with "restore" (recreates it on the same commit), a link to see the unmerged commits and one to also delete the remote branch, which stays available in the notifications panel until restart [doc]; multi-select | `git branch -D`; restore: `git branch <name> <sha>`; remote: `git push <remote> --delete <branch>` | 1 |
| Favorites | Mark favorite branches (main is one by default); they are pinned at the top and can filter the log | none | 2 |
| Compare with current | Opens a log with the commits of the selected branch that are not in the current one; swap sides; see every changed file | `git log <current>..<other>` | 2 |
| Diff with working tree | Files that differ between another branch and the local tree (only there / only local / different); get a whole file from that branch | `git diff --name-status <branch>`; `git restore --source=<branch> -- <path>` | 2 |
| Compare a file or folder with a branch | From a file: compare with the version in another branch or tag and take it | `git diff <branch> -- <path>` | 2 |
| Branch menu | Checkout; new branch from here; "checkout and rebase onto current"; "rebase current onto this one"; "merge this one into current"; new worktree; pull into current with rebase/merge; update; push; rename; delete | see areas 7 and 11 | 1–2 |
| Branches in every repository | In multi-repository projects, run the branch operation in every repository and offer to roll back where it succeeded if one fails | the same operation per root | 4 |
| Tags | Create a lightweight tag (from the log) or an annotated one (with a message, on a commit or an expression such as `branch~2`; an empty message makes it lightweight; force to move an existing one); jump to a tag; checkout (detached); push to origin; delete locally; show tag names in the log [doc] | `git tag [-a -m] [-f] <name> <ref>`; `git push <remote> <tag>`; `git tag -d` | 2 |
| Delete a remote tag | After a tag is deleted locally, the notification offers Restore and Delete on Remote, which removes it from the remotes that hold it and reports each result [src]. The tag help covers only local deletion [doc]. gikli also offers it directly from a tag, behind a confirmation (gikli) | `git push <remote> --delete refs/tags/<tag>` | 2 |
| Single-click action | Setting: a click on a branch filters the log or goes to its head [doc] | none | 2 |
| Selector filters | Filter the selector by action or by repository, and group by repository [src] | none | 2 |
| Hide the branches pane | Toggle for the branches pane in the log [src] | none | 2 |
| Get a file from a branch | Copy a whole file from another branch [doc] | `git restore --source=<branch> -- <path>` | 2 |

### 6. Log, history and blame

The log has four panes: branches, commit list with graph, changed files of the selected commit and commit details, plus an optional diff preview pane.

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Commit list with graph | Graph of branches and merges; branch labels with different colors for HEAD, local and remote; highlighted background for commits of the current branch; own commits in bold; `*` when author ≠ committer; long edges off by default, drawn as jumping arrows; left/right arrow to go to the parent or child. Default order is by commit date [doc]. Stashes and gikli's backups stay out of the graph and labels (gikli) | `git log --branches --remotes --tags HEAD --date-order` | 1 |
| Performance | The log stays responsive on repositories with hundreds of thousands of commits, with optional instant filtering [doc] | none | 1 (large logs), 3 (instant filtering) |
| Columns | Author, date, hash, GPG signature, CI checks (GitHub); root column in multi-repository | `git log --format=%G?`; GitHub CI status | 2 (CI: 4) |
| Text search | Searches message, author and hash, matching any of them; search history; regex and match-case options | `git log --grep=… [-E] [-i]`; `--author` | 1 |
| Filters | Branch (list, favorites, all, or a range `a..b`), user (several, "me"), date (presets or a specific date), paths (folders) and roots; open another log tab with the current filters | `git log <revs> --author --since --until -- <paths>` | 1 (branch, text), 2 (rest) |
| Graph options | Sort by commit date (default) or topologically; first parent only; no merges; collapse and expand linear branches (all or one) [doc] | `--date-order` / `--topo-order`, `--first-parent`, `--no-merges` | 2 |
| Presentation options | Compact references, show tag names, long edges (off by default), commit timestamp instead of author time, references on the left, show root names [doc] | none (presentation) | 2 |
| Highlights | My commits, merge commits dimmed, current branch, commits not cherry-picked | `git log --cherry-mark --right-only <current>...<other>` | 2 (not picked: 3) |
| Go to hash, branch or tag | Jump with autocomplete | `git rev-parse --verify <ref>` | 1 |
| Commit details | Message (with linked hashes and issues), hash, author with email, date, signature, root, branches that contain it (first 6 and "show all") | `git show -s`; `git branch -a --contains <sha>` | 1 |
| Changed files | Tree or list; for merge commits, changes against each parent or only the conflicted files | `git diff-tree -r --root -M --name-status <sha>`; for merges, once per parent | 1 |
| Actions on a commit's files | Show diff (also in a new window); compare with local; compare the previous version with local; open the repository version; revert only those changes; cherry-pick only those changes; extract to a separate commit; drop from the commit; create patch; get that version of the file; history up to here | `git show <sha>:<path>`, `git diff <sha> -- <path>`, `git restore --source=<sha> -- <path>`; see area 8 for extract and drop | 2 |
| Commit menu | Copy hash; create patch; cherry-pick; checkout revision; show the repository at that revision (read-only); compare with local; compare two commits; reset current branch to here; revert; undo commit; edit message; fixup; squash into; drop commits; squash several; interactively rebase from here; push up to here; rebase current onto this commit; new branch; new tag; go to parent/child; open on GitHub/GitLab | see areas 7 and 8 | 1–3 |
| Repository at a revision | Browse the file tree as it was at a commit, read-only | `git ls-tree -r <sha>`, `git show <sha>:<path>` | 3 |
| File history | List of the file's revisions with a diff preview; branch filter; renames column; show every file affected in a revision; from there: get that version, create patch, revert, fixup/squash, blame; configurable row limit | `git log --follow --name-status -- <path>` | 1 |
| Folder history | Log filtered by that path | `git log -- <folder>` | 2 |
| History for selection | History of the selected lines (or the current line) | `git log -L <start>,<end>:<path>` | 3 |
| History for a method or class | The JetBrains history page describes no dedicated view, only a code-author hint that opens the annotation. **\[IDE\]** **\[unconfirmed\]** | `git log -L :<function>:<path>` approximates it for some languages | 4 |
| Blame (annotate) | Column with author, date and revision of every line; colors per commit; lines from the current revision in bold; click goes to the commit in the log; tooltip with the message; hovering highlights that commit's lines; configurable format (date, name, commit number) | `git blame [-w] [-M] [-C] -- <path>` | 2 |
| Blame options | Ignore whitespace; detect moves within the file and across files; use the commit date [doc] | `-w`, `-M`, `-C` | 2 |
| Blame previous revision | Redo the blame on the parent of a line's commit, to see who wrote it before. Annotate Revision shows the file right after a chosen commit [doc] | `git blame <sha>^ -- <path>` | 2 |
| Hide revisions | Hide a commit (a reformat, for example) from the blame; a notice lists hidden revisions and restores them [doc] | `git blame --ignore-rev <sha>` | 3 |
| Compare with local | Repository at a commit against the working tree [doc] | `git diff <sha>` | 2 |
| Log indexing | Optional setting for fast filters [doc] | none | 3 |
| External repository log | Open the log of any repository, outside the current project [src] | none | 3 |
| Log in an editor tab | Open another log tab in the editor area [src] | none | 3 |
| Diff preview position | Show the log diff preview at the right or the bottom [src] | none | 2 |
| Changed files options | Show only changes affecting the filtered paths, and show changes from each parent [src] | none | 2 |
| Copy commit subject | Copy the subject of the selected commit [src] | none | 2 |
| Select in log | Jump from a file or revision to the commit in the log [src] | none | 2 |
| Show current revision | Show which revision the current file is at [src] | `git log -1 -- <path>` | 2 |

### 7. Merge, rebase, cherry-pick and conflicts

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Quick merge | From a branch: "merge this one into current" | `git merge <branch>` | 1 |
| Merge dialog | Options `--no-ff`, `--ff-only`, `--squash`, edit message, `--no-commit`, `--no-verify`, `--allow-unrelated-histories` | `git merge [options] <branch>` | 2 |
| Smart merge | If local changes would be overwritten by the merge: stash, merge and unstash [doc]. Aborting the merge restores the local changes [git] | `git merge --autostash <branch>` | 2 |
| Quick rebase | "Checkout and rebase onto current" (also from remote branches, creating a tracking branch); "rebase current onto the selected one"; "rebase current onto this commit" | `git rebase <base>`; `git switch <branch>` + `git rebase <current>` | 2 |
| Rebase dialog | `--interactive`, `--onto` (new base and old base), pick another branch to rebase (checked out first), `--rebase-merges`, `--keep-empty`, `--root`, `--update-refs`, `--autosquash` [doc] | `git rebase [options]` | 3 |
| Operation in progress | While a merge, rebase, revert or cherry-pick is half done, the branch selector and the status bar show the state with Abort and Continue. A multi-commit cherry-pick stays marked as unfinished after a commit in the middle of the sequence [git] (gikli) | `--abort` / `--continue` | 1 |
| Skip a commit during rebase | Skip Commit sits with Abort and Continue among the rebase actions [src], though the help mentions only those two [doc] | `git rebase --skip` | 2 |
| Commit Merge | Finish a merge with a commit from the menu, alongside Abort Merge [src] | `git commit` | 1 |
| Abort Revert | Abort a revert that stopped [src] | `git revert --abort` | 1 |
| Revert Resolved | Undo the resolution of a resolved conflict [src] | `git checkout -m -- <path>` | 2 |
| Merge and rebase widget | Toolbar widget for an ongoing merge or rebase [src] | none | 2 |
| Cherry-pick | One or several commits from the log (in order); disabled if the commit is already in the current branch; setting, on by default [src], to add "cherry picked from" only when the source commit is on a protected branch [doc] | `git cherry-pick [-x] <sha>…` | 1 |
| Partial cherry-pick | Apply only some files of a commit, without committing | `git cherry-pick -n` | 3 |
| Conflicts dialog | Appears after any operation that leaves conflicts (pull, merge, rebase, cherry-pick, applying a stash or patch): list of conflicted files with "accept yours", "accept theirs", "merge…" (opens the 3-pane tool), "resolve all simple conflicts", undo a resolution, "accept and finish". If closed, the conflicts stay as a group in the changes view. During a rebase, "yours" stays the user's side, which git calls `--theirs`. Delete/modify conflicts are handled too | `git checkout --ours/--theirs -- <path>` + `git add`; `git rm` for deletions | 1 (list and pick a side), 2 (rest) |
| 3-pane merge tool | Left "yours" and right "theirs" read-only; editable center with the result, which starts from the base. Toolbar: apply every non-conflicting change (or only from one side) and "resolve simple conflicts" (different edits at the start and end of the same line). Per change: accept (» «) or ignore (×) from each side; menu "resolve using left/right"; compare the base with each side and the result with each side; gutter markers with revert; undo and redo kept in sync with the conflicts | `git merge-file --diff3`; index stages `:1:`, `:2:`, `:3:` | 2 (basic), 3 (resolve simple) |
| Apply non-conflicting changes automatically | Setting to apply changes without conflicts when the tool opens | none | 3 |
| Review a merge | On a merge commit, see the files that had conflicts and compare the result with each parent | `git diff <merge>^1 <merge>`, `^2`; `git show --cc` | 3 |
| AI merge | AI-assisted resolution with per-line apply and revert | out of scope for the base product | 4 |

### 8. Rewriting history

Common constraints: only commits on the current branch are rewritten; never commits already pushed to a protected branch; if the commits were already pushed to another branch, a force push is needed afterwards [doc]. "Interactively rebase from here" is disabled when the commit has several parents, is not on the current branch or is pushed to a protected branch, and the app explains why [doc]. Fixup, squash, drop and extract also work on pushed commits of non-protected branches [doc]. The help does not say how merges inside the range are handled, so gikli keeps them and warns the user (gikli).

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Edit message | Of a commit on the current branch that is not pushed yet [doc] | HEAD: `git commit --amend`; older: `reword` in `git rebase -i` | 2 |
| Fixup / Squash into | Puts the staged changes into an earlier commit, pushed ones included, through commit and rebase; fixup keeps the original message, squash lets you combine it [doc]. Works on the root commit too [git] | `git commit --fixup=<sha>` / `--squash=<sha>` + `git rebase -i --autosquash` | 3 |
| Squash several commits | Joins the selected commits; the dialog proposes the combined messages | `squash` in `git rebase -i` | 3 |
| Drop commits | Removes one or several commits from history. Later merges are kept [git], and the root commit can be dropped too (gikli) | `drop` in `git rebase -i` | 3 |
| Extract changes to another commit | Splits the selected files of a commit into a new commit | `edit` in `git rebase -i` | 3 |
| Drop changes from a commit | Removes the changes of the selected files from a commit | `edit` in `git rebase -i` | 3 |
| Revert selected changes | Creates a commit that undoes only the chosen files | `git revert -n <sha>` | 3 |
| Interactive rebase | Dialog with the list of commits: move up/down, pick, stop to edit, reword (double-click), squash, fixup, drop, "reset" the plan and start; preview of the resulting graph. Stopping to edit lets you continue or abort, and amend in between [doc] | `git rebase -i <base>` | 3 |
| Fast rewrite | Reword, drop, squash, reorder and extract finish in seconds on very large repositories, without checking out each commit and without touching unrelated uncommitted changes [git]. On a conflict the rewrite stops as a regular rebase so the user can resolve it. Default in JetBrains since 2026.1 [doc]. Signing and hooks keep working as the user configured them (gikli) | same result as `git rebase -i` | 3 |
| exec, break, label, merge lines | Not part of the dialog, which offers the actions listed under "Interactive rebase", and not mentioned in the history page [doc] | the same lines in `git rebase -i` | 4 |
| Undo commit | On the last commit of the current branch, for your own commits; blocked only if it was pushed to a protected branch [doc]. Puts its changes back as uncommitted. On the root commit the changes stay staged [git] (gikli); whether JetBrains allows that case is **\[unconfirmed\]** | `git reset --soft HEAD~1` | 1 |
| Revert commit | Creates an inverse commit; with several, one per commit; opens the commit with an automatic message; files can be unticked | `git revert [-n] <sha>…` | 1 |
| Reset current branch to here | Four modes: soft (later changes staged), mixed (unstaged), hard (discarded), keep (discards what was committed and keeps what was not) | `git reset --soft/--mixed/--hard/--keep <sha>` | 2 |
| Get a file version | Restores a file to its state at a revision | `git restore --source=<sha> -- <path>` | 1 |
| Backup before rewriting | Not a visible JetBrains feature, but it is gikli's safety net: before any operation in this table, gikli saves the previous state and offers "undo". Backups stay out of the log and are not sent by a normal push, and they expire after 30 days (gikli) | none | 1 |

### 9. Stash and shelve

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Stash | Dialog with root, current branch, message and an option to keep the index. It always takes all uncommitted changes, and JetBrains exposes no include-untracked option [doc]. gikli adds that option (gikli) | `git stash push [-m <msg>] [--keep-index] [-u]` | 1 |
| Stash silently | Stash without the dialog [src] | `git stash push` | 2 |
| Save local changes | Shared step for checkout, update and merge, using the "Clean working tree using" setting. If there is nothing to save, nothing is restored afterwards, and only what was saved by this operation is restored [git] (gikli). With no commits yet the stash method is unavailable [git] | `git stash push`, `git stash pop`, or the shelf | 1 |
| Stash list | Tab with the stashes; double-clicking a file shows its diff (against local or against the parent commit, configurable) | `git stash list`; `git stash show -p stash@{n}` | 1 |
| Apply, pop, as branch | Apply (keeps the stash), pop (applies and drops), create a new branch from the stash, also restore the index. The help says a stash cannot be applied to a tree with uncommitted changes [doc]; git allows it when paths do not overlap, so gikli warns and does not block (gikli) | `git stash apply/pop [--index] stash@{n}`; `git stash branch <branch> stash@{n}` | 1 |
| Drop and clear | Drop one stash or all of them | `git stash drop` / `git stash clear` | 1 |
| Stash only some files | See "Stash selected files" in section 2 [src] | `git stash push -- <paths>` | 2 |
| Shelf | The own alternative to stash: named sets of changes kept outside git, from selected files or a changelist. Unversioned files cannot be shelved [doc] | no git equivalent | 2 |
| Shelve silently | Shelve at once, with the changelist name as the shelf name [doc] | none | 2 |
| Save to shelf | Copy changes to a shelf and keep them in the working tree [doc] | none | 2 |
| Unshelve | Dialog with target changelist (existing or new), comment, make it active, and remove applied files from the shelf. Conflicts go to the conflict tools [doc] | none | 2 |
| Quick unshelve | Unshelve silently into the active list, or drag a shelf onto the changes; a modifier keeps the shelf. Select files or a list to unshelve part of it [doc] | none | 2 |
| Rename, delete and restore shelves | Rename a shelf; delete it; applied and deleted entries stay recoverable until removed, shown through Already Unshelved and Recently Deleted [doc] [src] | none | 2 |
| Shelf base versions | Setting that stores the base of each file with the shelf, so restoring can do a 3-way merge even after a rebase. Without it the base is searched in history, which can be slow or fail [doc] | none | 2 |
| Import patches | Import a patch file as a shelf [doc] | none | 3 |
| Shelf location | Under the project by default; can be moved, optionally with existing shelves, for example to share them [doc] | none | 3 |
| Stashes and shelves together | Option to show stashes and shelves in one tab | none | 2 |

### 10. Patches

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Create patch | From local changes, a commit, a revision in the history, selected files or a stash; to a file or the clipboard; options for base path, reverse patch and encoding | `git diff [--binary] [-R]`; for commits `git format-patch -1 <sha>` | 3 |
| Apply patch | From a file, the clipboard or by dragging; git-format patches fill in message and author; dialog with a diff view (conflicts in red), remap the base directory, strip or restore leading folder levels; conflicts go to the merge tool | `git apply -3 [-p<n>] [--directory=<dir>]`; mailbox format `git am -3` | 3 |
| Detect a patch on the clipboard | Offers to apply it when detected | none | 4 |

### 11. Worktrees, submodules and forks

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Worktrees | Only for single-repository projects; the tab appears when more than one worktree exists. Create one from a branch (a remote branch creates a same-named local one unless "New branch" is set) or a new branch, with a name and location; open one in another window; open from a branch checked out elsewhere. Status shows only Locked or Prunable. Lock and unlock [src]. Delete (not the main one or the current one) and prune all. Open the review of the branch on the hosting service [src]. Limits: a branch cannot be in two worktrees; nesting inside the project is unsupported [doc]. Version introduced is [unconfirmed] | `git worktree list`, `add [-b <new>] <path> <branch>`, `remove`, `prune` | 3 |
| Submodules | Cloned recursively and shown as separate repositories; updated when the project is updated (with `submodule update` when in detached HEAD). Adding or removing submodules **\[unconfirmed\]** | `git submodule update --init --recursive`; `git submodule status` | 3 |
| Sparse checkout | Not documented in the help. gikli must not break on sparse repositories | `git sparse-checkout` | 4 |
| Sync fork | Treats origin as your fork, adds or reuses the `upstream` remote, fetches and rebases the current branch onto upstream's default branch | `git remote add upstream`, `git fetch upstream`, `git rebase upstream/<default>` | 3 |

### 12. Diff viewer

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Layout | Side by side or unified; the local side is editable (can be turned off); colors for added, modified and deleted | versions of the file at `<ref>:<path>` and on disk | 1 (side by side), 2 (unified) |
| Navigation | Previous/next difference (and move to the next file at the end); previous/next file; jump to source | none | 1 |
| Apply between sides | Arrows to move a change from one side to the other, append instead of replace, revert on the base side, swap sides | none | 2 |
| Ignore differences | None (default); trim whitespace; ignore whitespace; ignore whitespace and empty lines; ignore imports and formatting **\[IDE\]** | `git diff -w`, `--ignore-blank-lines`, `--ignore-space-at-eol` | 2 |
| Fine highlighting | By words, lines, split changes, characters or none | `git diff --word-diff` | 2 |
| Collapse unchanged | With a context-lines control; align changes side by side; synchronized scrolling | none | 1 |
| Header | The base version's hash with a tooltip (message, author, date) and a click to the log | `git log -1` | 2 |
| Other comparisons | Compare two files, with the clipboard, with an external file, with text pasted into a blank window, add a third file; compare folders (by content, size or date, filters and sync) | `git diff --no-index` | 4 |
| Git comparisons | Show local diff; compare with a revision; with a branch or tag; with local; previous version with local; two versions; HEAD, index and local three-way with an editable index | `git diff <a> <b> -- <path>`, `git show :<path>` | 1–2 |
| External tool | Configure an external diff and merge program with an argument pattern | none | 4 |
| Binary and image files | Detect binaries and limit the diff options [doc] | none | 2 |
| Combined diff | Show the changes of several files in one scrolling view [src] | none | 2 |
| Search in changes | Limit search in the diff to changed text [src] | none | 3 |

### 13. Local History **\[IDE\]**

Automatic file history independent of git: it saves every edit (inside and outside the IDE) and adds automatic labels on tests, deployments and commits, plus manual labels. It shows the history of a file, folder or project with a diff against the current version, reverts a selection or "this and later changes", restores deleted files, searches content across revisions and creates a patch. It is kept for 5 working days by default, counting days when the file changed [doc]. **Scope 4.**

### 14. Miscellaneous

| Feature | What it does | Git equivalent | Scope |
| --- | --- | --- | --- |
| Change markers in the gutter | In the editor, marks for modified, added, deleted and whitespace-only lines; on click: see the previous content, revert, show diff, stage, commit that change | diff against the index or HEAD | 3 (in gikli's file viewer) |
| File status colors | Added, modified, deleted, renamed, copied, ignored, untracked, conflicted; highlight folders with changes and files changed in the last N days | `git status`, `git log --since` | 1 |
| Quick git operations menu | A shortcut opens a menu with the most used git operations | none | 2 |
| Git console | Log of the git operations the app performs and their output, with clear and soft wrap. The help says only that it shows the results of VCS commands. It is read-only in gikli (gikli) | none | 1 |
| Git hooks | Run normally; per-commit and global toggle to skip them; their output is shown when they fail | `--no-verify` | 2 |
| Issue links | Regex → URL patterns (with templates for Jira and YouTrack) that turn references into links in the log, details and blame | none | 3 |
| Other settings | Apply content filters when reading from git; show the branch name in the recent projects list | `git cat-file --filters` | 4 |
| Notifications with actions | Long-lived messages with links: restore a deleted branch, resolve conflicts, abort or continue an operation [doc] | none | 1 |
| Clean untracked files | Not documented by JetBrains (gikli) | `git clean -n`, then `git clean -f -- <paths>` | 4 |
| Bisect | Not documented by JetBrains (gikli) | `git bisect` | 4 |
| Git LFS | Not verified. Repositories using LFS work normally (gikli) | `git lfs` | 3 |
| GPG prompts | Embedded passphrase prompt, with a Configure GPG Agent action [src] | `gpg-agent` | 3 |
| Windows certificate store | Setting to use it for HTTPS, off by default [src] | `http.sslBackend=schannel` | 3 |
| Internal JetBrains actions | Marked internal in the source, so not shown in normal use: Clean Up Branches, Find Merged Local Branches, Update Force-Pushed Branch, Push Selected Commits, Add Commits to Remote Branch, Prefill Recent Push Targets, Show Git Log for Command [src] | see the action names | 4 |
| GitHub | Accounts (token or browser); share a project by creating the repository; open a file, line or commit in the browser; gists; import protection rules; CI column in the log | GitHub | 4 |
| GitHub pull requests | List with filters and sorting; create a PR (bases, title, description from template, reviewers, labels, draft); timeline; checkout the branch; review mode with gutter comments, applicable suggestions, submit review (approve, request changes, comment); merge with merge, squash or rebase | GitHub | 4 |
| GitLab merge requests | The GitLab equivalent | GitLab | 4 |

## Repository conditions gikli handles

gikli works on any repository the user can open, and these conditions must not break it.

| Condition | Expected behavior |
| --- | --- |
| Repository with no commits yet | Changes, staging and the first commit work; the log is empty, and stash-based flows are unavailable [git] |
| Detached HEAD | Shown clearly, with the warning from section 3 |
| SHA-256 repository | 64-character hashes are shown and accepted wherever a hash appears [git] |
| Shallow clone | The graph shows where history is cut, and the app offers to fetch the rest |
| Sparse checkout | Paths outside the sparse set are not reported as deleted |
| Submodules | Shown with their state in the changes view |
| Linked worktrees | State and operations in progress belong to the worktree that is open [git] |
| Unicode and spaces in paths | Paths appear exactly as they are on disk |
| Renames | Shown as renames, with the original path [git] |
| Line endings | Staging and diffs work with `core.autocrlf` [git] |
| User git configuration | Settings such as `apply.whitespace=error`, `log.initialDecorationSet=all`, `diff.noprefix` or `core.commentChar` change no gikli behavior other than the ones they are meant to change [git] |
| Hooks | Run as configured. A hook that needs a terminal fails with a message and does not hang the app |
| Passphrases | SSH passphrase and signing prompts reach the user. gikli stores no passwords |
| Stale `index.lock` | Explained in plain words; removal is offered only when no other git operation is running |
| Dubious ownership | Explained, with an offer to trust the folder after confirmation |
| Long paths on Windows | Reported, recommending `core.longpaths` without setting it silently |
| Changes made outside gikli | Views reflect them shortly after they happen |
| Long operations | Show progress, and the views stay readable while they run |

## IDE-dependent features and how to adapt them

A standalone app has no code analysis, project model or full editor. This is what changes:

| JetBrains feature | Why it depends on the IDE | What gikli does |
| --- | --- | --- |
| Pre-commit checks | Use the IDE's inspections, formatter and run configurations | Only a configurable "run a command before commit" per repository (for example `npm test`), and respect git hooks |
| Ignore imports and formatting in diff | Needs to understand the language | Out of scope; offer git's whitespace options |
| History for a method or class | Needs the syntax tree | History for selection (`git log -L`) covers most of it |
| Code highlighting and navigation in diff | Provided by the IDE editor | Syntax highlighting; no semantic navigation |
| Local History | Needs to watch and store every edit | Out of scope |
| Gutter markers in the editor | Live in the IDE editor | The same actions inside gikli's file viewer and diff |
| Restore the workspace when switching branch | Open files, run configuration, breakpoints | Remember per branch only the app's own state (tabs, filters, selection) |
| Group by module and directory mappings | Depend on the module model | Group by folder; manage a list of open repositories |
| Upload files after commit (FTP/SFTP) | Deployment plugin | Out of scope |
| AI features | AI Assistant plugin | Not in the base product; can be designed later |

## Product scope

Scope 1 is the MVP: open or clone a repository, review changes, organize them in changelists or stage them, commit, browse the log, manage branches, sync with remotes, integrate branches, resolve conflicts, stash and undo. Changelists, the shelf and partial commits are what set gikli apart from other Git clients, so they are part of the product and not extras.

In Scope 1, conflict handling covers the list, picking a side, editing the file by hand and marking it resolved; the 3-pane tool is Scope 2. When local changes block a checkout, Scope 1 shows the blocking files, and Smart Checkout arrives in Scope 2.

## Product identity

This is not legal advice.

- Do not use JetBrains, IntelliJ or other JetBrains product names in the app name, interface, icons or marketing, beyond a truthful reference.
- Do not copy icons, colors, layouts, shortcuts or help text. Use gikli's own wording for branded terms such as "Smart Checkout". Generic terms such as changelist and shelf stay.

## Appendix A: settings and defaults

"Not stated" means the help gives no default. gikli defines its own.

| Setting | JetBrains default | Section |
| --- | --- | --- |
| Path to Git executable, with test | auto-detect; WSL fallback | 1 |
| Enable staging area | off [src] | 2 |
| Warn if CRLF is about to be committed | on [src] | 2 |
| Warn when committing large files | on, 50 MB [src] | 3 |
| Warn about problematic file names | on [src] | 3 |
| Warn when committing in detached HEAD or during rebase | on [src] | 3 |
| Show Push dialog for Commit and Push | on [src] | 3 |
| Show Push dialog only for protected branches | off [src] | 3 |
| Cherry-pick suffix for protected branches | on [src] | 7 |
| Auto-update if push is rejected | off [src]; first use merges | 4 |
| Use Safe Force Push | on [src] | 4 |
| Protected branches (regular expressions) | `master`, `main` [src] | 4 |
| Load branch protection rules from GitHub | not stated | 4 |
| Update method: Merge, Rebase | Merge [src] | 4 |
| Clean working tree using: Stash, Shelve | Shelve [src] | 4, 5 |
| Filter update info by paths | empty | 4 |
| Fetch remote changes automatically | not stated; interval 20 minutes | 4 |
| Check remote for incoming changes | not stated | 4 |
| Fetch tags: Auto, Sync, Always, Never | Auto [src] | 4 |
| Use credential helper | on | 1 |
| Combine stashes and shelves in one tab | not stated | 9 |
| Stash diff against: local version, parent commit | local version | 9 |
| Log sort | by commit date | 6 |
| Log long edges | off | 6 |
| Recent branches shown | 5, fixed | 5 |
| Branch name cleanup symbol | `-` [src] | 5 |
| Recursively clone submodules | on [src] | 1 |
| Skip commit hooks | off [src] | 3 |
| Sign off commits | off [src] | 3 |
| Diff whitespace policy | None | 12 |
| Diff context lines; go to next file after the last change | not stated | 12 |
| Automatically apply non-conflicting changes | not stated | 7 |
| Enable Commit tool window (advanced) | on | 3 |
| Auto-close Commit tool window (advanced) | on | 3 |
| Open diff as editor tab (advanced) | on | 12 |
| Auto-open update info (advanced) | on | 4 |
| Create changelists automatically | not stated | 2 |
| Allow changes within one file in different changelists | not stated | 2 |
| Highlight files from inactive changelists | not stated | 2 |
| Dialog on editing a file from an inactive changelist | not stated | 2 |
| Highlight files with changelist conflicts | not stated | 2 |
| When an empty changelist becomes inactive | not stated | 2 |
| Shelve base revisions | not stated | 9 |
| Shelf location | under the project | 9 |
| Local History retention (advanced) | 5 working days; 0 disables it | 13 |
| Confirmation page: show update dialog, add new files ask/always/never | **\[unconfirmed\]** in this revision | 1, 4 |

## Appendix B: failure behavior

Expected behavior when an operation fails or stops. \[doc\] and \[src\] are JetBrains behavior, \[gikli\] is a gikli product decision.

- **Checkout blocked by local changes.** The client SHALL list the blocking files and offer discard or save, switch and restore \[doc\]. It SHALL restore only what this operation saved \[gikli\]. If the restore conflicts, it SHALL keep the saved changes and open conflict resolution \[gikli\].
- **Checkout blocked by untracked files.** The client SHALL list the files and tell the user to move or commit them \[src\].
- **Merge conflicts.** The client SHALL show the conflicts list with accept ours, accept theirs, merge and resolve all simple \[doc\]. Closing it SHALL leave a persistent entry with a Resolve link and Abort \[doc\].
- **Merge blocked by local changes.** The client SHALL offer stash, merge and unstash \[doc\].
- **Rebase stops** on an edit or a conflict. A persistent notification SHALL offer Continue and Abort, also available in the branch selector \[doc\]. Skip SHALL be available too \[src\].
- **Cherry-pick conflict.** The conflicted changes SHALL appear in local changes, and the status area SHALL offer Abort and Continue \[doc\]. An unfinished sequence SHALL stay visible after a mid-sequence commit \[gikli\].
- **Push rejected as non-fast-forward.** Show the update choice or auto-update, according to the setting \[doc\]. Force push SHALL use `--force-with-lease` and SHALL NOT be available on protected branches \[doc\].
- **Stash pop conflict.** git keeps the entry [git]. The client SHALL open conflict resolution and SHALL NOT drop the entry until the user confirms \[gikli\].
- **Update Project conflict.** The client SHALL stop and resolve, and restore saved local changes only after the integration succeeds \[gikli\]. Submodules in detached HEAD SHALL get `git submodule update` \[doc\].
- **Update of a branch checked out in another worktree.** The client SHALL report that branch and continue with the others \[gikli\].
- **Rewrite actions.** The client SHALL save a backup before undo, drop, squash, reword or reset \[gikli\], and SHALL refuse commits pushed to protected branches \[doc\].
- **Interrupted environment.** `index.lock`, dubious ownership, authentication, host key, hook and signing failures SHALL each show a dedicated message, never a raw stderr dump \[gikli\].

## Git behavior to account for

Observed on 4 November 2026 with git 2.43.0 on Linux, on real repositories. Windows, macOS and other git versions were not observed. Each fact shapes a behavior defined above.

- In a repository with no commits, `HEAD` does not resolve, `git restore --source=HEAD` fails, and `git stash push` exits 1.
- `git stash push` on a clean tree, or with only untracked files, exits 0 and saves nothing.
- A tracked stash does not unblock a checkout stopped by an untracked file. `stash -u` does, but `stash pop` on a target branch holding that file exits 1 and keeps the entry.
- `stash pop` with a conflict exits 1 and keeps the entry.
- `git commit -F` keeps lines starting with `#`, and `--cleanup=strip` removes them.
- `git commit --only` keeps other staged paths and fails during a merge.
- `git fetch --all --prune --prune-tags` deleted a tag present on only one of two remotes.
- `git fetch <remote> <remote-branch>:<local-branch>` fails when the branch is checked out in a worktree.
- `git clone --depth 1` tracks a single branch, and `git fetch --unshallow` keeps it that way; `--no-single-branch` tracks them all.
- A commit in the middle of a cherry-pick sequence leaves the sequence unfinished.
- `git merge --autostash` followed by `--abort` restores the local changes and leaves no stash entry.
- `git rebase --onto <sha>^ <sha>` flattens later merges, and `--rebase-merges` keeps them.
- `git reset --soft HEAD~1` fails on the root commit, and deleting `HEAD` leaves the changes staged.
- `git check-ref-format --branch` expands `@{-1}` to a real branch name, and rejects `-x`, `a b`, `a..b`, `HEAD` and `x.lock`.

## Sources

Official IntelliJ IDEA 2026.2 help, read on 16 October and 4 November 2026. The separate Commit tool window reference page no longer exists; its behavior is covered by the commit pages and Advanced Settings.

- [Set up a Git repository](https://www.jetbrains.com/help/idea/set-up-a-git-repository.html) and [add files](https://www.jetbrains.com/help/idea/adding-files-to-version-control.html)
- [Commit and push](https://www.jetbrains.com/help/idea/commit-and-push-changes.html), [changelists](https://www.jetbrains.com/help/idea/managing-changelists.html) and [undo changes](https://www.jetbrains.com/help/idea/undo-changes.html)
- [Commit settings](https://www.jetbrains.com/help/idea/settings-version-control-commit.html) and [GPG signing](https://www.jetbrains.com/help/idea/set-up-GPG-commit-signing.html)
- [Sync with a remote](https://www.jetbrains.com/help/idea/sync-with-a-remote-repository.html) and [Git settings](https://www.jetbrains.com/help/idea/settings-version-control-git.html)
- [Manage branches](https://www.jetbrains.com/help/idea/manage-branches.html) and [tags](https://www.jetbrains.com/help/idea/use-tags-to-mark-specific-commits.html)
- [Log tab](https://www.jetbrains.com/help/idea/log-tab.html), [investigate changes](https://www.jetbrains.com/help/idea/investigate-changes.html) and [log settings](https://www.jetbrains.com/help/idea/settings-version-control-log.html)
- [Apply changes from one branch to another](https://www.jetbrains.com/help/idea/apply-changes-from-one-branch-to-another.html) and [resolve conflicts](https://www.jetbrains.com/help/idea/resolve-conflicts.html)
- [Edit project history](https://www.jetbrains.com/help/idea/edit-project-history.html)
- [Shelve and stash](https://www.jetbrains.com/help/idea/shelving-and-unshelving-changes.html) and [patches](https://www.jetbrains.com/help/idea/using-patches.html)
- [Worktrees](https://www.jetbrains.com/help/idea/use-git-worktrees.html)
- [Differences viewer](https://www.jetbrains.com/help/idea/differences-viewer.html) and [compare file versions](https://www.jetbrains.com/help/idea/comparing-file-versions.html)
- [Local History](https://www.jetbrains.com/help/idea/local-history.html)
- [GitHub pull requests](https://www.jetbrains.com/help/idea/work-with-github-pull-requests.html) and [GitLab merge requests](https://www.jetbrains.com/help/idea/create-and-merge-gitlab-merge-requests.html)
- [Push dialog](https://www.jetbrains.com/help/idea/push-dialog-mercurial-git.html), [Diff and Merge settings](https://www.jetbrains.com/help/idea/settings-tools-diff-and-merge.html) and [Version Control window](https://www.jetbrains.com/help/idea/version-control-tool-window.html)
- [Speeding up interactive rebase in JetBrains IDEs](https://blog.jetbrains.com/platform/2026/04/speeding-up-interactive-rebase-in-jetbrains-ides/), JetBrains blog, April 2026
- [IntelliJ IDEA 2026.3 latest builds](https://youtrack.jetbrains.com/articles/IDEA-A-2100662737/IntelliJ-IDEA-2026.3-Latest-Builds)
- [Changelist settings](https://www.jetbrains.com/help/idea/settings-version-control-changelist-conflicts.html)
- [intellij-community](https://github.com/JetBrains/intellij-community), action definitions and default settings of the Git and VCS modules
- [git documentation](https://git-scm.com/docs)
