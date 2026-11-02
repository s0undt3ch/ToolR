# Installer Attestation Signer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `install.sh`, `install.ps1` and the documented manual command reject toolr archives whose attestation
wasn't produced by `release.yml` on `main` for that archive (F-44). The mise docs recommend pinning the packslip
signer (FU-11).

**Architecture:** Port the action's `verify-attestation` step into each installer's verify function. Test
`install.sh` by extracting its verify function and running it under `sh` with a stubbed `gh`. Keep `install.ps1`,
the docs and `action.yml` in step with a cross-file consistency test.

**Tech Stack:** POSIX sh, PowerShell 5.1/7, `gh attestation verify`, pytest, `jq` (tests only).

**Spec:** [`2026-11-02-installer-attestation-signer-design.md`](./2026-11-02-installer-attestation-signer-design.md)

## Global Constraints

- Branch `fix/installer-attestation-signer` stacks on `fix/action-attestation-signer` (#646). Don't touch
  `action.yml`.
- Factory fixtures for `tmp_path` setup, imports top-of-file.
- Conventional Commits; stage by path, never `git add -A`; never stage `audit/`.

## Review Focus

1. **Same rules as the action.** With the default repository, identities and the `--jq` filter equal `action.yml`'s
   (`tests/installation/test_attestation_pins.py`).
2. **POSIX.** shellcheck in `sh` mode, and the tests run the function with `sh`.
3. **Mode table.** `auto` still skips only when `gh` is absent; an old `gh` fails in `auto` and `require`.
4. **Real releases pass, real CI artifacts and renamed archives fail** (Task 2, Step 3).

---

### Task 1: install.sh

**Files:** modify `installation/install.sh`; create `tests/installation/__init__.py`,
`tests/installation/test_install_sh_attestation.py`.

- [ ] **Step 1: Write the tests.** Fixture `verify_runner(tmp_path)` extracts `err`/`info`/`warn` and
  `verify_attestation()` from `install.sh` by text, prepends `REPO`/`VERIFY_ATTESTATION`, and runs them under `sh`
  with a stub `gh` executable on `PATH` (prints `gh version …`, logs its arguments, enforces `--cert-identity`,
  pipes a fake verification result through the real `jq` with the given `--jq`). Cases: CI build rejected; release
  build accepted; any-of accepted; other workflow or ref rejected; no attestations rejected; gh failure keeps the
  installer message; flags passed; signer off main rejected; gh below 2.56 rejected in `require` and `auto`;
  2.56+ accepted; other archive subject rejected; non-numeric count fails closed; `skip` never calls `gh`; `gh`
  absent fails in `require` and warns in `auto`; `TOOLR_REPO` override drives the identities.
- [ ] **Step 2: Run them against the old script.** Expected: the rejection and flag tests fail.
- [ ] **Step 3: Port the step.** Version floor, identities from `$REPO`, exported `ARCHIVE_NAME` and
  `RELEASE_WORKFLOW`, `if ! count=$(gh …)`, fail-closed count, message naming `--verify-attestation=skip`.
- [ ] **Step 4: Run the tests and shellcheck.** All pass.

### Task 2: install.ps1 and real-gh checks

**Files:** modify `installation/install.ps1`; create `tests/installation/test_attestation_pins.py`.

- [ ] **Step 1: Port the step** per the spec (one-line filter, `$env:` save and restore, no stderr redirection,
  `[int]::TryParse`).
- [ ] **Step 2: Consistency test.** Parse `SIGNER_WORKFLOW`/`RELEASE_WORKFLOW` and the `--jq` filter from
  `action.yml`; extract the same from both installers with the default repository substituted; assert equal
  (whitespace-collapsed). If `pwsh` is available, also run `Test-AttestationVerified` against a stub `gh`.
- [ ] **Step 3: Real `gh`, read-only.** In the scratchpad:
    - `sh installation/install.sh --version 0.35.1 --prefix <scratch>` installs.
    - v0.34.0's x86_64 musl archive renamed to the 0.35.1 name is rejected by the installer's verify function.
    - A `toolr-archive-*` artifact from the latest `ci.yml` run on `main` is rejected.

### Task 3: Docs and the mise pin

**Files:** modify `docs/installation/index.md`, `docs/installation/mise.md`; extend
`tests/installation/test_attestation_pins.py`.

- [ ] **Step 1:** Replace the manual command, add the expected output and the gh 2.56 floor. Update the mise blurb.
- [ ] **Step 2:** In `mise.md`, add the `workflow = "release.yml@refs/heads/main"` recommendation, what it covers and
  doesn't, the mise 2026.10.5 floor, and why a bare `release.yml` fails.
- [ ] **Step 3:** Tests: the docs command carries both identities, `--deny-self-hosted-runners` and the filter; the
  mise pin expands to `publish-packslip`'s `--identity` and to `action.yml`'s `RELEASE_WORKFLOW`.
- [ ] **Step 4:** `uv run --group docs mkdocs build --strict`.

### Task 4: Release note, verification, archive

- [ ] `toolr release-notes add --slug installer-attestation-signer --text "…"`, committed as
  `docs(release-notes): …`; `toolr release-notes check --base fix/action-attestation-signer`.
- [ ] `uv run pytest tests/installation tests/action_yml tests/distribution/test_install_sh.py`,
  `prek run --all-files`, `cargo xtask build-skill-refs --check`.
- [ ] Last commit: `git mv` both spec files to `specs/archive/2026/`, status implemented.
