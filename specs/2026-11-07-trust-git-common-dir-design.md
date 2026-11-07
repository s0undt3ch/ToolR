# Repository trust keyed on the git common dir

## Why

toolr refuses to run a repository's code until the user has trusted it once
(`crates/toolr-core/src/trust.rs`). Trust is keyed on the canonical project root:
the directory holding `tools/`. A git worktree has its own project root, so a
worktree of a trusted repository is untrusted. The user is told to run
`toolr project trust` again, or reaches for `TOOLR_TRUST_ALL=1`, which turns the
check off entirely.

Per-worktree trust buys no security. The main checkout can check out any branch,
including an untrusted one, and keeps its trust. A worktree shares the same object
store and refs. Trusting a repository should mean trusting the repository, in
every checkout of it.

## Goals

- Trust is keyed on the repository's git common dir: the directory
  `git rev-parse --git-common-dir` prints, canonicalised.
- Trusting any checkout of a repository (main checkout or any worktree) trusts all
  of them. Worktrees made later are trusted without another prompt.
- Untrusting any checkout of a repository untrusts all of them. There is one
  record per repository, so this falls out of the keying.
- A project outside any git repository keeps today's behaviour: the key is the
  canonical project root.

## Non-goals

- Backwards compatibility. Trust has not been released. Records written by
  unreleased builds need not be read, migrated or recognised. Bump
  `RECORD_SCHEMA_VERSION` if the record shape changes; old records may be ignored
  or listed as unreadable, whichever is simpler.
- Trust across separate repositories. A nested clone, a submodule or a vendored
  repository with its own git dir is a different repository.
- Changing the trust UX beyond what the new key requires: the prompt flow,
  `TOOLR_TRUST_ALL`, `--quiet` behaviour and trust-then-sync stay as they are.

## Anti-goals

These would make the change a failure even if worktrees work:

- **A crafted `.git` borrows another repository's trust.** A directory that is
  not a checkout of a trusted repository must never resolve to that
  repository's key. A downloaded archive that ships a `.git` *file* with
  `gitdir: /home/me/trusted/.git` (or `…/.git/worktrees/x`) must not become
  trusted. See *Resolving the key*.
- **The trust check starts spawning processes or needs `git` installed.** It runs
  on every dispatch and on the bootstrap path.
- **Environment variables move the key.** `GIT_DIR`, `GIT_COMMON_DIR`,
  `GIT_WORK_TREE` and `GIT_CEILING_DIRECTORIES` must not change which record a
  directory resolves to.

## Decided by the user

- The trust key is the git common dir (`git rev-parse --git-common-dir`).
- Trusting a repository trusts all its worktrees.
- Untrusting a repository untrusts everything: the repository and all its
  worktrees.
- No backwards compatibility.
- A git repository holding several toolr projects (several `tools/` roots in
  different subdirectories) is trusted as a whole: trusting one project trusts
  every project in that repository. Today each root is trusted separately.

## Resolving the key

My calls, marked as such. The builder may change the mechanics as long as the
anti-goals hold.

**Read git's files directly; don't run `git`.** (My call.) The result must equal
what `git rev-parse --git-common-dir` prints, canonicalised, for every case below
where git and toolr agree a key is inherited. Tests should compare against real
`git` output where `git` is available.

From the canonical project root, walk up to the nearest ancestor (inclusive) that
contains a `.git` entry. No `.git` anywhere up to the filesystem root means a
non-git project: key = canonical project root.

- **`.git` is a directory** (a main checkout): key = that directory, canonicalised.
  A `commondir` file inside it is ignored; a main checkout's git dir is its own
  common dir. (My call: git would honour a planted `commondir`, but following it
  would let a crafted `.git` directory point at another repository.)
- **`.git` is a file** (`gitdir: <path>`, relative paths resolved against the
  file's directory): let `G` be the gitdir it names.
  - Accept `G` only if it **links back**: `G/gitdir` exists and names this `.git`
    file (compare canonicalised). Git writes that back-link when it creates a
    worktree. An attacker can't write into another repository's git dir, so a
    crafted `.git` file fails this check.
  - If it links back and `G/commondir` exists: key = `G/commondir` resolved
    (relative to `G`), canonicalised. This is the worktree case.
  - Anything else (no back-link, a missing or unreadable `G`, a submodule whose
    gitdir lives in `.git/modules/<name>` and has no `gitdir` back-link file,
    a malformed `.git` file): key = canonical project root. That is today's
    per-path behaviour: the checkout is trusted on its own and borrows nothing.
    (My call: failing closed to per-path trust rather than to an error.)

Bare repositories with worktrees (`git clone --bare` + `git worktree add`) take
the worktree branch: the common dir is the bare repository directory.

## What users see

The key may be a `.git` directory, which is a poor thing to show. Wherever a
path is shown (the trust prompt, the `Untrusted` error, `toolr project trust`,
`toolr project untrust`, `toolr self trusted`), show the **repository path**:

- key ends in `.git` and its parent exists → the parent (the main checkout);
- otherwise (bare repository, non-git project) → the key itself.

(My call.) Commands that print the path to stdout keep printing exactly one path,
so scripts that read it still work.

The record stores enough to answer `toolr self trusted` without re-resolving:
the key and the display path. `toolr self trusted` marks a record `(missing)`, and
`--prune` drops it, when the key no longer exists. A repository whose main
checkout was deleted but whose worktrees survive is therefore still listed and
still trusted, since its common dir still exists.

Prompt and error text should say "repository" where it means the repository and
mention that worktrees are covered, e.g. the prompt line
"Trusting it also trusts its git worktrees." (Wording is the builder's.)

## Constraints

- The store guard (`TrustStore::guard`: the store must not resolve inside the
  repository whose trust it holds) checks against the **key**, and also against
  the project root when the two differ. The `$HOME`-as-root exemption stays as
  it is, applied to the project root.
- `Trusted` proofs cover a key. `Trusted::covers(root)` resolves `root` to its key
  and compares keys, so a proof made in one worktree covers the project in
  another worktree of the same repository. Proofs still only come from this
  module.
- `toolr project init` trusts the directory it scaffolds; it goes through the
  same resolution, so `init` inside a worktree trusts the whole repository.
- Every `trust.rs` entry point (`is_trusted`, `trust`/`grant`, `untrust`,
  `decide`, `is_trusted_now`, `require_trust`) resolves the key the same way.
  One function does the resolution; nothing else canonicalises roots for trust.
- `crates/toolr-core` rules apply: no new dependency unless `optional = true` and
  listed under `full` (see `CONTRIBUTING.md`). Parsing two small text files
  needs none.
- Windows: canonicalised paths there carry the `\\?\` prefix; the `.git` file's
  `gitdir:` uses forward slashes. Compare canonicalised forms on both sides.

## Tests the builder must cover

- Trust the main checkout → a worktree under `.worktrees/` is trusted, and so is
  one created outside the repository.
- Trust from inside a worktree → the main checkout and other worktrees are trusted.
- Untrust from a worktree → the main checkout is untrusted, and vice versa.
- Two toolr projects in one repository share trust.
- A nested independent clone inside a trusted repository is not trusted.
- A submodule of a trusted repository is not trusted by its superproject's record.
- **Crafted `.git` file** pointing at a trusted repository's `.git` or at one of
  its `.git/worktrees/<name>` directories → not trusted.
- **Crafted `.git` directory** with a `commondir` file pointing at a trusted
  repository → not trusted.
- A non-git project keeps exact-path trust.
- Setting `GIT_DIR` / `GIT_COMMON_DIR` doesn't change the outcome.
- The key matches `git rev-parse --git-common-dir` for main checkout, linked
  worktree and bare-repository worktree.
- `toolr self trusted` shows the repository path; `--prune` removes a record whose
  common dir is gone and keeps one whose main checkout alone is gone.

## Docs and release note

- Update the trust sections in `docs/concepts.md`, `docs/cli.md` and
  `docs/quickstart.md`, and regenerate doc snippets
  (`toolr pre-commit regen-doc-snippets`).
- Trust is unreleased, so append to the existing queued note
  (`release-notes/2026-11-01.00-00-12.repository-trust.md`) instead of adding a
  breaking note: trust covers the whole git repository and its worktrees.

## Left to the builder

- Record field names and schema version number.
- Exact prompt, error and help wording.
- Whether resolution lives in `trust.rs` or a small `trust/git.rs` module.
- How to structure the tests between `toolr-core` unit tests and
  `crates/toolr/tests/` integration tests.
