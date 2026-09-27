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

/// Guards against an include cycle between spliced Markdown snippets.
const MAX_INCLUDE_DEPTH: usize = 8;

/// Read `repo_root/doc_rel`, find `section`, and render it. Includes
/// resolve against `repo_root`, matching `pymdownx.snippets`' `base_path: .`.
pub fn extract(repo_root: &Path, doc_rel: &str, section: &str) -> Result<String> {
    let doc_path = repo_root.join(doc_rel);
    let doc = std::fs::read_to_string(&doc_path)
        .with_context(|| format!("reading {}", doc_path.display()))?;
    let ctx = || format!("{doc_rel}: section `{section}`");
    let (first_line, body) = locate_section(&doc, section).with_context(ctx)?;
    let resolve = |p: &str| {
        let path = repo_root.join(p);
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
    };
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
            para.push(item);
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
                if next.text.trim().is_empty() || next.text.starts_with("    ") {
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
                .map(|b| format!("{}\n", b.text.strip_prefix("    ").unwrap_or("")))
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
    let mut joined = String::new();
    let mut starts: Vec<(usize, usize)> = Vec::with_capacity(para.len());
    for (n, item) in para.iter().enumerate() {
        if n > 0 {
            joined.push('\n');
        }
        starts.push((joined.len(), item.lineno));
        joined.push_str(&item.text);
    }
    let line_of = |pos: usize| {
        let k = starts.partition_point(|(off, _)| *off <= pos);
        starts[k.saturating_sub(1)].1
    };
    let rewritten = rewrite_links(&joined, &line_of)?;
    for line in rewritten.split('\n') {
        let line = strip_attr_lists(line);
        if !line.trim().is_empty() {
            out.push(line);
        }
    }
    para.clear();
    Ok(())
}

/// `[label]: target` at the start of a line (up to 3 spaces of indent).
fn is_reference_definition(text: &str) -> bool {
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
struct Fence {
    ch: char,
    len: usize,
    line: usize,
}

impl Fence {
    fn opened_by(trimmed: &str, line: usize) -> Option<Self> {
        let ch = trimmed.chars().next().filter(|c| *c == '`' || *c == '~')?;
        let len = trimmed.chars().take_while(|c| *c == ch).count();
        // CommonMark: a backtick fence's info string can't contain a
        // backtick, so "```x``` more" is inline code in prose.
        if len < 3 || (ch == '`' && trimmed[len..].contains('`')) {
            return None;
        }
        Some(Self { ch, len, line })
    }

    fn closed_by(self, trimmed: &str) -> bool {
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
        return Ok(Some(Include {
            path: path.to_string(),
            kind: IncludeKind::Named(name.to_string()),
        }));
    }
    Ok(Some(Include {
        path: inner.to_string(),
        kind: IncludeKind::Whole,
    }))
}

/// Generated `.md` snippets carry their own `DO_NOT_EDIT` header line;
/// splicing it into the middle of a skill reference reads as if the
/// reference itself broke off mid-page, so it's dropped on include.
const DO_NOT_EDIT: &str = "<!-- generated by `cargo xtask build-skill-refs`; do not edit by hand -->";

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
            Ok(lines[*a - 1..*b].iter().map(|l| (*l).to_string()).collect())
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
            Ok(lines)
        }
    }
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

fn blockquote(label: &str, body: &[String]) -> Vec<String> {
    let quote = |l: &String| {
        if l.is_empty() {
            ">".to_string()
        } else {
            format!("> {l}")
        }
    };
    let head = format!("> **{label}:**");
    match body.split_first() {
        Some((first, rest))
            if !first.trim().is_empty() && Fence::opened_by(first.trim_start(), 0).is_none() =>
        {
            std::iter::once(format!("{head} {first}"))
                .chain(rest.iter().map(quote))
                .collect()
        }
        _ => std::iter::once(head)
            .chain(body.iter().map(quote))
            .collect(),
    }
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
fn code_span_end(bytes: &[u8], i: usize) -> Option<usize> {
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

fn in_code_span(text: &str, pos: usize) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < pos {
        if bytes[i] == b'`' {
            let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
            match code_span_end(bytes, i) {
                Some(end) if end > pos => return true,
                Some(end) => i = end,
                None => i += run,
            }
        } else {
            i += 1;
        }
    }
    false
}

/// Find inline and reference-style links and images in prose, skipping
/// code spans. `text` should be a whole paragraph (lines joined with `\n`,
/// first line numbered `first_lineno`): link text may wrap across source
/// lines, so a line-at-a-time scan would see only half a link. The caller
/// is responsible for not feeding it text inside fences.
///
/// A `](` that no `[` opens is an error, since it means a link this
/// scanner failed to recognise.
// Consumed by the skill link lint; only tests call it until that lands.
#[cfg_attr(not(test), allow(dead_code))]
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
                let target_end = rest
                    .find([closer, '\n'])
                    .filter(|k| rest[*k..].starts_with(closer))
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
                let image = i > 0 && bytes[i - 1] == b'!';
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

fn strip_attr_lists(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut last = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let run = bytes[i..].iter().take_while(|b| **b == b'`').count();
            i = code_span_end(bytes, i).unwrap_or(i + run);
            continue;
        }
        if bytes[i..].starts_with(b"{:") {
            if let Some(k) = line[i..].find('}') {
                out.push_str(line[last..i].trim_end());
                last = i + k + 1;
                i = last;
                continue;
            }
        }
        i += 1;
    }
    let removed = last > 0;
    out.push_str(&line[last..]);
    let mut out = if removed {
        out.trim_end().to_string()
    } else {
        out
    };
    // attr_list also accepts the colon-less `{#id}` / `{ .class }` spelling,
    // but only as a trailing block, where it can't be ordinary braces.
    let t = out.trim_end();
    if let Some(open) = t.strip_suffix('}').and_then(|b| b.rfind('{')) {
        let inner = t[open + 1..t.len() - 1].trim();
        if (inner.starts_with('#') || inner.starts_with('.')) && !in_code_span(t, open) {
            out = t[..open].trim_end().to_string();
        }
    }
    out
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
        let file = "# --8<-- [start:x]\na\n# --8<-- [end:x]\n# --8<-- [start:x]\nb\n# --8<-- [end:x]\n";
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
    fn attr_list_is_dropped() {
        let src = "Heading text {: #custom }\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), "Heading text\n");
    }

    #[test]
    fn attr_list_and_links_next_to_non_ascii_text() {
        let src = "Título — [ç](é.md) {: #t }\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), "Título — ç\n");
    }

    #[test]
    fn attr_list_line_is_dropped() {
        let src = "Para.\n{: .note }\nNext.\n";
        assert_eq!(transform(src, &fake(&[])).unwrap(), "Para.\nNext.\n");
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
    fn colonless_trailing_attr_lists_are_dropped() {
        let src = "### Output Options {#output-options}\n\nTitle { .cls }\n\n{#only}\n";
        let out = transform(src, &fake(&[])).unwrap();
        assert_eq!(out, "### Output Options\n\nTitle\n");
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
}
