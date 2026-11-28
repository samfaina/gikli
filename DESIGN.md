---
name: gikli
description: A desktop Git client that draws git as a transit network, on rounded cards over a quiet ground.
colors:
  ground: "#e9ebee"
  panel: "#ffffff"
  field: "#eef0f2"
  rule: "#e3e6e9"
  row-rule: "#f2f3f5"
  ink: "#1d2328"
  ink-muted: "#58616a"
  ink-faint: "#5f6770"
  accent: "#2f6f5e"
  on-accent: "#ffffff"
  accent-wash: "#e6f0ec"
  selection: "#e3efea"
  lane-1: "#2f6f5e"
  lane-2: "#b5541c"
  lane-3: "#3767d0"
  lane-4: "#c43a4c"
  lane-5: "#8656bd"
  lane-6: "#1d7686"
  lane-7: "#86680a"
  on-lane: "#ffffff"
  status-modified: "#1f5fbf"
  status-added: "#2a7a2a"
  status-deleted: "#9b3d6b"
  status-renamed: "#00788a"
  status-copied: "#7a4fb5"
  status-untracked: "#b35c00"
  status-ignored: "#6f7a2a"
  status-conflicted: "#c4262e"
  info-bg: "#e6f0ec"
  info-ink: "#1f4d41"
  warning-bg: "#fff8c5"
  warning-ink: "#7d4e00"
  error-bg: "#fde8ea"
  error-ink: "#a1202d"
  ground-dark: "#141618"
  panel-dark: "#1d2023"
  field-dark: "#24282c"
  rule-dark: "#2c3135"
  row-rule-dark: "#23272a"
  ink-dark: "#e2e6e9"
  ink-muted-dark: "#a3abb2"
  ink-faint-dark: "#868f97"
  accent-dark: "#5fb89f"
  on-accent-dark: "#0e1f1a"
  accent-wash-dark: "#1f3530"
  selection-dark: "#22332f"
  lane-1-dark: "#5fb89f"
  lane-2-dark: "#f08f52"
  lane-3-dark: "#6f9bf0"
  lane-4-dark: "#ef6b7b"
  lane-5-dark: "#b28ae6"
  lane-6-dark: "#4cc3d4"
  lane-7-dark: "#e0bd4a"
  on-lane-dark: "#0e1214"
  status-modified-dark: "#6ea8ff"
  status-added-dark: "#6cc66c"
  status-deleted-dark: "#e07fb0"
  status-renamed-dark: "#4fc6d6"
  status-copied-dark: "#b996ea"
  status-untracked-dark: "#f0a050"
  status-ignored-dark: "#b5c06a"
  status-conflicted-dark: "#ff7b72"
  info-bg-dark: "#1f3530"
  info-ink-dark: "#a6e0cf"
  warning-bg-dark: "#3d3214"
  warning-ink-dark: "#f2cc60"
  error-bg-dark: "#3a1c20"
  error-ink-dark: "#ff9aa2"
typography:
  view-title:
    fontFamily: "Inter, system-ui, sans-serif"
    fontSize: "18px"
    fontWeight: 700
    lineHeight: "24px"
    letterSpacing: "-0.01em"
  title:
    fontFamily: "Inter, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 600
    lineHeight: "20px"
  body:
    fontFamily: "Inter, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: "20px"
    fontFeature: "\"tnum\" 1"
  body-strong:
    fontFamily: "Inter, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 600
    lineHeight: "20px"
  small:
    fontFamily: "Inter, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: "16px"
  label:
    fontFamily: "Inter, system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 600
    lineHeight: "16px"
    letterSpacing: "0.06em"
  bullet:
    fontFamily: "Inter, system-ui, sans-serif"
    fontSize: "11.5px"
    fontWeight: 700
    lineHeight: "16px"
  mono:
    fontFamily: "ui-monospace, \"Cascadia Mono\", \"SF Mono\", Menlo, Consolas, \"Liberation Mono\", monospace"
    fontSize: "12px"
    fontWeight: 400
    lineHeight: "18px"
rounded:
  xs: "4px"
  sm: "6px"
  md: "8px"
  lg: "10px"
  xl: "12px"
  dialog: "14px"
  pill: "999px"
spacing:
  "1": "2px"
  "2": "4px"
  "3": "6px"
  "4": "8px"
  "5": "12px"
  "6": "16px"
  "7": "20px"
  "8": "24px"
  "9": "32px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    typography: "{typography.body-strong}"
    rounded: "{rounded.pill}"
    padding: "0 14px"
    height: "28px"
  button-default:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.ink}"
    rounded: "{rounded.pill}"
    padding: "0 12px"
    height: "28px"
  button-ghost:
    textColor: "{colors.ink-muted}"
    rounded: "{rounded.pill}"
    padding: "0 8px"
    height: "28px"
  button-danger:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.status-conflicted}"
    rounded: "{rounded.pill}"
    padding: "0 12px"
    height: "28px"
  button-icon:
    textColor: "{colors.ink-muted}"
    rounded: "{rounded.pill}"
    size: "28px"
  text-field:
    backgroundColor: "{colors.field}"
    textColor: "{colors.ink}"
    rounded: "{rounded.pill}"
    padding: "0 12px"
    height: "28px"
  text-area:
    backgroundColor: "{colors.field}"
    textColor: "{colors.ink}"
    rounded: "{rounded.lg}"
    padding: "10px 12px"
  panel:
    backgroundColor: "{colors.panel}"
    rounded: "{rounded.xl}"
  project-tab-active:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
    padding: "0 10px 0 12px"
    height: "30px"
  project-tab:
    textColor: "{colors.ink-muted}"
    rounded: "{rounded.md}"
    padding: "0 10px 0 12px"
    height: "30px"
  nav-capsule:
    backgroundColor: "{colors.panel}"
    rounded: "{rounded.pill}"
    padding: "4px"
    width: "40px"
  nav-item-active:
    backgroundColor: "{colors.accent-wash}"
    textColor: "{colors.accent}"
    rounded: "{rounded.pill}"
    size: "32px"
  line-bullet:
    backgroundColor: "{colors.lane-1}"
    textColor: "{colors.on-lane}"
    typography: "{typography.bullet}"
    rounded: "{rounded.pill}"
    padding: "0 9px"
    height: "20px"
  list-row:
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    padding: "0 14px"
    height: "28px"
  list-row-selected:
    backgroundColor: "{colors.selection}"
    textColor: "{colors.ink}"
    height: "28px"
  banner-warning:
    backgroundColor: "{colors.warning-bg}"
    textColor: "{colors.warning-ink}"
    rounded: "{rounded.lg}"
    padding: "8px 12px"
  tooltip:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.panel}"
    typography: "{typography.small}"
    rounded: "{rounded.sm}"
    padding: "4px 8px"
---

<!-- DIRECTION: set with the maintainer in #199, before implementation. The tokens below are the starting values that #202 writes into `ui/src/tokens.css`; #202 measures their contrast, completes the inventory of tokens and components, and updates this file with what ships. -->

# Design System: gikli

## Overview

**Creative North Star: "Line Map"**

gikli draws git the way a transit diagram draws a city. Branches are coloured lines, commits are stations along them, and every ref is a line bullet in its line's colour, so a glance at the Log tells which work runs where and where lines meet. The same language carries the rest of the app: the current branch sits in the Status area as a bullet, and each Changelist in the Changes view is a short line whose files are its stations, with Unversioned Files drawn as a planned, dashed line.

Around that one move the shell stays calm and familiar. A quiet grey ground (light) or near-black (dark) holds rounded cards separated by 8 px gaps; the Navigation bar is two capsules of icons; controls are pills; text is Inter at a compact desktop size. Colour does real jobs and nothing else: lanes, file status, selection, the accent on the one primary action. The look is gikli's own on every OS; only the title bar is native.

This direction replaces the current UI, which is unstyled browser controls stacked in one column (`docs/design/current/`). Nothing of its look carries over except the accent and the file status colours.

**Key Characteristics:**
- Transit-diagram log graph: 3 px lines, ringed stations, merge commits as interchanges, refs as line bullets.
- Rounded cards on a ground, 8 px apart; nothing floats without a card.
- Pill controls, capsule navigation, centred main navigation group.
- Light and dark carry the same weight and follow the system, with no setting.
- Dense where people work (28 px rows), calm everywhere else.

## Colors

A restrained shell with a full palette of line colours: the ground and cards stay neutral so the lanes, status colours and the accent can carry meaning.

Every token exists once for light and once for dark (the `-dark` keys in the frontmatter). The theme follows the operating system through `prefers-color-scheme`; there is no setting to choose it.

### Primary
- **Junction Green** (accent, `#2f6f5e` light / `#5fb89f` dark): the primary action of each place (Commit, Push, Pull, the confirming button of a dialog), the active Navigation bar item, focus rings, the first lane of the graph and the active Changelist's line. Fixed by the brief; it does not change.
- **Junction Wash** (accent-wash): the ground of the active Navigation bar item and of informational banners.

### Secondary: the lanes
Seven line colours, used in order by the log graph, the Branches pane dots, ref bullets and the Changelist lines. Lane 1 is the accent. Light lanes are dark enough to carry white bullet text; dark lanes are bright enough to carry near-black bullet text (`on-lane`).
- **Lane 2, Signal Orange**; **Lane 3, Line Blue**; **Lane 4, Route Red**; **Lane 5, Violet Line**; **Lane 6, Harbour Teal**; **Lane 7, Ochre Line**. Values in the frontmatter (`lane-2` to `lane-7`, and their `-dark` pairs).

### Tertiary: file status and notifications
- **File status** (`status-*`): modified, added, deleted, renamed, copied, untracked, ignored, conflicted. These keep today's values, which follow the JetBrains convention; status is always also written as a word or letter, never colour alone.
- **Banners** (`info-*`, `warning-*`, `error-*`): background and text pairs for notification banners and inline messages.

### Neutral
- **Ground** (`ground`): the window behind the cards: tab strip, Navigation bar, view header and Status area sit on it.
- **Card** (`panel`): every working area.
- **Field** (`field`): text fields, text areas, the search box, selects: a sunken fill instead of an outline.
- **Rule** (`rule`, `row-rule`): hairlines inside cards and between rows.
- **Ink** (`ink`, `ink-muted`, `ink-faint`): primary text; secondary text (authors, paths, ghost buttons); metadata (dates, hashes, counts). `ink-faint` is the lightest colour text may take.

### Named Rules
**The Lane Rule.** A lane colour means a line: a branch, a ref or a Changelist. It never decorates a button, a heading or an empty state.

**The One Filled Pill Rule.** In any card, at most one control is filled with the accent: the action the place exists for.

**The Word-Beside-Colour Rule.** File status, notifications and conflicts always say what they are in text or a letter. Colour confirms; it never carries the meaning alone.

## Typography

**Interface:** Inter, bundled with the app (OFL), with `system-ui, sans-serif` as fallback. Provisional per the brief; to be revisited.
**Code, diffs and hashes:** the system monospace (`ui-monospace`, Cascadia Mono, SF Mono, Menlo, Consolas, Liberation Mono).

**Character:** a compact desktop scale where rank comes from weight more than size; tabular figures everywhere so hashes, dates and counts line up in columns.

### Hierarchy
- **View title** (700, 18/24 px, -0.01em): the title of each view in its header (Changes, Log, Stashes, Operation Log, Settings).
- **Title** (600, 15/20 px): dialog titles and the commit subject in the details card.
- **Body** (400, 13/20 px): everything else: rows, labels, buttons (500), messages.
- **Body strong** (600, 13/20 px): card headings (Branches, Commit), file names, the subject of the selected commit.
- **Small** (400, 12/16 px): the Status area, metadata, tooltips.
- **Label** (600, 11/16 px, 0.06em, uppercase): group headings inside cards only (LOCAL, the remote name, CHANGED FILES).
- **Bullet** (700, 11.5/16 px): text inside line bullets.
- **Mono** (400, 12/18 px): hashes, diffs, merge editor, the Operation Log's commands.

### Named Rules
**The Four Sizes Rule.** Interface text uses 18, 15, 13 and 12 px (11 px only for uppercase labels, 11.5 px only in bullets). A new size needs a new token and a line here.

**The Tabular Rule.** `font-variant-numeric: tabular-nums` is set once on the root; nothing turns it off.

## Layout

The window, from the top:
1. **Native title bar.**
2. **Project tabs** (44 px strip on the ground), starting after the Navigation bar column; the + ends the row.
3. **Navigation bar** (52 px column on the ground, about 48 px as the brief asks): the main capsule (Changes, Log, Stashes) is centred vertically in the column; the foot capsule (Operation Log, Settings) sits 36 px above the bottom edge. This centring is the maintainer's decision from the reference `docs/design/refs/nav-rail-ref.png`, recorded in the brief.
4. **View header** (44 px, on the ground): the view title, then the view's actions beside it; the Repository selector, when shown, sits left of the title.
5. **Banners** under the view header, when there are any.
6. **The view's cards**, 8 px apart, with 8 px between the last card and the window's right edge.
7. **Operation strip** above the Status area while a long operation runs.
8. **Status area** (32 px, on the ground).

Column widths: Changes has the Changelists card (flexible) and the commit card (360 px). Log has the Branches card (220 px), the graph card (flexible) and the details card (340 px). Columns are neither resizable nor collapsible. Below about 900 px of window width, the side columns of a view go below its main column. The window keeps today's minimum size.

Spacing follows a 4 px base (tokens `1` to `9`: 2, 4, 6, 8, 12, 16, 20, 24, 32 px). Rows in lists, trees and the log are 28 px; controls are 28 px high; the commit card's main buttons are 34 px.

### Named Rules
**The Density Rule.** Lists are 28 px per row with no extra padding. Space goes between groups (a Changelist and the next), never inside rows.

## Elevation & Depth

Depth is tonal first: cards are lighter (light) or lighter-than-ground (dark) than the ground they sit on. Shadows are minimal and structural.

### Shadow Vocabulary
- **Card** (light `0 1px 2px rgba(20,30,40,.06), 0 0 0 1px rgba(20,30,40,.05)`; dark `0 0 0 1px rgba(255,255,255,.04)`): every card, the active Project tab, the Navigation bar capsules, default buttons on the ground.
- **Overlay** (light `0 8px 24px rgba(20,30,40,.16), 0 0 0 1px rgba(20,30,40,.06)`; dark `0 8px 24px rgba(0,0,0,.5), 0 0 0 1px rgba(255,255,255,.06)`): context menus, selects' lists, tooltips, dialogs.
- **Scrim** (light `rgba(10,14,18,.40)`; dark `rgba(0,0,0,.55)`): behind modal dialogs.

### Named Rules
**The Two Shadows Rule.** There are exactly two shadows, card and overlay. Hover never lifts anything.

## Shapes

Rounded, from the GitButler reference, in a fixed scale (`rounded` tokens):
- **Pill** (999 px): buttons, single-line fields, selects, the search box, line bullets, Navigation bar capsules and items.
- **12 px**: cards.
- **14 px**: dialogs.
- **10 px**: text areas, banners, context menus.
- **8 px**: Project tabs.
- **6 px**: tooltips.
- **4 px**: checkboxes.

Lines in the graph and in Changelists are 3 px with round caps; stations are circles of 5.5 px radius with a 3 px ring, filled with the card colour; a merge commit is an interchange, a 16 × 14 px capsule ring.

## Components

The base components (buttons, inputs, checkboxes, segmented choices, selects, list rows with file status, group headers, context menus, tooltips) are built in #202, which completes this inventory with each component's states and when to use it. Here is the direction each must follow.

### Buttons
- **Shape:** pill (999 px), 28 px high; icons from Lucide at 16 px before the label.
- **Primary:** accent fill, `on-accent` text, weight 600. One per card (The One Filled Pill Rule).
- **Default:** card fill with the card shadow when on the ground; on a card, `field` fill.
- **Ghost:** no fill, `ink-muted` text; the view header's actions (Refresh, Fetch, Pull…, Push…) and row actions (Rename…, Comment…).
- **Danger:** default shape with `status-conflicted` text; filled red only in the confirming button of a destructive dialog.
- **Icon:** 28 px circle, ghost colours, always with a tooltip.
- **Hover / Focus / Press:** hover darkens the fill one step (or adds `field` to ghosts); focus shows a 2 px accent ring with 2 px offset; press darkens one more step. No movement.

### Inputs and fields
- **Style:** `field` fill, no outline at rest, pill for single-line, 10 px for text areas.
- **Focus:** 2 px accent ring.
- **Error:** `error-ink` ring and a message below in `error-ink`.
- **Checkboxes:** 15 px, 4 px radius, 1.5 px `ink-faint` border; checked fills with the accent.

### Cards
- **Corner:** 12 px. **Background:** `panel`. **Shadow:** card. **Padding:** 14 px sides; card headings 40 px high.

### Navigation
- **Project tabs:** 30 px, 8 px radius, Inter 500. The active tab is a card (panel fill, card shadow); others are text on the ground. Each tab shows the folder name, a close button, and one marker at a time, by priority: a spinner while a long operation runs; a `warning-ink` dot for an Operation in progress or an unread notification; an accent dot for uncommitted changes. The tab's tooltip says which.
- **Navigation bar:** two capsules (40 px wide, pill, card fill and shadow) holding 32 px circular items with 16 px Lucide icons. Active item: `accent-wash` fill and accent icon. Hover: `field` fill and `ink` icon. Each item has a tooltip to its right.
- **Status area:** on the ground, small text; the current branch as a line bullet in its lane colour, then ahead/behind counts, then the Repository.

### Line bullet (signature)
A 20 px pill in a lane colour with `on-lane` text: local branches. Remote-tracking refs are the same pill outlined (2 px) in the lane colour with lane-coloured text. Tags are outlined in `ink-faint` with `ink` text. HEAD is not a separate label: the HEAD branch's bullet comes first and its station is filled.

### Log graph (signature)
Lanes take colours in order from `lane-1`. Lines are 3 px, round caps, curving between lanes with smooth bezier corners rather than diagonals. Commits are ringed stations; merge commits are interchanges; the commit at HEAD has a filled station. Rows are 28 px; columns after the graph are message, author, date and hash (mono, `ink-faint`). Only the look comes from Fork; collapsing branches is out of this work.

### Changelist line (signature)
In the Changes view each Changelist header carries its name as a line bullet; a 3 px line in that colour runs down from the first file's station to the last, and each file row has a station on it before its checkbox. The active Changelist takes `lane-1`; others take the next lanes in order. Unversioned Files is an outlined bullet in `status-untracked` with a dashed line (5 px dash, 4 px gap). The Conflicts group uses `status-conflicted`. Status is written at the right of each row in its status colour.

### Notification banners
Compact, 10 px radius, a 16 px Lucide icon (info, triangle-alert, circle-x) in the banner's ink colour, the message, its actions as ghost buttons, and a dismiss button. They sit under the view header, inside the tab of their Project, until dismissed.

### Operation strip
A card-coloured strip above the Status area: the operation's text, a 4 px accent progress bar (indeterminate when git reports no progress), and Cancel as a ghost button.

### Dialogs
Centred modal over the scrim, 14 px radius, overlay shadow, 20 px padding, title in the Title style (a Prompt's title names its Project), actions bottom-right with the primary last.

### Context menus and tooltips
Menus: card fill, 10 px radius, overlay shadow, 4 px padding, 28 px items with 8 px radius hover in `field`. Tooltips: `ink` fill with `panel` text, small type, 6 px radius, after a 500 ms delay.

### Diff and merge (CodeMirror 6)
Mono 12/18 px. Added lines take a light tint of `status-added`, removed lines of `status-deleted`, changed words a stronger tint of the same; conflict blocks are outlined in `status-conflicted`. Gutters in `ink-faint`. The exact tints are set with the Diff view, taking the Sublime Merge screenshots as loose inspiration.

### Iconography
Lucide only, inline SVG of the icons actually used (ISC notice shipped with the app), 16 px, stroke 2, `currentColor`, round caps and joins. No icon fonts and no other sets.

### Motion
Quiet and short. Colour changes on hover and press: 120 ms ease-out. Menus, tooltips and dialogs appear with opacity and a 4 px rise over 160 ms, `cubic-bezier(0.2, 0, 0, 1)`, and leave in 100 ms. Spinners turn once every 800 ms. Nothing moves on page changes between views. Under `prefers-reduced-motion: reduce`, nothing moves: overlays only fade, and spinners pulse their opacity instead of turning.

## Do's and Don'ts

### Do:
- **Do** take every colour, radius, spacing and shadow from `ui/src/tokens.css`, the tokens file: custom properties for light on `:root` and for dark under `@media (prefers-color-scheme: dark)`, imported once. `ui/src/styles.css` stays the one stylesheet for components and holds no literal values.
- **Do** define each new token in both themes at once, and add it to this file in the same change.
- **Do** keep lane colours for lines, refs and Changelists only (The Lane Rule).
- **Do** give every control a visible 2 px accent focus ring and keep every action reachable by keyboard.
- **Do** check that text meets WCAG AA in both themes, `ink-faint` on `ground` included.
- **Do** keep the main Navigation bar group centred vertically and the foot group at the foot.

### Don't:
- **Don't** add a theme setting; the system decides.
- **Don't** change the accent (`#2f6f5e` light, `#5fb89f` dark).
- **Don't** use icons outside Lucide or bundle the whole Lucide set.
- **Don't** lift on hover, add gradients, or add a third shadow.
- **Don't** let colour alone carry status (The Word-Beside-Colour Rule).
- **Don't** copy GitButler's code or assets; it is visual inspiration only (FSL).
