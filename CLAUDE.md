## Language

- Everything in the project is written in English: code, comments, identifiers, UI text, commit messages, issues, pull requests and all documentation (`DESIGN.md`, `GLOSSARY.md`, ADRs, briefs, specs).
- The conversation with me is in Spanish: questions, explanations and summaries. Write to files and GitHub in English even when we discuss them in Spanish.

## Agent skills

### Issue tracker

Issues are managed in GitHub Issues (`samfaina/gikli`) with the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

The five canonical default labels: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the root. See `docs/agents/domain.md`.

## UI design

- Before any UI work (views, dialogs or styles in `ui/`), read `DESIGN.md` at the root. It holds the visual direction and the inventory of tokens and components, and it takes precedence over whatever the current CSS looks like.
- Use design tokens for every color, radius, spacing and shadow. No literal values outside the tokens file (its location is in `DESIGN.md`).
- When a task adds or changes a token or component, update `DESIGN.md` in the same change.
- Tickets that change `ui/` are implemented with `/implement-ui`, which runs `ui-visual-review` before code-review. If one is implemented another way, run `ui-visual-review` before code-review anyway.
