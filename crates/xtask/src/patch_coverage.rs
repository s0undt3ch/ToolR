//! `patch-coverage` subcommand — reports how much of a branch's changed
//! Rust lines the lcov file marks as hit, mirroring Codecov's patch gate.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Directories and files `.codecov.yml` ignores that would otherwise match `crates/*/src/**/*.rs`.
/// Keep this and `IGNORED_SUFFIXES` in sync with `.codecov.yml`'s `ignore` list.
const IGNORED_PATHS: [&str; 5] = [
    "crates/toolr-py/src/",
    "crates/toolr-core/src/complete/scripts/",
    "crates/toolr/src/init_templates/",
    "crates/toolr-core/src/test_env.rs",
    "crates/toolr-core/src/test_fs.rs",
];

/// File names `.codecov.yml` ignores anywhere under `crates/*/src/`: unit tests and the test-only
/// files they `include!`.
const IGNORED_SUFFIXES: [&str; 2] = ["/tests.rs", "_test.rs"];

const LCOV_HINT: &str = "generate it with `cargo llvm-cov --workspace --exclude toolr-py --tests \
                         --lcov --output-path target/coverage/lcov.info -- --test-threads=1`";

/// Changed lines per workspace-relative file.
type ChangedLines = BTreeMap<String, BTreeSet<u32>>;
/// Summed hit count per instrumented line, per workspace-relative file.
type LineHits = BTreeMap<String, BTreeMap<u32, u64>>;

/// Coverage of one file's changed, instrumented lines.
#[derive(Debug, PartialEq, Eq)]
pub struct FileCoverage {
    pub path: String,
    pub covered: usize,
    pub instrumented: usize,
    pub missed: Vec<u32>,
}

#[derive(Debug, PartialEq)]
pub struct Summary {
    pub files: Vec<FileCoverage>,
    pub covered: usize,
    pub total: usize,
}

impl Summary {
    /// Percentage of instrumented changed lines that were hit; 100 when
    /// there are none, as Codecov treats an empty patch.
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            100.0
        } else {
            100.0 * self.covered as f64 / self.total as f64
        }
    }

    /// Per-file miss lines followed by the `patch coverage:` total.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for f in self.files.iter().filter(|f| !f.missed.is_empty()) {
            out.push_str(&format!(
                "{}: {}/{}, missed lines {:?}\n",
                f.path, f.covered, f.instrumented, f.missed
            ));
        }
        out.push_str(&format!(
            "patch coverage: {}/{} = {:.1}%\n",
            self.covered,
            self.total,
            self.percent()
        ));
        out
    }
}

/// Entry point invoked by `main`.
pub fn run(root: &Path, base: &str, lcov: &Path, fail_under: f64) -> Result<()> {
    let root = root
        .canonicalize()
        .with_context(|| format!("resolving root {}", root.display()))?;
    let lcov_path = if lcov.is_absolute() {
        lcov.to_path_buf()
    } else {
        root.join(lcov)
    };
    let lcov_text = std::fs::read_to_string(&lcov_path)
        .with_context(|| format!("reading {}; {LCOV_HINT}", lcov_path.display()))?;

    let diff = git_diff(&root, base)?;
    let summary = summarize(&parse_diff(&diff), &parse_lcov(&lcov_text, &root));
    print!("{}", summary.render());

    if summary.percent() < fail_under {
        bail!(
            "patch coverage {:.1}% is below the {fail_under}% threshold",
            summary.percent()
        );
    }
    Ok(())
}

fn git_diff(root: &Path, base: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", "-U0", "--no-color", "--no-ext-diff"])
        .arg(format!("{base}...HEAD"))
        .args(["--", "crates"])
        .current_dir(root)
        .output()
        .context("running git")?;
    if !output.status.success() {
        bail!(
            "git diff {base}...HEAD failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn is_measured(path: &str) -> bool {
    let mut parts = path.splitn(4, '/');
    let in_crate_src = parts.next() == Some("crates")
        && parts.next().is_some()
        && parts.next() == Some("src")
        && parts.next().is_some();
    in_crate_src
        && path.ends_with(".rs")
        && !IGNORED_PATHS.iter().any(|p| path.starts_with(p))
        && !IGNORED_SUFFIXES.iter().any(|s| path.ends_with(s))
}

/// Collect the added/changed new-side line numbers of measured files from
/// `git diff -U0` output.
pub fn parse_diff(text: &str) -> ChangedLines {
    let mut changed = ChangedLines::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            // `/dev/null` marks a deletion; nothing on the new side to cover.
            current = path
                .strip_prefix("b/")
                .filter(|p| is_measured(p))
                .map(str::to_owned);
        } else if line.starts_with("@@")
            && let Some(file) = &current
            && let Some((start, count)) = hunk_new_range(line)
        {
            // A zero count is a pure deletion and adds no lines.
            changed
                .entry(file.clone())
                .or_default()
                .extend(start..start + count);
        }
    }
    changed.retain(|_, lines| !lines.is_empty());
    changed
}

/// New-side `(start, count)` of a `@@ -a,b +c,d @@` header; `d` defaults to 1.
fn hunk_new_range(header: &str) -> Option<(u32, u32)> {
    let new = header.split_whitespace().find(|t| t.starts_with('+'))?;
    let (start, count) = match new[1..].split_once(',') {
        Some((s, c)) => (s, c),
        None => (&new[1..], "1"),
    };
    Some((start.parse().ok()?, count.parse().ok()?))
}

/// Sum `DA:` hits per line across `SF` records, keeping only files inside `root`.
pub fn parse_lcov(text: &str, root: &Path) -> LineHits {
    let mut hits = LineHits::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(path) = line.strip_prefix("SF:") {
            current = Path::new(path)
                .strip_prefix(root)
                .ok()
                .map(|rel| rel.to_string_lossy().replace('\\', "/"));
            if let Some(rel) = &current {
                hits.entry(rel.clone()).or_default();
            }
        } else if line == "end_of_record" {
            current = None;
        } else if let Some(rest) = line.strip_prefix("DA:")
            && let Some(rel) = &current
        {
            let mut fields = rest.split(',');
            if let (Some(Ok(n)), Some(Ok(c))) = (
                fields.next().map(str::parse::<u32>),
                fields.next().map(str::parse::<u64>),
            ) {
                *hits.entry(rel.clone()).or_default().entry(n).or_insert(0) += c;
            }
        }
    }
    hits
}

/// Intersect changed lines with instrumented lines; uninstrumented lines
/// (comments, signatures, blanks) don't count, as in Codecov.
pub fn summarize(changed: &ChangedLines, hits: &LineHits) -> Summary {
    let mut files = Vec::new();
    let (mut covered, mut total) = (0, 0);
    for (path, lines) in changed {
        let Some(da) = hits.get(path) else { continue };
        let instrumented: Vec<u32> = lines
            .iter()
            .copied()
            .filter(|n| da.contains_key(n))
            .collect();
        let missed: Vec<u32> = instrumented
            .iter()
            .copied()
            .filter(|n| da[n] == 0)
            .collect();
        covered += instrumented.len() - missed.len();
        total += instrumented.len();
        files.push(FileCoverage {
            path: path.clone(),
            covered: instrumented.len() - missed.len(),
            instrumented: instrumented.len(),
            missed,
        });
    }
    Summary {
        files,
        covered,
        total,
    }
}

#[cfg(test)]
mod tests;
