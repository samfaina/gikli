# gikli drives the user's git executable

Every git operation runs through the `git` executable that gikli detects or the user configures, WSL git included, instead of an embedded library such as libgit2, gitoxide or JGit. The product promises that hooks, credential helpers, signing, `core.autocrlf`, LFS, sparse checkout and the rest of the user's configuration work as configured, and no library reproduces all of that faithfully. The reference also defines behavior in terms of observed git commands.

## Consequences

- Reads that turn out too slow for large logs may later use a library, but only for reads. Writes always go through the CLI.
- gikli parses git output, so every invocation must pin the output format (porcelain formats, `-z`, explicit `--format`) and must not depend on the user's presentation settings such as `log.initialDecorationSet` or `diff.noprefix`.
