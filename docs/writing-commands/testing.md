# Testing your commands

ToolR ships a small testing helper in the `toolr-py` wheel — `toolr.testing.CommandsTester` —
that lets you write pytest assertions against your `tools/*.py` discovery without invoking the
`toolr` binary as a subprocess.

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

Calling `.discover()` inside the context runs the same `tools/`-import pass that
`python -m toolr._introspect` runs for the Rust binary's dynamic manifest layer. After it returns,
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

## Stability

`toolr.testing.CommandsTester` is part of toolr-py's public API and is tested in toolr's own suite.
Its surface — the constructor, the context-manager protocol, `.discover()`, and
`.collected_command_groups()` — is stable across the pre-1.0 series; any change is called out in
the changelog.

Internal attributes (`sys_path`, `sys_modules`, `command_group_patcher`, `cwd`) are implementation
detail and may change without notice.
