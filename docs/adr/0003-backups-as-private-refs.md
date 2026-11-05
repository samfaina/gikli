# Backups are refs in a private namespace

Before any history rewrite (undo, drop, squash, reword, reset and the rest of section 8 of the reference), gikli writes a Backup as a ref under `refs/gikli/backups/`. gikli keeps those refs out of the log and the branch lists and deletes them after 30 days. Refs protect the objects from `git gc`, survive uninstalling gikli, are not sent by a normal push, and any other git tool can read them, so a backup remains recoverable with plain git.

## Considered Options

- **A gikli-owned file listing SHAs**: depends on the reflog and `gc` to keep the objects alive, so a backup can silently disappear.
- **Bundles on disk**: heavy, and invisible to other git tools.
