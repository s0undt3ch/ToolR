# Bind the pending venv claim to the directory uv left

## Problem

Before uv runs, toolr records a pending claim for an in-tree venv
(`TrustStore::claim_venv`). `pending_build` (`crates/toolr-core/src/venv/sync.rs`) treats any
venv with a pending claim and no marker as toolr's own. Only a successful stamp clears the
claim. After a failed first sync the claim therefore never expires: delete `tools/.venv`, plant
a fresh marker-less directory, and toolr runs it without the "toolr didn't build this venv"
question. Verified in the Fable audit (S-01); a control repo without the claim is refused.

The "needs a token" condition (`is_in_tree() && !trust_all_env()`) is also written four times
in the same file (M-06).

## Fix

- The entry records `claim_identity`, the `(dev, ino)` of the venv directory, set by
  `TrustStore::bind_claim` once uv exits, on failure too. `pending_build` requires the
  directory's identity now to equal it.
- When uv left no directory, `bind_claim` ends the claim and keeps the token.
- A sync killed before it can bind leaves an unbound claim, which covers nothing: the venv is
  asked about (fail-safe).
- `token_store(resolved)` is the single predicate; the check, lock, claim and stamp use it.

Residual: a filesystem that reuses an inode immediately could match a replaced directory.

## Tests

Regression for the reported shape (failed sync, no directory, marker-less venv appears: Foreign),
a replaced bound directory, an unbound claim, and bind with no pending claim.
