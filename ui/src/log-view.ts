// The log: the commit list with its graph. The core reads the commits and lays out the graph a page
// at a time; this draws only the rows in view, and asks for the next page as the user nears the end.

import {
  endLogRead,
  readLog,
  isFailure,
  settings,
  setShowLongEdges,
  type GraphArrow,
  type GraphLine,
  type LogCursor,
  type LogRow,
  type Repository,
} from "./core";
import { element } from "./dom";
import { t } from "./strings";

/** What the log view needs from the window around it. */
export interface LogViewHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The log took the place of the changes view. */
  opened(): void;
  /** The changes view is back. */
  closed(): void;
}

export interface LogView {
  /** Shows the log of `repository` from its newest commit. */
  open(repository: Repository): void;
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
const SHORT_HASH_LENGTH = 8;
const SVG = "http://www.w3.org/2000/svg";

const dates = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });

export function createLogView(host: LogViewHost): LogView {
  const view = element<HTMLElement>("log");
  const title = element<HTMLElement>("log-title");
  const message = element<HTMLElement>("log-message");
  const list = element<HTMLElement>("log-list");
  const rowsView = element<HTMLElement>("log-rows");
  const longEdgesInput = element<HTMLInputElement>("log-long-edges");
  const refreshButton = element<HTMLButtonElement>("log-refresh");
  const closeButton = element<HTMLButtonElement>("log-close");

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

  element<HTMLElement>("log-long-edges-label").textContent = t("logLongEdges");
  refreshButton.textContent = t("logRefresh");
  closeButton.textContent = t("logClose");
  refreshButton.addEventListener("click", () => void startOver());
  closeButton.addEventListener("click", close);
  longEdgesInput.addEventListener("change", () => void changeLongEdges());
  list.addEventListener("scroll", scheduleDraw);
  list.addEventListener("keydown", onKey);
  rowsView.addEventListener("click", onClick);
  window.addEventListener("resize", scheduleDraw);

  function open(toShow: Repository): void {
    repository = toShow;
    selected = null;
    title.textContent = t("logTitle", { root: toShow.root });
    view.hidden = false;
    host.opened();
    list.scrollTop = 0;
    void settings().then((current) => {
      longEdgesInput.checked = current.showLongEdges;
    });
    void startOver();
    list.focus();
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
    rowsView.replaceChildren();
    view.hidden = true;
    host.closed();
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
    const read = readPageOf(repository, after, count);
    reading = read;
    return read;
  }

  /** Reads a page of the log of `shown` for {@link readPage}, and draws it. */
  async function readPageOf(shown: Repository, after: LogCursor | null, count: number): Promise<boolean> {
    latestRead += 1;
    const thisRead = latestRead;
    try {
      const page = await readLog(shown, after, count);
      if (thisRead !== latestRead) {
        endReadOf(page.next);
        return false;
      }
      reading = null;
      rows.push(...page.rows);
      next = page.next;
      columns = page.rows.reduce((widest, row) => Math.max(widest, columnsOf(row)), columns);
      message.replaceChildren(rows.length === 0 ? t("logEmpty") : "");
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

  /** Selects the commit `hash`, and scrolls it into view. */
  async function select(hash: string): Promise<void> {
    const index = await placeOf(hash);
    if (index < 0) {
      return;
    }
    selected = hash;
    const top = index * ROW_HEIGHT;
    if (top < list.scrollTop) {
      list.scrollTop = top;
    } else if (top + ROW_HEIGHT > list.scrollTop + list.clientHeight) {
      list.scrollTop = top + ROW_HEIGHT - list.clientHeight;
    }
    draw();
  }

  /** The left and right arrows go to the parent and the child, the up and down arrows along the list. */
  function onKey(event: KeyboardEvent): void {
    const index = rows.findIndex((row) => row.hash === selected);
    const current = rows[index];
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

  return { open, close };
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

function laneColor(column: number): string {
  return `log-lane-${column % LANE_COLORS}`;
}
