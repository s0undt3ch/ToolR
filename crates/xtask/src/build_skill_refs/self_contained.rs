//! Self-containment gate: a skill ships on its own, so nothing it tells an
//! agent to read may point back into this repository.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::docs_section::{
    Fence, code_span_end, find_links, html_targets, is_reference_definition,
};
use super::{normalize_newlines, read_text};

const DESIGN: &str = "specs/archive/2026/2026-09-27-skills-self-contained-design.md";

const CARGO_TOML: &str = "Cargo.toml";
const REPOSITORY_KEY: [&str; 3] = ["workspace", "package", "repository"];
const PYPROJECT: &str = "crates/toolr/pyproject.toml";
const DOCUMENTATION_KEY: [&str; 3] = ["project", "urls", "Documentation"];

const REPO_PATHS: [&str; 5] = ["docs/", "crates/", "examples/", "specs/", "skills/"];

#[derive(Debug)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub target: String,
    pub rule: &'static str,
}

/// Lint every file each skill under `skills/` ships.
pub fn lint(repo_root: &Path) -> Result<()> {
    let urls = repo_urls(repo_root)?;
    let skills_root = repo_root.join("skills");
    let mut violations = Vec::new();
    for skill_dir in sorted_entries(&skills_root)? {
        if !skill_dir.is_dir() {
            continue;
        }
        for rel in shipped_files(&skill_dir)? {
            let path = skill_dir.join(&rel);
            let shown = path.strip_prefix(repo_root).unwrap_or(&path).to_path_buf();
            // Listed in the index but deleted from the worktree: nothing ships.
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                let target = std::fs::read_link(&path)
                    .with_context(|| format!("reading link {}", path.display()))?;
                violations.push(Violation {
                    file: shown,
                    line: 1,
                    target: slash_path(&target),
                    rule: "symlink",
                });
                continue;
            }
            let bytes =
                std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
            let Ok(body) = String::from_utf8(bytes) else {
                continue;
            };
            let body = normalize_newlines(&body);
            if body.contains('\0') {
                continue;
            }
            let found = if rel.extension().is_some_and(|e| e == "md") {
                lint_file(&skill_dir, &rel, &body, &|p: &Path| p.exists(), &urls)
            } else {
                lint_text(&body, &urls)
            };
            violations.extend(found.into_iter().map(|v| Violation {
                file: shown.clone(),
                ..v
            }));
        }
    }
    if violations.is_empty() {
        return Ok(());
    }
    let listing = violations
        .iter()
        .map(|v| {
            format!(
                "{}:{}: {}: {}",
                slash_path(&v.file),
                v.line,
                v.rule,
                v.target
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    bail!("{listing}\nskills must be self-contained; see {DESIGN}");
}

// Repo-relative paths print the same on every OS, so CI logs and tests match.
fn slash_path(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Scheme-less, lowercased prefixes of every URL that leads back to this
/// repository or its docs, read from the project metadata that already
/// names them.
fn repo_urls(repo_root: &Path) -> Result<Vec<String>> {
    let repository = metadata_string(repo_root, CARGO_TOML, &REPOSITORY_KEY)?;
    let rest = scheme_less(without_fragment(&repository));
    let rest = rest.strip_suffix(".git").unwrap_or(&rest);
    let [host, owner, repo] = rest.split('/').collect::<Vec<_>>()[..] else {
        bail!(not_github(&repository));
    };
    if host != "github.com" || owner.is_empty() || repo.is_empty() {
        bail!(not_github(&repository));
    }
    let docs = metadata_string(repo_root, PYPROJECT, &DOCUMENTATION_KEY)?;
    let docs_host = scheme_less(without_fragment(&docs))
        .split('/')
        .next()
        .unwrap_or_default()
        .to_string();
    if docs_host.is_empty() {
        bail!(
            "{PYPROJECT}: `{}` has no host: {docs}",
            DOCUMENTATION_KEY.join(".")
        );
    }
    Ok(vec![
        format!("github.com/{owner}/{repo}"),
        format!("raw.githubusercontent.com/{owner}/{repo}"),
        docs_host,
    ])
}

fn not_github(url: &str) -> String {
    format!(
        "{CARGO_TOML}: `{}` is not a github.com/<owner>/<repo> URL: {url}",
        REPOSITORY_KEY.join(".")
    )
}

fn without_fragment(url: &str) -> &str {
    url.split(['#', '?']).next().unwrap_or(url)
}

fn scheme_less(url: &str) -> String {
    let url = url.to_ascii_lowercase();
    let rest = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    rest.trim_end_matches('/').to_string()
}

fn metadata_string(repo_root: &Path, rel: &str, key: &[&str]) -> Result<String> {
    let text = read_text(&repo_root.join(rel))?;
    let table: toml::Table = toml::from_str(&text).with_context(|| format!("parsing {rel}"))?;
    let (last, parents) = key.split_last().expect("key is non-empty");
    parents
        .iter()
        .try_fold(&table, |t, k| t.get(*k)?.as_table())
        .and_then(|t| t.get(*last)?.as_str())
        .map(str::to_string)
        .with_context(|| format!("{rel}: no string `{}`", key.join(".")))
}

fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = std::fs::read_dir(dir)
        .with_context(|| format!("listing {}", dir.display()))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("listing {}", dir.display()))?;
    entries.sort();
    Ok(entries)
}

/// Every file the skill would ship if committed now, relative to
/// `skill_dir`: tracked or untracked but not gitignored, so a local `.venv/`
/// is left out. `README.md`, `REVIEW.md` and the skill-root `tests/` are for
/// maintainers. Symlinks are listed, not followed.
fn shipped_files(skill_dir: &Path) -> Result<Vec<PathBuf>> {
    let out = Command::new("git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            ".",
        ])
        .current_dir(skill_dir)
        .output()
        .with_context(|| format!("running git ls-files in {}", skill_dir.display()))?;
    if !out.status.success() {
        bail!(
            "git ls-files in {} failed: {}",
            skill_dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let mut files: Vec<PathBuf> = String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|rel| !rel.is_empty() && !rel.starts_with("tests/"))
        .map(PathBuf::from)
        .filter(|rel| {
            let name = rel.file_name().unwrap_or_default();
            name != "README.md" && name != "REVIEW.md"
        })
        .collect();
    files.sort();
    files.dedup();
    Ok(files)
}

/// Non-Markdown files have no link syntax to resolve; only a repo URL
/// anywhere in them can send an agent back to this repository.
fn lint_text(body: &str, urls: &[String]) -> Vec<Violation> {
    body.lines()
        .enumerate()
        .flat_map(|(i, line)| {
            bare_urls(line)
                .into_iter()
                .filter(|(_, url)| is_repo_url(url, urls))
                .map(move |(_, url)| Violation {
                    file: PathBuf::new(),
                    line: i + 1,
                    target: url.to_string(),
                    rule: "repo-url",
                })
        })
        .collect()
}

/// Lint one file's body. `rel` is the file's path inside `skill_dir`;
/// `exists` is only asked about `skill_dir`-joined paths.
fn lint_file(
    skill_dir: &Path,
    rel: &Path,
    body: &str,
    exists: &dyn Fn(&Path) -> bool,
    urls: &[String],
) -> Vec<Violation> {
    let (paragraphs, definitions) = split(body);
    let mut lint = FileLint {
        skill_dir,
        file_dir: rel.parent().unwrap_or(Path::new("")),
        exists,
        urls,
        found: Vec::new(),
    };
    let mut used: Vec<&str> = Vec::new();
    for (first, text) in &paragraphs {
        let links = match find_links(text, *first) {
            Ok(links) => links,
            Err(e) => {
                lint.flag(e.line, "malformed-markdown", &e.message);
                continue;
            }
        };
        let mut masked = text.clone().into_bytes();
        for link in &links {
            masked[link.span.clone()].fill(b' ');
            if !link.reference {
                lint.target(link.lineno, &link.target);
                continue;
            }
            let label = if link.target.is_empty() {
                &link.text
            } else {
                &link.target
            };
            // An unresolved `[x][y]` is prose in brackets, not a path.
            if let Some((label, (target, _))) = definitions.get_key_value(&normalize_label(label)) {
                used.push(label);
                lint.target(link.lineno, target);
            }
        }
        let line_of = |pos: usize| first + text[..pos].matches('\n').count();
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'`' {
                i += 1;
                continue;
            }
            let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
            let Some(end) = code_span_end(bytes, i) else {
                i += run;
                continue;
            };
            masked[i..end].fill(b' ');
            // Only the first token: later ones (`cat docs/x`) are as likely
            // to be the user's own project paths.
            let token = text[i + run..end - run]
                .split_whitespace()
                .next()
                .unwrap_or("");
            if lint.repo_path(token) {
                lint.flag(line_of(i), "repo-path", token);
            }
            i = end;
        }
        for (range, value) in html_targets(&String::from_utf8_lossy(&masked)) {
            lint.target(line_of(range.start), &value);
            masked[range].fill(b' ');
        }
        let masked =
            String::from_utf8(masked).expect("masking only overwrites ASCII-delimited spans");
        for (pos, url) in bare_urls(&masked) {
            if is_repo_url(url, urls) {
                lint.flag(line_of(pos), "repo-url", url);
            }
        }
    }
    for (label, (target, line)) in &definitions {
        if !used.contains(&label.as_str()) {
            lint.target(*line, target);
        }
    }
    lint.found.sort_by_key(|v| v.line);
    lint.found
}

/// First line number and the paragraph's lines joined with `\n`.
type Paragraph = (usize, String);

/// Reference label (normalised) to its target and definition line.
type Definitions = BTreeMap<String, (String, usize)>;

/// Split `body` into prose paragraphs outside fences, plus the reference
/// definitions keyed by label.
fn split(body: &str) -> (Vec<Paragraph>, Definitions) {
    let mut paragraphs = Vec::new();
    let mut definitions = BTreeMap::new();
    let mut current: Option<Paragraph> = None;
    let mut fence: Option<Fence> = None;
    for (i, line) in body.lines().enumerate() {
        let lineno = i + 1;
        let trimmed = line.trim_start();
        if let Some(open) = fence {
            if open.closed_by(trimmed) {
                fence = None;
            }
            continue;
        }
        let opened = Fence::opened_by(trimmed, lineno);
        if trimmed.is_empty() || opened.is_some() || is_reference_definition(line) {
            paragraphs.extend(current.take());
            fence = opened;
            if opened.is_none() && !trimmed.is_empty() {
                let (label, target) = trimmed[1..].split_once("]:").expect("is a definition");
                let target = target.split_whitespace().next().unwrap_or("");
                let target = target
                    .strip_prefix('<')
                    .and_then(|t| t.strip_suffix('>'))
                    .unwrap_or(target);
                definitions
                    .entry(normalize_label(label))
                    .or_insert((target.to_string(), lineno));
            }
            continue;
        }
        match &mut current {
            Some((_, text)) => {
                text.push('\n');
                text.push_str(line);
            }
            None => current = Some((lineno, line.to_string())),
        }
    }
    paragraphs.extend(current);
    (paragraphs, definitions)
}

struct FileLint<'a> {
    skill_dir: &'a Path,
    file_dir: &'a Path,
    exists: &'a dyn Fn(&Path) -> bool,
    urls: &'a [String],
    found: Vec<Violation>,
}

impl FileLint<'_> {
    fn flag(&mut self, line: usize, rule: &'static str, target: &str) {
        self.found.push(Violation {
            file: PathBuf::new(),
            line,
            target: target.to_string(),
            rule,
        });
    }

    /// Whether `rel` (relative to the skill root) stays inside it and exists.
    fn inside(&self, rel: &Path) -> bool {
        normalize(rel).is_some_and(|p| (self.exists)(&self.skill_dir.join(p)))
    }

    /// A backticked token naming a path in this repository rather than in
    /// the skill or in the user's project.
    fn repo_path(&self, token: &str) -> bool {
        let mut token = token;
        while let Some(rest) = token.strip_prefix("./") {
            token = rest;
        }
        if token.starts_with("../") {
            return !self.inside(&self.file_dir.join(token));
        }
        REPO_PATHS.iter().any(|p| token.starts_with(p)) && !self.inside(Path::new(token))
    }

    fn target(&mut self, line: usize, target: &str) {
        if target.is_empty() || target.starts_with('#') || target.starts_with("mailto:") {
            return;
        }
        if target
            .get(..5)
            .is_some_and(|s| s.eq_ignore_ascii_case("file:"))
        {
            self.flag(line, "link-escapes-skill", target);
            return;
        }
        if target.contains("://") {
            if is_repo_url(target, self.urls) {
                self.flag(line, "repo-url", target);
            }
            return;
        }
        let path = target.split(['#', '?']).next().unwrap_or(target);
        if !self.inside(&self.file_dir.join(path)) {
            self.flag(line, "link-escapes-skill", target);
        }
    }
}

/// Resolve `.` and `..` without touching the filesystem; `None` when the
/// path is absolute or climbs above its starting directory.
fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// CommonMark matches reference labels case-insensitively with internal
/// whitespace collapsed.
fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `urls` come from [`repo_urls`], already lowercased and scheme-less.
fn is_repo_url(url: &str, urls: &[String]) -> bool {
    let url = url.to_ascii_lowercase();
    let rest = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    urls.iter()
        .any(|p| rest.strip_prefix(p.as_str()).is_some_and(at_boundary))
}

/// Whether a matched prefix ends on a segment boundary, so `<repo>` doesn't
/// also match `<repo>-other`. A `.git` clone suffix still counts as the repo.
fn at_boundary(rest: &str) -> bool {
    let rest = rest.strip_prefix(".git").unwrap_or(rest);
    rest.is_empty() || rest.starts_with(['/', '#', '?', ':'])
}

/// `http(s)://` tokens in `text`, with their byte offsets.
fn bare_urls(text: &str) -> Vec<(usize, &str)> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(k) = lower[from..].find("http") {
        let start = from + k;
        let rest = &lower[start..];
        if !(rest.starts_with("http://") || rest.starts_with("https://")) {
            from = start + 4;
            continue;
        }
        let len = text[start..]
            .find(|c: char| c.is_whitespace() || "<>()[]\"'`".contains(c))
            .unwrap_or(text.len() - start);
        let url = text[start..start + len].trim_end_matches(['.', ',', ';', ':', '!', '?']);
        out.push((start, url));
        from = start + len;
    }
    out
}

#[cfg(test)]
mod tests;
