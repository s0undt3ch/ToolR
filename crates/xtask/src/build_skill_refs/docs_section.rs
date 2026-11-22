//! Pull a named `--8<-- [start:x]` / `[end:x]` section out of an mkdocs
//! page and turn it into Markdown an agent can read without mkdocs:
//! snippet includes expanded, cross-page links flattened to plain text,
//! admonitions rewritten as blockquotes. Anything this can't faithfully
//! translate is an error rather than a silent pass-through, so a docs edit
//! can't smuggle mkdocs-only syntax into a skill.

use std::collections::VecDeque;
use std::ops::Range;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::read_text;

/// Guards against an include cycle between spliced Markdown snippets.
const MAX_INCLUDE_DEPTH: usize = 8;

/// Read `repo_root/doc_rel`, find `section`, and render it. Includes
/// resolve against `repo_root`, matching `pymdownx.snippets`' `base_path: .`.
pub fn extract(repo_root: &Path, doc_rel: &str, section: &str) -> Result<String> {
    let doc_path = repo_root.join(doc_rel);
    let doc = read_text(&doc_path)?;
    let ctx = || format!("{doc_rel}: section `{section}`");
    let (first_line, body) = locate_section(&doc, section).with_context(ctx)?;
    let resolve = |p: &str| read_text(&repo_root.join(p));
    transform_from(&body, &resolve, first_line).with_context(ctx)
}

#[cfg(test)]
fn find_section(doc: &str, name: &str) -> Result<String> {
    locate_section(doc, name).map(|(_, body)| body)
}

/// Returns the section body and the 1-based doc line its first line sits
/// on, so errors can point at the page rather than the section.
fn locate_section(doc: &str, name: &str) -> Result<(usize, String)> {
    let start_pat = format!("--8<-- [start:{name}]");
    let end_pat = format!("--8<-- [end:{name}]");
    let lines: Vec<&str> = doc.lines().collect();
    let starts: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].contains(&start_pat))
        .collect();
    let ends: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].contains(&end_pat))
        .collect();
    let (start, end) = match (starts.as_slice(), ends.as_slice()) {
        ([], _) => bail!("missing `{start_pat}` marker"),
        (_, []) => bail!("missing `{end_pat}` marker"),
        ([s], [e]) if s < e => (*s, *e),
        ([_], [_]) => bail!("`{end_pat}` comes before `{start_pat}`"),
        _ => bail!("section `{name}` appears more than once"),
    };
    let mut body = String::new();
    for line in &lines[start + 1..end] {
        body.push_str(line);
        body.push('\n');
    }
    Ok((start + 2, body))
}

#[cfg(test)]
fn transform(
    section_src: &str,
    resolve_include: &dyn Fn(&str) -> Result<String>,
) -> Result<String> {
    transform_from(section_src, resolve_include, 1)
}

fn transform_from(
    src: &str,
    resolve: &dyn Fn(&str) -> Result<String>,
    first_line: usize,
) -> Result<String> {
    let lines = render(src, resolve, first_line, 0)?;
    let is_blank = |l: &String| l.trim().is_empty();
    let start = lines
        .iter()
        .position(|l| !is_blank(l))
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|l| !is_blank(l))
        .map_or(start, |i| i + 1);
    let mut out = lines[start..end].join("\n");
    out.push('\n');
    Ok(out)
}

struct Item {
    text: String,
    lineno: usize,
    depth: usize,
}

fn render(
    src: &str,
    resolve: &dyn Fn(&str) -> Result<String>,
    first_line: usize,
    depth: usize,
) -> Result<Vec<String>> {
    let mut queue: VecDeque<Item> = src
        .lines()
        .enumerate()
        .map(|(i, l)| Item {
            text: l.to_string(),
            lineno: first_line + i,
            depth,
        })
        .collect();
    let mut out = Vec::new();
    let mut fence: Option<Fence> = None;
    let mut para: Vec<Item> = Vec::new();

    while let Some(item) = queue.pop_front() {
        // Prose is buffered per paragraph so a link whose text wraps onto
        // the next source line is still seen as one link.
        if fence.is_none() && is_prose(&item.text) {
            // A heading ends the paragraph before it and never continues
            // into the next line, as in CommonMark.
            let heading = is_atx_heading(&item.text);
            if heading {
                flush_paragraph(&mut para, &mut out)?;
            }
            para.push(item);
            if heading {
                flush_paragraph(&mut para, &mut out)?;
            }
            continue;
        }
        flush_paragraph(&mut para, &mut out)?;
        let Item {
            text,
            lineno,
            depth,
        } = item;
        let trimmed = text.trim_start();
        let indent = &text[..text.len() - trimmed.len()];

        if let Some(open) = fence {
            if let Some(inc) = parse_include(trimmed, lineno)? {
                // Code is copied verbatim: a `.py` file's own text must
                // never be read as Markdown or as a fence close.
                out.extend(indented(indent, load(&inc, resolve, lineno)?));
            } else {
                if open.closed_by(trimmed) {
                    fence = None;
                }
                out.push(text);
            }
            continue;
        }

        if let Some(open) = Fence::opened_by(trimmed, lineno) {
            fence = Some(open);
            out.push(text);
            continue;
        }

        if text.contains("--8<--") {
            if let Some(inc) = parse_include(trimmed, lineno)? {
                if depth >= MAX_INCLUDE_DEPTH {
                    bail!("line {lineno}: includes nested deeper than {MAX_INCLUDE_DEPTH}");
                }
                // Spliced back into the queue so an included Markdown
                // snippet gets the same fence-aware treatment as the page.
                for l in indented(indent, load(&inc, resolve, lineno)?).rev() {
                    queue.push_front(Item {
                        text: l,
                        lineno,
                        depth: depth + 1,
                    });
                }
                continue;
            }
            if is_section_marker(trimmed) {
                continue;
            }
            bail!("line {lineno}: unsupported snippet syntax: {}", text.trim());
        }

        // The reference links that use it have already been flattened.
        if is_reference_definition(&text) {
            continue;
        }

        for (prefix, what) in UNSUPPORTED {
            if trimmed.starts_with(prefix) {
                bail!("line {lineno}: unsupported {what}: {}", text.trim());
            }
        }

        if trimmed.starts_with("!!!") {
            if !indent.is_empty() {
                bail!("line {lineno}: unsupported indented admonition");
            }
            let label = admonition_label(&text, lineno)?;
            let mut body: Vec<Item> = Vec::new();
            while let Some(next) = queue.front() {
                if next.text.trim().is_empty() || admonition_body_line(&next.text).is_some() {
                    body.push(queue.pop_front().expect("front was Some"));
                } else {
                    break;
                }
            }
            while body.last().is_some_and(|b| b.text.trim().is_empty()) {
                queue.push_front(body.pop().expect("last was Some"));
            }
            let body_src: String = body
                .iter()
                .map(|b| format!("{}\n", admonition_body_line(&b.text).unwrap_or("")))
                .collect();
            let body_line = body.first().map_or(lineno + 1, |b| b.lineno);
            let rendered = render(&body_src, resolve, body_line, depth)?;
            out.extend(blockquote(&label, &rendered));
            continue;
        }

        debug_assert!(text.trim().is_empty(), "only blank lines reach here");
        out.push(String::new());
    }
    flush_paragraph(&mut para, &mut out)?;
    if let Some(open) = fence {
        bail!("line {}: unclosed code fence", open.line);
    }
    Ok(out)
}

const UNSUPPORTED: [(&str, &str); 4] = [
    ("???", "collapsible admonition"),
    ("=== \"", "content tab"),
    ("<details", "raw <details> block"),
    ("<div", "raw <div> block"),
];

/// A line that only needs the inline transforms (links, attr lists).
fn is_prose(text: &str) -> bool {
    let t = text.trim_start();
    !t.is_empty()
        && Fence::opened_by(t, 0).is_none()
        && !text.contains("--8<--")
        && !t.starts_with("!!!")
        && !is_reference_definition(text)
        && !UNSUPPORTED.iter().any(|(p, _)| t.starts_with(p))
}

fn flush_paragraph(para: &mut Vec<Item>, out: &mut Vec<String>) -> Result<()> {
    if para.is_empty() {
        return Ok(());
    }
    // Spliced include lines all carry their directive's line number, so a
    // line can't be derived by counting newlines from the first item.
    let linenos: Vec<usize> = para.iter().map(|item| item.lineno).collect();
    let joined = para
        .iter()
        .map(|item| item.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let starts = line_starts(&joined, &linenos);
    let line_of = |pos: usize| line_at(&starts, pos);
    reject_local_html(&joined, &line_of)?;
    // Attribute lists never span lines, so stripping keeps one line per item.
    let stripped = strip_attr_lists(&joined, &line_of)?;
    let starts = line_starts(&stripped, &linenos);
    let rewritten = rewrite_links(&stripped, &|pos| line_at(&starts, pos))?;
    out.extend(
        rewritten
            .split('\n')
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string),
    );
    para.clear();
    Ok(())
}

fn line_starts(text: &str, linenos: &[usize]) -> Vec<(usize, usize)> {
    let mut offset = 0;
    text.split('\n')
        .zip(linenos)
        .map(|(line, lineno)| {
            let start = offset;
            offset += line.len() + 1;
            (start, *lineno)
        })
        .collect()
}

fn line_at(starts: &[(usize, usize)], pos: usize) -> usize {
    let k = starts.partition_point(|(off, _)| *off <= pos);
    starts[k.saturating_sub(1)].1
}

/// A local `href`/`src` in raw HTML would dangle in a skill; catch it here
/// with the page's line rather than later in the self-containment gate.
fn reject_local_html(text: &str, line_of: &dyn Fn(usize) -> usize) -> Result<()> {
    let mut masked = text.as_bytes().to_vec();
    let mut i = 0;
    while i < masked.len() {
        if masked[i] != b'`' {
            i += 1;
            continue;
        }
        let run = masked[i..].iter().take_while(|b| **b == b'`').count();
        match code_span_end(text.as_bytes(), i) {
            Some(end) => {
                masked[i..end].fill(b' ');
                i = end;
            }
            None => i += run,
        }
    }
    let masked = String::from_utf8(masked).expect("masking only overwrites ASCII-delimited spans");
    if let Some((range, target)) = html_targets(&masked)
        .into_iter()
        .find(|(_, target)| !is_remote(target))
    {
        bail!(
            "line {}: raw HTML target `{target}` can't ship in a skill reference",
            line_of(range.start)
        );
    }
    Ok(())
}

/// `[label]: target` at the start of a line (up to 3 spaces of indent).
pub(super) fn is_reference_definition(text: &str) -> bool {
    let t = text.trim_start();
    if text.len() - t.len() > 3 {
        return false;
    }
    let Some(rest) = t.strip_prefix('[') else {
        return false;
    };
    rest.find("]:")
        .is_some_and(|k| k > 0 && !rest[..k].contains(['[', ']']))
}

#[derive(Clone, Copy)]
pub(super) struct Fence {
    ch: char,
    len: usize,
    line: usize,
}

impl Fence {
    pub(super) fn opened_by(trimmed: &str, line: usize) -> Option<Self> {
        let ch = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
        let len = trimmed.chars().take_while(|c| *c == ch).count();
        // CommonMark: a backtick fence's info string can't contain a
        // backtick, so "```x``` more" is inline code in prose.
        if len < 3 || (ch == '`' && trimmed[len..].contains('`')) {
            return None;
        }
        Some(Self { ch, len, line })
    }

    pub(super) fn closed_by(self, trimmed: &str) -> bool {
        let t = trimmed.trim_end();
        t.len() >= self.len && t.chars().all(|c| c == self.ch)
    }
}

struct Include {
    path: String,
    kind: IncludeKind,
}

enum IncludeKind {
    Whole,
    /// 1-based, inclusive line range.
    Range(usize, usize),
    /// `--8<-- [start:name]` / `[end:name]`, mkdocs' own named-section
    /// syntax — the same markers [`locate_section`] reads on the page
    /// itself, so a `.py` include can mark them as `# --8<-- [start:x]`
    /// comments and stay valid Python.
    Named(String),
}

/// `Ok(None)` when the line isn't a quoted include at all; an error when
/// it's a quoted include in a form this extractor doesn't implement.
fn parse_include(trimmed: &str, lineno: usize) -> Result<Option<Include>> {
    let Some(rest) = trimmed.strip_prefix("--8<--") else {
        return Ok(None);
    };
    let rest = rest.trim();
    let Some(inner) = rest
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .filter(|i| !i.is_empty())
    else {
        return Ok(None);
    };
    let ranged = inner.rsplit_once(':').and_then(|(head, b)| {
        let (path, a) = head.rsplit_once(':')?;
        Some((path, a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
    });
    if let Some((path, a, b)) = ranged {
        return Ok(Some(Include {
            path: path.to_string(),
            kind: IncludeKind::Range(a, b),
        }));
    }
    if let Some((path, name)) = inner.rsplit_once(':') {
        // A single number after the colon (`file.py:5`) looks like a
        // botched range, not a section name — reject rather than treat
        // "5" as a section that will never exist.
        if name.is_empty() || name.parse::<usize>().is_ok() {
            bail!("line {lineno}: unsupported snippet include form: {trimmed}");
        }
        // Anything else that isn't a valid section name belongs to the
        // path, as in pymdownx.snippets (`C:\x.py` is one path).
        if is_section_name(name) {
            return Ok(Some(Include {
                path: path.to_string(),
                kind: IncludeKind::Named(name.to_string()),
            }));
        }
    }
    Ok(Some(Include {
        path: inner.to_string(),
        kind: IncludeKind::Whole,
    }))
}

/// Generated `.md` snippets carry their own `DO_NOT_EDIT` header line;
/// splicing it into the middle of a skill reference reads as if the
/// reference itself broke off mid-page, so it's dropped on include.
const DO_NOT_EDIT: &str =
    "<!-- generated by `cargo xtask build-skill-refs`; do not edit by hand -->";

fn strip_leading_do_not_edit(path: &str, content: String) -> String {
    if !path.ends_with(".md") {
        return content;
    }
    let Some(rest) = content.strip_prefix(DO_NOT_EDIT) else {
        return content;
    };
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    rest.to_string()
}

fn load(
    inc: &Include,
    resolve: &dyn Fn(&str) -> Result<String>,
    lineno: usize,
) -> Result<Vec<String>> {
    let content =
        resolve(&inc.path).with_context(|| format!("line {lineno}: including {}", inc.path))?;
    let content = strip_leading_do_not_edit(&inc.path, content);
    match &inc.kind {
        IncludeKind::Whole => Ok(content.lines().map(str::to_string).collect()),
        IncludeKind::Range(a, b) => {
            let lines: Vec<&str> = content.lines().collect();
            if *a == 0 || *b < *a || *b > lines.len() {
                bail!(
                    "line {lineno}: include range {a}:{b} is outside {} ({} lines)",
                    inc.path,
                    lines.len(),
                );
            }
            Ok(dedent(
                lines[*a - 1..*b].iter().map(|l| (*l).to_string()).collect(),
            ))
        }
        IncludeKind::Named(name) => {
            let (_, body) = locate_section(&content, name).map_err(|e| {
                anyhow::anyhow!("line {lineno}: including {}:{name}: {e}", inc.path)
            })?;
            // Formatters like ruff insist on a blank line before a
            // following top-level def, right where our own end marker
            // sits — trim it so the spliced code doesn't carry it.
            let mut lines: Vec<String> = body.lines().map(str::to_string).collect();
            while lines.first().is_some_and(|l| l.trim().is_empty()) {
                lines.remove(0);
            }
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
            Ok(dedent(lines))
        }
    }
}

/// pymdownx.snippets' section-name pattern, `[a-z][-_0-9a-z]*`, matched
/// case-insensitively.
fn is_section_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Python's `textwrap.dedent`, which `dedent_subsections: true` in
/// `mkdocs.yml` applies to line-range and named-section includes.
fn dedent(lines: Vec<String>) -> Vec<String> {
    let common = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| &l[..l.len() - l.trim_start().len()])
        .reduce(|a, b| {
            let n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
            &a[..n]
        })
        .unwrap_or("")
        .len();
    lines
        .into_iter()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                l[common..].to_string()
            }
        })
        .collect()
}

fn indented(indent: &str, lines: Vec<String>) -> impl DoubleEndedIterator<Item = String> + '_ {
    lines.into_iter().map(move |l| {
        if l.is_empty() {
            l
        } else {
            format!("{indent}{l}")
        }
    })
}

fn is_section_marker(trimmed: &str) -> bool {
    let t = trimmed.trim_end();
    let t = t.strip_prefix("<!--").unwrap_or(t);
    let t = t.strip_suffix("-->").unwrap_or(t).trim();
    (t.starts_with("--8<-- [start:") || t.starts_with("--8<-- [end:")) && t.ends_with(']')
}

fn admonition_label(line: &str, lineno: usize) -> Result<String> {
    let rest = line["!!!".len()..].trim();
    let (kind, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    if kind.is_empty() {
        bail!("line {lineno}: admonition without a kind");
    }
    let mut chars = kind.chars();
    let kind: String = chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    let tail = tail.trim();
    if tail.is_empty() {
        return Ok(kind);
    }
    let Some(title) = tail.strip_prefix('"').and_then(|t| t.strip_suffix('"')) else {
        bail!(
            "line {lineno}: unsupported admonition header: {}",
            line.trim()
        );
    };
    if title.is_empty() {
        return Ok(kind);
    }
    Ok(format!("{kind} — {}", rewrite_links(title, &|_| lineno)?))
}

/// Python-Markdown accepts either four spaces or a tab as the indent.
fn admonition_body_line(line: &str) -> Option<&str> {
    line.strip_prefix("    ")
        .or_else(|| line.strip_prefix('\t'))
}

fn blockquote(label: &str, body: &[String]) -> Vec<String> {
    let quote = |l: &String| {
        if l.is_empty() {
            ">".to_string()
        } else {
            format!("> {l}")
        }
    };
    let head = format!("> **{label}:**");
    let Some((first, rest)) = body.split_first() else {
        return vec![head];
    };
    if first.trim().is_empty() || Fence::opened_by(first.trim_start(), 0).is_some() {
        return std::iter::once(head)
            .chain(body.iter().map(quote))
            .collect();
    }
    // Other block-level lines need a blank line after the label, or they
    // read as part of the label's paragraph.
    if starts_block(first) {
        return [head, ">".to_string()]
            .into_iter()
            .chain(body.iter().map(quote))
            .collect();
    }
    std::iter::once(format!("{head} {first}"))
        .chain(rest.iter().map(quote))
        .collect()
}

/// A line that opens a block of its own, so it can't share a line with
/// an admonition's label.
fn starts_block(line: &str) -> bool {
    let t = line.trim_start();
    if is_atx_heading(t) || t.starts_with(['|', '>']) {
        return true;
    }
    if let Some(rest) = t.strip_prefix(['-', '*', '+']) {
        return rest.is_empty() || rest.starts_with([' ', '\t']);
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    (1..=9).contains(&digits)
        && t[digits..].starts_with(['.', ')'])
        && t[digits + 1..]
            .chars()
            .next()
            .is_none_or(|c| c == ' ' || c == '\t')
}

/// CommonMark ATX heading: one to six `#`, then a space, tab or line end.
fn is_atx_heading(line: &str) -> bool {
    let t = line.trim_start();
    if line.len() - t.len() > 3 {
        return false;
    }
    let hashes = t.bytes().take_while(|b| *b == b'#').count();
    (1..=6).contains(&hashes)
        && t[hashes..]
            .chars()
            .next()
            .is_none_or(|c| c == ' ' || c == '\t')
}

/// A Markdown link or image found in a run of prose.
pub(super) struct Link {
    /// Byte range of the whole construct in the scanned text, `!` included
    /// for images. For a link that wraps lines, the range contains the `\n`.
    pub span: Range<usize>,
    /// Link text, verbatim; it keeps any `\n` from a wrapped link.
    pub text: String,
    /// Inline target (`(target)`), or the reference label for `[text][ref]`
    /// and `[text][]` (empty for the collapsed form).
    pub target: String,
    pub image: bool,
    /// `[text][ref]` / `[text][]` rather than `[text](target)`.
    pub reference: bool,
    /// 1-based line the link starts on.
    pub lineno: usize,
}

/// Byte index just past the code span opening at `i`, or `None` when the
/// backtick run at `i` has no matching close (and so is literal text).
pub(super) fn code_span_end(bytes: &[u8], i: usize) -> Option<usize> {
    let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
    let mut j = i + run;
    while j < bytes.len() {
        if bytes[j] == b'`' {
            let close = bytes[j..].iter().take_while(|b| **b == b'`').count();
            if close == run {
                return Some(j + close);
            }
            j += close;
        } else {
            j += 1;
        }
    }
    None
}

/// Find inline and reference-style links and images in prose, skipping
/// code spans. `text` should be a whole paragraph (lines joined with `\n`,
/// first line numbered `first_lineno`): link text may wrap across source
/// lines, so a line-at-a-time scan would see only half a link. The caller
/// is responsible for not feeding it text inside fences.
///
/// A `](` that no `[` opens is an error, since it means a link this
/// scanner failed to recognise.
pub(super) fn find_links(text: &str, first_lineno: usize) -> Result<Vec<Link>, LinkError> {
    scan_links(text, &|pos| {
        first_lineno + text[..pos].matches('\n').count()
    })
}

/// A link the scanner can't read, at the line it sits on.
#[derive(Debug)]
pub(super) struct LinkError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for LinkError {}

/// [`find_links`] with a caller-supplied byte-offset-to-line mapping.
fn scan_links(text: &str, line_of: &dyn Fn(usize) -> usize) -> Result<Vec<Link>, LinkError> {
    let fail = |pos: usize, message: &str| {
        Err(LinkError {
            line: line_of(pos),
            message: message.to_string(),
        })
    };
    let bytes = text.as_bytes();
    let mut links = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'`' => {
                let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
                i = code_span_end(bytes, i).unwrap_or(i + run);
            }
            b']' if bytes.get(i + 1) == Some(&b'(') => {
                return fail(i, "`](` without a matching `[`");
            }
            b'[' => {
                let mut depth = 1;
                let mut nested = false;
                let mut j = i + 1;
                while j < bytes.len() {
                    match bytes[j] {
                        b'\\' => j += 1,
                        b'`' => {
                            if let Some(end) = code_span_end(bytes, j) {
                                j = end;
                                continue;
                            }
                        }
                        b'[' => {
                            depth += 1;
                            nested = true;
                        }
                        b']' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                let close = j;
                let (open, closer) = match bytes.get(close + 1) {
                    Some(b'(') => (b'(', ')'),
                    Some(b'[') => (b'[', ']'),
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                let rest = &text[close + 2..];
                let target_end = if open == b'(' {
                    inline_target_end(rest)
                } else {
                    rest.find([closer, '\n'])
                        .filter(|k| rest[*k..].starts_with(closer))
                }
                .map(|k| close + 2 + k);
                let Some(target_end) = target_end else {
                    if open == b'(' {
                        return fail(close, "unterminated link target");
                    }
                    i += 1;
                    continue;
                };
                if nested {
                    return fail(i, "nested brackets in link text are not supported");
                }
                let image = i > 0 && bytes[i - 1] == b'!' && !is_escaped(bytes, i - 1);
                let raw = text[close + 2..target_end].trim();
                let target = if open == b'(' {
                    let t = raw.split_whitespace().next().unwrap_or("");
                    t.strip_prefix('<')
                        .and_then(|t| t.strip_suffix('>'))
                        .unwrap_or(t)
                } else {
                    raw
                };
                let start = if image { i - 1 } else { i };
                links.push(Link {
                    span: start..target_end + 1,
                    text: text[i + 1..close].to_string(),
                    target: target.to_string(),
                    image,
                    reference: open == b'[',
                    lineno: line_of(start),
                });
                i = target_end + 1;
            }
            _ => i += 1,
        }
    }
    Ok(links)
}

/// An odd run of backslashes before `at` escapes the byte there.
fn is_escaped(bytes: &[u8], at: usize) -> bool {
    bytes[..at]
        .iter()
        .rev()
        .take_while(|b| **b == b'\\')
        .count()
        % 2
        == 1
}

/// Offset in `rest` (the text after `](`) of the `)` that closes an inline
/// link: a destination with balanced parentheses or in `<…>`, then an
/// optional `"…"`, `'…'` or `(…)` title, which may itself contain `)`.
fn inline_target_end(rest: &str) -> Option<usize> {
    let b = rest.as_bytes();
    let skip_blanks = |mut i: usize| {
        while b.get(i).is_some_and(|c| *c == b' ' || *c == b'\t') {
            i += 1;
        }
        i
    };
    // Past the byte equal to `close`, honouring escapes; `None` at a line end.
    let past = |mut i: usize, close: u8| {
        while let Some(&c) = b.get(i) {
            match c {
                b'\n' => return None,
                b'\\' => i += 2,
                _ if c == close => return Some(i + 1),
                _ => i += 1,
            }
        }
        None
    };
    let mut i = skip_blanks(0);
    if b.get(i) == Some(&b'<') {
        i = past(i + 1, b'>')?;
    } else {
        let mut depth = 0usize;
        while let Some(&c) = b.get(i) {
            match c {
                b'\\' => i += 1,
                b'(' => depth += 1,
                b')' if depth == 0 => break,
                b')' => depth -= 1,
                b' ' | b'\t' | b'\n' => break,
                _ => {}
            }
            i += 1;
        }
    }
    i = skip_blanks(i);
    if let Some(close) = match b.get(i) {
        Some(b'"') => Some(b'"'),
        Some(b'\'') => Some(b'\''),
        Some(b'(') => Some(b')'),
        _ => None,
    } {
        i = skip_blanks(past(i + 1, close)?);
    }
    (b.get(i) == Some(&b')')).then_some(i)
}

/// Values of raw HTML `href=` / `src=` attributes, quoted or not, with
/// their byte ranges. Prefixed names such as `data-src` and `xlink:href`
/// count too; `srcset` doesn't, since its value isn't a single URL.
pub(super) fn html_targets(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut out = Vec::new();
    for attr in ["href=", "src="] {
        let mut from = 0;
        while let Some(k) = lower[from..].find(attr) {
            let at = from + k;
            from = at + attr.len();
            let name_start = at
                - bytes[..at]
                    .iter()
                    .rev()
                    .take_while(|b| b.is_ascii_alphanumeric() || b"-_:.".contains(b))
                    .count();
            let prefixed = name_start < at && b"-:".contains(&bytes[at - 1]);
            if (name_start < at && !prefixed)
                || (name_start > 0 && !bytes[name_start - 1].is_ascii_whitespace())
            {
                continue;
            }
            let (start, len) = match text[from..].chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let Some(len) = text[from + 1..].find(quote) else {
                        continue;
                    };
                    (from + 1, len)
                }
                _ => {
                    let mut len = text[from..]
                        .find(|c: char| c.is_whitespace() || c == '>')
                        .unwrap_or(text.len() - from);
                    // `<a href=x.md/>`: the `/` closes the tag, it isn't part of the value.
                    if text[from..from + len].ends_with('/') && text[from + len..].starts_with('>')
                    {
                        len -= 1;
                    }
                    if len == 0 {
                        continue;
                    }
                    (from, len)
                }
            };
            out.push((start..start + len, text[start..start + len].to_string()));
            from = start + len;
        }
    }
    out.sort_by_key(|(r, _)| r.start);
    out
}

fn is_remote(target: &str) -> bool {
    target.starts_with("http://") || target.starts_with("https://")
}

/// Docs-internal links would dangle once the text leaves the docs site,
/// so they collapse to their text; remote inline links still resolve and
/// stay. Reference links always collapse: their definitions (or, for
/// mkdocstrings autorefs, the API page) don't travel with the section.
fn rewrite_links(text: &str, line_of: &dyn Fn(usize) -> usize) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for link in scan_links(text, line_of)? {
        if !link.reference && is_remote(&link.target) {
            continue;
        }
        if link.image {
            bail!(
                "line {}: image `{}` can't ship in a skill reference",
                link.lineno,
                link.target
            );
        }
        out.push_str(&text[last..link.span.start]);
        out.push_str(&link.text);
        last = link.span.end;
    }
    out.push_str(&text[last..]);
    Ok(out)
}

/// Drop the `{…}` blocks Python-Markdown's `attr_list` would consume and
/// keep every other `{…}` as the literal text mkdocs renders. It consumes
/// one right after a link or code span, one trailing a heading, and a
/// paragraph's last line when that line is nothing else.
fn strip_attr_lists(para: &str, line_of: &dyn Fn(usize) -> usize) -> Result<String> {
    let link_ends: Vec<usize> = scan_links(para, line_of)?
        .iter()
        .map(|link| link.span.end)
        .collect();
    let lines: Vec<&str> = para.split('\n').collect();
    let heading = lines.len() == 1 && is_atx_heading(lines[0]);
    let mut out = Vec::with_capacity(lines.len());
    let mut base = 0;
    for (n, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let closes_para = n > 0 && n + 1 == lines.len();
        if closes_para && attr_list_len(trimmed) == Some(trimmed.len()) {
            out.push(String::new());
        } else {
            let ctx = AttrLine {
                line,
                base,
                heading,
                link_ends: &link_ends,
                lineno: line_of(base),
            };
            out.push(ctx.strip()?);
        }
        base += line.len() + 1;
    }
    Ok(out.join("\n"))
}

struct AttrLine<'a> {
    line: &'a str,
    /// Offset of `line` in the paragraph, which `link_ends` is relative to.
    base: usize,
    heading: bool,
    link_ends: &'a [usize],
    lineno: usize,
}

impl AttrLine<'_> {
    fn strip(&self) -> Result<String> {
        let line = self.line;
        let bytes = line.as_bytes();
        let mut code_end = None;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'`' {
                let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
                match code_span_end(bytes, i) {
                    Some(end) => {
                        code_end = Some(end);
                        i = end;
                    }
                    None => i += run,
                }
                continue;
            }
            if bytes[i] == b'{'
                && !is_escaped(bytes, i)
                && let Some(len) = attr_list_len(&line[i..])
                && let Some(kept) = self.apply(i, len, code_end == Some(i))?
            {
                return Ok(kept);
            }
            i += 1;
        }
        Ok(line.to_string())
    }

    /// The line with the attribute list at `at..at + len` removed, or
    /// `None` when `attr_list` leaves it as literal text.
    fn apply(&self, at: usize, len: usize, after_code: bool) -> Result<Option<String>> {
        let line = self.line;
        let end = at + len;
        let prev = line[..at].chars().next_back();
        let trailing = line[end..].trim().is_empty();
        let inline = after_code || self.link_ends.contains(&(self.base + at));
        let in_heading = self.heading && trailing && prev.is_some_and(|c| c == ' ' || c == '\t');
        let in_cell = line.trim_start().starts_with('|')
            && prev.is_some_and(|c| c == ' ' || c == '\t')
            && (trailing || line[end..].trim_start().starts_with('|'));
        let after_other = matches!(prev, Some('*' | '_' | '>'));
        if !(inline || in_heading || in_cell || after_other) {
            return Ok(None);
        }
        let what = &line[at..end];
        if in_cell || after_other {
            bail!(
                "line {}: unsupported attribute list placement: {what}",
                self.lineno
            );
        }
        // attr_list reads to the line's last `}`, so the content holds any
        // braces between; what that yields isn't worth mirroring.
        if what[1..what.len() - 1].contains(['{', '}']) {
            bail!("line {}: ambiguous attribute list: {what}", self.lineno);
        }
        if in_heading {
            return Ok(Some(line[..at].trim_end().to_string()));
        }
        Ok(Some(format!("{}{}", &line[..at], &line[end..])))
    }
}

/// Length of the Python-Markdown attribute list opening `text`: `{`, an
/// optional `:`, content that starts with neither `}` nor a space, and the
/// line's last `}`.
fn attr_list_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('{')?;
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    let content = rest.trim_start_matches(' ');
    if content.starts_with('}') || content.is_empty() {
        return None;
    }
    let line_end = text.find('\n').unwrap_or(text.len());
    let close = text[..line_end].rfind('}')?;
    (close > text.len() - content.len()).then_some(close + 1)
}

#[cfg(test)]
mod tests;
