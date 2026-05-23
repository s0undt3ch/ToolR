# toolr — mise plugin

This is the [mise](https://mise.jdx.dev/) plugin for
[toolr](https://github.com/s0undt3ch/ToolR). It installs and manages
prebuilt `toolr` binary releases from GitHub.

## Install

```sh
mise plugin add toolr https://github.com/s0undt3ch/ToolR.git#installation/mise
mise install toolr@latest
mise use --global toolr@latest
```

(In the source tree this plugin lives at `installation/mise/`.
Historically that path was `dist/mise-plugin/`; the URL above is the
canonical post-rename location.)

## Layout

The plugin follows the asdf plugin layout:

- `bin/list-all` — list available `toolr` versions from GitHub releases.
- `bin/download` — fetch the release archive for the host's target triple.
- `bin/install` — extract the binary into the mise-managed install dir.

## Docs

For project pinning (`.mise.toml`, `.tool-versions`), task integration,
and troubleshooting, see the
[mise installation guide](https://toolr.readthedocs.io/latest/installation/mise/)
on the toolr docs site.
