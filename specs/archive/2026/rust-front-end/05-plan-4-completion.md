<!-- rumdl-disable MD046 MD076 -->

# Plan 4: Shell Completion

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Lint:** Plan docs nest fenced code inside list items for step-by-step
> structure. The `<!-- rumdl-disable MD046 MD076 -->` directive above turns
> off the code-block-style and list-item-spacing rules for this file only.

**Goal:** Make `toolr` tab-completion work in bash, zsh, and fish. At the end,
typing `toolr <Tab>` in a shell offers user-defined groups, `toolr <group>
<Tab>` offers commands, and `toolr <group> <command> --<Tab>` offers argument
names plus enum/`Literal` values — all in under 50 ms cold, under 10 ms warm,
even if `tools/*.py` has changed since the manifest was last committed.

**Architecture:** Follow the kubectl / gh / cargo / uv "static script delegates
to binary" pattern. Tiny shell-installed scripts (one per shell) call a hidden
top-level `toolr __complete <cwd> <args...>` endpoint on every Tab press. The
endpoint walks up from `<cwd>` to find `tools/`, hashes `tools/**/*.py`,
compares against the cached `manifest.static_hash`, and either serves
completions from the cached manifest (sub-millisecond) or re-parses on the fly
via Plan 1's `build_static_manifest` (sub-50 ms typical). Dynamic-layer
manifest entries are served from the cache regardless of freshness — staleness
in that layer is corrected at execute time, not Tab time.

The completion script lives **inside** the Rust binary as embedded strings
(`include_str!`). `toolr self completion print [shell]` writes it to stdout
and `toolr self completion install [shell]` writes it to the standard location
for the chosen shell.

**Tech Stack:** Already-present deps from Plan 1 (`clap`, `anyhow`,
`thiserror`, `serde_json`, `walkdir`, `blake3`, `ruff_python_parser`,
`assert_cmd`, `tempfile`). No new runtime crates required. `clap_complete`
is intentionally **not** added: hand-written shell snippets are simpler than
the clap_complete builder for a single hidden endpoint, and the manifest is
data-driven so a generated static script would only cover the binary's
built-in subcommands anyway.

**Reading order in this plan:** Tasks build on each other. The pure completion
core (Tasks 1-2) is exercised directly by unit tests; the Tab-time freshness
logic (Task 3) wraps that core; the CLI surface (Tasks 4-8) and integration
tests (Task 9) come last.

---

## Task 1: Skeleton `complete` module

Stand up an empty `_rust_utils::complete` module with a public API surface
that later tasks fill in. This task adds no behaviour — it carves the shape
so subsequent commits land small.

**Files:**

- Create: `src/complete/mod.rs`

- Create: `src/complete/tests.rs`

- Modify: `src/lib.rs`

- [x] **Step 1.1: Expose the new module from `src/lib.rs`**

    Append to `src/lib.rs` (preserving alphabetical order with the existing
    `manifest`, `discovery`, `hash`, `parser` declarations):

    ```rust
    pub mod complete;
    ```

- [x] **Step 1.2: Create `src/complete/mod.rs`**

    ```rust
    //! Shell-completion engine.
    //!
    //! Backs the hidden `toolr __complete <cwd> <args...>` endpoint that
    //! shell completion scripts shell out to on every Tab press. The engine
    //! is split into three concerns:
    //!
    //! 1. [`serve_completions`] — pure prefix-matching against a loaded
    //!    `Manifest`. No I/O.
    //! 2. [`resolve_manifest_at_tab`] — Tab-time freshness check that loads
    //!    the cached manifest, compares its `static_hash` against the live
    //!    `tools/**/*.py` hash, and either returns the cached manifest or a
    //!    fresh one built by [`crate::parser::build_static_manifest`].
    //! 3. [`scripts`] — embedded shell-completion scripts (bash, zsh, fish).

    pub mod engine;
    pub mod freshness;
    pub mod scripts;

    pub use engine::serve_completions;
    pub use freshness::{resolve_manifest_at_tab, ResolvedManifest};
    pub use scripts::{Shell, completion_script};

    #[cfg(test)]
    mod tests;
    ```

- [x] **Step 1.3: Create placeholder `engine`, `freshness`, `scripts` files**

    Create `src/complete/engine.rs`:

    ```rust
    //! Pure prefix-matching completion engine. No I/O.

    use crate::manifest::Manifest;

    /// Compute the list of completion candidates for a fully tokenised
    /// command line. `tokens` is everything after `toolr` itself — e.g.
    /// `["ci", "hello", "--na"]`. Returns one candidate per line in shell
    /// output.
    pub fn serve_completions(_manifest: &Manifest, _tokens: &[String]) -> Vec<String> {
        // Filled in by Task 2.
        Vec::new()
    }
    ```

    Create `src/complete/freshness.rs`:

    ```rust
    //! Tab-time manifest freshness logic.

    use std::path::PathBuf;

    use crate::manifest::Manifest;

    /// Outcome of resolving the manifest for a completion request.
    pub struct ResolvedManifest {
        pub manifest: Manifest,
        /// `true` if the cached on-disk manifest matched the live tools hash.
        pub from_cache: bool,
        /// The directory that contained `tools/` (the project root).
        pub project_root: PathBuf,
    }

    /// Resolve the manifest to serve for a completion request rooted at
    /// `cwd`. Filled in by Task 3.
    pub fn resolve_manifest_at_tab(_cwd: &std::path::Path) -> anyhow::Result<ResolvedManifest> {
        anyhow::bail!("resolve_manifest_at_tab not implemented yet")
    }
    ```

    Create `src/complete/scripts.rs`:

    ```rust
    //! Embedded shell-completion scripts.

    use std::fmt;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Shell {
        Bash,
        Zsh,
        Fish,
    }

    impl fmt::Display for Shell {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(match self {
                Shell::Bash => "bash",
                Shell::Zsh => "zsh",
                Shell::Fish => "fish",
            })
        }
    }

    /// Return the static completion script for the given shell. Filled in
    /// by Tasks 5-7.
    pub fn completion_script(_shell: Shell) -> &'static str {
        ""
    }
    ```

- [x] **Step 1.4: Create `src/complete/tests.rs` as a placeholder**

    ```rust
    //! Cross-module tests for the completion engine land here as later
    //! tasks fill in real behaviour. Initially empty.
    ```

- [x] **Step 1.5: Verify the crate still builds**

    ```bash
    cargo build --lib
    ```

    Expected: success with warnings for unused functions (these go away in
    Task 2).

- [x] **Step 1.6: Commit**

    ```bash
    git add src/lib.rs src/complete/
    git commit -m "feat(complete): Scaffold shell-completion module"
    ```

---

## Task 2: `serve_completions` core (prefix-match engine)

Implement pure prefix-matching against the manifest's groups, commands,
arguments, and per-argument allowed values. No I/O. Drives the entire
completion endpoint.

**Semantics for `tokens` (everything after `toolr`):**

- `[]` or `["<partial-group>"]` → complete to group names.
- `["<group>"]` (exact match) → still complete groups (user hasn't hit space
  yet from their shell's perspective). The shell-side scripts handle the
  distinction via the trailing-space convention (see Task 5).
- `["<group>", "<partial-cmd>"]` → complete to commands in that group.
- `["<group>", "<command>", "--<partial>"]` → complete to argument flag
  names. Positional arguments are not flag-completed.
- `["<group>", "<command>", "--<flag>", "<partial-value>"]` → if the flag's
  argument has a non-empty `allowed_values`, complete from that set;
  otherwise return an empty list (the shell falls back to its own default,
  typically filename completion).
- `["<group>", "<command>", "<partial-positional>"]` → if a positional
  argument is expected at that index and has `allowed_values`, complete
  from that set; otherwise empty.

**Files:**

- Modify: `src/complete/engine.rs`

- Modify: `src/complete/tests.rs`

- [x] **Step 2.1: Write the failing tests in `src/complete/tests.rs`**

    Replace the placeholder with:

    ```rust
    use crate::complete::serve_completions;
    use crate::manifest::{
        Argument, ArgumentKind, Command, Group, Manifest, Origin, SCHEMA_VERSION,
    };

    fn fixture() -> Manifest {
        Manifest {
            schema_version: SCHEMA_VERSION,
            static_hash: "h".into(),
            dynamic_hash: String::new(),
            groups: vec![
                Group {
                    name: "ci".into(),
                    title: "CI utilities".into(),
                    description: String::new(),
                    origin: Origin::Static,
                },
                Group {
                    name: "data".into(),
                    title: "Data utilities".into(),
                    description: String::new(),
                    origin: Origin::Static,
                },
            ],
            commands: vec![
                Command {
                    name: "hello".into(),
                    group: "ci".into(),
                    module: "tools.ci".into(),
                    function: "hello".into(),
                    summary: "Say hello.".into(),
                    description: String::new(),
                    arguments: vec![Argument {
                        name: "name".into(),
                        kind: ArgumentKind::Optional,
                        help: "Who to greet".into(),
                        default: Some("\"world\"".into()),
                        type_annotation: Some("str".into()),
                        allowed_values: vec![],
                    }],
                    imports: vec![],
                    origin: Origin::Static,
                },
                Command {
                    name: "deploy".into(),
                    group: "ci".into(),
                    module: "tools.ci".into(),
                    function: "deploy".into(),
                    summary: "Deploy something.".into(),
                    description: String::new(),
                    arguments: vec![Argument {
                        name: "env".into(),
                        kind: ArgumentKind::Optional,
                        help: "Target env".into(),
                        default: None,
                        type_annotation: Some("Literal".into()),
                        allowed_values: vec!["staging".into(), "production".into()],
                    }],
                    imports: vec![],
                    origin: Origin::Static,
                },
                Command {
                    name: "load".into(),
                    group: "data".into(),
                    module: "tools.data".into(),
                    function: "load".into(),
                    summary: "Load data.".into(),
                    description: String::new(),
                    arguments: vec![Argument {
                        name: "shape".into(),
                        kind: ArgumentKind::Positional,
                        help: "Shape".into(),
                        default: None,
                        type_annotation: Some("Literal".into()),
                        allowed_values: vec!["wide".into(), "tall".into()],
                    }],
                    imports: vec![],
                    origin: Origin::Static,
                },
            ],
        }
    }

    fn tokens(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn empty_tokens_lists_all_groups() {
        let out = serve_completions(&fixture(), &tokens(&[""]));
        assert_eq!(out, vec!["ci".to_string(), "data".to_string()]);
    }

    #[test]
    fn group_prefix_filters_groups() {
        let out = serve_completions(&fixture(), &tokens(&["c"]));
        assert_eq!(out, vec!["ci".to_string()]);
    }

    #[test]
    fn after_group_lists_its_commands() {
        let out = serve_completions(&fixture(), &tokens(&["ci", ""]));
        assert_eq!(out, vec!["deploy".to_string(), "hello".to_string()]);
    }

    #[test]
    fn command_prefix_filters_commands() {
        let out = serve_completions(&fixture(), &tokens(&["ci", "h"]));
        assert_eq!(out, vec!["hello".to_string()]);
    }

    #[test]
    fn flag_prefix_lists_argument_flags() {
        let out = serve_completions(&fixture(), &tokens(&["ci", "hello", "--"]));
        assert_eq!(out, vec!["--name".to_string()]);
    }

    #[test]
    fn flag_value_completes_to_allowed_values() {
        let out = serve_completions(&fixture(), &tokens(&["ci", "deploy", "--env", ""]));
        assert_eq!(out, vec!["production".to_string(), "staging".to_string()]);
    }

    #[test]
    fn flag_value_partial_filters_allowed_values() {
        let out = serve_completions(&fixture(), &tokens(&["ci", "deploy", "--env", "s"]));
        assert_eq!(out, vec!["staging".to_string()]);
    }

    #[test]
    fn positional_value_completes_to_allowed_values() {
        let out = serve_completions(&fixture(), &tokens(&["data", "load", ""]));
        assert_eq!(out, vec!["tall".to_string(), "wide".to_string()]);
    }

    #[test]
    fn unknown_group_returns_no_completions() {
        let out = serve_completions(&fixture(), &tokens(&["nope", ""]));
        assert!(out.is_empty());
    }

    #[test]
    fn flag_without_allowed_values_returns_empty() {
        // `--name` has no allowed_values → shell falls back to filename completion.
        let out = serve_completions(&fixture(), &tokens(&["ci", "hello", "--name", ""]));
        assert!(out.is_empty());
    }
    ```

- [x] **Step 2.2: Run the tests and confirm they fail**

    ```bash
    cargo test --lib complete::tests::
    ```

    Expected: every test fails because `serve_completions` returns `vec![]`.

- [x] **Step 2.3: Implement the engine in `src/complete/engine.rs`**

    ```rust
    //! Pure prefix-matching completion engine. No I/O.

    use crate::manifest::{Argument, ArgumentKind, Command, Manifest};

    /// Compute the list of completion candidates for a tokenised command
    /// line. `tokens` is everything after `toolr` itself — for example
    /// `["ci", "hello", "--na"]`. The last token is treated as the
    /// in-progress word and is matched as a prefix; earlier tokens are
    /// matched exactly.
    ///
    /// The returned vector is alphabetically sorted and deduplicated.
    pub fn serve_completions(manifest: &Manifest, tokens: &[String]) -> Vec<String> {
        let mut out = match classify(manifest, tokens) {
            Slot::Group { prefix } => groups(manifest, &prefix),
            Slot::Command { group, prefix } => commands(manifest, &group, &prefix),
            Slot::Flag { command, prefix } => flags(command, &prefix),
            Slot::FlagValue { argument, prefix } => values(argument, &prefix),
            Slot::Positional { argument, prefix } => values(argument, &prefix),
            Slot::None => Vec::new(),
        };
        out.sort();
        out.dedup();
        out
    }

    enum Slot<'a> {
        Group {
            prefix: String,
        },
        Command {
            group: String,
            prefix: String,
        },
        Flag {
            command: &'a Command,
            prefix: String,
        },
        FlagValue {
            argument: &'a Argument,
            prefix: String,
        },
        Positional {
            argument: &'a Argument,
            prefix: String,
        },
        None,
    }

    fn classify<'a>(manifest: &'a Manifest, tokens: &[String]) -> Slot<'a> {
        // The last token is the in-progress word; anything earlier is
        // considered "committed". An empty `tokens` slice is treated as a
        // single empty token (the user just typed `toolr <Tab>`).
        if tokens.is_empty() {
            return Slot::Group {
                prefix: String::new(),
            };
        }
        let prefix = tokens.last().cloned().unwrap_or_default();
        let committed = &tokens[..tokens.len() - 1];

        // No committed tokens → completing the group name.
        if committed.is_empty() {
            return Slot::Group { prefix };
        }

        // First committed token is the group.
        let group_name = &committed[0];
        let Some(_group) = manifest.groups.iter().find(|g| &g.name == group_name) else {
            return Slot::None;
        };

        // One committed token (the group) → completing the command name.
        if committed.len() == 1 {
            return Slot::Command {
                group: group_name.clone(),
                prefix,
            };
        }

        // Two+ committed tokens → group, command, then args.
        let command_name = &committed[1];
        let Some(command) = manifest
            .commands
            .iter()
            .find(|c| &c.group == group_name && &c.name == command_name)
        else {
            return Slot::None;
        };

        // From committed[2..], figure out what argument we're inside.
        let arg_tokens = &committed[2..];

        // If the previous committed token was a `--flag`, we're completing
        // that flag's value.
        if let Some(prev) = arg_tokens.last() {
            if let Some(flag_name) = prev.strip_prefix("--") {
                if let Some(arg) = command.arguments.iter().find(|a| a.name == flag_name) {
                    if !matches!(arg.kind, ArgumentKind::Flag) {
                        return Slot::FlagValue {
                            argument: arg,
                            prefix,
                        };
                    }
                }
            }
        }

        // Otherwise: if the in-progress word starts with `--`, complete to a
        // flag name. If not, treat it as the next positional value.
        if prefix.starts_with("--") || prefix == "-" {
            return Slot::Flag {
                command,
                prefix,
            };
        }

        // Positional path: count how many positional values have already
        // been provided in `arg_tokens` (skipping `--flag value` pairs and
        // bare `--flag` boolean flags) and pick the matching Argument.
        let positional_index = count_positionals_consumed(command, arg_tokens);
        let positional_args: Vec<&Argument> = command
            .arguments
            .iter()
            .filter(|a| matches!(a.kind, ArgumentKind::Positional))
            .collect();
        if let Some(&arg) = positional_args.get(positional_index) {
            return Slot::Positional {
                argument: arg,
                prefix,
            };
        }

        Slot::None
    }

    fn count_positionals_consumed(command: &Command, arg_tokens: &[String]) -> usize {
        let mut idx = 0usize;
        let mut i = 0usize;
        while i < arg_tokens.len() {
            let t = &arg_tokens[i];
            if let Some(flag_name) = t.strip_prefix("--") {
                if let Some(arg) = command.arguments.iter().find(|a| a.name == flag_name) {
                    if matches!(arg.kind, ArgumentKind::Flag) {
                        i += 1;
                        continue;
                    }
                    // --flag value pair
                    i += 2;
                    continue;
                }
                // Unknown flag — skip just the token.
                i += 1;
                continue;
            }
            idx += 1;
            i += 1;
        }
        idx
    }

    fn groups(manifest: &Manifest, prefix: &str) -> Vec<String> {
        manifest
            .groups
            .iter()
            .map(|g| g.name.clone())
            .filter(|name| name.starts_with(prefix))
            .collect()
    }

    fn commands(manifest: &Manifest, group: &str, prefix: &str) -> Vec<String> {
        manifest
            .commands
            .iter()
            .filter(|c| c.group == group)
            .map(|c| c.name.clone())
            .filter(|name| name.starts_with(prefix))
            .collect()
    }

    fn flags(command: &Command, prefix: &str) -> Vec<String> {
        command
            .arguments
            .iter()
            .filter(|a| !matches!(a.kind, ArgumentKind::Positional))
            .map(|a| format!("--{}", a.name))
            .filter(|flag| flag.starts_with(prefix))
            .collect()
    }

    fn values(argument: &Argument, prefix: &str) -> Vec<String> {
        argument
            .allowed_values
            .iter()
            .filter(|v| v.starts_with(prefix))
            .cloned()
            .collect()
    }
    ```

- [x] **Step 2.4: Run the tests, expect all PASS**

    ```bash
    cargo test --lib complete::
    ```

    Expected: 10 tests passing.

- [x] **Step 2.5: Commit**

    ```bash
    git add src/complete/engine.rs src/complete/tests.rs
    git commit -m "feat(complete): Prefix-match groups, commands, flags, and allowed values"
    ```

---

## Task 3: Tab-time freshness logic

Wire `resolve_manifest_at_tab` to (1) walk up from `cwd` to find `tools/`,
(2) hash `tools/**/*.py`, (3) load the cached manifest and compare hashes,
(4) fall back to `build_static_manifest` on mismatch or absent cache. Dynamic
entries from the cached manifest are kept even when the static layer is
re-parsed — the merge is "static slots from fresh parse, dynamic slots from
cache".

**Files:**

- Modify: `src/complete/freshness.rs`

- Modify: `src/complete/tests.rs`

- [x] **Step 3.1: Write the failing tests**

    Append to `src/complete/tests.rs`:

    ```rust
    use crate::complete::{resolve_manifest_at_tab, ResolvedManifest};
    use crate::manifest::{load_manifest, write_manifest, Origin};
    use tempfile::TempDir;

    fn make_tree(py_files: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("tools")).unwrap();
        for (name, contents) in py_files {
            let path = tmp.path().join("tools").join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }
        tmp
    }

    #[test]
    fn returns_fresh_manifest_when_no_cache_exists() {
        let tmp = make_tree(&[(
            "ci.py",
            "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
        )]);
        let ResolvedManifest { manifest, from_cache, project_root } =
            resolve_manifest_at_tab(tmp.path()).unwrap();
        assert!(!from_cache, "no cache file existed");
        assert_eq!(project_root, tmp.path());
        assert!(manifest.groups.iter().any(|g| g.name == "ci"));
        assert!(manifest.commands.iter().any(|c| c.name == "hello"));
    }

    #[test]
    fn returns_cached_manifest_when_hash_matches() {
        let tmp = make_tree(&[(
            "ci.py",
            "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
        )]);
        // Build once and write to disk.
        let built = crate::parser::build_static_manifest(&tmp.path().join("tools")).unwrap();
        let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
        write_manifest(&manifest_path, &built).unwrap();

        let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
        assert!(resolved.from_cache);
        assert_eq!(resolved.manifest, built);
    }

    #[test]
    fn re_parses_when_cached_hash_is_stale() {
        let tmp = make_tree(&[(
            "ci.py",
            "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
        )]);
        // Write a stale manifest with a bogus hash.
        let mut stale = crate::parser::build_static_manifest(&tmp.path().join("tools")).unwrap();
        stale.static_hash = "deliberately-stale".into();
        let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
        write_manifest(&manifest_path, &stale).unwrap();

        let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
        assert!(!resolved.from_cache, "stale hash should trigger reparse");
        assert_ne!(resolved.manifest.static_hash, "deliberately-stale");
    }

    #[test]
    fn preserves_dynamic_entries_from_cache_when_reparsing() {
        let tmp = make_tree(&[(
            "ci.py",
            "group = command_group(\"ci\", \"CI utilities\")\n\n@group.command\ndef hello(ctx):\n    pass\n",
        )]);
        // Seed a manifest with a fake dynamic command and a stale static_hash
        // so the re-parse path runs.
        let mut seeded = crate::parser::build_static_manifest(&tmp.path().join("tools")).unwrap();
        seeded.static_hash = "stale".into();
        seeded.commands.push(crate::manifest::Command {
            name: "from-plugin".into(),
            group: "dyn-group".into(),
            module: "third_party_pkg".into(),
            function: "from_plugin".into(),
            summary: String::new(),
            description: String::new(),
            arguments: vec![],
            imports: vec![],
            origin: Origin::Dynamic,
        });
        seeded.groups.push(crate::manifest::Group {
            name: "dyn-group".into(),
            title: "Dynamic group".into(),
            description: String::new(),
            origin: Origin::Dynamic,
        });
        let manifest_path = tmp.path().join("tools").join(".toolr-manifest.json");
        write_manifest(&manifest_path, &seeded).unwrap();

        let resolved = resolve_manifest_at_tab(tmp.path()).unwrap();
        assert!(!resolved.from_cache);
        // Static-layer entry survives.
        assert!(resolved.manifest.commands.iter().any(|c| c.name == "hello"));
        // Dynamic-layer entry from the cache is preserved through the reparse.
        assert!(resolved
            .manifest
            .commands
            .iter()
            .any(|c| c.name == "from-plugin" && matches!(c.origin, Origin::Dynamic)));
        assert!(resolved
            .manifest
            .groups
            .iter()
            .any(|g| g.name == "dyn-group" && matches!(g.origin, Origin::Dynamic)));
    }

    #[test]
    fn errors_when_no_tools_dir_exists() {
        let tmp = TempDir::new().unwrap();
        let err = resolve_manifest_at_tab(tmp.path()).expect_err("no tools/");
        let msg = err.to_string();
        assert!(msg.contains("tools"), "expected hint about tools/, got: {msg}");
    }
    ```

    Note: the `load_manifest` import is unused in these tests — it's listed
    in the `use` block so later ad-hoc test additions don't have to edit
    the imports. Drop it if `cargo check` warns and you don't need it.

- [x] **Step 3.2: Implement `resolve_manifest_at_tab` in `src/complete/freshness.rs`**

    ```rust
    //! Tab-time manifest freshness logic.

    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};

    use crate::discovery::discover_project_root;
    use crate::hash::hash_tools_dir;
    use crate::manifest::{load_manifest, Manifest, Origin};
    use crate::parser::build_static_manifest;

    /// Outcome of resolving the manifest for a completion request.
    pub struct ResolvedManifest {
        pub manifest: Manifest,
        /// `true` if the cached on-disk manifest matched the live tools hash.
        pub from_cache: bool,
        /// The directory that contained `tools/` (the project root).
        pub project_root: PathBuf,
    }

    /// Resolve the manifest to serve for a completion request rooted at
    /// `cwd`. Walks up to find `tools/`, hashes its `*.py` files, and either
    /// returns the cached manifest verbatim or re-parses and returns a fresh
    /// one (with any dynamic-layer entries from the cache preserved).
    pub fn resolve_manifest_at_tab(cwd: &Path) -> Result<ResolvedManifest> {
        let project_root = discover_project_root(cwd)
            .with_context(|| format!("walking up from {} to find tools/", cwd.display()))?;
        let tools_dir = project_root.join("tools");
        let manifest_path = tools_dir.join(".toolr-manifest.json");

        let live_hash =
            hash_tools_dir(&tools_dir).with_context(|| format!("hashing {}", tools_dir.display()))?;
        let cached = load_manifest(&manifest_path).ok();

        if let Some(cached) = cached.as_ref() {
            if cached.static_hash == live_hash {
                return Ok(ResolvedManifest {
                    manifest: cached.clone(),
                    from_cache: true,
                    project_root,
                });
            }
        }

        // Reparse and preserve any dynamic-layer entries from the cache.
        let mut fresh = build_static_manifest(&tools_dir)?;
        if let Some(cached) = cached {
            for group in cached.groups {
                if matches!(group.origin, Origin::Dynamic)
                    && !fresh.groups.iter().any(|g| g.name == group.name)
                {
                    fresh.groups.push(group);
                }
            }
            for cmd in cached.commands {
                if matches!(cmd.origin, Origin::Dynamic)
                    && !fresh
                        .commands
                        .iter()
                        .any(|c| c.group == cmd.group && c.name == cmd.name)
                {
                    fresh.commands.push(cmd);
                }
            }
            fresh.dynamic_hash = cached.dynamic_hash;
        }

        Ok(ResolvedManifest {
            manifest: fresh,
            from_cache: false,
            project_root,
        })
    }
    ```

- [x] **Step 3.3: Run tests, expect PASS**

    ```bash
    cargo test --lib complete::
    ```

    Expected: 5 new freshness tests + 10 engine tests = 15 passing.

- [x] **Step 3.4: Commit**

    ```bash
    git add src/complete/freshness.rs src/complete/tests.rs
    git commit -m "feat(complete): Tab-time hash check with cache + fresh-reparse fallback"
    ```

---

## Task 4: Hidden `toolr __complete` subcommand

Add the hidden top-level subcommand that bash/zsh/fish scripts shell out to.
Convention follows `kubectl __complete`, `gh __complete`, `cargo __complete`:
top-level, hidden, takes `<cwd>` plus the user's typed argv. Output is one
candidate per line to stdout; errors are silent (exit non-zero, no stderr).
Tab completion must never spew error messages into the user's prompt.

**Files:**

- Modify: `src/bin/toolr/cli.rs`

- Modify: `src/bin/toolr/dispatch.rs`

- [x] **Step 4.1: Register the hidden subcommand in `src/bin/toolr/cli.rs`**

    In `build_command`, alongside the existing
    `__build-static-manifest` registration, add:

    ```rust
    use clap::{Arg, ArgAction, Command};

    root = root.subcommand(
        Command::new("__complete")
            .hide(true)
            .about("(internal) Emit completion candidates for the shell scripts")
            .arg(
                Arg::new("cwd")
                    .required(true)
                    .help("Absolute path of the shell's working directory at Tab time"),
            )
            .arg(
                Arg::new("args")
                    .num_args(0..)
                    .trailing_var_arg(true)
                    .allow_hyphen_values(true)
                    .help("The user's argv minus the leading `toolr`"),
            ),
    );
    ```

- [x] **Step 4.2: Add dispatch in `src/bin/toolr/dispatch.rs`**

    Before the existing `__build-static-manifest` check, add:

    ```rust
    if let Some(("__complete", sub)) = matches.subcommand() {
        return run_complete(sub);
    }
    ```

    And add the handler at module level:

    ```rust
    use std::path::PathBuf;

    use _rust_utils::complete::{resolve_manifest_at_tab, serve_completions};

    fn run_complete(matches: &clap::ArgMatches) -> anyhow::Result<std::process::ExitCode> {
        // Tab completion must be quiet: any error produces a silent exit
        // code 1 so the shell falls back to its default completion. We do
        // not write to stderr here — that would clobber the user's prompt.
        let Some(cwd) = matches.get_one::<String>("cwd").map(PathBuf::from) else {
            return Ok(std::process::ExitCode::from(1));
        };
        let tokens: Vec<String> = matches
            .get_many::<String>("args")
            .map(|v| v.cloned().collect())
            .unwrap_or_default();
        let Ok(resolved) = resolve_manifest_at_tab(&cwd) else {
            return Ok(std::process::ExitCode::from(1));
        };
        for candidate in serve_completions(&resolved.manifest, &tokens) {
            println!("{candidate}");
        }
        Ok(std::process::ExitCode::SUCCESS)
    }
    ```

- [x] **Step 4.3: Smoke-check the wiring**

    ```bash
    cargo build --bin toolr
    ./target/debug/toolr __complete "$PWD" "" 2>&1 | head -20
    ./target/debug/toolr __complete "$PWD" "ci" "" 2>&1 | head -20
    ```

    Expected: lines of candidates on stdout, exit code 0 (or 1 with no
    output if no `tools/` is reachable from `$PWD`).

- [x] **Step 4.4: Commit**

    ```bash
    git add src/bin/toolr/cli.rs src/bin/toolr/dispatch.rs
    git commit -m "feat(cli): Add hidden __complete subcommand backing shell scripts"
    ```

---

## Task 5: Bash completion script + `toolr self completion print bash`

Embed a small bash script that calls `toolr __complete`. Wire up
`toolr self completion print bash` to write it to stdout. Installation lands
in Task 8.

**Files:**

- Create: `src/complete/scripts/bash.sh`

- Modify: `src/complete/scripts.rs`

- Modify: `src/bin/toolr/cli.rs`

- Modify: `src/bin/toolr/dispatch.rs`

- [x] **Step 5.1: Create `src/complete/scripts/bash.sh`**

    ```bash
    # toolr bash completion — delegates to `toolr __complete`.
    #
    # Install via `toolr self completion install bash`, or source this file
    # directly. Re-source on every shell start; manifest contents are read
    # at Tab time, not when this script is sourced.

    _toolr_complete() {
        local cur prev words cword
        _init_completion || return

        # Pass everything after `toolr` (words[0]) to the binary. `cur` is
        # already the trailing in-progress word; include it as the final
        # element so the engine treats it as the prefix.
        local args=("${words[@]:1}")

        local IFS=$'\n'
        local candidates
        candidates=$(toolr __complete "$PWD" "${args[@]}" 2>/dev/null) || return 0

        COMPREPLY=($(compgen -W "$candidates" -- "$cur"))
    }

    complete -F _toolr_complete toolr
    ```

    **Notes for the implementer:**
    - `_init_completion` comes from `bash-completion`. On systems without
      `bash-completion` installed, this script will fail to load — that
      matches kubectl/gh behaviour and is documented as a prerequisite.
    - `compgen -W` re-applies the prefix filter on the bash side. The Rust
      side already filters, but `_init_completion` may have munged `cur`
      after we read `words`, so re-filtering is defensive.

- [x] **Step 5.2: Embed the script and return it from `completion_script`**

    Replace `src/complete/scripts.rs`:

    ```rust
    //! Embedded shell-completion scripts.

    use std::fmt;
    use std::str::FromStr;

    use anyhow::{anyhow, Result};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Shell {
        Bash,
        Zsh,
        Fish,
    }

    impl fmt::Display for Shell {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(match self {
                Shell::Bash => "bash",
                Shell::Zsh => "zsh",
                Shell::Fish => "fish",
            })
        }
    }

    impl FromStr for Shell {
        type Err = anyhow::Error;
        fn from_str(s: &str) -> Result<Self> {
            match s {
                "bash" => Ok(Shell::Bash),
                "zsh" => Ok(Shell::Zsh),
                "fish" => Ok(Shell::Fish),
                other => Err(anyhow!("unsupported shell: {other} (expected bash, zsh, or fish)")),
            }
        }
    }

    const BASH_SCRIPT: &str = include_str!("scripts/bash.sh");
    // The zsh and fish constants are added by Tasks 6 and 7.

    /// Return the static completion script for the given shell.
    pub fn completion_script(shell: Shell) -> &'static str {
        match shell {
            Shell::Bash => BASH_SCRIPT,
            Shell::Zsh => "", // Task 6
            Shell::Fish => "", // Task 7
        }
    }
    ```

- [x] **Step 5.3: Wire `toolr self completion print [shell]` into clap**

    In `src/bin/toolr/cli.rs`, alongside the other root subcommands, add:

    ```rust
    root = root.subcommand(
        Command::new("self")
            .about("Operations on toolr itself")
            .subcommand_required(true)
            .arg_required_else_help(true)
            .subcommand(
                Command::new("completion")
                    .about("Manage shell completion scripts")
                    .subcommand_required(true)
                    .arg_required_else_help(true)
                    .subcommand(
                        Command::new("print")
                            .about("Print the completion script for a shell to stdout")
                            .arg(
                                Arg::new("shell")
                                    .required(true)
                                    .value_parser(["bash", "zsh", "fish"])
                                    .help("Shell to emit a completion script for"),
                            ),
                    ),
            ),
    );
    ```

    **Note:** Plan 8 (cache management) and future plans will add further
    children under `self`. The `subcommand_required(true)` plus
    `arg_required_else_help(true)` pair ensures users typing
    `toolr self <Tab>` see the available children instead of a no-op
    success.

- [x] **Step 5.4: Dispatch `toolr self completion print <shell>`**

    In `src/bin/toolr/dispatch.rs`, before the user-command lookup, add:

    ```rust
    use _rust_utils::complete::{completion_script, Shell as CompletionShell};

    if let Some(("self", self_matches)) = matches.subcommand() {
        return run_self(self_matches);
    }

    fn run_self(matches: &clap::ArgMatches) -> anyhow::Result<std::process::ExitCode> {
        let Some(("completion", completion_matches)) = matches.subcommand() else {
            anyhow::bail!("expected a `self` subcommand");
        };
        let Some((action, action_matches)) = completion_matches.subcommand() else {
            anyhow::bail!("expected a `self completion` subcommand");
        };
        match action {
            "print" => run_completion_print(action_matches),
            // "install" is added by Task 8
            other => anyhow::bail!("unsupported self completion subcommand: {other}"),
        }
    }

    fn run_completion_print(matches: &clap::ArgMatches) -> anyhow::Result<std::process::ExitCode> {
        let shell_str = matches
            .get_one::<String>("shell")
            .ok_or_else(|| anyhow::anyhow!("missing <shell>"))?;
        let shell: CompletionShell = shell_str.parse()?;
        print!("{}", completion_script(shell));
        Ok(std::process::ExitCode::SUCCESS)
    }
    ```

- [x] **Step 5.5: Add a unit test for the bash script content**

    Append to `src/complete/tests.rs`:

    ```rust
    use crate::complete::{completion_script, Shell};

    #[test]
    fn bash_script_invokes_toolr_complete() {
        let script = completion_script(Shell::Bash);
        assert!(script.contains("toolr __complete"));
        assert!(script.contains("complete -F _toolr_complete toolr"));
    }
    ```

- [x] **Step 5.6: Run tests**

    ```bash
    cargo test --lib complete::
    cargo test --test cli_smoke
    ```

    Expected: previous tests + 1 new test pass. The existing
    `cli_smoke.rs` tests should still pass because `self completion print`
    is an additive subcommand.

- [x] **Step 5.7: Commit**

    ```bash
    git add src/complete/scripts/ src/complete/scripts.rs src/complete/tests.rs src/bin/toolr/cli.rs src/bin/toolr/dispatch.rs
    git commit -m "feat(complete): Embed bash completion script with self completion print bash"
    ```

---

## Task 6: Zsh completion script

Add the zsh equivalent and route `toolr self completion print zsh` through
the same `completion_script(Shell::Zsh)` path.

**Files:**

- Create: `src/complete/scripts/zsh.zsh`

- Modify: `src/complete/scripts.rs`

- Modify: `src/complete/tests.rs`

- [x] **Step 6.1: Create `src/complete/scripts/zsh.zsh`**

    ```zsh
    #compdef toolr
    # toolr zsh completion — delegates to `toolr __complete`.
    #
    # Install via `toolr self completion install zsh`, or place this file in
    # a directory on your $fpath under the name `_toolr` and rerun
    # `compinit`.

    _toolr() {
        local -a candidates
        local cur
        cur="${words[CURRENT]}"

        # words[1] is `toolr`; pass the rest plus the in-progress word.
        local -a passthrough
        passthrough=("${(@)words[2,CURRENT]}")
        # When CURRENT points one past the last typed word, the in-progress
        # word is empty — make sure we still send an empty trailing token.
        if [[ ${#passthrough} -eq 0 ]]; then
            passthrough=("")
        fi

        candidates=("${(@f)$(toolr __complete "$PWD" "${passthrough[@]}" 2>/dev/null)}")

        if (( ${#candidates} > 0 )); then
            compadd -- "${candidates[@]}"
        fi
    }

    compdef _toolr toolr
    ```

- [x] **Step 6.2: Embed and wire into `completion_script`**

    In `src/complete/scripts.rs`, add:

    ```rust
    const ZSH_SCRIPT: &str = include_str!("scripts/zsh.zsh");
    ```

    Update the match arm:

    ```rust
    Shell::Zsh => ZSH_SCRIPT,
    ```

- [x] **Step 6.3: Add a unit test**

    Append to `src/complete/tests.rs`:

    ```rust
    #[test]
    fn zsh_script_invokes_toolr_complete() {
        let script = completion_script(Shell::Zsh);
        assert!(script.starts_with("#compdef toolr"));
        assert!(script.contains("toolr __complete"));
        assert!(script.contains("compdef _toolr toolr"));
    }
    ```

- [x] **Step 6.4: Run tests**

    ```bash
    cargo test --lib complete::
    ```

    Expected: previous tests + 1 new test pass.

- [x] **Step 6.5: Commit**

    ```bash
    git add src/complete/scripts/zsh.zsh src/complete/scripts.rs src/complete/tests.rs
    git commit -m "feat(complete): Embed zsh completion script"
    ```

---

## Task 7: Fish completion script

Add the fish equivalent.

**Files:**

- Create: `src/complete/scripts/fish.fish`

- Modify: `src/complete/scripts.rs`

- Modify: `src/complete/tests.rs`

- [x] **Step 7.1: Create `src/complete/scripts/fish.fish`**

    ```fish
    # toolr fish completion — delegates to `toolr __complete`.
    #
    # Install via `toolr self completion install fish`, or place this file
    # at ~/.config/fish/completions/toolr.fish.

    function __toolr_complete
        # `commandline -opc` returns the tokens already on the command line,
        # excluding the in-progress word. `commandline -ct` returns the
        # in-progress word itself (may be empty).
        set -l tokens (commandline -opc)
        set -l current (commandline -ct)
        # Drop the leading `toolr` token.
        set -l args $tokens[2..-1]
        set -a args -- $current
        toolr __complete "$PWD" $args 2>/dev/null
    end

    complete -c toolr -f -a "(__toolr_complete)"
    ```

    **Notes:**
    - `-f` disables file completion as the default fallback. Per-argument
      file completion is out of scope for v1 (see open questions); fish
      users who want filename completion for, say, a `--path` flag can
      either edit this script or wait for the dynamic-completer work.
    - `set -a args -- $current` appends an explicit empty token when
      `$current` is empty, which keeps the engine's "last token is prefix"
      contract honest.

- [x] **Step 7.2: Embed and wire into `completion_script`**

    In `src/complete/scripts.rs`, add:

    ```rust
    const FISH_SCRIPT: &str = include_str!("scripts/fish.fish");
    ```

    Update the match arm:

    ```rust
    Shell::Fish => FISH_SCRIPT,
    ```

- [x] **Step 7.3: Add a unit test**

    Append to `src/complete/tests.rs`:

    ```rust
    #[test]
    fn fish_script_invokes_toolr_complete() {
        let script = completion_script(Shell::Fish);
        assert!(script.contains("toolr __complete"));
        assert!(script.contains("complete -c toolr"));
    }
    ```

- [x] **Step 7.4: Run tests**

    ```bash
    cargo test --lib complete::
    ```

    Expected: previous tests + 1 new test pass.

- [x] **Step 7.5: Commit**

    ```bash
    git add src/complete/scripts/fish.fish src/complete/scripts.rs src/complete/tests.rs
    git commit -m "feat(complete): Embed fish completion script"
    ```

---

## Task 8: `toolr self completion install [shell]`

Write the embedded script to the standard location for the chosen shell.
Default to a non-interactive overwrite-with-confirm behaviour: if the file
already exists with different content, prompt unless `--force` is passed; in
non-interactive sessions (no TTY) `--force` is required to overwrite.

**Standard locations:**

- **bash:** `$XDG_DATA_HOME/bash-completion/completions/toolr` (default
  `~/.local/share/bash-completion/completions/toolr`). User-scoped; doesn't
  require root.
- **zsh:** `~/.zfunc/_toolr`. Toolr also prints a one-line hint reminding
  the user to add `fpath=(~/.zfunc $fpath)` and `autoload -Uz compinit
  && compinit` to their `.zshrc` if not already present. Detecting that
  programmatically is out of scope.
- **fish:** `$XDG_CONFIG_HOME/fish/completions/toolr.fish` (default
  `~/.config/fish/completions/toolr.fish`). Fish auto-loads completions
  from this directory.

**Files:**

- Create: `src/complete/install.rs`

- Modify: `src/complete/mod.rs`

- Modify: `src/bin/toolr/cli.rs`

- Modify: `src/bin/toolr/dispatch.rs`

- Modify: `src/complete/tests.rs`

- [x] **Step 8.1: Add the failing tests**

    Append to `src/complete/tests.rs`:

    ```rust
    use crate::complete::install::{install_path_for, install_script, InstallOptions, InstallOutcome};

    #[test]
    fn install_path_for_bash_uses_xdg_data_home() {
        let tmp = TempDir::new().unwrap();
        let xdg_data = tmp.path().join("share");
        let path = install_path_for(Shell::Bash, Some(&xdg_data), tmp.path()).unwrap();
        assert_eq!(path, xdg_data.join("bash-completion/completions/toolr"));
    }

    #[test]
    fn install_path_for_zsh_uses_home_zfunc() {
        let tmp = TempDir::new().unwrap();
        let path = install_path_for(Shell::Zsh, None, tmp.path()).unwrap();
        assert_eq!(path, tmp.path().join(".zfunc/_toolr"));
    }

    #[test]
    fn install_path_for_fish_uses_xdg_config_home() {
        let tmp = TempDir::new().unwrap();
        let xdg_config = tmp.path().join("config");
        let path = install_path_for(Shell::Fish, Some(&xdg_config), tmp.path()).unwrap();
        assert_eq!(path, xdg_config.join("fish/completions/toolr.fish"));
    }

    #[test]
    fn install_creates_file_when_absent() {
        let tmp = TempDir::new().unwrap();
        let opts = InstallOptions {
            shell: Shell::Bash,
            xdg_data_home: Some(tmp.path().join("data")),
            xdg_config_home: None,
            home: tmp.path().to_path_buf(),
            force: false,
            interactive: false,
        };
        let outcome = install_script(&opts).unwrap();
        assert!(matches!(outcome, InstallOutcome::Wrote { .. }));
        let target = tmp
            .path()
            .join("data/bash-completion/completions/toolr");
        assert!(target.exists());
    }

    #[test]
    fn install_refuses_to_overwrite_differing_file_without_force() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("data/bash-completion/completions/toolr");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "# someone else's script\n").unwrap();
        let opts = InstallOptions {
            shell: Shell::Bash,
            xdg_data_home: Some(tmp.path().join("data")),
            xdg_config_home: None,
            home: tmp.path().to_path_buf(),
            force: false,
            interactive: false,
        };
        let outcome = install_script(&opts).unwrap();
        assert!(matches!(outcome, InstallOutcome::SkippedNeedsForce { .. }));
        let contents = std::fs::read_to_string(&target).unwrap();
        assert_eq!(contents, "# someone else's script\n");
    }

    #[test]
    fn install_is_idempotent_when_content_matches() {
        let tmp = TempDir::new().unwrap();
        let opts = InstallOptions {
            shell: Shell::Bash,
            xdg_data_home: Some(tmp.path().join("data")),
            xdg_config_home: None,
            home: tmp.path().to_path_buf(),
            force: false,
            interactive: false,
        };
        let first = install_script(&opts).unwrap();
        let second = install_script(&opts).unwrap();
        assert!(matches!(first, InstallOutcome::Wrote { .. }));
        assert!(matches!(second, InstallOutcome::AlreadyInstalled { .. }));
    }

    #[test]
    fn install_with_force_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("data/bash-completion/completions/toolr");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "# stale\n").unwrap();
        let opts = InstallOptions {
            shell: Shell::Bash,
            xdg_data_home: Some(tmp.path().join("data")),
            xdg_config_home: None,
            home: tmp.path().to_path_buf(),
            force: true,
            interactive: false,
        };
        let outcome = install_script(&opts).unwrap();
        assert!(matches!(outcome, InstallOutcome::Wrote { .. }));
        let contents = std::fs::read_to_string(&target).unwrap();
        assert!(contents.contains("toolr __complete"));
    }
    ```

- [x] **Step 8.2: Implement `src/complete/install.rs`**

    ```rust
    //! Install the embedded shell-completion script into the standard
    //! location for the target shell.

    use std::path::{Path, PathBuf};

    use anyhow::{anyhow, Result};

    use super::scripts::{completion_script, Shell};

    pub struct InstallOptions {
        pub shell: Shell,
        /// Override for `$XDG_DATA_HOME`. `None` means read from environment
        /// at call time (callers usually pass `None` in production).
        pub xdg_data_home: Option<PathBuf>,
        /// Override for `$XDG_CONFIG_HOME`. `None` means read from
        /// environment at call time.
        pub xdg_config_home: Option<PathBuf>,
        /// Path to use as the user's home directory.
        pub home: PathBuf,
        /// Overwrite a non-matching existing file without prompting.
        pub force: bool,
        /// Currently informational only — the file API is non-interactive.
        /// Reserved for future "prompt before overwrite" behaviour in the
        /// CLI dispatcher.
        pub interactive: bool,
    }

    #[derive(Debug)]
    pub enum InstallOutcome {
        Wrote { path: PathBuf },
        AlreadyInstalled { path: PathBuf },
        SkippedNeedsForce { path: PathBuf },
    }

    /// Compute the target install path for `shell`.
    pub fn install_path_for(
        shell: Shell,
        xdg_override: Option<&Path>,
        home: &Path,
    ) -> Result<PathBuf> {
        match shell {
            Shell::Bash => {
                let base = xdg_override
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| home.join(".local/share"));
                Ok(base.join("bash-completion/completions/toolr"))
            }
            Shell::Zsh => Ok(home.join(".zfunc/_toolr")),
            Shell::Fish => {
                let base = xdg_override
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| home.join(".config"));
                Ok(base.join("fish/completions/toolr.fish"))
            }
        }
    }

    /// Write the embedded script to the chosen location.
    pub fn install_script(opts: &InstallOptions) -> Result<InstallOutcome> {
        let path = match opts.shell {
            Shell::Bash => install_path_for(
                opts.shell,
                opts.xdg_data_home.as_deref(),
                &opts.home,
            )?,
            Shell::Fish => install_path_for(
                opts.shell,
                opts.xdg_config_home.as_deref(),
                &opts.home,
            )?,
            Shell::Zsh => install_path_for(opts.shell, None, &opts.home)?,
        };

        let payload = completion_script(opts.shell);
        if payload.is_empty() {
            return Err(anyhow!("no embedded completion script for {}", opts.shell));
        }

        if let Ok(existing) = std::fs::read_to_string(&path) {
            if existing == payload {
                return Ok(InstallOutcome::AlreadyInstalled { path });
            }
            if !opts.force {
                return Ok(InstallOutcome::SkippedNeedsForce { path });
            }
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, payload)?;
        Ok(InstallOutcome::Wrote { path })
    }
    ```

- [x] **Step 8.3: Re-export from `src/complete/mod.rs`**

    Add:

    ```rust
    pub mod install;

    pub use install::{install_path_for, install_script, InstallOptions, InstallOutcome};
    ```

- [x] **Step 8.4: Add the `install` subcommand to clap**

    In `src/bin/toolr/cli.rs`, alongside the `print` subcommand under
    `self completion`, add:

    ```rust
    Command::new("install")
        .about("Install the completion script for a shell into its standard location")
        .arg(
            Arg::new("shell")
                .required(true)
                .value_parser(["bash", "zsh", "fish"])
                .help("Shell to install the completion script for"),
        )
        .arg(
            Arg::new("force")
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Overwrite an existing differing file without prompting"),
        ),
    ```

    Make sure to chain `.subcommand(...)` so this lives under
    `self completion`, peer to `print`.

- [x] **Step 8.5: Dispatch the install command**

    In `src/bin/toolr/dispatch.rs`, extend `run_self`:

    ```rust
    match action {
        "print" => run_completion_print(action_matches),
        "install" => run_completion_install(action_matches),
        other => anyhow::bail!("unsupported self completion subcommand: {other}"),
    }
    ```

    And add:

    ```rust
    use _rust_utils::complete::{install_script, InstallOptions, InstallOutcome};

    fn run_completion_install(matches: &clap::ArgMatches) -> anyhow::Result<std::process::ExitCode> {
        let shell_str = matches
            .get_one::<String>("shell")
            .ok_or_else(|| anyhow::anyhow!("missing <shell>"))?;
        let shell: CompletionShell = shell_str.parse()?;
        let force = matches.get_flag("force");

        let home = dirs_home()?;
        let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(std::path::PathBuf::from);
        let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from);
        let opts = InstallOptions {
            shell,
            xdg_data_home,
            xdg_config_home,
            home,
            force,
            interactive: std::io::IsTerminal::is_terminal(&std::io::stdin()),
        };

        let outcome = install_script(&opts)?;
        match outcome {
            InstallOutcome::Wrote { path } => {
                println!("toolr: wrote {} completion script to {}", shell, path.display());
                if matches!(shell, CompletionShell::Zsh) {
                    println!(
                        "toolr: ensure your ~/.zshrc includes `fpath=(~/.zfunc $fpath)` and \
                         `autoload -Uz compinit && compinit`."
                    );
                }
                Ok(std::process::ExitCode::SUCCESS)
            }
            InstallOutcome::AlreadyInstalled { path } => {
                println!(
                    "toolr: {} completion already installed at {}",
                    shell,
                    path.display()
                );
                Ok(std::process::ExitCode::SUCCESS)
            }
            InstallOutcome::SkippedNeedsForce { path } => {
                eprintln!(
                    "toolr: refusing to overwrite {} (use --force to replace)",
                    path.display()
                );
                Ok(std::process::ExitCode::from(1))
            }
        }
    }

    fn dirs_home() -> anyhow::Result<std::path::PathBuf> {
        // Avoid taking on a new crate dep — read $HOME directly.
        let home = std::env::var_os("HOME")
            .ok_or_else(|| anyhow::anyhow!("$HOME is not set; cannot pick install path"))?;
        Ok(std::path::PathBuf::from(home))
    }
    ```

    `std::io::IsTerminal` is stable since Rust 1.70 and requires no new
    dependency.

- [x] **Step 8.6: Run tests**

    ```bash
    cargo test --lib complete::
    cargo test --test cli_smoke
    ```

    Expected: 7 new install tests pass; existing tests still pass.

- [x] **Step 8.7: Commit**

    ```bash
    git add src/complete/install.rs src/complete/mod.rs src/complete/tests.rs src/bin/toolr/cli.rs src/bin/toolr/dispatch.rs
    git commit -m "feat(complete): Install completion scripts into shell-standard locations"
    ```

---

## Task 9: End-to-end integration tests via `assert_cmd`

Drive the real binary through `__complete` with several fixture manifests
and confirm stdout matches expectations. This proves the wiring from clap
through `serve_completions` to stdout works on every supported shell call
shape.

**Files:**

- Create: `tests/complete_smoke.rs`

- [x] **Step 9.1: Create `tests/complete_smoke.rs`**

    ```rust
    use assert_cmd::Command;
    use tempfile::TempDir;

    /// Build a tmpdir containing a tools/ directory with one ci.py file and
    /// a freshly-built manifest committed alongside it. This mirrors the
    /// happy path: the cached manifest's static_hash matches the live tree.
    fn fixture() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        std::fs::create_dir(&tools).unwrap();
        std::fs::write(
            tools.join("ci.py"),
            r#""""CI utilities."""
from typing import Literal

group = command_group("ci", "CI utilities", docstring=**doc**)

@group.command
def hello(ctx, name="world"):
    """Say hello.

    Args:
        name: Who to greet.
    """
    pass

@group.command
def deploy(ctx, env: Literal["staging", "production"]):
    """Deploy something."""
    pass
"#,
        )
        .unwrap();

        // Build the manifest in-process so the static_hash matches.
        Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .arg("__build-static-manifest")
            .assert()
            .success();

        tmp
    }

    fn complete(tmp: &TempDir, args: &[&str]) -> String {
        let cwd = tmp.path().to_path_buf();
        let mut full: Vec<String> = vec!["__complete".into(), cwd.to_string_lossy().to_string()];
        for a in args {
            full.push((*a).to_string());
        }
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .args(&full)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "expected __complete to succeed, got status {:?}, stderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn completes_groups_at_top_level() {
        let tmp = fixture();
        let stdout = complete(&tmp, &[""]);
        let lines: Vec<&str> = stdout.lines().collect();
        assert!(lines.contains(&"ci"), "missing ci in {stdout}");
    }

    #[test]
    fn completes_commands_under_a_group() {
        let tmp = fixture();
        let stdout = complete(&tmp, &["ci", ""]);
        let lines: Vec<&str> = stdout.lines().collect();
        assert!(lines.contains(&"hello"), "missing hello in {stdout}");
        assert!(lines.contains(&"deploy"), "missing deploy in {stdout}");
    }

    #[test]
    fn completes_command_prefixes() {
        let tmp = fixture();
        let stdout = complete(&tmp, &["ci", "h"]);
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines, vec!["hello"]);
    }

    #[test]
    fn completes_literal_flag_values() {
        let tmp = fixture();
        let stdout = complete(&tmp, &["ci", "deploy", "--env", ""]);
        let mut lines: Vec<&str> = stdout.lines().collect();
        lines.sort();
        assert_eq!(lines, vec!["production", "staging"]);
    }

    #[test]
    fn returns_no_completions_for_unknown_group() {
        let tmp = fixture();
        let stdout = complete(&tmp, &["unknown", ""]);
        assert!(stdout.trim().is_empty());
    }

    #[test]
    fn reparses_when_tools_change_after_manifest_was_written() {
        let tmp = fixture();
        // Add a new command after the cached manifest was built.
        std::fs::write(
            tmp.path().join("tools/extra.py"),
            r#"group = command_group("extra", "Extra utilities")

@group.command
def shiny(ctx):
    """Shiny new command."""
    pass
"#,
        )
        .unwrap();

        let stdout = complete(&tmp, &[""]);
        let lines: Vec<&str> = stdout.lines().collect();
        assert!(lines.contains(&"extra"), "missing freshly-added group in {stdout}");
    }

    #[test]
    fn silent_failure_when_no_tools_dir_anywhere() {
        let tmp = TempDir::new().unwrap();
        let cwd = tmp.path().to_path_buf();
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(&cwd)
            .args(["__complete", &cwd.to_string_lossy(), ""])
            .output()
            .unwrap();
        // Exit code 1 with empty stderr — the shell falls back silently.
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty(), "expected silent failure");
    }

    #[test]
    fn self_completion_print_emits_bash_script() {
        let tmp = TempDir::new().unwrap();
        let output = Command::cargo_bin("toolr")
            .unwrap()
            .current_dir(tmp.path())
            .args(["self", "completion", "print", "bash"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("toolr __complete"));
        assert!(stdout.contains("complete -F _toolr_complete toolr"));
    }
    ```

- [x] **Step 9.2: Run the smoke tests**

    ```bash
    cargo test --test complete_smoke
    ```

    Expected: 8 tests passing.

- [x] **Step 9.3: Commit**

    ```bash
    git add tests/complete_smoke.rs
    git commit -m "test(complete): End-to-end smoke tests against the real toolr binary"
    ```

---

## Task 10: Manual end-to-end sanity check against the real repo

Sanity-check the wiring against the actual `tools/` in this repo, in each
supported shell. This is not automated — manual confirmation that the
scripts work in a real terminal session.

- [x] **Step 10.1: Rebuild and regenerate the manifest**

    ```bash
    cargo build --bin toolr --release
    ./target/release/toolr __build-static-manifest
    ```

- [x] **Step 10.2: Bash sanity check**

    ```bash
    bash --noprofile --norc -c '
        source <(./target/release/toolr self completion print bash)
        complete -p toolr
        # Manually exercise:
        COMP_LINE="toolr " COMP_POINT=6 ./target/release/toolr __complete "$PWD" ""
        COMP_LINE="toolr ci " COMP_POINT=9 ./target/release/toolr __complete "$PWD" "ci" ""
    '
    ```

    Expected: groups listed, then `ci` commands listed.

- [x] **Step 10.3: Zsh sanity check (skip if zsh not installed)**

    ```bash
    zsh -f -c '
        eval "$(./target/release/toolr self completion print zsh)"
        # We can'\''t easily script tab; just smoke-check the function exists.
        whence -w _toolr
    '
    ```

    Expected: `_toolr: function`.

- [x] **Step 10.4: Fish sanity check (skip if fish not installed)**

    ```bash
    fish -c '
        ./target/release/toolr self completion print fish | source
        complete -c toolr
        echo "fish completion installed."
    '
    ```

    Expected: completion listed; no errors.

- [x] **Step 10.5: Latency budget check**

    ```bash
    hyperfine --warmup 3 './target/release/toolr __complete "$PWD" ""'
    hyperfine --warmup 3 './target/release/toolr __complete "$PWD" "ci" ""'
    ```

    Expected: warm runs well under 10 ms; cold runs (no committed manifest)
    under 50 ms on a typical `tools/` tree.

    If a run exceeds the budget, profile with `cargo flamegraph` and file
    an issue — do not block the plan landing on it, but record the number
    in the PR description.

- [x] **Step 10.6: No commit needed**

    This task is a verification gate, not a code change.

---

## Task 11: Update the roadmap

Mark Plan 4 as Done in the roadmap once everything above is merged.

**Files:**

- Modify: `specs/rust-front-end/01-roadmap.md`

- [x] **Step 11.1: Update the Plan 4 entry**

    Change `### Plan 4: Shell completion`:

    ```markdown
    ### Plan 4: Shell completion

    - **Status:** ✅ Done
    - **Plan doc:** [05-plan-4-completion.md](./05-plan-4-completion.md)
    - **Depends on:** Plan 1
    - **Unblocks:** —
    - **Produces:**
        - …(unchanged)…
    ```

- [x] **Step 11.2: Commit**

    ```bash
    git add specs/rust-front-end/01-roadmap.md
    git commit -m "docs(roadmap): Mark Plan 4 as done"
    ```

---

## Done criteria

Plan 4 is complete when:

- `cargo test` passes for `complete::` unit tests, `cli_smoke`, and
  `complete_smoke`.
- `toolr __complete <cwd> <args>` returns the expected candidates against
  both a cached-and-fresh manifest and a cached-but-stale manifest (the
  latter triggers an in-process re-parse via `build_static_manifest`).
- `toolr __complete <cwd> <args>` exits non-zero **silently** (no stderr)
  when no `tools/` directory is reachable from `<cwd>`. This matters: a
  noisy completion endpoint clobbers the user's prompt.
- `toolr self completion print [bash|zsh|fish]` writes the corresponding
  embedded script to stdout.
- `toolr self completion install [shell]` writes the script to the
  shell-standard location, is idempotent on a second run with matching
  contents, and refuses to overwrite a differing file unless `--force` is
  passed.
- Manual sanity checks pass in bash, zsh, and fish: typing
  `toolr <Tab>` in a real terminal session offers user-defined groups from
  the current repo's `tools/`.
- The latency budget (Task 10.5) is met: <50 ms cold, <10 ms warm.
- The roadmap status table reflects Plan 4 as `✅ Done`.

## Open questions (for the implementer)

These are deliberately deferred — surface to the spec author if any block
progress, otherwise resolve in line:

1. **Filename / path completion for `pathlib.Path` arguments.** v1 emits no
   candidates for arguments without `allowed_values`, which lets the shell
   fall back to its own filename completion (bash via `_init_completion`
   and zsh via `_default`; fish disables file fallback via `-f` in this
   plan). A richer mode that explicitly returns filename suggestions tagged
   for the shell (e.g. zsh's `_files`-style markup) is future work alongside
   the dynamic-completer story listed under "Future work" in the design.
2. **Async manifest write-back after a stale-cache reparse.** The design
   says "optionally write back an updated manifest asynchronously"; this
   plan deliberately does **not** do that. Reasons: (a) Tab time is not a
   safe moment to mutate `tools/.toolr-manifest.json` if multiple shells
   race; (b) `toolr project manifest rebuild` and the pre-commit hook
   (Plan 6) cover the durable-write story. If profiling shows the in-place
   reparse is too slow on big trees, revisit then.
3. **Zsh `fpath` configuration.** `install` prints a one-line hint reminding
   the user to add `~/.zfunc` to `$fpath`, but does not edit the user's
   `.zshrc` for them — that's user-owned configuration. Should the hint
   live somewhere more prominent (e.g. a final summary block from
   `toolr self completion install zsh`)?
4. **Bash `bash-completion` dependency.** The embedded bash script uses
   `_init_completion`, which requires the `bash-completion` package. On
   stock macOS bash this isn't installed by default. Document as a
   prerequisite, or hand-roll the completion logic without `_init_completion`
   at the cost of edge-case correctness on words containing `=` or `:`?
5. **`clap_complete` for the static-script generation, later.** This plan
   hand-writes the shell snippets. If/when toolr adds more top-level
   built-ins (cache, project, etc.), the snippets stay one-liners that
   shell out to `toolr __complete` — the volume never grows. Worth a note:
   we may revisit this if `clap_complete`'s `complete` builder ships a
   "delegate to a custom command" mode that fits cleanly.
