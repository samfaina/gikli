// The log: the commit list with its graph, the search, filter and go-to above it, the branches
// pane before it, and what the selected commit changed and its details beside it. The core reads
// the commits and lays out the graph a page at a time; this draws only the rows in view, and asks
// for the next page as the user nears the end.

import { branchGroup, branchList, sameLabel, type BranchActions, type BranchGroups } from "./branch-tree";
import { createCommitMenu } from "./commit-menu";
import {
  branches as readBranches,
  endLogRead,
  fetchAll,
  fetchBranch,
  fetchRestOfHistory,
  filteredLogReaches,
  goTo,
  goToBranch,
  goToSuggestions,
  logSearchHistory,
  readLog,
  isFailure,
  settings,
  setShowLongEdges,
  type BranchNode,
  type GraphArrow,
  type GraphLine,
  type LogCursor,
  type LogFilter,
  type LogRow,
  type RefLabel,
  type Repository,
  type TextSearch,
} from "./core";
import { element } from "./dom";
import { createCommitPanes, SHORT_HASH_LENGTH } from "./log-details";
import { t } from "./strings";

/** What the log view needs from the window around it. */
export interface LogViewHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The log took the place of the changes view. */
  opened(): void;
  /** The changes view is back. */
  closed(): void;
  /** Opens the history of the file at `path` in `repository`. */
  showFileHistory(repository: Repository, path: string): void;
}

export interface LogView {
  /** Shows the log of `repository` from its newest commit. */
  open(repository: Repository): void;
  /** Shows the log of `repository`, opening it if it is not shown, at the latest commit of `branch`. */
  showBranch(repository: Repository, branch: RefLabel): void;
  /** Goes back to the changes view. */
  close(): void;
}

/** Commits to a page: enough to fill the list several times over. */
const PAGE_SIZE = 200;
/** Commits to a page read on the way to a commit further down, such as the parent an arrow leads to. */
const SEARCH_PAGE_SIZE = 5000;
/** The height of a row in pixels. */
const ROW_HEIGHT = 26;
/** Rows drawn beyond each end of the view, and how close to the end the next page is asked for. */
const ROWS_BEYOND_VIEW = 30;
/** The width of a lane of the graph in pixels. */
const LANE_WIDTH = 14;
/** The number of lane colors in the styles. */
const LANE_COLORS = 8;
/** How far an arrow of a cut edge stays from the edge of its row, in pixels. */
const ARROW_GAP = 4;
/** The length of the sides of an arrowhead, in pixels. */
const ARROWHEAD = 4;
/** How long the user stops typing a revision before the names that match are looked up, in milliseconds. */
const SUGGESTION_DELAY = 150;
const SVG = "http://www.w3.org/2000/svg";

const dates = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });

export function createLogView(host: LogViewHost): LogView {
  const view = element<HTMLElement>("log");
  const title = element<HTMLElement>("log-title");
  const message = element<HTMLElement>("log-message");
  const historyCut = element<HTMLElement>("log-history-cut");
  const fetchRestButton = element<HTMLButtonElement>("log-fetch-rest");
  const list = element<HTMLElement>("log-list");
  const rowsView = element<HTMLElement>("log-rows");
  const longEdgesInput = element<HTMLInputElement>("log-long-edges");
  const fetchButton = element<HTMLButtonElement>("log-fetch");
  const refreshButton = element<HTMLButtonElement>("log-refresh");
  const closeButton = element<HTMLButtonElement>("log-close");
  const searchInput = element<HTMLInputElement>("log-search");
  const searchHistory = element<HTMLSelectElement>("log-search-history");
  const regexInput = element<HTMLInputElement>("log-search-regex");
  const matchCaseInput = element<HTMLInputElement>("log-search-match-case");
  const branchesPane = element<HTMLElement>("log-branches-list");
  const goToBranchButton = element<HTMLButtonElement>("log-go-to-branch");
  const goToInput = element<HTMLInputElement>("log-go-to");
  const goToList = element<HTMLDataListElement>("log-go-to-suggestions");

  let repository: Repository | null = null;
  let rows: LogRow[] = [];
  let next: LogCursor | null = null;
  /** The read of a page under way, if any. */
  let reading: Promise<boolean> | null = null;
  /** Tells the answer to the latest read from one that a refresh or a close overtook. */
  let latestRead = 0;
  let columns = 1;
  let drawScheduled = false;
  /** The hash of the selected commit, if any. */
  let selected: string | null = null;
  /** What the log shows, as the search and the branch filter set it. */
  let filter: LogFilter = { branches: [], text: null };
  /** The branch the user chose in the branches pane to filter the log by, if any. */
  let chosen: RefLabel | null = null;
  /** The local and remote branches the branches pane lists, as the core grouped them. */
  let listed: BranchGroups = { local: [], remote: [] };
  /** The searches the user ran, newest first, as the recent searches list them. */
  let searches: TextSearch[] = [];
  let suggestionTimer: number | undefined;

  const panes = createCommitPanes({
    errorView: (error) => host.errorView(error),
    goToCommit: (hash) => void goToCommit(hash, hash.slice(0, SHORT_HASH_LENGTH)),
    showFileHistory: (shown, path) => host.showFileHistory(shown, path),
  });
  const menu = createCommitMenu(view, (error) => message.replaceChildren(host.errorView(error)));

  element<HTMLElement>("log-long-edges-label").textContent = t("logLongEdges");
  element<HTMLElement>("log-search-regex-label").textContent = t("logSearchRegex");
  element<HTMLElement>("log-search-match-case-label").textContent = t("logSearchMatchCase");
  element<HTMLElement>("log-branches-title").textContent = t("logBranchesTitle");
  goToBranchButton.textContent = t("logGoToBranch");
  goToBranchButton.title = t("logGoToBranchTitle");
  goToBranchButton.addEventListener("click", () => {
    if (chosen !== null) {
      void goToBranchTip(chosen);
    }
  });
  searchInput.placeholder = t("logSearch");
  searchInput.setAttribute("aria-label", t("logSearch"));
  searchHistory.setAttribute("aria-label", t("logRecentSearches"));
  goToInput.placeholder = t("logGoTo");
  goToInput.setAttribute("aria-label", t("logGoTo"));
  element<HTMLElement>("log-history-cut-text").textContent = t("logHistoryCut");
  fetchRestButton.textContent = t("logFetchRest");
  fetchRestButton.addEventListener("click", () => void fetchRest());
  fetchButton.textContent = t("logFetchAll");
  fetchButton.addEventListener("click", () => void fetchChosen());
  refreshButton.textContent = t("logRefresh");
  closeButton.textContent = t("logClose");
  refreshButton.addEventListener("click", refresh);
  closeButton.addEventListener("click", close);
  longEdgesInput.addEventListener("change", () => void changeLongEdges());
  searchInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      applyFilter();
    }
  });
  // Clearing the search shows every commit again.
  searchInput.addEventListener("input", () => {
    if (searchInput.value === "" && filter.text !== null) {
      applyFilter();
    }
  });
  regexInput.addEventListener("change", () => searchInput.value !== "" && applyFilter());
  matchCaseInput.addEventListener("change", () => searchInput.value !== "" && applyFilter());
  searchHistory.addEventListener("change", runRecentSearch);
  goToInput.addEventListener("input", scheduleSuggestions);
  goToInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && goToInput.value.trim() !== "") {
      void goToRevision(goToInput.value);
    }
  });
  list.addEventListener("scroll", scheduleDraw);
  list.addEventListener("keydown", onKey);
  rowsView.addEventListener("click", onClick);
  rowsView.addEventListener("contextmenu", onContextMenu);
  window.addEventListener("resize", scheduleDraw);

  function open(toShow: Repository): void {
    repository = toShow;
    selected = null;
    panes.show(null, null);
    filter = { branches: [], text: null };
    chosen = null;
    listed = { local: [], remote: [] };
    drawBranches();
    labelFetchButton();
    searchInput.value = "";
    goToInput.value = "";
    title.textContent = t("logTitle", { root: toShow.root });
    historyCut.hidden = true;
    view.hidden = false;
    host.opened();
    list.scrollTop = 0;
    void settings().then((current) => {
      longEdgesInput.checked = current.showLongEdges;
    });
    void listBranches(toShow);
    void listSearches();
    void startOver();
    list.focus();
  }

  function showBranch(toShow: Repository, branch: RefLabel): void {
    if (repository?.root !== toShow.root || view.hidden) {
      open(toShow);
    } else if (chosen !== null) {
      // A log filtered by another branch may not show the one asked for.
      chooseBranch(null);
    }
    void goToBranchTip(branch);
  }

  /** Reads the log again, and the branches it can be filtered by, which may have changed. */
  function refresh(): void {
    if (repository !== null) {
      void listBranches(repository);
    }
    void startOver();
  }

  /**
   * Lists the local and remote branches of `shown` in the branches pane, after every branch,
   * keeping the one chosen. Each local branch says how far it is from its upstream.
   */
  async function listBranches(shown: Repository): Promise<void> {
    let read: BranchGroups;
    try {
      read = await readBranches(shown, "");
    } catch (error) {
      // Without the branches, the pane offers every branch only, and says why.
      if (repository === shown) {
        listed = { local: [], remote: [] };
        drawBranches(host.errorView(error));
      }
      return;
    }
    if (repository !== shown) {
      return;
    }
    listed = read;
    const labels = [...labelsOf(listed.local), ...labelsOf(listed.remote)];
    const kept = chosen;
    if (kept !== null && !labels.some((label) => sameLabel(label, kept))) {
      chosen = null;
    }
    drawBranches();
    labelFetchButton();
  }

  /** Draws the branches pane: every branch, then the local and remote branches, with the chosen one marked. */
  function drawBranches(failure?: HTMLElement): void {
    const actions: BranchActions = {
      pick: (branch) => chooseBranch(branch.label),
      open: (branch) => void goToBranchTip(branch.label),
      chosen,
    };
    const every = document.createElement("button");
    every.type = "button";
    every.className = "branch-item";
    every.dataset.testid = "log-all-branches";
    every.textContent = t("logAllBranches");
    every.classList.toggle("chosen", chosen === null);
    every.setAttribute("aria-pressed", String(chosen === null));
    every.addEventListener("click", () => chooseBranch(null));
    const groups = [
      { id: "local", heading: t("branchesLocal"), nodes: listed.local },
      { id: "remote", heading: t("branchesRemote"), nodes: listed.remote },
    ]
      .filter((group) => group.nodes.length > 0)
      .map((group) => {
        const heading = document.createElement("h4");
        heading.textContent = group.heading;
        return branchGroup(`log-branch-group-${group.id}`, heading, branchList(group.nodes, actions));
      });
    branchesPane.replaceChildren(every, ...groups, ...(failure === undefined ? [] : [failure]));
    goToBranchButton.disabled = chosen === null;
  }

  /** Filters the log by `branch`, or shows every branch again for null. Choosing it again, as a double-click does, changes nothing. */
  function chooseBranch(branch: RefLabel | null): void {
    const same = branch === null || chosen === null ? branch === chosen : sameLabel(branch, chosen);
    if (same) {
      return;
    }
    chosen = branch;
    drawBranches();
    labelFetchButton();
    applyFilter();
  }

  /** Selects the latest commit of `branch`, or says why it cannot. */
  async function goToBranchTip(branch: RefLabel): Promise<void> {
    const shown = repository;
    if (shown === null) {
      return;
    }
    try {
      const hash = await goToBranch(shown, branch);
      if (repository === shown) {
        await goToCommit(hash, branch.name);
      }
    } catch (error) {
      if (repository === shown) {
        message.replaceChildren(host.errorView(error));
      }
    }
  }

  /** Says on the fetch button what it fetches: the branch chosen in the filter, or every remote. */
  function labelFetchButton(): void {
    fetchButton.textContent = chosen === null ? t("logFetchAll") : t("logFetchBranch", { branch: chosen.name });
  }

  /** Fetches the branch chosen in the filter, or every remote when none is, and reads the log again. */
  async function fetchChosen(): Promise<void> {
    const shown = repository;
    if (shown === null) {
      return;
    }
    // The branch chosen when the fetch starts, as the user may choose another while it runs.
    const picked = chosen;
    fetchButton.disabled = true;
    try {
      await (picked === null ? fetchAll(shown) : fetchBranch(shown, picked));
      if (repository === shown) {
        refresh();
      }
    } catch (error) {
      if (repository === shown) {
        message.replaceChildren(host.errorView(error));
      }
    } finally {
      fetchButton.disabled = false;
    }
  }

  /** Offers the searches the user ran, newest first, with their options, beside the search field. */
  async function listSearches(): Promise<void> {
    searches = await logSearchHistory().catch(() => [] as TextSearch[]);
    const prompt = new Option(t("logRecentSearches"), "");
    prompt.disabled = true;
    searchHistory.replaceChildren(prompt, ...searches.map((search, index) => new Option(recentSearchLabel(search), String(index))));
    searchHistory.value = "";
    searchHistory.disabled = searches.length === 0;
  }

  /** The search as the recent searches name it: its text, and the options it has on. */
  function recentSearchLabel(search: TextSearch): string {
    const options: string[] = [];
    if (search.regex) {
      options.push(t("logSearchRegex"));
    }
    if (search.matchCase) {
      options.push(t("logSearchMatchCase"));
    }
    return options.length === 0 ? search.text : t("logRecentSearch", { text: search.text, options: options.join(", ") });
  }

  /** Runs again the search the user picked from the recent searches, with its text and options. */
  function runRecentSearch(): void {
    const picked = searches[Number(searchHistory.value)];
    searchHistory.value = "";
    if (picked === undefined) {
      return;
    }
    searchInput.value = picked.text;
    regexInput.checked = picked.regex;
    matchCaseInput.checked = picked.matchCase;
    applyFilter();
  }

  /** Reads the log again with the search and the branch filter as the user set them. */
  function applyFilter(): void {
    const text = searchInput.value;
    filter = {
      branches: chosen === null ? [] : [chosen],
      text: text === "" ? null : { text, regex: regexInput.checked, matchCase: matchCaseInput.checked },
    };
    list.scrollTop = 0;
    void startOver().then(() => listSearches());
  }

  function scheduleSuggestions(): void {
    window.clearTimeout(suggestionTimer);
    suggestionTimer = window.setTimeout(() => void suggest(), SUGGESTION_DELAY);
  }

  /** Offers the branches and tags whose name holds what the user typed to go to. */
  async function suggest(): Promise<void> {
    const shown = repository;
    if (shown === null) {
      return;
    }
    const text = goToInput.value;
    const suggestions = await goToSuggestions(shown, text).catch(() => [] as RefLabel[]);
    if (repository === shown && goToInput.value === text) {
      goToList.replaceChildren(...suggestions.map((label) => new Option(label.name)));
    }
  }

  /** Selects the commit `revision` names: a hash, a branch or a tag. */
  async function goToRevision(revision: string): Promise<void> {
    const shown = repository;
    if (shown === null) {
      return;
    }
    try {
      const hash = await goTo(shown, revision);
      if (repository === shown) {
        await goToCommit(hash, revision.trim());
      }
    } catch (error) {
      message.replaceChildren(host.errorView(error));
    }
  }

  /**
   * Selects the commit `hash`, which the user knows as `revision`, or says the log does not show it:
   * at once when none of the branches it is filtered by reach it or the search does not match it,
   * and otherwise once it is read.
   */
  async function goToCommit(hash: string, revision: string): Promise<void> {
    const shown = repository;
    const asked = filter;
    if (shown === null) {
      return;
    }
    // The answer is of no use once the log shows another repository or filters by something else.
    const stillAsked = () => repository === shown && filter === asked;
    try {
      const reached = await filteredLogReaches(shown, asked, hash);
      if (stillAsked() && (!reached || !(await select(hash))) && stillAsked()) {
        message.replaceChildren(t("logNotShown", { revision }));
      }
    } catch (error) {
      if (stillAsked()) {
        message.replaceChildren(host.errorView(error));
      }
    }
  }

  function close(): void {
    if (repository === null) {
      return;
    }
    repository = null;
    latestRead += 1;
    endReadOf(next);
    rows = [];
    next = null;
    reading = null;
    selected = null;
    panes.show(null, null);
    rowsView.replaceChildren();
    view.hidden = true;
    host.closed();
  }

  /** Fetches the rest of the cut history, and reads the log again to show it whole. */
  async function fetchRest(): Promise<void> {
    const shown = repository;
    if (shown === null) {
      return;
    }
    fetchRestButton.disabled = true;
    try {
      await fetchRestOfHistory(shown);
      if (repository === shown) {
        refresh();
      }
    } catch (error) {
      if (repository === shown) {
        message.replaceChildren(host.errorView(error));
      }
    } finally {
      fetchRestButton.disabled = false;
    }
  }

  /** Saves whether long edges are drawn whole, and reads the log again to draw them so. */
  async function changeLongEdges(): Promise<void> {
    longEdgesInput.disabled = true;
    try {
      const saved = await setShowLongEdges(longEdgesInput.checked);
      longEdgesInput.checked = saved.showLongEdges;
      await startOver();
    } catch (error) {
      longEdgesInput.checked = !longEdgesInput.checked;
      message.replaceChildren(host.errorView(error));
    } finally {
      longEdgesInput.disabled = false;
    }
  }

  /** Reads the log again from its newest commit. Resolves to whether it came. */
  function startOver(): Promise<boolean> {
    endReadOf(next);
    rows = [];
    next = null;
    reading = null;
    columns = 1;
    message.replaceChildren(t("logReading"));
    return readPage(null);
  }

  /**
   * Reads the page of `count` commits after `after`, or the first page. Resolves to whether the log
   * came: false when the read failed, or a refresh or a close overtook it.
   */
  function readPage(after: LogCursor | null, count = PAGE_SIZE): Promise<boolean> {
    if (repository === null) {
      return Promise.resolve(false);
    }
    const read = readPageOf(repository, filter, after, count);
    reading = read;
    return read;
  }

  /** Reads a page of the log of `shown` with `shownFilter` for {@link readPage}, and draws it. */
  async function readPageOf(
    shown: Repository,
    shownFilter: LogFilter,
    after: LogCursor | null,
    count: number,
  ): Promise<boolean> {
    latestRead += 1;
    const thisRead = latestRead;
    try {
      const page = await readLog(shown, shownFilter, after, count);
      if (thisRead !== latestRead) {
        endReadOf(page.next);
        return false;
      }
      reading = null;
      rows.push(...page.rows);
      next = page.next;
      historyCut.hidden = !page.historyCut;
      columns = page.rows.reduce((widest, row) => Math.max(widest, columnsOf(row)), columns);
      const filtered = shownFilter.text !== null || shownFilter.branches.length > 0;
      // A page of a search can find nothing while the search goes on through the history.
      const emptyMessage = next !== null ? t("logSearching") : t(filtered ? "logNoMatches" : "logEmpty");
      message.replaceChildren(rows.length === 0 ? emptyMessage : "");
      draw();
      return true;
    } catch (error) {
      if (thisRead !== latestRead) {
        return false;
      }
      reading = null;
      if (isFailure(error) && error.kind === "logChanged") {
        return startOver();
      }
      message.replaceChildren(host.errorView(error));
      return false;
    }
  }

  /** The place of the commit `hash` in the list, reading pages until it comes; -1 if it does not. */
  async function placeOf(hash: string): Promise<number> {
    const shown = repository;
    for (;;) {
      const index = rows.findIndex((row) => row.hash === hash);
      if (index >= 0 || repository !== shown) {
        return index;
      }
      const read = reading ?? (next === null ? null : readPage(next, SEARCH_PAGE_SIZE));
      if (read === null || !(await read)) {
        return -1;
      }
    }
  }

  /** Selects the commit `hash`, and scrolls it into view. Resolves to whether the log shows it. */
  async function select(hash: string): Promise<boolean> {
    const index = await placeOf(hash);
    if (index < 0) {
      return false;
    }
    if (selected !== hash) {
      selected = hash;
      panes.show(repository, hash);
    }
    const top = index * ROW_HEIGHT;
    if (top < list.scrollTop) {
      list.scrollTop = top;
    } else if (top + ROW_HEIGHT > list.scrollTop + list.clientHeight) {
      list.scrollTop = top + ROW_HEIGHT - list.clientHeight;
    }
    draw();
    return true;
  }

  /**
   * The left and right arrows go to the parent and the child, the up and down arrows along the list,
   * and the menu key or Shift+F10 opens the commit menu of the selected commit.
   */
  function onKey(event: KeyboardEvent): void {
    const index = rows.findIndex((row) => row.hash === selected);
    const current = rows[index];
    if (event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey)) {
      event.preventDefault();
      const row = rowsView.querySelector<HTMLElement>(".log-row.selected");
      if (current !== undefined && row !== null) {
        const box = row.getBoundingClientRect();
        menu.open(current.hash, box.left + 16, box.bottom, list);
      }
      return;
    }
    let target: string | undefined;
    if (event.key === "ArrowLeft") {
      target = current?.parents[0];
    } else if (event.key === "ArrowRight") {
      target = current?.children[0];
    } else if (event.key === "ArrowDown") {
      target = rows[current === undefined ? 0 : index + 1]?.hash;
    } else if (event.key === "ArrowUp") {
      target = rows[current === undefined ? 0 : index - 1]?.hash;
    } else {
      return;
    }
    event.preventDefault();
    if (target !== undefined) {
      void select(target);
    }
  }

  /** A click on an arrow of a cut edge goes to the commit at its other end, and one elsewhere on a row selects it. */
  function onClick(event: MouseEvent): void {
    const target = event.target instanceof Element ? event.target : null;
    const arrow = target?.closest<SVGElement>("[data-target]");
    const row = target?.closest<HTMLElement>(".log-row");
    const hash = arrow?.dataset.target ?? row?.dataset.hash;
    if (hash !== undefined) {
      void select(hash);
    }
  }

  /** A right click on a row selects it and opens the commit menu. */
  function onContextMenu(event: MouseEvent): void {
    const target = event.target instanceof Element ? event.target : null;
    const hash = target?.closest<HTMLElement>(".log-row")?.dataset.hash;
    if (hash === undefined) {
      return;
    }
    event.preventDefault();
    void select(hash);
    menu.open(hash, event.clientX, event.clientY, list);
  }

  function scheduleDraw(): void {
    if (!drawScheduled) {
      drawScheduled = true;
      requestAnimationFrame(() => {
        drawScheduled = false;
        draw();
      });
    }
  }

  /** Draws the rows in view, and asks for the next page when they come near the last one read. */
  function draw(): void {
    if (repository === null) {
      return;
    }
    const firstInView = Math.floor(list.scrollTop / ROW_HEIGHT);
    const inView = Math.ceil(list.clientHeight / ROW_HEIGHT);
    const start = Math.max(0, firstInView - ROWS_BEYOND_VIEW);
    const end = Math.min(rows.length, firstInView + inView + ROWS_BEYOND_VIEW);
    rowsView.style.height = `${(rows.length + (next === null ? 0 : 1)) * ROW_HEIGHT}px`;
    list.style.setProperty("--log-graph-width", `${columns * LANE_WIDTH}px`);
    list.style.setProperty("--log-row-height", `${ROW_HEIGHT}px`);
    rowsView.replaceChildren(
      ...rows.slice(start, end).map((row, index) => rowView(row, start + index, columns, row.hash === selected)),
    );
    if (next !== null && reading === null && end + ROWS_BEYOND_VIEW >= rows.length) {
      void readPage(next);
    }
  }

  return { open, showBranch, close };
}

/** Lets the core go of the read that the page at `cursor` goes on with, which the log does not read, if any. Nothing depends on it. */
function endReadOf(cursor: LogCursor | null): void {
  if (cursor !== null) {
    endLogRead(cursor).catch(() => undefined);
  }
}

/** How many columns of the graph `row` takes. */
function columnsOf(row: LogRow): number {
  const { graph } = row;
  const lines = [...graph.above, ...graph.below].flatMap((line) => [line.from, line.to]);
  const arrows = [...graph.arrowsUp, ...graph.arrowsDown].map((arrow) => arrow.column);
  return Math.max(graph.column, ...lines, ...arrows) + 1;
}

function rowView(row: LogRow, index: number, columns: number, selected: boolean): HTMLElement {
  const view = document.createElement("div");
  view.className = "log-row";
  view.classList.toggle("current-branch", row.onCurrentBranch);
  view.classList.toggle("own", row.own);
  view.classList.toggle("selected", selected);
  view.dataset.testid = "log-row";
  view.dataset.hash = row.hash;
  view.setAttribute("aria-selected", String(selected));
  view.style.top = `${index * ROW_HEIGHT}px`;
  const author = cell("log-author", row.committerDiffers ? `${row.author}*` : row.author);
  if (row.committerDiffers) {
    author.title = t("logCommitterDiffers");
  }
  view.append(
    graphView(row, columns),
    summaryView(row),
    author,
    cell("log-date", dates.format(new Date(row.committedAt * 1000))),
    cell("log-hash", row.hash.slice(0, SHORT_HASH_LENGTH)),
  );
  return view;
}

/** The labels of the references on `row`'s commit, then its subject. */
function summaryView(row: LogRow): HTMLElement {
  const summary = document.createElement("span");
  summary.className = "log-summary";
  for (const label of row.labels) {
    const chip = cell(`log-label log-label-${label.kind}`, label.name);
    chip.title = label.name;
    summary.append(chip);
  }
  summary.append(cell("log-subject", row.subject));
  return summary;
}

function cell(className: string, text: string): HTMLElement {
  const span = document.createElement("span");
  span.className = className;
  span.textContent = text;
  return span;
}

/** The middle of the lane in `column`. */
function laneCenter(column: number): number {
  return column * LANE_WIDTH + LANE_WIDTH / 2;
}

/** The graph of `row`, as wide as `columns` lanes. */
function graphView(row: LogRow, columns: number): SVGElement {
  const graph = document.createElementNS(SVG, "svg");
  graph.classList.add("log-graph");
  graph.setAttribute("width", String(columns * LANE_WIDTH));
  graph.setAttribute("height", String(ROW_HEIGHT));
  const middle = ROW_HEIGHT / 2;
  for (const line of row.graph.above) {
    graph.append(lineView(line, 0, middle));
  }
  for (const line of row.graph.below) {
    graph.append(lineView(line, middle, ROW_HEIGHT));
  }
  const nodeColumn = row.graph.column;
  for (const arrow of row.graph.arrowsUp) {
    graph.append(arrowView(arrow, nodeColumn, middle, ARROW_GAP, t("logGoToChild")));
  }
  for (const arrow of row.graph.arrowsDown) {
    graph.append(arrowView(arrow, nodeColumn, middle, ROW_HEIGHT - ARROW_GAP, t("logGoToParent")));
  }
  if (row.historyCut) {
    graph.append(cutView(nodeColumn, middle));
  }
  const node = document.createElementNS(SVG, "circle");
  node.classList.add(laneColor(nodeColumn));
  node.setAttribute("cx", String(laneCenter(nodeColumn)));
  node.setAttribute("cy", String(middle));
  node.setAttribute("r", "4");
  graph.append(node);
  return graph;
}

/** `line`, from the height `top` to the height `bottom`, in the color of the branch it belongs to: the one further right. */
function lineView(line: GraphLine, top: number, bottom: number): SVGElement {
  const view = document.createElementNS(SVG, "line");
  view.classList.add(laneColor(Math.max(line.from, line.to)));
  view.setAttribute("x1", String(laneCenter(line.from)));
  view.setAttribute("y1", String(top));
  view.setAttribute("x2", String(laneCenter(line.to)));
  view.setAttribute("y2", String(bottom));
  return view;
}

/**
 * The arrow of a cut edge: a line from the node in `nodeColumn`, at the height `middle`, to the
 * arrow's column at the height `end`, with its head there. A click on it goes to the commit at the
 * other end.
 */
function arrowView(arrow: GraphArrow, nodeColumn: number, middle: number, end: number, label: string): SVGElement {
  const group = document.createElementNS(SVG, "g");
  group.classList.add("log-arrow", laneColor(Math.max(nodeColumn, arrow.column)));
  group.dataset.target = arrow.target;
  const titleView = document.createElementNS(SVG, "title");
  titleView.textContent = label;
  const x = laneCenter(arrow.column);
  const shaft = document.createElementNS(SVG, "line");
  shaft.setAttribute("x1", String(laneCenter(nodeColumn)));
  shaft.setAttribute("y1", String(middle));
  shaft.setAttribute("x2", String(x));
  shaft.setAttribute("y2", String(end));
  // The head points away from the node: up at the top of the row, down at the bottom.
  const back = end < middle ? end + ARROWHEAD : end - ARROWHEAD;
  const head = document.createElementNS(SVG, "polyline");
  head.setAttribute("points", `${x - ARROWHEAD},${back} ${x},${end} ${x + ARROWHEAD},${back}`);
  group.append(titleView, shaft, head);
  return group;
}

/**
 * The mark of a history cut at the node in `nodeColumn`, at the height `middle`: a dashed line down
 * from the node that ends in a cross stroke, where the parents the Repository lacks would be.
 */
function cutView(nodeColumn: number, middle: number): SVGElement {
  const group = document.createElementNS(SVG, "g");
  group.classList.add("log-cut", laneColor(nodeColumn));
  group.dataset.testid = "log-history-cut-mark";
  const titleView = document.createElementNS(SVG, "title");
  titleView.textContent = t("logHistoryCutHere");
  const x = laneCenter(nodeColumn);
  const end = ROW_HEIGHT - ARROW_GAP;
  const stub = document.createElementNS(SVG, "line");
  stub.setAttribute("x1", String(x));
  stub.setAttribute("y1", String(middle));
  stub.setAttribute("x2", String(x));
  stub.setAttribute("y2", String(end));
  const cross = document.createElementNS(SVG, "polyline");
  cross.setAttribute("points", `${x - ARROWHEAD},${end + 1} ${x + ARROWHEAD},${end - 1}`);
  group.append(titleView, stub, cross);
  return group;
}

function laneColor(column: number): string {
  return `log-lane-${column % LANE_COLORS}`;
}

/** Every branch `nodes` hold, in their directories or not. */
function labelsOf(nodes: BranchNode[]): RefLabel[] {
  return nodes.flatMap((node) => (node.kind === "directory" ? labelsOf(node.children) : [node.label]));
}

