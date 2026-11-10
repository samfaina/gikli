// The log: the commit list with its graph. The core reads the commits and lays out the graph a page
// at a time; this draws only the rows in view, and asks for the next page as the user nears the end.

import { readLog, isFailure, type GraphLine, type LogCursor, type LogRow, type Repository } from "./core";
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
/** The height of a row in pixels, the same as `--log-row-height` in the styles. */
const ROW_HEIGHT = 26;
/** Rows drawn beyond each end of the view, and how close to the end the next page is asked for. */
const ROWS_BEYOND_VIEW = 30;
/** The width of a lane of the graph in pixels. */
const LANE_WIDTH = 14;
/** The number of lane colors in the styles. */
const LANE_COLORS = 8;
const SHORT_HASH_LENGTH = 8;
const SVG = "http://www.w3.org/2000/svg";

const dates = new Intl.DateTimeFormat(undefined, { dateStyle: "short", timeStyle: "short" });

export function createLogView(host: LogViewHost): LogView {
  const view = element<HTMLElement>("log");
  const title = element<HTMLElement>("log-title");
  const message = element<HTMLElement>("log-message");
  const list = element<HTMLElement>("log-list");
  const rowsView = element<HTMLElement>("log-rows");
  const refreshButton = element<HTMLButtonElement>("log-refresh");
  const closeButton = element<HTMLButtonElement>("log-close");

  let repository: Repository | null = null;
  let rows: LogRow[] = [];
  let next: LogCursor | null = null;
  let loading = false;
  /** Tells the answer to the latest read from one that a refresh or a close overtook. */
  let reads = 0;
  let columns = 1;
  let drawScheduled = false;

  refreshButton.textContent = t("logRefresh");
  closeButton.textContent = t("logClose");
  refreshButton.addEventListener("click", () => void startOver());
  closeButton.addEventListener("click", close);
  list.addEventListener("scroll", scheduleDraw);
  window.addEventListener("resize", scheduleDraw);

  function open(toShow: Repository): void {
    repository = toShow;
    title.textContent = t("logTitle", { root: toShow.root });
    view.hidden = false;
    host.opened();
    list.scrollTop = 0;
    void startOver();
    list.focus();
  }

  function close(): void {
    if (repository === null) {
      return;
    }
    repository = null;
    reads += 1;
    rows = [];
    next = null;
    loading = false;
    rowsView.replaceChildren();
    view.hidden = true;
    host.closed();
  }

  /** Reads the log again from its newest commit. */
  async function startOver(): Promise<void> {
    rows = [];
    next = null;
    columns = 1;
    message.replaceChildren(t("logReading"));
    await readPage(null);
  }

  /** Reads the page after `after`, or the first page. */
  async function readPage(after: LogCursor | null): Promise<void> {
    if (repository === null) {
      return;
    }
    reads += 1;
    const read = reads;
    loading = true;
    try {
      const page = await readLog(repository, after, PAGE_SIZE);
      if (read !== reads) {
        return;
      }
      loading = false;
      rows = rows.concat(page.rows);
      next = page.next;
      columns = page.rows.reduce((widest, row) => Math.max(widest, columnsOf(row)), columns);
      message.replaceChildren(rows.length === 0 ? t("logEmpty") : "");
      draw();
    } catch (error) {
      if (read !== reads) {
        return;
      }
      loading = false;
      if (isFailure(error) && error.kind === "logChanged") {
        await startOver();
      } else {
        message.replaceChildren(host.errorView(error));
      }
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
    rowsView.replaceChildren(...rows.slice(start, end).map((row, index) => rowView(row, start + index, columns)));
    if (next !== null && !loading && end + ROWS_BEYOND_VIEW >= rows.length) {
      void readPage(next);
    }
  }

  return { open, close };
}

/** How many columns of the graph `row` takes. */
function columnsOf(row: LogRow): number {
  const lines = [...row.graph.above, ...row.graph.below];
  return Math.max(row.graph.column, ...lines.flatMap((line) => [line.from, line.to])) + 1;
}

function rowView(row: LogRow, index: number, columns: number): HTMLElement {
  const view = document.createElement("div");
  view.className = "log-row";
  view.dataset.testid = "log-row";
  view.dataset.hash = row.hash;
  view.style.top = `${index * ROW_HEIGHT}px`;
  view.append(
    graphView(row, columns),
    cell("log-subject", row.subject),
    cell("log-author", row.author),
    cell("log-date", dates.format(new Date(row.committedAt * 1000))),
    cell("log-hash", row.hash.slice(0, SHORT_HASH_LENGTH)),
  );
  return view;
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
  graph.setAttribute("aria-hidden", "true");
  const middle = ROW_HEIGHT / 2;
  for (const line of row.graph.above) {
    graph.append(lineView(line, 0, middle));
  }
  for (const line of row.graph.below) {
    graph.append(lineView(line, middle, ROW_HEIGHT));
  }
  const node = document.createElementNS(SVG, "circle");
  node.classList.add(laneColor(row.graph.column));
  node.setAttribute("cx", String(laneCenter(row.graph.column)));
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

function laneColor(column: number): string {
  return `log-lane-${column % LANE_COLORS}`;
}
