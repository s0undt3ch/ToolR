# Plan: keep the pending venv claim to its run

1. `trust/venv_entries.rs`: drop `claim_identity` and `bind_claim`; add `end_claim`;
   `lock_venv` ends a claim it finds.
2. `venv/sync.rs`: only the marker vouches for a venv, never a claim; `sync_if_needed` waits on
   the lock when it finds a claim, before its first check; `claim_before_build` returns
   whether the venv was absent; `abandon_build` removes what a failed first sync left and
   ends the claim.
3. `venv/sync.rs`: collapse the token predicate into `token_store`.
4. Tests through the real `sync_if_needed` with a stub uv, plus the lock-bound claim cases and
   trust store unit tests.
5. Release note; `prek run --all-files`; `mise run test`; patch coverage.
