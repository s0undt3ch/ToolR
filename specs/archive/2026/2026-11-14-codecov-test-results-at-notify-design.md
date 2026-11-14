# Codecov test results uploaded at notify time — design

## Problem

`.codecov.yml` sets `codecov.notify.manual_trigger: true`, and `ci.yml`'s `codecov-notify` job runs
`send-notifications` after every test job. Even so, Codecov posts its check runs and the PR comment
after each test job, computed from whatever coverage has arrived so far. On PR #747 the first
comment read `Project coverage is 33.4%`, and the checks failed, until the last upload landed.

The cause is in Codecov's worker (`codecov/umbrella`, `apps/worker`):

- The coverage path honours the setting: `tasks/upload_finisher.py` returns `DO_NOT_NOTIFY` when
  `codecov.notify.manual_trigger` is set.
- The test-results path doesn't. `services/test_analytics/ta_finish_upload.py` returns
  `queue_notify: True` when all tests passed, and `tasks/test_results_finisher.py` then queues the
  full coverage notify task, which posts checks and the comment.

Every `_test.yml` and `_test_distribution.yml` job uploads `report_type: test_results` straight after
its coverage upload, so each one triggers an early notify.

Commit statuses did wait (they posted after `send-notifications`), so `github_checks: false` would
only move the early results from checks to statuses. That is an inference from the worker code, not
tested.

## Decision

Upload test results only once all coverage is in. The test-results notify then runs against
complete coverage, whenever it lands.

Two in-repo composite actions, shaped so they can move to a shared actions repo later unchanged:

- `.github/actions/codecov-stage-test-results`: runs in each test job. Copies the JUnit files and a
  `meta.json` (flags, name) into one artifact.
- `.github/actions/codecov-publish-test-results`: runs in `codecov-notify`. Downloads every staged
  artifact, uploads each with its own flags and name, then runs `send-notifications`.

A composite action can't call `codecov/codecov-action` in a loop, and one call applies one set of
flags to every file, so the publish action drives `codecovcli` from a stdlib-only Python script.

### What the publish script must replicate from `codecov-action`

- OIDC: request `$ACTIONS_ID_TOKEN_REQUEST_URL&audience=https://codecov.io` with
  `Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN` (what `core.getIDToken` does).
- Forks (`pull_request.head.repo.full_name != GITHUB_REPOSITORY`): no OIDC token; upload tokenless
  with `--pr <number>` and `--branch <head.label>`.
- Commit: `--sha` from `pull_request.head.sha` on PRs, `GITHUB_SHA` otherwise. The CLI's own PR
  lookup needs a deep checkout; the notify job's is shallow.
- Token goes in the `CODECOV_TOKEN` environment variable, never on the command line. The CLI
  declares `envvar="TOKEN"`, but its `BrandedOption` prefixes the brand, so `TOKEN` alone is ignored.

Build URL and job code need no override: the CLI derives them from `GITHUB_RUN_ID` and
`GITHUB_WORKFLOW` (`codecov_cli/helpers/ci_adapters/github_actions.py`), which match between the
test job and the notify job.

### CLI install

mise installs Codecov's standalone `codecovcli` binary with its `github:` backend. The tool lives
in a CI-only overlay, `mise.ci.toml`, so contributors never install it. `codecov-notify` sets
`MISE_ENV=ci`, and `jdx/mise-action` installs it with `--locked` against the per-platform sha256 in
`mise.ci.lock`. Renovate's mise manager reads `mise.<env>.toml` and re-locks it with that
`MISE_ENV`.

On linux-x64 mise's asset autodetection picks `codecovcli_alpine_x86_64`, whose ELF interpreter is
`/lib/ld-musl-x86_64.so.1`. That loader isn't on the glibc runners, so `asset_pattern` pins
`codecovcli_linux` there.

Codecov publishes no GitHub artifact attestations for the CLI (`gh attestation verify` returns
404), so the lockfile's digest is the check.

The stage step runs a stdlib-only `stage.py` with the job's `python`, on every runner OS.

### Edge cases

- A test job dies before writing its JUnit file: the stage action warns and uploads nothing.
- No staged artifacts at all (for example every test job was skipped): the publish action warns,
  uploads nothing and still sends notifications, so coverage results still post.
- A staged directory without a readable `meta.json`: error. That is a bug in the stage side.
- `codecov-notify` keeps its `run-full-ci` condition: when that is false no test job runs, so there
  is nothing to publish.

## Out of scope

- Coverage uploads stay in the test jobs; they already wait for `send-notifications`.
- Moving the actions to a separate repository.
