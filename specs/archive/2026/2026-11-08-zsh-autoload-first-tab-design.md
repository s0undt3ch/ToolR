# Zsh completion: complete on the first autoloaded Tab

## Problem

`toolr self completion install zsh` writes the script to `~/.zfunc/_toolr`. On the first Tab in a new
shell, zsh autoloads that file as the body of `_toolr`. The file defines `_toolr()` and calls
`compdef`, then ends. Running the body only redefines the function, so nothing completes. The second
Tab calls the new definition and works.

## Design

Keep the `#compdef toolr` first line and end the script with a guard on `$zsh_eval_context`:

```zsh
if [[ ${zsh_eval_context[-1]-} == loadautofunc ]]; then
    _toolr "$@"
else
    compdef _toolr toolr
fi
```

- Autoloaded: `compinit` has already bound `toolr` to `_toolr` through the `#compdef` line, so the
  script only needs to run the function it has just defined, with the completion arguments.
- Sourced or eval'd from `.zshrc`: the last context is not `loadautofunc`, so `compdef` registers the
  function as before.

## Testing

A pty test starts an interactive zsh whose `fpath` holds the script as `_toolr`, runs `compinit`,
and presses Tab once. It fails before the change. The same test also runs the sourced mode.

## Out of scope

Changing where the installer writes the file, or the install hint text.
