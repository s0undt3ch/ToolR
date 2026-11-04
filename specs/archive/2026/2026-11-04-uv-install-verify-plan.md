# uv install artefact verification: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** The uv auto-install checks the downloaded archive against the `.sha256` sidecar from the
same uv release, and refuses to install on any verification failure.

**Architecture:** `perform_install` delegates to a new `perform_install_from(base, dest)`. That
function resolves the release tag from the `/releases/latest/download/<file>` redirect without
following it. It then fetches `<file>.sha256` and `<file>` from `/releases/download/<tag>/`,
hashes the bytes with `sha2`, and only unpacks on a match.

**Tech Stack:** Rust 2024, `reqwest` blocking (`redirect::Policy::none()`), `sha2`, `std::net`
test server.

**Spec:** `specs/2026-11-04-uv-install-verify-design.md`

## Global Constraints

- Branch `fix/uv-install-verify` off `main`. Worktree `.worktrees/uv-install-verify`.
- Stage files by path. Never `git add -A`. Never stage `audit/`.
- Conventional Commits. No Co-Authored-By trailer. British English.
- Format only the files you touch (`rustfmt --edition 2024 <file>`). Never run `cargo fmt` on a
  whole crate.
- `sha2` is `optional = true` in `crates/toolr-core/Cargo.toml` and listed as `dep:sha2` in `full`.
- No pinned uv version, no checksum table, no bypass env var, no mirror env var.
- Fail closed: no verification failure may leave a file at `dest`.
- 99% patch coverage on changed lines. Use real tests against a local HTTP server, not mocks.

## Review Focus

- The tag comes from one redirect and is reused for both the sidecar and the archive. No second
  request goes to `/latest`.
- Tag validation rejects `..`, `.`, empty, `/`, `%`-encoded and other unexpected characters.
- The hash is computed over the exact bytes that get unpacked, before anything is written to disk.
- The no-redirect client still honours `HTTPS_PROXY` (built with `Client::builder()`, not
  `.no_proxy()`).
- Error text names the asset, the tag and both digests, and `user_message` points to the manual
  install.

---

### Task 1: Multi-route test server

**Files:**

- Modify: `crates/toolr-core/src/uv/install.rs` (test module: replace `serve_once`/`OneShotServer`)

**Interfaces:**

- Produces (test-only):
    - `struct Route { status: u16, headers: Vec<(String, String)>, body: Vec<u8> }`
    - `fn serve(routes: HashMap<String, Route>) -> TestServer` binds `127.0.0.1:0` and serves
      connections until dropped. It answers by request-line path and returns 404 for unknown paths.
      `TestServer { addr: String, hits: Arc<Mutex<Vec<String>>> }` records each requested path.
- [ ] **Step 1:** Implement `serve` and port the three existing `serve_once` callers to a single
      route each.
- [ ] **Step 2:** Run `cargo test -p toolr-core uv::install`. Expected: PASS, unchanged.
- [ ] **Step 3: Commit** with `test(uv): serve several routes from the install test server` and a
      `Release-Notes: skip` trailer.

### Task 2: Tag resolution and sidecar parsing

**Files:**

- Modify: `crates/toolr-core/src/uv/install.rs`
- Modify: `crates/toolr-core/src/uv/mod.rs` (`UvError` variants and `user_message` arms)
- Modify: `crates/toolr-core/Cargo.toml`, `Cargo.toml` (workspace `sha2`), `Cargo.lock`

**Interfaces:**

- Produces:
    - `const UV_RELEASES: &str = "https://github.com/astral-sh/uv";`
    - `fn latest_asset_url(base: &str, file: &str) -> String` and
      `fn tagged_asset_url(base: &str, tag: &str, file: &str) -> String`, which replace
      `asset_url`.
    - `fn resolve_tag(base: &str, file: &str) -> Result<String, UvError>`
    - `fn parse_sidecar(text: &str, file: &str) -> Result<String, UvError>` returns the lowercase
      hex digest.
    - `UvError::Unverified { asset: String, reason: String }` and
      `UvError::ChecksumMismatch { asset: String, tag: String, expected: String, actual: String }`.
- [ ] **Step 1: Write the failing tests.**
    - `parse_sidecar`: accepts `<hex>  <file>`, `<hex> *<file>`, a bare `<hex>` and uppercase hex.
      It rejects an empty string, 63 digits, a non-hex character and another file name.
    - `resolve_tag` against `serve`: a 302 to `/releases/download/v9.9.9/<file>` gives `v9.9.9`.
      An absolute `Location` (`http://<addr>/…`) works too. It returns `Unverified` for a 200, a
      302 with no `Location`, a `Location` with another file name or path shape, and the tags `..`
      and `a%2Fb`.
    - URL builders produce the expected strings. Delete `asset_url_points_at_latest_download`.
    - `user_message` for both new variants contains
      `https://docs.astral.sh/uv/getting-started/installation/` and doesn't start with `toolr:`.
- [ ] **Step 2:** Run `cargo test -p toolr-core uv::`. Expected: FAIL to compile.
- [ ] **Step 3: Implement.**
    - Add `sha2` to the workspace and to `toolr-core` as optional under `full`.
    - Build `resolve_tag` on a client from `reqwest::blocking::Client::builder()` with
      `.redirect(reqwest::redirect::Policy::none())`. Resolve a relative `Location` against the
      request URL with `Url::join`. The path must end in `/releases/download/<tag>/<file>`, and
      `<tag>` must match `[0-9A-Za-z._+-]+` and not be `.` or `..`.
    - Add the error variants with `#[error(...)]` text and `user_message` arms per the spec.
- [ ] **Step 4:** Run `cargo test -p toolr-core uv::`. Expected: PASS.
- [ ] **Step 5: Commit** with `feat(uv): resolve the uv release tag and parse its checksum` and a
      `Release-Notes: skip` trailer (the note lands with Task 3).

### Task 3: Verify before unpacking

**Files:**

- Modify: `crates/toolr-core/src/uv/install.rs`
- Modify: `docs/installation/index.md` (around line 155)
- Create: a release note via `toolr release-notes add`

**Interfaces:**

- Consumes: Task 2's functions and variants, and Task 1's `serve`.
- Produces:
  `pub(crate) fn perform_install_from(base: &str, dest: &Path) -> Result<UvBinary, UvError>`.
  `perform_install()` becomes `managed_uv_path()` plus `perform_install_from(UV_RELEASES, &dest)`.
  `download_and_extract` takes `expected_sha256: &str` and `tag: &str`.
- [ ] **Step 1: Write the failing tests.** Each test builds a fake release with `serve`:
      `/releases/latest/download/<file>` returns a 302, and the tagged archive and sidecar routes
      sit under `/releases/download/v9.9.9/`. Use the host's `host_asset()` file name so
      `perform_install_from` picks it.
    - `#[cfg(unix)]` tar.gz happy path: `dest` is executable. `hits` contains the tagged sidecar and
      archive and never `/releases/latest/download/<file>.sha256`.
    - Happy path through `download_and_extract` with a zip asset and a correct digest.
    - Mismatch: `ChecksumMismatch` whose `expected` and `actual` are the two digests, and `dest`
      doesn't exist.
    - Sidecar 404: `Unverified`, and `dest` doesn't exist.
    - Malformed sidecar: `Unverified`, and `dest` doesn't exist.
    - Unreachable host: rework `perform_install_surfaces_http_failure_for_unreachable_host` to call
      `perform_install_from("http://this-host-cannot-exist.invalid", …)`. Expect `Http`.
    - Update the existing `download_and_extract` tests to pass the payload's real digest.
- [ ] **Step 2:** Run `cargo test -p toolr-core uv::install`. Expected: FAIL.
- [ ] **Step 3: Implement.** In `perform_install_from`:
  1. Resolve the tag.
  2. Fetch the sidecar. A transport failure or non-2xx gives `Unverified`.
  3. Parse the sidecar.
  4. Fetch the archive, with transport failures as `Http`, as today.
  5. Hash the archive with `Sha256::digest` and compare it to the sidecar digest. A mismatch gives
     `ChecksumMismatch`.
  6. Only then write the archive to the temp dir, unpack, copy and chmod.
  7. Finish with `probe(dest, UvSource::FreshlyInstalled)`.
- [ ] **Step 4:** Run `cargo test -p toolr-core uv:: && cargo test -p toolr --test '*'`.
      Expected: PASS.
- [ ] **Step 5: Docs.** After "Toolr downloads prebuilt uv binaries…", add: "Before unpacking,
      toolr checks the download against the SHA-256 checksum published with the same uv release.
      If the checksum is missing or doesn't match, toolr installs nothing and stops with an error.
      Install uv yourself in that case. The checksum catches a corrupted or altered download, not a
      compromised uv release." Run `mkdocs build --strict`.
- [ ] **Step 6: Release note.** Run `toolr release-notes add --text "The uv auto-install now
      checks the downloaded archive against the SHA-256 checksum published with the same uv
      release, and refuses to install it if the checksum is missing or doesn't match."`.
- [ ] **Step 7: Commit** with `fix(uv): verify the auto-installed uv against its release checksum`,
      including the docs and the note file.

### Final: verify and archive

- [ ] Run `mise run test`. Polling every 30–60s, expect it to be green.
- [ ] Manual smoke test: with uv off `PATH` and a temporary `XDG_DATA_HOME`, run
      `cargo run -p toolr -- __install-uv-now` against real GitHub. The install should succeed and
      the installed uv should report the latest version.
- [ ] Run `cargo llvm-cov` and then `cargo xtask patch-coverage --base main`. Expected: at least
      99%.
- [ ] Run `git mv` on both spec files into `specs/archive/2026/`, then commit with
      `docs(specs): archive the uv install verification design and plan`.
