<!-- rumdl-disable MD046 MD076 -->

# Plan 10: `toolr project init` Bootstrap Command

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Lint:** Plan docs nest fenced code inside list items for step-by-step structure. The `<!-- rumdl-disable MD046 MD076 -->` directive above turns off the code-block-style and list-item-spacing rules for this file only.

**Goal:** Ship `toolr project init` — a new built-in subcommand that scaffolds `tools/` in the current repo (writes `tools/pyproject.toml`, `tools/.gitignore`, `tools/example.py`), then runs `ensure_venv_ready` so the user can go from `init` to a runnable example in one command. No `tools/__init__.py` is generated (PEP 420 namespace packages, validated).

**Architecture:** A new `init_templates` module embeds three template files (`pyproject.toml.tmpl`, `gitignore.tmpl`, `example.py.tmpl`) via `include_str!`, performs tiny string substitution for `{REQUIRES_PYTHON}` / `{VENV_LOCATION}`, and returns rendered strings. A new `project_init` dispatcher arm under `src/bin/toolr/project.rs` validates preconditions (refuse if `tools/` exists and non-empty unless `--force`), atomically writes the rendered files, then calls into the existing `_rust_utils::project::ensure_venv_ready` for the sync step (skippable via `--no-sync`).

**Tech Stack:** Existing repo deps — clap, anyhow, `_rust_utils` crate. New deps: none. Integration tests use `assert_cmd` + `tempfile` (already in `[dev-dependencies]`).

**Reading order in this plan:** Tasks build on each other. Don't skip ahead; later tasks reference template paths and types defined in earlier ones.

---

## Task 1: Roadmap entry + clap skeleton

Add Plan 10 to the roadmap as `🔧 In Progress` and register the `init` subcommand in clap without functionality (dispatcher just returns "not implemented yet"). This anchors the wiring so later tasks land in a known place.

**Files:**

- Modify: `specs/rust-front-end/01-roadmap.md`
- Modify: `src/bin/toolr/cli.rs`
- Modify: `src/bin/toolr/project.rs`

- [ ] **Step 1.1: Add Plan 10 to the roadmap**

    In `specs/rust-front-end/01-roadmap.md`, append a new sub-plan entry after Plan 9 (keep the existing sub-plans intact):

    ```markdown
    ### Plan 10: `toolr project init` bootstrap command

    - **Status:** 🔧 In Progress
    - **Plan doc:** [12-plan-10-project-init.md](./12-plan-10-project-init.md)
    - **Depends on:** Plans 1-9 (uses `ensure_venv_ready` from Plan 3 + meta sidecar from Plan 8)
    - **Unblocks:** Plan 11 (docs restructure references real `init` output)
    - **Produces:**
        - New `toolr project init` subcommand under the existing `project` namespace.
        - PEP 420 namespace-package scaffold (no `tools/__init__.py`):
          `tools/pyproject.toml`, `tools/.gitignore`, `tools/example.py` with four
          `ctx`-feature-exercising commands (`hello`, `commit`, `confirm`, `setlog`).
        - Auto-`uv sync` via `ensure_venv_ready` (skippable with `--no-sync`).
        - Integration tests in `tests/project_init.rs`.
    ```

- [ ] **Step 1.2: Register the `init` subcommand in `src/bin/toolr/cli.rs`**

    Locate the existing `project` subcommand registration (currently has `deps`, `venv`, `manifest` arms). Add `init` as a fourth arm, alphabetically first inside the block. Insert the following inside the `Command::new("project")` builder, immediately before the existing `.subcommand(Command::new("deps")...)`:

    ```rust
    .subcommand(
        Command::new("init")
            .about("Scaffold tools/ in the current directory")
            .arg(
                Arg::new("force")
                    .long("force")
                    .action(ArgAction::SetTrue)
                    .help("Overwrite an existing tools/ directory"),
            )
            .arg(
                Arg::new("no-sync")
                    .long("no-sync")
                    .action(ArgAction::SetTrue)
                    .help("Skip the automatic `uv sync` after scaffolding"),
            )
            .arg(
                Arg::new("venv-location")
                    .long("venv-location")
                    .value_name("LOCATION")
                    .value_parser(["cache", "in-tree"])
                    .default_value("cache")
                    .help("Where the tools venv should live"),
            )
            .arg(
                Arg::new("no-example")
                    .long("no-example")
                    .action(ArgAction::SetTrue)
                    .help("Skip generating tools/example.py"),
            )
            .arg(
                Arg::new("python")
                    .long("python")
                    .value_name("VERSION")
                    .help("`requires-python` value for tools/pyproject.toml \
                           (defaults to the running Python's >=major.minor)"),
            )
            .arg(
                Arg::new("quiet")
                    .long("quiet")
                    .short('q')
                    .action(ArgAction::SetTrue)
                    .help("Suppress informational output"),
            ),
    )
    ```

- [ ] **Step 1.3: Add the dispatcher arm in `src/bin/toolr/project.rs`**

    Locate the `dispatch_project` function. Inside the `match matches.subcommand()` block, add `init` as a new arm before the existing `("deps", _)` arm:

    ```rust
    Some(("init", init_m)) => project_init(init_m),
    ```

    Add the stub function at module level (under the existing `manifest_rebuild`):

    ```rust
    fn project_init(_matches: &ArgMatches) -> Result<ExitCode> {
        anyhow::bail!("toolr project init is implemented in a later task")
    }
    ```

- [ ] **Step 1.4: Verify the wiring compiles + appears in help**

    ```bash
    cargo build --bin toolr
    ./target/debug/toolr project init --help
    ```

    Expected: help text lists all six flags (`--force`, `--no-sync`, `--venv-location`, `--no-example`, `--python`, `--quiet`). The actual command exits with the "not implemented" message — that's fine; later tasks fill it in.

- [ ] **Step 1.5: Commit**

    ```bash
    git add specs/rust-front-end/01-roadmap.md src/bin/toolr/cli.rs src/bin/toolr/project.rs
    git commit -m "feat(cli): Skeleton toolr project init subcommand"
    ```

---

## Task 2: Template files + `init_templates` module

Add the three template files under `src/bin/toolr/init_templates/` and a small module that loads them via `include_str!`, renders them with placeholder substitution, and exposes a `ScaffoldOptions` struct that downstream tasks use.

**Files:**

- Create: `src/bin/toolr/init_templates/pyproject.toml.tmpl`
- Create: `src/bin/toolr/init_templates/gitignore.tmpl`
- Create: `src/bin/toolr/init_templates/example.py.tmpl` (Task 3 fills in real content; this task uses a placeholder)
- Create: `src/bin/toolr/init_templates.rs`
- Modify: `src/bin/toolr/main.rs`

- [ ] **Step 2.1: Create `src/bin/toolr/init_templates/pyproject.toml.tmpl`**

    Exact content (note the `{REQUIRES_PYTHON}` and `{VENV_LOCATION}` placeholders):

    ```toml
    [project]
    name = "tools"
    version = "0.0.0"
    requires-python = "{REQUIRES_PYTHON}"
    dependencies = [
        "toolr",
    ]

    [tool.toolr]
    venv-location = "{VENV_LOCATION}"
    ```

- [ ] **Step 2.2: Create `src/bin/toolr/init_templates/gitignore.tmpl`**

    Exact content (single line; in-tree venv layout drops `.venv/` inside `tools/`):

    ```text
    .venv/
    ```

- [ ] **Step 2.3: Create `src/bin/toolr/init_templates/example.py.tmpl` (placeholder)**

    Single-line placeholder — Task 3 replaces this with the full four-function example:

    ```python
    # Placeholder — replaced in Task 3.
    ```

- [ ] **Step 2.4: Create `src/bin/toolr/init_templates.rs`**

    Full module content:

    ```rust
    //! Embedded scaffold templates for `toolr project init`.

    use std::path::Path;

    /// Where the tools venv should live.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VenvLocation {
        Cache,
        InTree,
    }

    impl VenvLocation {
        pub fn as_str(self) -> &'static str {
            match self {
                VenvLocation::Cache => "cache",
                VenvLocation::InTree => "in-tree",
            }
        }
    }

    /// Render-time options for the scaffold templates.
    #[derive(Debug)]
    pub struct ScaffoldOptions {
        pub requires_python: String,
        pub venv_location: VenvLocation,
        pub include_example: bool,
    }

    const PYPROJECT_TMPL: &str = include_str!("init_templates/pyproject.toml.tmpl");
    const GITIGNORE: &str = include_str!("init_templates/gitignore.tmpl");
    const EXAMPLE_PY: &str = include_str!("init_templates/example.py.tmpl");

    /// One rendered file ready to be written to disk.
    #[derive(Debug)]
    pub struct RenderedFile {
        pub relative_path: &'static str,
        pub contents: String,
    }

    /// Render every file the scaffold should write, in deterministic order.
    /// Caller writes each `RenderedFile` to `<tools_dir>/<relative_path>`.
    pub fn render_all(opts: &ScaffoldOptions) -> Vec<RenderedFile> {
        let mut out = Vec::with_capacity(3);
        out.push(RenderedFile {
            relative_path: "pyproject.toml",
            contents: render_pyproject(opts),
        });
        out.push(RenderedFile {
            relative_path: ".gitignore",
            contents: GITIGNORE.to_string(),
        });
        if opts.include_example {
            out.push(RenderedFile {
                relative_path: "example.py",
                contents: EXAMPLE_PY.to_string(),
            });
        }
        out
    }

    fn render_pyproject(opts: &ScaffoldOptions) -> String {
        PYPROJECT_TMPL
            .replace("{REQUIRES_PYTHON}", &opts.requires_python)
            .replace("{VENV_LOCATION}", opts.venv_location.as_str())
    }

    /// Parse the venv-location CLI value.
    pub fn parse_venv_location(value: &str) -> anyhow::Result<VenvLocation> {
        match value {
            "cache" => Ok(VenvLocation::Cache),
            "in-tree" => Ok(VenvLocation::InTree),
            other => anyhow::bail!("invalid --venv-location value: {other} (use cache or in-tree)"),
        }
    }

    /// Suppress the unused `Path` import warning until later tasks use it.
    #[allow(dead_code)]
    fn _path_is_used(_p: &Path) {}

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn render_pyproject_substitutes_placeholders() {
            let opts = ScaffoldOptions {
                requires_python: ">=3.13".into(),
                venv_location: VenvLocation::InTree,
                include_example: true,
            };
            let rendered = render_pyproject(&opts);
            assert!(rendered.contains(r#"requires-python = ">=3.13""#));
            assert!(rendered.contains(r#"venv-location = "in-tree""#));
            assert!(!rendered.contains("{REQUIRES_PYTHON}"));
            assert!(!rendered.contains("{VENV_LOCATION}"));
        }

        #[test]
        fn render_all_with_example_returns_three_files() {
            let opts = ScaffoldOptions {
                requires_python: ">=3.11".into(),
                venv_location: VenvLocation::Cache,
                include_example: true,
            };
            let files = render_all(&opts);
            let names: Vec<_> = files.iter().map(|f| f.relative_path).collect();
            assert_eq!(names, vec!["pyproject.toml", ".gitignore", "example.py"]);
        }

        #[test]
        fn render_all_without_example_returns_two_files() {
            let opts = ScaffoldOptions {
                requires_python: ">=3.11".into(),
                venv_location: VenvLocation::Cache,
                include_example: false,
            };
            let files = render_all(&opts);
            let names: Vec<_> = files.iter().map(|f| f.relative_path).collect();
            assert_eq!(names, vec!["pyproject.toml", ".gitignore"]);
        }

        #[test]
        fn parse_venv_location_accepts_both_known_values() {
            assert_eq!(parse_venv_location("cache").unwrap(), VenvLocation::Cache);
            assert_eq!(parse_venv_location("in-tree").unwrap(), VenvLocation::InTree);
        }

        #[test]
        fn parse_venv_location_rejects_unknown_values() {
            assert!(parse_venv_location("system").is_err());
        }
    }
    ```

- [ ] **Step 2.5: Declare the module in `src/bin/toolr/main.rs`**

    Add to the top of `main.rs`, alphabetically with the other `mod` declarations:

    ```rust
    mod init_templates;
    ```

- [ ] **Step 2.6: Run the unit tests**

    ```bash
    cargo test --bin toolr init_templates::
    ```

    Expected: 5 tests pass.

- [ ] **Step 2.7: Commit**

    ```bash
    git add src/bin/toolr/init_templates.rs src/bin/toolr/init_templates/ src/bin/toolr/main.rs
    git commit -m "feat(init): Add scaffold template files and render helpers"
    ```

---

## Task 3: Example.py content with four ctx-feature commands

Replace the `example.py.tmpl` placeholder with the full four-command example. Keep this in its own task — the content is the user-facing demo, worth a focused commit.

**Files:**

- Modify: `src/bin/toolr/init_templates/example.py.tmpl`

- [ ] **Step 3.1: Replace `src/bin/toolr/init_templates/example.py.tmpl` with the full template**

    Exact content (this is the file users will see after running `toolr project init`):

    ```python
    """Example commands generated by ``toolr project init``.

    Edit or delete this file as you build out your own commands.
    """

    from __future__ import annotations

    from typing import Literal

    from toolr import Context
    from toolr import command_group

    group = command_group(
        "example",
        "Example commands",
        description="Generated by `toolr project init` — edit or delete as you build out your own.",
    )


    @group.command
    def hello(ctx: Context, name: str = "world") -> None:
        """Greet someone.

        Demonstrates the simplest possible toolr command: a function with a
        single keyword argument, a Google-style docstring, and a call to
        ``ctx.print``.

        Args:
            name: The name to greet.
        """
        ctx.print(f"hello, {name}")


    @group.command
    def commit(ctx: Context) -> None:
        """Print the short SHA of the current git HEAD.

        Demonstrates ``ctx.run(..., capture_output=True)`` for capturing
        subprocess output without streaming it to the terminal.
        """
        result = ctx.run("git", "rev-parse", "--short", "HEAD", capture_output=True)
        if result.returncode != 0:
            ctx.exit(result.returncode, "git rev-parse failed")
        sha = result.stdout.read().strip()
        ctx.print(f"current HEAD: {sha}")


    @group.command
    def confirm(ctx: Context) -> None:
        """Prompt for confirmation before doing something destructive.

        Demonstrates ``ctx.prompt(...)`` for interactive input and
        ``ctx.exit(...)`` for early-exit with a non-zero status.
        """
        answer = ctx.prompt("Continue?", expected_type=bool, default=False)
        if not answer:
            ctx.exit(1, "aborted")
        ctx.print("continuing")


    @group.command
    def setlog(
        ctx: Context,
        level: Literal["debug", "info", "warning"] = "info",
    ) -> None:
        """Set a log level (for demonstration only — does nothing).

        Demonstrates ``Literal[...]`` rendering as a ``--level
        {debug,info,warning}`` choice in the generated ``--help`` output.

        Args:
            level: Which log level to use.
        """
        ctx.print(f"log level set to {level}")
    ```

- [ ] **Step 3.2: Sanity-check the rendered example.py parses as Python**

    The example is rendered as a static string; the easiest way to validate it before committing is to write it to a tmp file and `python -c "import ast; ast.parse(open('/tmp/example.py').read())"`. Or use the `cargo test` from Task 2 — it doesn't parse the example yet, so add a fourth test under `init_templates::tests`:

    Append to `src/bin/toolr/init_templates.rs`'s test module:

    ```rust
        #[test]
        fn example_template_is_non_empty_and_mentions_each_command() {
            // Cheap structural check — the integration tests in Task 6
            // actually execute the example. Here we just guard against
            // accidentally truncating the template.
            assert!(EXAMPLE_PY.contains("def hello("));
            assert!(EXAMPLE_PY.contains("def commit("));
            assert!(EXAMPLE_PY.contains("def confirm("));
            assert!(EXAMPLE_PY.contains("def setlog("));
            assert!(EXAMPLE_PY.contains("Literal["));
        }
    ```

- [ ] **Step 3.3: Run the tests**

    ```bash
    cargo test --bin toolr init_templates::
    ```

    Expected: 6 tests pass (5 from Task 2 + 1 new).

- [ ] **Step 3.4: Commit**

    ```bash
    git add src/bin/toolr/init_templates/example.py.tmpl src/bin/toolr/init_templates.rs
    git commit -m "feat(init): Fill in example.py with four ctx-exercising commands"
    ```

---

## Task 4: Scaffolding logic + dispatcher

Implement the real `project_init` function: read CLI args into `ScaffoldOptions`, validate preconditions (refuse if `tools/` exists non-empty without `--force`), atomically write the rendered files, and print success output.

**Files:**

- Create: `src/bin/toolr/init_scaffold.rs`
- Modify: `src/bin/toolr/main.rs`
- Modify: `src/bin/toolr/project.rs`

- [ ] **Step 4.1: Create `src/bin/toolr/init_scaffold.rs`**

    Full module — file-writing logic with rollback on failure:

    ```rust
    //! Scaffold writer for `toolr project init`. Atomically writes the
    //! rendered template files into `<cwd>/tools/`, rolling back any
    //! partial state on failure.

    use std::fs;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use anyhow::{Context as _, Result, anyhow};

    use crate::init_templates::{RenderedFile, render_all, ScaffoldOptions};

    /// Outcome of a successful scaffold.
    #[derive(Debug)]
    pub struct ScaffoldOutcome {
        pub tools_dir: PathBuf,
        pub files_written: Vec<PathBuf>,
    }

    /// Scaffold `tools/` under `cwd`, refusing without `force` if `tools/`
    /// already exists and is non-empty. On partial-write failure, every
    /// file this call created is removed before returning the error.
    pub fn scaffold(cwd: &Path, opts: &ScaffoldOptions, force: bool) -> Result<ScaffoldOutcome> {
        let tools_dir = cwd.join("tools");
        if tools_dir.exists() && !force {
            let mut iter = fs::read_dir(&tools_dir)
                .with_context(|| format!("reading {}", tools_dir.display()))?;
            if iter.next().is_some() {
                return Err(anyhow!(
                    "tools/ already exists at {} (use --force to overwrite)",
                    tools_dir.display()
                ));
            }
        }
        fs::create_dir_all(&tools_dir)
            .with_context(|| format!("creating {}", tools_dir.display()))?;

        let rendered = render_all(opts);
        let mut written: Vec<PathBuf> = Vec::with_capacity(rendered.len());
        for file in &rendered {
            let dest = tools_dir.join(file.relative_path);
            if let Err(e) = write_file(&dest, &file.contents) {
                // Roll back anything we wrote so far.
                for path in written.iter().rev() {
                    let _ = fs::remove_file(path);
                }
                return Err(e).with_context(|| format!("writing {}", dest.display()));
            }
            written.push(dest);
        }
        Ok(ScaffoldOutcome {
            tools_dir,
            files_written: written,
        })
    }

    fn write_file(path: &Path, contents: &str) -> Result<()> {
        // Write via `<name>.tmp` then rename so a crashed half-write never
        // leaves a torn file on disk.
        let tmp = with_extension(path, "tmp");
        {
            let mut f = fs::File::create(&tmp)
                .with_context(|| format!("creating {}", tmp.display()))?;
            f.write_all(contents.as_bytes())?;
            f.flush()?;
        }
        fs::rename(&tmp, path)
            .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
        Ok(())
    }

    fn with_extension(path: &Path, extra: &str) -> PathBuf {
        let mut name = path
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_default();
        name.push(".");
        name.push(extra);
        path.with_file_name(name)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::init_templates::VenvLocation;
        use tempfile::TempDir;

        fn opts() -> ScaffoldOptions {
            ScaffoldOptions {
                requires_python: ">=3.11".into(),
                venv_location: VenvLocation::Cache,
                include_example: true,
            }
        }

        #[test]
        fn scaffold_writes_three_files() {
            let tmp = TempDir::new().unwrap();
            let outcome = scaffold(tmp.path(), &opts(), false).unwrap();
            assert_eq!(outcome.tools_dir, tmp.path().join("tools"));
            assert_eq!(outcome.files_written.len(), 3);
            assert!(tmp.path().join("tools/pyproject.toml").is_file());
            assert!(tmp.path().join("tools/.gitignore").is_file());
            assert!(tmp.path().join("tools/example.py").is_file());
        }

        #[test]
        fn scaffold_without_example_writes_two_files() {
            let tmp = TempDir::new().unwrap();
            let mut o = opts();
            o.include_example = false;
            let outcome = scaffold(tmp.path(), &o, false).unwrap();
            assert_eq!(outcome.files_written.len(), 2);
            assert!(!tmp.path().join("tools/example.py").exists());
        }

        #[test]
        fn scaffold_refuses_when_tools_non_empty_without_force() {
            let tmp = TempDir::new().unwrap();
            let tools = tmp.path().join("tools");
            fs::create_dir(&tools).unwrap();
            fs::write(tools.join("existing.py"), "x = 1").unwrap();

            let err = scaffold(tmp.path(), &opts(), false).expect_err("should refuse");
            assert!(err.to_string().contains("already exists"));
            // The existing file is untouched.
            assert_eq!(fs::read_to_string(tools.join("existing.py")).unwrap(), "x = 1");
        }

        #[test]
        fn scaffold_force_overwrites() {
            let tmp = TempDir::new().unwrap();
            let tools = tmp.path().join("tools");
            fs::create_dir(&tools).unwrap();
            fs::write(tools.join("pyproject.toml"), "# stale").unwrap();

            scaffold(tmp.path(), &opts(), true).unwrap();
            let pyproject = fs::read_to_string(tools.join("pyproject.toml")).unwrap();
            assert!(pyproject.contains(r#"name = "tools""#));
        }

        #[test]
        fn scaffold_accepts_an_empty_tools_dir() {
            let tmp = TempDir::new().unwrap();
            fs::create_dir(tmp.path().join("tools")).unwrap();
            // Empty directory — no `--force` needed.
            scaffold(tmp.path(), &opts(), false).unwrap();
            assert!(tmp.path().join("tools/pyproject.toml").is_file());
        }
    }
    ```

- [ ] **Step 4.2: Declare the module in `src/bin/toolr/main.rs`**

    Add alongside `mod init_templates;`:

    ```rust
    mod init_scaffold;
    ```

- [ ] **Step 4.3: Replace the `project_init` stub in `src/bin/toolr/project.rs`**

    Imports at the top of the file:

    ```rust
    use crate::init_scaffold::scaffold;
    use crate::init_templates::{ScaffoldOptions, parse_venv_location};
    ```

    Replace the existing stub function:

    ```rust
    fn project_init(matches: &ArgMatches) -> Result<ExitCode> {
        let force = matches.get_flag("force");
        let no_sync = matches.get_flag("no-sync");
        let no_example = matches.get_flag("no-example");
        let quiet = matches.get_flag("quiet");
        let venv_location_str = matches
            .get_one::<String>("venv-location")
            .map(String::as_str)
            .unwrap_or("cache");
        let venv_location = parse_venv_location(venv_location_str)?;
        let requires_python = matches
            .get_one::<String>("python")
            .cloned()
            .unwrap_or_else(detect_requires_python);

        let cwd = std::env::current_dir()?;
        let opts = ScaffoldOptions {
            requires_python,
            venv_location,
            include_example: !no_example,
        };
        let outcome = scaffold(&cwd, &opts, force)?;

        if !quiet {
            println!("toolr: scaffolded tools/ at {}", outcome.tools_dir.display());
            for path in &outcome.files_written {
                let rel = path
                    .strip_prefix(&cwd)
                    .unwrap_or(path)
                    .display();
                println!("toolr:   wrote {rel}");
            }
        }

        if no_sync {
            if !quiet {
                println!("toolr: skipping `uv sync` (--no-sync)");
                println!("toolr: run `toolr project deps sync` when you are ready");
            }
            return Ok(ExitCode::SUCCESS);
        }

        // Task 5 wires up the auto-sync; for now, just stop here so the
        // scaffold-only path is testable independently.
        Ok(ExitCode::SUCCESS)
    }

    /// Default `requires-python` value for new projects.
    ///
    /// Matches the running interpreter's `>=major.minor` so the venv that
    /// `ensure_venv_ready` materialises is guaranteed to satisfy it.
    fn detect_requires_python() -> String {
        // Read the running CPython's version via the macros baked in by
        // pyo3 if we have access; otherwise fall back to a conservative
        // floor that matches what the project itself supports.
        // Concretely: probe `python3 --version` once. If absent, fall back
        // to ">=3.11" (the project's own floor per `pyproject.toml`).
        if let Ok(output) = std::process::Command::new("python3")
            .arg("-c")
            .arg("import sys; print(f'>={sys.version_info.major}.{sys.version_info.minor}')")
            .output()
        {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !s.is_empty() {
                    return s;
                }
            }
        }
        ">=3.11".to_string()
    }
    ```

- [ ] **Step 4.4: Run the scaffold tests**

    ```bash
    cargo test --bin toolr init_scaffold::
    ```

    Expected: 5 tests pass.

- [ ] **Step 4.5: Smoke-check the binary**

    Build and run against a tmpdir to verify the end-to-end happy path:

    ```bash
    cargo build --bin toolr
    tmp=$(mktemp -d) && (cd "$tmp" && /path/to/repo/target/debug/toolr project init --no-sync --quiet && ls tools/) && rm -rf "$tmp"
    ```

    Expected output: `pyproject.toml`, `.gitignore`, `example.py`.

- [ ] **Step 4.6: Commit**

    ```bash
    git add src/bin/toolr/init_scaffold.rs src/bin/toolr/main.rs src/bin/toolr/project.rs
    git commit -m "feat(init): Scaffold tools/ with atomic file writes and refuse-without-force"
    ```

---

## Task 5: Wire `ensure_venv_ready` for the auto-sync path

Hook the post-scaffold step into the existing `_rust_utils::project::ensure_venv_ready`. The default is sync-after-scaffold; `--no-sync` skips it.

**Files:**

- Modify: `src/bin/toolr/project.rs`

- [ ] **Step 5.1: Extend `project_init` with the sync step**

    Replace the `if no_sync { ... } return Ok(ExitCode::SUCCESS); }` block + the trailing `Ok(ExitCode::SUCCESS)` line from Task 4 with:

    ```rust
    if no_sync {
        if !quiet {
            println!("toolr: skipping `uv sync` (--no-sync)");
            println!("toolr: run `toolr project deps sync` when you are ready");
        }
        return Ok(ExitCode::SUCCESS);
    }

    // Auto-sync — same path as `toolr project deps sync`.
    let consent = _rust_utils::uv::install::ConsentMode::from_env();
    let (resolved, uv) =
        _rust_utils::project::ensure_venv_ready(&cwd, consent, /*force_sync=*/ true)?;
    if !quiet {
        println!(
            "toolr: synced venv at {} using uv {}.{}.{}",
            resolved.venv_dir.display(),
            uv.version.0,
            uv.version.1,
            uv.version.2,
        );
        println!("toolr:");
        println!("toolr: next steps:");
        println!("toolr:   toolr example hello");
        println!("toolr:   toolr example commit");
        println!("toolr:   toolr self completion install <bash|zsh|fish>   # optional, for tab completion");
    }
    Ok(ExitCode::SUCCESS)
    ```

- [ ] **Step 5.2: Build + smoke**

    ```bash
    cargo build --bin toolr
    tmp=$(mktemp -d) && (cd "$tmp" && /path/to/repo/target/debug/toolr project init --no-sync --quiet && ls tools/) && rm -rf "$tmp"
    ```

    Expected: same as Task 4.5 (still works with `--no-sync`). The full sync path is exercised by the integration tests in Task 6 (it requires `uv` to be installed).

- [ ] **Step 5.3: Commit**

    ```bash
    git add src/bin/toolr/project.rs
    git commit -m "feat(init): Auto-run uv sync after scaffolding (skippable via --no-sync)"
    ```

---

## Task 6: Integration tests in `tests/project_init.rs`

End-to-end test the new subcommand via `assert_cmd`. Cover refuse / force / no-sync / no-example / run-example. The full sync-then-run-example test is gated on a usable Python being available (matches the existing `running_a_user_command_invokes_python_runner` pattern from `tests/cli_smoke.rs`).

**Files:**

- Create: `tests/project_init.rs`

- [ ] **Step 6.1: Create `tests/project_init.rs`**

    Full content:

    ```rust
    //! Integration tests for `toolr project init`.

    use std::fs;
    use std::path::PathBuf;

    use assert_cmd::Command;
    use tempfile::TempDir;

    fn cargo_bin() -> Command {
        Command::cargo_bin("toolr").unwrap()
    }

    /// Returns `Some(path-to-python)` if a Python interpreter with `toolr`
    /// installed is reachable; otherwise `None`. The sync-then-run test
    /// skips when no such interpreter is available.
    fn detect_test_python() -> Option<PathBuf> {
        let candidate = std::env::var_os("TOOLR_TEST_PYTHON").map(PathBuf::from);
        let candidate = candidate.or_else(|| {
            let p = PathBuf::from(".venv/bin/python");
            if p.exists() { Some(p) } else { None }
        })?;
        let python = if candidate.is_absolute() {
            candidate
        } else {
            std::env::current_dir().ok()?.join(candidate)
        };
        let status = std::process::Command::new(&python)
            .args(["-c", "import toolr._runner"])
            .status()
            .ok()?;
        if status.success() { Some(python) } else { None }
    }

    #[test]
    fn init_no_sync_writes_three_files() {
        let tmp = TempDir::new().unwrap();
        cargo_bin()
            .current_dir(tmp.path())
            .args(["project", "init", "--no-sync", "--quiet"])
            .assert()
            .success();
        assert!(tmp.path().join("tools/pyproject.toml").is_file());
        assert!(tmp.path().join("tools/.gitignore").is_file());
        assert!(tmp.path().join("tools/example.py").is_file());
        // No __init__.py — PEP 420 namespace package.
        assert!(!tmp.path().join("tools/__init__.py").exists());
    }

    #[test]
    fn init_no_example_skips_example_py() {
        let tmp = TempDir::new().unwrap();
        cargo_bin()
            .current_dir(tmp.path())
            .args(["project", "init", "--no-sync", "--no-example", "--quiet"])
            .assert()
            .success();
        assert!(tmp.path().join("tools/pyproject.toml").is_file());
        assert!(!tmp.path().join("tools/example.py").exists());
    }

    #[test]
    fn init_refuses_when_tools_already_non_empty() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("tools")).unwrap();
        fs::write(tmp.path().join("tools/existing.py"), "x = 1").unwrap();

        let output = cargo_bin()
            .current_dir(tmp.path())
            .args(["project", "init", "--no-sync", "--quiet"])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("already exists"), "stderr:\n{stderr}");
        // Existing content untouched.
        assert_eq!(
            fs::read_to_string(tmp.path().join("tools/existing.py")).unwrap(),
            "x = 1"
        );
        assert!(!tmp.path().join("tools/pyproject.toml").exists());
    }

    #[test]
    fn init_force_overwrites_existing_tools() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("tools")).unwrap();
        fs::write(tmp.path().join("tools/pyproject.toml"), "# stale").unwrap();

        cargo_bin()
            .current_dir(tmp.path())
            .args(["project", "init", "--no-sync", "--force", "--quiet"])
            .assert()
            .success();
        let pyproject = fs::read_to_string(tmp.path().join("tools/pyproject.toml")).unwrap();
        assert!(pyproject.contains(r#"name = "tools""#));
    }

    #[test]
    fn init_in_tree_writes_correct_venv_location() {
        let tmp = TempDir::new().unwrap();
        cargo_bin()
            .current_dir(tmp.path())
            .args([
                "project",
                "init",
                "--no-sync",
                "--venv-location",
                "in-tree",
                "--quiet",
            ])
            .assert()
            .success();
        let pyproject = fs::read_to_string(tmp.path().join("tools/pyproject.toml")).unwrap();
        assert!(pyproject.contains(r#"venv-location = "in-tree""#));
    }

    #[test]
    fn init_example_has_all_four_commands() {
        let tmp = TempDir::new().unwrap();
        cargo_bin()
            .current_dir(tmp.path())
            .args(["project", "init", "--no-sync", "--quiet"])
            .assert()
            .success();
        let example = fs::read_to_string(tmp.path().join("tools/example.py")).unwrap();
        assert!(example.contains("def hello("));
        assert!(example.contains("def commit("));
        assert!(example.contains("def confirm("));
        assert!(example.contains("def setlog("));
        assert!(example.contains("Literal["));
    }

    /// End-to-end: scaffold + run `toolr example hello` against the result.
    /// Skipped if no usable Python is available (CI sets `TOOLR_TEST_PYTHON`
    /// or `.venv/bin/python` exists).
    #[test]
    fn init_then_run_example_hello() {
        let Some(python) = detect_test_python() else {
            eprintln!("skipping: no .venv/bin/python with toolr installed");
            return;
        };
        let tmp = TempDir::new().unwrap();
        // Scaffold with --no-sync (the test python already has toolr
        // installed; we don't want this test to depend on `uv`).
        cargo_bin()
            .current_dir(tmp.path())
            .args(["project", "init", "--no-sync", "--quiet"])
            .assert()
            .success();

        let output = cargo_bin()
            .current_dir(tmp.path())
            .env("TOOLR_PYTHON", &python)
            .env("PYTHONPATH", tmp.path())
            .args(["example", "hello", "--name", "Plan10"])
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "expected success, got {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
            output.status.code()
        );
        assert!(stdout.contains("hello, Plan10"), "stdout was:\n{stdout}");
    }
    ```

    The test uses the same `TOOLR_PYTHON` env-var override that `cli_smoke.rs::running_a_user_command_invokes_python_runner` uses for the legacy-fallback path. Plan 2's dispatch reads `TOOLR_PYTHON` when there is no `tools/pyproject.toml` — but the init test scaffolds a `tools/pyproject.toml`, so dispatch will try to resolve a real tools venv unless we bypass that.

    **Important:** the scaffolded `tools/pyproject.toml` makes the dispatcher take the venv path, which requires `uv`. For the test we either need to (a) install uv in CI, or (b) delete the `pyproject.toml` after scaffolding so dispatch falls back to `TOOLR_PYTHON`. Option (b) is simpler:

    Adjust `init_then_run_example_hello` to delete the pyproject after scaffolding:

    ```rust
        fs::remove_file(tmp.path().join("tools/pyproject.toml")).unwrap();
    ```

    Insert this line immediately after the `cargo_bin().current_dir(...).args(["project", "init", ...]).assert().success();` call but before the second `cargo_bin()` invocation. The scaffolded pyproject removal makes dispatch take the legacy `TOOLR_PYTHON` path. Add a comment explaining why.

- [ ] **Step 6.2: Run the tests**

    ```bash
    cargo test --test project_init
    ```

    Expected: 7 tests; the `init_then_run_example_hello` test passes if `.venv/bin/python` is set up (post `uv sync`), or self-skips otherwise. The other 6 tests always pass.

- [ ] **Step 6.3: Commit**

    ```bash
    git add tests/project_init.rs
    git commit -m "test(init): End-to-end integration tests for toolr project init"
    ```

---

## Task 7: Update the roadmap

Mark Plan 10 as Done once Tasks 1–6 are merged.

**Files:**

- Modify: `specs/rust-front-end/01-roadmap.md`

- [ ] **Step 7.1: Flip the Plan 10 status**

    Change Plan 10's `**Status:**` line from `🔧 In Progress` to `✅ Done`. Leave the rest of the entry intact.

- [ ] **Step 7.2: Commit**

    ```bash
    git add specs/rust-front-end/01-roadmap.md
    git commit -m "docs(roadmap): Mark Plan 10 as done"
    ```

---

## Done criteria

Plan 10 is complete when:

- `toolr project init` runs in an empty directory and produces `tools/pyproject.toml`, `tools/.gitignore`, and `tools/example.py` (the latter unless `--no-example`).
- No `tools/__init__.py` is generated.
- `toolr project init` refuses (exit code non-zero) when `tools/` already exists and is non-empty, unless `--force`.
- `--force` overwrites existing files.
- `--venv-location in-tree` writes `venv-location = "in-tree"` into the generated `pyproject.toml`.
- `--no-sync` scaffolds without running `uv sync`.
- The default path (no `--no-sync`) runs `ensure_venv_ready` after scaffolding.
- `cargo test --bin toolr init_templates:: init_scaffold::` passes.
- `cargo test --test project_init` passes (with the run-example test self-skipping on hosts without a usable Python).
- The roadmap shows Plan 10 as `✅ Done`.

## Open questions (for the implementer)

1. **`requires-python` detection.** Task 4 detects via `python3 --version`. If no Python is on PATH, falls back to `>=3.11`. Acceptable for v1; revisit if users hit it on bare-Windows machines.
2. **Atomic-write tmp suffix.** `<file>.tmp` is human-readable but could collide with a real file. If that proves a problem, switch to `tempfile::NamedTempFile::new_in(...)`.
3. **`detect_requires_python` cross-platform.** Reads from a child `python3`. Windows shells without `python3` on PATH (only `python.exe`) fall back to the hard-coded floor. Document as a known limitation, or extend the probe to also try `python` / `py -3`.
