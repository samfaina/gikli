//! The log: the history of a Repository as a commit list with a graph, read a page at a time.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::git::{GitRunner, Invocation};
use crate::{Failure, Repository, failure_of, git_failed_to_start};

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefLabel {
    /// Its name as the user knows it: `HEAD`, `main`, `origin/main`, `v1`.
    pub name: String,
    pub kind: RefKind,
}

/// What a [`RefLabel`] names. Each kind shows in its own color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RefKind {
    Head,
    Local,
    Remote,
    Tag,
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
    /// A digest of where the log started from, which tells whether the history moved since.
    tips: String,
    /// Whether long edges are drawn whole, as when the log started, so the graph goes on as it was.
    show_long_edges: bool,
    walk: Walk,
}

/// What the log remembers from one row to the next. It reads from the newest commit down, and git
/// lists every commit after all of its children, so what a row needs to know of the rows above it
/// is what they left waiting for the commits still to come.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Walk {
    lanes: Lanes,
    /// The commits still to come that HEAD reaches: HEAD's own, and the parents of each one
    /// reached.
    on_current_branch: BTreeSet<String>,
    /// The children seen of each commit still to come, newest first.
    children: BTreeMap<String, Vec<Child>>,
    /// The children whose edges to each commit still to come were cut, newest first.
    cut_children: BTreeMap<String, Vec<String>>,
}

/// A child seen of a commit still to come.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Child {
    hash: String,
    /// The column of its node.
    column: usize,
}

impl Walk {
    /// The walk at the newest commit, with HEAD at `head`.
    fn from_head(head: Option<&String>) -> Self {
        Self {
            on_current_branch: head.into_iter().cloned().collect(),
            ..Self::default()
        }
    }

    /// The row of the commit git printed as `fields`, and the walk as the next row finds it.
    /// `user_email` is the email of the user's Commit identity, if they have one, and `is_long`
    /// tells whether the edge to a parent is cut.
    fn row(
        &mut self,
        fields: &[&str],
        labels: &HashMap<String, Vec<RefLabel>>,
        user_email: Option<&str>,
        is_long: impl Fn(&str) -> bool,
    ) -> LogRow {
        let hash = fields[0].to_owned();
        let parents: Vec<String> = fields[1].split_whitespace().map(str::to_owned).collect();
        let [author, author_email, committer, committer_email] = [3, 4, 5, 6].map(|at| fields[at]);
        let on_current_branch = self.on_current_branch.remove(&hash);
        if on_current_branch {
            self.on_current_branch.extend(parents.iter().cloned());
        }
        let cut: Vec<bool> = parents.iter().map(|parent| is_long(parent)).collect();
        let children_cut = self.cut_children.remove(&hash).unwrap_or_default();
        let graph = self.lanes.place(&hash, &parents, &cut, children_cut);
        for (parent, _) in parents.iter().zip(&cut).filter(|(_, cut)| **cut) {
            let children_cut = self.cut_children.entry(parent.clone()).or_default();
            children_cut.push(hash.clone());
        }
        let mut children = self.children.remove(&hash).unwrap_or_default();
        if let Some(on_its_line) = children
            .iter()
            .position(|child| child.column == graph.column)
        {
            let child = children.remove(on_its_line);
            children.insert(0, child);
        }
        for parent in &parents {
            self.children
                .entry(parent.clone())
                .or_default()
                .push(Child {
                    hash: hash.clone(),
                    column: graph.column,
                });
        }
        LogRow {
            graph,
            labels: labels.get(&hash).cloned().unwrap_or_default(),
            on_current_branch,
            children: children.into_iter().map(|child| child.hash).collect(),
            hash,
            parents,
            committed_at: fields[2].parse().unwrap_or_default(),
            own: user_email.is_some_and(|email| email.eq_ignore_ascii_case(author_email)),
            committer_differs: (author, author_email) != (committer, committer_email),
            author: author.to_owned(),
            subject: fields[7].to_owned(),
        }
    }
}

/// What git prints of each commit, a field per NUL. `-z` ends each commit with one more.
const FORMAT: &str = "--format=%H%x00%P%x00%ct%x00%an%x00%ae%x00%cn%x00%ce%x00%s";
const FIELDS: usize = 8;

/// A page of the log of `repository`: at most `count` commits, and at least one when there are any,
/// after `after` or from the newest commit.
///
/// A later page goes on from the lanes of the page before, which only fit the history they were
/// laid out for. So it needs the log to start from the same commits as then, and otherwise this
/// returns [`Failure::LogChanged`].
///
/// Unless `show_long_edges`, an edge that spans more than [`LONG_EDGE`] rows is cut. A later page
/// keeps to what the first one did.
pub(crate) fn read(
    runner: &GitRunner,
    repository: &Repository,
    after: Option<&LogCursor>,
    count: usize,
    show_long_edges: bool,
) -> Result<LogPage, Failure> {
    let count = count.max(1);
    let references = References::read(runner, repository)?;
    let tips = references.tips;
    if after.is_some_and(|cursor| cursor.tips != tips) {
        return Err(Failure::LogChanged);
    }
    let user_email = user_email_of(runner, repository)?;
    let seen = after.map_or(0, |cursor| cursor.seen);
    let show_long_edges = after.map_or(show_long_edges, |cursor| cursor.show_long_edges);
    let skip = format!("--skip={seen}");
    // One more than the page, to know whether another comes after it, and enough beyond it to know
    // whether the edges from the page are long.
    let beyond = if show_long_edges { 0 } else { LONG_EDGE };
    let max_count = format!("--max-count={}", count + 1 + beyond);
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
    if references.head.is_some() {
        args.push("HEAD");
    }
    args.push("--");
    let text = run_git(runner, repository, &args)?.stdout_text();
    let fields: Vec<&str> = text.split_terminator('\0').collect();
    let (commits, _) = fields.as_chunks::<FIELDS>();
    let rows_of: HashMap<&str, usize> = commits
        .iter()
        .enumerate()
        .map(|(row, commit)| (commit[0], row))
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
    let mut walk = after.map_or_else(
        || Walk::from_head(references.head.as_ref()),
        |cursor| cursor.walk.clone(),
    );
    let rows: Vec<LogRow> = commits
        .iter()
        .take(count)
        .enumerate()
        .map(|(row, commit)| {
            let labels = &references.labels;
            walk.row(commit, labels, user_email.as_deref(), |parent| {
                is_long(row, parent)
            })
        })
        .collect();
    let next = (commits.len() > count).then(|| LogCursor {
        seen: seen + rows.len(),
        tips,
        show_long_edges,
        walk,
    });
    Ok(LogPage {
        repository: repository.clone(),
        rows,
        next,
    })
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

/// What git prints of each reference, a field per NUL: what it points at, what an annotated tag
/// points at, where a symbolic ref leads, and its full name.
const REF_FORMAT: &str = "--format=%(objectname)%00%(*objectname)%00%(symref)%00%(refname)";

impl References {
    fn read(runner: &GitRunner, repository: &Repository) -> Result<Self, Failure> {
        let mut args = vec!["for-each-ref", REF_FORMAT];
        args.extend(REF_KINDS.iter().map(|(prefix, _)| *prefix));
        let listed = run_git(runner, repository, &args)?.stdout;
        // `--verify --quiet` fails quietly for an unborn HEAD.
        let head = runner
            .run(
                &repository.root,
                &["rev-parse", "--verify", "--quiet", "HEAD"],
            )
            .map_err(git_failed_to_start)?;
        let head = head
            .success
            .then(|| head.stdout_text().trim_end().to_owned());

        let mut labels: HashMap<String, Vec<RefLabel>> = HashMap::new();
        if let Some(head) = &head {
            labels.entry(head.clone()).or_default().push(RefLabel {
                name: "HEAD".to_owned(),
                kind: RefKind::Head,
            });
        }
        for line in String::from_utf8_lossy(&listed).lines() {
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
        Ok(Self {
            head,
            labels,
            tips: format!("{:x}", digest.finish()),
        })
    }
}

/// The email of the user's Commit identity in `repository`, if it has one.
fn user_email_of(runner: &GitRunner, repository: &Repository) -> Result<Option<String>, Failure> {
    let email = runner
        .run(&repository.root, &["config", "--get", "user.email"])
        .map_err(git_failed_to_start)?
        .stdout_text();
    let email = email.trim();
    Ok((!email.is_empty()).then(|| email.to_owned()))
}

/// Runs git with `args` in `repository`, for a read of its log.
fn run_git(
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
    /// edges to the parents `cut` marks, and from `children_cut`, are drawn as arrows and hold no
    /// lane.
    fn place(
        &mut self,
        hash: &str,
        parents: &[String],
        cut: &[bool],
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
        for (index, (parent, cut)) in parents.iter().zip(cut).enumerate() {
            if *cut {
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
