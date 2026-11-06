# Testing your commands

ToolR ships two small testing helpers in the `toolr-py` wheel, plus a pytest plugin that needs no
setup at all.

- `toolr.testing.CommandsTester` lets you write pytest assertions against your `tools/*.py`
  discovery without invoking the `toolr` binary as a subprocess.
- `toolr.testing.make_context` builds a real `Context` so you can call an `@command`-decorated
  function directly and assert on what it did — see [Calling a command directly](#calling-a-command-directly)
  below.
- A `pytest11` plugin appends your repo root to `sys.path` automatically, so `import tools.*`
  resolves under `pytest tests/tools/` with zero configuration.

Keep these tests out of `tools/`: toolr reads every `*.py` there for commands. Put them in your project's own tests
tree, e.g. `tests/tools/<module>/test_<command>.py`, with an `__init__.py` in `tests/` and each directory below it so
pytest doesn't import `tests/tools/` as a second `tools` package. Keep fixture data, such as a sample `tools/` tree for
`CommandsTester`, in a `files/` directory next to the tests that use it, without an `__init__.py`.

It's designed for the case where you want to test *your own* command modules: "does my decorator
land in the registry?", "do my command groups collect the right commands?", "does my dispatcher
pick up the right children?". For end-to-end behaviour (Tab completion, `--help` output, real
subprocess execution) you'll still want to drive the binary directly.

## What it does

`CommandsTester(search_path=tmp_path)` is a context manager that:

1. Puts `search_path` first on `sys.path` and makes the top-level `tools` package resolve from
   `search_path` only. Every other `sys.path` entry stays importable, so the standard library and
   anything installed still import, but no other `tools` (the host repo's own, or a stray one some
   installed package ships) can replace the fixture's `tools/` or merge into it.
   With no `tools` in `search_path`, `importlib.util.find_spec("tools")` raises
   `ModuleNotFoundError` instead of returning `None`, since that is what stops the lookup reaching
   another `tools`. pytest's assertion rewriting doesn't apply to the top-level
   `tools/__init__.py` under the tester; `tools.*` submodules are unaffected.
2. Patches `toolr._decorators._get_command_group_storage` with a fresh `dict` so the test gets an isolated registry.
3. Restores everything on exit (`sys.path`, `sys.meta_path`, `sys.modules`, `cwd`).

Calling `.discover()` inside the context imports every `.py` module under `tools/`, including
subdirectories with no `__init__.py`, registering each `command_group` / `@command` call exactly as
a real `import tools.*` would. It only walks directories under `search_path / "tools"`, so a
fixture `tools/__init__.py` that calls `pkgutil.extend_path` doesn't pull in another `tools/`.
After it returns,
`.collected_command_groups()` gives you a `{full_name: CommandGroup}` dict you can assert against.

If any module fails to import, `.discover()` still imports the rest, then raises one
`ExceptionGroup` holding each module's error, so a discovery test fails the way `toolr` would.
For example, a command without a docstring:

```python
with CommandsTester(search_path=tmp_path) as tester:
    with pytest.raises(ExceptionGroup) as excinfo:
        tester.discover()

assert excinfo.group_contains(ValueError, match="add a docstring")
```

Before importing anything, `.discover()` also raises an error when a `*.py` under `tools/`
is a symlink, with the same message `toolr` prints, since `toolr` refuses that tree too.

## Usage

```python
from pathlib import Path

from toolr.testing import CommandsTester


def test_my_tools_register_a_group(tmp_path: Path) -> None:
    tools = tmp_path / "tools"
    tools.mkdir()
    (tools / "ci.py").write_text(
        '"""CI helpers."""\n'
        "from toolr import Context, command_group\n"
        "\n"
        'ci = command_group("ci", "CI", "CI helpers")\n'
        "\n"
        "@ci.command\n"
        "def hello(ctx: Context, name: str = 'world') -> None:\n"
        '    """Say hello."""\n'
        "    ctx.print(f'hi {name}')\n",
    )

    with CommandsTester(search_path=tmp_path) as tester:
        tester.discover()
        groups = tester.collected_command_groups()

    assert "tools.ci" in groups
    assert "hello" in groups["tools.ci"].get_commands()
```

A typical pytest fixture wires the boilerplate once per test:

```python
import pytest
from collections.abc import Iterator
from pathlib import Path
from toolr.testing import CommandsTester


@pytest.fixture
def commands_tester(tmp_path: Path) -> Iterator[CommandsTester]:
    tester = CommandsTester(search_path=tmp_path)
    with tester:
        tester.discover()
        yield tester
```

## What you can assert

`collected_command_groups()` returns a dict keyed by the dotted full name (e.g. `tools.ci`,
`tools.docker.image`). Each value is a `toolr._decorators.CommandGroup` instance, which exposes:

- `name`, `title`, `description`, `parent` — what you passed to `command_group(...)`.
- `full_name` — same key the dict uses.
- `get_commands()` → `dict[name, Callable]` of registered commands.

Common assertions:

```python
groups = tester.collected_command_groups()

# A group exists at the expected dotted path.
assert "tools.docker.image" in groups

# A command is registered under it.
assert "build" in groups["tools.docker.image"].get_commands()

# A specific function got decorated.
assert groups["tools.docker.image"].get_commands()["build"].__name__ == "build"

# Cross-file attachment: a group declared in one file, commands added from another.
assert {"helm-diff-pr-comment", "snippet-checker"} <= set(
    groups["tools.ci"].get_commands().keys()
)
```

Commands declared with the standalone `@command(group="…")` decorator appear under their group, whether the group
is declared in the same file or another one, and whatever order the files are imported in. A `group=` that names no
declared group, or a standalone `@command` with no `group=` at all, makes `collected_command_groups()` raise
`ValueError` with the message the manifest build gives:

```text
unknown group references (1):
  - tools.cmds::check: references group `ci.typo` which has no `command_group(...)` declaration.
```

`CommandsTester` runs your code, but `toolr` reads it without running it, so the tester checks the source for what
`toolr` can't read. `@command(...)`, `@toolr.command(...)` (also after `import toolr as t`) and an alias from
`from toolr import command as cmd` all work, on a top-level `def` with `group=` and any name as string literals.
Commands must be top-level `def`s: put a platform check inside the command body, not around the `def`. Groups follow
the same rule: any `command_group(...)` call that isn't the whole value of a module-level assignment or bare
statement, e.g. under `if`/`try`/`with`/loops/`match`, inside a function, lambda, comprehension or class, or nested in
another expression such as a decorator or call argument fails both. A name-only lookup such as `command_group("ci")`
can't create a group, so it is fine anywhere.
A command on a nested `def` (inside another function, a class, an `if TYPE_CHECKING:` or any other `if`), a
`def` carrying two command decorators, a `group=SOME_CONSTANT` or a non-literal name fails both:

```text
unsupported command declarations (3):
  - tools.cmds::command_group("ci"): `command_group(...)` must be a top-level `name = command_group(...)` assignment or a bare statement, which toolr reads without running it.
  - tools.cmds::nested: `@command` must decorate a top-level `def`, which toolr reads without running it.
  - tools.cmds::check: `group=` must be a string literal.
```

The tester reads the source of every `tools` module, so a nested command fails it even when the code around it never
runs, such as a factory nobody calls or an `if` that's false on this platform. A command a test body defines isn't
checked.

Two functions that define the same command in a group fail both too, listing every definition:

```text
duplicate commands (1):
  - `ci check` is defined more than once: tools.a::check, tools.b::check. Rename all but one, e.g. with `name="…"`.
```

`collected_command_groups()` reports unsupported declarations first, then unknown groups, then duplicates, the order
`toolr` checks them in.

## What it can't do

`CommandsTester` only exercises the Python-side discovery path. It deliberately does not:

- Boot the Rust binary or invoke clap.
- Build a real `tools/.toolr-manifest.json`.
- Spawn `toolr` as a subprocess.
- Run the static AST parser (which is the *Rust* path; this helper drives only the *dynamic* / runtime-import path).

If you need any of those, drive the `toolr` binary directly via `subprocess`
(`shutil.which("toolr")` works under `mise` / `pip install toolr` / the install scripts), or use
`assert_cmd` from a Rust integration test.

## Calling a command directly

`CommandsTester` proves your commands *register* correctly; it doesn't call them. To test the body
of an `@command`-decorated function — does it read `ctx.repo_root` correctly, does it call
`ctx.info` with the right message, does it `ctx.exit` on a bad input — build a real `Context` with
`toolr.testing.make_context`:

```python
from toolr.testing import make_context

from tools.example import hello


def test_hello_prints_a_greeting(tmp_path):
    ctx = make_context(tmp_path)
    hello(ctx, name="Pedro")
    assert "hello, Pedro" in ctx.stdout
```

A function-scoped fixture wires the boilerplate once per test, the same way `commands_tester` does above:

```python
import pytest
from collections.abc import Iterator
from pathlib import Path
from toolr.testing import ContextForTesting
from toolr.testing import make_context


@pytest.fixture
def ctx(tmp_path: Path) -> Iterator[ContextForTesting]:
    yield make_context(tmp_path)


def test_hello_prints_a_greeting(ctx: ContextForTesting) -> None:
    hello(ctx, name="Pedro")
    assert "hello, Pedro" in ctx.stdout
```

Prior to this, stubbing `ctx.prompt` by assigning a lambda directly onto `ctx.prompt` looked
tempting — don't: `Context` is a frozen `msgspec.Struct`, and that assignment raises
`AttributeError: immutable type: 'Context'`. Feed canned answers through `make_context`'s
`prompt_input` parameter instead:

```python
--8<-- "tests/test_make_context.py:mock-prompt-example"
```

Feed a "no" answer the same way to test the abort path of a confirm-style command:

```python
--8<-- "tests/test_make_context.py:mock-prompt-abort-example"
```

`make_context` returns a `ContextForTesting` — a `Context` subclass — so `ctx.repo_root` is
set, `ctx.exit(...)` raises `SystemExit` via a real `ArgumentParser`, exactly as it does under the
CLI, and `ctx.run`/`ctx.chdir`/`ctx.prompt` all run for real unless you pass
`run=`/`chdir=`/`prompt_input=` to `make_context` (see below). `ctx.stdout` captures what
`ctx.print` and `ctx.prompt` write; `ctx.stderr` captures the log lines (`ctx.info`, `ctx.warn`,
`ctx.error`, `ctx.debug`, including the `Running '…'` line `ctx.run` logs) and the `ctx.exit`
message. Verbosity gates `ctx.debug` and `ctx.info`: `make_context` defaults to
`ConsoleVerbosity.NORMAL`, which drops `ctx.debug`, so pass
`make_context(..., verbosity=ConsoleVerbosity.VERBOSE)` (`from toolr.testing import
ConsoleVerbosity`) to capture it. The [`ContextForTesting` reference](../reference/testing.md)
lists exactly what each stream holds at each verbosity.

### Mocking `ctx.run`

`make_context`'s `run` parameter replaces the real subprocess runner with a test double. Pass a
`toolr.testing.RunMock`, configure its `.mock.return_value`/`.mock.side_effect` with a
`CommandResult` built via `make_command_result(...)` — the same `unittest.mock` idiom you already
know — and assert on it with the standard `Mock`-style API. `return_value` hands back the *same*
result (and its already-read stream) on every call, so it's only safe for a command that calls
`ctx.run` once; for more than one call, use `side_effect` to mint a fresh
`make_command_result(...)` each time:

```python
--8<-- "tests/test_make_context.py:mock-run-example"
```

### Mocking `ctx.chdir`

`chdir()` calls the real `os.chdir` twice (enter and restore) by default — fine for most tests, but
a test that wants to assert *which* directory a command tried to move into, without touching the
real filesystem, passes a bare `unittest.mock.Mock` as `chdir`:

```python
--8<-- "tests/test_make_context.py:mock-chdir-example"
```

## Stability

`toolr.testing.CommandsTester` and `toolr.testing.make_context` are part of toolr-py's public API
and are tested in toolr's own suite. `CommandsTester`'s surface — the constructor, the
context-manager protocol, `.discover()`, and `.collected_command_groups()` — is stable across the
pre-1.0 series; any change is called out in the changelog. Same for `make_context`'s signature and
the `ContextForTesting` it returns — a `Context` subclass, not a wrapper.

`toolr.testing.RunMock` and `toolr.testing.make_command_result` are likewise part of toolr-py's
public API and tested in toolr's own suite. `RunMock`'s `Mock`-style assertion surface and
`make_command_result`'s signature are stable across the pre-1.0 series; any change is called out
in the changelog. So is `toolr.testing.ConsoleVerbosity`, the type of `make_context`'s
`verbosity` parameter: its `QUIET`/`NORMAL`/`VERBOSE` members are stable across the pre-1.0
series.

Internal attributes (`sys_path`, `sys_modules`, `command_group_patcher`, `cwd`) are implementation
detail and may change without notice.
