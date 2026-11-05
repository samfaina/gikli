# Changelists live in each Repository's git directory

gikli stores Changelists inside the git directory of each Repository: `.git/gikli/` for the main working tree and `.git/worktrees/<name>/gikli/` for a linked worktree. A Changelist that spans several Repositories is stored as one part per Repository under the same identifier. Changelists then follow the Repository when its folder moves or another Project opens it, stay out of `git status`, belong to the worktree whose changes they hold, and survive reinstalling gikli.

The Shelf follows the same rule: by default it lives in `.git/gikli/shelf/` of each Repository, a shelved change that spans Repositories is stored as one part per Repository, and the "Shelf location" setting can move it to a folder the user chooses, for example to share it.

## Considered Options

- **A `.gikli/` folder in the Project**, like JetBrains' `.idea/`: it shows up in `git status` unless it is excluded, and it ties Changelists to one Project.
- **The application data directory, keyed by Project path**: Changelists are lost when the folder moves, and the data is invisible to a user looking at the repository.
