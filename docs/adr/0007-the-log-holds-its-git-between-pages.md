# The log holds its git between pages

The log reads a Repository's history with one `git log --date-order` that stays running while the user reads it. Each page takes the next commits from that git, and the git waits for gikli to read on. Without a commit-graph file, git goes through the whole history before it lists the first commit in date order, which takes most of a second on a few hundred thousand commits, and `--skip` makes it do that again for every page. A held git does it once, so only the first page waits for it.

## Consequences

- gikli holds at most four reads of the log at once, the oldest dropped first, and ends a read's git when the log no longer needs it: when the user closes the log or it starts over, when it reaches the last commit, and when the history changed. A page whose read gikli let go of still comes, from a new git that skips the commits before it, only as slowly as the first page.
- While the log is open, its git keeps the Repository's pack files open, as `git log` in a terminal pager does. On Windows a `git gc` cannot delete those packs until the log closes.
- The operation log records the read once its git ends, with the output gikli read from it.

## Considered Options

- **A new `git log --skip` for each page**: every page takes as long as the first, far over the budget of a later page.
- **Writing a commit-graph file for the user**: git would list the first commits at once, but gikli would change the Repository without being asked, and the first open would wait for the file to be written.
- **Reading the whole history into memory in the background**: the git ends at once, but gikli holds the text of every commit, hundreds of megabytes on the largest repositories, when the user reads only a few pages.
