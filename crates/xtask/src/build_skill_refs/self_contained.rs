//! Self-containment gate: a skill ships on its own, so nothing it tells an
//! agent to read may point back into this repository.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

use super::docs_section::{Fence, code_span_end, find_links, is_reference_definition};

const DESIGN: &str = "specs/archive/2026/2026-09-27-skills-self-contained-design.md";

// Compared lowercased; the brief lists both spellings of the repo name.
const REPO_URLS: [&str; 4] = [
    "github.com/s0undt3ch/toolr",
    "github.com/s0undt3ch/ToolR",
    "raw.githubusercontent.com/s0undt3ch/",
    "toolr.readthedocs.io",
];

const REPO_PATHS: [&str; 5] = ["docs/", "crates/", "examples/", "specs/", "skills/"];

#[derive(Debug)]
pub struct Violation {
    pub file: PathBuf,
    pub line: usize,
    pub target: String,
    pub rule: &'static str,
}

/// Lint every shipped Markdown file of every skill under `skills/`.
pub fn lint(repo_root: &Path) -> Result<()> {
    let skills_root = repo_root.join("skills");
    let mut violations = Vec::new();
    for skill_dir in sorted_entries(&skills_root)? {
        if !skill_dir.is_dir() {
            continue;
        }
        let mut files = vec![PathBuf::from("SKILL.md")];
        for sub in ["references", "examples"] {
            collect_md(&skill_dir, Path::new(sub), &mut files)?;
        }
        for rel in files {
            let path = skill_dir.join(&rel);
            if !path.is_file() {
                continue;
            }
            let body = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let shown = path.strip_prefix(repo_root).unwrap_or(&path).to_path_buf();
            violations.extend(
                lint_file(&skill_dir, &rel, &body, &|p: &Path| p.exists())
                    .into_iter()
                    .map(|v| Violation {
                        file: shown.clone(),
                        ..v
                    }),
            );
        }
    }
    if violations.is_empty() {
        return Ok(());
    }
    let listing = violations
        .iter()
        .map(|v| format!("{}:{}: {}: {}", v.file.display(), v.line, v.rule, v.target))
        .collect::<Vec<_>>()
        .join("\n");
    bail!("{listing}\nskills must be self-contained; see {DESIGN}");
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

fn collect_md(skill_dir: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let dir = skill_dir.join(rel);
    if !dir.is_dir() {
        return Ok(());
    }
    for path in sorted_entries(&dir)? {
        let name = path.file_name().unwrap_or_default();
        let child = rel.join(name);
        if path.is_dir() {
            collect_md(skill_dir, &child, out)?;
        } else if path.extension().is_some_and(|e| e == "md")
            && name != "README.md"
            && name != "REVIEW.md"
        {
            out.push(child);
        }
    }
    Ok(())
}

/// Lint one file's body. `rel` is the file's path inside `skill_dir`;
/// `exists` is only asked about `skill_dir`-joined paths.
fn lint_file(
    skill_dir: &Path,
    rel: &Path,
    body: &str,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<Violation> {
    let (paragraphs, definitions) = split(body);
    let mut lint = FileLint {
        skill_dir,
        file_dir: rel.parent().unwrap_or(Path::new("")),
        exists,
        found: Vec::new(),
    };
    let mut used: Vec<&str> = Vec::new();
    for (first, text) in &paragraphs {
        let links = match find_links(text, *first) {
            Ok(links) => links,
            Err(e) => {
                lint.flag(*first, "malformed-markdown", &format!("{e:#}"));
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
            let token = text[i + run..end - run]
                .split_whitespace()
                .next()
                .unwrap_or("");
            if REPO_PATHS.iter().any(|p| token.starts_with(p)) && !lint.inside(Path::new(token)) {
                lint.flag(line_of(i), "repo-path", token);
            }
            i = end;
        }
        let masked =
            String::from_utf8(masked).expect("masking only overwrites ASCII-delimited spans");
        for (pos, url) in bare_urls(&masked) {
            if is_repo_url(url) {
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

    fn target(&mut self, line: usize, target: &str) {
        if target.is_empty() || target.starts_with('#') || target.starts_with("mailto:") {
            return;
        }
        if target.contains("://") {
            if is_repo_url(target) {
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

fn is_repo_url(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    let rest = url.split_once("://").map_or(url.as_str(), |(_, r)| r);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    REPO_URLS
        .iter()
        .any(|p| rest.starts_with(&p.to_ascii_lowercase()))
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
mod tests {
    use std::path::Path;

    use super::*;

    fn exists_in(paths: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| paths.iter().any(|x| Path::new(x) == p)
    }

    fn rules(body: &str, paths: &'static [&'static str]) -> Vec<&'static str> {
        lint_file(
            Path::new("s"),
            Path::new("SKILL.md"),
            body,
            &exists_in(paths),
        )
        .into_iter()
        .map(|v| v.rule)
        .collect()
    }

    #[test]
    fn relative_link_escaping_skill_is_flagged() {
        assert_eq!(rules("[a](../../docs/x.md)\n", &[]), ["link-escapes-skill"]);
    }

    #[test]
    fn link_to_own_reference_passes() {
        assert!(rules("[a](references/x.md)\n", &["s/references/x.md"]).is_empty());
    }

    #[test]
    fn link_to_missing_own_file_is_flagged() {
        assert_eq!(
            rules("[a](references/nope.md)\n", &[]),
            ["link-escapes-skill"]
        );
    }

    #[test]
    fn repo_and_docs_urls_are_flagged_case_insensitively() {
        let body = "[a](https://github.com/s0undt3ch/ToolR/tree/main/skills/x)\nhttps://toolr.readthedocs.io/latest/\n";
        assert_eq!(rules(body, &[]), ["repo-url", "repo-url"]);
    }

    #[test]
    fn third_party_urls_pass() {
        assert!(rules("[t](https://github.com/Canop/termimad)\n", &[]).is_empty());
    }

    #[test]
    fn backticked_repo_path_outside_skill_is_flagged() {
        assert_eq!(rules("See `docs/project-config.md`.\n", &[]), ["repo-path"]);
    }

    #[test]
    fn backticked_own_example_and_user_paths_pass() {
        let body = "`examples/tools/greet.py`, `tools/ci.py`, `.github/workflows/ci.yml`\n";
        assert!(rules(body, &["s/examples/tools/greet.py"]).is_empty());
    }

    #[test]
    fn fenced_code_is_ignored() {
        assert!(rules("```\n[a](../../x.md) `docs/x`\n```\n", &[]).is_empty());
    }

    #[test]
    fn wrapped_link_reports_its_own_line() {
        let body = "Intro.\n\nSome [wrapped\ntext](../out.md) here.\n";
        let v = lint_file(Path::new("s"), Path::new("SKILL.md"), body, &exists_in(&[]));
        assert_eq!((v[0].rule, v[0].line), ("link-escapes-skill", 3));
    }

    #[test]
    fn reference_links_resolve_through_their_definitions() {
        let body = "[a][docs] and [b][] and [c][nowhere]\n\n[docs]: https://toolr.readthedocs.io/x\n[b]: references/x.md\n";
        assert_eq!(rules(body, &[]), ["repo-url", "link-escapes-skill"]);
    }

    #[test]
    fn unused_definition_is_still_linted() {
        assert_eq!(rules("[x]: ../../docs/x.md\n", &[]), ["link-escapes-skill"]);
    }

    #[test]
    fn malformed_markdown_is_reported_not_swallowed() {
        assert_eq!(rules("a ](b)\n", &[]), ["malformed-markdown"]);
    }

    #[test]
    fn repo_url_in_inline_code_is_ignored() {
        assert!(rules("`https://github.com/s0undt3ch/ToolR`\n", &[]).is_empty());
    }
}
