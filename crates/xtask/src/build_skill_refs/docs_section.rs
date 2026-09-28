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
pub(super) fn find_links(text: &str, first_lineno: usize) -> Result<Vec<Link>> {
    scan_links(text, &|pos| {
        first_lineno + text[..pos].matches('\n').count()
    })
}

/// [`find_links`] with a caller-supplied byte-offset-to-line mapping.
fn scan_links(text: &str, line_of: &dyn Fn(usize) -> usize) -> Result<Vec<Link>> {
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
                bail!("line {}: `](` without a matching `[`", line_of(i));
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
                        bail!("line {}: unterminated link target", line_of(close));
                    }
                    i += 1;
                    continue;
                };
                if nested {
                    bail!(
                        "line {}: nested brackets in link text are not supported",
                        line_of(i)
                    );
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
/// their byte ranges.
pub(super) fn html_targets(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    for attr in ["href=", "src="] {
        let mut from = 0;
        while let Some(k) = lower[from..].find(attr) {
            let at = from + k;
            from = at + attr.len();
            if at > 0 && !lower.as_bytes()[at - 1].is_ascii_whitespace() {
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
                    let len = text[from..]
                        .find(|c: char| c.is_whitespace() || c == '>')
                        .unwrap_or(text.len() - from);
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
            if bytes[i] == b'{' && !is_escaped(bytes, i) {
                if let Some(len) = attr_list_len(&line[i..]) {
                    if let Some(kept) = self.apply(i, len, code_end == Some(i))? {
                        return Ok(kept);
                    }
                }
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
mod tests {
    use super::*;

    fn fake<'a>(files: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Result<String> + 'a {
        move |p: &str| {
            files
                .iter()
                .find(|(k, _)| *k == p)
                .map(|(_, v)| v.to_string())
                .ok_or_else(|| anyhow::anyhow!("no such file {p}"))
        }
    }

    #[test]
    fn section_bounds_are_exclusive_and_comment_wrapped() {
        let doc = "a\n<!-- --8<-- [start:s] -->\nkeep\n<!-- --8<-- [end:s] -->\nb\n";
        assert_eq!(find_section(doc, "s").unwrap(), "keep\n");
    }

    #[test]
    fn bare_markers_also_delimit_a_section() {
        let doc = "--8<-- [start:s]\nkeep\n--8<-- [end:s]\n";
        assert_eq!(find_section(doc, "s").unwrap(), "keep\n");
    }

    #[test]
    fn missing_end_marker_is_an_error() {
        assert!(find_section("<!-- --8<-- [start:s] -->\nx\n", "s").is_err());
    }

    #[test]
    fn missing_start_marker_is_an_error() {
        assert!(find_section("x\n<!-- --8<-- [end:s] -->\n", "s").is_err());
    }

    #[test]
    fn duplicate_section_is_an_error() {
        let doc = "--8<-- [start:s]\na\n--8<-- [end:s]\n--8<-- [start:s]\nb\n--8<-- [end:s]\n";
        assert!(find_section(doc, "s").is_err());
    }

    #[test]
    fn section_name_must_match_exactly() {
        let doc = "--8<-- [start:ss]\na\n--8<-- [end:ss]\n";
        assert!(find_section(doc, "s").is_err());
    }

    #[test]
    fn include_with_line_range_is_inclusive_and_keeps_indent() {
        let src = "```python\n    --8<-- \"f.py:2:3\"\n```\n";
        let out = transform(src, &fake(&[("f.py", "l1\nl2\nl3\nl4\n")])).unwrap();
        assert_eq!(out, "```python\n    l2\n    l3\n```\n");
    }

    #[test]
    fn include_range_out_of_bounds_is_an_error() {
        let src = "--8<-- \"f.py:3:9\"\n";
        assert!(transform(src, &fake(&[("f.py", "a\nb\nc\n")])).is_err());
    }

    #[test]
    fn include_range_starting_at_zero_is_an_error() {
        let src = "--8<-- \"f.py:0:1\"\n";
        assert!(transform(src, &fake(&[("f.py", "a\n")])).is_err());
    }

    #[test]
    fn whole_file_include_drops_trailing_newline() {
        let src = "```python\n--8<-- \"f.py\"\n```\n";
        let out = transform(src, &fake(&[("f.py", "a = 1\n\nb = 2\n")])).unwrap();
        assert_eq!(out, "```python\na = 1\n\nb = 2\n```\n");
    }

    #[test]
    fn included_fence_content_is_not_transformed() {
        let src = "```python\n--8<-- \"f.py\"\n```\n";
        let body = "x = \"[a](b.md)\"  # {: .c }\n!!! note\n";
        let out = transform(src, &fake(&[("f.py", body)])).unwrap();
        assert_eq!(out, format!("```python\n{body}```\n"));
    }

    #[test]
    fn included_markdown_outside_fence_is_transformed() {
        let src = "Intro.\n\n--8<-- \"t.md\"\n";
        let md = "<!-- generated -->\n\n| A |\n|---|\n| [x](y.md) |\n";
        let out = transform(src, &fake(&[("t.md", md)])).unwrap();
        assert_eq!(out, "Intro.\n\n<!-- generated -->\n\n| A |\n|---|\n| x |\n");
    }

    #[test]
    fn missing_include_file_is_an_error() {
        assert!(transform("--8<-- \"nope.py\"\n", &fake(&[])).is_err());
    }

    #[test]
    fn named_include_inside_fence_uses_hash_markers() {
        let src = "```python\n--8<-- \"f.py:x\"\n```\n";
        let file = "a\n# --8<-- [start:x]\nb\nc\n# --8<-- [end:x]\nd\n";
        let out = transform(src, &fake(&[("f.py", file)])).unwrap();
        assert_eq!(out, "```python\nb\nc\n```\n");
    }

    #[test]
    fn named_include_outside_fence_is_transformed() {
        let src = "Intro.\n\n--8<-- \"t.md:x\"\n";
        let file = "<!-- --8<-- [start:x] -->\nBody [a](b.md).\n<!-- --8<-- [end:x] -->\n";
        let out = transform(src, &fake(&[("t.md", file)])).unwrap();
        assert_eq!(out, "Intro.\n\nBody a.\n");
    }

    #[test]
    fn named_include_trims_blank_lines_at_the_section_edges() {
        let src = "```python\n--8<-- \"f.py:x\"\n```\n";
        let file = "a\n# --8<-- [start:x]\n\nb\n\n# --8<-- [end:x]\nc\n";
        let out = transform(src, &fake(&[("f.py", file)])).unwrap();
        assert_eq!(out, "```python\nb\n```\n");
    }

    #[test]
    fn named_include_missing_name_is_an_error() {
        let err = transform("--8<-- \"f.py:missing\"\n", &fake(&[("f.py", "a\n")]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 1"), "{err}");
        assert!(err.contains("missing"), "{err}");
    }

    #[test]
    fn named_include_duplicate_name_is_an_error() {
        let file =
            "# --8<-- [start:x]\na\n# --8<-- [end:x]\n# --8<-- [start:x]\nb\n# --8<-- [end:x]\n";
        assert!(transform("--8<-- \"f.py:x\"\n", &fake(&[("f.py", file)])).is_err());
    }

    #[test]
    fn leading_do_not_edit_comment_is_dropped_from_included_md() {
        let src = "Intro.\n\n--8<-- \"t.md\"\n";
        let md = "<!-- generated by `cargo xtask build-skill-refs`; do not edit by hand -->\n\n\
                  | A |\n|---|\n| x |\n";
        let out = transform(src, &fake(&[("t.md", md)])).unwrap();
        assert_eq!(out, "Intro.\n\n| A |\n|---|\n| x |\n");
    }

    #[test]
    fn fenced_blocks_pass_through_byte_for_byte() {
        let src = "~~~text\n!!! note\n    body\n??? tip\n=== \"Tab\"\n<div>\nx {: .y }\n~~~\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn longer_fence_is_only_closed_by_matching_run() {
        let src = "````md\n```\n[a](b.md)\n```\n````\n[c](d.md)\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(out, "````md\n```\n[a](b.md)\n```\n````\nc\n");
    }

    #[test]
    fn doc_links_become_plain_text_http_links_stay() {
        let src = "See [ctx](context.md#run), [top](#x) and [web](https://e.com).\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(out, "See ctx, top and [web](https://e.com).\n");
    }

    #[test]
    fn relative_parent_links_become_plain_text() {
        let out = transform("Go [up](../x/y.md) now.\n", &fake(&[])).unwrap();
        assert_eq!(out, "Go up now.\n");
    }

    #[test]
    fn links_inside_fences_are_untouched() {
        let src = "```python\nx = \"[a](b.md)\"\n```\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn links_inside_inline_code_are_untouched() {
        let src = "Write `[a](b.md)` literally.\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn nested_brackets_in_link_text_are_an_error() {
        let err = transform("x\n[a [b] c](d.md)\n", &fake(&[]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 2"), "{err}");
    }

    #[test]
    fn brackets_without_a_target_are_left_alone() {
        let src = "A list[str] value and [x] box.\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn local_image_is_rejected() {
        assert!(transform("![d](imgs/x.png)\n", &fake(&[])).is_err());
    }

    #[test]
    fn remote_image_is_kept() {
        let src = "![d](https://e.com/x.png)\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn find_links_reports_spans_and_targets() {
        let line = "a [b](c.md#d) `[e](f)` ![g](h.png)";
        let links = find_links(line, 1).unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(&line[links[0].span.clone()], "[b](c.md#d)");
        assert_eq!(links[0].text, "b");
        assert_eq!(links[0].target, "c.md#d");
        assert!(!links[0].image);
        assert_eq!(&line[links[1].span.clone()], "![g](h.png)");
        assert_eq!(links[1].target, "h.png");
        assert!(links[1].image);
    }

    #[test]
    fn admonition_becomes_blockquote() {
        let src = "!!! note \"Shell tab completion\"\n    Line one.\n    Line two.\n\nAfter.\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(
            out,
            "> **Note — Shell tab completion:** Line one.\n> Line two.\n\nAfter.\n",
        );
    }

    #[test]
    fn admonition_without_title() {
        let src = "!!! warning\n    Careful.\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(out, "> **Warning:** Careful.\n");
    }

    #[test]
    fn admonition_body_is_transformed_and_keeps_inner_blank_lines() {
        let src = "!!! tip\n    See [x](y.md).\n\n        indented\n\nOut.\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(out, "> **Tip:** See x.\n>\n>     indented\n\nOut.\n");
    }

    #[test]
    fn admonition_starting_with_a_fence_puts_the_label_on_its_own_line() {
        let src = "!!! example\n    ```python\n    x = 1\n    ```\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(out, "> **Example:**\n> ```python\n> x = 1\n> ```\n");
    }

    #[test]
    fn heading_attr_list_is_dropped_and_prose_one_kept() {
        let src = "## Heading text {: #custom }\n\nPara text {: #p }\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(out, "## Heading text\n\nPara text {: #p }\n");
    }

    #[test]
    fn attr_list_and_links_next_to_non_ascii_text() {
        let src = "Título — [ç](é.md){: #t }\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), "Título — ç\n");
    }

    #[test]
    fn attr_list_line_closing_a_paragraph_is_dropped() {
        let src = "Para.\n{: .note }\n\nNext.\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), "Para.\n\nNext.\n");
    }

    #[test]
    fn attr_list_line_mid_paragraph_is_kept() {
        let src = "Para.\n{: .note }\nNext.\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn html_comments_pass_through() {
        let src = "<!-- generated by `x`; do not edit by hand -->\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn unknown_constructs_are_rejected_with_line_number() {
        for bad in [
            "??? note \"x\"\n",
            "???+ note \"x\"\n",
            "=== \"Tab\"\n",
            "<details>\n",
            "<div class=\"x\">\n",
            "--8<-- \"f.py:5\"\n",
            "--8<-- f.py\n",
            "  !!! note\n",
            "!!! note inline end\n",
        ] {
            let err = transform(bad, &fake(&[("f.py", "a\n")]))
                .unwrap_err()
                .to_string();
            assert!(err.contains("line 1"), "{bad:?} -> {err}");
        }
    }

    #[test]
    fn nested_section_markers_are_dropped() {
        let src = "a\n<!-- --8<-- [start:inner] -->\nb\n<!-- --8<-- [end:inner] -->\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), "a\nb\n");
    }

    #[test]
    fn output_is_trimmed_and_ends_with_one_newline() {
        let src = "\n\n  \nBody.\n\n\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), "Body.\n");
    }

    #[test]
    fn extract_reads_the_doc_and_resolves_includes_from_repo_root() {
        let root = std::env::temp_dir().join(format!("xtask-docs-section-{}", std::process::id()));
        std::fs::create_dir_all(root.join("docs/files")).unwrap();
        std::fs::write(
            root.join("docs/page.md"),
            "# P\n<!-- --8<-- [start:s] -->\nSee [c](c.md).\n\n```python\n--8<-- \"docs/files/x.py:2:2\"\n```\n<!-- --8<-- [end:s] -->\n",
        )
        .unwrap();
        std::fs::write(root.join("docs/files/x.py"), "a\nb\nc\n").unwrap();
        let out = extract(&root, "docs/page.md", "s");
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(out.unwrap(), "See c.\n\n```python\nb\n```\n");
    }

    #[test]
    fn extract_errors_name_the_doc_line() {
        let root =
            std::env::temp_dir().join(format!("xtask-docs-section-err-{}", std::process::id()));
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(
            root.join("docs/page.md"),
            "a\nb\n--8<-- [start:s]\nok\n=== \"Tab\"\n--8<-- [end:s]\n",
        )
        .unwrap();
        let out = extract(&root, "docs/page.md", "s");
        std::fs::remove_dir_all(&root).unwrap();
        let err = format!("{:#}", out.unwrap_err());
        assert!(err.contains("line 5"), "{err}");
    }

    /// Lay out the reported page shape — prose, a bare generated-`.md`
    /// include, then whole/range/named code includes — with every source
    /// file's newlines rewritten to `eol`, and extract the section.
    fn extract_with_eol(tag: &str, eol: &str) -> String {
        let root = std::env::temp_dir().join(format!(
            "xtask-docs-section-eol-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("docs/files")).unwrap();
        let files = [
            (
                "docs/page.md",
                "# P\n<!-- --8<-- [start:s] -->\nOpt-in checks through\n`Annotated[Path, arg(...)]`:\n\n\
                 --8<-- \"docs/files/path-constraints.md\"\n\n\
                 ```python\n--8<-- \"docs/files/x.py\"\n```\n\n\
                 ```python\n--8<-- \"docs/files/x.py:2:3\"\n```\n\n\
                 ```python\n--8<-- \"docs/files/x.py:n\"\n```\n\
                 <!-- --8<-- [end:s] -->\n",
            ),
            (
                "docs/files/path-constraints.md",
                "<!-- generated by `cargo xtask build-skill-refs`; do not edit by hand -->\n\n\
                 | Constraint | Effect |\n|---|---|\n| a | b |\n",
            ),
            (
                "docs/files/x.py",
                "a = 1\nb = 2\n\n# --8<-- [start:n]\n\nc = 3\n\n# --8<-- [end:n]\n",
            ),
        ];
        for (rel, body) in files {
            std::fs::write(root.join(rel), body.replace('\n', eol)).unwrap();
        }
        let out = extract(&root, "docs/page.md", "s");
        std::fs::remove_dir_all(&root).unwrap();
        out.unwrap()
    }

    #[test]
    fn crlf_sources_extract_byte_identically_to_lf() {
        let lf = extract_with_eol("lf", "\n");
        assert!(lf.contains(":\n\n| Constraint"), "{lf}");
        assert_eq!(extract_with_eol("crlf", "\r\n"), lf);
    }

    #[test]
    fn lone_cr_sources_extract_byte_identically_to_lf() {
        assert_eq!(
            extract_with_eol("cr", "\r"),
            extract_with_eol("cr-lf", "\n")
        );
    }

    #[test]
    fn link_wrapped_across_lines_is_flattened() {
        let src = "See the [`*args` for variadic\npositionals](#args) section.\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(out, "See the `*args` for variadic\npositionals section.\n");
    }

    #[test]
    fn wrapped_link_to_other_page_is_flattened() {
        let src = "Runs (see [Project configuration → sync\ninteraction](project-config.md#x)).\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(
            out,
            "Runs (see Project configuration → sync\ninteraction).\n"
        );
    }

    #[test]
    fn unmatched_close_bracket_paren_is_an_error() {
        let err = transform("Fine.\n\nx\ny](z.md)\n", &fake(&[]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 4"), "{err}");
    }

    #[test]
    fn reference_links_and_autorefs_are_flattened() {
        let src = "Receives a [`Context`][toolr.Context] and [x][].\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(out, "Receives a `Context` and x.\n");
    }

    #[test]
    fn reference_definitions_are_dropped() {
        let src = "A [PEP][pep-420] ref.\n\n[pep-420]: https://peps.python.org/pep-0420/\n[l]: other.md\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(out, "A PEP ref.\n");
    }

    #[test]
    fn find_links_reports_reference_links() {
        let links = find_links("a [b][c.d] e\n[f][]", 3).unwrap();
        assert_eq!(links.len(), 2);
        assert!(links[0].reference && links[1].reference);
        assert_eq!(links[0].target, "c.d");
        assert_eq!(links[1].target, "");
        assert_eq!((links[0].lineno, links[1].lineno), (3, 4));
    }

    #[test]
    fn reference_image_is_rejected() {
        assert!(transform("![d][img]\n", &fake(&[])).is_err());
    }

    #[test]
    fn unclosed_fence_is_an_error_naming_the_opener() {
        let err = transform("a\n\n```python\nx = 1\n", &fake(&[]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 3") && err.contains("unclosed"), "{err}");
    }

    #[test]
    fn inline_triple_backtick_code_is_not_a_fence() {
        let src = "```x``` and [a](b.md)\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), "```x``` and a\n");
    }

    #[test]
    fn colonless_attr_list_is_dropped_only_where_attr_list_applies() {
        let src = "### Output Options {#output-options}\n\nTitle { .cls }\n\n{#only}\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(out, "### Output Options\n\nTitle { .cls }\n\n{#only}\n");
    }

    #[test]
    fn trailing_prose_braces_are_kept() {
        let src = "Para one\nlast {#x}\n\nratio {.5}\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn ordinary_braces_are_kept() {
        let src = "Keep {braces} here and `{#x}`\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn error_after_include_names_the_page_line() {
        let src = "--8<-- \"s.md\"\ny](z.md)\n";
        let err = transform(src, &fake(&[("s.md", "a\nb\nc\n")]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 2:"), "{err}");
    }

    #[test]
    fn unterminated_link_target_is_reported_as_such() {
        let err = transform("[a](b\n", &fake(&[])).unwrap_err().to_string();
        assert!(
            err.contains("line 1") && err.contains("unterminated link target"),
            "{err}"
        );
    }

    #[test]
    fn end_marker_before_start_marker_is_an_error() {
        let err = find_section("--8<-- [end:s]\nx\n--8<-- [start:s]\n", "s")
            .unwrap_err()
            .to_string();
        assert!(err.contains("comes before"), "{err}");
    }

    #[test]
    fn self_including_snippet_hits_the_depth_limit() {
        let err = transform("--8<-- \"a.md\"\n", &fake(&[("a.md", "--8<-- \"a.md\"\n")]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("nested deeper than 8"), "{err}");
    }

    #[test]
    fn admonition_without_a_kind_is_an_error() {
        let err = transform("!!!\n    Body.\n", &fake(&[]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 1: admonition without a kind"), "{err}");
    }

    #[test]
    fn admonition_with_empty_title_uses_the_kind_alone() {
        let out = transform("!!! note \"\"\n    Body.\n", &fake(&[])).unwrap();
        assert_eq!(out, "> **Note:** Body.\n");
    }

    #[test]
    fn code_span_end_skips_shorter_backtick_runs() {
        assert_eq!(code_span_end(b"`a``b", 0), None);
        assert_eq!(code_span_end(b"``a`b``c", 0), Some(7));
        let src = "A `` b ` c `` and [x](y.md)\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), "A `` b ` c `` and x\n");
    }

    #[test]
    fn attr_list_right_after_code_span_is_dropped() {
        let out = transform("x `a`{#b} y\n", &fake(&[])).unwrap();
        assert_eq!(out, "x `a` y\n");
    }

    #[test]
    fn attr_list_after_a_space_following_code_is_kept() {
        let src = "x `a` {#b}\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn attr_list_after_unmatched_backtick_is_kept() {
        let src = "x ` a {#b}\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn attr_list_lookalike_inside_code_span_is_kept() {
        let src = "`{#a}` }\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn escaped_brackets_are_not_links() {
        let src = "Price \\[x\\] ok [a](b.md)\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), "Price \\[x\\] ok a\n");
    }

    #[test]
    fn escaped_bracket_inside_link_text_does_not_close_it() {
        let out = transform("[a \\] b](c.md)\n", &fake(&[])).unwrap();
        assert_eq!(out, "a \\] b\n");
    }

    #[test]
    fn unmatched_backtick_inside_link_text_is_literal() {
        let out = transform("[a ` b](c.md)\n", &fake(&[])).unwrap();
        assert_eq!(out, "a ` b\n");
    }

    #[test]
    fn unterminated_reference_label_is_left_alone() {
        let src = "[a][b\nc]\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn unclosed_colon_attr_list_is_kept() {
        let src = "a {: b\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn admonition_starting_with_a_block_puts_the_label_on_its_own_line() {
        for (body, first) in [
            ("    - a\n    - b\n", "> - a"),
            ("    1. a\n", "> 1. a"),
            ("    | A |\n    |---|\n", "> | A |"),
            ("    ## Heading\n", "> ## Heading"),
            ("    !!! tip\n        Inner.\n", "> > **Tip:** Inner."),
        ] {
            let out = transform(&format!("!!! note\n{body}"), &fake(&[])).unwrap();
            assert!(
                out.starts_with(&format!("> **Note:**\n>\n{first}\n")),
                "{body:?} -> {out:?}"
            );
        }
    }

    #[test]
    fn admonition_starting_with_prose_that_looks_list_like_stays_inline() {
        let out = transform("!!! note\n    -dash and 2024. year\n", &fake(&[])).unwrap();
        assert_eq!(out, "> **Note:** -dash and 2024. year\n");
    }

    #[test]
    fn link_title_containing_a_paren_keeps_the_whole_link() {
        for src in [
            "[a](b.md \"x (y)\") z\n",
            "[a](b.md 'x (y)') z\n",
            "[a](b.md (x \\) y)) z\n",
            "[a](<b c.md> \"t\") z\n",
        ] {
            assert_eq!(transform(src, &fake(&[])).unwrap(), "a z\n", "{src:?}");
        }
    }

    #[test]
    fn link_destination_with_balanced_parens_is_one_target() {
        let links = find_links("[a](https://x/f(1)) z", 1).unwrap();
        assert_eq!(links[0].target, "https://x/f(1)");
    }

    #[test]
    fn unterminated_link_title_is_an_error() {
        let err = transform("[a](b.md \"x\n", &fake(&[]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("unterminated link target"), "{err}");
    }

    #[test]
    fn tab_indented_admonition_body_is_recognised() {
        let out = transform("!!! note\n\tLine one.\n\tLine two.\n\nAfter.\n", &fake(&[])).unwrap();
        assert_eq!(out, "> **Note:** Line one.\n> Line two.\n\nAfter.\n");
    }

    #[test]
    fn escaped_image_marker_is_a_plain_link() {
        let out = transform("\\![x](y.png)\n", &fake(&[])).unwrap();
        assert_eq!(out, "\\!x\n");
    }

    #[test]
    fn escaped_backslash_before_image_still_leaves_an_image() {
        assert!(transform("\\\\![x](y.png)\n", &fake(&[])).is_err());
    }

    #[test]
    fn heading_is_its_own_paragraph_for_link_scanning() {
        let src = "# H [a\n](b.md)\n";
        let err = transform(src, &fake(&[])).unwrap_err().to_string();
        assert!(
            err.contains("line 2") && err.contains("without a matching"),
            "{err}"
        );
    }

    #[test]
    fn heading_followed_by_prose_renders_both() {
        let out = transform("# H\n[a](b.md) c\n", &fake(&[])).unwrap();
        assert_eq!(out, "# H\na c\n");
    }

    #[test]
    fn ranged_and_named_includes_are_dedented() {
        let file = "class A:\n    # --8<-- [start:x]\n    def f(self):\n\n        pass\n    # --8<-- [end:x]\n";
        let src = "```python\n--8<-- \"f.py:x\"\n```\n";
        let out = transform(src, &fake(&[("f.py", file)])).unwrap();
        assert_eq!(out, "```python\ndef f(self):\n\n    pass\n```\n");
        let src = "```python\n--8<-- \"f.py:3:5\"\n```\n";
        let out = transform(src, &fake(&[("f.py", file)])).unwrap();
        assert_eq!(out, "```python\ndef f(self):\n\n    pass\n```\n");
    }

    #[test]
    fn whole_file_include_is_not_dedented() {
        let src = "```python\n--8<-- \"f.py\"\n```\n";
        let out = transform(src, &fake(&[("f.py", "    x = 1\n")])).unwrap();
        assert_eq!(out, "```python\n    x = 1\n```\n");
    }

    #[test]
    fn local_raw_html_targets_are_rejected() {
        for src in ["See <img src=\"a.png\">.\n", "<a href='x.md'>x</a>\n"] {
            let err = transform(src, &fake(&[])).unwrap_err().to_string();
            assert!(
                err.contains("line 1") && err.contains("raw HTML target"),
                "{err}"
            );
        }
    }

    #[test]
    fn remote_and_code_span_raw_html_is_kept() {
        let src = "<a href=\"https://x.org\">x</a> and `<img src=\"a.png\">`\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), src);
    }

    #[test]
    fn windows_absolute_path_include_is_one_path() {
        let files = [("C:\\x.py", "a\n")];
        let out = transform("```python\n--8<-- \"C:\\x.py\"\n```\n", &fake(&files)).unwrap();
        assert_eq!(out, "```python\na\n```\n");
    }

    #[test]
    fn named_include_after_a_windows_path_still_names_the_section() {
        let file = "# --8<-- [start:sec]\nb\n# --8<-- [end:sec]\n";
        let files = [("C:\\x.py", file)];
        let out = transform("```python\n--8<-- \"C:\\x.py:sec\"\n```\n", &fake(&files)).unwrap();
        assert_eq!(out, "```python\nb\n```\n");
    }

    #[test]
    fn attr_lists_attr_list_would_mishandle_are_errors() {
        for (src, what) in [
            ("| a {#c} |\n", "placement"),
            ("**b**{.c} x\n", "placement"),
            ("`x`{a} {b}\n", "ambiguous"),
        ] {
            let err = transform(src, &fake(&[])).unwrap_err().to_string();
            assert!(
                err.contains("line 1") && err.contains(what),
                "{src:?}: {err}"
            );
        }
    }

    #[test]
    fn attr_list_after_a_remote_link_is_dropped() {
        let out = transform("[l](https://x.org){: .k } rest\n", &fake(&[])).unwrap();
        assert_eq!(out, "[l](https://x.org) rest\n");
    }
}
