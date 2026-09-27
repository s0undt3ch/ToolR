# Packslip release manifest: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** From the next release on, every toolr release carries a signed
`packslip.sigstore.json`. It declares the five binary archives, the `toolr`
executable, bash, zsh and fish completions, and the three self-contained
agent skills. A PR-time gate catches archive-layout and manifest regressions
before release day.

**Architecture:** Both the release job and the PR gate read one shared
manifest, `.github/packslip.toml`. The PR gate is a checked-in script,
`.github/scripts/packslip-check.sh`, that CI runs against the CI-built
archives, and that runs locally against any release's archives. It signs with
an ephemeral key and `--no-log`. The release job is inline in `release.yml`,
because the Sigstore identity must stay `release.yml`. It runs `jdx/packslip`
after `publish-release`, then checks the published bundle.

**Tech Stack:** GitHub Actions, packslip 1.3.0 (action and CLI), bash, jq, mise.

**Spec:** `specs/2026-09-27-packslip-design.md`

**Branch:** `packslip` (git-spice), stacked on `skills-self-contained`. That
branch holds the self-contained skills work this PR depends on. Its spec is
now at `specs/archive/2026/2026-09-27-skills-self-contained-design.md`.

## Model assignment

| Task | Implementer | Adversarial reviewer |
| --- | --- | --- |
| 1. Shared manifest and `packslip-check.sh`, verified locally | `sonnet` | `opus` |
| 2. `packslip-check` PR job in `ci.yml` | `sonnet` | `opus` |
| 3. `publish-packslip` release job in `release.yml` | `opus` | `llmtrim-codex` (cross-family), falling back to `opus` if its proxy is down |
| 4. Docs, release note and spec cross-reference | `sonnet` | `opus` |
| 5. Final verification and spec archive | `sonnet` | whole-branch review: `opus` |

Task 3 touches a job that has `contents: write` and `id-token: write`, and
it runs once per release with no dry run. It gets the strongest implementer
and the most independent reviewer.

## Global Constraints

- packslip action pin:
  `jdx/packslip@4920350c39c234c7a969f9775040bd02829f1923 # v1.3.0`.
  The CLI in the PR gate is `github:jdx/packslip@1.3.0`. The two pins sit
  side by side conceptually, and each carries a comment that names the
  other, so they get bumped together.
- Every other `uses:` stays pinned by full SHA with a `# vX.Y.Z` comment.
  The `pin-github-actions` prek hook enforces this. Reuse the repo's existing
  pins, for example:
    - `step-security/harden-runner@e14015d583714f6e62063499dc959a02595150a1 # v2.21.1`
    - `actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1`
    - `actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c # v8.0.1`
    - `jdx/mise-action@c2a87611a18de5b3828c5652fe268e992400cb5c # v4.3.0`
- The project is `github.com/s0undt3ch/ToolR`. The source repo URL is
  `https://github.com/s0undt3ch/ToolR`.
- Release archive name patterns: `toolr-*-*-apple-darwin.tar.gz`,
  `toolr-*-*-linux-musl.tar.gz` and `toolr-*-*-windows-msvc.zip`. Never use
  `toolr-*.tar.gz`, which also matches the sdist.
- The release source commit is `git rev-parse "v${VERSION}^{commit}"`,
  because the tag is annotated. Never use `github.sha`.
- The signing identity is `https://github.com/${{ github.workflow_ref }}`,
  which is `…/.github/workflows/release.yml@<ref>`, with issuer
  `https://token.actions.githubusercontent.com`.
- New jobs join their workflow's `set-pipeline-exit-status.needs`. The
  `check-pipeline-gate-needs` prek hook fixes this automatically; stage what
  it rewrites.
- Shell steps: `set -euo pipefail`. Pass untrusted `${{ }}` values through
  `env:`, never interpolate them straight into `run:`. The `zizmor` and
  `actionlint` hooks check this.
- Never write the employer's name or absolute `/Users/...` paths. Stage by
  explicit path, never `git add -A`. Use Conventional Commits, with no
  `Co-Authored-By` and no `--no-verify`.
- Comments explain *why*, not *what*. Default to none.

## Review Focus

1. **A re-run of `publish-packslip` after it already uploaded a bundle.**
   Expected: it re-signs and overwrites cleanly, or fails loudly, and never
   leaves two bundles or a half-written asset. Task 3 decides how, and writes
   it down.
2. **A skill added under `skills/` without a manifest entry.** Expected: the
   PR gate fails and names the missing skill. This is tested in Task 1.
3. **A Linux archive that is no longer musl, or an archive that no longer
   contains `toolr` or `toolr.exe`.** Expected: the PR gate fails. This is
   tested in Task 1.
4. **A release dispatched from a branch other than `main`.** Expected: the
   post-publish verify uses the run's actual `workflow_ref` and still passes.
   This is covered by Task 3's identity expression.
5. **A release version with a prerelease suffix (`0.40.0-rc.1`).**
   Expected: the version passes to packslip unchanged and the verify step's
   version check still matches. This is covered by Task 3.

---

### Task 1: Shared manifest and `packslip-check.sh`

**Files:**

- Create: `.github/packslip.toml`
- Create: `.github/scripts/packslip-check.sh` (executable)

**Interfaces:**

- Produces:
    - `.github/scripts/packslip-check.sh <archive-dir>`. It exits 0 when the
  signed statement is valid. Otherwise it exits non-zero with a message
  that names the failing check.
    - It uses `packslip` from `$PACKSLIP`, or `packslip` on `PATH`, and it
  needs `jq`.
    - Tasks 2 and 3 consume `.github/packslip.toml`.

- [ ] **Step 1: Write `.github/packslip.toml`** exactly as spec §1 shows:
  `bin = ["toolr"]`, one completion resource, and three skill resources. Add
  a one-line comment at the top saying a new skill directory needs a
  `[[resource]]` entry here, and that the PR gate enforces it.

- [ ] **Step 2: Fetch real archives for local testing.** They go to the
  session scratchpad, never into the repo.

```bash
D=$(mktemp -d)
gh release download v0.33.0 -R s0undt3ch/ToolR -D "$D" \
  -p 'toolr-*-*-apple-darwin.tar.gz' -p 'toolr-*-*-linux-musl.tar.gz' -p 'toolr-*-*-windows-msvc.zip'
```

- [ ] **Step 3: Write the script.** It runs these steps in order, and each
  failure prints `packslip-check: <what> failed: <detail>` and exits 1:
  1. `cd` to the repo root (`git rev-parse --show-toplevel`).
  2. Make a temp dir, with a `trap` to clean it up. Run
     `packslip keygen -o "$tmp/k"`, which also writes `$tmp/k.pub`.
  3. Run `packslip create`:

     ```text
     packslip create --manifest .github/packslip.toml --key "$tmp/k" --no-log \
       --out "$tmp/out" --project github.com/s0undt3ch/ToolR \
       --version 0.0.0-ci --commit "$(git rev-parse HEAD)" \
       --source-repo https://github.com/s0undt3ch/ToolR <archives>
     ```

     `<archives>` is every `*.tar.gz` and `*.zip` in `<archive-dir>`. Fail if
     there are none.
  4. Run `packslip verify "$tmp/out/packslip.sigstore.json"` with
     `--pubkey "$tmp/k.pub" --allow-unlogged` and one `--artifact` for each
     archive.
  5. Run `packslip show` and use `jq` on the output to assert:
     - Every artifact has exactly one `bin` entry, and it ends in `/toolr` or
       `/toolr.exe`. Entries may be strings or `{path,name}` objects; handle
       both.
     - Every artifact with `os == "linux"` has `libc == "musl"`.
     - The sorted skill resource names equal the sorted directory names of
       `skills/*/SKILL.md`.
     - For each named skill, the `name:` in its `SKILL.md` frontmatter
       equals the directory name.
     - A completion resource exists with `shells == ["bash","zsh","fish"]`.

- [ ] **Step 4: RED. Prove each check can fail.** Run the script against
  broken inputs, one at a time, and record each failure message in the
  report. Restore after each.
    - A copy of the manifest with one skill entry deleted. Point the script
  at it through a `PACKSLIP_MANIFEST` environment override: add the
  override, defaulting to `.github/packslip.toml`.
    - An archive dir that contains only the sdist-shaped
  `toolr-0.33.0.tar.gz`. Build it from any tarball without a `toolr`
  binary.
    - An empty archive dir.

- [ ] **Step 5: GREEN.** Run

  ```bash
  PACKSLIP=$(mise which -t github:jdx/packslip@1.3.0 packslip 2>/dev/null || echo packslip) \
    .github/scripts/packslip-check.sh "$D"
  ```

  against the v0.33.0 archives. Install the CLI first with
  `mise x github:jdx/packslip@1.3.0 -- packslip --version` if needed.
  Expected: exit 0.

- [ ] **Step 6: Lint and commit.** Run `prek run --files .github/packslip.toml .github/scripts/packslip-check.sh`,
  which runs shellcheck.

```bash
git add .github/packslip.toml .github/scripts/packslip-check.sh
git commit -m "ci(packslip): add shared release manifest and offline check script"
```

---

### Task 2: `packslip-check` job in `ci.yml`

**Files:**

- Modify: `.github/workflows/ci.yml`

**Interfaces:**

- Consumes: `.github/scripts/packslip-check.sh <archive-dir>`, which
  reads `$PACKSLIP`, from Task 1.
- Consumes: the `toolr-archive-<triple>` artifacts that
  `build-binary-archive` uploads.
- [ ] **Step 1: Add the job.**
    - Place it near `build-binary-archive`: `needs: [build-binary-archive]`,
  `runs-on: ubuntu-latest`, `permissions: { contents: read }`.
    - Its steps:
    1. harden-runner with egress audit.
    2. `actions/checkout`.
    3. `actions/download-artifact` with `pattern: toolr-archive-*`,
       `path: ${{ runner.temp }}/archives` and `merge-multiple: true`.
    4. `jdx/mise-action`, with the repo's usual `version` and cache inputs
       but `install: false`, so it doesn't install the whole toolchain.
    5. A step that runs
       `PACKSLIP="$(mise which -t github:jdx/packslip@1.3.0 packslip)"`
       after `mise install github:jdx/packslip@1.3.0`, then
       `.github/scripts/packslip-check.sh "$RUNNER_TEMP/archives"`.
       Add a comment: the version pin must match the `jdx/packslip`
       action pin in `release.yml`.
    - Check how other jobs in `ci.yml` load `MISE_VERSION` and `CACHE_SEED`,
  for example the "Load shared env vars" step, and do the same.

- [ ] **Step 2: Wire the gate.** Commit once so the
  `check-pipeline-gate-needs` hook adds `packslip-check` to
  `set-pipeline-exit-status.needs`. Stage its rewrite and commit again.
  Check that the job appears in `needs`.

- [ ] **Step 3: Lint.** Run `prek run --files .github/workflows/ci.yml`,
  which covers actionlint, zizmor and pin-github-actions. Expected: pass.

- [ ] **Step 4: Commit.**

```bash
git add .github/workflows/ci.yml
git commit -m "ci(packslip): check the release manifest against CI-built archives on every PR"
```

---

### Task 3: `publish-packslip` job in `release.yml`

**Files:**

- Modify: `.github/workflows/release.yml`

**Interfaces:**

- Consumes: `.github/packslip.toml` from Task 1, and
  `needs.prepare-release.outputs.release-version`.

- [ ] **Step 1: Add the job inline, after `publish-release`.**
    - Settings:
        - `name: Publish packslip`
        - `needs: [prepare-release, publish-release]`
        - `runs-on: ubuntu-latest`
        - `permissions: { contents: write, id-token: write }`

  Don't add `attestations: write`: `attest: link` doesn't need it.
  Don't add `environment:` either: the signer is the workflow path, and
  this keeps the job free of environment approval gates.
    - Add a job-level comment: the job must stay inline in `release.yml`,
  because Sigstore records the calling workflow file as the signing
  identity and mise pins it.
    - Steps:
    1. harden-runner with egress audit.
    2. `actions/checkout` with
       `ref: v${{ needs.prepare-release.outputs.release-version }}`. Pass
       the version through `env` wherever it's used in `run:`.
    3. `id: source`. Run
       `echo "commit=$(git rev-parse "v${RELEASE_VERSION}^{commit}")" >> "$GITHUB_OUTPUT"`.
    4. `id: packslip`, using
       `jdx/packslip@4920350c39c234c7a969f9775040bd02829f1923 # v1.3.0`
       with these inputs:
       - `tag: v${{ … }}`
       - `version: ${{ … }}`
       - `commit: ${{ steps.source.outputs.commit }}`
       - `manifest: .github/packslip.toml`
       - `download:` set to the three archive patterns from Global
         Constraints, separated by spaces
       - `attest: link`

       Add a comment that its version must match the
       `github:jdx/packslip@1.3.0` pin in `ci.yml`.
    5. `name: Verify the published packslip`, with
       `GH_TOKEN: ${{ github.token }}` and the version and commit in `env`.
       It runs in a temp dir:
       - `gh release download "v$VERSION" -p packslip.sigstore.json`
         followed by the three archive patterns.
       - `packslip verify packslip.sigstore.json --identity "https://github.com/${WORKFLOW_REF}" --issuer https://token.actions.githubusercontent.com`,
         with one `--artifact` for each archive. `WORKFLOW_REF` comes from
         `${{ github.workflow_ref }}` through `env`.
       - Check version and commit with `packslip show` and `jq`:

         ```bash
         packslip show packslip.sigstore.json \
           | jq -e --arg v "$VERSION" --arg c "$COMMIT" \
               '.predicate.version == $v and .predicate.source.commit == $c'
         ```

- [ ] **Step 2: Decide and document re-run behaviour** (Review Focus 1).
  Read the action's `action.yml` at the pinned SHA
  (`gh api repos/jdx/packslip/contents/action.yml?ref=4920350c39c234c7a969f9775040bd02829f1923`)
  to see how it uploads: `gh release upload --clobber`, or something else.
    - If a re-run overwrites the bundle cleanly, add a one-line comment
  saying so.
    - If it would fail on an existing asset, add a step before the action
  that deletes an existing `packslip.sigstore.json` asset
  (`gh release delete-asset … --yes || true`), with a comment explaining
  why.
    - Record the finding in the report either way.

- [ ] **Step 3: Wire the gate.** The `check-pipeline-gate-needs` hook
  adds `publish-packslip` to `set-pipeline-exit-status.needs`. Stage its
  rewrite.

- [ ] **Step 4: Lint.** Run `prek run --files .github/workflows/release.yml`.
  Expected: actionlint, zizmor and pin-github-actions all pass. Fix any
  zizmor finding at its root; don't suppress it without a written reason.

- [ ] **Step 5: Dry-run the verify logic locally.** Take the
  `create`/`verify`/`show` sequence from the verify step and run it
  against the offline bundle from Task 1's script, using `--pubkey` in
  place of `--identity`. This proves the `jq` expression and the flags
  parse. Record it in the report.

- [ ] **Step 6: Commit.**

```bash
git add .github/workflows/release.yml
git commit -m "ci(release): sign and publish a packslip for each release"
```

---

### Task 4: Docs, release note and spec cross-reference

**Files:**

- Modify: `README.md` (the mise section)
- Modify: `docs/installation/mise.md`
- Modify: `docs/skills.md`
- Modify: `UNRELEASED.md`
- Modify: `specs/2026-09-27-packslip-design.md` (§4 path)
- [ ] **Step 1: `README.md` and `docs/installation/mise.md`.**
    - Lead with `mise use packslip:github.com/s0undt3ch/ToolR`, plus the
  pinned `@<version>` form and the global form.
    - Say that mise checks the release's signature against toolr's release
  workflow identity and verifies each download.
    - Keep aqua as the fallback. Label it as needed for toolr versions
  released before packslip support, and don't guess the version number:
  say "releases before packslip support".
    - Update the `.mise.toml` / `[tools]` examples and the "Why" bullets to
  match. For example, "Supply-chain verified" now means the signed
  packslip.
    - Keep the existing section structure.
- [ ] **Step 2: `docs/skills.md`.** Add a "Via mise" install section next
  to skillshare. It covers:
    - `mise skills ls` and `mise skills sync --dir .agents/skills`;
    - the opt-in `[settings.skills]` block
  (`dir = ".agents/skills"`, `auto_sync = true`, `prune = true`);
    - packslip's warning, restated: auto-sync means a tool upgrade can
  change what your agent reads, so review before you enable it;
    - that the skills mise installs match the toolr version active in the
  project;
    - version-matched completions through
  `mise completion <shell> --tool toolr --install`.

  Cite the packslip announcement with a meaningful link text:
  `https://jdx.dev/posts/2026-09-05-introducing-packslip/`.
- [ ] **Step 3: `UNRELEASED.md`.** Append:

```markdown
### Install toolr with mise's packslip backend

Releases now ship a signed [packslip](https://packslip.dev/) manifest, so
`mise use packslip:github.com/s0undt3ch/ToolR` installs toolr with signature
and checksum verification, version-matched shell completions, and the three
toolr agent skills (`mise skills sync`). The aqua backend remains available
for releases published before packslip support.
```

- [ ] **Step 4: Fix the spec cross-reference.** In
  `specs/2026-09-27-packslip-design.md` §4, change
  `2026-09-27-skills-self-contained-design.md` to
  `archive/2026/2026-09-27-skills-self-contained-design.md`.

- [ ] **Step 5: Verify.** Run `uv run --group docs mkdocs build --strict`
  and `prek run --files <touched files>`. Regenerate doc snippets only if
  the snippets hook asks: `toolr pre-commit regen-doc-snippets`.

- [ ] **Step 6: Commit.**

```bash
git add README.md docs/installation/mise.md docs/skills.md UNRELEASED.md specs/2026-09-27-packslip-design.md
git commit -m "docs: document installing toolr and its skills through mise's packslip backend"
```

---

### Task 5: Final verification and spec archive

- [ ] **Step 1: Full verification.** Poll `mise run test` every 30–60s.

```bash
mise run test
uv run --group docs mkdocs build --strict
prek run --all-files
git grep -i "$(printf 'p%sddle' a)" -- . ':!audit' || true
```

  Expected: everything green, and the grep prints nothing.

- [ ] **Step 2: Archive the spec. This is the last commit, made after the
  final whole-branch review.**

```bash
git mv specs/2026-09-27-packslip-design.md specs/archive/2026/
git mv specs/2026-09-27-packslip-plan.md specs/archive/2026/
git commit -m "chore(specs): archive packslip design and plan"
```
