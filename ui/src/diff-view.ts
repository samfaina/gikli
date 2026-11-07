// The side-by-side diff of a changed file. The core decides what differs, what collapses and which
// file comes next; this shows it in CodeMirror (ADR-0002), which keeps the two sides aligned and
// scrolling together and follows the user's edits until they save.

import { confirm } from "@tauri-apps/plugin-dialog";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { Change, MergeView, diff as diffOfEdit } from "@codemirror/merge";
import { Compartment, EditorState, RangeSetBuilder, StateEffect, StateField, Text, type Extension } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, drawSelection, highlightSpecialChars, keymap, lineNumbers, type DecorationSet } from "@codemirror/view";

import {
  readDiff,
  saveLocalFile,
  type FileDiff,
  type LineRange,
  type LocalReadOnly,
  type Repository,
  type TextDiff,
} from "./core";
import { element, hasShortcutKey, isMac } from "./dom";
import { t, type StringKey } from "./strings";

/** What the diff view needs from the window around it. */
export interface DiffViewHost {
  /** What to show for an error from the core. */
  errorView(error: unknown): HTMLElement;
  /** The diff took the place of the changes view. */
  opened(): void;
  /** The changes view is back, and the user was last looking at the file at `path`. */
  closed(path: string): void;
}

export interface DiffView {
  /** Shows the diff of the file at `path`, first saving what the user edited in the file shown. */
  open(repository: Repository, path: string): Promise<void>;
  /** Saves what the user edited and goes back to the changes view. Stays when that fails. */
  close(): Promise<void>;
  /** Saves what the user edited, and returns whether nothing is left unsaved. */
  saveEdits(): Promise<boolean>;
  /** Reads the file shown again, unless it holds edits the user has not saved. */
  refresh(): Promise<void>;
  /** The root of the Repository of the file shown, if any. */
  shownRoot(): string | null;
}

/** The file shown, with what it takes to show it and to save it. */
interface Shown {
  diff: FileDiff;
  /** The editors, when the file has a text diff. */
  merge: MergeView | null;
  /** The local side as it is on disk. */
  savedLocal: Text;
}

const READ_ONLY_LABELS: Record<LocalReadOnly, StringKey> = {
  turnedOff: "diffLocalSideTurnedOff",
  noLocalFile: "diffLocalSideMissing",
  notUtf8: "diffLocalSideNotUtf8",
  symbolicLink: "diffLocalSideLink",
};

/** Brings back the lines of the collapsed region with this index. */
const expandRegion = StateEffect.define<number>();

/** The marker in place of the lines of a collapsed region. A click brings them back on both sides. */
class CollapsedLines extends WidgetType {
  constructor(
    readonly count: number,
    readonly expand: () => void,
  ) {
    super();
  }

  override eq(other: CollapsedLines): boolean {
    return this.count === other.count;
  }

  override toDOM(): HTMLElement {
    const marker = document.createElement("div");
    marker.className = "cm-collapsedLines";
    marker.dataset.testid = "diff-collapsed-lines";
    marker.textContent = t("diffUnchangedLines", { count: String(this.count) });
    marker.addEventListener("click", this.expand);
    return marker;
  }

  override ignoreEvent(event: Event): boolean {
    return event instanceof MouseEvent;
  }
}

/** Collapses `regions`, lines of the side this goes on. `expand` brings one back by its index. */
function collapsedLines(regions: LineRange[], expand: (index: number) => void): Extension {
  return StateField.define<DecorationSet>({
    create(state) {
      const hidden = new RangeSetBuilder<Decoration>();
      regions.forEach((region, index) => {
        if (region.end <= state.doc.lines) {
          hidden.add(
            state.doc.line(region.start + 1).from,
            state.doc.line(region.end).to,
            Decoration.replace({
              block: true,
              widget: new CollapsedLines(region.end - region.start, () => expand(index)),
              region: index,
            }),
          );
        }
      });
      return hidden.finish();
    },
    update(hidden, transaction) {
      let updated = hidden.map(transaction.changes);
      for (const effect of transaction.effects) {
        if (effect.is(expandRegion)) {
          updated = updated.update({ filter: (_from, _to, decoration) => decoration.spec.region !== effect.value });
        }
      }
      return updated;
    },
    provide: (field) => EditorView.decorations.from(field),
  });
}

/** The position in `doc` where the line `line`, counted from 0, starts, or its end past the last line. */
function positionOfLine(doc: Text, line: number): number {
  return line < doc.lines ? doc.line(line + 1).from : doc.length;
}

/**
 * A document of the lines of `text`, which has `\n` between them. CodeMirror takes a carriage return
 * in a string for a line break as well, and to git it is part of the line.
 */
function documentOf(text: string | null): Text {
  return Text.of((text ?? "").split("\n"));
}

export function createDiffView(host: DiffViewHost): DiffView {
  const section = element<HTMLElement>("diff");
  const title = element<HTMLElement>("diff-title");
  const message = element<HTMLElement>("diff-message");
  const headLabel = element<HTMLElement>("diff-head-label");
  const localLabel = element<HTMLElement>("diff-local-label");
  const content = element<HTMLElement>("diff-content");
  const saveButton = element<HTMLButtonElement>("diff-save");
  const previousFileButton = element<HTMLButtonElement>("diff-previous-file");
  const nextFileButton = element<HTMLButtonElement>("diff-next-file");

  const key = t(isMac() ? "shortcutKeyMac" : "shortcutKey");
  const setUpButton = (id: string, label: StringKey, shortcut: StringKey, act: () => void): void => {
    const made = element<HTMLButtonElement>(id);
    made.textContent = t(label);
    made.title = t("diffWithShortcut", { action: t(label), shortcut: t(shortcut, { key }) });
    made.addEventListener("click", act);
  };

  setUpButton("diff-previous-difference", "diffPreviousDifference", "shortcutPreviousDifference", () => void moveToDifference(-1));
  setUpButton("diff-next-difference", "diffNextDifference", "shortcutNextDifference", () => void moveToDifference(1));
  setUpButton("diff-previous-file", "diffPreviousFile", "shortcutPreviousFile", () => void moveToFile("previous"));
  setUpButton("diff-next-file", "diffNextFile", "shortcutNextFile", () => void moveToFile("next"));
  setUpButton("diff-save", "diffSave", "shortcutSave", () => void saveEdits());
  setUpButton("diff-close", "diffClose", "shortcutClose", () => void close());

  section.addEventListener("keydown", (event) => {
    const withModifier = hasShortcutKey(event);
    if (event.key === "F7") {
      event.preventDefault();
      void moveToDifference(event.shiftKey ? -1 : 1);
    } else if (withModifier && event.altKey && event.key === "ArrowLeft") {
      event.preventDefault();
      void moveToFile("previous");
    } else if (withModifier && event.altKey && event.key === "ArrowRight") {
      event.preventDefault();
      void moveToFile("next");
    } else if (withModifier && !event.altKey && event.key.toLowerCase() === "s") {
      event.preventDefault();
      void saveEdits();
    } else if (event.key === "Escape") {
      event.preventDefault();
      void close();
    }
  });

  /** CodeMirror picks its colors for the theme the operating system follows. */
  const dark = window.matchMedia("(prefers-color-scheme: dark)");
  const darkTheme = new Compartment();
  dark.addEventListener("change", () => {
    if (shown?.merge != null) {
      dispatchOnBothSides(shown.merge, darkTheme.reconfigure(EditorView.darkTheme.of(dark.matches)));
    }
  });

  let shown: Shown | null = null;
  /** Counts the files asked for: a read for an earlier one no longer counts, whatever it was for. */
  let lastOpen = 0;
  /** Counts the times the file shown was read again, or saved: the answer to an earlier read no longer counts. */
  let lastRefresh = 0;
  /** The save running, so that nothing else starts from what it is about to change. */
  let saving: Promise<boolean> | null = null;

  function hasEdits(): boolean {
    return shown?.merge != null && !shown.merge.b.state.doc.eq(shown.savedLocal);
  }

  function say(text: string): void {
    message.replaceChildren(document.createTextNode(text));
  }

  /** Shows `diff`. With `keepPlace`, stays where the user was in the same file. */
  function show(diff: FileDiff, keepPlace: boolean): void {
    const scrollTop = keepPlace ? (shown?.merge?.dom.scrollTop ?? 0) : 0;
    shown?.merge?.destroy();
    content.replaceChildren();
    message.replaceChildren();
    const merge = diff.content.kind === "text" ? showText(diff.content) : null;
    shown = {
      diff,
      merge,
      savedLocal: merge?.b.state.doc ?? Text.empty,
    };
    if (diff.content.kind === "binary") {
      say(t("diffBinaryFile"));
    } else if (diff.content.kind === "folder") {
      say(t("diffFolder"));
    } else if (diff.content.differences.length === 0) {
      say(t("diffNoDifferences"));
    }
    updateControls();
    if (merge !== null) {
      if (keepPlace) {
        merge.dom.scrollTop = scrollTop;
      } else {
        goToFirstDifference(merge);
        merge.b.focus();
      }
    } else if (!keepPlace) {
      element<HTMLElement>("diff-close").focus();
    }
  }

  /** The two editors of a text diff, in the view. */
  function showText(text: TextDiff): MergeView {
    const headDocument = documentOf(text.head);
    const localDocument = documentOf(text.local);
    const head = headDocument.toString();
    const local = localDocument.toString();
    const differences = text.differences.map(
      ({ head: inHead, local: inLocal }) =>
        new Change(
          positionOfLine(headDocument, inHead.start),
          positionOfLine(headDocument, inHead.end),
          positionOfLine(localDocument, inLocal.start),
          positionOfLine(localDocument, inLocal.end),
        ),
    );
    const expand = (index: number): void => dispatchOnBothSides(merge, expandRegion.of(index));
    const shared: Extension = [
      lineNumbers(),
      highlightSpecialChars(),
      drawSelection(),
      darkTheme.of(EditorView.darkTheme.of(dark.matches)),
    ];
    const merge: MergeView = new MergeView({
      parent: content,
      a: {
        doc: headDocument,
        extensions: [
          shared,
          EditorState.readOnly.of(true),
          collapsedLines(text.collapsed.map((region) => region.head), expand),
        ],
      },
      b: {
        doc: localDocument,
        extensions: [
          shared,
          EditorState.readOnly.of(text.localReadOnly !== null),
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
          EditorView.updateListener.of((update) => {
            if (update.docChanged) {
              updateControls();
            }
          }),
          collapsedLines(text.collapsed.map((region) => region.local), expand),
        ],
      },
      highlightChanges: false,
      gutter: true,
      diffConfig: {
        // What the core says for the file as it read it. It knows of line endings, and CodeMirror
        // does not. CodeMirror follows the user's edits from there with its own diff, also after a
        // save, as drawing the editors again would lose the cursor and the undo history.
        override: (a, b) => (a === head && b === local ? differences : diffOfEdit(a, b, { scanLimit: 500 })),
      },
    });
    return merge;
  }

  function goToFirstDifference(merge: MergeView): void {
    const first = merge.chunks[0];
    if (first !== undefined) {
      placeCursor(merge, first.fromB);
    }
  }

  function placeCursor(merge: MergeView, position: number): void {
    merge.b.dispatch({
      selection: { anchor: position },
      effects: EditorView.scrollIntoView(position, { y: "center" }),
    });
    merge.b.focus();
  }

  function updateControls(): void {
    if (shown === null) {
      return;
    }
    const { diff } = shown;
    title.textContent =
      diff.originalPath === null ? diff.path : t("diffTitleMoved", { path: diff.path, original: diff.originalPath });
    previousFileButton.disabled = diff.previousFile === null;
    nextFileButton.disabled = diff.nextFile === null;
    saveButton.hidden = diff.content.kind !== "text" || diff.content.localReadOnly !== null;
    saveButton.disabled = !hasEdits();
    const text = diff.content.kind === "text" ? diff.content : null;
    headLabel.textContent = t(text !== null && text.head === null ? "diffHeadSideMissing" : "diffHeadSide");
    const readOnly = text?.localReadOnly ?? null;
    const edited = hasEdits() ? "diffLocalSideEdited" : "diffLocalSide";
    localLabel.textContent = t(readOnly === null ? edited : READ_ONLY_LABELS[readOnly]);
    section.dataset.edited = String(hasEdits());
  }

  /** Moves to the next difference after the cursor or the one before it, and on to the next file after the last one when the core says so. */
  async function moveToDifference(direction: 1 | -1): Promise<void> {
    if (shown === null) {
      return;
    }
    const { diff, merge } = shown;
    if (merge === null) {
      // A file without a text diff has one place to look, so the review moves on from it.
      if (direction === 1 && diff.afterLastDifference !== null) {
        await open(diff.repository, diff.afterLastDifference);
      }
      return;
    }
    const cursor = merge.b.state.selection.main.head;
    const chunks = merge.chunks;
    const target =
      direction === 1
        ? chunks.find((chunk) => chunk.fromB > cursor)
        : [...chunks].reverse().find((chunk) => chunk.fromB < cursor);
    if (target !== undefined) {
      if (message.textContent === t("diffNoNextDifference") || message.textContent === t("diffNoPreviousDifference")) {
        message.replaceChildren();
      }
      placeCursor(merge, target.fromB);
    } else if (direction === 1 && diff.afterLastDifference !== null) {
      await open(diff.repository, diff.afterLastDifference);
    } else {
      say(t(direction === 1 ? "diffNoNextDifference" : "diffNoPreviousDifference"));
    }
  }

  async function moveToFile(direction: "previous" | "next"): Promise<void> {
    const path = direction === "previous" ? shown?.diff.previousFile : shown?.diff.nextFile;
    if (shown !== null && path != null) {
      await open(shown.diff.repository, path);
    }
  }

  async function open(repository: Repository, path: string): Promise<void> {
    if (!(await saveEdits())) {
      return;
    }
    const asked = ++lastOpen;
    try {
      const diff = await readDiff(repository, path);
      if (asked === lastOpen) {
        section.hidden = false;
        host.opened();
        show(diff, false);
      }
    } catch (error) {
      if (asked !== lastOpen) {
        return;
      }
      if (shown === null) {
        // Nothing to go back from, so the error shows in the view of the file asked for.
        section.hidden = false;
        host.opened();
        title.textContent = path;
        content.replaceChildren();
        headLabel.textContent = "";
        localLabel.textContent = "";
        saveButton.hidden = true;
        previousFileButton.disabled = true;
        nextFileButton.disabled = true;
        lastAskedFor = path;
      }
      message.replaceChildren(host.errorView(error));
    }
  }

  /** The file asked for when reading it failed, which is then what the user was looking at. */
  let lastAskedFor: string | null = null;

  function saveEdits(): Promise<boolean> {
    if (saving !== null) {
      return saving;
    }
    if (shown === null || shown.merge === null || !hasEdits()) {
      return Promise.resolve(true);
    }
    lastRefresh++;
    saving = saveShown(shown, shown.merge).finally(() => {
      saving = null;
      updateControls();
    });
    return saving;
  }

  async function saveShown(file: Shown, merge: MergeView): Promise<boolean> {
    if (file.diff.content.kind !== "text") {
      return true;
    }
    const edited = merge.b.state.doc;
    try {
      const saved = await saveLocalFile(file.diff.repository, file.diff.path, edited.toString(), file.diff.content.localVersion);
      // The editors stay as they are, with the user's cursor and undo history, and already show this text.
      file.diff = saved;
      file.savedLocal = edited;
      message.replaceChildren();
      if (saved.content.kind === "text" && saved.content.differences.length === 0) {
        say(t("diffNoDifferences"));
      }
      return true;
    } catch (error) {
      message.replaceChildren(host.errorView(error), discardEditsButton(file));
      return false;
    }
  }

  /**
   * The way out of edits that cannot be saved, whatever the reason, as moving on and closing wait for
   * the save. It discards them after asking, and shows the file as it is now.
   */
  function discardEditsButton(file: Shown): HTMLElement {
    const reload = document.createElement("button");
    reload.type = "button";
    reload.dataset.testid = "diff-discard-edits";
    reload.textContent = t("diffDiscardEdits");
    reload.addEventListener("click", async () => {
      if (!(await confirm(t("confirmDiffDiscardEdits", { path: file.diff.path }), { kind: "warning" }))) {
        return;
      }
      try {
        show(await readDiff(file.diff.repository, file.diff.path), true);
      } catch (error) {
        message.replaceChildren(host.errorView(error));
      }
    });
    return reload;
  }

  async function close(): Promise<void> {
    if (section.hidden || !(await saveEdits())) {
      return;
    }
    lastOpen++;
    const path = shown?.diff.path ?? lastAskedFor ?? "";
    shown?.merge?.destroy();
    shown = null;
    lastAskedFor = null;
    content.replaceChildren();
    message.replaceChildren();
    section.hidden = true;
    host.closed(path);
  }

  async function refresh(): Promise<void> {
    if (shown === null || saving !== null || hasEdits()) {
      return;
    }
    const { diff } = shown;
    const opened = lastOpen;
    const asked = ++lastRefresh;
    const stillCounts = (): boolean => opened === lastOpen && asked === lastRefresh;
    try {
      const fresh = await readDiff(diff.repository, diff.path);
      if (!stillCounts() || shown === null || saving !== null || hasEdits()) {
        return;
      }
      if (sameEditors(shown.diff, fresh)) {
        shown.diff = fresh;
        updateControls();
      } else {
        show(fresh, true);
      }
    } catch (error) {
      if (stillCounts()) {
        message.replaceChildren(host.errorView(error));
      }
    }
  }

  return {
    open,
    close,
    saveEdits,
    refresh,
    shownRoot: () => shown?.diff.repository.root ?? null,
  };
}

function dispatchOnBothSides(merge: MergeView, effects: StateEffect<unknown>): void {
  merge.a.dispatch({ effects });
  merge.b.dispatch({ effects });
}

/** Whether the editors of `shown` also show `fresh`, so nothing moves under the user. */
function sameEditors(shown: FileDiff, fresh: FileDiff): boolean {
  if (shown.content.kind !== "text" || fresh.content.kind !== "text") {
    return shown.content.kind === fresh.content.kind;
  }
  return (
    shown.content.head === fresh.content.head &&
    shown.content.local === fresh.content.local &&
    shown.content.localReadOnly === fresh.content.localReadOnly &&
    JSON.stringify(shown.content.differences) === JSON.stringify(fresh.content.differences)
  );
}
