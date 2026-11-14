# Docs toolchain cap and small release hygiene fixes

## Problem

- Material for MkDocs reaches end of life on 2027-05-05. MkDocs 2 and Material 10 are not drop-in
  replacements, so an unbounded `mkdocs` / `mkdocs-material` specifier lets a lock refresh pull in a
  broken docs build. The move to Zensical is tracked separately.
- C-02: `release.yml` does not check that CI on `main` is green for the commit being released.
- C-05: the `renovate.json5` comment says OSV PRs are opened for direct deps only. PR #746 was an OSV PR
  for `soupsieve`, a transitive dependency in `uv.lock`, so the comment is wrong.

## Design

- Cap `mkdocs>=1.6.1,<2` (every group that lists it) and `mkdocs-material>=9.7.7,<10` in `pyproject.toml`,
  with a one-line why-comment. Run plain `uv lock`; the lock diff is the specifier change only.
- C-02: one line in the `CONTRIBUTING.md` release section telling the releaser to wait for green CI on
  `main` before dispatching the workflow.
- C-05: reword the comment: transitive advisories can get a PR too; `lockFileMaintenance`, manual
  `uv lock --upgrade-package` and `cargo update -p` remain the fallback.

No user-visible change; all commits carry `Release-Notes: skip`.
