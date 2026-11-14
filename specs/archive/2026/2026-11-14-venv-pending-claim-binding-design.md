# Keep the pending venv claim to the run that made it

## Problem

Before uv runs, toolr records a pending claim for an in-tree venv
(`TrustStore::claim_venv`). `pending_build` (`crates/toolr-core/src/venv/sync.rs`) treated any
venv with a pending claim and no marker as toolr's own. Only a successful stamp cleared the
claim, so after a failed first sync it never expired: delete `tools/.venv`, plant a fresh
marker-less directory, and toolr runs it without the "toolr didn't build this venv" question.
Verified in the Fable audit (S-01); a control repo without the claim is refused.

A first attempt bound the claim to the venv directory's `(dev, ino)` once uv exited. Review
found it incomplete:

- uv writes a `.gitignore` holding `*` into the venv, so after a failed first sync a branch can
  commit files into `tools/.venv/` and a checkout writes them into the same directory: same
  identity, no marker, still counted as toolr's, and the next sync runs uv, which runs the
  venv's `bin/python`.
- On Windows the identity is the creation time, which NTFS tunnelling keeps across a fast
  delete and recreate; inode reuse after `rm -rf` was untested too.

The "needs a token" condition (`is_in_tree() && !trust_all_env()`) was also written four times
in the same file (M-06).

## What still needs a claim

The claim's original job, keeping a failed first sync's venv as toolr's in later runs, goes:
a failed first sync now removes what it left. One cross-process job stays. While toolr A
builds a first venv, the directory exists with no marker. toolr B checks it without the lock:
`sync_locked` asks about a foreign venv before taking the lock, and `is_foreign`
(`dispatch/run.rs`, `usable_venv_dir`) and `resync_if_needed` never take it. Without a live
claim B reads A's build as foreign: on a terminal it asks to remove it, without one it fails.
The lock alone can't stop that, so the claim stays, scoped to the lock.

## Design

- A pending claim counts only while some toolr holds that venv's sync lock
  (`TrustStore::venv_locked`, a `try_lock` probe that opens the lock file without creating
  it). flock and `LockFileEx` locks die with their process, so a killed sync's claim covers
  nothing: that venv is foreign and gets the usual question.
- Claims are written only under the lock, so a claim the lock holder finds is stale.
  `lock_venv` ends it before returning, else the holder's own re-check would read it as live
  (the probe sees this process's lock too). If ending it fails, the lock is released and the
  claim stays dead.
- The sync records whether the venv was absent just before claiming (after any approved
  foreign removal). When uv fails, or the stamp after it fails, toolr removes what uv left if
  the venv was absent, is now a real directory and sits directly in a real `tools/`
  (`remove_dir_all` doesn't follow symlinks), then ends the claim, keeping the token, still
  under the lock. A directory that can't be removed stays, unclaimed, and is asked about.
- A failed resync of toolr's own venv removes nothing; its marker still matches.
- `TrustStore::end_claim` ends a claim; an unreadable entry holds none, so only a failed write
  is an error.
- `token_store(resolved)` is the single predicate; the check, lock, claim and stamp use it.
- No identity is stored. Entries written by the first attempt carry `claim_identity`; serde
  ignores the unknown key, and such a claim is lock-bound like any other.

With the claim off identity, `venv_identity` serves only the approval carried over the lock,
whose short-window comment holds again.

Residual: between a lock holder acquiring the lock and ending a stale claim (two file
operations), another toolr's probe could read that stale claim as live.

## Tests

- Finding 1 through the real `sync_if_needed` with a stub uv that creates the venv and its
  `.gitignore` and fails: the venv is removed and the claim ended; files that arrive later
  are foreign. A variant where `tools/` refuses the removal: the directory stays, files
  checked out into it are foreign.
- A claim left by an interrupted sync covers nothing, and the next lock holder ends it.
- A claim counts while its builder holds the lock, and not after.
- A stored entry carrying `claim_identity` loads and covers nothing.
- A failed resync keeps toolr's own venv.
- Trust store: `end_claim`, `venv_locked`, and `lock_venv` ending a stale claim.
