---
version: 1
slug: "ui-index-html"
primary_target: "ui/index.html"
related_targets: []
---

# gikli app shell and views

- **Scope:** the whole UI under `ui/`: window shell (Project tabs, Navigation bar, Status area), the views (Changes, Log, Diff, Stashes, Operation Log, Settings) and every dialog. Mode: Operate.
- **Audience and job:** developers reviewing local changes, sorting them into Changelists, committing, pulling and pushing, reading the Log, handling branches, Conflicts and Operations in progress. Long sessions, all day, beside an IDE.
- **Constraints:** the [visual] decisions of `docs/design/BRIEF.md`; no new features; every `data-testid` stays; WCAG AA in both themes.
- **Chosen direction:** Line Map, locked by the maintainer on 2026-11-27 after three rounds (the first roll, a Line Map follow-up in light and dark, and a bolder re-roll that kept Line Map on the table).
- **Maintainer decisions taken during the rounds:** the Navigation bar follows `docs/design/refs/nav-rail-ref.png`: icon groups in rounded capsules on the window ground, the main group (Changes, Log, Stashes) centred vertically, Operation Log and Settings in a capsule at the foot. The brief records it.
- **Memorable moment:** the Log reads as a transit map, and the same line language carries the current branch and the Changelists.
- **Unresolved:** exact contrast of every token pair, to be measured in #202; diff and merge colours inside CodeMirror, settled with the Diff view.

## Direction contract

THESIS: gikli draws git as a transit network. Branches are coloured lines, commits are stations, refs are line bullets in their line's colour, and each Changelist is a short line whose files are its stations. It refuses the category default of a grey screen with one accent and anonymous coloured graph strokes.

OWN-WORLD: grey window ground (light) or near-black (dark) under rounded white or graphite cards with 8 px gaps; capsule icon groups for navigation; pill-shaped controls; seven lane colours at bullet strength over the fixed accent `#2f6f5e` / `#5fb89f`; 3 px route lines; ringed stations; Inter, tabular figures, system monospace for hashes and code.

STORY: the user sees at once which branch they are on, which Changelist is active and what is uncommitted. They read history as lines meeting and parting, and act from the view header and the commit card.

FIRST VIEWPORT: native title bar; Project tabs on the ground below it, the active tab a raised card; Navigation bar capsules at the left, main group centred; view title with its actions on the ground; the view's cards (Changes: Changelists card plus 360 px commit card; Log: 220 px Branches card, graph card, 340 px details card); Status area on the ground with the current branch as a line bullet. The primary action (Commit) is the only filled accent pill in the commit card.

FORM: transit diagram (Vignelli's 1972 subway map, enamel signage), my first-ranked grounded candidate, shown as IMPECCABLE'S PICK in the first round; seed:27a127944395ad5e.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
