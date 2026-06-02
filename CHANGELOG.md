# Changelog

All notable changes to this project will be documented in this file.

This project uses [*git-cliff*](https://git-cliff.org/) to automatically generate changelog entries
from [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.22.1 - 2026-06-02

### Notes

## Bug fixes

### `command_group(docstring=...)` now splits the docstring into title + description

`command_group("name", docstring=__doc__)` previously stuffed the entire
docstring into the group's `description` field and left `title` empty,
so the blurb never appeared next to the group in the parent `--help`
listing (clap shows `about`, which is what `title` populates). The
parser now mirrors how `@command` function docstrings are handled — the
first paragraph becomes the group's `title` (clap's `about`) and the
remainder becomes the `description` (clap's `long_about`). An explicit
positional `title=` / `description=` still wins for either side.

Both the static parser (`parser/groups.rs`) and the runtime decorator
(`toolr._decorators.command_group`) now go through the same shared
`parse_docstring` helper, so static-manifest output and runtime
introspection agree. The runtime's old `title = name` fallback —
which would have produced a redundant `dbt-config  dbt-config` in
the parent listing — has been removed; an unset title now stays
empty, matching the static parser.

See [#292](https://github.com/s0undt3ch/ToolR/issues/292).

### <!-- 1 -->🐛 Bug Fixes

- *(parser)* Split command_group docstring into title + description ([`f09fc6c`](https://github.com/s0undt3ch/ToolR/commit/f09fc6c147cf35e3b2c02f23af197e05046b543c))

### <!-- 2 -->🚜 Refactor

- *(tests)* Drop `del fixture` pattern via autouse / usefixtures ([`5589c6f`](https://github.com/s0undt3ch/ToolR/commit/5589c6f05c5877ca3687fa0f4a5b868f7573b266))
## 0.22.0 - 2026-06-02

### Notes

## ⚠ Breaking changes

### `toolr project deps` removed; replaced by `toolr project venv`

- **What changed:** the `toolr project deps` subcommand group has
  been removed. Its two commands moved under `toolr project venv`:
    - `toolr project deps sync` → `toolr project venv sync`
    - `toolr project deps upgrade <pkg>` → `toolr project venv upgrade <pkg>`
- **Behavior change on `sync`:** the new `toolr project venv sync`
  honours the tools venv's freshness stamp by default and no-ops
  (exit 0, no `uv sync`) when the venv is already up to date.
  Use `--force` to re-run unconditionally — that matches what
  `toolr project deps sync` did before.
- **New `--quiet` flag on `sync`:** silent on success and on
  benign unattended-mode exits ("not a toolr repo", "lock missing",
  "uv install needs consent"). Designed for use from a mise
  `[hooks].enter` recipe — see
  [Auto-sync the tools venv on shell-enter](https://toolr.readthedocs.io/latest/installation/mise/#auto-sync-the-tools-venv-on-shell-enter).
- **Migration:** running `toolr project deps <anything>` at 0.22
  prints a tailored error pointing at the new path and exits with
  code 2.
- **Why:** the `deps` group only ever held venv-touching operations;
  collapsing it under `venv` puts every tools-venv operation in one
  place and makes room for future uv-wrapper subcommands (`add`,
  `remove`, `lock`, …) — see
  [#288](https://github.com/s0undt3ch/ToolR/issues/288) — to land in
  the obvious location.

### <!-- 0 -->🚀 Features

- *(venv)* Thread --quiet through run_uv_sync and sync_if_needed ([`114e3bd`](https://github.com/s0undt3ch/ToolR/commit/114e3bd355232582c6b9f40c81fcbe8d99c78af8))
- *(uv)* Add silent_refuse to ConsentMode for unattended callers ([`964921e`](https://github.com/s0undt3ch/ToolR/commit/964921e471b6242d925295ed381d16dce0b71943))
- *(cli)* Move project deps to project venv; flip sync default ([`6d93be1`](https://github.com/s0undt3ch/ToolR/commit/6d93be1e69e9769d103dd6339a082031dd3cb7be))
- *(completions)* Move project venv sync/upgrade; drop deps ([`6617956`](https://github.com/s0undt3ch/ToolR/commit/661795643abecdd01f5cd85c80605ee797cbe5cb))

### <!-- 1 -->🐛 Bug Fixes

- *(hints)* Point users to project venv sync, not project deps sync ([`7053a6b`](https://github.com/s0undt3ch/ToolR/commit/7053a6b8b4325724a8d5a56a25243000c9315fe2))
- *(ci)* Update Python test assertions and changelog link for venv rename ([`699a30b`](https://github.com/s0undt3ch/ToolR/commit/699a30bcb4250827300afa71e155623b15f2c87e))

### <!-- 10 -->💼 Other

- Add design for mise enter-hook auto-sync of tools venv ([`ede2108`](https://github.com/s0undt3ch/ToolR/commit/ede2108af24fce0638154779e2b7c2756826adf2))
- Add implementation plan for mise-enter-hook auto-sync ([`fdb0fb2`](https://github.com/s0undt3ch/ToolR/commit/fdb0fb20384b297dde3414187f6c51c7e90edc1d))
- Link design + plan to follow-up issue #288 ([`c5a8fea`](https://github.com/s0undt3ch/ToolR/commit/c5a8feaaad63a333dd01639e58effe8604e4b699))
- Archive mise-enter-auto-sync design + plan (implemented) ([`e7833a2`](https://github.com/s0undt3ch/ToolR/commit/e7833a2b3b35fc65a892b5728fc11a3fce6e921f))
- Design venv ↔ uv parity (drop venv upgrade, add lock/add/remove) ([`0f4676d`](https://github.com/s0undt3ch/ToolR/commit/0f4676d42fcdc237c645d10c9a0a7cc9daa29dd9))
- Implementation plan for venv ↔ uv parity ([`6eeb250`](https://github.com/s0undt3ch/ToolR/commit/6eeb25093ad42e9912d7a719eb60168d4f785246))
- Introduce UpgradeMode enum ([`051b382`](https://github.com/s0undt3ch/ToolR/commit/051b3829f9f1c728fb4ec47ce92c385e8ac0326e))
- Thread UpgradeMode through run_uv_sync + sync_if_needed ([`2916619`](https://github.com/s0undt3ch/ToolR/commit/291661908992b1a06b86e441065ffe4ae10f5bf9))
- Replace run_uv_lock_upgrade with general run_uv_lock ([`9015bbf`](https://github.com/s0undt3ch/ToolR/commit/9015bbf5b973951ab717d721f27f2395716dfe90))
- Add edit module with run_uv_add + run_uv_remove ([`485f41c`](https://github.com/s0undt3ch/ToolR/commit/485f41c63411f9be6daa2d594a15bcd22d50ee79))
- Thread UpgradeMode through EnsureOpts ([`a8c3322`](https://github.com/s0undt3ch/ToolR/commit/a8c332270e460591c2e78d4fb3dab35ed39f965f))
- Document venv upgrade removal + new lock/add/remove + sync -U/-P ([`5949d96`](https://github.com/s0undt3ch/ToolR/commit/5949d96cdd30892e29648347692f702b0c262657))
- Archive venv-uv-parity design + plan (implemented) ([`1503e53`](https://github.com/s0undt3ch/ToolR/commit/1503e535083bcf4e56302a9fc94a5070ab1c383a))

### <!-- 2 -->🚜 Refactor

- *(venv)* Replace ensure_venv_ready force flag with EnsureOpts ([`af7c111`](https://github.com/s0undt3ch/ToolR/commit/af7c1117f7ef23a7fdcd6aa546b2a680456655e3))

### <!-- 3 -->📚 Documentation

- *(snippets)* Regenerate project venv sync --help capture ([`18561a3`](https://github.com/s0undt3ch/ToolR/commit/18561a3155c2d3ecb33b1eebfe98ec0eae42efc1))
- *(snippets)* Remove obsolete project-deps-sync help capture ([`7bf6aea`](https://github.com/s0undt3ch/ToolR/commit/7bf6aea4464e2c8df0ac302ea38249a10ef50444))
- Rename toolr project deps references to project venv ([`5aa4c42`](https://github.com/s0undt3ch/ToolR/commit/5aa4c42d72c7a42fc4e199c5e298811085c47252))
- *(mise)* Document the enter-hook auto-sync recipe ([`bbce59e`](https://github.com/s0undt3ch/ToolR/commit/bbce59e738bcfd13917523df550254d5832248fd))
- *(unreleased)* Note the project deps → project venv rename ([`c5489b1`](https://github.com/s0undt3ch/ToolR/commit/c5489b10bb8f9da525a74693201672b13e02e33f))
- Regenerate cli-files snippets for venv lock/add/remove + sync -U/-P ([`6c49bd2`](https://github.com/s0undt3ch/ToolR/commit/6c49bd2b443594c93eed862ea99e04e29687cdc9))
- Cover venv lock/add/remove and migrate venv upgrade references ([`fcdbb97`](https://github.com/s0undt3ch/ToolR/commit/fcdbb977b34b76e6319f2ff51d3f285cbb23d1a7))

### <!-- 6 -->🧪 Testing

- *(cli)* Update smoke assertions to project venv sync ([`ea462ce`](https://github.com/s0undt3ch/ToolR/commit/ea462ceae140a17346751dab91b7a1d39a75677d))
- *(cli)* Rename project_deps_upgrade to project_venv_upgrade ([`35405fb`](https://github.com/s0undt3ch/ToolR/commit/35405fb524115ca80b45dfa885dbc888987d9c79))
- *(cli)* Integration coverage for project venv sync flag matrix ([`fb1574f`](https://github.com/s0undt3ch/ToolR/commit/fb1574f451611cb3030d94c675f116ddee08a1d0))
- *(deps-check)* Update missing-deps hint assertion to project venv sync ([`ffb4ca1`](https://github.com/s0undt3ch/ToolR/commit/ffb4ca173d8ca67d9459d809f67609290612832d))
- Cover venv handler success paths via stub-uv + fake-venv fixture ([`6f4fed6`](https://github.com/s0undt3ch/ToolR/commit/6f4fed6e45960b3e1f453b1435bd86fa8ac61459))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- Bump throttle action max-sleep default to 15s ([`6c40d5a`](https://github.com/s0undt3ch/ToolR/commit/6c40d5a1139bf73b7e4f783c629d30a63240b4b5))
## Unreleased

### Notes

## ⚠ Breaking changes

### `project venv upgrade` removed in favour of `venv sync -U / -P`

- **What changed:** `toolr project venv upgrade <pkg>` is gone.
  Use `toolr project venv sync -U <pkg>` to upgrade a single package
  (or `-P <pkg>` repeatedly), or `toolr project venv sync -U` to
  re-resolve all packages.
- **Why:** uv expresses upgrades as flags on `lock` and `sync`, not as
  a standalone verb. Aligning toolr with uv's surface removes a
  toolr-specific verb that didn't pull its weight.
- **Migration:** mechanical rename. `venv upgrade foo` → `venv sync -P foo`.

## 🚀 New features

- *(project venv)* `lock` — wrap `uv lock` for refreshing
  `tools/uv.lock` without applying ([#288](https://github.com/s0undt3ch/ToolR/issues/288))
- *(project venv)* `add <package>[@<version>]…` — wrap `uv add` against
  `tools/` ([#288](https://github.com/s0undt3ch/ToolR/issues/288))
- *(project venv)* `remove <package>…` — wrap `uv remove` against
  `tools/` ([#288](https://github.com/s0undt3ch/ToolR/issues/288))
- *(project venv sync)* `-U` / `--upgrade` and `-P` / `--upgrade-package`
  flags mirroring uv ([#288](https://github.com/s0undt3ch/ToolR/issues/288))

## 0.21.1 - 2026-06-01

### <!-- 1 -->🐛 Bug Fixes

- *(dispatch)* Surface arg() metadata TypeError from dispatcher detection ([`415435d`](https://github.com/s0undt3ch/ToolR/commit/415435d41c2cdfa90d2bd5f96781c559df6b0410))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(ci)* Bump GitHub Action pin SHAs and MISE_VERSION ([`8b1ae08`](https://github.com/s0undt3ch/ToolR/commit/8b1ae08f256c185b05d35eb1b578af786b26126f))
## 0.21.0 - 2026-05-29

### Notes

## ⚠ Breaking changes

### `installation/mise/` plugin removed in favour of the aqua backend

- **What changed:** the bundled asdf-style mise plugin under
  `installation/mise/` has been deleted. mise users now install
  toolr through mise's built-in
  [aqua backend](https://mise.jdx.dev/dev-tools/backends/aqua.html)
  against the
  [aqua-registry entry](https://github.com/aquaproj/aqua-registry/tree/main/pkgs/s0undt3ch/ToolR):

  ```sh
  mise use aqua:s0undt3ch/ToolR@latest
  ```

  No plugin to register, no repository to clone, no `git::<url>//<subdir>`
  syntax to maintain. The aqua-registry entry pulls the same signed
  GitHub release archive the in-tree plugin used to fetch, so the
  installed binary is identical.

- **Why:** the in-tree plugin duplicated logic that mise already
  provides through the aqua backend (release-asset selection,
  SHA-256 verification, archive extraction). Owning that surface
  meant keeping the asdf-protocol hooks in sync with mise's
  evolving subdir-URL semantics and shipping our own attestation
  logic — work that aqua does once for everyone.

- **Migration:** replace the install command everywhere it appears
  (README, quickstart, mise docs page, internal runbooks, CI
  workflows). The old form

  ```sh
  mise plugin add toolr git::https://github.com/s0undt3ch/ToolR.git//installation/mise
  mise use --global toolr@latest
  ```

  becomes

  ```sh
  mise plugin uninstall toolr   # if previously installed
  mise use aqua:s0undt3ch/ToolR@latest
  ```

  (Add `--global` if you want toolr available across every
  directory; the project-scoped form above is the default we
  show in the headline docs.)

  The aqua-registry entry requires aqua-registry `v4.518.0` or
  newer (the first release that contains
  `pkgs/s0undt3ch/ToolR/registry.yaml`) and mise's aqua backend,
  which is built in and needs no extra configuration.

### <!-- 0 -->🚀 Features

- *(action)* Install uv via astral-sh/setup-uv when missing or version-pinned ([`8eb6906`](https://github.com/s0undt3ch/ToolR/commit/8eb69066471d8b3d6816f11d9f462e4e87944604))

### <!-- 1 -->🐛 Bug Fixes

- *(uv)* Detect musl libc in auto-installer and add actionable exec-failure hint ([`5af1ca0`](https://github.com/s0undt3ch/ToolR/commit/5af1ca056d8444b53c2c75f31d0c361af02380ca))
- *(action)* Bake released version into action.yml so SHA pins resolve correctly ([`180b3d1`](https://github.com/s0undt3ch/ToolR/commit/180b3d197f6da599c10485eb22b7256b6c430aee))
- Regenerate toolr-ci-setup action.md and widen build-skill-refs trigger ([`71b1830`](https://github.com/s0undt3ch/ToolR/commit/71b183025a0a6157b6a95a1db8f603840064f2bc))
- Regenerate toolr-ci-setup action.md for new `uv-version` input ([`62ae52c`](https://github.com/s0undt3ch/ToolR/commit/62ae52c48a63c1bd01dee03445112a33667c0b95))

### <!-- 10 -->💼 Other

- Drop in-tree asdf plugin, install via aqua backend ([`5f60df4`](https://github.com/s0undt3ch/ToolR/commit/5f60df48949a0f07d8ac5c51025d1ca02dc12330))
- Drop --global from headline install snippets ([`52e7357`](https://github.com/s0undt3ch/ToolR/commit/52e73572ec7834d8a3f1cf0c6649ac8e85690371))
- Leave the uv workspace, resolve toolr-py from PyPI ([`fa9fa20`](https://github.com/s0undt3ch/ToolR/commit/fa9fa20329539bd2374c2ee6cbae6f7a2eeb10d6))

### <!-- 2 -->🚜 Refactor

- *(version)* Write dev versions to action.yml on every push so the bake-in is exercised in CI ([`7995fb0`](https://github.com/s0undt3ch/ToolR/commit/7995fb0bc2cde905448e35cb5c08fe678c029d4f))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- Drop self action-version pin and auto-sync rolling tags on release ([`e46c19c`](https://github.com/s0undt3ch/ToolR/commit/e46c19c826d414a828471bbf79775eef3ce01a35))
- *(prepare-release)* Skip tools venv sync (built-in toolr only) ([`c5ef129`](https://github.com/s0undt3ch/ToolR/commit/c5ef129fbd1659e1c149b08a3ce586af12773b2e))
- Format `build-skill-refs` and `regen-doc-snippets` `files:` regexes as verbose multi-line ([`44649d2`](https://github.com/s0undt3ch/ToolR/commit/44649d2e50989e4d73b3075fad946007698d1527))
- Collapse per-alt `$` anchors in `build-skill-refs` regex to a single trailing `)$` ([`563467d`](https://github.com/s0undt3ch/ToolR/commit/563467d3aa046d9e4838d8f19b2192740e50a2fc))
- *(_test)* Pass `--no-sync` to `uv run` so prebuilt toolr-py wheel survives ([`c901231`](https://github.com/s0undt3ch/ToolR/commit/c901231a4a42d2bf84b1f1a8f984e3e7fcb67d90))
## 0.20.1 - 2026-05-28

### Notes

## ⚠ Breaking changes

### `s0undt3ch/ToolR` GitHub Action: binary-only install, no more pipx

- **What changed:** the `Setup ToolR` composite action no longer
  installs toolr via `pipx install toolr==<version>`. The 0.20.0
  release shipped toolr as a binary-only PyPI wheel (no Python
  source), which pipx cannot install — so the old action path was
  already broken at the point this change landed.

  The rewritten action downloads the toolr binary archive directly
  from a GitHub release (`gh release download` with a `curl`
  fallback), cryptographically verifies the SLSA build provenance
  via `gh attestation verify`, caches the result, and puts the
  binary on `PATH`. It also caches `tools/.venv` keyed on
  `tools/pyproject.toml` + `tools/uv.lock` and sets
  `TOOLR_VENV_LOCATION=in-tree` so the cache works out of the box.

- **Minimum version:** the action refuses to install toolr below
  `0.20.0`. Earlier releases used the Python source distribution
  and are not compatible with the binary-only flow.

- **Migration:** remove the deprecated `python-path` and
  `requirements-file` inputs from your workflow's `Setup ToolR`
  step; the action has no use for them now that toolr is a
  standalone binary with its Python deps in the per-project
  `tools/.venv`. New optional inputs: `version` (defaults to the
  action ref, falling back to `latest`), `skip-attestation`
  (defaults to `false`), `cache-prefix` (defaults to `setup-toolr`),
  and `cache-tools-venv` (defaults to `true`).

### `installation/mise/` plugin: minimum toolr `0.20.0`, attests by default

- **What changed:** the bundled mise plugin under
  `installation/mise/` now rejects toolr versions below `0.20.0`
  (matching the action's cutoff) and verifies the SLSA build
  provenance via `gh attestation verify` on every install. Set
  `TOOLR_SKIP_ATTESTATION=1` to bypass — the plugin tells you so
  loudly if `gh` is missing from `PATH`.

### `installation/mise/` plugin: install URL requires mise `v2026.5.11+`

- **What changed:** the documented install command moved from
  `mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise`
  to
  `mise plugin add toolr git::https://github.com/s0undt3ch/ToolR.git//installation/mise`.

  The `#<subdir>` form was never a valid mise syntax — mise has
  always interpreted `#` as a git ref selector. The correct
  subdirectory syntax (`git::<git-url>//<subdir>`) only landed in
  mise [v2026.5.11](https://github.com/jdx/mise/pull/9893)
  (May 17, 2026). Workflows pinning `MISE_VERSION` below that
  cutoff need to bump.

- **Migration:** swap the install command everywhere it appears
  (READMEs, CI workflows, internal runbooks) and ensure your mise
  installation is `v2026.5.11` or newer. The plugin source itself
  is unchanged.

## New features

### `TOOLR_VENV_LOCATION` environment variable

The new `TOOLR_VENV_LOCATION` env var overrides the
`[tool.toolr] venv-location` setting in `tools/pyproject.toml`.
Accepts the same `in-tree` / `cache` spellings the TOML key does.
Intended primarily for CI: the `Setup ToolR` action sets it to
`in-tree` automatically so workflows can cache `tools/.venv`
directly without forcing every consumer repo's
`tools/pyproject.toml` to declare `venv-location = "in-tree"`.

### Agent skills

Toolr now ships two in-tree agent skills, installable via
`skillshare` from this repository:

- **`toolr-command-authoring`** — teaches LLM coding assistants
  how to author toolr commands in a project's own `tools/*.py`
  files. Anchored on `toolr project init` and
  `toolr <group> <cmd> --help`; the API surface and docstring
  conventions are regenerated from `toolr-py`'s public surface
  and the parser's section-header table.
- **`toolr-command-packaging`** — teaches LLM coding assistants
  how to ship an existing set of toolr commands as a
  distributable Python plugin. Anchored on the in-tree
  `examples/plugin-package/`; the manifest fragment schema is
  regenerated from `toolr-core`'s serde types.
- **`toolr-ci-setup`** — teaches LLM coding assistants how to
  integrate toolr into a project's GitHub Actions CI. Covers the
  `s0undt3ch/ToolR` GitHub Action: pinning policy, two canonical
  recipes (run a toolr command; gate
  `toolr self build-manifest --check`), and the common failure
  modes a caller hits first. Installable via `skillshare` from
  `skills/toolr-ci-setup/`.

A new maintainer-only `crates/xtask/` workspace crate hosts the
generator (`cargo xtask build-skill-refs`). The `--check` variant
runs in CI on every PR (alongside the existing example-plugin
manifest check) so a public-surface change that forgets to
regenerate the skill references cannot land. A `prek` hook entry
gives the same gate locally.

`cargo xtask build-skill-refs` gains a third generator
(`ci_setup::action`) that rebuilds
`skills/toolr-ci-setup/references/action.md` from the
repository-root `action.yml`. The existing `--check` CI gate
automatically covers the new file.

`docs/skills.md` install instructions now use the `skillshare`
parent-path picker pattern
(`skillshare install s0undt3ch/toolr/skills`) instead of one
command per skill, so the install block does not grow with each
new skill.

See [docs/skills.md](https://toolr.readthedocs.io/latest/skills/)
for the user-facing installation flow.

### <!-- 0 -->🚀 Features

- *(venv)* TOOLR_VENV_LOCATION env var overrides venv-location config ([`5bba1a3`](https://github.com/s0undt3ch/ToolR/commit/5bba1a3e1605ef9cccfbf0fd6780e1d83da9a1bc))
- *(action)* Binary-download + SLSA attest install for setup-toolr ([`ca66e25`](https://github.com/s0undt3ch/ToolR/commit/ca66e251de4c9648e1db91718ee56da3df7285b3))
- *(mise)* Minimum toolr 0.20.0 + verify SLSA build provenance ([`51d38c5`](https://github.com/s0undt3ch/ToolR/commit/51d38c56185b2dbbf313173e1ce3add50064bc62))
- *(mise)* Prefer 'gh' CLI over curl/wget for github.com hits ([`3b2edf0`](https://github.com/s0undt3ch/ToolR/commit/3b2edf0c3b0a66349dfac4d44103e09b1e401d23))
- *(config)* Add .rustfmt.toml + .editorconfig with 100-col policy ([`261e5ea`](https://github.com/s0undt3ch/ToolR/commit/261e5ea6a287be4823768ccc3489ae14ccd8b9eb))
- *(xtask)* Scaffold maintainer-only crates/xtask/ workspace crate ([`b53ebbd`](https://github.com/s0undt3ch/ToolR/commit/b53ebbdfcbc3f1e69a5b8e9d4ff936abeb577c04))
- *(docstrings)* Expose KNOWN_SECTION_HEADERS as introspectable table ([`b8248e3`](https://github.com/s0undt3ch/ToolR/commit/b8248e306b5f9e22521d26ad853051a8e68526d0))
- *(xtask)* Generate authoring skill references from toolr-py source ([`aeef5d2`](https://github.com/s0undt3ch/ToolR/commit/aeef5d284aa1b5e5aaae375156b6433d631f6f3c))
- *(skills)* Author skill prose for toolr-command-authoring ([`1bcb0a3`](https://github.com/s0undt3ch/ToolR/commit/1bcb0a381174607b0b0678629d47910a3d85acee))
- *(xtask)* Generate packaging skill references from toolr-core types ([`fb440b4`](https://github.com/s0undt3ch/ToolR/commit/fb440b4380303f72f461b5c18b9f7f95a4f6b619))
- *(skills)* Author skill prose for toolr-command-packaging ([`41d5a4d`](https://github.com/s0undt3ch/ToolR/commit/41d5a4dc82536af0c9b6873267b8246c1d9ac0ff))
- *(xtask)* Break generator prose at sentence boundaries ([`b85c464`](https://github.com/s0undt3ch/ToolR/commit/b85c464ae99a3a0dff62eb44ec6be9a0d52cd471))

### <!-- 1 -->🐛 Bug Fixes

- *(action)* Run `toolr project deps sync` + cache the resolved venv path ([`153052a`](https://github.com/s0undt3ch/ToolR/commit/153052a8bc94cedae8a01b79890282694de4ab59))
- *(action)* Drive uv sync directly with --no-default-groups + --frozen ([`08aa7db`](https://github.com/s0undt3ch/ToolR/commit/08aa7dbf558da93064e8dc063d912b56db4d2fc7))
- *(install-smoke)* PS7 latest-version resolver + mise plugin link ([`33810bd`](https://github.com/s0undt3ch/ToolR/commit/33810bda27800ec89d196baf0ecad44537a8b7e0))
- *(mise)* Authenticate list-all against api.github.com when possible ([`a4602eb`](https://github.com/s0undt3ch/ToolR/commit/a4602ebef79ebaaa36b24f947b626c40dc32e61e))
- *(mise)* Also authenticate bin/download against github.com ([`3ede133`](https://github.com/s0undt3ch/ToolR/commit/3ede133bdcd3a8e1662e23e0fc1cfdd8969980e1))
- *(lint)* Exclude generated skills/*/references from rumdl ([`0d03a35`](https://github.com/s0undt3ch/ToolR/commit/0d03a351d70e036c830e0b5b1c7dc0bc8e0108d5))
- *(skill-refs)* LF line endings on Windows + show diff in CI on drift ([`42866c8`](https://github.com/s0undt3ch/ToolR/commit/42866c8d09a00596ef0431c2c1dfb71ddee872e1))
- *(skill-refs)* LF line endings on skill example .py files ([`528c8c7`](https://github.com/s0undt3ch/ToolR/commit/528c8c742b7eb54990b2566331752f87a11f322b))
- *(mise)* Correct plugin URL syntax + bump MISE_VERSION ([`eb52b85`](https://github.com/s0undt3ch/ToolR/commit/eb52b859ad1c9c83abaca4faa0ad78f33df25dd1))

### <!-- 10 -->💼 Other

- *(xtask)* Collapse to single debug alias + prek hook regenerates ([`b7f32ab`](https://github.com/s0undt3ch/ToolR/commit/b7f32abe27b808c223b443ae1de62db9d77dc51d))
- Add design for toolr-ci-setup agent skill ([`3e0d9b7`](https://github.com/s0undt3ch/ToolR/commit/3e0d9b7383fff66152bdb5d118628e58308560ea))
- Add implementation plan for toolr-ci-setup skill ([`b8a21f4`](https://github.com/s0undt3ch/ToolR/commit/b8a21f443e98d889bb33cca5d8b6b3b8629bc14a))
- Add serde_yml + indexmap for upcoming action.yml generator ([`701a546`](https://github.com/s0undt3ch/ToolR/commit/701a546f5a890b72e844c989cdd3a61a5242b0f4))
- Add ci_setup generator (renders action.md from action.yml) ([`54234df`](https://github.com/s0undt3ch/ToolR/commit/54234dfb6f6c1d20b91bd99b13fad5e4a5296079))
- Register ci_setup generator; commit generated action.md ([`7179547`](https://github.com/s0undt3ch/ToolR/commit/71795479fd214a2ee014a884d298cd1602022f63))
- Assert action.yml inputs/outputs match references/action.md ([`5c5b722`](https://github.com/s0undt3ch/ToolR/commit/5c5b722db02d00dccd9c70618ee3955e20177480))
- Author SKILL.md body and trigger ([`f9d3c6c`](https://github.com/s0undt3ch/ToolR/commit/f9d3c6ce65f3669bbb27680039645abe62b800fa))
- Add README ([`9e45ac6`](https://github.com/s0undt3ch/ToolR/commit/9e45ac6a35230d48250fb68e6e39baf745c9e6a7))
- Add REVIEW checklist ([`ac6e9d4`](https://github.com/s0undt3ch/ToolR/commit/ac6e9d4d0b07fd27c3e85bf4ff8df68da3e03186))
- Add trigger fixture ([`d6b4e51`](https://github.com/s0undt3ch/ToolR/commit/d6b4e51902bf9f3ab9213216f4d1ecdd7d07c3ee))
- Cross-link to toolr-ci-setup ([`498fa9b`](https://github.com/s0undt3ch/ToolR/commit/498fa9bdb2dbaea66e16aa2e354d533cb1d020e1))
- Cross-link to toolr-ci-setup from rule 3 ([`bb884cb`](https://github.com/s0undt3ch/ToolR/commit/bb884cbdfdc281b73aa0de190a0564c7bb0cca74))
- Note toolr-ci-setup skill and install-pattern change ([`77265bd`](https://github.com/s0undt3ch/ToolR/commit/77265bd6fa0d808e14dbbcf7e71e21b03fb516f1))
- Swap serde_yml for serde_yaml_ng to drop unsound libyml dep ([`e0615de`](https://github.com/s0undt3ch/ToolR/commit/e0615def09bed9f62e296469742f907b858ad8c9))

### <!-- 3 -->📚 Documentation

- *(specs)* Add toolr command-authoring skill design ([`e5e095b`](https://github.com/s0undt3ch/ToolR/commit/e5e095b548fcf79618965eabbd494e66ed61c13f))
- *(specs)* Add toolr command-packaging skill design ([`91ef08e`](https://github.com/s0undt3ch/ToolR/commit/91ef08e49d1088e41c2fb898d8672d3a7cd0ea38))
- *(specs)* Drop python introspection subprocess from skill-refs generator ([`7a5154b`](https://github.com/s0undt3ch/ToolR/commit/7a5154be85f7b1fc609b545c3ae69d371c09f66f))
- *(specs)* Use toolr.__all__ as the public-surface contract for skill-refs ([`273bf9d`](https://github.com/s0undt3ch/ToolR/commit/273bf9d09753f0f002b9b22fb746fdf04d22a72a))
- *(specs)* Add docstring conventions; reuse toolr-core parser/docstrings ([`b90d7c3`](https://github.com/s0undt3ch/ToolR/commit/b90d7c3d4cd2d94869d3fc63494a4dd409ffdb97))
- *(specs)* Packaging skill anchors on examples/plugin-package and new CLI surface ([`5dccaf9`](https://github.com/s0undt3ch/ToolR/commit/5dccaf967eaf6ee4f2f208822219d710a11d880d))
- *(specs)* Consolidated plan for authoring + packaging skills ([`d92167b`](https://github.com/s0undt3ch/ToolR/commit/d92167bf97712554cd2fa891ec2a0b4d0cbf568b))
- *(skills)* Add docs landing page, queue release notes, archive designs ([`5b34278`](https://github.com/s0undt3ch/ToolR/commit/5b3427837bfb06c8fb9f28a501986374f79fe96c))
- *(skills)* Link agent skills from README and docs landing ([`d72bec8`](https://github.com/s0undt3ch/ToolR/commit/d72bec8b5ada26d124446c275b635dc7e5498da0))
- List toolr-ci-setup and consolidate install block ([`b6086fb`](https://github.com/s0undt3ch/ToolR/commit/b6086fbfee1a152cffe740fd3a569ab7f28270f4))

### <!-- 5 -->🎨 Styling

- *(python)* Split ruff line-length (format 100, lint 120) + reformat ([`85dbecc`](https://github.com/s0undt3ch/ToolR/commit/85dbeccc15abe2b9497eca2449fb57442bfcc0f2))
- *(docs)* Enable MD013 with 120-char ceiling + reflow long lines ([`350c88f`](https://github.com/s0undt3ch/ToolR/commit/350c88fcc0ad5d313a1563102dbfcf375cf8d02f))

### <!-- 6 -->🧪 Testing

- *(install-smoke)* Exercise mise plugin via canonical URL form ([`8797fc4`](https://github.com/s0undt3ch/ToolR/commit/8797fc43474aec6934f1994828d0cbbe851b40e1))
- *(skills)* Snapshot manifest over the authoring skill's examples tree ([`f347410`](https://github.com/s0undt3ch/ToolR/commit/f347410e71f7c16c1a6788a8ea04130e34b3852f))
- *(skills)* Refresh authoring-skill manifest snapshot after ruff reformat ([`9a709ba`](https://github.com/s0undt3ch/ToolR/commit/9a709ba0eab746e1505a98fb041ee8d321bc8b23))
- *(xtask)* Use assert_cmd::cargo_bin so subprocess coverage counts ([`2b5e351`](https://github.com/s0undt3ch/ToolR/commit/2b5e35135f6dbe7f181c7293a6036b226963e448))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- Update workflows + docs for new setup-toolr action shape ([`a6ec0e8`](https://github.com/s0undt3ch/ToolR/commit/a6ec0e8dd2dfd6085d826e0965faa2c7dfae69f6))
- *(install-smoke)* Add pull_request trigger + gate remote-only jobs ([`b1bad02`](https://github.com/s0undt3ch/ToolR/commit/b1bad021a2eaeb1a75ee5fc7bfcfd9d1de1448d5))
- *(_test)* Regen-and-diff the example-plugin manifest gate too ([`dcf470c`](https://github.com/s0undt3ch/ToolR/commit/dcf470ce7f7199f21e81d6d565cf956d34fa9399))
## 0.20.0 - 2026-05-26

### Notes

This release lands the **Rust front-end rewrite** together with a
**workspace split** and a **distribution-channel reshuffle**. The
argparse-driven Python CLI is fully retired; the `toolr` command is
now a native Rust binary, and the PyPI footprint splits into two
packages so the CLI and the Python runtime can be installed
independently.

If you only invoke `toolr ...` from a project that already ships a
`tools/` directory, the smallest migration is:

1. Install the new CLI binary (`pip install toolr`, `installation/install.sh`,
   mise, or a GitHub release archive — see below).
2. Run `toolr project init` from your repo root. This scaffolds the
   new `tools/pyproject.toml` (with `toolr-py` already declared) and a
   `tools/uv.lock` alongside your existing `tools/*.py` scripts.
3. Move any Python dependencies your `tools/*.py` previously pulled in
   (e.g. via the project's main `pyproject.toml` dev group) into the
   `[project.dependencies]` list inside the new `tools/pyproject.toml`,
   then run `toolr project deps sync`.
4. **Commit `tools/pyproject.toml` and `tools/uv.lock` to git.** Both
   are part of the per-project tools venv contract — without them,
   collaborators and CI can't reproduce your tools venv.

The sections below spell out every other place this rewrite is visible
from the outside.

## ⚠ Breaking changes

### `python -m toolr` is gone

- **What changed:** the `[project.scripts] toolr` console entry
  point and `toolr/__main__.py` have been removed. The Python
  package no longer ships a CLI at all — invoking `python -m toolr`
  fails with `No module named toolr.__main__`.
- **Migration:** install the new Rust CLI (via `pip install toolr`,
  `install.sh`, mise, or a GitHub release archive — see below) and
  invoke it as `toolr <args>`. The argument surface is unchanged;
  only the entry point moved.

### `pip install toolr` no longer makes `import toolr` work

- **What changed:** PyPI now hosts **two** packages. `toolr` is a
  binary-only wheel (`bindings = "bin"`) — it drops the `toolr`
  executable into the wheel's `scripts/` directory and has **no
  Python source**. The Python runtime (`import toolr`, `Context`,
  `command_group`, `toolr.utils`, the `_rust_utils` extension)
  lives in a separate package, `toolr-py`.
- **Migration:** if your project's `tools/*.py` scripts do
  `from toolr import ...`, declare `toolr-py` as a dependency of
  the tools venv. The fastest path is `toolr project init` from
  your repo root — it scaffolds `tools/pyproject.toml` with
  `toolr-py` already declared and a matching `tools/uv.lock`.
  Both files belong in git. The CLI on `PATH` will then find
  `toolr-py` when it shells out to execute a command. See the
  [installation docs](https://toolr.readthedocs.io/en/latest/installation/)
  for the full layout.

### mise plugin: external `mise-toolr` repo retired

- **What changed:** the mise plugin used to be hosted out-of-tree
  at `s0undt3ch/mise-toolr`. It is now self-contained at
  `installation/mise/` inside this repo, and the external
  `mise-toolr` repository is retired. The old
  `mise plugin add toolr https://github.com/s0undt3ch/mise-toolr`
  installation stops working.
- **Migration:**

  ```sh
  mise plugin remove toolr   # if previously installed from the old path
  mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise
  mise use --global toolr@latest
  ```

### The argparse Python CLI internals were deleted

- **What changed:** `toolr/__main__.py`, `toolr/_parser.py`, and
  `toolr/_registry.py` have been deleted. Anything that imported
  `Parser`, `CommandRegistry`, or other internals from those
  modules will break.
- **Migration:** the user-facing decorator surface
  (`command_group`, `@command`, `CommandGroup`,
  `MANIFEST_SCHEMA_VERSION`) is preserved. It has moved to
  `toolr._decorators` and is re-exported from the top-level
  package, so the public form continues to work:

  ```python
  from toolr import command_group, command, CommandGroup
  ```

  If you were reaching into `toolr._registry` or `toolr._parser`
  directly, there is no replacement — the Rust binary owns
  manifest discovery, argument parsing, and dispatch now. The
  Python runtime is invoked per-command via a JSON spec file
  (`toolr._runner`).

### `testing.py` import surface tightened

- **What changed:** `toolr.testing` previously re-exposed a few
  helpers that leaned on the now-deleted `_parser` /
  `_registry` modules. Those have been replaced or removed as
  part of the three-way test prune; the supported public surface
  is whatever `toolr.testing` exports today.
- **Migration:** if your test suite imported internal helpers
  from `toolr.testing` and the import now fails, lean on the
  documented `Context` / `command_group` factories or open an
  issue describing the use case.

### `rich-argparse` is no longer pulled in

- **What changed:** `rich-argparse` was only used by the deleted
  `_parser.py`. It has been dropped from `toolr-py`'s
  dependencies. Anything that relied on toolr transitively
  bringing it into the tools venv will need to declare it
  explicitly.
- **Migration:** add `rich-argparse` to your own
  `pyproject.toml` if you depend on it for non-toolr code.

## 🚀 New features

- **Rust CLI binary.** `toolr` is now a native binary built from a
  Cargo workspace. Manifest discovery, argument parsing, help
  rendering, and command dispatch all run in Rust. Python is only
  involved at execution time (per-command subprocess via
  `toolr._runner`), so cold-start latency drops dramatically and
  shell completion is no longer gated on Python import overhead.
- **Three install channels, one source of truth.** The same
  `toolr` binary ships through:
    - `pip install toolr` (a new `py3-none-<plat>` binary wheel),
    - `curl ... | sh` via `installation/install.sh`,
    - mise via `installation/mise/`,
    - GitHub Release archives (`toolr-<version>-<target>.tar.gz`,
      with `.sha256` siblings).

  All four are produced from the same workspace build and share a
  single version number.
- **`toolr-py` PyPI package.** A standalone wheel providing
  `import toolr` for user tool scripts — declared in
  `tools/pyproject.toml`, materialised into the tools venv by
  `uv sync`. Decouples "what CLI you have on PATH" from "what
  Python bindings your tool scripts pin."
- **Python 3.14 support.** Added to the test matrix and the
  `toolr-py` classifier list.
- **Per-project `tools/` venv with uv.** The Rust binary
  materialises (and, if needed, bootstraps) a `tools/` venv via
  `uv` before each execute. Includes missing-dependency
  diagnostics, manifest caching, and cache pruning. See the
  rebuilt [installation /
  usage](https://toolr.readthedocs.io/en/latest/) docs for the
  end-to-end story.
- **Native shell completion.** Generated by the Rust frontend
  (clap-based), available for the usual shells.
- **In-repo mise plugin smoke test.** The plugin lives at
  `installation/mise/` and is covered by the same end-to-end
  smoke harness as the other install channels.
- **SLSA build provenance on every shipped artifact.** Every
  wheel (`toolr-*.whl`, `toolr_py-*.whl`), sdist, per-triple
  binary archive (`toolr-<version>-<triple>.tar.gz` /
  `.zip`), and the release notes / patch files carry a
  cryptographically signed attestation generated by
  `actions/attest-build-provenance`. Verify any of them with:

  ```sh
  gh attestation verify <file> --owner s0undt3ch
  ```

  `install.sh` already passes `--verify-attestation=require` to
  reject any archive whose attestation does not validate. See
  GitHub's [artifact attestations
  docs](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations-to-establish-provenance-for-builds)
  for the full verification model.

## 🗺 Migration cheat-sheet

| If you used... | Replace with... |
|---|---|
| `python -m toolr ...` | `toolr ...` (install the CLI via pip / install.sh / mise / release archive) |
| `pip install toolr` to get `import toolr` | `pip install toolr-py` (or run `toolr project init` to scaffold `tools/pyproject.toml` + `tools/uv.lock` with `toolr-py` declared) |
| `mise plugin add toolr https://github.com/s0undt3ch/mise-toolr` | `mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise` |
| `from toolr._registry import command_group` | `from toolr import command_group` |
| `from toolr._registry import CommandGroup` | `from toolr import CommandGroup` |
| `from toolr._parser import Parser` (and friends) | No replacement — the Rust binary owns parsing now |
| Relying on `rich-argparse` via toolr | Depend on `rich-argparse` directly |

## 🧱 Internal — for contributors

These changes affect anyone hacking on toolr itself but are
invisible to end users:

- **Cargo workspace split.** Three crates under `crates/`:
    - `toolr-core` — private library (no pyo3, no clap). Manifest
      discovery, AST parsing, manifest cache, venv plumbing.
    - `toolr` — the binary crate (`bindings = "bin"`). Depends on
      `toolr-core` plus `clap` and `termimad`.
    - `toolr-py` — the pyo3 dynlib + Python source
      (`bindings = "pyo3"`, `module-name = "toolr.utils._rust_utils"`).
- **Python source location.** Moved from `python/toolr/` to
  `crates/toolr-py/python/toolr/`. The repo-root `python/`
  directory is gone.
- **Two PyPI wheels, one workspace version.** `toolr` and
  `toolr-py` are released together at the same
  `[workspace.package] version`. Both have their own
  `pyproject.toml` and `cibuildwheel` matrices; CI fans the
  wheel builds out per crate and reassembles them at release.
- **Root `pyproject.toml` is dev-tooling only.** It retains
  `[tool.ruff]`, `[tool.mypy]`, `[tool.pytest.ini_options]`,
  `[tool.uv]`, `[tool.uv.workspace]`, `[dependency-groups]` —
  no `[build-system]`, no `[project]`, no `[tool.maturin]`.
- **No `python` feature flag.** The
  `[features] python = ["pyo3"]` dance and the
  `#[cfg(feature = "python")]` annotations are gone. pyo3 lives
  exclusively in `crates/toolr-py/` as a non-optional
  dependency.
- **`tools/version.py` simplified.** Cargo.toml writes go
  through `cargo set-version` (via cargo-edit) instead of
  hand-rolled regex edits.
- **`rich` is a direct `toolr-py` dependency.** Previously
  transitive through `rich-argparse`.
- **`UNRELEASED.md` → release notes pipeline.** The file you're
  reading is now part of every release: `_prepare-release.yml`
  strips this comment header, exports the body as
  `TOOLR_RELEASE_NOTES`, and the cliff template renders it as
  a `### Notes` section in both the GitHub release body and
  `CHANGELOG.md` under the new version's heading.
- **Dogfooding tools venv.** `tools/pyproject.toml` declares
  `toolr-py` as a workspace dependency — the repo's own
  `tools/*.py` scripts run against the same Python runtime
  users get from PyPI.

### Breaking — entry-point plugins removed

The `toolr.commands` entry-point mechanism for registering third-party
plugins is removed. Plugin authors must instead ship a static
`toolr-manifest.json` at the root of their installed Python package.
toolr's dispatch path is now pure Rust and never spawns Python just to
discover commands.

Migrating a plugin:

1. From inside the plugin's repo, run `toolr self build-manifest <pkg>`
   (replace `<pkg>` with the dotted package name). This writes a
   `toolr-manifest.json` next to your package's `__init__.py`.
2. Include the file in your built wheel. For hatchling, add this to
   `pyproject.toml`:

   ```toml
   [tool.hatch.build.targets.wheel]
   include = ["src/<pkg>/toolr-manifest.json"]
   ```

   For setuptools, add `include src/<pkg>/toolr-manifest.json` to
   `MANIFEST.in`.
3. Wire `toolr self build-manifest <pkg> --check` into CI and as a
   pre-commit hook. The `--check` flag exits non-zero when the
   committed `toolr-manifest.json` no longer matches what would be
   generated from current sources.
4. Delete the now-inert `[project.entry-points.'toolr.commands']`
   section from your plugin's `pyproject.toml`.

If you don't ship the file, your plugin's commands will not appear in
`toolr --help` or `toolr <group> --help`.

### Improved — argparse options with underscores accept both spellings

`toolr` normalises the canonical CLI form for argparse-scanned options
to dashes, so `add_argument('--skip_warm_cache', ...)` shows up in
`--help` and shell completion as `--skip-warm-cache`. The original
underscored spelling is now also accepted at parse time, so muscle
memory from the upstream tool (`--skip_warm_cache`) keeps working
without the user having to know about the rewrite.

### Improved — dispatch detects stale manifests automatically

Adding, removing, or editing `tools/*.py` is now reflected on the very
next `toolr <user-cmd>` or `toolr --help` invocation — no
`toolr project manifest rebuild` needed. Installing or upgrading a
third-party plugin that ships its own `toolr-manifest.json` is
similarly picked up automatically. The check is pure Rust and adds
single-digit milliseconds on a warm cache. When a rebuild fails (for
example a syntax error in `tools/foo.py`), toolr serves the cached
manifest with a warning identifying the offending file rather than
blocking dispatch.

### <!-- 0 -->🚀 Features

- *(rust)* Add toolr binary target with placeholder main ([`27d1279`](https://github.com/s0undt3ch/ToolR/commit/27d12794da2a5cdcad3ad9754969906f294a9c63))
- *(rust)* Wire toolr binary to clap for --help and --version ([`bbdbb2b`](https://github.com/s0undt3ch/ToolR/commit/bbdbb2b8e01ac89d49a7e3abfa4647af7d99d5fc))
- *(manifest)* Add manifest data model with serde round-trip tests ([`9b8ecfc`](https://github.com/s0undt3ch/ToolR/commit/9b8ecfc86a95a2e5d498814db68c62af5bd3c471))
- *(manifest)* Add manifest load/write with schema version validation ([`0e6283d`](https://github.com/s0undt3ch/ToolR/commit/0e6283ddd99f3d3fd3ad7b4b3b4db8e15e666b70))
- *(discovery)* Walk upward to locate project root from cwd ([`5108aad`](https://github.com/s0undt3ch/ToolR/commit/5108aad1662e1c9729718500f26934ae134777f9))
- *(hash)* Deterministic hashing over tools/**/*.py contents ([`b7db766`](https://github.com/s0undt3ch/ToolR/commit/b7db766b0947e5201cab07b33bfc9a9386f51849))
- *(parser)* Wire ruff_python_parser for AST access ([`b47a1c1`](https://github.com/s0undt3ch/ToolR/commit/b47a1c176130b2d0d911868f5c57e7857c6ac8a6))
- *(parser)* Extract command_group() calls from module ASTs ([`435ce60`](https://github.com/s0undt3ch/ToolR/commit/435ce60456de562d359218bc8ff74ce8fb2cc509))
- *(parser)* Extract @group.command decorated functions ([`bdb42e9`](https://github.com/s0undt3ch/ToolR/commit/bdb42e9f34d7eb7568fb2b391667d0054c8ee748))
- *(parser)* Extract function signatures into Command arguments ([`41f0cb7`](https://github.com/s0undt3ch/ToolR/commit/41f0cb767febb5b3fbecabab00204ec0da9320df))
- *(parser)* Populate command summary, description, and arg help from docstrings ([`0411a02`](https://github.com/s0undt3ch/ToolR/commit/0411a02184bc9b37a6de511b6e278e86923682d5))
- *(parser)* Extract typing.Literal values into allowed_values ([`14ec3ba`](https://github.com/s0undt3ch/ToolR/commit/14ec3baf27bcfbcbd095996bf0d5d8ccf1218bb4))
- *(parser)* Resolve local enum.Enum members for allowed_values ([`689db73`](https://github.com/s0undt3ch/ToolR/commit/689db73659a3af8d2e549fb71d00045e07c2ac40))
- *(parser)* Build complete static manifest from tools directory ([`cf158c4`](https://github.com/s0undt3ch/ToolR/commit/cf158c49740bf8a5edd6e2f7587deddf154badc0))
- *(cli)* Build subcommand tree from manifest and stub execution ([`bb47f9b`](https://github.com/s0undt3ch/ToolR/commit/bb47f9b4b1d9550677bd03c8f0b530df835f1967))
- *(cli)* Add hidden __build-static-manifest dev command ([`79847b4`](https://github.com/s0undt3ch/ToolR/commit/79847b43073c73be989de54d0a2ee5c7a33e4846))
- *(runner)* Add msgspec schema for toolr._runner spec payload ([`5672c30`](https://github.com/s0undt3ch/ToolR/commit/5672c3073ae5ec114bf989574ea452e8c6b6162d))
- *(runner)* Load + validate spec JSON from TOOLR_SPEC_FILE ([`7b02436`](https://github.com/s0undt3ch/ToolR/commit/7b024362a9e3fa7d9e0ad1fd282cdc9be5829b5a))
- *(runner)* Dispatch into target function via Context ([`ea01f6d`](https://github.com/s0undt3ch/ToolR/commit/ea01f6db928a30562f4d9f975260d7e909ae7943))
- *(execute)* Add ExecutionSpec serde types matching runner schema ([`df94fc4`](https://github.com/s0undt3ch/ToolR/commit/df94fc4c4253344609c3ae11bc3c3573758ed4a5))
- *(execute)* Write spec JSON to private tempfile with drop-cleanup ([`9f1dfa1`](https://github.com/s0undt3ch/ToolR/commit/9f1dfa19301102e1793198010a83b3428f895e33))
- *(execute)* Minimal python interpreter resolver (TOOLR_PYTHON or PATH) ([`a2a45b0`](https://github.com/s0undt3ch/ToolR/commit/a2a45b0a61e7a394b88eff152497b2e50f1426f1))
- *(execute)* Spawn python -m toolr._runner with inherited stdio ([`16dab01`](https://github.com/s0undt3ch/ToolR/commit/16dab01f24d1e1e0421da23ee10620f0e30eea8a))
- *(execute)* Build ExecutionSpec from clap ArgMatches ([`70b9efb`](https://github.com/s0undt3ch/ToolR/commit/70b9efbdf24be7722edf5d450b9c26a78a3feded))
- *(execute)* Wire Python runner into dispatch with exit-code propagation ([`db98c55`](https://github.com/s0undt3ch/ToolR/commit/db98c55060b6cc46e4f23b1110973106ac27644f))
- *(execute)* Forward SIGINT/SIGTERM to Python runner subprocess ([`6541cf9`](https://github.com/s0undt3ch/ToolR/commit/6541cf99ae999176076521587af931d708fe9d6b))
- *(uv)* Add uv module skeleton with discovery types ([`c56cc21`](https://github.com/s0undt3ch/ToolR/commit/c56cc2196da2b64ad6bcbb8a6edd2f846fdcedaa))
- *(uv)* Probe uv --version on PATH and managed paths ([`79cc988`](https://github.com/s0undt3ch/ToolR/commit/79cc9881b57ee820376786a58ac52f81866fa82a))
- *(uv)* Decision logic for consent-based uv install ([`6410d7e`](https://github.com/s0undt3ch/ToolR/commit/6410d7e34c3c2fdc8009adb00b2408967f98b6e9))
- *(uv)* Download and install uv to $XDG_DATA_HOME/toolr/bin ([`d578125`](https://github.com/s0undt3ch/ToolR/commit/d57812594fd3961508d4230863eb5ee3a091632c))
- *(uv)* Unified ensure_uv entrypoint and hidden install command ([`99aed4d`](https://github.com/s0undt3ch/ToolR/commit/99aed4ddc44969a6713ccd8517e0d47c6cf72239))
- *(venv)* Parse [tool.toolr] config from tools/pyproject.toml ([`3fe9fe2`](https://github.com/s0undt3ch/ToolR/commit/3fe9fe20ac5d1ff60cebff8c4340adfdf8b9a279))
- *(venv)* Stable repo-key hash for cache-slot disambiguation ([`9c27607`](https://github.com/s0undt3ch/ToolR/commit/9c276074ee54ab7679839e512e27c5ea3bc5e5c4))
- *(venv)* Resolve cache vs in-tree venv path ([`9a73832`](https://github.com/s0undt3ch/ToolR/commit/9a738322a634994200f6070558cf90b1e4fee280))
- *(venv)* Run uv sync with mtime-based freshness check ([`f9d7e94`](https://github.com/s0undt3ch/ToolR/commit/f9d7e94863d15641f8ce1a6db11902e8eebde839))
- *(venv)* Validate toolr Python package presence in venv ([`8b94b81`](https://github.com/s0undt3ch/ToolR/commit/8b94b812d9667c6e952bebc5fb8c12f28f52dd5e))
- *(venv)* Best-effort editable installs with warn-on-fail ([`e47104c`](https://github.com/s0undt3ch/ToolR/commit/e47104cf5bda2da9d5b8f71be4d5ba96e61e370c))
- *(cli)* Reserve `toolr project` namespace and stub subcommands ([`24aafdb`](https://github.com/s0undt3ch/ToolR/commit/24aafdb60be78b6b718c641ac2875328d8902de5))
- *(project)* Implement `toolr project deps sync` ([`b111f83`](https://github.com/s0undt3ch/ToolR/commit/b111f83d3a21d97691f5488889571f777646e120))
- *(project)* Implement `toolr project venv path` ([`7ce4b36`](https://github.com/s0undt3ch/ToolR/commit/7ce4b3646f86eded9d1b66b3990818980a498630))
- *(project)* Implement `toolr project venv shell` ([`4b3a845`](https://github.com/s0undt3ch/ToolR/commit/4b3a8455dab5da3bee6ee096657ee29eeffa119d))
- *(dispatch)* Spawn the runner via the tools venv python ([`2a000a3`](https://github.com/s0undt3ch/ToolR/commit/2a000a3a83c735b1c2714750b859b4a42dcb78d7))
- *(complete)* Scaffold shell-completion module ([`5badb46`](https://github.com/s0undt3ch/ToolR/commit/5badb4609d8cc969e70748425dd302109348d6c2))
- *(complete)* Prefix-match groups, commands, flags, and allowed values ([`5412dc4`](https://github.com/s0undt3ch/ToolR/commit/5412dc4e31c55132f1ed57a692509d0d9bfae5af))
- *(complete)* Tab-time hash check with cache + fresh-reparse fallback ([`88ee2b3`](https://github.com/s0undt3ch/ToolR/commit/88ee2b301f61a0053f3c2268ff84063d359ad670))
- *(cli)* Add hidden __complete subcommand backing shell scripts ([`b92783e`](https://github.com/s0undt3ch/ToolR/commit/b92783e5b13748d69f066bfc79c79fb0e587dc0f))
- *(complete)* Embed bash completion script with self completion print bash ([`2c31758`](https://github.com/s0undt3ch/ToolR/commit/2c31758efdf7d5ca47b8268389f70cad782720a6))
- *(complete)* Embed zsh completion script ([`4db30ee`](https://github.com/s0undt3ch/ToolR/commit/4db30eefdaf8e2223822825389707c7dc5bf7e34))
- *(complete)* Embed fish completion script ([`f980645`](https://github.com/s0undt3ch/ToolR/commit/f980645ffe1c473b1aa685264cc17200523d4e92))
- *(complete)* Install completion scripts into shell-standard locations ([`c8bbc9f`](https://github.com/s0undt3ch/ToolR/commit/c8bbc9f554e9e4abc1cf1e133fd48931e5d36815))
- *(third_party)* Add fragment model and site-packages glob helper ([`910a73b`](https://github.com/s0undt3ch/ToolR/commit/910a73b7ce1c336fd9a940a9e958c2856c627bf5))
- *(third_party)* Parse + validate manifest fragments with schema-version guard ([`179acd2`](https://github.com/s0undt3ch/ToolR/commit/179acd21467bfa85724579948913d8984a01ba47))
- *(third_party)* Add schema-version migration framework with identity step ([`f5950f9`](https://github.com/s0undt3ch/ToolR/commit/f5950f98334dfc171963a8710a3b598e79651b30))
- *(third_party)* Merge fragments into Manifest with conflict resolution ([`f94f431`](https://github.com/s0undt3ch/ToolR/commit/f94f431ce0e55fdbe600079cc9e0fd7bbc253155))
- *(third_party)* Add discover_and_merge orchestrator with fail-fast on bad fragments ([`89e3fd9`](https://github.com/s0undt3ch/ToolR/commit/89e3fd9699f8c037ddbcb15ee2320d541548ad8b))
- *(parser)* Add build_static_manifest_with_venv merging third-party fragments ([`436ec74`](https://github.com/s0undt3ch/ToolR/commit/436ec74d23a4533c86ca698c4e144d1d8006e6e3))
- *(toolr.build)* Programmatic build_manifest API for third-party packages ([`eb00bc0`](https://github.com/s0undt3ch/ToolR/commit/eb00bc032b948083b19edf2850a6e1f62342c07b))
- *(toolr.build)* Add python -m toolr.build CLI with --check drift detection ([`13604ab`](https://github.com/s0undt3ch/ToolR/commit/13604ab6ed56c6f7a39f10ba202e87774397d2a5))
- *(toolr.build)* Validate fragment schema before writing ([`5348a50`](https://github.com/s0undt3ch/ToolR/commit/5348a50f8d2fff68ad8d273538132afb010a8e45))
- *(cli)* Add toolr self build-manifest Rust wrapper around python -m toolr.build ([`5463e85`](https://github.com/s0undt3ch/ToolR/commit/5463e856b4622bfe3dc5c9e85b6042999cc9a8b1))
- *(dynamic)* Define dynamic-layer payload schema and tests ([`b5cf221`](https://github.com/s0undt3ch/ToolR/commit/b5cf2219385a910b9a58ed68ecd8431ed30eac25))
- *(introspect)* Add dynamic-manifest helper skeleton with empty-payload smoke test ([`f112c3a`](https://github.com/s0undt3ch/ToolR/commit/f112c3a474c0cdcc263379d6ce6643a7e36ce2c5))
- *(introspect)* Walk tools.* and emit registry-derived groups and commands ([`f8ddd35`](https://github.com/s0undt3ch/ToolR/commit/f8ddd353c1d48b14b936c3e51685408ff8d2da15))
- *(introspect)* Load toolr.commands entry points so legacy packages appear in dynamic layer ([`ab81b41`](https://github.com/s0undt3ch/ToolR/commit/ab81b416c7e3bd353ea56c95c2c73995f79f2543))
- *(dynamic)* Compute dynamic-layer hash over venv dist-info names ([`f684346`](https://github.com/s0undt3ch/ToolR/commit/f6843465350c123a6129858ab35c3770f5acc919))
- *(dynamic)* Spawn introspect helper and decode payload with schema check ([`dfb673d`](https://github.com/s0undt3ch/ToolR/commit/dfb673d165fbfaa998b56e87478c000fd1625785))
- *(dynamic)* Merge dynamic payload into static manifest with static-wins policy ([`96fe2f8`](https://github.com/s0undt3ch/ToolR/commit/96fe2f80dc59a82e3e4c3fefdabde31562610701))
- *(dynamic)* Add rebuild_manifest_full and rebuild_dynamic_only orchestration ([`ba62143`](https://github.com/s0undt3ch/ToolR/commit/ba621433ee6348e42710c5a1899d00a76ce150bd))
- *(cli)* Add toolr project manifest rebuild command ([`fb77f69`](https://github.com/s0undt3ch/ToolR/commit/fb77f6980a516ae980e5806d358700c95e634e26))
- *(dispatch)* Auto-rebuild dynamic manifest layer when venv state changes ([`ce239be`](https://github.com/s0undt3ch/ToolR/commit/ce239beb144f119f63ce9641f6739dfcf639ea75))
- *(pre-commit)* Ship toolr-manifest hook config for downstream consumers ([`325c6ac`](https://github.com/s0undt3ch/ToolR/commit/325c6acb929840b9ab50ce370ae94c569f053219))
- *(deps-check)* Add filesystem probe for top-level imports ([`d6f5f7b`](https://github.com/s0undt3ch/ToolR/commit/d6f5f7bb67ab69f0b5c953bd0bbdd74f76726834))
- *(deps-check)* Add pre-flight check_imports with structured error ([`8b80c21`](https://github.com/s0undt3ch/ToolR/commit/8b80c218cdb3b5fe15256f9c402681ec62fe2f4b))
- *(cli)* Run pre-flight missing-deps check before runner spawn ([`f9cccbc`](https://github.com/s0undt3ch/ToolR/commit/f9cccbc737ec723a945b91129502d909f8efe41c))
- *(cli)* Add TOOLR_NO_PREFLIGHT_DEPS escape hatch ([`8968356`](https://github.com/s0undt3ch/ToolR/commit/8968356ac2f5398dc2d97cfbe28bd9ef93ed938e))
- *(deps-check)* Add post-mortem ImportError interceptor ([`12ed37d`](https://github.com/s0undt3ch/ToolR/commit/12ed37d71157016e91a212c696e3ac21af892042))
- *(runner)* Intercept ImportError tracebacks and append sync hint ([`0abbaaa`](https://github.com/s0undt3ch/ToolR/commit/0abbaaacc465bd68c24983f3c0a89c4086505063))
- *(cache)* Add Meta sidecar data model with serde round-trip tests ([`9d156b3`](https://github.com/s0undt3ch/ToolR/commit/9d156b3c6a8cf1291a1609dcba24a3e2ceea86e0))
- *(cache)* Drop meta.json sidecar on venv creation ([`9ee0814`](https://github.com/s0undt3ch/ToolR/commit/9ee08142d38787c33b68ae18acb79c17cfa9e8c6))
- *(cache)* Touch last_used_at on every invocation ([`502b03c`](https://github.com/s0undt3ch/ToolR/commit/502b03c7659de26c9d74f5923d3a53cc2043d571))
- *(cache)* Enumerate cache entries with size accounting ([`cf8cdda`](https://github.com/s0undt3ch/ToolR/commit/cf8cdda54fc529041c7ee6e8edc2e9ef1f2052f5))
- *(cli)* Add toolr self cache list with tabular output ([`f1a5aa6`](https://github.com/s0undt3ch/ToolR/commit/f1a5aa6473c02bee929e8f8ca99432e61ae67089))
- *(cache)* Classify cache entries as keep, orphan, or stale ([`a8c73ee`](https://github.com/s0undt3ch/ToolR/commit/a8c73eebb621de6eab0422fcbf4df24447ada959))
- *(cli)* Add toolr self cache prune with --all and --dry-run ([`9e50b87`](https://github.com/s0undt3ch/ToolR/commit/9e50b877241a92b7f9144855c1b35178eedfb751))
- *(cache)* Emit passive size-hint when cache exceeds thresholds ([`36ebf4e`](https://github.com/s0undt3ch/ToolR/commit/36ebf4e9968fa40d79875420066f8a427e77cdf3))
- *(build)* Pin maturin and add wheel-contents tests ([`a34bd12`](https://github.com/s0undt3ch/ToolR/commit/a34bd12e8643c5bb9c903fa0c281243c7ac607ff))
- *(cli)* Replace python -m toolr with binary-exec deprecation shim ([`31cdb92`](https://github.com/s0undt3ch/ToolR/commit/31cdb92df2bedd59386512f7cfc9be052147f55e))
- *(ci)* Build per-platform Rust binary archives workflow ([`7026d31`](https://github.com/s0undt3ch/ToolR/commit/7026d311305f65787da3b4530d8eeb973da97371))
- *(ci)* Generate release-manifest.json for installer consumption ([`f591157`](https://github.com/s0undt3ch/ToolR/commit/f59115715d53dea6471805d7c8b938ccb068ee68))
- *(install)* Add cross-platform install.sh + install.ps1 scripts ([`ca72140`](https://github.com/s0undt3ch/ToolR/commit/ca721405ab6508047ab2f7765c6bd18326e85ee7))
- *(mise)* Add Rust-binary-fetching mise plugin (staging dir) ([`d163773`](https://github.com/s0undt3ch/ToolR/commit/d1637736d30e08cb4694f00d6a7b642195919ecf))
- *(mise)* Stage mise plugin scripts under dist/mise-plugin/ ([`9af5e82`](https://github.com/s0undt3ch/ToolR/commit/9af5e82234bca4a3135c3b292be102ee6c43cd6d))
- *(cli)* [**breaking**] Remove python -m toolr entry point ([`0f76af7`](https://github.com/s0undt3ch/ToolR/commit/0f76af76905093e32cc4074842d85c7e5ca4b721))
- *(ci)* Always attest archives + verify provenance from install scripts ([`df7c9f3`](https://github.com/s0undt3ch/ToolR/commit/df7c9f3e8928962ca59d1bbb7b7162934466264b))
- *(cli)* Skeleton toolr project init subcommand ([`479c1c7`](https://github.com/s0undt3ch/ToolR/commit/479c1c7b2ae30def8641a078827c256cb3d81f10))
- *(init)* Add scaffold template files and render helpers ([`4b46671`](https://github.com/s0undt3ch/ToolR/commit/4b46671554be40c9f02b49c9df3ed47e8ed049f6))
- *(init)* Fill in example.py with four ctx-exercising commands ([`92966a4`](https://github.com/s0undt3ch/ToolR/commit/92966a4163b8db9476edf9162a226227537d3172))
- *(init)* Scaffold tools/ with atomic file writes and refuse-without-force ([`2c78866`](https://github.com/s0undt3ch/ToolR/commit/2c788662cf1b24868cc65dab3a3c018f3d7b531b))
- *(init)* Auto-run uv sync after scaffolding (skippable via --no-sync) ([`1836631`](https://github.com/s0undt3ch/ToolR/commit/1836631c10ef2507a7eca29e668c6dabbdde2be0))
- *(args)* List[T] / *args support, bool=False flags, hyphenated flags, msgspec coercion ([`325bc00`](https://github.com/s0undt3ch/ToolR/commit/325bc001065835c1cafe6b584c3e0ea309b46e7e))
- *(types)* Toolr.types namespace + rust SupportedType resolver foundation ([`07a3994`](https://github.com/s0undt3ch/ToolR/commit/07a39947b0fe40656d669c61f8d1187ebf48144d))
- *(types)* Clap-side per-type value_parsers + manifest-build type validation ([`7fd99e7`](https://github.com/s0undt3ch/ToolR/commit/7fd99e781b4778e09b0ec66ea9ec16886d57e991))
- *(groups)* Nested command groups (closes #193) ([`5be44dd`](https://github.com/s0undt3ch/ToolR/commit/5be44ddc6a9449c4190d84361fc7f3a0f64a9e52))
- *(complete)* Tab-complete nested groups + their child commands ([`5000a1e`](https://github.com/s0undt3ch/ToolR/commit/5000a1ed70589f438da5e6bcee8ab0ec15a86b25))
- *(types)* Wire heterogeneous tuple[T1, T2, ...] arity into clap ([`9cf6246`](https://github.com/s0undt3ch/ToolR/commit/9cf624655b55a7634fe57f667b2b4cd3d6b9c558))
- *(types)* Arg() Path constraints + msgspec dec_hook for richer types ([`5a53187`](https://github.com/s0undt3ch/ToolR/commit/5a5318793aecb8d5bcdabeb0138c764cd4921477))
- *(types)* Add toolr.types.Version (PEP 440 via pep440_rs / packaging) ([`518416b`](https://github.com/s0undt3ch/ToolR/commit/518416bc1fb504b6a735e881357aaa4f00b49ef7))
- *(types)* Follow module-level type aliases through resolution ([`b70ae97`](https://github.com/s0undt3ch/ToolR/commit/b70ae97f9600aeea53524200a182fdc748bb5bf2))
- *(help)* Render docstring markdown via termimad for --help output ([`355f825`](https://github.com/s0undt3ch/ToolR/commit/355f82586eb8c7336b8397dd4a322f702d16b65e))
- Parent=kwarg nesting + yellow/green help palette ([`1d36f38`](https://github.com/s0undt3ch/ToolR/commit/1d36f38c6af0ac3e5182552bd59f9e4587c0b23c))
- *(parser)* Cross-file group bindings + parent=<var> resolution ([`c75f10e`](https://github.com/s0undt3ch/ToolR/commit/c75f10ebe1233eec8d1fa74877f199c1d90eb65f))
- @command(group="path") string-path attachment + dotted command_group ([`2e31e9f`](https://github.com/s0undt3ch/ToolR/commit/2e31e9f99a235353a4e82ecc5602cc72ae9be7e4))
- ToolrDeprecationWarning on legacy decorators ([`cb81eaa`](https://github.com/s0undt3ch/ToolR/commit/cb81eaabe28f66204a6bdde08fd1080c68bfd698))
- *(arg)* Full arg() metadata + toolr.types.Count (closes #198) ([`eff9c2f`](https://github.com/s0undt3ch/ToolR/commit/eff9c2f6a3df43b916ec199d34c5cfed985b30e7))
- *(cli)* Output Options — timestamps + ctx.run timeout defaults (closes #191) ([`e378635`](https://github.com/s0undt3ch/ToolR/commit/e378635d5ec9ffc386a41b7726729974d1f7b1e2))
- *(retire)* Remove Python CLI frontend (_parser, _registry) ([`7d1c0d4`](https://github.com/s0undt3ch/ToolR/commit/7d1c0d46eb4ffe01836c26c18d3dfab7a1f0fe45))
- *(release)* Wire UNRELEASED.md into cliff via TOOLR_RELEASE_NOTES ([`3a023b2`](https://github.com/s0undt3ch/ToolR/commit/3a023b24ed8fc2e767561fe4b404f1b9e365d179))
- *(manifest)* Add Origin::ThirdParty for glob-merged entries ([`928bede`](https://github.com/s0undt3ch/ToolR/commit/928bede6425c5093efdd4e216005c72560070906))
- *(dynamic)* Hash third-party manifests instead of dist-info ([`7f142c9`](https://github.com/s0undt3ch/ToolR/commit/7f142c9d87cdd2050e30d72c109ce0de03e5e96d))
- *(toolr-py)* [**breaking**] Drop toolr.commands entry-point plugin support ([`28f4cc9`](https://github.com/s0undt3ch/ToolR/commit/28f4cc9ad73f5e849ed32abb5f68f2b7a6589c97))
- *(toolr-core)* Add freshness::compare for shared dispatch/tab logic ([`fa30d18`](https://github.com/s0undt3ch/ToolR/commit/fa30d181ab54d873bb46f3177b570f21225428d4))
- *(toolr)* Add ensure_manifest_fresh bootstrap step ([`3c94f44`](https://github.com/s0undt3ch/ToolR/commit/3c94f44fd5bf4d3f0343c63b6874260ffb6535d5))
- *(toolr)* Refresh manifest before clap parses argv ([`ffe4361`](https://github.com/s0undt3ch/ToolR/commit/ffe43610cd9f91d1eae6cb381832c8d97df80d44))
- *(examples)* Canonical plugin example shipping toolr-manifest.json ([`1984e3d`](https://github.com/s0undt3ch/ToolR/commit/1984e3d8a9115b44faea5a31cdacf439b63d3944))
- *(build_fragment)* Scaffold pure-Rust manifest fragment builder ([`0f61194`](https://github.com/s0undt3ch/ToolR/commit/0f611947525a7a2944ee184a266c221b8066310e))
- *(parser)* Support call-form @group.command and positional description ([`a3c4c3d`](https://github.com/s0undt3ch/ToolR/commit/a3c4c3d348b3e925ceec0d3016498ee00f41ea72))
- *(build_fragment)* Stable JSON serialisation matching Python output ([`07dc407`](https://github.com/s0undt3ch/ToolR/commit/07dc4071b66665baa9437a15738c1db218bf4efa))
- *(parser)* Backfill allowed_values from Literal aliases via resolved_type ([`95d604c`](https://github.com/s0undt3ch/ToolR/commit/95d604cdce380c111f0f2e89d76d8f88b7a4d1a1))
- *(toolr/cli)* Source+package resolution for build-manifest ([`60b60a1`](https://github.com/s0undt3ch/ToolR/commit/60b60a1b91f1d2c1ea6541f8e0c6df474ea057a3))
- *(cli)* Add --source-dir/--package, remove --python from build-manifest ([`35d432c`](https://github.com/s0undt3ch/ToolR/commit/35d432c966c6858e1dc161510d44fe39bd3ba447))
- *(dispatch)* Rewire self build-manifest to pure-Rust path ([`03ce318`](https://github.com/s0undt3ch/ToolR/commit/03ce3183a4e518e94abd50d0f1cb4cbb1ccd0ba3))
- *(toolr-py)* Remove Python toolr.build module ([`9be85fd`](https://github.com/s0undt3ch/ToolR/commit/9be85fdb110db850a23cca146d47682d1ce8dc1d))
- *(dispatch)* Inherit child stderr to preserve TTY; move missing-dep hint into the runner ([`401eb17`](https://github.com/s0undt3ch/ToolR/commit/401eb1734d8213757711deca9d2c1d6a83fadd0c))
- *(argparse)* Accept underscored long-flag spelling as hidden alias ([`6663d39`](https://github.com/s0undt3ch/ToolR/commit/6663d39eaa1716bf0ea3945a6240bc2bcfa5d3ce))
- *(argparse)* `T | None` without default → zero-or-one positional ([`22c7768`](https://github.com/s0undt3ch/ToolR/commit/22c7768e8991d399c8af07936cbdae78292e8b70))
- *(arg)* Validate aliases / conflicts_with / requires kwargs ([`3e28806`](https://github.com/s0undt3ch/ToolR/commit/3e288061c7b937647b2a8ff370cc5c431f56c3a8))

### <!-- 1 -->🐛 Bug Fixes

- *(gitignore)* Anchor MANIFEST pattern to repo root ([`7de3ef9`](https://github.com/s0undt3ch/ToolR/commit/7de3ef952f348db134d34334216c0245fdee4fde))
- *(discovery)* Canonicalize start path and tighten error-payload assertions ([`9f9e1e8`](https://github.com/s0undt3ch/ToolR/commit/9f9e1e8224530af7314219aeada030e7138cf985))
- *(gitignore)* Anchor venv/ pattern to repo root ([`2ff6f30`](https://github.com/s0undt3ch/ToolR/commit/2ff6f3013fc5b034a888b9c9545a639d7b7995f8))
- *(ci)* Resolve Windows compile error and typos CHANGELOG false positive ([`c0f9a02`](https://github.com/s0undt3ch/ToolR/commit/c0f9a020069d3737fef3798e8780795312c994eb))
- *(clippy)* Use sort_by_key with Reverse in self_cache::run_list ([`a60e4df`](https://github.com/s0undt3ch/ToolR/commit/a60e4dffbf585f4ca55da15ea0548c788cbafcee))
- *(parser)* Resolve enum-attribute defaults to their serialised value (#197) ([`8e2889e`](https://github.com/s0undt3ch/ToolR/commit/8e2889eacb17b2a5289c855b3994f9c494a0bb2c))
- *(runner)* Pass dec_hook to *args coercion; document arg() metadata gap ([`448a68a`](https://github.com/s0undt3ch/ToolR/commit/448a68a73873c17e85cf6e082589d61031b7eb6d))
- *(cli)* --help shows summary+description; root-only -d/-q flags ([`6a08334`](https://github.com/s0undt3ch/ToolR/commit/6a083346ec9de7314b0009c7d16e31c15bd64947))
- *(cli)* -h shows full description, matching argparse-era ergonomics ([`4152589`](https://github.com/s0undt3ch/ToolR/commit/4152589d0b47a0a2ad3763284e178496abb5c960))
- *(cli)* Parent listings show summary only; -h on leaves shows the full body ([`b2e6d31`](https://github.com/s0undt3ch/ToolR/commit/b2e6d31c9c11a365edb2fd8afd7029ea53dc5f68))
- *(cli)* Render Output Options help text through termimad ([`83d5604`](https://github.com/s0undt3ch/ToolR/commit/83d5604f70950093ec23dd0b93914f203a8fcb5c))
- *(parser)* Classify `Annotated[bool, arg(...)]` as a Flag ([`8138fd1`](https://github.com/s0undt3ch/ToolR/commit/8138fd19f04e9392091a15f9005078292b06ab58))
- *(ci)* Pin COLUMNS=100 for snippet captures + correct plan-doc links ([`56d3070`](https://github.com/s0undt3ch/ToolR/commit/56d3070a1251162200e1634f41064e0fb87e1ed9))
- *(rust)* Gate unix-only test helpers properly for Windows builds ([`c661e7c`](https://github.com/s0undt3ch/ToolR/commit/c661e7c31b601268b2b5474631071a1fbeca3e92))
- *(rust)* Gate execute::signals test module behind cfg(unix) ([`a7870de`](https://github.com/s0undt3ch/ToolR/commit/a7870debd906f325f27457670e66b02b9c02c3db))
- *(tests)* JSON-encode paths in cache fixture for Windows compatibility ([`0baef9f`](https://github.com/s0undt3ch/ToolR/commit/0baef9fcdf5db07810d07e7fdad2d8be56941abb))
- *(tests)* Canonicalise the in-tree venv-path expected value ([`cb4fa7c`](https://github.com/s0undt3ch/ToolR/commit/cb4fa7cb9bc50494c6dea55abc682a0f8950645f))
- *(tests)* JSON-encode paths in self-cache-prune fixture too ([`b4675ff`](https://github.com/s0undt3ch/ToolR/commit/b4675ff1c9bb26f2e4a6db9897e7017b9bdf262d))
- *(tests)* Skip ancestor-walk tests when host has a tools/ at root ([`bd11a62`](https://github.com/s0undt3ch/ToolR/commit/bd11a620bc0fd0452373f739e10d682198bc34ec))
- *(tests)* Skip silent_failure_when_no_tools_dir_anywhere when host has tools/ ([`2c658a7`](https://github.com/s0undt3ch/ToolR/commit/2c658a7b68e56e2308073ac5a883e6a5468ea83a))
- *(tests)* Serialise execute::python tests that mutate TOOLR_PYTHON ([`ed4d640`](https://github.com/s0undt3ch/ToolR/commit/ed4d640858c4447d1b3bad6abd1feae3d47d2462))
- *(workspace-split)* Stage 7 review fixes ([`68d300a`](https://github.com/s0undt3ch/ToolR/commit/68d300ae97fd12e0a41def8b9da57ccc2f4296b9))
- *(tests)* Patch correct getpass symbol; lift rich<14.3 cap ([`a1a034f`](https://github.com/s0undt3ch/ToolR/commit/a1a034f5c3db99869def3d3e8d254d9d9aa77c4c))
- *(docs)* Update mkdocs.yml paths to crates/toolr-py/python ([`1f6e2dc`](https://github.com/s0undt3ch/ToolR/commit/1f6e2dcd7b30db31318b0c7013bf745ff22d5245))
- *(packaging)* Tac portability + exclude __pycache__ from toolr-py wheel ([`363cb85`](https://github.com/s0undt3ch/ToolR/commit/363cb85e4527c614efaffefe21367b5d8674b44b))
- *(types)* Sync _context.pyi with the .py source (Output Options fields) ([`0ef3972`](https://github.com/s0undt3ch/ToolR/commit/0ef3972fa0fe7f4bcbefcbc9b8d4503800142a69))
- *(types)* Move TypeVar F into TYPE_CHECKING block (ruff TC001) ([`4d58ebf`](https://github.com/s0undt3ch/ToolR/commit/4d58ebffdb737eac593aa72acbebb7dccbade0ae))
- *(version)* Make dev-version cargo-compatible; fix Windows strftime ([`fddb203`](https://github.com/s0undt3ch/ToolR/commit/fddb20325fa0da138c6dc59a6c7d019bda7de5e0))
- *(version)* Realign workspace version with reality; bump patch for dev ([`dc9d459`](https://github.com/s0undt3ch/ToolR/commit/dc9d459b487c911337563cf9da8ad0ee4cb3bbf4))
- *(freshness)* Skip third-party axis when venv_dir is None ([`c3f6146`](https://github.com/s0undt3ch/ToolR/commit/c3f6146d8059422f9d8ecc8f39b98f7d19a7ef7b))
- *(ci)* Declare Rust toolchain for ReadTheDocs maturin build ([`e003c20`](https://github.com/s0undt3ch/ToolR/commit/e003c206795f477a14ff57d7a8c45b8d4f85691b))
- *(ci)* Pin RTD Rust to 1.93+ for ruff workspace deps ([`8019e97`](https://github.com/s0undt3ch/ToolR/commit/8019e972a7f4078d718ad3d178088b266ba9a673))
- *(ci)* Bypass asdf-rust on RTD, install rustup stable directly ([`cd45aef`](https://github.com/s0undt3ch/ToolR/commit/cd45aefe03cd984e771c2778acbe2520695b08bd))
- *(bootstrap)* Merge third-party fragments in rebuild_manifest_full ([`13fc028`](https://github.com/s0undt3ch/ToolR/commit/13fc028cf88a57f80237edb197cce401d4a94a82))
- *(dispatch)* Resolve venv Python on Windows for self build-manifest ([`08c1bdb`](https://github.com/s0undt3ch/ToolR/commit/08c1bdb73a9779ae1300a37ddf3327ed619aebb3))
- *(third-party)* Glob both Lib and lib/python* venv layouts ([`e393c66`](https://github.com/s0undt3ch/ToolR/commit/e393c661920bc6d958a563c5b56716595f9558dd))
- *(parser)* Skip dot-prefixed directories in list_python_files ([`4c7fd88`](https://github.com/s0undt3ch/ToolR/commit/4c7fd883ec294cb8ea0fef8c6a5281eabadaf9ca))
- *(dispatch)* Emit clear error when tools venv python is missing ([`fed5051`](https://github.com/s0undt3ch/ToolR/commit/fed50517c0fb477940e0be5fc5fef0f6b80e218d))
- *(dispatch)* Wire UvError::user_message into production error paths ([`0034e92`](https://github.com/s0undt3ch/ToolR/commit/0034e92b8abec53f9146e0c471dd5d4ed3833ac3))
- *(version)* Drop spurious `= None` default on bump's positional ([`2676213`](https://github.com/s0undt3ch/ToolR/commit/2676213dcffdccfb668bdd7797a65ac9679a90f4))
- *(release)* Pass --new-version as a flag, restore bump's `= None` default ([`27c4c4e`](https://github.com/s0undt3ch/ToolR/commit/27c4c4e3ffbeb66b5c94235e95aec8bd88a9be3b))

### <!-- 10 -->💼 Other

- *(wheels)* Split pyproject.toml; add toolr binary-wheel + toolr-py pyo3-wheel ([`e5d9b8f`](https://github.com/s0undt3ch/ToolR/commit/e5d9b8f8fe03caecb31ccd297f1f2bc9d2a89cde))
- *(tools)* Declare toolr-py as dogfooding-tools dep ([`d4ba5bd`](https://github.com/s0undt3ch/ToolR/commit/d4ba5bde08c2f87181b8fc457847bf27bb484a4c))
- *(tools)* Inherit packaging from toolr-py; widen venv-cache hash ([`89bc57d`](https://github.com/s0undt3ch/ToolR/commit/89bc57d77464b72479411dd5a8375dfacc460ed1))
- Install prebuilt toolr-py wheel instead of rebuilding it ([`393cd25`](https://github.com/s0undt3ch/ToolR/commit/393cd25b4e25a027ebfdc4fea45dffc1836e4dc2))
- Call _test_distribution.yml instead of inlining dist tests ([`9726a8d`](https://github.com/s0undt3ch/ToolR/commit/9726a8da6130473d9dab8c9a35ba9774987e08e5))
- Un-deprecate @group.command, keep parent.command_group on track for 1.0 removal ([`93986dd`](https://github.com/s0undt3ch/ToolR/commit/93986dd11cdce4b75fb598081bb250655a9272fc))
- Exclude superpowers/ subtree from the published site ([`f041644`](https://github.com/s0undt3ch/ToolR/commit/f041644dd119977ca51137bedf4cd337106a25b6))
- Migrate off deprecated codecov/test-results-action ([`9cef749`](https://github.com/s0undt3ch/ToolR/commit/9cef749d02d260a1ad54c0481f6248ecb2376ca1))
- *(deps)* Bump ruff_python_parser from 0.14.0 to 0.15.13 ([`4c167b9`](https://github.com/s0undt3ch/ToolR/commit/4c167b978e358c3a828fe287eaded8ff1795f4fe))
- *(deps)* Bump signal-hook from 0.3.18 to 0.4.4 ([`90e5b95`](https://github.com/s0undt3ch/ToolR/commit/90e5b95840d25daa18362071165f35a5e8dc41a1))
- *(deps)* Bump thiserror from 1.0.69 to 2.0.18 ([`0842727`](https://github.com/s0undt3ch/ToolR/commit/0842727bbdf1b4010985c50ac831e58a8370a38b))
- Upgrade Python and Rust dependencies to latest ([`e2862a0`](https://github.com/s0undt3ch/ToolR/commit/e2862a0e5b7162a99fd45fc13b9b2e6ea581d315))
- Pin ToolR action to a SHA that supports python-path ([`f54a7fe`](https://github.com/s0undt3ch/ToolR/commit/f54a7fee9c66942924ce46e28f2aeeb4e49825e0))
- *(deps-dev)* Bump ruff from 0.15.12 to 0.15.13 ([`7d01305`](https://github.com/s0undt3ch/ToolR/commit/7d01305ad4bd5ad76b1537edda172675ab6c5a19))
- *(deps-dev)* Bump coverage from 7.13.4 to 7.14.0 ([`0e472e3`](https://github.com/s0undt3ch/ToolR/commit/0e472e3d2014666e779fbf449eb6b8424fc7fef1))
- *(deps-dev)* Bump packaging from 26.0 to 26.2 ([`19c60ad`](https://github.com/s0undt3ch/ToolR/commit/19c60ad47b7f77ad69f0159f60bfdb64d7758f6b))
- Bump pre-commit hook revs and drop retired macos-13 runner ([`3969f29`](https://github.com/s0undt3ch/ToolR/commit/3969f2936f38dd2812e35cccd8b561d65f416acd))
- *(deps-dev)* Update mkdocstrings[python] requirement ([`1912ca8`](https://github.com/s0undt3ch/ToolR/commit/1912ca83e0a6735307c9fcf73a4f887c40ea1d28))
- External command sources and dispatchers ([`665fa69`](https://github.com/s0undt3ch/ToolR/commit/665fa69ed1af38a9b9fb4b3bc904d5d9232068fe))
- Refresh uv.lock for packaging>=26.2 in toolr-py ([`5ed62d6`](https://github.com/s0undt3ch/ToolR/commit/5ed62d6663d3338ecaf0eba537e85c874fa36de0))
- Introduce ArgSchema for externally-discovered commands ([`08fbd71`](https://github.com/s0undt3ch/ToolR/commit/08fbd71d0127c1237acb8008a68de4835ec45822))
- Add CommandSchema ([`580045e`](https://github.com/s0undt3ch/ToolR/commit/580045e65431f48367557746e0caf37f63f3c38a))
- Add DispatchCommand ([`533acaf`](https://github.com/s0undt3ch/ToolR/commit/533acaf934fc95fd871a056f2831c33eee52ae0b))
- Implement DispatchCommand.argv reconstruction ([`303a88d`](https://github.com/s0undt3ch/ToolR/commit/303a88d987bd3e120f6a114bc8d6edc75ec8dce9))
- Lock down toolr.sources public surface ([`5be3d57`](https://github.com/s0undt3ch/ToolR/commit/5be3d57ebc9f0150ff93bf240ecfac717c04a2c8))
- Detect dispatcher commands via DispatchCommand annotation ([`aba6995`](https://github.com/s0undt3ch/ToolR/commit/aba6995e64524c67a1ec7720e4cf23c6d4a86c49))
- Add optional dispatched_from field on Command ([`cbf1540`](https://github.com/s0undt3ch/ToolR/commit/cbf1540eb12eb22848bbc9dd84944aca468924cf))
- Scaffold module + parse [tool.toolr.argparse.*] blocks ([`060fac4`](https://github.com/s0undt3ch/ToolR/commit/060fac4c5f49f3208b8d1ac51d3062477da5e2fb))
- AST-extract parser.add_argument calls from a Python source ([`edbe4fe`](https://github.com/s0undt3ch/ToolR/commit/edbe4fefe88f2dba5c4e02f269a75b0d74099c6f))
- Glob-expand scan_paths and scan each file ([`733708b`](https://github.com/s0undt3ch/ToolR/commit/733708bfa696d54569abbfe54940e8c7b85f0aec))
- Apply common_args to each scanned command ([`27dad20`](https://github.com/s0undt3ch/ToolR/commit/27dad20b169a126ceb80f80308752aa95bb218db))
- Validate attachments and detect collisions ([`34b4452`](https://github.com/s0undt3ch/ToolR/commit/34b445273191963791c9c761c7008e3ff9b490f2))
- Graft scanned commands under attached parents ([`f860d6e`](https://github.com/s0undt3ch/ToolR/commit/f860d6eb0f98f4c0a69125a57bcde77fec543633))
- End-to-end run_for_project orchestrator ([`39a63e0`](https://github.com/s0undt3ch/ToolR/commit/39a63e0564dd0f654693158b3b2e5ed18c578674))
- Run the argparse scanner inside build_static_manifest ([`ea60410`](https://github.com/s0undt3ch/ToolR/commit/ea604100d5fcdebc25e69f4391c74081ee92e69f))
- Pack dispatched-child args for runtime injection ([`b4db253`](https://github.com/s0undt3ch/ToolR/commit/b4db253c402f496d486e300b6e87b8b5c20badbb))
- Construct DispatchCommand and invoke parent on dispatched leaves ([`f8713bb`](https://github.com/s0undt3ch/ToolR/commit/f8713bb568491201e44cd951ffd6669a9b41791e))
- Skip DispatchCommand-annotated kwargs in static arg extraction ([`93a5401`](https://github.com/s0undt3ch/ToolR/commit/93a5401d61f8aec09a898e89d8a29dbdcce8991f))
- Handle Annotated[DispatchCommand, ...] and string forward refs ([`e6ace63`](https://github.com/s0undt3ch/ToolR/commit/e6ace6373968f9a9488da354723357029cac7c6c))
- Fill the gaps from Plan A (auto-rebuild + dispatcher-hosts-children) ([`e958a34`](https://github.com/s0undt3ch/ToolR/commit/e958a34bccd27c11f937fe923169f3aabb9403b5))
- Fill the gaps from Plan A (8 tasks across 2 stacks) ([`8209afb`](https://github.com/s0undt3ch/ToolR/commit/8209afbbfc1b42a464f2a64b79e0411f1661b3d3))
- Add should_skip_auto_rebuild argv inspector ([`6ea19dc`](https://github.com/s0undt3ch/ToolR/commit/6ea19dc5a00f1f4cfa28e38615d251d8c68b6502))
- Auto-rebuild manifest when missing for user commands ([`e2cca50`](https://github.com/s0undt3ch/ToolR/commit/e2cca50bfe1d7e4a05ec4832c15f1b7fd7651840))
- Add optional is_dispatcher field on Command ([`31b648c`](https://github.com/s0undt3ch/ToolR/commit/31b648c40d381e4a05998c9427e424332c1ae747))
- Mark dispatcher commands via GraftResult.dispatchers ([`3792fef`](https://github.com/s0undt3ch/ToolR/commit/3792fef6f124d2f05167455e25b7b90461024c19))
- Hoist grafted children into dispatcher subcommands ([`61d8142`](https://github.com/s0undt3ch/ToolR/commit/61d81429a919fb81378e39e0cc6becb15ae7d1f5))
- Widen path lookup to handle 3-segment dispatcher paths ([`a4539a9`](https://github.com/s0undt3ch/ToolR/commit/a4539a97a21ef01bd87599dc16c860b343a6599c))
- Look up grafted children at dispatcher dotted name ([`166413a`](https://github.com/s0undt3ch/ToolR/commit/166413a970d1e74466de1ad2329ec15a74161660))
- Correct Task 6 bucketing to match the working code ([`35d1507`](https://github.com/s0undt3ch/ToolR/commit/35d1507c01f8ca8c018ae396442c3b578462b439))
- Fix rumdl MD007 indent depth in fill-the-gaps plan ([`f830437`](https://github.com/s0undt3ch/ToolR/commit/f830437544d6d532442af986b8f74e11945cd8ed))
- Drop spurious `--` in fish completion script ([`46b652d`](https://github.com/s0undt3ch/ToolR/commit/46b652d662f0938d2100a1c98d3afb28f427c98d))
- Ship completion for built-in `self` and `project` commands ([`1eeb8c2`](https://github.com/s0undt3ch/ToolR/commit/1eeb8c2e9be1938771479feb3732831819ffde51))
- Honour `--force` literally + distinguish install outcomes ([`774a27f`](https://github.com/s0undt3ch/ToolR/commit/774a27f6ed893c3f198718e6fbfa44c759e70099))
- Backfill meta.json on touch when sidecar is missing ([`cc98c18`](https://github.com/s0undt3ch/ToolR/commit/cc98c188271d43b6e640fca1bd3c7c354afaf5a7))
- Additive-merge scaffold — per-file conflict detection and prompts ([`5906b0f`](https://github.com/s0undt3ch/ToolR/commit/5906b0fa39acee0be31a3a1e102ba0221f3fac34))
- Harden scaffold after code review — snapshot rollback, --yes flag, injectable confirm ([`6f373e9`](https://github.com/s0undt3ch/ToolR/commit/6f373e9e4aae0dc9edb42052a733dea80a4b03ca))
- Add build tasks for each package ([`d7cc39a`](https://github.com/s0undt3ch/ToolR/commit/d7cc39adc669e6c1a9319b1dba3249822c8f405b))
- Apply remaining review findings — exit 2 for conflicts, symlink guard, PID .tmp ([`99918a2`](https://github.com/s0undt3ch/ToolR/commit/99918a201bb3b5322d7a915eda1b176c46421e39))
- Fix maturin invocations to use crates/toolr-py/Cargo.toml ([`0c90594`](https://github.com/s0undt3ch/ToolR/commit/0c9059476d8dcefe51ee3da877c3f005c6c7b047))
- Copy build artifacts into dist/ ([`886908c`](https://github.com/s0undt3ch/ToolR/commit/886908c856cd3e8f5b03c7b768686cfc0fd7ef52))
- Add build-clean task to wipe target/ and dist/ ([`68cb2c1`](https://github.com/s0undt3ch/ToolR/commit/68cb2c11118458b5e2ad4b28fde1901151782c1a))
- Extend build-clean to remove .so files, __pycache__, and .egg-info ([`c64476a`](https://github.com/s0undt3ch/ToolR/commit/c64476a4fc5d46c9064f8637b7c0aa5c21b0626c))
- Fix error message to say toolr-py not toolr ([`312a426`](https://github.com/s0undt3ch/ToolR/commit/312a4269a74acddc2902870078a55c3d03f5ac0e))
- Unset VIRTUAL_ENV when invoking uv sync to silence mismatch warning ([`01c0685`](https://github.com/s0undt3ch/ToolR/commit/01c0685b06b8e0c2af667a35a5bc8a552026073d))
- Locate toolr package via .pth files for editable installs ([`233da24`](https://github.com/s0undt3ch/ToolR/commit/233da2481bd830262f67e2bbfb671b14c26d391f))
- Scope build-clean .so removal to crates/ only ([`6129c99`](https://github.com/s0undt3ch/ToolR/commit/6129c999dc0fa1294d2cbef4290d4735398a9054))
- Add develop-toolr-py task for editable installs ([`36b0553`](https://github.com/s0undt3ch/ToolR/commit/36b05539fb0edeea2aa0bbf496724dbf8bf6bd64))
- Add `deps upgrade <pkg>` for bumping a single dependency ([`b9483b7`](https://github.com/s0undt3ch/ToolR/commit/b9483b7cf820bd8c8943a0a153b4e002a54381b6))
- Replace vague schema-mismatch error with copy-pasteable fix ([`fc3c6ee`](https://github.com/s0undt3ch/ToolR/commit/fc3c6ee52cc67e99fe67a0cef316aecff02ae1b3))
- Document bump policy on both sides ([`62f192e`](https://github.com/s0undt3ch/ToolR/commit/62f192e4e1786857d121735afb7cf33b23f9710f))
- Rebuild manifest on `--help` and bare `toolr` ([`f4c7d7a`](https://github.com/s0undt3ch/ToolR/commit/f4c7d7ac97d4a5995ece751bcfbb227a89a2373e))
- Re-export `DispatchCommand` from package root ([`0ee2079`](https://github.com/s0undt3ch/ToolR/commit/0ee2079cba18e594634341c8289c78a4277b5635))
- Skip files with no add_argument calls + warn on intra-source dups ([`25210a2`](https://github.com/s0undt3ch/ToolR/commit/25210a2c2f3dd7fc7ec6c0db2ba292b292e2ba2c))
- Walk through dispatchers so grafted children tab-complete ([`f2390c2`](https://github.com/s0undt3ch/ToolR/commit/f2390c26a5cc6075be3f661122e539d58b448df1))
- Fall back to flags when no positional candidates remain ([`b5c1a17`](https://github.com/s0undt3ch/ToolR/commit/b5c1a17831801784eb9e46059c4538f7b795e264))
- Preserve literal long-flag spelling for dispatch round-trip ([`55b1b2e`](https://github.com/s0undt3ch/ToolR/commit/55b1b2e500a9f61eb1e9b6c6050e0701665e97bb))
- Add similar crate for unified-diff output in --check mode ([`4bbb990`](https://github.com/s0undt3ch/ToolR/commit/4bbb990bdc954eb59416879e0bc5cc8469115006))
- *(bench)* Add task-runner startup-time comparison ([`275e27f`](https://github.com/s0undt3ch/ToolR/commit/275e27f0bd71b3c257314d5723acf36d25b061a9))
- *(deps)* Bump actions-cool/check-user-permission from 2.3.0 to 2.4.0 ([`d9d7960`](https://github.com/s0undt3ch/ToolR/commit/d9d79605ec42861387f10eaa784eb3fd3a5be15a))
- *(deps)* Bump s0undt3ch/ToolR ([`e7e2596`](https://github.com/s0undt3ch/ToolR/commit/e7e2596c99423bbe6ab089361777d07babcf08d9))
- *(deps-dev)* Bump ruff from 0.15.13 to 0.15.14 ([`4831624`](https://github.com/s0undt3ch/ToolR/commit/4831624f4694c2f1f8f5fe494a1167f557c91d54))
- *(deps-dev)* Bump hypothesis from 6.152.8 to 6.152.9 ([`6abb094`](https://github.com/s0undt3ch/ToolR/commit/6abb0941e6b9f918a3086782affcb9c80990bb1e))
- *(deps)* Bump rust dependencies ([`2db3cef`](https://github.com/s0undt3ch/ToolR/commit/2db3cefcd1c5ad7c81e2fc0f37ad9586eb606c03))
- *(deps)* Bump transitive deps to latest compatible versions ([`a532afe`](https://github.com/s0undt3ch/ToolR/commit/a532afe4b82f4641a7089856fd810a50510bfeaf))

### <!-- 2 -->🚜 Refactor

- *(tests)* Convert test_dispatch helpers to pytest factory fixtures ([`8b60d8b`](https://github.com/s0undt3ch/ToolR/commit/8b60d8b437f44143e65829f5d1b4f471fb8a1479))
- *(tests)* Convert test_spec_loader helpers to pytest factory fixtures ([`0856956`](https://github.com/s0undt3ch/ToolR/commit/0856956a1cddf382596ba9113c7a71bd08882c5c))
- *(workspace)* Move toolr-rust-utils crate under crates/toolr-core ([`0e3d91f`](https://github.com/s0undt3ch/ToolR/commit/0e3d91f1d4e5c41de958c46ac4d5cccf4ed6d936))
- *(workspace)* Extract toolr binary into crates/toolr ([`9bd0297`](https://github.com/s0undt3ch/ToolR/commit/9bd0297b9ef7b7ccee27d042630b70586a407d6d))
- *(workspace)* Extract toolr-py pyo3 crate; remove python feature flag ([`00c0463`](https://github.com/s0undt3ch/ToolR/commit/00c04637231566ee4d7b0d19e00e615e165904fe))
- *(workspace)* Move python/toolr/ into crates/toolr-py/python/ ([`533ab81`](https://github.com/s0undt3ch/ToolR/commit/533ab818d9561c81622c74bd94476e9185df62df))
- *(install)* Rename dist/ -> installation/, mise-plugin/ -> mise/ ([`b4f1197`](https://github.com/s0undt3ch/ToolR/commit/b4f119730136405f6c9addffaa781e0e084c4cec))
- *(version)* Delegate Cargo.toml writes to `cargo set-version` ([`18e0e74`](https://github.com/s0undt3ch/ToolR/commit/18e0e740ffbe614ad14a31726d4c7512fc415c1a))
- *(manifest)* Rename dynamic_hash to third_party_hash ([`0ad7df9`](https://github.com/s0undt3ch/ToolR/commit/0ad7df9998a3b267f15f48c44937fd8d1d970460))
- *(dispatch)* Remove execute-time dynamic-layer freshness ([`504075e`](https://github.com/s0undt3ch/ToolR/commit/504075e97655efd10b087fbb57eff2842503691c))
- *(complete)* Use freshness::compare in resolve_manifest_at_tab ([`305f3be`](https://github.com/s0undt3ch/ToolR/commit/305f3bef811c872a65d405a3c9deca9c60b8f7bc))
- *(parser)* Factor list_python_files + module_path_for_prefix for reuse ([`2e998fa`](https://github.com/s0undt3ch/ToolR/commit/2e998fa768ef2a3aae44a549d38a5073a7caae69))
- *(parser)* Extract import-alias tracking into types/imports ([`9c36795`](https://github.com/s0undt3ch/ToolR/commit/9c36795554a6f66489316cf131ffa7016cebb7a8))
- *(parser)* Extract PathConstraints into types/path_constraints ([`b706163`](https://github.com/s0undt3ch/ToolR/commit/b706163383e4c83cfeb84bfd31d64425d86257b9))
- *(parser)* Extract type taxonomy into types/supported ([`57e2001`](https://github.com/s0undt3ch/ToolR/commit/57e2001c488e583e9e1a81cce20243215503c20d))
- *(parser)* Extract small literal extractors into types/literals ([`22020fa`](https://github.com/s0undt3ch/ToolR/commit/22020fa4a517953652cc77290bcf1ed2e5a0397d))
- *(parser)* Extract arg-metadata into types/arg_metadata ([`b92541f`](https://github.com/s0undt3ch/ToolR/commit/b92541f0f2d5c68e1c715abb5e319d4e480b1edc))
- *(parser)* Extract type resolver into types/resolve ([`2c0fd8f`](https://github.com/s0undt3ch/ToolR/commit/2c0fd8f3fe321d861213111749c53d4d573fb2d5))

### <!-- 3 -->📚 Documentation

- *(spec)* Add Rust front-end design ([`1263dec`](https://github.com/s0undt3ch/ToolR/commit/1263dec93f0ef9eec11f46829f95d2c8f176330a))
- *(spec)* Add implementation roadmap ([`ef9eee8`](https://github.com/s0undt3ch/ToolR/commit/ef9eee8f57c44564b0b34b6834e8d85205c4dbdf))
- *(spec)* Reorganize specs/ by topic with sequenced filenames ([`d52994f`](https://github.com/s0undt3ch/ToolR/commit/d52994f25c9e9ae73929d3f8627b25448657a5c6))
- *(spec)* Draft Plan 1 (Rust binary skeleton + static manifest layer) ([`b21b5d3`](https://github.com/s0undt3ch/ToolR/commit/b21b5d3b1c93b4eb34d59c7428c13b5e34d05d71))
- *(roadmap)* Mark Plan 1 as 🔧 In Progress ([`f7728b7`](https://github.com/s0undt3ch/ToolR/commit/f7728b7607a0d1357ddbdadc4b3bc96fe7211f69))
- *(roadmap)* Mark Plan 1 as ✅ Done ([`e08a977`](https://github.com/s0undt3ch/ToolR/commit/e08a9771ae1f1f40f19a0479df0f9fccc95d73aa))
- *(spec)* Draft Plans 2-9 for the Rust front-end rewrite ([`8ec018b`](https://github.com/s0undt3ch/ToolR/commit/8ec018b17af07c7cb48896f1893c08a763a49f24))
- *(roadmap)* Mark Plan 2 as 🔧 In Progress ([`0d1ac8f`](https://github.com/s0undt3ch/ToolR/commit/0d1ac8f3281d25ce0eb924087441265e1d11efac))
- *(spec)* Refactor plan-doc test snippets to use pytest factory fixtures ([`8c513d0`](https://github.com/s0undt3ch/ToolR/commit/8c513d0383f2607b541b14270fc798b104901425))
- *(toolr)* Note toolr._runner module + ship-test ([`7f088b3`](https://github.com/s0undt3ch/ToolR/commit/7f088b3b9cc808eb450ed85ffb31596d45295684))
- *(roadmap)* Mark Plan 2 as ✅ Done ([`c5993fa`](https://github.com/s0undt3ch/ToolR/commit/c5993fa6e493caa0a5f309b4f7e48a4a38d71ee8))
- *(roadmap)* Mark Plan 3 as 🔧 In Progress ([`e1ca4e0`](https://github.com/s0undt3ch/ToolR/commit/e1ca4e059a4d74dd3b32de989dbac00c1582e5fe))
- *(roadmap)* Mark Plan 3 as ✅ Done ([`055cbc0`](https://github.com/s0undt3ch/ToolR/commit/055cbc09d0399080b2ce0734f454a4592bad0530))
- *(roadmap)* Mark Plan 4 as done ([`baf49cd`](https://github.com/s0undt3ch/ToolR/commit/baf49cdf3c33d91229ce2d99ede1f03dea432bcd))
- *(roadmap)* Mark Plan 5 as done ([`93b32e8`](https://github.com/s0undt3ch/ToolR/commit/93b32e85a71a54dc0b72a14f31b107e843fac870))
- *(plans)* Mark all completed steps in plans 4 and 5 as done ([`c3f7af1`](https://github.com/s0undt3ch/ToolR/commit/c3f7af10f4e95374fc8eaf647e5c778fa23a1639))
- *(roadmap)* Mark Plan 6 as done ([`8fe6101`](https://github.com/s0undt3ch/ToolR/commit/8fe61017116334631001c32adae5bf1e37bc8e52))
- *(roadmap)* Mark Plan 7 as done ([`b89c77a`](https://github.com/s0undt3ch/ToolR/commit/b89c77a053795140eebf8fec52d2e29e77bb87ef))
- *(roadmap)* Mark Plan 8 as done ([`ebff6b9`](https://github.com/s0undt3ch/ToolR/commit/ebff6b9f70587370004cc97b99268d05129c1d31))
- *(changelog)* Document Plan 9 distribution + breaking changes ([`fb276c4`](https://github.com/s0undt3ch/ToolR/commit/fb276c47e0d180c07e54ef1495dfac3fd5c1d55c))
- *(readme)* Document new standalone install + pip wheel paths ([`5e3c7f8`](https://github.com/s0undt3ch/ToolR/commit/5e3c7f836ffedab0a3bba3cb50d47a28a8cc3ee6))
- *(roadmap)* Mark Plan 9 as done ([`812de25`](https://github.com/s0undt3ch/ToolR/commit/812de253aae214c0fd014282a493406de7b72d62))
- *(readme)* Document SLSA attestation verification flow ([`7763973`](https://github.com/s0undt3ch/ToolR/commit/77639735de56dbdc83461916467b0743bb8e64fe))
- *(spec)* Brainstorm design for docs overhaul + toolr project init ([`b5e29b2`](https://github.com/s0undt3ch/ToolR/commit/b5e29b2a20c6d1c4181f50a976b5bce61ab1022e))
- *(spec)* Relocate regen-doc-snippets to .pre-commit-hooks/ and link issue #191 ([`c43eec0`](https://github.com/s0undt3ch/ToolR/commit/c43eec07178a5d0e318b67ab3d3e7810a351661e))
- *(plans)* Write Plan 10 (project init) and Plan 11 (docs overhaul) ([`b08e90e`](https://github.com/s0undt3ch/ToolR/commit/b08e90ef34a7482a4e7df7d06035553d0d66b1d8))
- *(roadmap)* Mark Plan 10 as done ([`976e715`](https://github.com/s0undt3ch/ToolR/commit/976e7155a65a8511d7fa0b5f9429f2c095a1efbc))
- Roadmap entry + nav skeleton for the docs overhaul ([`ab9e816`](https://github.com/s0undt3ch/ToolR/commit/ab9e8167b291f85073389bb66d27ab3c7eab85fe))
- *(tooling)* Add regen-doc-snippets script + sample-repo fixture ([`425fcde`](https://github.com/s0undt3ch/ToolR/commit/425fcdee14106b8ff880f11d82f2afdfb9f35ffc))
- Quickstart page (install → init → run) ([`90cb46f`](https://github.com/s0undt3ch/ToolR/commit/90cb46f46cde45296e2b115dbe889f21d5f7e14e))
- Rewrite installation page with new install matrix + SLSA section ([`e4c8332`](https://github.com/s0undt3ch/ToolR/commit/e4c833257a2b2262291d6972637ac4bb00f6cee0))
- Concepts page (orientation tour of toolr's moving pieces) ([`3ae9c2b`](https://github.com/s0undt3ch/ToolR/commit/3ae9c2b3a3d0220b0a93d36171222dd6e7bc6922))
- Writing commands chapter (port old usage + examples, restructure) ([`5a73fae`](https://github.com/s0undt3ch/ToolR/commit/5a73faed105a3c404d1006ce4e560967b0017e2e))
- Project configuration page (tools/pyproject.toml + venv model) ([`591e3da`](https://github.com/s0undt3ch/ToolR/commit/591e3da1dc424b25accff99e74998ff6fb5aade5))
- CLI reference (single page, every subcommand documented uniformly) ([`57f009f`](https://github.com/s0undt3ch/ToolR/commit/57f009f9bdc1c7b0b66c880c2fc0bef34a99a0d9))
- Third-party packages page (static manifest convention + toolr.build) ([`08fd639`](https://github.com/s0undt3ch/ToolR/commit/08fd63925c4c364e34af72f68e69fbecb1083e8f))
- Internals chapter (manifest layers, cache, pre-commit, diagnostics) ([`be3012e`](https://github.com/s0undt3ch/ToolR/commit/be3012eccab2031f58a580f21ac753d940311089))
- *(reference)* Prune private-module pages, keep only the public API surface ([`8fbf2c5`](https://github.com/s0undt3ch/ToolR/commit/8fbf2c50dd38d950b1087f01f89faf66abdef78d))
- Update nav, third-party link, and snippet whitespace handling for the reference rename ([`aac13ab`](https://github.com/s0undt3ch/ToolR/commit/aac13abed7dfcf3fa0ae757fa55854139505d16c))
- Slim home page to a landing + audience-shortcut layout ([`b71fa6d`](https://github.com/s0undt3ch/ToolR/commit/b71fa6d6e5be356d26c7c19b5c77c347bb7363ee))
- *(roadmap)* Mark Plan 11 as done ([`e8cb029`](https://github.com/s0undt3ch/ToolR/commit/e8cb029a9ff325aa7e670fd81818dff57e29da73))
- Rename writing-commands/limitations.md → known-bugs.md ([`62d3282`](https://github.com/s0undt3ch/ToolR/commit/62d3282a26b380dbaad08f7387f1f5edaf2b8c80))
- Supported-types matrix + manifest schema reference ([`158a194`](https://github.com/s0undt3ch/ToolR/commit/158a194b6b47353b2c4b8383377de211e98ce2a6))
- Catch the writing-commands chapter up to the new decorator API ([`61dbb11`](https://github.com/s0undt3ch/ToolR/commit/61dbb119dcaaeb71a2a9babe86418d131999b1da))
- Lead with the string-path API, deprecate legacy in prose ([`b08c273`](https://github.com/s0undt3ch/ToolR/commit/b08c2730ff05accfab68e5d327b7652888aee8d9))
- *(spec)* Add cargo workspace split design ([`8a26dde`](https://github.com/s0undt3ch/ToolR/commit/8a26dde1f6d3b8e7c6f531dc134b31fd3b85f533))
- *(plan)* Add Plan 12 (workspace split) under specs/rust-front-end/ ([`f495f0e`](https://github.com/s0undt3ch/ToolR/commit/f495f0e8db6416a88f895d0037b9db8fefeb14ac))
- *(mise)* Salvage MISE_SUPPORT.md into docs/installation/mise.md ([`24b4a24`](https://github.com/s0undt3ch/ToolR/commit/24b4a242be1ba5ed9244d30158f5dc725b614669))
- *(install)* Clarify the two PyPI packages — toolr (CLI) vs toolr-py (bindings) ([`d07c72d`](https://github.com/s0undt3ch/ToolR/commit/d07c72d4a94f3fca70b51b20739f6cc060003270))
- *(release)* Populate UNRELEASED.md with rearchitecture notes ([`ea7188c`](https://github.com/s0undt3ch/ToolR/commit/ea7188c972803252210834adc68d94621bda70b4))
- *(specs)* Auto-rebuild stale manifest on dispatch, drop entry-point plugins ([`e62577f`](https://github.com/s0undt3ch/ToolR/commit/e62577fbc51a65c0e82c463949c2d0b49ef413ed))
- *(specs)* Add dispatch manifest freshness implementation plan ([`d06c9a0`](https://github.com/s0undt3ch/ToolR/commit/d06c9a06acb7fe5a09389eb748e968cffa540bed))
- *(unreleased)* Note entry-point removal and dispatch freshness ([`15ec310`](https://github.com/s0undt3ch/ToolR/commit/15ec310c648dbfc7ffb2140e6a303ce1aec222f5))
- *(third-party)* Plugin-author guide for shipping toolr-manifest.json ([`2075e49`](https://github.com/s0undt3ch/ToolR/commit/2075e4971cf7f06446dcccff0d9370e79cce5f3d))
- *(specs)* Design for pure-Rust toolr self build-manifest ([`24e74b3`](https://github.com/s0undt3ch/ToolR/commit/24e74b32175212220660d21d7fb44d29f0bc5acd))
- *(specs)* Implementation plan for pure-Rust toolr self build-manifest ([`4234929`](https://github.com/s0undt3ch/ToolR/commit/42349293fdcf24c45d11790cd18de6a6d758f189))
- *(third-party)* Drop Python build references; add static-only contract ([`3cd50e5`](https://github.com/s0undt3ch/ToolR/commit/3cd50e52e8442f027fcff1742336528a22097bdd))
- Clean up stale --python references and refresh CLI help snippets ([`bc46d07`](https://github.com/s0undt3ch/ToolR/commit/bc46d0721150babdda85e32135b8393ce7096226))
- *(reference)* Drop stale build.md link after toolr.build removal ([`908e0c8`](https://github.com/s0undt3ch/ToolR/commit/908e0c80af767db11e5763c663fc3ea73343ec6f))
- *(specs)* Repo presentation pass design ([`b109e51`](https://github.com/s0undt3ch/ToolR/commit/b109e519b51a0f4b1e1c3c52dcc52a2c4adfc1f6))
- *(specs)* Give each install path first-class treatment ([`19a5b36`](https://github.com/s0undt3ch/ToolR/commit/19a5b367a634760ca09a3a34d4451d1afb199868))
- *(specs)* Lead the install section with mise ([`a5a0026`](https://github.com/s0undt3ch/ToolR/commit/a5a0026985803c7e2955f66998fc06b1ac033304))
- *(specs)* Reorder install section to mise, pip, curl|sh, powershell, gh ([`df8b87d`](https://github.com/s0undt3ch/ToolR/commit/df8b87d463a735043562b6811867e6ca34d2008b))
- *(specs)* Repo presentation pass implementation plan ([`8f47ab5`](https://github.com/s0undt3ch/ToolR/commit/8f47ab52fe55a694a4dc7682bed9cfc30b5487e3))
- *(writing-commands)* Scope the deprecation admonition to the subgroup method form ([`7adc1b4`](https://github.com/s0undt3ch/ToolR/commit/7adc1b4d7bdc53678346cedf504982bdfa3debbb))
- *(writing-commands)* Use dotted-string form in the nested-groups bullet ([`f177125`](https://github.com/s0undt3ch/ToolR/commit/f177125f565ca353fa972f6069296cb4db06fbc5))
- *(internals)* Correct the third_party_hash File-shape bullet ([`43831d4`](https://github.com/s0undt3ch/ToolR/commit/43831d4e3378a2464c0c00461d2f0e2f54b5c65e))
- *(internals)* Drop entry-points discovery from the dynamic layer ([`f67d888`](https://github.com/s0undt3ch/ToolR/commit/f67d88829418687aed74cc59d340e31394fb4665))
- *(internals)* Align the hashing-details third_party_hash entry ([`ff659cd`](https://github.com/s0undt3ch/ToolR/commit/ff659cdd07e86ddea122964175db1b879e831a72))
- *(dynamic)* Remove stale entry-points reference in DynamicPayload ([`c7dc16b`](https://github.com/s0undt3ch/ToolR/commit/c7dc16b2859a97c9133dd8130b9d8c35706d6cfc))
- *(urls)* Point at toolr.readthedocs.io/latest for published docs ([`233f7ad`](https://github.com/s0undt3ch/ToolR/commit/233f7ad232a19d1c8a6c2945099750e882e34bb2))
- *(readme)* Rewrite around the Rust front-end value-prop ([`652d1c7`](https://github.com/s0undt3ch/ToolR/commit/652d1c79ae766cca2c83feef9c8a72b516daad6a))
- CONTRIBUTING rewrite + point README links at toolr.readthedocs.io/latest ([`3f707a7`](https://github.com/s0undt3ch/ToolR/commit/3f707a7993bd9c6967fbc6a8079445055d6461f6))
- *(specs)* Document the live-vs-archive split ([`005f2ed`](https://github.com/s0undt3ch/ToolR/commit/005f2ede009c5350a65853b33ae3bdae153de580))
- *(specs)* Codebase audit snapshot (2026-05-23) ([`018ab63`](https://github.com/s0undt3ch/ToolR/commit/018ab63a892a42dbd489942630bf783d29c1230e))
- *(writing-commands)* Document toolr.testing.CommandsTester ([`e6b8362`](https://github.com/s0undt3ch/ToolR/commit/e6b836260c9af3f00030413a689b349115cbbb12))
- *(installation)* Align quickstart + install index with README ([`11d83c6`](https://github.com/s0undt3ch/ToolR/commit/11d83c67223ad8e061f72661ab9d0defa3d7fe95))

### <!-- 6 -->🧪 Testing

- *(project)* Integration tests for venv path resolution ([`8fc400f`](https://github.com/s0undt3ch/ToolR/commit/8fc400fb5508c8f5bdaf6f9d2e1529dd10b184c3))
- *(project)* End-to-end deps-sync + execute smoke (ignored by default) ([`e5ac8ff`](https://github.com/s0undt3ch/ToolR/commit/e5ac8ff3711a0bc2d10fd8fe5a1a7506c88e6fff))
- *(complete)* End-to-end smoke tests against the real toolr binary ([`ac7648c`](https://github.com/s0undt3ch/ToolR/commit/ac7648c7654139cb140ea59bd3de5d4c45dd0a0e))
- *(third_party)* Round-trip Python build_manifest into Rust-readable fragment ([`bf35cc8`](https://github.com/s0undt3ch/ToolR/commit/bf35cc86461d06d6b1457f5055520e11de8ac48e))
- *(dynamic)* End-to-end static-plus-dynamic merge integration test ([`68f7527`](https://github.com/s0undt3ch/ToolR/commit/68f75274d8ec68c61bd4597eab2e55252344dd9a))
- *(cli)* Cover pre-flight vs post-mortem split end-to-end ([`30947b3`](https://github.com/s0undt3ch/ToolR/commit/30947b38fa6bf7308a9e06b20d249d51081059ae))
- *(cache)* End-to-end fixture run of list, prune, prune --all ([`2771a60`](https://github.com/s0undt3ch/ToolR/commit/2771a604713b590c0c642e813adf0b5e32693731))
- *(build)* Add editable-install test (xfail: maturin pyo3 limitation) ([`5a2ef47`](https://github.com/s0undt3ch/ToolR/commit/5a2ef475170a1154d2106367c8e536350cf2a464))
- *(compat)* Add end-to-end command_group compat test (xfail: maturin limitation) ([`81ba0d6`](https://github.com/s0undt3ch/ToolR/commit/81ba0d63ee48b022e587d53e3daf9a6488993393))
- *(init)* End-to-end integration tests for toolr project init ([`a90c788`](https://github.com/s0undt3ch/ToolR/commit/a90c788f913b5955ee1577c48ea9ed592eae86ac))
- *(retire)* Three-way prune of Python frontend tests ([`0d07fd1`](https://github.com/s0undt3ch/ToolR/commit/0d07fd10e0a78a373b3bdf91a5c94881c7bdf90f))
- *(distribution)* Assert wheel shapes and cross-wheel install path ([`b868709`](https://github.com/s0undt3ch/ToolR/commit/b86870908263f03836ebe4fb91bf69aa214d2c37))
- *(distribution)* Add maturin to dev deps so distribution tests run ([`9061022`](https://github.com/s0undt3ch/ToolR/commit/90610224a3639f03012b0ee16d812faa6aa05a84))
- *(distribution)* Consume pre-built wheels in a dedicated CI job ([`7dcb5c1`](https://github.com/s0undt3ch/ToolR/commit/7dcb5c1b42955a9731996ae2969c66b14369b495))
- Update pyo3 TypeError message for pyo3 0.28 ([`b4c8ea3`](https://github.com/s0undt3ch/ToolR/commit/b4c8ea38899b39ff4b4419c7f634993927fd2e19))
- E2e happy path + dispatch fix for parent lookup ([`3d8acb8`](https://github.com/s0undt3ch/ToolR/commit/3d8acb8ee08283435981a3da6a83ec18c8e238c3))
- E2e multi-attach + collision detection (auto-rebuild deferred) ([`ebf4163`](https://github.com/s0undt3ch/ToolR/commit/ebf4163835dadb645c37fa972700827e0664a0b4))
- Unskip auto-rebuild E2E now that bootstrap is wired in ([`d913abf`](https://github.com/s0undt3ch/ToolR/commit/d913abf3136d54a0d09a593f3401099ba426eeaa))
- E2e dispatcher outer flags reachable via grafted child path ([`5bfd5ec`](https://github.com/s0undt3ch/ToolR/commit/5bfd5ec956b36aa4c3eb44035f2edb38f9393eae))
- Update spec-loader assertion to match new schema-mismatch error ([`ed1eb53`](https://github.com/s0undt3ch/ToolR/commit/ed1eb537ea4d8ed690f43d541c03366d2db5dcbf))
- *(toolr)* Regression — added tools/*.py is detected on dispatch ([`a896b2c`](https://github.com/s0undt3ch/ToolR/commit/a896b2c35be5c3825367bba7163c783b6bdda704))
- *(toolr)* Syntax error in tools/ soft-fails with cached fallback ([`ad9faf0`](https://github.com/s0undt3ch/ToolR/commit/ad9faf0179dcb1f3acfd394c1b60a6135b2fd716))
- *(toolr)* Bypass argv (self/project/--version) skips freshness ([`7bce171`](https://github.com/s0undt3ch/ToolR/commit/7bce171d35463186c7f6ac25c799e0243eacbbf3))
- *(toolr)* Tab completion never persists the manifest ([`f76836e`](https://github.com/s0undt3ch/ToolR/commit/f76836ed27ebb4eb1e9bb39211ac89451bab0a31))
- *(distribution)* Pull toolr + toolr-py wheels from wheelhouse ([`3cd75b4`](https://github.com/s0undt3ch/ToolR/commit/3cd75b47692e3f85cde97009d02cb0b41ab13c4c))
- *(distribution)* Make_uv_venv fixture for cross-OS venv setup ([`04d7ff3`](https://github.com/s0undt3ch/ToolR/commit/04d7ff39fe47beba7280cbda7a706dc5860d1ae2))
- *(build_fragment)* Equivalence vs committed example plugin manifest ([`5dd9655`](https://github.com/s0undt3ch/ToolR/commit/5dd9655ad97e4bdf4bcd5a2c4c802c67af21911f))
- *(build_fragment)* Use include_str! for example plugin fixture ([`3761674`](https://github.com/s0undt3ch/ToolR/commit/3761674722f8676f27cea9380d4b077dbc173c06))
- *(build_fragment)* Filtering + edge-case coverage ([`9c2d976`](https://github.com/s0undt3ch/ToolR/commit/9c2d976a3d717b03879ab3624f40f9c0cb505347))
- *(toolr)* Drop obsolete --python error-path test from cli_smoke ([`d0ef6c4`](https://github.com/s0undt3ch/ToolR/commit/d0ef6c4dc97618ca9d08ef64e873d5b4abed13ab))
- *(toolr)* Integration coverage for self build-manifest CLI ([`0bca68d`](https://github.com/s0undt3ch/ToolR/commit/0bca68da84a6ab9167a13fe87e8527498fa0b06e))
- Drop Python build_manifest tests superseded by Rust path ([`2bcf2f8`](https://github.com/s0undt3ch/ToolR/commit/2bcf2f8c0cd807c3ea51ff5334f53542a8c54199))
- *(command)* Resolve python via python3-first lookup, skip if missing ([`90ae2de`](https://github.com/s0undt3ch/ToolR/commit/90ae2ded3be75f615a43225f2c496f51395edce9))
- *(command)* Wall-clock-bound tokio command tests, fix #247 workspace hang ([`da25483`](https://github.com/s0undt3ch/ToolR/commit/da25483cb4153aadc6c3f82a3bd9a4a380196892))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(release)* Update ToolR action versions in workflows ([`ba95d8e`](https://github.com/s0undt3ch/ToolR/commit/ba95d8e3b65bdc329de48eb2357d0c94b5a5d0b5))
- Add nightly install-channel smoke tests ([`32bc64f`](https://github.com/s0undt3ch/ToolR/commit/32bc64f4effca285bea846b52ee9ca231a08b884))
- Wire doc-snippet drift check into pre-commit + CI ([`90afb89`](https://github.com/s0undt3ch/ToolR/commit/90afb89e7be0b17e763aa3f61530de28cb031a67))
- Stop tracking tools/.toolr-manifest.json — it's a cache ([`9a3051e`](https://github.com/s0undt3ch/ToolR/commit/9a3051eca29f315d350860e226c993cc45aa0b79))
- *(pre-commit)* Install uv + sync dev venv for regen-doc-snippets hook ([`8dffbda`](https://github.com/s0undt3ch/ToolR/commit/8dffbda447e898a9c7c6b5c559f93a11c769114c))
- *(release)* Bump version to 0.20.0 ([`43dbdaf`](https://github.com/s0undt3ch/ToolR/commit/43dbdafab9ba291765edb92031f2d843fc8e955a))
- *(mise)* Provision all CI tooling via jdx/mise-action ([`ea23f9c`](https://github.com/s0undt3ch/ToolR/commit/ea23f9cffd09fb94e3b8e48240ca1796038f2a9c))
- *(ci)* Prefix reusable workflow filenames with underscore ([`7cdf234`](https://github.com/s0undt3ch/ToolR/commit/7cdf234f769ea55a53f391ad1f75f38fab505240))
- *(cache)* Add CACHE_SEED with single-bump invalidation ([`eee8674`](https://github.com/s0undt3ch/ToolR/commit/eee867420e98bf1a69f7456db59a6eb738b9e3e7))
- *(workspace-split)* Fan out wheel builds; rewire version bump and smoke ([`e325843`](https://github.com/s0undt3ch/ToolR/commit/e325843894ad81426e83e9f0e8f8e5d66fb32a74))
- *(workspace-split)* Post-split carry-over cleanups ([`e03c6b9`](https://github.com/s0undt3ch/ToolR/commit/e03c6b90a781db1c08c25eac2291e52dbf0b3577))
- *(workspace-split)* Final loose ends — roadmap status, __init__ version lookup, binary-archive wiring ([`cadb37e`](https://github.com/s0undt3ch/ToolR/commit/cadb37eaaada48c59dce70be47e32b83e320a9d7))
- *(coverage)* Exclude toolr-py from cargo tarpaulin ([`07ef1bf`](https://github.com/s0undt3ch/ToolR/commit/07ef1bf02ebe127483a7ae21f9461e844f920521))
- *(release)* Replace external ToolR bootstrap with local workspace ([`31d4472`](https://github.com/s0undt3ch/ToolR/commit/31d447226b3b9c581242a4cc19cab9d08a0ec3ea))
- Enforce Rust/Python schema-version lock-step via integration test ([`d08f948`](https://github.com/s0undt3ch/ToolR/commit/d08f9482f4789d47fa645a6e03c859278f7f3e8b))
- Use --source-dir for the example-plugin manifest sync check ([`bf3f53e`](https://github.com/s0undt3ch/ToolR/commit/bf3f53e69a884f8337311811574d2f533fdb67f2))
- Skip toolr-plugin-example in test-job uv sync ([`a16ca1a`](https://github.com/s0undt3ch/ToolR/commit/a16ca1af3e90db9258a1e09f80bdb064b1fb5a81))
- Force LF line endings for *.json across all checkouts ([`4ebe915`](https://github.com/s0undt3ch/ToolR/commit/4ebe9150dfcb11d31cd031621766631f573e7507))
- Build toolr-plugin-example once and share via wheelhouse ([`09e7e62`](https://github.com/s0undt3ch/ToolR/commit/09e7e62e292c2f569bb4962867d53d97e659cac8))
- Some workflow organization ([`6fb509d`](https://github.com/s0undt3ch/ToolR/commit/6fb509d70eb42c4efdaa7c2882fd5b225ec6de72))
- Show OS display name ([`8460eba`](https://github.com/s0undt3ch/ToolR/commit/8460eba8fd65b74a3ce71b2088129c11722ba363))
- *(specs)* Archive shipped rust-front-end design tree ([`870bde9`](https://github.com/s0undt3ch/ToolR/commit/870bde9b37e4e953538f7af18fe22edf251ef42b))
- *(specs)* Archive the nine top-level shipped design and plan files ([`bb77fb0`](https://github.com/s0undt3ch/ToolR/commit/bb77fb0df762020265dc6bafab36f25fb8712bfe))
- *(specs)* Repair in-tree references to moved spec paths ([`db6185b`](https://github.com/s0undt3ch/ToolR/commit/db6185b10b7f00708f723a59e712fc207287b856))
- *(docs)* Delete the dead .nav.yml ([`26a6025`](https://github.com/s0undt3ch/ToolR/commit/26a6025fcd426ed92966932525fba6ba751eba4f))
- *(tools)* Drop tools/__init__.py to match documented PEP 420 model ([`98e6b0c`](https://github.com/s0undt3ch/ToolR/commit/98e6b0c556b0059ab82a6397860c75ee5152390b))
- *(toolr)* Drop rich-argparse comparison comments ([`ba28980`](https://github.com/s0undt3ch/ToolR/commit/ba28980244b3a572dfc8c87ca45255357e8bbc4a))
- Drop dead test helpers and migration-era regression tests ([`f5ee391`](https://github.com/s0undt3ch/ToolR/commit/f5ee391061c34dec907ce11b30715c75e91fd0db))
- Rewrite Plan N / Task N comments + tighten migration prose ([`b7a4a27`](https://github.com/s0undt3ch/ToolR/commit/b7a4a27d7b008d0fee10064ba9ad60861c286acd))
- *(mise)* Add single `test` task aggregating cargo + pytest ([`f0fc8e1`](https://github.com/s0undt3ch/ToolR/commit/f0fc8e1e3009febfac3be68a732690c1020e6c86))
- *(specs)* Archive the repo-presentation-pass design + plan ([`6e4d966`](https://github.com/s0undt3ch/ToolR/commit/6e4d9667f6db1f484ce368e1098389c56942feae))
- *(specs)* Archive the 2026-05-23 codebase audit ([`a3ab98a`](https://github.com/s0undt3ch/ToolR/commit/a3ab98ac567724e5ce4b2ec4919e3d1f4afeeb22))
- *(release)* Use setup-toolr action for matrix generation, drop the workspace build ([`79b76da`](https://github.com/s0undt3ch/ToolR/commit/79b76da9c9444b9399c089a5ce27904350f9c5bb))
- *(build)* Drop `cross`, build musl natively, run full matrix on main ([`c65dd09`](https://github.com/s0undt3ch/ToolR/commit/c65dd095ad79dd272e2df57657172c77ffece2ea))
- *(matrix)* Label-driven opt-in + write the *why* to job summary ([`4ed277a`](https://github.com/s0undt3ch/ToolR/commit/4ed277aa28c18d72d2d6614e75cf99c3db6e6170))
- *(throttle)* Add a GitHub Action to throttle workflow run builds ([`a8b0a87`](https://github.com/s0undt3ch/ToolR/commit/a8b0a87317576cd8e7d60d5adaed07ecb5782b3f))
- Switch from `Literal` to `StrEnum` ([`046e654`](https://github.com/s0undt3ch/ToolR/commit/046e654c83f8210efd4ba0924997043dae893f25))

### <!-- 8 -->🛡️ Security

- *(deps)* Bump step-security/harden-runner from 2.19.3 to 2.19.4 ([`d403382`](https://github.com/s0undt3ch/ToolR/commit/d4033821b9a816ac5a275ea573e9d7c5c8d54b2c))
## 0.11.1 - 2026-05-13

### <!-- 0 -->🚀 Features

- *(logs)* Include extra keywords in logs output ([`695dc58`](https://github.com/s0undt3ch/ToolR/commit/695dc5899d29e7562a3a90850ff458045891c195))
- *(tests)* Fuzzy testing ([`f34c9cd`](https://github.com/s0undt3ch/ToolR/commit/f34c9cd84307c3c00cc7fa7b82ffde9f4b8cfffe))
- *(security)* Add ``SECURITY.md`` file ([`131b75a`](https://github.com/s0undt3ch/ToolR/commit/131b75ab2eab6aecbad36a0f865a0f011fe8e11b))

### <!-- 1 -->🐛 Bug Fixes

- *(pypi)* We can't have local version parts in PyPi ([`fa3c515`](https://github.com/s0undt3ch/ToolR/commit/fa3c515c5690f611b6485bdf108236ed4c7b119d))
- *(signature)* Return `VarArg` for `*args` (VAR_POSITIONAL) parameters ([`f8a967d`](https://github.com/s0undt3ch/ToolR/commit/f8a967d15a80e53f14329468a5382502165facd9))
- *(signature)* Match `KwArg` before `Arg` in `Signature.__call__` ([`1228344`](https://github.com/s0undt3ch/ToolR/commit/1228344b15b3e33296dfaf7bce7ff0f9afe1d212))
- *(docs)* Bump pymdown-extensions to 10.21.2 for pygments 2.20.0 compat ([`7422193`](https://github.com/s0undt3ch/ToolR/commit/7422193e201fd88cb333db33f3425ffab9d4d527))
- *(docs)* Use descriptive link text in SECURITY.md ([`212454a`](https://github.com/s0undt3ch/ToolR/commit/212454a5b4bd02f2ce609e7d2a2344248351181a))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(release)* Update ToolR action versions in workflows ([`0d11014`](https://github.com/s0undt3ch/ToolR/commit/0d110142417767c317a9b2fa79608ab32abd1d30))
- *(ci)* Switch to prek ([`64c5997`](https://github.com/s0undt3ch/ToolR/commit/64c599784b7bc3639d94beea71150bc92743576f))
- *(pre-commit)* Update pre-commit hook versions ([`6438077`](https://github.com/s0undt3ch/ToolR/commit/64380774c55a509af331809b861c2cdced949832))
- *(ci)* Switch to `macos-15-intel` to build Intel wheels ([`211b0fa`](https://github.com/s0undt3ch/ToolR/commit/211b0faa402a5304648180ae0fa48059d2eb035a))
- *(ci)* Lock permissions on the build.yml workflow ([`aabe4e5`](https://github.com/s0undt3ch/ToolR/commit/aabe4e59aecf0af7e06bd2c27f10cd1bd96286f3))
- *(ci)* Lock `build.yml` GitHub Actions to SHA hashes ([`08bdda3`](https://github.com/s0undt3ch/ToolR/commit/08bdda3f740fef78db3f2b1b549c2bbf425e11a9))
- *(ci)* Only run attestations on the main repo ([`0d2b361`](https://github.com/s0undt3ch/ToolR/commit/0d2b361c20638d696c0554b956fa4fafd662570b))
- *(ci)* Improve `cibuildwheel`` build performance by adding cache ([`60a438f`](https://github.com/s0undt3ch/ToolR/commit/60a438f5c00b406f170ff7d07e4ac9968984c967))
- *(ci)* Fix the chicken & egg issue with releases. ([`77cc4f6`](https://github.com/s0undt3ch/ToolR/commit/77cc4f636cc60b9f2656932ee3a6a2108e3b8c58))
- *(ci)* Remove no longer required process ([`821592f`](https://github.com/s0undt3ch/ToolR/commit/821592f76fae15ea4ba83525a92fd335c5c59685))
- *(ci)* Update but still lock to the SHA ([`57e3719`](https://github.com/s0undt3ch/ToolR/commit/57e37194bf90de7b9228610d3ba5c9677262bbe2))
- *(ci)* When updating our own usage or toolr in GH Actions, lock it ([`b666088`](https://github.com/s0undt3ch/ToolR/commit/b6660881c6ab62fc7d23fd01799fe5eae679ea3c))
- *(ci)* Restrict GH Actions jobs permissions ([`14bfc6d`](https://github.com/s0undt3ch/ToolR/commit/14bfc6d433b4937d0cb00c07198c0bd7f2e6821b))
- *(pre-commit)* Add pre-commit hook to lock GH Actions steps ([`5d69b7a`](https://github.com/s0undt3ch/ToolR/commit/5d69b7a80743681bc392413f2d4f8175d1c5f14c))
- *(ci)* Switch `prepare-release` to a reusable workflow ([`9e6c657`](https://github.com/s0undt3ch/ToolR/commit/9e6c657d8863f829766f588d45092beea0a5ea47))
- *(ci)* Update actions versions ([`2faf97c`](https://github.com/s0undt3ch/ToolR/commit/2faf97c0018d376a3a8bb01910e4ed7e8cecca57))
- *(ci)* Fix auto version bump ([`2a85e6c`](https://github.com/s0undt3ch/ToolR/commit/2a85e6c577351f6c0987952a5735aa0f9b6fe364))
- *(docs)* Add CONTRIBUTING document ([`7691338`](https://github.com/s0undt3ch/ToolR/commit/769133820372966454c2e013fbb65035cb3661c1))
- *(pre-commit)* Swap markdownlint-cli2 for rumdl, add gitleaks ([`a4f6bc5`](https://github.com/s0undt3ch/ToolR/commit/a4f6bc5e58f12b36dc727c49b53565b81d2b3ce3))

### New Contributors

* @step-security-bot made their first contribution
## 0.11.0 - 2025-09-24

### <!-- 0 -->🚀 Features

- *(docstrings)* We now use a rust extension to parse the docstrings ([`a2744f0`](https://github.com/s0undt3ch/ToolR/commit/a2744f0ff3c4b5c086f780d9f0433fc29c3af832))
- *(commands help)* The command's help message is now formatted with Markdown ([`d786915`](https://github.com/s0undt3ch/ToolR/commit/d786915ab9c92726aff05f79c0a079115dd199f9))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(sync-rolling-tags)* Fix `sync-rolling-tags` workflow trigger ([`889e36f`](https://github.com/s0undt3ch/ToolR/commit/889e36fea9025c584ad3d7ea4173ba09aff2beb8))
- *(ci)* Sync'ing rolling tags is now done on demand ([`773745b`](https://github.com/s0undt3ch/ToolR/commit/773745b4156aecee998f8a2e6e494b1a144704e8))
- *(ci)* Fix sync-rolling-tags command ([`59c2955`](https://github.com/s0undt3ch/ToolR/commit/59c29551a6be607748ab97d12dc9958bfa2fdbe4))
- *(ci)* More fixes to the sync-rolling-tags process ([`45ac806`](https://github.com/s0undt3ch/ToolR/commit/45ac80617a69bec0ba2c32b91deec634a98d6eb1))
- *(release)* Update ToolR action versions in workflows ([`6cc4bde`](https://github.com/s0undt3ch/ToolR/commit/6cc4bdecb59d83d95d52f6a908fb5712a888acd0))
- *(ci)* Final sync-rolling-tags fix ([`c48fcbe`](https://github.com/s0undt3ch/ToolR/commit/c48fcbe64867018814992c2712f38c3648b23e53))
- *(ci)* Refresh some caches ([`a91de3d`](https://github.com/s0undt3ch/ToolR/commit/a91de3d2b2ad13887dd6e19a47570b4a2bb80bcf))

## 0.10.1 - 2025-09-19

### <!-- 1 -->🐛 Bug Fixes

- *(parent)* Fix command nesting ([`c223dfc`](https://github.com/s0undt3ch/ToolR/commit/c223dfc88e2981dbd7cd6aed304d219fc3f8f12a))
- *(command)* We now log the `.run()` cmdline at the `INFO` level ([`1311f8d`](https://github.com/s0undt3ch/ToolR/commit/1311f8d817fd71b4aa2b061c48d3e067b8076486))
- *(tests)* Fix `ctx.which` tests to make them less brittle. ([`8a55cf3`](https://github.com/s0undt3ch/ToolR/commit/8a55cf32e5e793472b5059a5a433b3e6a90cfc38))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(release)* Don't update ToolR action usage in workflows ([`fbe4992`](https://github.com/s0undt3ch/ToolR/commit/fbe49927887da44cfbba0e8e4892a672d14837df))
- *(release)* Add workflow that updates ToolR versions in workflows ([`9782974`](https://github.com/s0undt3ch/ToolR/commit/9782974d40ff2d62a183025c9c48dee0d3e92143))

## 0.10.0 - 2025-09-17

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(ci)* Improve CI build matrix reporting ([`777595c`](https://github.com/s0undt3ch/ToolR/commit/777595c0ce4c32fe0a88e249e49fb36bc54206f9))
- *(ci)* Prevent unnecessary branch builds on PRs ([`4d7e911`](https://github.com/s0undt3ch/ToolR/commit/4d7e911fb7448e22fe9f5b961bfe368b3ef50868))
- *(release)* Update all self ToolR actions usage on release ([`38e2a8b`](https://github.com/s0undt3ch/ToolR/commit/38e2a8b92b7c95431cf3ff05e8af50d020b4f11d))
- *(ci)* Consistent workflow toolr output width ([`4475406`](https://github.com/s0undt3ch/ToolR/commit/447540685a36ddae3936ee4c3232474c42f59748))

## 0.9.0 - 2025-09-13

### <!-- 0 -->🚀 Features

- *(cli)* Provide a `Context` class
- *(registry)* Implemented the registry and it's tests
- *(logging)* Add `toolr.utils.logs` to improve logging support
- *(cli)* Provide the package CLI entry point
- *(help)* We now use ``RichHelpFormatter`` to render the help
- *(docstrings)* Parse docstrings to construct help
- *(docs)* Capture each parameter description from docstrings
- *(coverage)* Upload code coverage to codecov
- *(ci)* Upload test results to codecov
- *(signatures)* Add signature parsing
- *(signature)* Handle append action, including weird boolean append.
- *(nargs)* Support ``nargs`` and ``*variable`` in function signatures
- *(docs)* Documentation!
- *(context)* Implemented prompt support in ``Context``.
- *(github-actions)* Allow setting ToolR from a github-action
- *(signature)* Add support for mutually exclusive groups
- *(logging)* Add `setup_logging` function.

### <!-- 1 -->🐛 Bug Fixes

- *(imports)* Handle import errors when searching for tools
- *(descriptions)* Differentiate descriptions
- *(docstring)* Fix dosctring class reference
- *(decorator)* Fix decorator usage.
- *(help)* Parse each decorated command docstring to provide help
- *(log)* Only log the time on specific occasions.
- *(tests)* Fix tests according to latest code changes
- *(tests)* Fix rust tests on windows
- *(scope)* Let the codecov CLI tool find the coverage files
- *(coverage)* Don't track coverage in ``if TYPE_CHECKING:`` code blocks
- *(signature)* `dest` is always set to the name of the positional parameter
- *(tests)* Small refactor to improve testing
- *(signature)* On positional arguments, the name will always be the first alias
- *(enums)* Handle enums by name instead of by value
- *(cli)* Fix early verbose/debug output CLI parsing logic
- *(tests)* Skip problematic windows test
- *(docs)* Include missing docs examples
- *(docs)* Remove `uv run` prefix from examples
- *(command)* Command names from functions auto-naming
- *(SignatureError)* `SignatureError` exceptions now point to command
- *(pypi)* Fix PyPi packaging uploads

### <!-- 2 -->🚜 Refactor

- *(toolr)* Support 3rd-party commands
- *(consoles)* Name context consoles explicitly
- *(3rd-party)* Fix commands and command groups augment/overrides
- *(consoles)* Refactor consoles setup

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(dependencies)* Add `rich=-argparse` as a dependency
- *(command)* Rename `command.run_command` to `command.run`
- *(context)* Make the ``context`` module "private".
- *(ci)* Define allowed concurrency
- *(requiremenst)* We no longer need to maintain separate requirements files
- *(tools)* Clean up the pre-existing tools directory
- *(pre-commit)* Update pre-commit hook versions
- *(lint)* Fix lint issues found with latest pre-commit hooks versions
- *(cibuildweel)* Bump `MACOSX_DEPLOYMENT_TARGET` to `11.0`
- *(cleanup)* Remove `changelog.d/`, it won't be needed anymore
- *(typing)* Make the typing gods happier
- *(msgspec)* Replaced all usages of ``dataclass`` with ``msgspec.Struct``
- *(pre-commit)* Upgrade some pre-commit hooks
- *(pre-commit)* Add ``codespell`` pre-commit hook
- *(parser)* Use a private method to set the parser instead.
- *(discovery)* Actually start discovering tools when running ``toolr``
- *(typing)* Fix typing
- *(samples)* Fix sample cases to respect the required signature
- *(rust)* Address clippy errors
- *(ci)* Define the pre-commit cache to be inside the workspace
- *(ci)* Parallelize package builds
- *(ci)* Use OIDC to authenticate codecov
- *(tests)* Add default pytest flags to config
- *(dependencies)* Add ``pytest-subtests`` to dev dependencies
- *(tests)* Add ``argv`` tests
- *(logs)* Logging utils module testing
- *(tests)* Add test coverage for the `__main__` module
- *(tests)* Improve test coverage of the context object
- *(README.md)* Fix logo file path
- *(mypy)* Have mypy ignore `tests/support/3rd-party-pkg/.*`
- *(tests)* Add test coverage to ``setup_consoles``
- *(pyproject.toml)* Define the 3rd-party test package as editable
- *(ci)* Improved parallelization
- *(pre-commit)* Update hook versions
- *(tests)* Split `tests/test_context.py` into several test modules
- *(docs)* Add ``ruff`` as a docs dependency
- *(ci)* Add and use ``.github/actions/setup-virtualenv``
- *(ci)* Push built packages to test.pypi.org on the default branch
- *(docs)* Fix logo URL in readme
- *(gitignore)* Ignore `*.code-workspace`
- *(pre-commit)* Upgrade pre-commit hook versions
- *(ConsoleVerbosity)* Move `ConsoleVerbosity` to  `toolr.utils._console`
- *(action)* Simplify action
- *(release)* Update the release process
- *(release)* Separate release workflow
- *(security)* Include build provenance attestations
- *(debug)* Set verbose to true when running in debug mode
- *(oackages)* Stop building for `s390x`.
- *(dependabot)* Add `dependabot` configuration
- *(docs)* Add `.readthedocs.yaml` config file
- *(release)* Fix attestations on release workflow
- *(release)* Fix generate build matrix step
- *(changelog)* Add cliff config file
- *(release)* More release workflow fixes
- *(release)* Use the global permissions
- *(release)* Use GH App to push the tags
- *(release)* The action now just configures git with higher privileges
- *(release)* Just repeat, it's simpler in the end
- *(release)* Use `sdist` to build wheels
- *(release)* Prepare for 0.1.1 release
- *(release)* Publish GH release fixes
- *(release)* Prepare for 0.1.2 release
- *(release)* Change release notes filename name
- *(docs)* Update the docs URL to  the right one
- *(release)* Remove the PyPi url
- *(release)* Revert debug release changes
- *(release)* Fix package name to be compliant with PyPi
- *(changelog)* Fix white-space issues around changelog generation
- *(prepare-release)* Run `pre-commit` against the prepare release changes
- *(ci)* Pre-commit needs to be setup and run in a few places

### New Contributors

- @s0undt3ch-gh-actions-automations[bot] made their first contribution
- @dependabot[bot] made their first contribution
