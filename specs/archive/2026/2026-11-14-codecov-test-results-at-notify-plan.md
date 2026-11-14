# Codecov test results uploaded at notify time — plan

Design: [`2026-11-14-codecov-test-results-at-notify-design.md`](2026-11-14-codecov-test-results-at-notify-design.md).

1. Add `.github/actions/codecov-stage-test-results/action.yml`: inputs `files` (newline list),
   `flags` (comma list), `name`, `artifact-name`, `retention-days`. A stdlib-only `stage.py`, run
   with the job's `python`, stages the files and writes `meta.json`; `actions/upload-artifact`
   uploads it as `codecov-test-results-<artifact-name>`.
2. Add `.github/actions/codecov-publish-test-results/`: `action.yml` and `publish.py` (stdlib
   only). Steps: download `codecov-test-results-*`, run `publish.py` against the `codecovcli` on
   `PATH`. Add `github:codecov/codecov-cli` to a new `mise.ci.toml`, lock it into `mise.ci.lock`
   with `MISE_ENV=ci mise lock`, and install it in `codecov-notify` with `jdx/mise-action`.
3. Tests in `tests/action_yml/`: drive `stage.py`, and `publish.py` with real staged trees in
   `tmp_path` and a fake `codecovcli` on `PATH` that records its arguments; serve the OIDC endpoint
   from a local HTTP server. Cover forks, pushes, missing `meta.json`, no artifacts, missing token env.
4. Wire `_test.yml` and `_test_distribution.yml` to the stage action, replacing the
   `report_type: test_results` uploads. Keep the flags they send today.
5. Replace `codecov-notify`'s `codecov-action` call with the publish action.
6. `prek run --all-files`, the new tests, then a PR and a check that the Codecov comment and checks
   only post after `Codecov Notify`.
7. Archive this design and plan under `specs/archive/2026/`.
