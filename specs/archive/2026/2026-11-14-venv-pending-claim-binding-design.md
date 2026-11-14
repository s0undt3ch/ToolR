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
builds a first venv, the directory exists with no marker. toolr B's sync checks it before
taking the lock, so it can ask about a foreign venv without making other toolrs wait on the
answer. Without the claim, B reads A's build as foreign: on a terminal it asks to remove it,
without one it fails. So the claim stays, but only to tell B's sync to wait for the lock.

A second review showed why the claim must never vouch for the venv. The second design
counted a claim while some toolr held the venv's sync lock, through a `try_lock` probe. Any
holder counted, a toolr that was only probing included, and a SIGKILLed sync leaves its claim
and its lock file behind (on Windows the file always stays). So a killed sync's claim read as
live whenever another toolr probed at that moment. Dispatch took the resulting `Missing` as
"nothing to sync" and ran `tools/.venv/bin/python`, which a hostile checkout can fill. The probe
also opened the lock file read-only, which an flock emulated over NFS byte-range locks may
refuse, so a real build could read as free.

## Design

- Only a marker holding the store's token vouches for an in-tree venv. A pending claim never
  does, under the lock or not, so a marker-less venv always reads `Foreign`. Every caller that
  sees `Foreign` already fails closed: dispatch syncs, which asks or refuses; `venv run
  --no-sync` refuses; `usable_venv_dir` hides the venv's plugins. The `venv_locked` probe is
  gone.
- Before its first check, `sync_if_needed` looks for a pending claim. If there is one, it takes
  the sync lock and drops it straight away: that waits for a live builder, and `lock_venv` ends
  a claim a killed sync left. Then it checks and asks before locking, as before, so no toolr
  waits on a human while holding the lock. A lock that fails there fails again in
  `sync_locked`, which reports it.
- Claims are written only under the lock, so a claim the lock holder finds is stale.
  `lock_venv` ends it before returning, so a later sync doesn't wait for a build that isn't
  running.
- The sync records whether the venv was absent just before claiming (after any approved
  foreign removal). When uv fails, or the stamp after it fails, toolr removes what uv left if
  the venv was absent, is now a real directory and sits directly in a real `tools/`
  (`remove_dir_all` doesn't follow symlinks), then ends the claim, keeping the token, still
  under the lock. A directory that can't be removed stays, unclaimed, and is asked about.
- A failed resync of toolr's own venv removes nothing; its marker still matches.
- `TrustStore::end_claim` ends a claim; an unreadable entry holds none, so only a failed write
  is an error.
- `token_store(resolved)` is the single predicate; the check, lock, claim and stamp use it.
- No identity is stored.

With the claim off identity, `venv_identity` serves only the approval carried over the lock,
whose short-window comment holds again.

Trade-offs, all failing closed, while another toolr's first build runs: dispatch waits for it
through the sync; the venv's plugin commands are left out of the manifest and `--help`; `venv
run --no-sync` and `self build-manifest <package>` call the venv foreign rather than unbuilt.
A new claim taken by a build that starts between the wait and the check can still draw one
removal question; answered `y`, the removal happens only if the venv is still foreign under the
lock.

## Tests

- Finding 1 through the real `sync_if_needed` with a stub uv that creates the venv and its
  `.gitignore` and fails: the venv is removed and the claim ended; files that arrive later
  are foreign. A variant where `tools/` refuses the removal: the directory stays, files
  checked out into it are foreign.
- A claim left by an interrupted sync vouches for nothing, and the next lock holder ends it.
- A claimed, marker-less venv is foreign even while its builder holds the lock; the stamp
  settles the claim.
- A killed sync's claim, its lock file recreated and held by another handle. The
  check reads `Foreign`; through the binary, `toolr hello world` waits for the lock, then
  refuses the venv without running its `bin/python`.
- A sync that finds a claim waits for the builder's lock rather than asking, and finds the venv
  fresh once the builder stamps it.
- A failed resync keeps toolr's own venv.
- Trust store: `end_claim`, and `lock_venv` ending a stale claim.
