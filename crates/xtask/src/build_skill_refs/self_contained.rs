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
        .map(|v| format!("{}:{}: {}: {}", slash_path(&v.file), v.line, v.rule, v.target))
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
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "."])
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
    urls.iter().any(|p| rest.strip_prefix(p.as_str()).is_some_and(at_boundary))
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
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    const CARGO_TOML: &str =
        "[workspace.package]\nrepository = \"https://github.com/s0undt3ch/toolr\"\n";
    const PYPROJECT: &str = "[project.urls]\nDocumentation = \"https://toolr.readthedocs.io\"\n";

    /// A throwaway repo root holding this repo's URL metadata, then `files`
    /// (repo-relative path, body), which may override it.
    fn temp_repo(tag: &str, files: &[(&str, &str)]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("xtask-self-contained-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let meta = [
            ("Cargo.toml", CARGO_TOML),
            ("crates/toolr/pyproject.toml", PYPROJECT),
        ];
        for (rel, body) in meta.iter().chain(files) {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let git = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(git.success());
        root
    }

    fn lint_repo(tag: &str, files: &[(&str, &str)]) -> Result<()> {
        let root = temp_repo(tag, files);
        let out = lint(&root);
        std::fs::remove_dir_all(&root).unwrap();
        out
    }

    fn urls() -> Vec<String> {
        [
            "github.com/s0undt3ch/toolr",
            "raw.githubusercontent.com/s0undt3ch/toolr",
            "toolr.readthedocs.io",
        ]
        .map(String::from)
        .to_vec()
    }

    fn rules_in(rel: &str, body: &str, paths: &'static [&'static str]) -> Vec<&'static str> {
        lint_file(
            Path::new("s"),
            Path::new(rel),
            body,
            &exists_in(paths),
            &urls(),
        )
        .into_iter()
        .map(|v| v.rule)
        .collect()
    }

    fn exists_in(paths: &'static [&'static str]) -> impl Fn(&Path) -> bool {
        move |p: &Path| paths.iter().any(|x| Path::new(x) == p)
    }

    fn rules(body: &str, paths: &'static [&'static str]) -> Vec<&'static str> {
        lint_file(
            Path::new("s"),
            Path::new("SKILL.md"),
            body,
            &exists_in(paths),
            &urls(),
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
        let v = lint_file(
            Path::new("s"),
            Path::new("SKILL.md"),
            body,
            &exists_in(&[]),
            &urls(),
        );
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
    fn malformed_markdown_reports_the_offending_line() {
        let body = "Intro.\n\nfirst\nsecond\nthird ](b)\n";
        let v = lint_file(Path::new("s"), Path::new("SKILL.md"), body, &exists_in(&[]), &urls());
        assert_eq!((v[0].rule, v[0].line), ("malformed-markdown", 5));
        assert_eq!(v[0].target, "`](` without a matching `[`");
    }

    #[test]
    fn repo_url_in_inline_code_is_ignored() {
        assert!(rules("`https://github.com/s0undt3ch/ToolR`\n", &[]).is_empty());
    }

    #[test]
    fn repo_url_in_example_python_is_flagged() {
        let err = lint_repo(
            "py",
            &[
                ("skills/x/SKILL.md", "ok\n"),
                (
                    "skills/x/examples/tools/a.py",
                    "# ok\n# see https://github.com/s0undt3ch/ToolR/x\n",
                ),
            ],
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains(
                "skills/x/examples/tools/a.py:2: repo-url: https://github.com/s0undt3ch/ToolR/x"
            ),
            "{err}"
        );
    }

    #[test]
    fn crlf_skill_files_lint_identically_to_lf() {
        let md = "Intro.\n\nSome [wrapped\ntext](../out.md) here.\n\n```\n`docs/x`\n```\nRead `docs/y.md`.\n";
        let py = "# ok\n# see https://github.com/s0undt3ch/ToolR/x\n";
        let run = |tag: &str, eol: &str| {
            lint_repo(
                tag,
                &[
                    ("skills/x/SKILL.md", &md.replace('\n', eol)),
                    ("skills/x/examples/a.py", &py.replace('\n', eol)),
                ],
            )
            .unwrap_err()
            .to_string()
        };
        let lf = run("eol-lf", "\n");
        assert!(lf.contains("SKILL.md:3: link-escapes-skill"), "{lf}");
        assert_eq!(run("eol-crlf", "\r\n").replace("eol-crlf", "eol-lf"), lf);
        assert_eq!(run("eol-cr", "\r").replace("eol-cr", "eol-lf"), lf);
    }

    #[test]
    fn sibling_markdown_is_linted() {
        let err = lint_repo(
            "sibling",
            &[
                ("skills/x/SKILL.md", "ok\n"),
                ("skills/x/workflow.md", "Read `docs/x.md`.\n"),
            ],
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("skills/x/workflow.md:1: repo-path: docs/x.md"),
            "{err}"
        );
    }

    #[test]
    fn readme_review_tests_and_binary_files_are_skipped() {
        let bad = "[a](../../docs/x.md) https://toolr.readthedocs.io/\n";
        lint_repo(
            "skipped",
            &[
                ("skills/x/SKILL.md", "ok\n"),
                ("skills/x/README.md", bad),
                ("skills/x/REVIEW.md", bad),
                ("skills/x/tests/t.md", bad),
                ("skills/x/examples/README.md", bad),
                (
                    "skills/x/examples/blob.bin",
                    "\0\0https://toolr.readthedocs.io/\n",
                ),
            ],
        )
        .unwrap();
    }

    #[test]
    fn nested_tests_directory_ships_and_is_linted() {
        let err = lint_repo(
            "nested-tests",
            &[
                ("skills/x/SKILL.md", "ok\n"),
                ("skills/x/examples/tests/t.py", "# https://toolr.readthedocs.io/\n"),
            ],
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("skills/x/examples/tests/t.py:1: repo-url"), "{err}");
    }

    #[test]
    fn gitignored_files_do_not_ship_and_are_not_linted() {
        let bad = "[a](../../docs/x.md) https://toolr.readthedocs.io/\n";
        lint_repo(
            "ignored",
            &[
                (".gitignore", ".venv/\n"),
                ("skills/x/SKILL.md", "ok\n"),
                ("skills/x/examples/.venv/lib/x.md", bad),
            ],
        )
        .unwrap();
    }

    #[test]
    fn tracked_file_deleted_from_the_worktree_is_skipped() {
        let root = temp_repo(
            "deleted",
            &[
                ("skills/x/SKILL.md", "ok\n"),
                ("skills/x/gone.md", "[a](../../docs/x.md)\n"),
            ],
        );
        let added = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(added.success());
        std::fs::remove_file(root.join("skills/x/gone.md")).unwrap();
        let out = lint(&root);
        std::fs::remove_dir_all(&root).unwrap();
        out.unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_flagged_not_followed() {
        let root = temp_repo(
            "symlink",
            &[
                ("skills/x/SKILL.md", "ok\n"),
                ("outside/leak.md", "[a](../../docs/x.md)\n"),
            ],
        );
        let skill = root.join("skills/x");
        std::os::unix::fs::symlink("../../outside", skill.join("outside")).unwrap();
        std::os::unix::fs::symlink(".", skill.join("loop")).unwrap();
        let err = lint(&root).unwrap_err().to_string();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(err.contains("skills/x/loop:1: symlink: ."), "{err}");
        assert!(err.contains("skills/x/outside:1: symlink: ../../outside"), "{err}");
        assert!(!err.contains("leak.md"), "{err}");
    }

    #[test]
    fn dot_slash_repo_path_is_flagged() {
        assert_eq!(rules("`./docs/x.md`\n", &[]), ["repo-path"]);
        assert!(rules("`./examples/a.py`\n", &["s/examples/a.py"]).is_empty());
    }

    #[test]
    fn parent_relative_backticked_path_escaping_skill_is_flagged() {
        assert_eq!(
            rules_in("references/r.md", "`../../docs/x.md`\n", &[]),
            ["repo-path"]
        );
        assert!(
            rules_in("references/r.md", "`../examples/a.py`\n", &["s/examples/a.py"]).is_empty()
        );
    }

    #[test]
    fn parent_relative_backticked_path_missing_inside_skill_is_flagged() {
        assert_eq!(
            rules_in("references/r.md", "`../examples/nope.py`\n", &[]),
            ["repo-path"]
        );
    }

    #[test]
    fn only_the_first_backticked_token_is_checked() {
        assert!(rules("`cat docs/x.md`\n", &[]).is_empty());
    }

    #[test]
    fn file_scheme_links_are_flagged() {
        assert_eq!(
            rules("[a](file:///etc/x.md)\n", &[]),
            ["link-escapes-skill"]
        );
        assert_eq!(
            rules("[a](FILE:x.md)\n", &["s/x.md"]),
            ["link-escapes-skill"]
        );
    }

    #[test]
    fn raw_html_href_and_src_are_linted() {
        let body =
            "<a href=\"../../docs/x.md\">x</a>\n<img src='https://toolr.readthedocs.io/a.png'>\n";
        assert_eq!(rules(body, &[]), ["link-escapes-skill", "repo-url"]);
    }

    #[test]
    fn raw_html_to_own_file_or_in_code_passes() {
        assert!(
            rules(
                "<a href=\"references/x.md\">x</a>\n",
                &["s/references/x.md"]
            )
            .is_empty()
        );
        assert!(rules("`<a href=\"../../x.md\">`\n", &[]).is_empty());
        assert!(rules("```html\n<a href=\"../../x.md\">\n```\n", &[]).is_empty());
    }

    #[test]
    fn loose_files_directly_under_skills_are_not_a_skill() {
        lint_repo(
            "loose",
            &[
                ("skills/x/SKILL.md", "ok\n"),
                ("skills/notes.md", "[a](../../docs/x.md)\n"),
            ],
        )
        .unwrap();
    }

    #[test]
    fn non_utf8_files_are_skipped() {
        let root = temp_repo("non-utf8", &[("skills/x/SKILL.md", "ok\n")]);
        std::fs::create_dir_all(root.join("skills/x/examples")).unwrap();
        std::fs::write(
            root.join("skills/x/examples/latin1.txt"),
            b"\xff\xfe https://toolr.readthedocs.io/\n",
        )
        .unwrap();
        let out = lint(&root);
        std::fs::remove_dir_all(&root).unwrap();
        out.unwrap();
    }

    #[test]
    fn unmatched_backtick_run_does_not_hide_a_later_code_span() {
        assert_eq!(rules("``unclosed `docs/x.md`\n", &[]), ["repo-path"]);
    }

    #[test]
    fn anchor_mailto_and_empty_targets_pass() {
        assert!(rules("[a](#x) [b](mailto:a@b.c) [c]()\n", &[]).is_empty());
    }

    #[test]
    fn dot_slash_link_to_own_file_passes() {
        assert!(rules("[a](./references/x.md)\n", &["s/references/x.md"]).is_empty());
    }

    #[test]
    fn absolute_path_link_is_flagged() {
        assert_eq!(rules("[a](/etc/x.md)\n", &[]), ["link-escapes-skill"]);
    }

    #[test]
    fn prefixed_href_and_src_attributes_are_linted() {
        let body = "<a data-href=\"../../x.md\">x</a>\n<img data-src=../../y.png>\n<use xlink:href='../../z.svg'/>\n";
        assert_eq!(rules(body, &[]), ["link-escapes-skill"; 3]);
    }

    #[test]
    fn srcset_and_names_merely_ending_in_href_are_not_targets() {
        assert!(rules("<img srcset=\"../../a.png 2x\"> <a xhref=\"../../x.md\">\n", &[]).is_empty());
    }

    #[test]
    fn unquoted_html_attribute_values_are_linted() {
        let body =
            "<a href=../../docs/x.md>x</a>\n<img src=https://toolr.readthedocs.io/a.png alt=x>\n";
        assert_eq!(rules(body, &[]), ["link-escapes-skill", "repo-url"]);
        assert!(rules("<a href=>x</a>\n", &[]).is_empty());
    }

    #[test]
    fn unterminated_quoted_attribute_is_not_a_target() {
        assert!(rules("<a href=\"../../x.md\n", &[]).is_empty());
    }

    #[test]
    fn http_prefixed_words_do_not_stop_the_url_scan() {
        assert_eq!(
            rules("httpx and https://toolr.readthedocs.io/x\n", &[]),
            ["repo-url"]
        );
    }

    fn urls_for(tag: &str, files: &[(&str, &str)]) -> Result<Vec<String>> {
        let root = temp_repo(tag, files);
        let out = repo_urls(&root);
        std::fs::remove_dir_all(&root).unwrap();
        out
    }

    #[test]
    fn repo_urls_derive_from_this_repo_metadata() {
        assert_eq!(
            repo_urls(&super::super::repo_root().unwrap()).unwrap(),
            urls()
        );
    }

    #[test]
    fn repo_urls_follow_the_project_metadata() {
        let got = urls_for(
            "urls-other",
            &[
                (
                    "Cargo.toml",
                    "[workspace.package]\nrepository = \"https://www.GitHub.com/Acme/Widget.git/\"\n",
                ),
                (
                    "crates/toolr/pyproject.toml",
                    "[project.urls]\nDocumentation = \"https://Docs.Acme.dev/widget/\"\n",
                ),
            ],
        )
        .unwrap();
        assert_eq!(
            got,
            [
                "github.com/acme/widget",
                "raw.githubusercontent.com/acme/widget",
                "docs.acme.dev"
            ]
        );
    }

    #[test]
    fn lint_flags_the_urls_the_metadata_names() {
        let err = lint_repo(
            "urls-lint",
            &[
                (
                    "Cargo.toml",
                    "[workspace.package]\nrepository = \"https://github.com/acme/widget\"\n",
                ),
                (
                    "skills/x/SKILL.md",
                    "https://github.com/acme/widget/x https://github.com/s0undt3ch/toolr\n",
                ),
            ],
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("repo-url: https://github.com/acme/widget/x"),
            "{err}"
        );
        assert!(!err.contains("s0undt3ch/toolr"), "{err}");
    }

    #[test]
    fn repo_urls_ignore_a_fragment_or_query_in_the_metadata() {
        let got = urls_for(
            "urls-fragment",
            &[
                (
                    "Cargo.toml",
                    "[workspace.package]\nrepository = \"https://github.com/acme/widget#readme\"\n",
                ),
                (
                    "crates/toolr/pyproject.toml",
                    "[project.urls]\nDocumentation = \"https://docs.acme.dev?v=1\"\n",
                ),
            ],
        )
        .unwrap();
        assert_eq!(
            got,
            [
                "github.com/acme/widget",
                "raw.githubusercontent.com/acme/widget",
                "docs.acme.dev"
            ]
        );
    }

    #[test]
    fn repo_urls_match_on_a_path_segment_boundary() {
        let flagged = "https://github.com/s0undt3ch/ToolR/x https://github.com/s0undt3ch/toolr#f \
            https://github.com/s0undt3ch/ToolR.git https://toolr.readthedocs.io:443/x \
            https://raw.githubusercontent.com/s0undt3ch/ToolR/main/x\n";
        assert_eq!(rules(flagged, &[]), ["repo-url"; 5]);
        let other = "https://github.com/s0undt3ch/toolr-other https://toolr.readthedocs.io.evil.com/ \
            https://raw.githubusercontent.com/s0undt3ch/other/main/x\n";
        assert!(rules(other, &[]).is_empty());
    }

    #[test]
    fn missing_repository_key_is_an_error_naming_file_and_key() {
        let err = format!(
            "{:#}",
            urls_for(
                "urls-no-repo",
                &[("Cargo.toml", "[workspace.package]\nversion = \"1\"\n")]
            )
            .unwrap_err()
        );
        assert!(
            err.contains("Cargo.toml: no string `workspace.package.repository`"),
            "{err}"
        );
    }

    #[test]
    fn missing_documentation_url_is_an_error_naming_file_and_key() {
        let err = format!(
            "{:#}",
            urls_for(
                "urls-no-docs",
                &[("crates/toolr/pyproject.toml", "[project]\nname = \"x\"\n")]
            )
            .unwrap_err()
        );
        assert!(
            err.contains("crates/toolr/pyproject.toml: no string `project.urls.Documentation`"),
            "{err}"
        );
    }

    #[test]
    fn missing_or_invalid_metadata_file_is_an_error() {
        let root = temp_repo("urls-no-file", &[]);
        std::fs::remove_file(root.join("Cargo.toml")).unwrap();
        let missing = format!("{:#}", repo_urls(&root).unwrap_err());
        std::fs::remove_dir_all(&root).unwrap();
        assert!(
            missing.contains("reading ") && missing.contains("Cargo.toml"),
            "{missing}"
        );

        let bad = format!(
            "{:#}",
            urls_for("urls-bad-toml", &[("Cargo.toml", "[x\n")]).unwrap_err()
        );
        assert!(bad.contains("parsing Cargo.toml"), "{bad}");
    }

    #[test]
    fn repository_that_is_not_a_github_repo_url_is_an_error() {
        for url in [
            "https://gitlab.com/a/b",
            "https://github.com/a",
            "https://github.com/a/b/c",
        ] {
            let cargo = format!("[workspace.package]\nrepository = \"{url}\"\n");
            let err = format!(
                "{:#}",
                urls_for("urls-not-gh", &[("Cargo.toml", &cargo)]).unwrap_err()
            );
            assert!(
                err.contains("Cargo.toml: `workspace.package.repository` is not a github.com/<owner>/<repo> URL"),
                "{url}: {err}"
            );
        }
    }

    #[test]
    fn documentation_url_without_a_host_is_an_error() {
        let err = format!(
            "{:#}",
            urls_for(
                "urls-no-host",
                &[(
                    "crates/toolr/pyproject.toml",
                    "[project.urls]\nDocumentation = \"https:///x\"\n"
                )]
            )
            .unwrap_err()
        );
        assert!(
            err.contains("crates/toolr/pyproject.toml: `project.urls.Documentation` has no host"),
            "{err}"
        );
    }
}
