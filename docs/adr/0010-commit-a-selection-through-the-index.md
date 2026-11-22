# A commit of a selection goes through the index, not `git commit --only`

In Changelist mode gikli commits the selected changes by preparing the index and running `git commit` with no paths: it takes out of the index what is staged at paths the user did not select, adds the selected paths as they are in the working tree, commits, and stages again what it took out. If the commit fails, the index goes back to what it was. This holds whether or not an Operation in progress is there. git refuses `git commit --only <paths>` during a merge or a cherry-pick ("cannot do a partial commit during a merge", "... during a cherry-pick"), while a commit with no paths is allowed then, so one way of committing covers every state. It is also how the JetBrains IDEs commit a selection (`GitCheckinEnvironment.stageAndCommit` in `intellij-community`), whose behavior gikli reproduces.

## Consequences

- A commit holds exactly the selected changes, during an Operation in progress too. A change the operation brought that the user leaves out stays a local change, and a merge commit then keeps the version of HEAD at that path. During a merge gikli asks for confirmation before leaving any change out; during a cherry-pick it does not, as the JetBrains IDEs do.
- A commit from the commit panel is refused while any Conflict is left in the Repository, as `git commit` with no paths refuses unmerged entries. Before this decision, a revert stopped on a Conflict let the user commit other files with `--only`.

## Considered Options

- **`git commit --only` outside a merge or cherry-pick, the index way only during one**: keeps the commit that already worked, at the cost of two ways of committing a selection that behave differently around an Operation in progress.
- **During a merge or cherry-pick, always commit what the operation brought, with the selection deciding only the other local changes**: safer for a merge commit, but a selection would then mean something else during an operation, and the JetBrains IDEs do not work that way.
- **Hiding the selection during a merge and committing everything**: what `docs/reference/jetbrains-git.md` said before. It sweeps unrelated local changes into the merge commit.
