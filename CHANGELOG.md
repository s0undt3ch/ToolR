# Changelog

All notable changes to this project will be documented in this file.

This project uses [*git-cliff*](https://git-cliff.org/) to automatically generate changelog entries
from [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.11.0 - 2025-09-24

### <!-- 0 -->🚀 Features

- *(docstrings)* We now use a rust extension to parse the docstrings ([`a2744f0`](https://github.com/s0undt3ch/ToolR/commit/a2744f0ff3c4b5c086f780d9f0433fc29c3af832))
- *(commands help)* The command's help message is now formatted with Markdown ([`d786915`](https://github.com/s0undt3ch/ToolR/commit/d786915ab9c92726aff05f79c0a079115dd199f9))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(sync-rolling-tags)* Fix `sync-rolling-tags` workflow trigger ([`889e36f`](https://github.com/s0undt3ch/ToolR/commit/889e36fea9025c584ad3d7ea4173ba09aff2beb8))
- *(ci)* Sync'ing rolling tags is now done on demand ([`773745b`](https://github.com/s0undt3ch/ToolR/commit/773745b4156aecee998f8a2e6e494b1a144704e8))
- *(ci)* Fix sync-rolling-tags command ([`59c2955`](https://github.com/s0undt3ch/ToolR/commit/59c29551a6be607748ab97d12dc9958bfa2fdbe4))
- *(ci)* More fixes to the sync-rolling-tags process ([`45ac806`](https://github.com/s0undt3ch/ToolR/commit/45ac80617a69bec0ba2c32b91deec634a98d6eb1))
- *(release)* Update ToolR action versions in workflows ([`6cc4bde`](https://github.com/s0undt3ch/ToolR/commit/6cc4bdecb59d83d95d52f6a908fb5712a888acd0))
- *(ci)* Final sync-rolling-tags fix ([`c48fcbe`](https://github.com/s0undt3ch/ToolR/commit/c48fcbe64867018814992c2712f38c3648b23e53))
- *(ci)* Refresh some caches ([`a91de3d`](https://github.com/s0undt3ch/ToolR/commit/a91de3d2b2ad13887dd6e19a47570b4a2bb80bcf))

## 0.10.1 - 2025-09-19

### <!-- 1 -->🐛 Bug Fixes

- *(parent)* Fix command nesting ([`c223dfc`](https://github.com/s0undt3ch/ToolR/commit/c223dfc88e2981dbd7cd6aed304d219fc3f8f12a))
- *(command)* We now log the `.run()` cmdline at the `INFO` level ([`1311f8d`](https://github.com/s0undt3ch/ToolR/commit/1311f8d817fd71b4aa2b061c48d3e067b8076486))
- *(tests)* Fix `ctx.which` tests to make them less brittle. ([`8a55cf3`](https://github.com/s0undt3ch/ToolR/commit/8a55cf32e5e793472b5059a5a433b3e6a90cfc38))

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(release)* Don't update ToolR action usage in workflows ([`fbe4992`](https://github.com/s0undt3ch/ToolR/commit/fbe49927887da44cfbba0e8e4892a672d14837df))
- *(release)* Add workflow that updates ToolR versions in workflows ([`9782974`](https://github.com/s0undt3ch/ToolR/commit/9782974d40ff2d62a183025c9c48dee0d3e92143))

## 0.10.0 - 2025-09-17

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(ci)* Improve CI build matrix reporting ([`777595c`](https://github.com/s0undt3ch/ToolR/commit/777595c0ce4c32fe0a88e249e49fb36bc54206f9))
- *(ci)* Prevent unnecessary branch builds on PRs ([`4d7e911`](https://github.com/s0undt3ch/ToolR/commit/4d7e911fb7448e22fe9f5b961bfe368b3ef50868))
- *(release)* Update all self ToolR actions usage on release ([`38e2a8b`](https://github.com/s0undt3ch/ToolR/commit/38e2a8b92b7c95431cf3ff05e8af50d020b4f11d))
- *(ci)* Consistent workflow toolr output width ([`4475406`](https://github.com/s0undt3ch/ToolR/commit/447540685a36ddae3936ee4c3232474c42f59748))

## 0.9.0 - 2025-09-13

### <!-- 0 -->🚀 Features

- *(cli)* Provide a `Context` class
- *(registry)* Implemented the registry and it's tests
- *(logging)* Add `toolr.utils.logs` to improve logging support
- *(cli)* Provide the package CLI entry point
- *(help)* We now use ``RichHelpFormatter`` to render the help
- *(docstrings)* Parse docstrings to construct help
- *(docs)* Capture each parameter description from docstrings
- *(coverage)* Upload code coverage to codecov
- *(ci)* Upload test results to codecov
- *(signatures)* Add signature parsing
- *(signature)* Handle append action, including weird boolean append.
- *(nargs)* Support ``nargs`` and ``*variable`` in function signatures
- *(docs)* Documentation!
- *(context)* Implemented prompt support in ``Context``.
- *(github-actions)* Allow setting ToolR from a github-action
- *(signature)* Add support for mutually exclusive groups
- *(logging)* Add `setup_logging` function.

### <!-- 1 -->🐛 Bug Fixes

- *(imports)* Handle import errors when searching for tools
- *(descriptions)* Differentiate descriptions
- *(docstring)* Fix dosctring class reference
- *(decorator)* Fix decorator usage.
- *(help)* Parse each decorated command docstring to provide help
- *(log)* Only log the time on specific occasions.
- *(tests)* Fix tests according to latest code changes
- *(tests)* Fix rust tests on windows
- *(scope)* Let the codecov CLI tool find the coverage files
- *(coverage)* Don't track coverage in ``if TYPE_CHECKING:`` code blocks
- *(signature)* `dest` is always set to the name of the positional parameter
- *(tests)* Small refactor to improve testing
- *(signature)* On positional arguments, the name will always be the first alias
- *(enums)* Handle enums by name instead of by value
- *(cli)* Fix early verbose/debug output CLI parsing logic
- *(tests)* Skip problematic windows test
- *(docs)* Include missing docs examples
- *(docs)* Remove `uv run` prefix from examples
- *(command)* Command names from functions auto-naming
- *(SignatureError)* `SignatureError` exceptions now point to command
- *(pypi)* Fix PyPi packaging uploads

### <!-- 2 -->🚜 Refactor

- *(toolr)* Support 3rd-party commands
- *(consoles)* Name context consoles explicitly
- *(3rd-party)* Fix commands and command groups augment/overrides
- *(consoles)* Refactor consoles setup

### <!-- 7 -->⚙️ Miscellaneous Tasks

- *(dependencies)* Add `rich=-argparse` as a dependency
- *(command)* Rename `command.run_command` to `command.run`
- *(context)* Make the ``context`` module "private".
- *(ci)* Define allowed concurrency
- *(requiremenst)* We no longer need to maintain separate requirements files
- *(tools)* Clean up the pre-existing tools directory
- *(pre-commit)* Update pre-commit hook versions
- *(lint)* Fix lint issues found with latest pre-commit hooks versions
- *(cibuildweel)* Bump `MACOSX_DEPLOYMENT_TARGET` to `11.0`
- *(cleanup)* Remove `changelog.d/`, it won't be needed anymore
- *(typing)* Make the typing gods happier
- *(msgspec)* Replaced all usages of ``dataclass`` with ``msgspec.Struct``
- *(pre-commit)* Upgrade some pre-commit hooks
- *(pre-commit)* Add ``codespell`` pre-commit hook
- *(parser)* Use a private method to set the parser instead.
- *(discovery)* Actually start discovering tools when running ``toolr``
- *(typing)* Fix typing
- *(samples)* Fix sample cases to respect the required signature
- *(rust)* Address clippy errors
- *(ci)* Define the pre-commit cache to be inside the workspace
- *(ci)* Parallelize package builds
- *(ci)* Use OIDC to authenticate codecov
- *(tests)* Add default pytest flags to config
- *(dependencies)* Add ``pytest-subtests`` to dev dependencies
- *(tests)* Add ``argv`` tests
- *(logs)* Logging utils module testing
- *(tests)* Add test coverage for the `__main__` module
- *(tests)* Improve test coverage of the context object
- *(README.md)* Fix logo file path
- *(mypy)* Have mypy ignore `tests/support/3rd-party-pkg/.*`
- *(tests)* Add test coverage to ``setup_consoles``
- *(pyproject.toml)* Define the 3rd-party test package as editable
- *(ci)* Improved parallelization
- *(pre-commit)* Update hook versions
- *(tests)* Split `tests/test_context.py` into several test modules
- *(docs)* Add ``ruff`` as a docs dependency
- *(ci)* Add and use ``.github/actions/setup-virtualenv``
- *(ci)* Push built packages to test.pypi.org on the default branch
- *(docs)* Fix logo URL in readme
- *(gitignore)* Ignore `*.code-workspace`
- *(pre-commit)* Upgrade pre-commit hook versions
- *(ConsoleVerbosity)* Move `ConsoleVerbosity` to  `toolr.utils._console`
- *(action)* Simplify action
- *(release)* Update the release process
- *(release)* Separate release workflow
- *(security)* Include build provenance attestations
- *(debug)* Set verbose to true when running in debug mode
- *(oackages)* Stop building for `s390x`.
- *(dependabot)* Add `dependabot` configuration
- *(docs)* Add `.readthedocs.yaml` config file
- *(release)* Fix attestations on release workflow
- *(release)* Fix generate build matrix step
- *(changelog)* Add cliff config file
- *(release)* More release workflow fixes
- *(release)* Use the global permissions
- *(release)* Use GH App to push the tags
- *(release)* The action now just configures git with higher privileges
- *(release)* Just repeat, it's simpler in the end
- *(release)* Use `sdist` to build wheels
- *(release)* Prepare for 0.1.1 release
- *(release)* Publish GH release fixes
- *(release)* Prepare for 0.1.2 release
- *(release)* Change release notes filename name
- *(docs)* Update the docs URL to  the right one
- *(release)* Remove the PyPi url
- *(release)* Revert debug release changes
- *(release)* Fix package name to be compliant with PyPi
- *(changelog)* Fix white-space issues around changelog generation
- *(prepare-release)* Run `pre-commit` against the prepare release changes
- *(ci)* Pre-commit needs to be setup and run in a few places

### New Contributors

- @s0undt3ch-gh-actions-automations[bot] made their first contribution
- @dependabot[bot] made their first contribution
