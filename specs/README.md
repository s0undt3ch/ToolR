# Specs

Design records for toolr — both live work and historical post-mortems.

## Where work lives

- **Top level (`specs/<date>-<topic>-design.md`)** — active design work and proposed-but-not-shipped
  features. Each design pairs with a `<date>-<topic>-plan.md` implementation plan once it leaves
  brainstorming.
- **`specs/archive/<year>/`** — shipped or abandoned designs. Archived files are immutable
  post-mortem records; do not edit them in place. If a shipped design needs revising, write a new
  design that supersedes it.

## How to start a new design

Open a brainstorming session in Claude Code:

```text
/superpowers:brainstorming
```

The session writes the design here once you approve it. The implementation plan follows from `/superpowers:writing-plans`.

## How to archive

When the PR implementing a design merges to `main`:

```sh
git mv specs/<date>-<topic>-design.md specs/archive/<year>/
git mv specs/<date>-<topic>-plan.md specs/archive/<year>/
```

Land the move in the same PR (or as an immediate follow-up).
