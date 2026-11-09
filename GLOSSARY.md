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
Discarding the uncommitted changes of the selected files once the user has confirmed the list of what goes.
_Avoid_: Revert (that is git's operation on commits), reset

**Shelf**:
gikli's own store of named sets of changes, kept outside git, that the user can put back later. Each set is a shelved change.
_Avoid_: Stash (that is git's own mechanism), drafts

**Saved local changes**:
The uncommitted changes that gikli puts aside before an operation that needs a clean working tree and restores when the operation finishes, together with their Changelists.
_Avoid_: Auto-stash, temporary shelf

## Authentication

**Prompt**:
Something git, ssh or a program they start asks the user while an operation waits, such as a password or the passphrase of a key. It reaches the user inside gikli, and gikli keeps no answer.
_Avoid_: Question, credential dialog

## Safety

**Backup**:
The state of a branch saved by gikli before a history rewrite, so the rewrite can be undone. It expires after 30 days.
_Avoid_: Snapshot, restore point
