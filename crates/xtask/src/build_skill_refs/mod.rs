//! `build-skill-refs` subcommand — regenerates the `references/*.md`
//! files under `skills/*/` from toolr's own source.
//!
//! Adding a new skill: implement a generator function returning a
//! [`Generated`] value and register it inside [`run`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// One regenerated file, ready to either write to disk or compare
/// against the committed version when `--check` is in effect.
pub struct Generated {
    /// Absolute path the body belongs to.
    pub path: PathBuf,
    /// Rendered markdown body, including the trailing newline. The
    /// generator guarantees byte-identical output across runs against
    /// the same source tree.
    pub body: String,
}

/// Entry point invoked by `main`.
pub fn run(check: bool) -> Result<()> {
    // `_root` is a deliberate prefix until the first generator is
    // registered (next task) — flipping back to `root` then keeps the
    // diff tight.
    let _root = repo_root()?;

    // Generators are added in subsequent tasks. This vector defines
    // the registry — `--check` iterates over every entry.
    let outputs: Vec<Generated> = Vec::new();

    apply(outputs, check)
}

/// Either write each [`Generated`] to disk or, in `--check` mode,
/// collect the paths whose committed bodies do not match.
fn apply(outputs: Vec<Generated>, check: bool) -> Result<()> {
    let mut drift = Vec::new();
    for out in outputs {
        let current = std::fs::read_to_string(&out.path).ok();
        if current.as_deref() == Some(out.body.as_str()) {
            continue;
        }
        if check {
            drift.push(out.path);
        } else {
            if let Some(parent) = out.path.parent() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("creating parent directory for {}", out.path.display())
                })?;
            }
            std::fs::write(&out.path, &out.body)
                .with_context(|| format!("writing {}", out.path.display()))?;
        }
    }

    if !drift.is_empty() {
        let listing = drift
            .iter()
            .map(|p| format!("  {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow::bail!(
            "skill references are out of date — run `cargo xtask build-skill-refs`:\n{listing}",
        );
    }

    Ok(())
}

/// Resolve the workspace root (one above this crate's manifest dir).
///
/// `xtask` lives at `<repo>/crates/xtask`, so `repo_root` is
/// `manifest_dir/../..` — robust against the working directory the
/// alias is invoked from.
fn repo_root() -> Result<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let root = Path::new(manifest_dir)
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("crates/xtask is not nested two levels under the repo root")?;
    Ok(root)
}
