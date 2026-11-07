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
non-git project: key = canonical project root. Let `D` be the directory holding
the `.git` entry, canonicalised, and `E` = `D/.git` **uncanonicalised**.

Every rule below that doesn't name a key falls back to **per-path**: key =
canonical project root. That is today's behaviour: the checkout is trusted on its
own and borrows nothing. (My call: fail closed to per-path trust, not to an error.)

- **The walk stops at `$HOME`.** (User's decision.) If `D` is the canonical home
  directory and the project root is not the home directory itself → per-path. A
  dotfiles repository at `~/.git` therefore never covers a project below `~` that
  has no `.git` of its own: each such project gets its own prompt. A project with
  its own `.git` (`~/foo/.git`) is found first and is unaffected, as is any
  repository whose work tree isn't `$HOME`. `~/tools` itself still resolves to
  `~/.git` normally. This reuses the home directory the store guard already
  resolves, so it adds a path comparison and no filesystem access.

- **Classify `E` without following it** (`symlink_metadata`). A symlinked `.git`
  → per-path. (My call. Archives keep symlinks: `.git -> ~/trusted/.git` would
  otherwise resolve to the trusted key. Git accepts a symlinked `.git`; toolr is
  deliberately stricter.)
- **Ownership (Unix).** `E` not owned by the current user → per-path. (My call,
  the same idea as git's `safe.directory`: another user's `/tmp/.git` or a stale
  worktree path they recreated can't borrow anything.) On Windows, skip this
  check.
- **`E` is a directory** (a main checkout): key = `E`, canonicalised. A
  `commondir` file inside it is ignored; a main checkout's git dir is its own
  common dir. (Following a planted `commondir` would let a crafted `.git`
  directory point at another repository. Git 2.56 refuses such a directory as
  "not a git repository", so ignoring it costs nothing.)
- **`E` is a regular file** (`gitdir: <path>`, relative paths resolved against
  `D`): let `G` be the gitdir it names, canonicalised. The key is `G`'s
  grandparent, canonicalised, only when **both** hold:
  1. `G`'s parent directory is named exactly `worktrees`. Git keeps every linked
     worktree's gitdir in `<common dir>/worktrees/`, and an attacker can't create
     a directory inside another repository's git dir. Without this, an archive
     could ship its own `G` naming a trusted repository. Because of this rule the
     key is `G`'s grandparent directly; `G/commondir` is not read. (Reviewer's
     simplification: same result for every layout tested, one file fewer.)
  2. `G/gitdir` exists and names `D/.git`. Resolve its content **relative to
     `G`** when relative (git ≥ 2.48 writes relative back-links with
     `worktree.useRelativePaths`), then compare its canonicalised parent joined
     with `.git` against `D/.git`; don't canonicalise through `E` itself. Git
     writes this back-link when it creates a worktree; it is the only thing that
     stops a crafted `.git` file naming a live worktree's gitdir, which git
     itself accepts.

  Anything else is per-path. That covers a submodule (its gitdir sits in
  `.git/modules/<name>`, not under `worktrees/`), an unreadable or malformed
  `.git` file, and a crafted `.git` file. A worktree created *inside* a submodule
  has its gitdir under `.git/modules/<name>/worktrees/` and keys on the
  submodule's own git dir, as it should.

  **A worktree moved with plain `mv`** keeps a stale back-link: git still works,
  rule 2 fails, and the user is prompted again. The prompt and the `Untrusted`
  error, when `E` is a `.git` file that failed rule 2 only, hint
  `git worktree repair`.

**Walk details.** The `.git` walk starts at the project root and stops at the
first `.git` entry or at `$HOME`. With no home directory (`dirs::home_dir()` is
`None`) it continues to `/`. A `~/.git` that is itself a file (`$HOME` is a
linked worktree) changes nothing: projects below `~` are per-path, `~/tools`
follows the file rules.

**Residual risk: stale worktree paths.** A worktree deleted with `rm -rf` leaves
its gitdir and back-link until `git worktree prune` (or gc, three months by
default). An archive unpacked at that exact path with a matching `.git` file
passes every rule, as it does for git itself. The attacker needs the exact path;
documented, not engineered around. The ownership check doesn't help here (the
user owns what they unpack); rules 1 and 2 carry the defence against same-user
archives.

Bare repositories with worktrees (`git clone --bare` + `git worktree add`) take
the worktree branch: the common dir is the bare repository directory.

**Known gap: `--separate-git-dir`.** A main checkout created with
`git init --separate-git-dir` (and some dotfile managers) has a `.git` file
pointing at its git dir, with no back-link and no `commondir`. That is exactly the
shape of a crafted `gitdir:` attack, so it gets per-path trust, while its linked
worktrees get key `G`. Trusting one does not cover the other. (My call: rare
layout, and no safe rule tells it apart from the attack.)

**Accepted consequence: the whole work tree is trusted.** Any `tools/` root under
a trusted repository's work tree inherits trust, tracked or not: an unpacked
archive, a gitignored directory, a test fixture. Today each would be prompted for.
Getting files into a trusted work tree already lets someone edit the `tools/*.py`
the user trusts, so this adds no new power. Restricting trust to tracked roots
would mean reading the git index on every dispatch and would break
`toolr project init` before the first commit.

## What users see

The key may be a `.git` directory, which is a poor thing to show. Wherever a
path is shown (the trust prompt, the `Untrusted` error, `toolr project trust`,
`toolr project untrust`, `toolr self trusted`), show the **repository path**:

- key's file name is exactly `.git` → its parent (the main checkout). Not "ends
  in": bare repositories are conventionally `foo.git`, and their parent is just
  the directory holding them;
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
  repository whose trust it holds) checks against the **key**, the **displayed
  repository path** (the main checkout's work tree, see *What users see*) and the
  project root, whichever differ. (My call: from a worktree outside the
  repository, a store inside the main checkout's work tree would otherwise pass.)
  The `$HOME` exemption applies to any of those paths that equals the home
  directory: `~/tools` with a `~/.git` has displayed path `~`, which always holds
  the default store.
- `Trusted` proofs carry **both** the key and the project root.
  `Trusted::covers(root)` resolves `root` to its key and compares keys, so a
  proof made in one worktree covers the project in another worktree of the same
  repository. `Trusted::root()` stays the project root: `sync_after_trust` and
  `ensure_venv_ready` (`crates/toolr-core/src/project.rs:171`, `:205`) use it to
  find the venv. Proofs still only come from this module.
- `toolr self trusted --prune` and anything else working from a stored record
  operate on the stored key directly (e.g. an `untrust_key`), never by
  re-resolving the key as if it were a project root.
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
- **Crafted self-contained worktree**: `.git` file → `./g`, `g/gitdir` naming the
  `.git` file, `g/commondir` naming a trusted repository → not trusted.
- **Symlinked `.git`** to a trusted repository's `.git`, and to one of its
  worktrees' `.git` files → not trusted.
- (Unix) A `.git` owned by another user → per-path.
- A `--separate-git-dir` main checkout gets per-path trust.
- A worktree made with `worktree.useRelativePaths=true` shares the main
  checkout's trust.
- A worktree moved with plain `mv` falls to per-path and the prompt hints
  `git worktree repair`.
- A bare `foo.git` with a worktree: `toolr self trusted` shows `…/foo.git`, not
  its parent.
- A proof made from a worktree syncs that worktree's venv, not the main
  checkout's.
- With a trusted `~/.git` (use a fake `HOME`): a git-less `~/x/tools` is not
  trusted; `~/foo` with its own `.git` is keyed on `~/foo/.git`; `~/tools` is
  keyed on `~/.git` and the store guard doesn't refuse it.
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
- `docs/concepts.md` states the trust unit plainly: the git repository and all
  its worktrees; every separate repository (nested clone, submodule) is asked for
  on its own; a project outside git is trusted by its path. It has a short
  `~/.git` note: a home directory under git (dotfiles) is not treated as the
  repository for projects below it, so trusting `~` never trusts
  `~/Downloads/foo`. It also names the `--separate-git-dir` gap and the
  ownership and symlink rules, one line each.
- Trust is unreleased, so append to the existing queued note
  (`release-notes/2026-11-01.00-00-12.repository-trust.md`) instead of adding a
  breaking note: trust covers the whole git repository and its worktrees.

## Left to the builder

- Record field names and schema version number.
- Exact prompt, error and help wording.
- Whether resolution lives in `trust.rs` or a small `trust/git.rs` module.
- How to structure the tests between `toolr-core` unit tests and
  `crates/toolr/tests/` integration tests.
