# Using `ctx`

Every command function receives a [`Context`][toolr.Context] as its
first parameter. It's the only object you need to talk to the
terminal, run subprocesses, prompt the user, or exit cleanly.

## Output

`ctx.print(...)` writes Rich-rendered text to stdout. `ctx.info(...)`
and `ctx.error(...)` write structured log records (respecting
`--debug` / `--quiet`).

```python
--8<-- "docs/writing-commands/files/example.py:20:31"
```

```sh
toolr example hello --name Pedro
```

```text
--8<-- "docs/writing-commands/files/context-hello.txt"
```

(The example here is the `hello` function from the scaffold's
`example.py` — same file that `toolr project init` generates.)

## Running subprocesses

`ctx.run(*cmd, capture_output=True, check=True)` is the canonical way
to run a subprocess. `capture_output=True` gives you a result object
with `.stdout` and `.stderr`; default (no capture) streams the
subprocess's output to the user's terminal.

```python
--8<-- "docs/writing-commands/files/example.py:34:45"
```

## Working directory

Commands run with the working directory set to the **repo root**, no
matter where you invoke `toolr` from — the same convention `make` and
`cargo` follow. `ctx.repo_root` always points there, and `ctx.run(...)`
subprocesses inherit it as their cwd unless you override it.

Reach for `ctx.repo_root` whenever you need an absolute path into the
project. **Prefer it over deriving the root from `__file__`** (for
example `Path(__file__).parents[2]`): that hard-codes the file's depth
in the tree, so it breaks the moment the file moves or the command is
packaged as a plugin. toolr already discovered the root — ask it.

Because of this, a **relative path argument resolves from the repo
root, not from your current directory**. Running `toolr build ./out.txt`
from a subdirectory writes `<repo-root>/out.txt`. Pass an absolute path
when you need a file relative to where you ran the command. toolr prints
a one-line note on stderr if you pass a relative path argument from a
subdirectory, so the behaviour is never silent.

## Prompting and exiting

`ctx.prompt(...)` reads a line from stdin (returns the typed string,
empty if the user just hits enter). `ctx.exit(code, message=None)`
writes the message to stderr and exits with the given code — no
exception traceback.

```python
--8<-- "docs/writing-commands/files/example.py:48:58"
```

## The full surface

See [`Context`][toolr.Context] in the API reference for every method
and attribute. Highlights:

- `ctx.print(...)` / `ctx.info(...)` / `ctx.error(...)` / `ctx.debug(...)`
- `ctx.run(*cmd, ...)` — subprocess execution
- `ctx.prompt(prompt)` — read a line from stdin
- `ctx.exit(code, message=None)` — early exit with a message
- `ctx.chdir(path)` — context-manager to switch directories
- `ctx.repo_root` — the path toolr discovered as the project root

Next: [Annotations →](annotations.md)
