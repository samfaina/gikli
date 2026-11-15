//! The log: the history of a Repository as a commit list with a graph, read a page at a time.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::git::{GitRunner, GitStream, Invocation};
use crate::{Failure, Repository, commit, failure_of, git_failed_to_start, lock};

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
    /// The hashes of its parents, the first parent first. The left arrow goes to the first.
    pub parents: Vec<String>,
    /// The hashes of its children, newest first, except that the one in its own column of the
    /// graph, whose line runs straight down to it, comes first. The right arrow goes to the first.
    pub children: Vec<String>,
    pub subject: String,
    pub author: String,
    /// When it was committed, in seconds since the epoch.
    pub committed_at: i64,
    pub graph: GraphRow,
    /// The references that point at it, HEAD first, then local branches, remote branches and
    /// tags, each kind by name. Stashes and gikli's own refs are not among them.
    pub labels: Vec<RefLabel>,
    /// Whether HEAD reaches it, so it is part of the current branch.
    pub on_current_branch: bool,
    /// Whether the user wrote it: its author has the email of their Commit identity.
    pub own: bool,
    /// Whether its author and its committer are not the same name and email.
    pub committer_differs: bool,
}

/// A reference shown on the commit it points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefLabel {
    /// Its name as the user knows it: `HEAD`, `main`, `origin/main`, `v1`.
    pub name: String,
    pub kind: RefKind,
}

/// What a [`RefLabel`] names. Each kind shows in its own color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefKind {
    /// HEAD itself, on the commit it points at.
    Head,
    /// A local branch.
    Local,
    /// A remote branch.
    Remote,
    /// A tag, lightweight or annotated.
    Tag,
}

impl RefKind {
    /// Whether it is a local or a remote branch.
    pub(crate) fn is_branch(self) -> bool {
        matches!(self, Self::Local | Self::Remote)
    }
}

/// What the log shows of the history.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LogFilter {
    /// The branches whose commits the log shows. When there are none, it shows the commits of
    /// every branch, remote branch and tag, and of HEAD.
    pub branches: Vec<RefLabel>,
    /// The text the commits shown match, if any. A search shows them without their graph.
    pub text: Option<TextSearch>,
}

/// A search of the log for the commits whose message, author or hash match a text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextSearch {
    /// What the user typed.
    pub text: String,
    /// Whether the text is a regular expression. Otherwise a commit matches when its message or
    /// author holds the text, or its hash starts with it.
    pub regex: bool,
    /// Whether a match must have the same case as the text.
    pub match_case: bool,
}

/// What tells the commits a [`TextSearch`] finds.
struct Matcher {
    pattern: Regex,
    /// The text in lowercase, which a hash starts with, for a search that is no regular expression.
    hash_prefix: Option<String>,
}

impl Matcher {
    /// The matcher of `search`, or None when it has no text, so it finds every commit. A regular
    /// expression gikli cannot read returns [`Failure::SearchPatternInvalid`].
    fn new(search: &TextSearch) -> Result<Option<Self>, Failure> {
        if search.text.is_empty() {
            return Ok(None);
        }
        let pattern = if search.regex {
            search.text.clone()
        } else {
            regex::escape(&search.text)
        };
        // `^` and `$` match at the start and end of each line, as in git's own search.
        let pattern = RegexBuilder::new(&pattern)
            .case_insensitive(!search.match_case)
            .multi_line(true)
            .build()
            .map_err(|error| Failure::SearchPatternInvalid {
                message: error.to_string(),
            })?;
        Ok(Some(Self {
            pattern,
            hash_prefix: (!search.regex).then(|| search.text.trim().to_lowercase()),
        }))
    }

    /// Whether the commit git printed as `commit` matches.
    fn matches(&self, commit: &Commit) -> bool {
        let [hash, _, _, author, _, _, _, _, message] = commit;
        let hash_matches = match &self.hash_prefix {
            Some(prefix) => !prefix.is_empty() && hash.starts_with(prefix.as_str()),
            None => self.pattern.is_match(hash),
        };
        hash_matches || self.pattern.is_match(author) || self.pattern.is_match(message)
    }
}

impl LogFilter {
    /// The revisions git lists the log from with this filter, given the references there are now:
    /// None when it names only branches that are gone, so the log shows nothing.
    fn revisions(&self, references: &References) -> Option<Vec<String>> {
        if self.branches.is_empty() {
            let mut whole = vec!["--branches", "--remotes", "--tags"];
            // An unborn HEAD is no revision git can start from.
            if references.head.is_some() {
                whole.push("HEAD");
            }
            return Some(whole.into_iter().map(str::to_owned).collect());
        }
        let existing: Vec<String> = self
            .branches
            .iter()
            .filter(|branch| references.commit_of(branch).is_some())
            .map(RefLabel::full_name)
            .collect();
        (!existing.is_empty()).then_some(existing)
    }

    /// The commits from which the log marks those of the current branch, which HEAD at `head`
    /// reaches. Without a branch filter it is HEAD alone. With one, HEAD may be no commit the log
    /// shows, so they are where the history of each branch shown meets HEAD's.
    fn current_branch_tips(
        &self,
        runner: &GitRunner,
        repository: &Repository,
        references: &References,
    ) -> Result<Vec<String>, Failure> {
        let Some(head) = &references.head else {
            return Ok(Vec::new());
        };
        let mut tips = vec![head.clone()];
        for branch in &self.branches {
            let Some(commit) = references.commit_of(branch) else {
                continue;
            };
            let bases = runner
                .run(&repository.root, &["merge-base", "--all", head, commit])
                .map_err(git_failed_to_start)?;
            // git fails, with nothing printed, when the histories never meet.
            tips.extend(bases.stdout_text().lines().map(str::to_owned));
        }
        Ok(tips)
    }
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
    /// The edges to children far above, cut short: an arrow from the top of the row to the node
    /// for each, which leads to the child.
    pub arrows_up: Vec<GraphArrow>,
    /// The edges to parents far below, cut short: an arrow from the node to the bottom of the row
    /// for each, which leads to the parent.
    pub arrows_down: Vec<GraphArrow>,
}

impl GraphRow {
    /// The row of a node with no lines.
    fn node_alone() -> Self {
        Self {
            column: 0,
            above: Vec::new(),
            below: Vec::new(),
            arrows_up: Vec::new(),
            arrows_down: Vec::new(),
        }
    }
}

/// The end of an edge too long to draw whole, which leads to the commit at its other end. A long
/// edge spans more than [`LONG_EDGE`] rows. Unless the user shows long edges, it is drawn as an
/// arrow at each end, and it holds no lane between them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphArrow {
    /// The column the arrow ends in, at the top or the bottom of the row. No line of the row meets
    /// that end.
    pub column: usize,
    /// The hash of the commit at the other end of the edge.
    pub target: String,
}

/// The most commits a page of a search goes through. A page that finds fewer than it was asked for
/// in as many ends there, with more to come, so the log stays responsive while a search that finds
/// little goes through a long history, and a newer search does not wait for it.
pub const LOG_SEARCH_BATCH: usize = 10_000;

/// The most rows an edge spans and is still drawn whole when long edges are off.
pub const LONG_EDGE: usize = 30;

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
    /// The read of the log this page belongs to, which [`LogReads`] may still hold.
    read: u64,
    /// A digest of where the log started from, which tells whether the history moved since.
    tips: String,
    /// Whether long edges are drawn whole, as when the log started, so the graph goes on as it was.
    show_long_edges: bool,
    /// What the log shows, as when it started.
    filter: LogFilter,
    walk: Walk,
}

/// What the log remembers from one row to the next. It reads from the newest commit down, and git
/// lists every commit after all of its children, so what a row needs to know of the rows above it
/// is what they left waiting for the commits still to come.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Walk {
    /// The lanes of the graph.
    lanes: Lanes,
    /// The commits still to come that HEAD reaches: HEAD's own, and the parents of each one
    /// reached.
    on_current_branch: BTreeSet<String>,
    /// The children seen of each commit still to come, newest first.
    children: BTreeMap<String, Vec<Child>>,
}

/// A child seen of a commit still to come.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Child {
    hash: String,
    /// The column of its node, where its edge to the commit starts. None when that edge is cut.
    column: Option<usize>,
}

impl Walk {
    /// The walk at the newest commit, where the commits of the current branch start from
    /// `current_branch_tips`.
    fn from_tips(current_branch_tips: Vec<String>) -> Self {
        Self {
            on_current_branch: current_branch_tips.into_iter().collect(),
            ..Self::default()
        }
    }

    /// Goes past the commit git printed as `fields`: returns its row, and leaves the walk as the
    /// next row finds it. `user_email` is the email of the user's Commit identity, if they have
    /// one, and `is_long` tells whether the edge to a parent is cut. Unless `with_graph`, the row
    /// has no lines of the graph, only its node, as in a search, whose commits do not follow one
    /// another.
    fn row(
        &mut self,
        fields: &[&str; FIELDS],
        labels: &HashMap<String, Vec<RefLabel>>,
        user_email: Option<&str>,
        is_long: impl Fn(&str) -> bool,
        with_graph: bool,
    ) -> LogRow {
        let [
            hash,
            parents,
            committed_at,
            author,
            author_email,
            committer,
            committer_email,
            subject,
            _message,
        ] = *fields;
        let parents: Vec<String> = parents.split_whitespace().map(str::to_owned).collect();
        let on_current_branch = self.on_current_branch.remove(hash);
        if on_current_branch {
            self.on_current_branch.extend(parents.iter().cloned());
        }
        let mut children = self.children.remove(hash).unwrap_or_default();
        let children_cut = children
            .iter()
            .filter(|child| child.column.is_none())
            .map(|child| child.hash.clone())
            .collect();
        let graph = if with_graph {
            self.lanes.place(hash, &parents, is_long, children_cut)
        } else {
            GraphRow::node_alone()
        };
        // The child in the same column, with its edge drawn, has its line run straight down here.
        if let Some(on_its_line) = children
            .iter()
            .position(|child| child.column == Some(graph.column))
        {
            let child = children.remove(on_its_line);
            children.insert(0, child);
        }
        for parent in &parents {
            let cut = graph
                .arrows_down
                .iter()
                .any(|arrow| &arrow.target == parent);
            let child = Child {
                hash: hash.to_owned(),
                column: (with_graph && !cut).then_some(graph.column),
            };
            self.children.entry(parent.clone()).or_default().push(child);
        }
        LogRow {
            labels: labels.get(hash).cloned().unwrap_or_default(),
            on_current_branch,
            children: children.into_iter().map(|child| child.hash).collect(),
            hash: hash.to_owned(),
            parents,
            committed_at: committed_at.parse().unwrap_or_default(),
            own: user_email.is_some_and(|email| email.eq_ignore_ascii_case(author_email)),
            committer_differs: (author, author_email) != (committer, committer_email),
            author: author.to_owned(),
            subject: subject.to_owned(),
            graph,
        }
    }
}

/// What git prints of each commit, a field per NUL, the last one, the whole message, left empty.
/// `-z` ends each commit with one more.
const FORMAT: &str = "--format=%H%x00%P%x00%ct%x00%an%x00%ae%x00%cn%x00%ce%x00%s%x00";
/// What git prints of each commit for a search, which also reads the whole message.
const SEARCH_FORMAT: &str = "--format=%H%x00%P%x00%ct%x00%an%x00%ae%x00%cn%x00%ce%x00%s%x00%B";
const FIELDS: usize = 9;

/// One commit as git printed it, a field of [`FORMAT`] each.
type Commit = [String; FIELDS];

/// The most reads of the log gikli holds at once. Each is a git waiting to list more commits, and
/// the oldest goes first.
const HELD_READS: usize = 4;

/// The reads of the log gikli holds between pages. Each is a git that lists the commits after the
/// page before, so a later page neither starts git again nor waits for it to go through the
/// history before the page, which on a long history takes far longer than the page itself. A read
/// gikli let go of starts again from where its last page left off (ADR-0007).
#[derive(Default)]
pub(crate) struct LogReads(Mutex<HeldReads>);

#[derive(Default)]
struct HeldReads {
    /// The number of the next read held.
    next: u64,
    /// The reads held, the oldest first, each with its number.
    reads: VecDeque<(u64, LogRead)>,
}

impl LogReads {
    /// Lets go of the read the page before `cursor` belongs to, which ends its git.
    pub fn end(&self, cursor: &LogCursor) {
        let ended = {
            let mut held = lock(&self.0);
            let at = held.reads.iter().position(|(read, _)| *read == cursor.read);
            at.and_then(|at| held.reads.remove(at))
        };
        // Ended without holding the other reads back.
        drop(ended);
    }

    /// The read that goes on from `cursor` in the Repository at `root`, when gikli holds it. It is
    /// no longer held: the page that goes on from it holds it again.
    fn take(&self, cursor: &LogCursor, root: &Path) -> Option<LogRead> {
        let mut held = lock(&self.0);
        let at = held
            .reads
            .iter()
            .position(|(number, read)| *number == cursor.read && read.goes_on_from(cursor, root))?;
        held.reads.remove(at).map(|(_, read)| read)
    }

    /// Holds `read` for the next page, and returns its number.
    fn hold(&self, read: LogRead) -> u64 {
        let (number, dropped) = {
            let mut held = lock(&self.0);
            let number = held.next;
            held.next += 1;
            held.reads.push_back((number, read));
            let dropped = (held.reads.len() > HELD_READS)
                .then(|| held.reads.pop_front())
                .flatten();
            (number, dropped)
        };
        // Ended without holding the other reads back.
        drop(dropped);
        number
    }
}

/// One read of the log: a git listing commits in the order the log shows them, and what it listed
/// beyond the pages laid out so far.
struct LogRead {
    root: PathBuf,
    /// None once git listed every commit.
    git: Option<GitStream>,
    /// The commits git listed that no page laid out yet, in its order.
    ahead: VecDeque<Commit>,
    /// How many commits came before those ahead.
    seen: usize,
    /// The email of the user's Commit identity when the read started, if they had one.
    user_email: Option<String>,
}

impl LogRead {
    /// Starts git listing the commits of `repository` after the first `seen`, from `revisions`,
    /// each with its whole message when `with_message`.
    fn start(
        runner: &GitRunner,
        repository: &Repository,
        seen: usize,
        revisions: &[String],
        with_message: bool,
    ) -> Result<Self, Failure> {
        let skip = format!("--skip={seen}");
        let format = if with_message { SEARCH_FORMAT } else { FORMAT };
        let mut args = vec![
            "log",
            "--date-order",
            "--encoding=UTF-8",
            "-z",
            format,
            &skip,
        ];
        args.extend(revisions.iter().map(String::as_str));
        args.push("--");
        let git = runner
            .stream(&repository.root, &args)
            .map_err(git_failed_to_start)?;
        Ok(Self {
            root: repository.root.clone(),
            git: Some(git),
            ahead: VecDeque::new(),
            seen,
            user_email: None,
        })
    }

    /// Whether this read goes on from `cursor` in the Repository at `root`: its next commit is the
    /// one after the page before `cursor`.
    fn goes_on_from(&self, cursor: &LogCursor, root: &Path) -> bool {
        self.root == root && self.seen == cursor.seen
    }

    /// The next `wanted` commits no page laid out yet, or all that are left when fewer are. It reads
    /// them from git as far as it has not yet.
    fn next_commits(&mut self, runner: &GitRunner, wanted: usize) -> Result<&[Commit], Failure> {
        while self.ahead.len() < wanted
            && let Some(git) = &mut self.git
        {
            let next = next_commit(git).map_err(|_| self.unreadable())?;
            match next {
                Some(commit) => self.ahead.push_back(commit),
                None => self.finish(runner)?,
            }
        }
        let ahead = self.ahead.make_contiguous();
        Ok(&ahead[..wanted.min(ahead.len())])
    }

    /// Waits for git, which listed every commit, and fails as the read of the log does when git
    /// failed.
    fn finish(&mut self, runner: &GitRunner) -> Result<(), Failure> {
        let Some(git) = self.git.take() else {
            return Ok(());
        };
        let invocation = git.finish().map_err(|_| self.unreadable())?;
        if !invocation.success {
            return Err(failure_of(
                runner,
                &self.root,
                &invocation,
                self.unreadable(),
            ));
        }
        Ok(())
    }

    /// The failure of a read that git could not finish.
    fn unreadable(&self) -> Failure {
        Failure::LogUnreadable {
            folder: self.root.clone(),
        }
    }

    /// Takes the next `count` commits as laid out on a page.
    fn lay_out(&mut self, count: usize) {
        let count = count.min(self.ahead.len());
        self.ahead.drain(..count);
        self.seen += count;
    }
}

/// The next commit `git` lists, or None once it listed them all.
fn next_commit(git: &mut GitStream) -> std::io::Result<Option<Commit>> {
    let mut fields = Vec::with_capacity(FIELDS);
    while fields.len() < FIELDS {
        let Some(field) = git.next_piece(0)? else {
            return Ok(None);
        };
        fields.push(String::from_utf8_lossy(&field).into_owned());
    }
    Ok(fields.try_into().ok())
}

/// A page of the log of `repository`: at most `count` of the commits `filter` shows, and at least
/// one when there are any, after `after` or from the newest commit. It goes on with the read `reads`
/// holds for `after`, or starts one. A later page keeps to the filter of the first.
///
/// A later page goes on from the lanes of the page before, which only fit the history they were
/// laid out for. So it needs the log to start from the same commits as then, and otherwise this
/// returns [`Failure::LogChanged`].
///
/// Unless `show_long_edges`, an edge that spans more than [`LONG_EDGE`] rows is cut. A later page
/// keeps to what the first one did.
pub(crate) fn read(
    runner: &GitRunner,
    reads: &LogReads,
    repository: &Repository,
    filter: &LogFilter,
    after: Option<&LogCursor>,
    count: usize,
    show_long_edges: bool,
) -> Result<LogPage, Failure> {
    let count = count.max(1);
    let seen = after.map_or(0, |cursor| cursor.seen);
    let show_long_edges = after.map_or(show_long_edges, |cursor| cursor.show_long_edges);
    let filter = after.map_or(filter, |cursor| &cursor.filter);
    let matcher = match &filter.text {
        Some(search) => Matcher::new(search)?,
        None => None,
    };
    let held = after.and_then(|cursor| reads.take(cursor, &repository.root));
    // Read before git starts, so a reference that moves in between makes the next page find the
    // history changed, instead of letting the read go on with a history the references left.
    let references = References::read(runner, repository)?;
    let tips = references.tips.clone();
    if after.is_some_and(|cursor| cursor.tips != tips) {
        return Err(Failure::LogChanged);
    }
    let Some(revisions) = filter.revisions(&references) else {
        return Ok(LogPage {
            repository: repository.clone(),
            rows: Vec::new(),
            next: None,
        });
    };
    let mut log_read = match held {
        Some(held) => held,
        None => {
            let with_message = matcher.is_some();
            let mut started = LogRead::start(runner, repository, seen, &revisions, with_message)?;
            // Read while git goes through the history, which takes longer.
            started.user_email = commit::user_email(runner, &repository.root)?;
            started
        }
    };
    let mut walk = match after {
        Some(cursor) => cursor.walk.clone(),
        None => Walk::from_tips(filter.current_branch_tips(runner, repository, &references)?),
    };
    let (rows, more) = match &matcher {
        Some(matcher) => {
            let found = found_rows(
                runner,
                &mut log_read,
                &mut walk,
                &references,
                matcher,
                count,
            )?;
            (found.rows, found.more)
        }
        None => {
            let laid_out = graph_rows(
                runner,
                &mut log_read,
                &mut walk,
                &references,
                count,
                show_long_edges,
            )?;
            (laid_out.rows, laid_out.more)
        }
    };
    let next = more.then(|| LogCursor {
        seen: log_read.seen,
        read: reads.hold(log_read),
        tips,
        show_long_edges,
        filter: filter.clone(),
        walk,
    });
    Ok(LogPage {
        repository: repository.clone(),
        rows,
        next,
    })
}

/// The rows of a page, and whether more come after it.
struct PageRows {
    rows: Vec<LogRow>,
    more: bool,
}

/// The next `count` rows of the log `log_read` reads, laid out with the graph from `walk`, which
/// they leave as the next page finds it. Unless `show_long_edges`, an edge that spans more than
/// [`LONG_EDGE`] rows is cut.
fn graph_rows(
    runner: &GitRunner,
    log_read: &mut LogRead,
    walk: &mut Walk,
    references: &References,
    count: usize,
    show_long_edges: bool,
) -> Result<PageRows, Failure> {
    // One more than the page, to know whether another comes after it, and enough beyond it to know
    // whether the edges from the page are long.
    let lookahead = if show_long_edges { 0 } else { LONG_EDGE };
    let user_email = log_read.user_email.clone();
    let commits = log_read.next_commits(runner, count + 1 + lookahead)?;
    let rows_of: HashMap<&str, usize> = commits
        .iter()
        .enumerate()
        .map(|(row, commit)| (commit[0].as_str(), row))
        .collect();
    // An edge is long when its parent is further down than the rows read beyond the page. git
    // lists every parent, so one not read yet is further down, unless the history ends before.
    let is_long = |row: usize, parent: &str| {
        !show_long_edges
            && rows_of
                .get(parent)
                .map_or(commits.len() > row + LONG_EDGE, |&at| {
                    at.saturating_sub(row) > LONG_EDGE
                })
    };
    let rows: Vec<LogRow> = commits
        .iter()
        .take(count)
        .enumerate()
        .map(|(row, commit)| {
            let labels = &references.labels;
            let fields = commit.each_ref().map(String::as_str);
            let is_long = |parent: &str| is_long(row, parent);
            walk.row(&fields, labels, user_email.as_deref(), is_long, true)
        })
        .collect();
    let more = commits.len() > count;
    log_read.lay_out(rows.len());
    Ok(PageRows { rows, more })
}

/// The next `count` commits that `matcher` finds in the log `log_read` reads, each a row of its
/// own with no lines of the graph. The commits it passes over still go through `walk`, which tells
/// the ones on the current branch. The read stops before the next commit found, after
/// [`LOG_SEARCH_BATCH`] commits, or at the end.
fn found_rows(
    runner: &GitRunner,
    log_read: &mut LogRead,
    walk: &mut Walk,
    references: &References,
    matcher: &Matcher,
    count: usize,
) -> Result<PageRows, Failure> {
    let user_email = log_read.user_email.clone();
    let mut rows = Vec::new();
    let mut gone_through = 0;
    let more = loop {
        let Some(commit) = log_read.next_commits(runner, 1)?.first() else {
            break false;
        };
        let found = matcher.matches(commit);
        if (found && rows.len() == count) || gone_through == LOG_SEARCH_BATCH {
            break true;
        }
        gone_through += 1;
        let fields = commit.each_ref().map(String::as_str);
        let row = walk.row(
            &fields,
            &references.labels,
            user_email.as_deref(),
            |_| false,
            false,
        );
        log_read.lay_out(1);
        if found {
            rows.push(row);
        }
    };
    Ok(PageRows { rows, more })
}

/// The references the log starts from and labels its commits with.
struct References {
    /// The commit HEAD points at, if it has one.
    head: Option<String>,
    /// The labels of each commit, in the order they show.
    labels: HashMap<String, Vec<RefLabel>>,
    /// A digest of the references, which tells whether the history moved since.
    tips: String,
}

/// Where each kind of reference the log shows lives, and the prefix its name loses. Refs anywhere
/// else, such as `refs/stash` and gikli's own under `refs/gikli/`, stay out of the log.
const REF_KINDS: [(&str, RefKind); 3] = [
    ("refs/heads/", RefKind::Local),
    ("refs/remotes/", RefKind::Remote),
    ("refs/tags/", RefKind::Tag),
];

impl RefLabel {
    /// Its full name, such as `refs/heads/main`.
    pub(crate) fn full_name(&self) -> String {
        match REF_KINDS.iter().find(|(_, kind)| *kind == self.kind) {
            Some((prefix, _)) => format!("{prefix}{}", self.name),
            None => self.name.clone(),
        }
    }

    /// The label of the reference whose full name is `name`, such as `refs/heads/main`, when it is
    /// of a kind the log shows.
    pub(crate) fn of_ref(name: &str) -> Option<Self> {
        REF_KINDS.iter().find_map(|(prefix, kind)| {
            name.strip_prefix(prefix).map(|short| Self {
                name: short.to_owned(),
                kind: *kind,
            })
        })
    }
}

/// What git prints of each reference, a field per NUL: what it points at, what an annotated tag
/// points at, where a symbolic ref leads, and its full name.
const REF_FORMAT: &str = "--format=%(objectname)%00%(*objectname)%00%(symref)%00%(refname)";

impl References {
    /// The commit `branch` points at, if it exists.
    fn commit_of(&self, branch: &RefLabel) -> Option<&String> {
        self.labels
            .iter()
            .find(|(_, labels)| labels.contains(branch))
            .map(|(commit, _)| commit)
    }

    /// The references of `repository` as they are now.
    fn read(runner: &GitRunner, repository: &Repository) -> Result<Self, Failure> {
        let (head, listed) = std::thread::scope(|scope| {
            let listed = scope.spawn(|| list_references(runner, repository));
            let head = head_commit(runner, repository);
            (
                head,
                listed.join().expect("listing the references panicked"),
            )
        });
        Ok(Self::new(head?, &listed?))
    }

    /// The references with HEAD at `head`, and the others as [`list_references`] `listed` them.
    fn new(head: Option<String>, listed: &[u8]) -> Self {
        let mut labels: HashMap<String, Vec<RefLabel>> = HashMap::new();
        if let Some(head) = &head {
            labels.entry(head.clone()).or_default().push(RefLabel {
                name: "HEAD".to_owned(),
                kind: RefKind::Head,
            });
        }
        for line in String::from_utf8_lossy(listed).lines() {
            let fields: Vec<&str> = line.split('\0').collect();
            let [object, peeled, symref, name] = fields[..] else {
                continue;
            };
            // A symbolic ref, such as `origin/HEAD`, only repeats the branch it leads to.
            if !symref.is_empty() {
                continue;
            }
            let Some((prefix, kind)) = REF_KINDS
                .iter()
                .find(|(prefix, _)| name.starts_with(prefix))
            else {
                continue;
            };
            let commit = if peeled.is_empty() { object } else { peeled };
            labels.entry(commit.to_owned()).or_default().push(RefLabel {
                name: name[prefix.len()..].to_owned(),
                kind: *kind,
            });
        }
        for commit_labels in labels.values_mut() {
            commit_labels.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        }

        let mut digest = DefaultHasher::new();
        listed.hash(&mut digest);
        head.hash(&mut digest);
        Self {
            head,
            labels,
            tips: format!("{:x}", digest.finish()),
        }
    }
}

/// The references of `repository` the log shows, as git lists them in [`REF_FORMAT`].
fn list_references(runner: &GitRunner, repository: &Repository) -> Result<Vec<u8>, Failure> {
    let mut args = vec!["for-each-ref", REF_FORMAT];
    args.extend(REF_KINDS.iter().map(|(prefix, _)| *prefix));
    Ok(run_git(runner, repository, &args)?.stdout)
}

/// The local branches, remote branches and tags of `repository`, each kind by name, as the log
/// labels them. A symbolic ref, such as `origin/HEAD`, only repeats the branch it leads to, so it
/// is not among them.
fn references(runner: &GitRunner, repository: &Repository) -> Result<Vec<RefLabel>, Failure> {
    Ok(labels_of(&list_references(runner, repository)?))
}

/// The local and remote branches of `repository`, local ones first, each kind by name: those the
/// log can be filtered by.
pub(crate) fn branches(
    runner: &GitRunner,
    repository: &Repository,
) -> Result<Vec<RefLabel>, Failure> {
    let mut branches = references(runner, repository)?;
    branches.retain(|label| label.kind.is_branch());
    Ok(branches)
}

/// The local and remote branches of `repository` that contain the commit `hash`, in the order of
/// [`branches`].
pub(crate) fn branches_containing(
    runner: &GitRunner,
    repository: &Repository,
    hash: &str,
) -> Result<Vec<RefLabel>, Failure> {
    let args = [
        "for-each-ref",
        REF_FORMAT,
        "--contains",
        hash,
        "refs/heads/",
        "refs/remotes/",
    ];
    Ok(labels_of(&run_git(runner, repository, &args)?.stdout))
}

/// The local branches, remote branches and tags of `repository` the user can go to when they have
/// typed `text`: those whose name holds it, ignoring case. The names that start with it come
/// first, then the others, each in the order of [`references`].
pub(crate) fn suggestions(
    runner: &GitRunner,
    repository: &Repository,
    text: &str,
) -> Result<Vec<RefLabel>, Failure> {
    let text = text.trim().to_lowercase();
    let (mut starting, holding): (Vec<RefLabel>, Vec<RefLabel>) = references(runner, repository)?
        .into_iter()
        .filter(|label| label.name.to_lowercase().contains(&text))
        .partition(|label| label.name.to_lowercase().starts_with(&text));
    starting.extend(holding);
    Ok(starting)
}

/// The references `listed` in [`REF_FORMAT`] that the log labels commits with, each kind by name,
/// without the symbolic ones.
fn labels_of(listed: &[u8]) -> Vec<RefLabel> {
    let mut labels: Vec<RefLabel> = String::from_utf8_lossy(listed)
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\0').collect();
            let [_, _, symref, name] = fields[..] else {
                return None;
            };
            symref.is_empty().then(|| RefLabel::of_ref(name)).flatten()
        })
        .collect();
    labels.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    labels
}

/// The commit HEAD of `repository` points at, if it has one.
fn head_commit(runner: &GitRunner, repository: &Repository) -> Result<Option<String>, Failure> {
    // `--verify --quiet` fails quietly for an unborn HEAD.
    let head = runner
        .run(
            &repository.root,
            &["rev-parse", "--verify", "--quiet", "HEAD"],
        )
        .map_err(git_failed_to_start)?;
    Ok(head
        .success
        .then(|| head.stdout_text().trim_end().to_owned()))
}

/// Runs git with `args` in `repository`, for a read of its log or of what it tells of a commit.
pub(crate) fn run_git(
    runner: &GitRunner,
    repository: &Repository,
    args: &[&str],
) -> Result<Invocation, Failure> {
    let root = &repository.root;
    let invocation = runner.run(root, args).map_err(git_failed_to_start)?;
    if !invocation.success {
        let unreadable = Failure::LogUnreadable {
            folder: root.clone(),
        };
        return Err(failure_of(runner, root, &invocation, unreadable));
    }
    Ok(invocation)
}

/// An arrow to each of `targets` from the node in `column`, each in the first column from there on
/// that no line in `taken` and no arrow before it ends in.
fn arrows(
    column: usize,
    taken: impl Iterator<Item = usize>,
    targets: Vec<String>,
) -> Vec<GraphArrow> {
    let mut taken: BTreeSet<usize> = taken.collect();
    targets
        .into_iter()
        .map(|target| {
            let column = (column..)
                .find(|free| !taken.contains(free))
                .unwrap_or(column);
            taken.insert(column);
            GraphArrow { column, target }
        })
        .collect()
}

/// The lanes of the graph at some row, each with the commit it runs down to. The layout goes from
/// the newest commit down, so this is all it has to remember between one row and the next.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
struct Lanes(Vec<Option<String>>);

impl Lanes {
    /// Draws the row of the commit `hash`, and leaves the lanes as the next row finds them. The
    /// edges to the parents `is_long` tells, and from `children_cut`, are drawn as arrows and hold
    /// no lane.
    fn place(
        &mut self,
        hash: &str,
        parents: &[String],
        is_long: impl Fn(&str) -> bool,
        children_cut: Vec<String>,
    ) -> GraphRow {
        let waiting = self.lane_of(hash);
        let column = waiting.unwrap_or_else(|| self.free_lane_from(0));
        let above = self.straight_lines();
        let arrows_up = arrows(column, above.iter().map(|line| line.from), children_cut);
        if let Some(lane) = waiting {
            self.0[lane] = None;
        }
        let mut below = self.straight_lines();
        let mut parents_cut = Vec::new();
        for (index, parent) in parents.iter().enumerate() {
            if is_long(parent) {
                parents_cut.push(parent.clone());
                continue;
            }
            let to = self.route_parent(parent, index == 0, column, &mut below);
            below.push(GraphLine { from: column, to });
        }
        below.sort_unstable();
        let arrows_down = arrows(column, below.iter().map(|line| line.to), parents_cut);
        while self.0.last().is_some_and(Option::is_none) {
            self.0.pop();
        }
        GraphRow {
            column,
            above,
            below,
            arrows_up,
            arrows_down,
        }
    }

    /// Gives `parent` of the commit in `column` its lane, and returns it. A first parent keeps to
    /// the column of its commit, even when a lane further right was already waiting for it, whose
    /// line in `below` turns to that column. So the first-parent line of a branch stays straight.
    fn route_parent(
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
