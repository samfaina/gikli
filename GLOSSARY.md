# gikli

gikli is a standalone desktop Git client that reproduces the behavior of the Git integration in JetBrains IDEs, with changelists, the shelf and partial commits at its core.

## Workspace

**Project**:
The folder the user opens in gikli. It holds one or more Repositories and is the scope of every view.
_Avoid_: Workspace, solution

**Repository**:
One git repository inside a Project, identified by its root folder.
_Avoid_: Root, repo (in UI text)

## Organizing changes

**Changelist**:
A named group of uncommitted changes that belongs to gikli and that git does not know about. It can be committed on its own.
_Avoid_: Group, set, batch

**Active changelist**:
The one Changelist that new changes go into. There is exactly one at a time.
_Avoid_: Default changelist, current changelist

**Unversioned Files**:
The group that shows new files git does not track yet. It is not a Changelist, and a file leaves it when it is committed.
_Avoid_: Untracked changelist, new files list

**Changelist mode**:
The default way of organizing uncommitted work, where changes are grouped into Changelists.
_Avoid_: Normal mode

**Staging mode**:
The alternative to Changelist mode, where the git index is shown as Staged and Unstaged changes. Changelists are kept and come back when the user switches back.
_Avoid_: Index mode

**Rollback**:
Discarding the uncommitted changes of the selected files once the user has confirmed the rollback plan, the list of what goes.
_Avoid_: Revert (that is git's operation on commits), reset

**Commit identity**:
The name and email git writes in a commit, from `user.name` and `user.email`. gikli asks for them when they are missing.
_Avoid_: Author, user info

**Shelf**:
gikli's own store of named sets of changes, kept outside git, that the user can put back later. Each set is a shelved change.
_Avoid_: Stash (that is git's own mechanism), drafts

**Saved local changes**:
The uncommitted changes that gikli puts aside before an operation that needs a clean working tree and restores when the operation finishes, together with their Changelists.
_Avoid_: Auto-stash, temporary shelf

## Synchronization

**Upstream**:
The branch a local branch is compared with and pulls from, usually a remote branch such as `origin/main`. It is gone when it is configured and no longer exists.
_Avoid_: Tracking branch, remote (that is the repository it comes from)

**Incoming commits**:
The commits of the upstream that the local branch does not have yet: what a pull would bring.
_Avoid_: Behind (git's word for it)

**Outgoing commits**:
The commits of the local branch that its upstream does not have yet: what a push would send.
_Avoid_: Ahead (git's word for it), unpushed

**Status area**:
The strip at the foot of every window that always shows what HEAD of each Repository is on, with the incoming and outgoing commits of the current branch.
_Avoid_: Status bar, footer

**Cut history**:
The history of a Repository that stops short of its first commits, as after a shallow clone: git holds some commits without their parents, and the log marks each one. Fetching the rest of the history brings the older commits and makes the history whole.
_Avoid_: Truncated history, partial history, grafts (an old git mechanism)

## Branches

**Branch selector**:
The list that opens from what HEAD is on in the Status area, with every branch and tag of a Repository in the groups Recent, Local, Remote and Tags, and a search across them.
_Avoid_: Branch popup, branch menu

**Recent branches**:
The local branches the user checked out last, up to 5, newest first, as the reflog of HEAD records the checkouts. The current branch is not among them.
_Avoid_: Recently used, history

**Branches pane**:
The list of local and remote branches beside the log, where the user filters the log by a branch, goes to its latest commit, and renames and deletes branches.
_Avoid_: Branch tree, sidebar

## Authentication

**Prompt**:
Something git, ssh or a program they start asks the user while an operation waits, such as a password or the passphrase of a key. It reaches the user inside gikli, and gikli keeps no answer.
_Avoid_: Question, credential dialog

## Safety

**Backup**:
The state of a branch saved by gikli before a history rewrite, so the rewrite can be undone. It expires after 30 days.
_Avoid_: Snapshot, restore point
