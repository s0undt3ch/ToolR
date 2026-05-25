# Repo Presentation Pass — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the gap between toolr's actual shipped state (Rust front-end, static-manifest plugins, pure-Rust authoring path) and its outward surface (README, CONTRIBUTING, docs/internals, specs/), without changing any runtime behaviour.

**Architecture:** Three stacked PRs via git-spice. PR-1 fixes six doc/code-comment contradictions with file:line precision. PR-2 rewrites README and CONTRIBUTING end-to-end around the actual value-prop. PR-3 archives shipped specs, deletes dead files, and refreshes auto-memory. Each PR is independently shippable.

**Tech Stack:** Markdown + prose. Rust source comments. `git-spice` (`gs`) for stacked PRs. `mkdocs build --strict` and `cargo test --workspace` as the regression nets. `hyperfine` for one-time README benchmark measurement (PR-2 only). `prek` runs the full pre-commit gate on every commit.

**Conventions used in this plan:**

- Conventional Commits with `(scope)` matching the touched area (`docs(internals)`, `docs(writing-commands)`, `chore(specs)`, etc.).
- No `Co-Authored-By` footer (per repo policy).
- Every commit ends a logical step so `prek` runs the gate before the next step starts.
- Blocks that *show* the literal markdown content of a file being written use **tilde fences** (`~~~~markdown`) so the inner triple-backticks render as literal text. Treat the content between the tilde fences as the file body, not as a nested code example.
- The plan was written against `main` at `df8b87d` (HEAD after spec commits). If `main` has moved by execution time, rebase first and re-verify line numbers in PR-1.

---

## PR-1: Doc correctness fixes

**Branch:** `presentation-pass-1-doc-fixes`
**Base:** `main`
**Surface:** ~30 lines changed, ~10 lines deleted across 4 files.
**Risk:** Near zero; mechanical edits guarded by `mkdocs --strict` and `cargo test`.

### Task 1.0: Branch setup

**Files:** None (git only).

- [ ] **Step 1: Confirm clean working tree on `main`**

```bash
git switch main
git pull --ff-only
git status --short
```

Expected: empty output (or only untracked files outside the repo's tracked tree).

- [ ] **Step 2: Create the stacked branch**

```bash
gs branch create presentation-pass-1-doc-fixes
```

Expected: `gs` creates the branch on top of `main` and switches to it.

- [ ] **Step 3: Verify baseline tests pass before any edits**

```bash
cargo test --workspace
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-baseline
```

Expected: both succeed. (If `mkdocs` isn't on PATH, use `uv run mkdocs build --strict ...`.)

This is the "before" picture — anything that fails *now* is not a regression caused by this plan.

### Task 1.1: Rewrite the writing-commands deprecation admonition

**Files:**

- Modify: `docs/writing-commands/index.md:25-30`

Per the 2026-05-18 keep-bound-command-decorator design, `@group.command` is no longer deprecated. The admonition at the bottom of the writing-commands index page still tells readers it is. Rewrite it to name only `parent.command_group("child", ...)` as deprecated.

- [ ] **Step 1: Apply the edit**

Replace lines 25–30 of `docs/writing-commands/index.md` with the following block (between the tilde markers — content only):

````markdown
!!! warning "Still using `parent.command_group("child", ...)`?"
    The bound *subgroup-method* form is deprecated and will be
    removed in toolr 1.0. Every call emits a runtime warning at
    import time pointing at the offending line. See the
    [migration guide](../migration.md) for the dotted-string
    replacement (`command_group("parent.child", ...)`).

    The bound `@group.command` decorator on a captured
    `CommandGroup` is **not** deprecated — see
    [Groups & commands](groups.md) for canonical single-file
    usage.
````

- [ ] **Step 2: Verify mkdocs still builds**

```bash
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-after-1-1
```

Expected: success. The new internal link `../migration.md` resolves; the existing `groups.md` link still resolves.

- [ ] **Step 3: Commit**

```bash
git add docs/writing-commands/index.md
git commit -m "docs(writing-commands): scope the deprecation admonition to the subgroup method form

The keep-bound-command-decorator design (2026-05-18) un-deprecated
@group.command. The remaining deprecation is only the bound
subgroup-method form parent.command_group(\"child\", ...). Rewrite
the admonition to name only that form, and point readers at the
canonical single-file usage in groups.md."
```

### Task 1.2: Fix the known-bugs example

**Files:**

- Modify: `docs/writing-commands/known-bugs.md:24-25`

The closed-bugs list demonstrates "nested groups build a proper subcommand tree" using the still-deprecated method form (`docker.command_group("image")`). Replace it with the dotted-string form so the closed-bug demonstration is also the recommended idiom.

- [ ] **Step 1: Apply the edit**

In `docs/writing-commands/known-bugs.md`, replace lines 24–25 with:

````markdown
- Nested groups (`command_group("docker.image", ...)`) build a
  proper subcommand tree at the CLI surface.
````

- [ ] **Step 2: Verify mkdocs still builds**

```bash
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-after-1-2
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add docs/writing-commands/known-bugs.md
git commit -m "docs(writing-commands): use dotted-string form in the nested-groups bullet

The closed-bugs list demonstrated nested groups via the deprecated
parent.command_group(\"image\") method form. Swap to the canonical
dotted-string form command_group(\"docker.image\", ...) so the
closed-bug celebration is also the recommended idiom."
```

### Task 1.3: Fix the `third_party_hash` File-shape bullet

**Files:**

- Modify: `docs/internals/manifest.md:32-33`

Per `crates/toolr-core/src/freshness/compare.rs` + the dispatch-manifest-freshness design, `third_party_hash` hashes only the sorted set of `toolr-manifest.json` files under `<tools-venv>/lib/python*/site-packages/*/`. The current text claims it covers the venv's full installed package set, which is the pre-rewrite definition.

- [ ] **Step 1: Apply the edit**

Replace lines 32–33 of `docs/internals/manifest.md` with:

````markdown
- **`third_party_hash`** — blake3 over the sorted set of
  `toolr-manifest.json` files found under
  `<tools-venv>/lib/python*/site-packages/*/`. Each entry's path
  and content go into the hash. Drives third-party-fragment
  rebuilds: installing an unrelated package no longer invalidates
  the manifest, only changes to packages that ship a toolr
  fragment do.
````

- [ ] **Step 2: Verify mkdocs still builds**

```bash
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-after-1-3
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add docs/internals/manifest.md
git commit -m "docs(internals): correct the third_party_hash File-shape bullet

The bullet still described the pre-rewrite definition (blake3 over
the venv's installed package set). The shipped implementation in
crates/toolr-core/src/freshness/compare.rs hashes only
toolr-manifest.json files; this is what dispatch-manifest-freshness
shipped. Rewrite the bullet to match. The §Hashing details section
at the bottom of the same file is updated in a follow-up commit."
```

### Task 1.4: Remove entry-point discovery from §Dynamic layer

**Files:**

- Modify: `docs/internals/manifest.md:142-156`

Step 3 of the §Dynamic layer description claims the helper walks `importlib.metadata.entry_points(group="toolr.tools")`. Entry-point discovery was removed by dispatch-manifest-freshness. The dynamic layer's actual job is now only to import `tools.*` modules so dynamically-registered commands (runtime patterns the AST parser can't see) appear in the manifest.

- [ ] **Step 1: Apply the edit**

Replace the §Dynamic layer section (lines 142–156) of `docs/internals/manifest.md` with:

````markdown
## Dynamic layer

Built by spawning `python -m toolr._introspect --tools-root <tools>`
inside the resolved tools venv. The helper:

1. Inserts `<tools>/..` on `sys.path` so `import tools` works.
2. Imports every `tools.*` module — registering every
   `command_group` / `@command` call. Catches dynamically-registered
   commands the static AST parser can't see (e.g. registrations
   inside `for` loops or conditionals).
3. Dumps a JSON payload to stdout describing the merged registry.

Third-party packages are **not** discovered through this layer.
They contribute via static `toolr-manifest.json` fragments shipped
inside the wheel and merged at static-build time
(see [Third-party packages](../third-party.md)).

Toolr regenerates the dynamic layer when:

- A command is invoked and the binary detects `static_hash` drift
  on entry (the dynamic layer runs alongside the static rebuild).
- The user explicitly runs `toolr project manifest rebuild`.
````

- [ ] **Step 2: Verify mkdocs still builds**

```bash
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-after-1-4
```

Expected: success. The new link `../third-party.md` resolves.

- [ ] **Step 3: Commit**

```bash
git add docs/internals/manifest.md
git commit -m "docs(internals): drop entry-points discovery from the dynamic layer

Step 3 still listed importlib.metadata.entry_points(\"toolr.tools\")
as a discovery hop. That mechanism was removed in
dispatch-manifest-freshness — third-party packages contribute via
static toolr-manifest.json fragments now. Rewrite the section: the
dynamic layer's sole job is importing tools.* so runtime-only
dynamic patterns surface in the manifest. Add an explicit
cross-reference to the third-party docs."
```

### Task 1.5: Fix the §Hashing details third_party_hash entry

**Files:**

- Modify: `docs/internals/manifest.md` (line numbers as of `main`; will shift after Tasks 1.3 + 1.4 land — re-locate by content).

The hashing-details section at the bottom describes `third_party_hash` as a sorted listing of `site-packages/*` entries with mtime. Wrong-in-a-different-direction from the File-shape bullet (already fixed in Task 1.3). Align both descriptions.

- [ ] **Step 1: Locate the current hashing-details bullet**

After Tasks 1.3 + 1.4's commits, the line numbers will have shifted. Find the section by content:

```bash
rg -n 'third_party_hash. input' docs/internals/manifest.md
```

Expected: one match in the §Hashing details section.

- [ ] **Step 2: Apply the edit**

Replace the `third_party_hash` bullet in §Hashing details with:

````markdown
- `third_party_hash` input: every `toolr-manifest.json` under
  `<tools-venv>/lib/python*/site-packages/*/`, sorted by path,
  each entry hashed as
  `len(path_bytes) || path_bytes || len(contents) || contents`.
  Identical scheme to `static_hash` but over the third-party
  fragment file set instead of `tools/**/*.py`.
````

- [ ] **Step 3: Verify mkdocs still builds**

```bash
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-after-1-5
```

Expected: success.

- [ ] **Step 4: Commit**

```bash
git add docs/internals/manifest.md
git commit -m "docs(internals): align the hashing-details third_party_hash entry

The §Hashing details section described an mtime-based scheme over
site-packages/* dirs. The actual implementation (and the §File
shape bullet, fixed in the prior commit) is a content blake3 over
toolr-manifest.json files. Use the same scheme description as
static_hash since the two hashes share a structure."
```

### Task 1.6: Fix the `DynamicPayload.groups` doc comment

**Files:**

- Modify: `crates/toolr-core/src/dynamic/payload.rs:16`

The doc comment for `DynamicPayload.groups` ends with "and via entry points". Entry-point discovery is gone.

- [ ] **Step 1: Apply the edit**

Open `crates/toolr-core/src/dynamic/payload.rs` and change line 16 from:

```rust
    /// Groups discovered by importing `tools.*` and via entry points.
```

to:

```rust
    /// Groups discovered by importing `tools.*` modules. Third-party
    /// packages contribute via the static layer, not via this payload.
```

Line 18 (`/// Commands discovered the same way.`) stays as-is.

- [ ] **Step 2: Verify the crate still compiles and tests pass**

```bash
cargo test -p toolr-core
```

Expected: success.

- [ ] **Step 3: Commit**

```bash
git add crates/toolr-core/src/dynamic/payload.rs
git commit -m "docs(dynamic): remove stale entry-points reference in DynamicPayload

The doc comment on DynamicPayload.groups still claimed entry-point
discovery contributed to the payload. dispatch-manifest-freshness
removed entry-point discovery entirely; third-party packages now
flow through the static layer. Update the comment to match."
```

### Task 1.7: PR-1 final verification + open as draft

**Files:** None (CI + git only).

- [ ] **Step 1: Full workspace test**

```bash
cargo test --workspace
```

Expected: success — identical to the baseline run in Task 1.0 Step 3.

- [ ] **Step 2: Strict mkdocs build**

```bash
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-pr1-final
```

Expected: success.

- [ ] **Step 3: Run full pre-commit gate**

```bash
prek run --all-files
```

Expected: every hook passes. If `rumdl` or `codespell` flags something, fix it inline and amend the relevant prior commit — these are mechanical fixes, not new tasks.

- [ ] **Step 4: Submit as a draft stacked PR**

```bash
gs branch submit --draft --fill
```

The PR title and body are auto-derived from the commit messages by `--fill`. Expected: draft PR opened against `main`.

- [ ] **Step 5: Run the project's code-review skill on the PR**

After the PR is open, invoke the project review pipeline (`/compound-engineering:ce-code-review` or `/paddle-shared:open-pr`). Address findings inline on the same branch. PR-1 closes when the user marks it ready and the review skill returns no high-confidence findings.

---

## PR-2: README + CONTRIBUTING rewrite

**Branch:** `presentation-pass-2-readme-contributing`
**Base:** `presentation-pass-1-doc-fixes`
**Surface:** ~250 lines of prose churn across 2 files. No code/test changes.
**Risk:** Low; the diff cannot break tests. The risk is review-cycle length.

### Task 2.0: Branch setup + benchmark capture

**Files:**

- Read-only: existing `README.md`, `CONTRIBUTING.md`, `docs/installation/index.md`, `docs/quickstart.md` (for tone-matching).

- [ ] **Step 1: Create the stacked branch**

```bash
gs branch checkout presentation-pass-1-doc-fixes
gs branch create presentation-pass-2-readme-contributing
```

- [ ] **Step 2: Build the release binary for honest benchmark numbers**

Headline numbers in the rewritten README come from a release build, not a debug build.

```bash
cargo build -p toolr --release
TOOLR_BIN="$(pwd)/target/release/toolr"
"$TOOLR_BIN" --version
```

Expected: prints a `0.11.x` version.

- [ ] **Step 3: Measure `--help` latency with hyperfine**

If `hyperfine` isn't installed, `mise install hyperfine` or `brew install hyperfine`.

```bash
cd /tmp && mkdir -p toolr-bench && cd toolr-bench
"$TOOLR_BIN" project init --no-sync
hyperfine --warmup 5 --runs 20 "$TOOLR_BIN --help"
hyperfine --warmup 5 --runs 20 "$TOOLR_BIN example --help"
```

Record the **mean ± stddev** for both runs. Add them to the PR description and reference them in the README rewrite (Task 2.1). If either mean exceeds 50 ms, drop the "sub-50ms" framing per the spec's Risks-and-mitigations section.

- [ ] **Step 4: Return to the repo working tree**

```bash
cd -
```

No commit yet — the benchmark numbers go into the README rewrite commit.

### Task 2.1: Rewrite `README.md`

**Files:**

- Modify (full rewrite): `README.md`

The new README follows the outline in `specs/2026-05-22-repo-presentation-pass-design.md` §PR-2. Use the *measured* benchmark numbers from Task 2.0, not the speculative "sub-50ms" framing.

- [ ] **Step 1: Replace `README.md` end-to-end with the following content**

Everything between the tilde fences is the literal file body. Replace `<MEAN ms>` and `<hardware>` with the values measured in Task 2.0 Step 3. Replace the `[ASCII transcript ...]` placeholder with the transcript captured in Step 2 below.

````markdown
<h1 align="center">
  <img width="240px" src="https://raw.githubusercontent.com/s0undt3ch/Toolr/main/docs/imgs/toolr.png" alt="ToolR">
</h1>

<h2 align="center">
  <em>In-project CLI tooling, with a Rust front-end.</em>
</h2>

ToolR is a Python task runner that boots in milliseconds because the
front-end is a Rust binary. Python only runs when you actually invoke
a command, inside a per-repo `uv`-managed venv that materialises on
first use.

```text
$ toolr --help          # <MEAN ms>  (measured on <hardware>)
$ toolr ci --help       # <MEAN ms>
```

[ASCII transcript of `toolr project init` → first command. ~12 lines.]

## Why ToolR

- **Sub-millisecond discovery.** The CLI is a Rust binary. `--help`
  and Tab completion read a cached static manifest; Python never
  boots for non-execute paths.
- **No system-Python dependency.** Toolr resolves a per-repo Python
  venv via `uv` on first invocation. The host OS doesn't need
  Python at all to install toolr; it's a single static binary.
- **Write Python, not framework boilerplate.** Drop a `tools/*.py`
  file with a `command_group` and a `@command` decorator. Type
  hints become CLI arguments; Google-style docstrings become
  `--help` text.
- **First-class third-party command packages.** Plugins ship a
  static `toolr-manifest.json` inside the wheel. Discovery is a
  glob + JSON parse; no Python import to find them.
- **Signed releases.** Every release archive ships with a SLSA
  build-provenance attestation. The install scripts verify it
  automatically when `gh` is on PATH.

## Two wheels, two roles

| Package    | What it is                                      | Where it lives                    |
|------------|-------------------------------------------------|-----------------------------------|
| `toolr`    | The Rust CLI binary you run from the shell.    | On `$PATH`, installed once.       |
| `toolr-py` | The Python runtime your `tools/*.py` import.   | In your `tools/pyproject.toml`.   |

Most projects want both: the CLI installed globally, `toolr-py`
declared in the per-repo `tools/pyproject.toml` so `from toolr
import Context, command_group` works when your commands run.

## Install

Five first-class install paths.

### mise

```sh
mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise
mise use --global toolr@latest
```

For projects that already pin tool versions via `.mise.toml`, this
is the most-natural fit — toolr's version becomes part of your
project's reproducible tool set. See
[docs/installation/mise/](https://s0undt3ch.github.io/ToolR/installation/mise/).

### pip

```sh
pip install toolr      # Rust CLI binary
pip install toolr-py   # Python runtime your tools/*.py import
```

The `toolr` wheel ships only the binary; the `toolr-py` wheel ships
the `import toolr` surface (`Context`, `command_group`, the
`_rust_utils` extension module). Most projects want both, in
different venvs — see "Two wheels, two roles" above.

### curl | sh (Linux + macOS)

```sh
curl -fsSL https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.sh | sh
```

Verifies the SLSA attestation when `gh` is on PATH. Pin a version
with `sh -s -- --version X.Y.Z`. Custom prefix:
`sh -s -- --prefix /opt/toolr/bin`.

### PowerShell (Windows)

```powershell
irm https://raw.githubusercontent.com/s0undt3ch/ToolR/main/installation/install.ps1 | iex
```

### GitHub release archives

Download `toolr-<version>-<target-triple>.tar.gz` (or `.zip` for
Windows) from <https://github.com/s0undt3ch/ToolR/releases>, verify
the `.sha256` sibling and the SLSA attestation, drop the binary on
`$PATH`. Useful in locked-down environments that audit binaries
before allowing them on a machine.

### Scaffold your repo

After the binary is on `$PATH`:

```sh
toolr project init                  # writes tools/{pyproject.toml,.gitignore,example.py}
toolr example hello                 # run the generated example
toolr self completion install bash  # or zsh / fish
```

The full install matrix (per-OS notes, attestation flags, prefix
overrides) lives in [docs/installation/](https://s0undt3ch.github.io/ToolR/installation/).

## What you write

```python
# tools/example.py
from toolr import Context, command_group

example = command_group("example", title="Example", description="Sample commands.")

@example.command
def hello(ctx: Context, name: str = "world") -> None:
    """Say hello to <name>.

    Args:
        name: who to greet.
    """
    ctx.print(f"Hello, {name}!")
```

```sh
$ toolr example hello --name Pedro
Hello, Pedro!
```

`toolr project init` writes a richer four-command starter than this
two-liner — open it and edit, or delete it and start from scratch.

## Where to go next

- [Quickstart](https://s0undt3ch.github.io/ToolR/quickstart/)
- [Writing commands](https://s0undt3ch.github.io/ToolR/writing-commands/)
- [Third-party packages](https://s0undt3ch.github.io/ToolR/third-party/)
- [Internals (manifest, freshness, cache)](https://s0undt3ch.github.io/ToolR/internals/)
- [CLI reference](https://s0undt3ch.github.io/ToolR/cli/)

## Project status

ToolR is pre-1.0. The on-disk manifest is versioned
(`schema_version` in `tools/.toolr-manifest.json`); the binary
refuses to load a higher version than it understands. The public
Python surface is `toolr.__all__`; anything not listed there is
implementation detail. Backwards-incompatible changes will be
explicit in the changelog (generated by `git-cliff` on release).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[Apache-2.0](LICENSE).
````

- [ ] **Step 2: Capture the ASCII transcript referenced above**

Generate it once and paste the result into the README. From a fresh temp directory:

```bash
cd "$(mktemp -d)"
{ echo "\$ toolr project init"
  "$TOOLR_BIN" project init 2>&1
  echo
  echo "\$ toolr example hello"
  "$TOOLR_BIN" example hello 2>&1
} > /tmp/toolr-readme-transcript.txt
cat /tmp/toolr-readme-transcript.txt
```

Trim noise (full file paths, timing chatter) so the transcript is ~12 lines. Paste into the README replacing the `[ASCII transcript ...]` placeholder.

- [ ] **Step 3: Verify mkdocs still builds**

```bash
cd -
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-after-2-1
```

Expected: success. (mkdocs doesn't render `README.md` itself; `docs/index.md` `--8<--` includes lines 5–15. Verify the included slice still renders sensibly.)

```bash
sed -n '5,15p' README.md
```

Expected: shows the `<em>In-project CLI tooling, with a Rust front-end.</em>` headline + the opening prose paragraph.

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs(readme): rewrite around the Rust front-end value-prop

The README still framed toolr as 'similar to invoke; next generation
of python-tools-scripts' — true in 2024, but it buried the actual
differentiator: a Rust binary front-end that boots in milliseconds.
Rewrite the README end-to-end around the shipped model:

- Lead with measured benchmark numbers from a release build.
- 'Why ToolR' replaces the abstract feature list with the concrete
  reasons (no Python boot, no system-Python dep, signed releases).
- 'Two wheels, two roles' pinned near the top so the toolr / toolr-py
  split is impossible to miss.
- Install section: mise, pip, curl|sh, PowerShell, GH-archive — all
  five first-class with their actual use cases.
- 'What you write' uses the canonical command_group + @group.command
  bound-decorator form (un-deprecated per keep-bound-command-decorator).
- Drop the Hypothesis-as-headline section; tests are tooling, not
  product.
- Drop the dead /usage/#advanced-topics link; point at the live IA."
```

### Task 2.2: Rewrite `CONTRIBUTING.md`

**Files:**

- Modify (full rewrite): `CONTRIBUTING.md`

The current CONTRIBUTING describes a Python-only project. Replace with content that reflects the three-crate workspace and the dogfooded toolr invocations.

- [ ] **Step 1: Replace `CONTRIBUTING.md` end-to-end with the following content**

Everything between the tilde fences is the literal file body.

````markdown
# Contributing to ToolR

Thanks for considering a contribution. ToolR is a small project with
a focused surface; bug reports, doc fixes, and well-scoped feature
PRs are all welcome.

## Repo layout

A Cargo workspace with three crates plus the Python source:

| Crate                  | What it is                                                                                                              |
|------------------------|-------------------------------------------------------------------------------------------------------------------------|
| `crates/toolr-core/`   | Pure-Rust library. Parser, manifest, freshness, argparse scanner, completion engine, cache. No pyo3.                    |
| `crates/toolr/`        | The binary. clap CLI, dispatch, subprocess control.                                                                     |
| `crates/toolr-py/`     | pyo3 dynlib + the Python source at `crates/toolr-py/python/toolr/`. Ships as the `toolr-py` wheel.                      |

The CI builds two PyPI wheels at the same workspace version:

- `toolr` — maturin `bindings = "bin"`. The Rust binary, no Python.
- `toolr-py` — maturin `bindings = "pyo3"`. The Python package plus
  the `_rust_utils` extension module.

A GitHub release archive of the standalone binary ships alongside.

## Dev setup

You need [mise](https://mise.jdx.dev/). Everything else (Rust,
Python, `uv`, `prek`) installs from the repo's `mise.toml`:

```sh
curl https://mise.run | sh        # if you don't have mise yet
mise install                      # pinned tool versions
uv sync --all-extras --dev        # Python deps
prek install --install-hooks      # pre-commit hooks
```

Run the dev binary against the dogfood `tools/` directory:

```sh
cargo run -p toolr -- --help
cargo run -p toolr -- self build-manifest toolr_example_plugin
```

For the release-shaped binary (used by benchmarks and the install
smoke tests):

```sh
cargo build -p toolr --release
./target/release/toolr --help
```

## Tests

| Suite                                  | Run with                          | Lives at                               |
|----------------------------------------|-----------------------------------|----------------------------------------|
| Rust unit tests                        | `cargo test -p toolr-core`        | `crates/toolr-core/src/**/*.rs`        |
| Rust integration tests                 | `cargo test -p toolr --test '*'`  | `crates/toolr/tests/*.rs` (assert_cmd) |
| Python unit tests                      | `uv run pytest`                   | `tests/**/*.py`                        |
| Distribution lock-tests (opt-in, slow) | `uv run pytest -m distribution`   | `tests/distribution/`                  |

The Rust integration tests spawn the built `toolr` binary via
`assert_cmd`. Don't shadow them with Python-level subprocess tests
unless the behaviour can't be exercised in Rust.

## RUNNER_SCHEMA_VERSION ↔ SCHEMA_VERSION lock-step

The Rust binary and the `toolr-py` Python runtime communicate over
a versioned JSON spec. Two constants must stay in lock-step:

- `RUNNER_SCHEMA_VERSION` in `crates/toolr-core/src/execute/spec.rs`
- `SCHEMA_VERSION` in `crates/toolr-py/python/toolr/_runner.py`

Both carry doc comments listing which changes require a bump and
which don't. Read those before changing either the Rust serde
structs or the Python `RunnerSpec` class. A CI gate fails the
build when the two values disagree.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/).
Examples:

- `feat(cli): add --quiet flag to project deps sync`
- `fix(parser): skip dot-prefixed dirs in list_python_files`
- `docs(internals): correct the third_party_hash File-shape bullet`

Repo policies:

- **No `Co-Authored-By:` footer** on commits.
- **No `--no-verify` / `--no-edit`** without a stated reason in the
  commit body. Pre-commit failures are signals, not obstacles.
- **Don't manually edit `CHANGELOG.md`** — `git-cliff` generates it
  on release from the conventional-commit history.

## Pre-commit hooks

`prek install --install-hooks` (above) wires the gate. Manually:

```sh
prek run --all-files
prek run rumdl --files docs/internals/manifest.md
```

Hooks include `ruff`, `mypy`, `clippy`, `cargo check`, `rumdl`,
`codespell`, `typos`, `actionlint`, `shellcheck`, plus the
project-local hooks (`pin-github-actions`, `regen-doc-snippets`).

## Filing bugs

Open a [GitHub issue](https://github.com/s0undt3ch/ToolR/issues/new) with:

- ToolR version (`toolr --version`)
- OS + shell
- Minimal `tools/*.py` (or repro repo URL) that triggers the bug
- Expected vs actual output

For suspected security issues, use [GitHub Security Advisories](https://github.com/s0undt3ch/ToolR/security/advisories/new) instead of a public issue.

## License

Apache-2.0. No sign-off required.
````

- [ ] **Step 2: Verify the symlink still points correctly**

`docs/contributing.md` is a symlink to `../CONTRIBUTING.md`. The mkdocs build will pick up the new content automatically.

```bash
ls -l docs/contributing.md
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-after-2-2
```

Expected: the symlink resolves; mkdocs builds.

- [ ] **Step 3: Commit**

```bash
git add CONTRIBUTING.md
git commit -m "docs(contributing): rewrite around the three-crate Cargo workspace

The previous CONTRIBUTING described a Python-only project with a
tests/cli/ + tests/parser/ layout that no longer exists. Rewrite
end-to-end:

- Repo layout table covers the toolr-core / toolr / toolr-py split
  and the two-wheels-plus-archive output.
- Dev setup leads with mise (the canonical install path for the
  project's own tooling) and the dogfood 'cargo run -p toolr' shape.
- Tests table covers all four suites (Rust unit, Rust integration
  via assert_cmd, Python unit, opt-in distribution).
- Keep the RUNNER_SCHEMA_VERSION ↔ SCHEMA_VERSION section as-is;
  it's the highest-leverage onboarding bit in the old file.
- Add explicit repo policies: no Co-Authored-By footer, no
  --no-verify without a stated reason, don't edit CHANGELOG by hand."
```

### Task 2.3: PR-2 final verification + open as draft

**Files:** None.

- [ ] **Step 1: Full workspace test**

```bash
cargo test --workspace
```

Expected: success (no code changes; nothing should have moved).

- [ ] **Step 2: Strict mkdocs build**

```bash
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-pr2-final
```

Expected: success.

- [ ] **Step 3: Run full pre-commit gate**

```bash
prek run --all-files
```

Expected: every hook passes.

- [ ] **Step 4: Verify the docs `--8<--` include still renders cleanly**

```bash
sed -n '5,15p' README.md
```

Expected: lines 5–15 (the slice `docs/index.md` pulls via `{!README.md!lines=5-15}`) are coherent prose on their own. If lines 5–15 happen to land on something fragmentary, reshape the README front-matter or update the `--8<--` directive in `docs/index.md` to a slice that works.

- [ ] **Step 5: Submit as a draft stacked PR**

```bash
gs branch submit --draft --fill
```

Expected: draft PR opened against `presentation-pass-1-doc-fixes`. After PR-1 merges to `main`, this PR will auto-rebase onto `main` per `gs`'s stack-tracking.

- [ ] **Step 6: Run review skills on the PR**

Same as PR-1 Task 1.7 Step 5. Address findings inline.

---

## PR-3: specs/ archival + cleanup

**Branch:** `presentation-pass-3-archive-cleanup`
**Base:** `presentation-pass-2-readme-contributing`
**Surface:** ~30 file moves, 2 file deletions, ~30 lines new (`specs/README.md`), 3 link rewrites, ~6 line deletions of stale source comments.
**Risk:** Low; the `cargo test --workspace` gate guards the `tools/__init__.py` deletion. The `git grep` pre-check guards the spec moves.

### Task 3.0: Branch setup + pre-move audit

**Files:** None (read-only audit).

- [ ] **Step 1: Create the stacked branch**

```bash
gs branch checkout presentation-pass-2-readme-contributing
gs branch create presentation-pass-3-archive-cleanup
```

- [ ] **Step 2: Verify no in-tree references into the to-be-moved spec paths**

```bash
git grep -nE 'specs/(rust-front-end|2026-05-(18|19|21|22))' -- ':!specs/'
```

Expected: no output. If anything matches, list the file:line and either (a) update the reference to the new `specs/archive/2026/` path in the same PR, or (b) stop and decide whether the spec really should be archived.

- [ ] **Step 3: Build the baseline test envelope**

```bash
cargo test --workspace
uv run pytest -q
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-pr3-baseline
```

Expected: all green. The deletion of `tools/__init__.py` will re-run pytest later; this baseline establishes the contract.

### Task 3.1: Create the archive structure and move `rust-front-end/`

**Files:**

- Create: `specs/archive/2026/` (directory)
- Move: `specs/rust-front-end/` → `specs/archive/2026/rust-front-end/`

The `git mv` preserves history for every file in the moved tree.

- [ ] **Step 1: Create the archive directory**

```bash
mkdir -p specs/archive/2026
```

- [ ] **Step 2: Move the rust-front-end tree**

```bash
git mv specs/rust-front-end specs/archive/2026/rust-front-end
```

Expected: git records 17 file renames (16 numbered files + `followups/2026-05-14-rich-argparse-dependency.md`).

- [ ] **Step 3: Sanity-check the move**

```bash
ls specs/archive/2026/rust-front-end/ | head
test -d specs/rust-front-end || echo "old path gone"
```

Expected: the archive directory contains the 16 numbered design files; the old path no longer exists.

- [ ] **Step 4: Commit the move alone (no other changes)**

```bash
git commit -m "chore(specs): archive shipped rust-front-end design tree

All 12 sub-plans of specs/rust-front-end/01-roadmap.md are merged.
Move the whole tree to specs/archive/2026/ so the top level of
specs/ reflects only live design work. Pure git mv — no content
changes; cross-references between archived files use relative
paths that survive the move unchanged. Upward links into ../../docs/
are repaired in a follow-up commit."
```

### Task 3.2: Fix the three broken upward links inside the archived rust-front-end tree

**Files:**

- Modify: `specs/archive/2026/rust-front-end/13-plan-11-docs-overhaul.md`

Per the design spec's "Upward-relative links" section, three markdown links in `13-plan-11-docs-overhaul.md` use `../../docs/...`. After the move, those resolve into `specs/archive/2026/docs/` — broken. Repair them to `../../../../docs/...` (four levels up).

- [ ] **Step 1: Locate the three links**

```bash
rg -n '\(\.\./\.\./docs/' specs/archive/2026/rust-front-end/13-plan-11-docs-overhaul.md
```

Expected: exactly three matches (lines 757, 1004, 1005 against `main` at `df8b87d`).

- [ ] **Step 2: Apply the edits**

For each of the three lines, replace `../../docs/` with `../../../../docs/`:

| Location | Before | After |
|----------|--------|-------|
| line 757  | `[Arguments](../../docs/writing-commands/arguments.md).` | `[Arguments](../../../../docs/writing-commands/arguments.md).` |
| line 1004 | `[Quickstart](../../docs/quickstart.md),`               | `[Quickstart](../../../../docs/quickstart.md),`               |
| line 1005 | `[Project configuration](../../docs/project-config.md).`| `[Project configuration](../../../../docs/project-config.md).`|

- [ ] **Step 3: Verify the links resolve from the new location**

```bash
cd specs/archive/2026/rust-front-end
test -f ../../../../docs/writing-commands/arguments.md && echo OK
test -f ../../../../docs/quickstart.md && echo OK
test -f ../../../../docs/project-config.md && echo OK
cd -
```

Expected: three `OK` lines.

- [ ] **Step 4: Commit**

```bash
git add specs/archive/2026/rust-front-end/13-plan-11-docs-overhaul.md
git commit -m "docs(specs): repair upward doc links in the archived plan

Moving rust-front-end/ from specs/ to specs/archive/2026/ pushed
the tree two levels deeper. Three markdown links in
13-plan-11-docs-overhaul.md used ../../docs/... and now resolve
inside specs/archive/2026/ instead of the repo's docs/. Rewrite
them to ../../../../docs/... so the archived design stays
readable on github.com without manual path adjustment."
```

### Task 3.3: Archive the five top-level shipped designs and their plans

**Files:**

- Move (9 files total — `keep-bound-command-decorator` shipped without a paired plan doc):
    - `specs/2026-05-18-keep-bound-command-decorator-design.md` → `specs/archive/2026/`
    - `specs/2026-05-19-external-command-sources-design.md` → `specs/archive/2026/`
    - `specs/2026-05-19-external-command-sources-plan-a.md` → `specs/archive/2026/`
    - `specs/2026-05-19-fill-the-gaps-design.md` → `specs/archive/2026/`
    - `specs/2026-05-19-fill-the-gaps-plan.md` → `specs/archive/2026/`
    - `specs/2026-05-21-dispatch-manifest-freshness-design.md` → `specs/archive/2026/`
    - `specs/2026-05-21-dispatch-manifest-freshness-plan.md` → `specs/archive/2026/`
    - `specs/2026-05-22-rust-build-manifest-design.md` → `specs/archive/2026/`
    - `specs/2026-05-22-rust-build-manifest-plan.md` → `specs/archive/2026/`

The *current* design (`specs/2026-05-22-repo-presentation-pass-design.md`) and *this plan* (`specs/2026-05-22-repo-presentation-pass-plan.md`) stay at the top level — they're the active work driving this very change.

- [ ] **Step 1: Move the shipped designs**

```bash
git mv specs/2026-05-18-keep-bound-command-decorator-design.md specs/archive/2026/
git mv specs/2026-05-19-external-command-sources-design.md specs/archive/2026/
git mv specs/2026-05-19-external-command-sources-plan-a.md specs/archive/2026/
git mv specs/2026-05-19-fill-the-gaps-design.md specs/archive/2026/
git mv specs/2026-05-19-fill-the-gaps-plan.md specs/archive/2026/
git mv specs/2026-05-21-dispatch-manifest-freshness-design.md specs/archive/2026/
git mv specs/2026-05-21-dispatch-manifest-freshness-plan.md specs/archive/2026/
git mv specs/2026-05-22-rust-build-manifest-design.md specs/archive/2026/
git mv specs/2026-05-22-rust-build-manifest-plan.md specs/archive/2026/
```

- [ ] **Step 2: Confirm the top level is clean**

```bash
ls specs/ | grep -v '^archive$' | grep -v 'repo-presentation-pass'
```

Expected: empty output. Only the two repo-presentation-pass files (this design + this plan) remain at the top level alongside `archive/`.

- [ ] **Step 3: Confirm cross-references between archived specs still resolve**

```bash
rg -n '\]\(\./' specs/archive/2026/*.md | head -5
rg -n '\]\(\.\./rust-front-end' specs/archive/2026/*.md
```

Expected: any matches reference files that now also live in `specs/archive/2026/`. No broken paths.

- [ ] **Step 4: Commit**

```bash
git commit -m "chore(specs): archive the five top-level shipped designs

Move the May 2026 design+plan pairs from the top level of specs/
into specs/archive/2026/ now that they're merged:
- keep-bound-command-decorator (2026-05-18)
- external-command-sources (2026-05-19, PR #222)
- fill-the-gaps (2026-05-19)
- dispatch-manifest-freshness (2026-05-21, PR #234)
- rust-build-manifest (2026-05-22, PR #235)

The repo-presentation-pass design + plan stay at the top level —
they're the active work driving this very change."
```

### Task 3.4: Add `specs/README.md`

**Files:**

- Create: `specs/README.md`

- [ ] **Step 1: Write `specs/README.md`**

Everything between the tilde fences is the literal file body.

````markdown
# Specs

Design records for toolr — both live work and historical post-mortems.

## Where work lives

- **Top level (`specs/<date>-<topic>-design.md`)** — active design
  work and proposed-but-not-shipped features. Each design pairs
  with a `<date>-<topic>-plan.md` implementation plan once it
  leaves brainstorming.
- **`specs/archive/<year>/`** — shipped or abandoned designs.
  Archived files are immutable post-mortem records; do not edit
  them in place. If a shipped design needs revising, write a new
  design that supersedes it.

## How to start a new design

Open a brainstorming session in Claude Code:

```text
/superpowers:brainstorming
```

The session writes the design here once you approve it. The
implementation plan follows from `/superpowers:writing-plans`.

## How to archive

When the PR implementing a design merges to `main`:

```sh
git mv specs/<date>-<topic>-design.md specs/archive/<year>/
git mv specs/<date>-<topic>-plan.md specs/archive/<year>/
```

Land the move in the same PR (or as an immediate follow-up).
````

- [ ] **Step 2: Verify rumdl passes on the new file**

```bash
prek run rumdl --files specs/README.md
```

Expected: pass.

- [ ] **Step 3: Commit**

```bash
git add specs/README.md
git commit -m "docs(specs): document the live-vs-archive split

Future contributors opening specs/ should be able to tell at a
glance which files describe active work vs. post-mortem records.
Add a short README explaining the top-level / archive/<year>/
split, how to start a new design (via the brainstorming skill),
and how to archive one once the implementing PR lands."
```

### Task 3.5: Delete `docs/.nav.yml`

**Files:**

- Delete: `docs/.nav.yml`

This file references `usage/`, `examples/`, `reference/toolr/` — none of which exist in `docs/`. `mkdocs.yml` has its own `nav:` block; `awesome-nav` ignores `.nav.yml` when an explicit `nav:` is set.

- [ ] **Step 1: Confirm `mkdocs.yml` has its own `nav:` block**

```bash
grep -A1 '^nav:' mkdocs.yml | head -3
```

Expected: `nav:` followed by `- Home: index.md` (or similar).

- [ ] **Step 2: Delete the file**

```bash
git rm docs/.nav.yml
```

- [ ] **Step 3: Verify mkdocs still builds**

```bash
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-after-3-5
```

Expected: success. The site still uses the explicit `nav:` from `mkdocs.yml`.

- [ ] **Step 4: Commit**

```bash
git commit -m "chore(docs): delete the dead .nav.yml

docs/.nav.yml referenced usage/, examples/, reference/toolr/ —
none of which exist in the current docs/ tree. mkdocs.yml has an
explicit nav: block, which mkdocs-awesome-nav respects over the
sibling .nav.yml. The file was dead; remove it."
```

### Task 3.6: Delete `tools/__init__.py` and verify dogfood

**Files:**

- Delete: `tools/__init__.py`

The file contradicts the documented PEP 420 namespace-package model. `toolr project init` deliberately does not create one. Removing it means the dogfood repo finally matches its own documentation.

- [ ] **Step 1: Delete the file**

```bash
git rm tools/__init__.py
```

- [ ] **Step 2: Run the dynamic introspection path against the dogfood tools/**

The dynamic layer imports `tools.*` modules via `_import_tools_modules` after putting `tools/..` on `sys.path`. For PEP 420 namespace packages this still works because Python 3.3+ resolves them automatically.

```bash
cargo run -p toolr -- project manifest rebuild
head -5 tools/.toolr-manifest.json
```

Expected: rebuild succeeds; the manifest contains the same `groups` and `commands` it did before. If the rebuild errors with `ModuleNotFoundError: No module named 'tools'`, that's a real bug to investigate before continuing — do not re-add `__init__.py` as a workaround.

- [ ] **Step 3: Run the full test envelope**

```bash
cargo test --workspace
uv run pytest -q
```

Expected: both succeed, matching the Task 3.0 baseline.

- [ ] **Step 4: Verify the dogfood commands still resolve**

```bash
cargo run -p toolr -- ci --help
```

Expected: the `ci` group's command list renders the same set as on `main`.

- [ ] **Step 5: Commit**

```bash
git add tools/.toolr-manifest.json tools/__init__.py
git commit -m "chore(tools): drop tools/__init__.py to match documented PEP 420 model

docs/concepts.md says 'tools/ is a PEP 420 namespace package - no
__init__.py is needed'. toolr project init scaffolds it that way.
The dogfood tools/__init__.py contradicted that. Remove it.

Both the static AST parser (walks tools/**/*.py directly) and the
dynamic introspect helper (relies on Python 3.3+ namespace-package
resolution after adding tools/.. to sys.path) handle the
__init__.py-less layout. Verified via cargo test --workspace and
uv run pytest."
```

If `tools/.toolr-manifest.json` doesn't change byte-for-byte after the deletion, drop it from the `git add` line.

### Task 3.7: Remove the two `rich-argparse` historical comments

**Files:**

- Modify: `crates/toolr/src/markdown.rs:6-7`
- Modify: `crates/toolr/src/cli.rs:7-8`

These comments describe the visual look the binary is matching against the *removed* Python frontend. They're vestigial; the design lives in workspace-split-design which is now archived.

- [ ] **Step 1: Edit `crates/toolr/src/markdown.rs`**

In the file header doc comment, replace lines 6–7:

```rust
//! something close to the rich rendering the legacy argparse path had
//! via `rich_argparse`.
```

with:

```rust
//! something close to a rich-rendered help page — bullet lists,
//! code spans, tables, headings.
```

- [ ] **Step 2: Edit `crates/toolr/src/cli.rs`**

In the doc comment on `help_styles`, replace lines 7–8:

```rust
/// Palette for `--help` output. Yellow + bold for section headers and
/// `Usage:`, green for arg names and choice values — closer to the
/// argparse / rich-argparse look the legacy toolr shipped.
```

with:

```rust
/// Palette for `--help` output. Yellow + bold for section headers and
/// `Usage:`, green for arg names and choice values.
```

- [ ] **Step 3: Verify the crate still compiles and tests pass**

```bash
cargo test -p toolr
```

Expected: success.

- [ ] **Step 4: Commit**

```bash
git add crates/toolr/src/markdown.rs crates/toolr/src/cli.rs
git commit -m "chore(toolr): drop rich-argparse comparison comments

The markdown.rs and cli.rs file/function doc comments referenced
rich-argparse and the 'legacy argparse path' that the Rust binary
replaced. That comparison stopped being useful once the Python
frontend was retired (workspace-split, now archived). Rewrite the
comments to describe what the code does now, not what it replaced."
```

### Task 3.8: PR-3 final verification + open as draft

**Files:** None.

- [ ] **Step 1: Full workspace test**

```bash
cargo test --workspace
```

Expected: success.

- [ ] **Step 2: Python tests**

```bash
uv run pytest -q
```

Expected: success.

- [ ] **Step 3: Strict mkdocs build**

```bash
mkdocs build --strict --site-dir /tmp/toolr-mkdocs-pr3-final
```

Expected: success.

- [ ] **Step 4: Full pre-commit gate**

```bash
prek run --all-files
```

Expected: every hook passes.

- [ ] **Step 5: Submit as a draft stacked PR**

```bash
gs branch submit --draft --fill
```

Expected: draft PR opened against `presentation-pass-2-readme-contributing`.

- [ ] **Step 6: Run review skills on the PR**

Same as PR-1 / PR-2.

### Task 3.9: Update auto-memory (post-merge)

**Files:**

- Modify: `~/.claude-work/projects/-Users-pedro-algarvio-projects-me-toolr/memory/toolr_rust_frontend_rewrite.md`
- Modify: `~/.claude-work/projects/-Users-pedro-algarvio-projects-me-toolr/memory/project_toolr_django_path.md` (and `MEMORY.md` index)

These edits happen **after** PR-3 merges, so the memory files reflect committed state, not in-flight work. They live outside the repo and don't get a git commit.

- [ ] **Step 1: Retire `toolr_rust_frontend_rewrite.md`**

Replace the file contents with:

````markdown
---
name: toolr Rust front-end rewrite (archived)
description: Multi-plan rewrite of toolr from Python-package-with-Rust-extension to Rust-binary front-end. Complete; design tree archived in-tree.
metadata:
  type: project
---

The 12 sub-plans of the Rust front-end rewrite are all merged.
Roadmap and design tree archived at
`specs/archive/2026/rust-front-end/` (moved from
`specs/rust-front-end/` in the 2026-05-22 repo presentation pass).

Subsequent design work that built on the rewrite — argparse scanner,
DispatchCommand, dispatch-manifest-freshness, rust-build-manifest —
ships in `specs/archive/2026/2026-05-*-{design,plan}.md`.

If you're resuming toolr work, read this repo's CONTRIBUTING.md
for current orientation; the rewrite-era roadmap is no longer the
right starting point.
````

- [ ] **Step 2: Correct `project_toolr_django_path.md`**

Replace the file contents with:

````markdown
---
name: toolr reference plugin
description: Canonical third-party plugin example for toolr lives at examples/plugin-package/ in-tree, package toolr_example_plugin, exercising the static-manifest discovery path.
metadata:
  type: project
---

The canonical reference plugin is `examples/plugin-package/`
inside the toolr repo. The package itself is
`toolr_example_plugin` (under `src/`); it ships a
`toolr-manifest.json` next to its `__init__.py`, demonstrating
the static-manifest discovery model documented at
`docs/third-party.md`.

There is no `crates/toolr-django/` directory; an earlier design
considered shipping one as a reference plugin, but the
external-command-sources work (PR #222) delivered Django support
generically via the built-in argparse scanner instead. The
worktree branch `worktree-toolr-command-authoring-skill-spec`
that references `crates/toolr-django/` predates that pivot and
is obsolete.

How to apply: when discussing toolr plugins, point at
`examples/plugin-package/toolr_example_plugin/` as the
copy-pasteable starting point.
````

- [ ] **Step 3: Update `MEMORY.md` if the description fields changed**

```bash
cd "$HOME/.claude-work/projects/-Users-pedro-algarvio-projects-me-toolr/memory"
cat MEMORY.md
```

If the descriptions in the index no longer match the file headers, update the matching lines to:

```text
- [toolr Rust front-end rewrite (archived)](toolr_rust_frontend_rewrite.md) — design tree archived to specs/archive/2026/; live work tracked in CONTRIBUTING.md.
- [toolr reference plugin](project_toolr_django_path.md) — canonical third-party plugin is examples/plugin-package/, not the never-built crates/toolr-django/.
```

(Filename of the second entry stays `project_toolr_django_path.md` for stability of incoming links from past sessions; the file's `name:` field is what changes.)

No git commit — memory lives outside the repo.

---

## Self-Review

Ran against `specs/2026-05-22-repo-presentation-pass-design.md`:

**1. Spec coverage:**

| Spec section                                       | Plan task(s)                                          | Covered? |
|----------------------------------------------------|-------------------------------------------------------|----------|
| PR-1 §Edits (writing-commands/index.md)            | Task 1.1                                              | yes      |
| PR-1 §Edits (known-bugs.md)                        | Task 1.2                                              | yes      |
| PR-1 §Edits (manifest.md File-shape)               | Task 1.3                                              | yes      |
| PR-1 §Edits (manifest.md Dynamic layer)            | Task 1.4                                              | yes      |
| PR-1 §Edits (manifest.md Hashing details)          | Task 1.5                                              | yes      |
| PR-1 §Edits (payload.rs comment)                   | Task 1.6                                              | yes      |
| PR-1 §Verification + size                          | Task 1.7                                              | yes      |
| PR-2 New README outline                            | Task 2.1                                              | yes      |
| PR-2 Benchmark process                             | Task 2.0 Step 3 + 2.1 references the measured numbers | yes      |
| PR-2 New CONTRIBUTING outline                      | Task 2.2                                              | yes      |
| PR-2 Verification + size                           | Task 2.3                                              | yes      |
| PR-3 specs/ archival (rust-front-end tree)         | Task 3.1                                              | yes      |
| PR-3 Upward-link repair (option A from design)     | Task 3.2                                              | yes      |
| PR-3 specs/ archival (top-level shipped designs)   | Task 3.3                                              | yes      |
| PR-3 New specs/README.md                           | Task 3.4                                              | yes      |
| PR-3 Delete docs/.nav.yml                          | Task 3.5                                              | yes      |
| PR-3 Delete tools/**init**.py                      | Task 3.6                                              | yes      |
| PR-3 Delete rich-argparse comments                 | Task 3.7                                              | yes      |
| PR-3 Pre-move grep                                 | Task 3.0 Step 2                                       | yes      |
| PR-3 Verification                                  | Task 3.8                                              | yes      |
| PR-3 Memory updates                                | Task 3.9                                              | yes      |

No spec section without a task.

**2. Placeholder scan:** No `TBD`, `TODO`, `FIXME`, `<placeholder>` strings. The benchmark numbers in Task 2.1's README content read `<MEAN ms>` / `<hardware>` as *templates for the engineer to fill in with measured data*, which is the intended workflow per Task 2.0 Step 3 — not unresolved spec gaps. The `[ASCII transcript ...]` line in the same block is also a template, filled in by Task 2.1 Step 2.

**3. Type consistency:** No type/method/property cross-references between tasks (this is a prose/file-move plan, not a code plan). Branch-name consistency verified: `presentation-pass-1-doc-fixes`, `presentation-pass-2-readme-contributing`, `presentation-pass-3-archive-cleanup` spelled identically across every reference (PR-1 Task 1.0, PR-2 Task 2.0, PR-3 Task 3.0, and the verification steps that submit each).

---

## Execution Handoff

Plan complete and saved to `specs/2026-05-22-repo-presentation-pass-plan.md`. Two execution options:

1. **Subagent-Driven (recommended)** — fresh subagent per task, review between tasks, fast iteration.
2. **Inline Execution** — execute in this session via `superpowers:executing-plans`, batch with checkpoints.

Which approach?
