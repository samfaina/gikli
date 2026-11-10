//! The log: the history of a Repository as a commit list with a graph, read a page at a time.

use serde::{Deserialize, Serialize};

use crate::git::GitRunner;
use crate::{Failure, Repository, commit, failure_of, git_failed_to_start};

/// One page of the log, newest commit first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    pub repository: Repository,
    pub rows: Vec<LogRow>,
    /// Where the next page starts. None on the last page.
    pub next: Option<LogCursor>,
}

/// One commit of the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRow {
    pub hash: String,
    /// The hashes of its parents, the first parent first.
    pub parents: Vec<String>,
    pub subject: String,
    pub author: String,
    /// When it was committed, in seconds since the epoch.
    pub committed_at: i64,
    pub graph: GraphRow,
}

/// How one row of the graph is drawn. A column is the place of a lane, counted from the left. The
/// lines below a row meet the lines above the next one in the same columns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRow {
    /// The column of the commit's node.
    pub column: usize,
    /// The lines from the top of the row to the height of the node, by column.
    pub above: Vec<GraphLine>,
    /// The lines from the height of the node to the bottom of the row, by column.
    pub below: Vec<GraphLine>,
}

/// One line of a [`GraphRow`], from a column at its start to a column at its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphLine {
    pub from: usize,
    pub to: usize,
}

/// Where a page of the log starts. It holds the lanes of the graph at that point, so a page is laid
/// out without going through the ones before it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogCursor {
    /// How many commits came before this point.
    seen: usize,
    /// The hash of the commit just before it.
    last: String,
    lanes: Vec<Option<String>>,
}

/// What git prints of each commit, a field per NUL. `-z` ends each commit with one more.
const FORMAT: &str = "--format=%H%x00%P%x00%ct%x00%an%x00%s";
const FIELDS: usize = 5;

/// A page of the log of `repository`: at most `count` commits, and at least one when there are any,
/// after `after` or from the newest commit.
///
/// A page after another one asks git for the last commit it showed again. When git no longer has it
/// there, what lies behind it has changed too, and this returns [`Failure::LogChanged`] rather than
/// going on with lanes that no longer fit.
pub(crate) fn read(
    runner: &GitRunner,
    repository: &Repository,
    after: Option<&LogCursor>,
    count: usize,
) -> Result<LogPage, Failure> {
    let count = count.max(1);
    let root = &repository.root;
    let seen = after.map_or(0, |cursor| cursor.seen);
    let skip = format!("--skip={}", seen.saturating_sub(1));
    // One more than the page, to know whether another comes after it.
    let max_count = format!("--max-count={}", count + usize::from(after.is_some()) + 1);
    let mut args = vec![
        "log",
        "--date-order",
        "--encoding=UTF-8",
        "-z",
        FORMAT,
        &skip,
        &max_count,
        "--branches",
        "--remotes",
        "--tags",
    ];
    // An unborn HEAD is no revision git can start from.
    if commit::has_commits(runner, root)? {
        args.push("HEAD");
    }
    args.push("--");
    let invocation = runner.run(root, &args).map_err(git_failed_to_start)?;
    if !invocation.success {
        let unreadable = Failure::LogUnreadable {
            folder: root.clone(),
        };
        return Err(failure_of(runner, root, &invocation, unreadable));
    }
    let text = invocation.stdout_text();
    let fields: Vec<&str> = text.split_terminator('\0').collect();
    let mut commits = fields.chunks_exact(FIELDS);
    if let Some(cursor) = after
        && commits.next().map(|commit| commit[0]) != Some(cursor.last.as_str())
    {
        return Err(Failure::LogChanged);
    }
    let commits: Vec<&[&str]> = commits.collect();
    let mut lanes = Lanes(after.map_or_else(Vec::new, |cursor| cursor.lanes.clone()));
    let rows: Vec<LogRow> = commits
        .iter()
        .take(count)
        .map(|commit| row_of(commit, &mut lanes))
        .collect();
    let next = rows
        .last()
        .filter(|_| commits.len() > count)
        .map(|last| LogCursor {
            seen: seen + rows.len(),
            last: last.hash.clone(),
            lanes: lanes.0,
        });
    Ok(LogPage {
        repository: repository.clone(),
        rows,
        next,
    })
}

fn row_of(fields: &[&str], lanes: &mut Lanes) -> LogRow {
    let hash = fields[0].to_owned();
    let parents: Vec<String> = fields[1].split_whitespace().map(str::to_owned).collect();
    LogRow {
        graph: lanes.place(&hash, &parents),
        hash,
        parents,
        committed_at: fields[2].parse().unwrap_or_default(),
        author: fields[3].to_owned(),
        subject: fields[4].to_owned(),
    }
}

/// The lanes of the graph at some row, each with the commit it runs down to. The layout goes from
/// the newest commit down, so this is all it has to remember between one row and the next.
struct Lanes(Vec<Option<String>>);

impl Lanes {
    /// Draws the row of the commit `hash`, and leaves the lanes as the next row finds them.
    fn place(&mut self, hash: &str, parents: &[String]) -> GraphRow {
        let waiting = self.lane_of(hash);
        let column = waiting.unwrap_or_else(|| self.free_lane_from(0));
        let above = self.straight_lines();
        if let Some(lane) = waiting {
            self.0[lane] = None;
        }
        let mut below = self.straight_lines();
        for (index, parent) in parents.iter().enumerate() {
            let to = self.lane_for_parent(parent, index == 0, column, &mut below);
            below.push(GraphLine { from: column, to });
        }
        below.sort_unstable();
        while self.0.last().is_some_and(Option::is_none) {
            self.0.pop();
        }
        GraphRow {
            column,
            above,
            below,
        }
    }

    /// The lane `parent` of the commit in `column` runs in. A first parent keeps to the column of
    /// its commit, even when a lane further right was already waiting for it, whose line in `below`
    /// turns to that column. So the first-parent line of a branch stays straight.
    fn lane_for_parent(
        &mut self,
        parent: &str,
        first: bool,
        column: usize,
        below: &mut [GraphLine],
    ) -> usize {
        if let Some(lane) = self.lane_of(parent) {
            if !first || lane < column {
                return lane;
            }
            self.0[lane] = None;
            if let Some(line) = below.iter_mut().find(|line| line.from == lane) {
                line.to = column;
            }
        }
        let lane = if first {
            column
        } else {
            self.free_lane_from(column + 1)
        };
        self.occupy(lane, parent);
        lane
    }

    /// A line straight down each lane something runs in.
    fn straight_lines(&self) -> Vec<GraphLine> {
        let running = self.0.iter().enumerate().filter(|(_, lane)| lane.is_some());
        running
            .map(|(lane, _)| GraphLine {
                from: lane,
                to: lane,
            })
            .collect()
    }

    /// The lane waiting for the commit `hash`. There is one at most: a parent something already
    /// waits for joins the lane that waits for it.
    fn lane_of(&self, hash: &str) -> Option<usize> {
        self.0
            .iter()
            .position(|awaited| awaited.as_deref() == Some(hash))
    }

    /// The first lane from `start` on that nothing runs in.
    fn free_lane_from(&self, start: usize) -> usize {
        (start..self.0.len())
            .find(|&lane| self.0[lane].is_none())
            .unwrap_or(self.0.len().max(start))
    }

    fn occupy(&mut self, lane: usize, hash: &str) {
        if lane >= self.0.len() {
            self.0.resize(lane + 1, None);
        }
        self.0[lane] = Some(hash.to_owned());
    }
}
