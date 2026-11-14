# Plan: venv pending-claim binding

1. `trust/venv_entries.rs`: add `claim_identity`, `bind_claim`, `claim_identity()`; share
   `store_entry`.
2. `venv/sync.rs`: `pending_build` compares identities; `bind_claim_after_build` runs after uv.
3. `venv/sync.rs`: collapse the token predicate into `token_store`.
4. Tests beside `a_claimed_venv_without_a_marker_is_toolrs_until_stamped`; trust store unit test.
5. Release note; `prek run --all-files`; `mise run test`; patch coverage.
