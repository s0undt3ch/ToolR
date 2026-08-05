# Testing your commands

ToolR ships two small testing helpers in the `toolr-py` wheel, plus a pytest plugin that needs no
setup at all.

- `toolr.testing.CommandsTester` lets you write pytest assertions against your `tools/*.py`
  discovery without invoking the `toolr` binary as a subprocess.
- `toolr.testing.make_context` builds a real `Context` so you can call an `@command`-decorated
  function directly and assert on what it did — see [Calling a command directly](#calling-a-command-directly)
  below.
- A `pytest11` plugin appends your repo root to `sys.path` automatically, so `import tools.*`
  resolves under `pytest tools/` with zero configuration.

It's designed for the case where you want to test *your own* command modules: "does my decorator
land in the registry?", "do my command groups collect the right commands?", "does my dispatcher
pick up the right children?". For end-to-end behaviour (Tab completion, `--help` output, real
subprocess execution) you'll still want to drive the binary directly.

## What it does

`CommandsTester(search_path=tmp_path)` is a context manager that:

1. Saves and replaces `sys.path` so only `search_path` and the host's `site-packages` are visible
   — your fixture's `tools/` tree wins.
2. Patches `toolr._decorators._get_command_group_storage` with a fresh `dict` so the test gets an isolated registry.
3. Restores everything on exit (`sys.path`, `sys.modules`, `cwd`).

Calling `.discover()` inside the context imports every `tools/*.py` module, registering each
`command_group` / `@command` call exactly as a real `import tools.*` would. After it returns,
`.collected_command_groups()` gives you a `{full_name: CommandGroup}` dict you can assert against.

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
import pytest

from toolr.testing import make_context

from tools.example import hello


def test_hello_prints_a_greeting(tmp_path):
    result = make_context(tmp_path)
    hello(result.ctx, name="Pedro")
    assert "hello, Pedro" in result.output.stdout


def test_confirm_aborts_on_no(tmp_path):
    result = make_context(tmp_path)
    result.ctx.prompt = lambda *a, **k: False  # stub the interactive prompt
    with pytest.raises(SystemExit):
        confirm(result.ctx)
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
    hello(ctx.ctx, name="Pedro")
    assert "hello, Pedro" in ctx.output.stdout
```

`result.ctx` is a real `Context` — `ctx.repo_root` is set, `ctx.run(...)` calls the real subprocess
runner (monkeypatch it if a test needs to intercept it), and `ctx.exit(...)` raises `SystemExit` via
a real `ArgumentParser`, exactly as it does under the CLI. `result.output.stdout` /
`result.output.stderr` capture everything written through `ctx.print`/`ctx.info`/`ctx.error`/etc.

## Stability

`toolr.testing.CommandsTester` and `toolr.testing.make_context` are part of toolr-py's public API
and are tested in toolr's own suite. `CommandsTester`'s surface — the constructor, the
context-manager protocol, `.discover()`, and `.collected_command_groups()` — is stable across the
pre-1.0 series; any change is called out in the changelog. Same for `make_context`'s signature and
the `ContextForTesting`/`CapturedOutput` shape it returns.

Internal attributes (`sys_path`, `sys_modules`, `command_group_patcher`, `cwd`) are implementation
detail and may change without notice.
