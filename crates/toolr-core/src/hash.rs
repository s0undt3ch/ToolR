//! Stable hashing over the inputs of the static manifest build.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use blake3::Hasher;
use walkdir::WalkDir;

use crate::manifest::{Content, InputRecord, InputRecords};
use crate::parser::BuiltManifest;
use crate::text::escape_control_chars;
use crate::trust::shown;

/// One file that feeds `static_hash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StaticInput {
    /// The path relative to the project root, components joined with `/`;
    /// a file outside the project root keeps its full path.
    pub key: String,
    pub path: PathBuf,
    pub kind: InputKind,
}

/// How [`snapshot_static_inputs`] treats a [`StaticInput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputKind {
    /// A `tools/**/*.py` module. Only a module's read errors fail the hash.
    Module,
    /// A file an argparse block's `scan_paths` match.
    Scanned,
    /// `tools/pyproject.toml`: only its `[tool.toolr.argparse.*]` blocks
    /// are hashed.
    Pyproject,
}

/// Tag after an input's key: its contents' digest follows.
const TAG_CONTENTS: &[u8] = b"\x00";
/// Tag after an input's key: it couldn't be read; the error kind follows.
const TAG_READ_ERROR: &[u8] = b"\x01";
/// Prefix of the pyproject's digest input: its serialised argparse blocks
/// follow.
const TAG_ARGPARSE_BLOCKS: &[u8] = b"\x02";

/// The manifest's `static_hash`: every input of the static build, hashed in
/// key order so the result is reproducible across runs and machines.
///
/// Fails unwrapped with [`SymlinkedModules`] like [`list_python_files`].
pub fn compute_static_hash(tools_dir: &Path) -> Result<String> {
    Ok(static_hash_of(
        &snapshot_static_inputs(tools_dir, None)?.files,
    ))
}

/// Stat every static input and hash the ones `previous` can't vouch for.
///
/// A record from `previous` is reused, without reading the file, when its
/// key, size, mtime, ctime, inode and device all match the live file and
/// its mtime is strictly older than `previous.snapshot_at_ns`, the clock
/// of `tools/`'s filesystem just before that snapshot's first stat (see
/// [`SnapshotClock`]). A file saved in the same timestamp tick as its stat
/// could have changed again without changing its stat, so it is always
/// reread (git's racy-index rule). A `snapshot_at_ns` of `0` reuses
/// nothing. The stat is taken before the read: a file that changes in
/// between keeps its old stat in the record, so the next check rereads it.
///
/// The bound is only comparable with mtimes from the same filesystem, so
/// only inputs under the project root on `tools/`'s device
/// (`previous.snapshot_dev`) are reused; any other input is always reread.
/// Off Unix there is no device number, so only the project-root rule
/// applies.
///
/// The returned records carry no bound: only a caller that persists them
/// takes one, before building.
///
/// `tools/pyproject.toml` is read at most once, for both its digest and
/// its `scan_paths` (twice when a `scan_paths` glob also matches it, since
/// it is then recorded whole, as a scanned file); when its record is
/// reused, so are the `scan_paths`.
/// The globs are expanded on every call, so a scanned file that appears or
/// vanishes changes the result.
///
/// The argparse config never fails the snapshot, or a typo in it would
/// fail `--help`, dispatch and TAB through the freshness check. A
/// pyproject that doesn't parse, or a block with a bad glob, adds no
/// scanned files; the pyproject itself is still an input, so fixing it
/// rebuilds, and the build reports the error.
///
/// A module that can't be read fails the snapshot, naming the file. Any
/// other input that can't be read gets a record carrying the error kind
/// instead: the scanner turns such a file into a placeholder with a
/// warning, and the pyproject's read error is the build's to report, so
/// neither may fail `--help`, dispatch and TAB. Such a record is never
/// reused, since fixing the permission changes neither size nor mtime.
pub fn snapshot_static_inputs(
    tools_dir: &Path,
    previous: Option<&InputRecords>,
) -> Result<InputRecords> {
    snapshot_from(tools_dir, &list_python_files(tools_dir)?, previous)
}

/// The bound a persisting build stamps on its records: the clock and
/// device of `tools/`'s filesystem, read before the build's first stat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotClock {
    /// ns since the Unix epoch; `0`, which reuses nothing, when unknown.
    pub at_ns: u64,
    /// The filesystem's device number; `0` off Unix.
    pub dev: u64,
}

impl SnapshotClock {
    /// Read from the mtime and device of a file created (and removed) in
    /// `dir`.
    ///
    /// Taken from the filesystem rather than the system clock, so an NFS
    /// server's skew can't make a record on that filesystem look older than
    /// it is. Writes to `dir`; callers go through [`build_to_persist`].
    pub(crate) fn now(dir: &Path) -> Self {
        tempfile::Builder::new()
            .prefix(".toolr-clock.")
            .suffix(".tmp")
            .tempfile_in(dir)
            .and_then(|f| f.as_file().metadata())
            .map_or(Self { at_ns: 0, dev: 0 }, |m| Self {
                at_ns: nanos(m.modified()),
                dev: device(&m),
            })
    }

    /// Bound `records` by this clock.
    pub(crate) fn stamp(self, records: &mut InputRecords) {
        records.snapshot_at_ns = self.at_ns;
        records.snapshot_dev = self.dev;
    }
}

/// Run `build` for a manifest about to be written to `tools_dir`: read the
/// [`SnapshotClock`] first, so it predates every stat the build takes, then
/// stamp it on the built records. Every path that writes the manifest goes
/// through here; paths that don't write must not, since the clock writes
/// to `tools_dir`.
pub fn build_to_persist<E>(
    tools_dir: &Path,
    build: impl FnOnce() -> std::result::Result<BuiltManifest, E>,
) -> std::result::Result<BuiltManifest, E> {
    let clock = SnapshotClock::now(tools_dir);
    let mut built = build()?;
    clock.stamp(&mut built.manifest.input_records);
    Ok(built)
}

/// [`snapshot_static_inputs`] with `tools/**/*.py` already listed, so the
/// build walks `tools/` once for both the parser and the hash.
pub(crate) fn snapshot_from(
    tools_dir: &Path,
    py_files: &[PathBuf],
    previous: Option<&InputRecords>,
) -> Result<InputRecords> {
    let project_root = project_root(tools_dir);
    let reuse = Reuse {
        files: previous.map_or(&[], |p| p.files.as_slice()),
        snapshot_at_ns: previous.map_or(0, |p| p.snapshot_at_ns),
        snapshot_dev: previous.map_or(0, |p| p.snapshot_dev),
        project_root,
    };
    let pyproject = crate::argparse::pyproject_path(project_root);
    let mut pyproject_record = None;
    let mut scan_paths = Vec::new();
    if pyproject.is_file() {
        let input = StaticInput {
            key: input_key(project_root, &pyproject),
            path: pyproject,
            kind: InputKind::Pyproject,
        };
        let mut parsed = None;
        pyproject_record = record(&input, &reuse, |bytes| {
            parsed = Some(parse_scan_paths(bytes))
        })?;
        scan_paths = match (parsed, &pyproject_record) {
            (Some(parsed), _) => parsed,
            // Reused rather than read: unchanged, and so are its scan paths.
            (
                None,
                Some(InputRecord {
                    content: Content::Digest(_),
                    ..
                }),
            ) => previous.map(|p| p.scan_paths.clone()).unwrap_or_default(),
            (None, _) => Vec::new(),
        };
    }
    let mut files = Vec::new();
    for input in inputs_for(project_root, py_files, &scan_paths) {
        files.extend(record(&input, &reuse, |_| {})?);
    }
    // A pyproject a `scan_paths` glob also matches is already recorded
    // whole, as a scanned file: the scanner reads all of it.
    if let Some(pyproject_record) = pyproject_record
        && let Err(at) = files.binary_search_by(|f| f.key.cmp(&pyproject_record.key))
    {
        files.insert(at, pyproject_record);
    }
    Ok(InputRecords {
        files,
        scan_paths,
        snapshot_at_ns: 0,
        snapshot_dev: 0,
    })
}

/// `static_hash` from the records of [`snapshot_static_inputs`], in their
/// order: each is fed as `len(key) as u64 LE || key || 0x00 || hex digest`,
/// or `len(key) as u64 LE || key || 0x01 || <error kind>` when unreadable.
pub fn static_hash_of(files: &[InputRecord]) -> String {
    let mut hasher = Hasher::new();
    for file in files {
        hasher.update(&(file.key.len() as u64).to_le_bytes());
        hasher.update(file.key.as_bytes());
        match &file.content {
            Content::Digest(digest) => hasher.update(TAG_CONTENTS).update(digest.as_bytes()),
            Content::Unreadable(kind) => hasher.update(TAG_READ_ERROR).update(kind.as_bytes()),
        };
    }
    hasher.finalize().to_hex().to_string()
}

/// Same root as `argparse::run_for_project`.
fn project_root(tools_dir: &Path) -> &Path {
    tools_dir.parent().unwrap_or(tools_dir)
}

/// Each argparse block's `scan_paths`; nothing when the pyproject doesn't
/// parse.
fn parse_scan_paths(bytes: &[u8]) -> Vec<Vec<String>> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| crate::argparse::parse_blocks(text).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|block| block.scan_paths)
        .collect()
}

/// The modules and scanned files, sorted by key without duplicates; a
/// module also matched by a `scan_paths` glob keeps its strict reads. The
/// pyproject is recorded separately, by [`snapshot_from`].
fn inputs_for(
    project_root: &Path,
    py_files: &[PathBuf],
    scan_paths: &[Vec<String>],
) -> Vec<StaticInput> {
    let mut paths: Vec<(PathBuf, InputKind)> = py_files
        .iter()
        .map(|p| (p.clone(), InputKind::Module))
        .collect();
    for block in scan_paths {
        if let Ok(scanned) = crate::argparse::scan::expand_scan_paths(project_root, block) {
            paths.extend(scanned.into_iter().map(|p| (p, InputKind::Scanned)));
        }
    }
    let mut inputs: Vec<StaticInput> = paths
        .into_iter()
        .map(|(path, kind)| StaticInput {
            key: input_key(project_root, &path),
            path,
            kind,
        })
        .collect();
    let not_module = |i: &StaticInput| i.kind != InputKind::Module;
    inputs.sort_by(|a, b| a.key.cmp(&b.key).then(not_module(a).cmp(&not_module(b))));
    inputs.dedup_by(|a, b| a.key == b.key);
    inputs
}

/// The previous records [`record`] may reuse.
struct Reuse<'a> {
    files: &'a [InputRecord],
    /// The previous snapshot's bound in ns; `0` reuses nothing.
    snapshot_at_ns: u64,
    /// The device the bound was read on.
    snapshot_dev: u64,
    project_root: &'a Path,
}

impl Reuse<'_> {
    fn get(&self, input: &StaticInput, live: &Stat) -> Option<&InputRecord> {
        // The bound came from `tools/`'s clock; an mtime from another
        // filesystem can't be compared with it.
        let same_clock = live.dev == self.snapshot_dev && input.path.starts_with(self.project_root);
        if !same_clock {
            return None;
        }
        let i = self
            .files
            .binary_search_by(|f| f.key.as_str().cmp(&input.key))
            .ok()?;
        let old = &self.files[i];
        let same = old.size == live.size
            && old.mtime_ns == live.mtime_ns
            && old.ctime_ns == live.ctime_ns
            && old.ino == live.ino
            && old.dev == live.dev;
        (same
            && matches!(old.content, Content::Digest(_))
            && old.mtime_ns != 0
            && old.mtime_ns < self.snapshot_at_ns)
            .then_some(old)
    }
}

/// `input`'s record: reused from `reuse` when its stat allows, else read and
/// hashed, calling `on_read` with the bytes. `None` when the file is gone.
fn record(
    input: &StaticInput,
    reuse: &Reuse,
    on_read: impl FnOnce(&[u8]),
) -> Result<Option<InputRecord>> {
    let failed = |e: std::io::Error| match e.kind() {
        // Deleted since it was listed: hash the tree as it now is.
        std::io::ErrorKind::NotFound => Ok(None),
        _ if input.kind == InputKind::Module => {
            Err(anyhow::Error::new(e).context(format!("reading {}", shown(&input.path))))
        }
        kind => Ok(Some(
            Stat::default().record(&input.key, Content::Unreadable(kind.to_string())),
        )),
    };
    let meta = match std::fs::metadata(&input.path) {
        Ok(meta) => meta,
        Err(e) => return failed(e),
    };
    let live = Stat::of(&meta);
    if let Some(old) = reuse.get(input, &live) {
        return Ok(Some(old.clone()));
    }
    let bytes = match read_input(&input.path) {
        Ok(bytes) => bytes,
        Err(e) => return failed(e),
    };
    on_read(&bytes);
    Ok(Some(live.record(
        &input.key,
        Content::Digest(digest(input, &bytes)),
    )))
}

/// A record's [`Content::Digest`]: blake3 of the contents, or for the pyproject, of
/// [`TAG_ARGPARSE_BLOCKS`] followed by its argparse blocks, so edits outside
/// `[tool.toolr.argparse.*]` (a dependency bump, `uv add`) leave it alone;
/// see [`canonical_argparse_blocks`]. A pyproject the build can't read its blocks
/// from keeps the raw-bytes digest, so fixing it still changes the hash.
fn digest(input: &StaticInput, bytes: &[u8]) -> String {
    let mut hasher = Hasher::new();
    let blocks = match input.kind {
        InputKind::Pyproject => canonical_argparse_blocks(bytes),
        InputKind::Module | InputKind::Scanned => None,
    };
    match blocks {
        Some(blocks) => hasher.update(TAG_ARGPARSE_BLOCKS).update(&blocks),
        None => hasher.update(bytes),
    };
    hasher.finalize().to_hex().to_string()
}

/// The stat fields of an [`InputRecord`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Stat {
    size: u64,
    mtime_ns: u64,
    ctime_ns: u64,
    ino: u64,
    dev: u64,
}

impl Stat {
    fn of(meta: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        let (ctime_ns, ino) = {
            use std::os::unix::fs::MetadataExt;
            let ctime = u64::try_from(meta.ctime()).map_or(0, |secs| {
                secs.saturating_mul(1_000_000_000)
                    .saturating_add(u64::try_from(meta.ctime_nsec()).unwrap_or(0))
            });
            (ctime, meta.ino())
        };
        #[cfg(not(unix))]
        let (ctime_ns, ino) = (0, 0);
        Self {
            size: meta.len(),
            mtime_ns: nanos(meta.modified()),
            ctime_ns,
            ino,
            dev: device(meta),
        }
    }

    fn record(self, key: &str, content: Content) -> InputRecord {
        InputRecord {
            key: key.to_owned(),
            size: self.size,
            mtime_ns: self.mtime_ns,
            ctime_ns: self.ctime_ns,
            ino: self.ino,
            dev: self.dev,
            content,
        }
    }
}

/// The device `meta`'s file lives on; `0` off Unix.
fn device(meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        std::os::unix::fs::MetadataExt::dev(meta)
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}

/// Nanoseconds since the Unix epoch; `0` when unknown or before 1970.
fn nanos(time: std::io::Result<SystemTime>) -> u64 {
    time.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

fn input_key(project_root: &Path, path: &Path) -> String {
    match path.strip_prefix(project_root) {
        Ok(rel) => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// Every `*.py` under `tools_dir`, sorted. Shared by the hasher and the
/// static parser so both agree on what makes up `tools/`.
///
/// Prunes `__pycache__` and dot-prefixed directories during the walk. The
/// in-tree venv always resolves to `tools/.venv`, so this keeps
/// site-packages out of both the hash and the manifest (#544); a module
/// under `.venv` would otherwise become `tools..venv.lib.<pkg>`. The root
/// itself is never pruned, even if its basename starts with a dot.
///
/// A `*.py` symlink fails the walk (F-17); symlinked directories are not
/// followed. The symlink check reads the directory entry, so it costs no
/// extra syscall.
///
/// `toolr.testing._discovery._reject_symlinked_modules` (the symlink check)
/// and `_import_package_modules` (the imports) mirror this walk in Python;
/// change all three together.
pub(crate) fn list_python_files(tools_dir: &Path) -> Result<Vec<PathBuf>, SymlinkedModules> {
    let mut paths = Vec::new();
    let mut links = Vec::new();
    let walk = WalkDir::new(tools_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0 || !e.file_type().is_dir() || !is_pruned_dir(e.file_name())
        })
        .filter_map(|e| e.ok());
    for e in walk {
        if e.path().extension().is_none_or(|x| x != "py") {
            continue;
        }
        if e.path_is_symlink() {
            // A `.py`-named link to a directory is a directory link: not
            // followed, not rejected. Only links pay for this stat.
            if !std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir()) {
                links.push(e.into_path());
            }
        } else if e.file_type().is_file() {
            paths.push(e.into_path());
        }
    }
    if !links.is_empty() {
        return Err(SymlinkedModules::new(tools_dir, links));
    }
    paths.sort();
    Ok(paths)
}

/// `*.py` symlinks found by [`list_python_files`]. toolr never reads or
/// imports code from wherever a link points, so these are rejected.
///
/// The message already names every offending path and must reach the user
/// unwrapped, and dispatch must not fall back to a cached manifest for it:
/// check [`is_symlinked_modules`] before adding context or recovering from
/// a build error ([`context_unless_symlinked`] does the former).
#[derive(Debug, thiserror::Error)]
#[error(
    "symlinked modules are not supported ({count}):\n{details}\nPut the file itself under {root}/, or share it as a Python package.",
    count = .paths.len(),
    details = .paths.iter().map(|p| format!("  - {p}")).collect::<Vec<_>>().join("\n"),
)]
pub struct SymlinkedModules {
    /// The walked directory's name, e.g. `tools` or a plugin package.
    root: String,
    /// Each link, relative to the walked directory's parent, sorted.
    paths: Vec<String>,
}

impl SymlinkedModules {
    pub(crate) fn new(tools_dir: &Path, mut links: Vec<PathBuf>) -> Self {
        links.sort();
        let root = root_name(tools_dir);
        let paths = links
            .iter()
            .map(|p| {
                let rel = p.strip_prefix(tools_dir).unwrap_or(p);
                let parts: Vec<_> = std::iter::once(root.clone())
                    .chain(
                        rel.components()
                            .map(|c| c.as_os_str().to_string_lossy().into()),
                    )
                    .collect();
                escape_control_chars(&parts.join("/"))
            })
            .collect();
        Self {
            root: escape_control_chars(&root),
            paths,
        }
    }
}

/// The walked directory's own name, even when it was given as `.` or `..`
/// (`toolr self build-manifest --source-dir .`).
fn root_name(dir: &Path) -> String {
    let named = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned());
    named(dir)
        .or_else(|| std::path::absolute(dir).ok().as_deref().and_then(named))
        // `absolute` keeps a trailing `..`, which has no name; resolve it.
        .or_else(|| dir.canonicalize().ok().as_deref().and_then(named))
        .unwrap_or_else(|| dir.display().to_string())
}

/// Whether `err` is a [`SymlinkedModules`] rejection.
pub fn is_symlinked_modules(err: &anyhow::Error) -> bool {
    err.is::<SymlinkedModules>()
}

/// Add `context` to `err`, unless it is a [`SymlinkedModules`] rejection,
/// which must read the same on every path.
pub fn context_unless_symlinked<C>(err: anyhow::Error, context: C) -> anyhow::Error
where
    C: std::fmt::Display + Send + Sync + 'static,
{
    if is_symlinked_modules(&err) {
        err
    } else {
        err.context(context)
    }
}

fn is_pruned_dir(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|n| n.starts_with('.') || n == "__pycache__")
}

/// The pyproject's `[tool.toolr.argparse.*]` blocks as the build parses
/// them, serialised in name order: the only part of the file the build
/// reads. `None` when they don't parse, so the caller hashes the raw bytes.
fn canonical_argparse_blocks(bytes: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut blocks = crate::argparse::parse_blocks(text).ok()?;
    blocks.sort_by(|a, b| a.name.cmp(&b.name));
    serde_json::to_vec(&blocks).ok()
}

fn read_input(path: &Path) -> std::io::Result<Vec<u8>> {
    #[cfg(test)]
    tests::count_read(path);
    // `path` is listed by `inputs_for`/`snapshot_from` from the project's own config, not untrusted input.
    std::fs::read(path) // nosemgrep: rust.actix.path-traversal.tainted-path.tainted-path
}

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        static READS: std::cell::RefCell<std::collections::HashMap<PathBuf, usize>> =
            Default::default();
    }

    pub(super) fn count_read(path: &Path) {
        READS.with(|r| *r.borrow_mut().entry(path.to_path_buf()).or_default() += 1);
    }

    /// Reads by this thread's static-hash code since the last call.
    pub(crate) fn take_reads() -> std::collections::HashMap<PathBuf, usize> {
        READS.with(|r| std::mem::take(&mut *r.borrow_mut()))
    }
    use tempfile::TempDir;

    fn setup(files: &[(&str, &str)]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        for (name, contents) in files {
            let path = tmp.path().join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, contents).unwrap();
        }
        tmp
    }

    /// `setup` with every file under `tools/`.
    fn tools_tree(files: &[(&str, &str)]) -> TempDir {
        let files: Vec<_> = files
            .iter()
            .map(|(name, contents)| (format!("tools/{name}"), *contents))
            .collect();
        let refs: Vec<_> = files.iter().map(|(n, c)| (n.as_str(), *c)).collect();
        setup(&refs)
    }

    /// Hash `inputs` as listed, reusing nothing.
    fn hash_paths(inputs: &[StaticInput]) -> Result<String> {
        let reuse = Reuse {
            files: &[],
            snapshot_at_ns: 0,
            snapshot_dev: 0,
            project_root: Path::new("/"),
        };
        let mut files = Vec::new();
        for input in inputs {
            files.extend(record(input, &reuse, |_| {})?);
        }
        Ok(static_hash_of(&files))
    }

    fn tools_hash(project: &TempDir) -> String {
        compute_static_hash(&project.path().join("tools")).unwrap()
    }

    #[test]
    fn identical_trees_hash_identically() {
        let a = tools_tree(&[("a.py", "x"), ("b/c.py", "y")]);
        let b = tools_tree(&[("a.py", "x"), ("b/c.py", "y")]);
        assert_eq!(tools_hash(&a), tools_hash(&b));
    }

    #[test]
    fn different_content_hashes_differently() {
        let a = tools_tree(&[("a.py", "x")]);
        let b = tools_tree(&[("a.py", "y")]);
        assert_ne!(tools_hash(&a), tools_hash(&b));
    }

    /// #544: with `venv-location = "in-tree"` the venv lives at
    /// `tools/.venv`; its site-packages must not feed the static hash.
    #[test]
    fn ignores_py_files_under_in_tree_venv() {
        let a = tools_tree(&[
            ("a.py", "x"),
            (".venv/lib/python3.13/site-packages/pkg/__init__.py", "y"),
            (".venv/lib/python3.13/site-packages/pkg/mod.py", "z"),
        ]);
        let b = tools_tree(&[("a.py", "x")]);
        assert_eq!(tools_hash(&a), tools_hash(&b));
    }

    #[test]
    fn ignores_py_files_under_pycache_and_dot_directories() {
        let a = tools_tree(&[
            ("a.py", "x"),
            ("__pycache__/a.py", "cached"),
            ("sub/__pycache__/b.py", "cached"),
            (".git/hooks/pre-commit.py", "hook"),
            (".tox/py313/lib/x.py", "tox"),
        ]);
        let b = tools_tree(&[("a.py", "x")]);
        assert_eq!(tools_hash(&a), tools_hash(&b));
    }

    /// A file listed by the walk but gone by the time it is read (e.g. a
    /// user module deleted mid-dispatch) is hashed as absent instead of
    /// failing the whole freshness check.
    #[test]
    fn file_vanishing_between_listing_and_read_is_skipped() {
        let tmp = tools_tree(&[("a.py", "x")]);
        let tools = tmp.path().join("tools");
        let input = |name: &str| StaticInput {
            key: format!("tools/{name}"),
            path: tools.join(name),
            kind: InputKind::Module,
        };
        let with_gone = hash_paths(&[input("a.py"), input("gone.py")]).unwrap();
        let without = hash_paths(&[input("a.py")]).unwrap();
        assert_eq!(with_gone, without);
        assert_eq!(with_gone, tools_hash(&tmp));
    }

    /// Only `NotFound` is tolerated; any other read error still fails.
    #[test]
    fn unreadable_listed_path_fails_the_hash() {
        let tmp = setup(&[("a.py", "x")]);
        let dir = tmp.path().join("pkg.py");
        std::fs::create_dir(&dir).unwrap();
        let inputs = [
            StaticInput {
                key: "a.py".into(),
                path: tmp.path().join("a.py"),
                kind: InputKind::Module,
            },
            StaticInput {
                key: "pkg.py".into(),
                path: dir.clone(),
                kind: InputKind::Module,
            },
        ];
        let err = hash_paths(&inputs).unwrap_err();
        assert!(
            format!("{err:#}").starts_with(&format!("reading {}: ", dir.display())),
            "{err:#}"
        );
    }

    /// #544 mechanism: `uv sync` recreating `tools/.venv` while toolr
    /// hashes `tools/` used to fail with `No such file or directory`.
    /// Churn the venv from another thread while hashing repeatedly.
    #[test]
    fn hashing_survives_in_tree_venv_being_recreated_concurrently() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let tmp = tools_tree(&[("a.py", "x"), ("ci/b.py", "y")]);
        let tools = tmp.path().join("tools");
        let expected = compute_static_hash(&tools).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let churn = {
            let stop = Arc::clone(&stop);
            let venv = tools.join(".venv");
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let sp = venv.join("lib/python3.13/site-packages/pkg");
                    // Errors ignored: Windows refuses writes into a directory
                    // that is still pending deletion.
                    let _ = std::fs::create_dir_all(&sp);
                    for i in 0..200 {
                        let _ = std::fs::write(sp.join(format!("m{i}.py")), "x = 1\n");
                    }
                    let _ = std::fs::remove_dir_all(&venv);
                }
            })
        };

        let results: Vec<_> = (0..200).map(|_| compute_static_hash(&tools)).collect();
        stop.store(true, Ordering::Relaxed);
        churn.join().unwrap();

        let errors: Vec<String> = results
            .iter()
            .filter_map(|r| r.as_ref().err().map(|e| format!("{e:#}")))
            .collect();
        assert!(errors.is_empty(), "hashing failed mid-churn: {errors:?}");
        for r in results {
            assert_eq!(r.unwrap(), expected);
        }
    }

    fn file_link(target: &Path, link: &Path) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target, link).unwrap();
    }

    /// F-17 repro tree: `tools/linked.py -> ../shared/linked.py` next to a
    /// regular `tools/demo.py`, plus a nested and a dangling link. Every link
    /// is reported at once, sorted, relative to the project root.
    #[test]
    fn symlinked_modules_are_rejected_all_at_once() {
        let tmp = setup(&[
            ("tools/demo.py", "x"),
            ("shared/linked.py", "y"),
            ("shared/other.py", "z"),
        ]);
        let tools = tmp.path().join("tools");
        // Windows runners cover this when they may create file symlinks.
        #[cfg(windows)]
        if std::os::windows::fs::symlink_file("demo.py", tools.join("probe")).is_err() {
            eprintln!("skipping: no permission to create file symlinks");
            return;
        }
        let up = Path::new("..");
        file_link(
            &up.join("shared").join("linked.py"),
            &tools.join("linked.py"),
        );
        std::fs::create_dir(tools.join("sub")).unwrap();
        let shared_other = up.join("..").join("shared").join("other.py");
        file_link(&shared_other, &tools.join("sub").join("other.py"));
        file_link(
            &up.join("shared").join("missing.py"),
            &tools.join("gone.py"),
        );

        let err = list_python_files(&tools).unwrap_err();
        assert_eq!(
            err.to_string(),
            "symlinked modules are not supported (3):\n  - tools/gone.py\n  - tools/linked.py\n  - tools/sub/other.py\nPut the file itself under tools/, or share it as a Python package."
        );
        let err = compute_static_hash(&tools).unwrap_err();
        assert!(is_symlinked_modules(&err));
        assert!(
            err.to_string()
                .starts_with("symlinked modules are not supported (3):"),
            "{err}"
        );
    }

    /// Links whose target can't be stat'd (a loop, a target inside an
    /// unreadable directory) are reported, not skipped or fatal to the walk.
    #[cfg(unix)]
    #[test]
    fn unresolvable_symlinked_modules_are_rejected() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let tmp = setup(&[("tools/demo.py", "x"), ("locked/x.py", "y")]);
        let tools = tmp.path().join("tools");
        let locked = tmp.path().join("locked");
        std::fs::create_dir(locked.join("d")).unwrap();
        symlink("loop.py", tools.join("loop.py")).unwrap();
        symlink("../locked/x.py", tools.join("inlocked.py")).unwrap();
        symlink("../locked/d", tools.join("dirlink.py")).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o0)).unwrap();
        let root_can_read = std::fs::read_dir(&locked).is_ok();
        let result = list_python_files(&tools);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

        let err = result.unwrap_err();
        // Root stats through mode 000, so there `dirlink.py` is a directory link.
        let expected: Vec<_> = ["tools/dirlink.py", "tools/inlocked.py", "tools/loop.py"]
            .into_iter()
            .filter(|p| !(root_can_read && *p == "tools/dirlink.py"))
            .collect();
        assert_eq!(err.paths, expected);
    }

    /// A walk rooted at `.` names the directory, not `.`. The expected name
    /// comes from the test process's cwd, whatever that is.
    #[test]
    fn symlinked_modules_name_a_relative_root() {
        let links = vec![PathBuf::from("./linked.py"), PathBuf::from("./sub/l.py")];
        let err = SymlinkedModules::new(Path::new("."), links);
        let cwd = std::env::current_dir().unwrap();
        let name = cwd.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            err.paths,
            [format!("{name}/linked.py"), format!("{name}/sub/l.py")]
        );
        assert_eq!(err.root, name);
    }

    /// `..` is resolved to the directory's name; a root with no name at all
    /// falls back to its display form.
    #[cfg(unix)]
    #[test]
    fn root_name_resolves_parent_and_nameless_paths() {
        let tmp = setup(&[("my_plugin/sub/x.py", "x")]);
        let sub = tmp.path().join("my_plugin/sub");
        assert_eq!(root_name(&sub.join("..")), "my_plugin");
        assert_eq!(root_name(Path::new("/")), "/");
    }

    /// Context is added to other errors but never to a rejection, which
    /// must read the same on every path.
    #[test]
    fn context_is_never_added_to_symlinked_modules() {
        let links = SymlinkedModules::new(Path::new("tools"), vec!["tools/x.py".into()]);
        let expected = links.to_string();
        let err = context_unless_symlinked(links.into(), "hashing tools");
        assert!(is_symlinked_modules(&err));
        assert_eq!(format!("{err:#}"), expected);

        let err = context_unless_symlinked(anyhow::anyhow!("disk on fire"), "hashing tools");
        assert!(!is_symlinked_modules(&err));
        assert_eq!(format!("{err:#}"), "hashing tools: disk on fire");
    }

    /// Directory links are still not followed, and links under pruned
    /// directories are never looked at.
    #[cfg(unix)]
    #[test]
    fn symlinked_directories_and_pruned_links_are_not_rejected() {
        use std::os::unix::fs::symlink;
        let tmp = setup(&[
            ("tools/demo.py", "x"),
            ("venv/lib/site-packages/pkg/mod.py", "y"),
            ("shared/lib/mod.py", "z"),
            ("shared/linked.py", "w"),
        ]);
        let tools = tmp.path().join("tools");
        symlink("../venv", tools.join(".venv")).unwrap();
        symlink("../shared/lib", tools.join("lib")).unwrap();
        symlink("../shared/lib", tools.join("pkg.py")).unwrap();
        symlink("../shared/linked.py", tools.join("notes.txt")).unwrap();
        for pruned in ["__pycache__", ".cache"] {
            std::fs::create_dir(tools.join(pruned)).unwrap();
            symlink("../../shared/linked.py", tools.join(pruned).join("x.py")).unwrap();
        }

        assert_eq!(
            list_python_files(&tools).unwrap(),
            vec![tools.join("demo.py")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_module_names_are_escaped() {
        let tmp = setup(&[("tools/demo.py", "x")]);
        let tools = tmp.path().join("tools");
        std::os::unix::fs::symlink("demo.py", tools.join("a\x1b]0;x\x07.py")).unwrap();
        let err = list_python_files(&tools).unwrap_err().to_string();
        assert!(err.contains("  - tools/a\\u{1b}]0;x\\u{7}.py\n"), "{err}");
    }

    const DJANGO_BLOCK: &str = r#"
[tool.toolr.argparse.django]
scan_paths = ["apps/*/management/commands/*.py"]
django = true

[[tool.toolr.argparse.django.attach]]
parent = "django"
"#;

    const MIGRATE_PY: &str = "def add_arguments(self, parser):\n    parser.add_argument('--check', action='store_true')\n";

    /// #655 layout: a dispatcher in `tools/` and a management command its
    /// argparse block scans from `apps/`.
    fn django_project() -> TempDir {
        let pyproject = format!("[project]\nname = \"tools\"\n{DJANGO_BLOCK}");
        setup(&[
            ("tools/pyproject.toml", &pyproject),
            ("tools/dispatcher.py", "x"),
            ("apps/billing/management/commands/migrate.py", MIGRATE_PY),
        ])
    }

    fn keys(project: &TempDir) -> Vec<String> {
        snapshot_static_inputs(&project.path().join("tools"), None)
            .unwrap()
            .files
            .into_iter()
            .map(|f| f.key)
            .collect()
    }

    fn write(project: &TempDir, rel: &str, contents: &str) {
        std::fs::write(project.path().join(rel), contents).unwrap();
    }

    #[test]
    fn static_inputs_cover_tools_pyproject_and_scanned_files() {
        let project = django_project();
        assert_eq!(
            keys(&project),
            [
                "apps/billing/management/commands/migrate.py",
                "tools/dispatcher.py",
                "tools/pyproject.toml",
            ]
        );
    }

    #[test]
    fn static_hash_changes_when_scanned_file_changes() {
        let project = django_project();
        let before = tools_hash(&project);
        write(
            &project,
            "apps/billing/management/commands/migrate.py",
            &format!("{MIGRATE_PY}    parser.add_argument('--database')\n"),
        );
        assert_ne!(tools_hash(&project), before);
    }

    #[test]
    fn static_hash_changes_when_pyproject_changes() {
        let project = django_project();
        let before = tools_hash(&project);
        let pyproject = std::fs::read_to_string(project.path().join("tools/pyproject.toml"))
            .unwrap()
            .replace("apps/*/management/commands/*.py", "nowhere/*.py");
        write(&project, "tools/pyproject.toml", &pyproject);
        assert_ne!(tools_hash(&project), before);
        assert_eq!(
            keys(&project),
            ["tools/dispatcher.py", "tools/pyproject.toml"]
        );
    }

    #[test]
    fn static_hash_ignores_unmatched_files_outside_tools() {
        let project = django_project();
        let before = tools_hash(&project);
        write(&project, "apps/billing/notes.txt", "x");
        write(&project, "apps/billing/models.py", "x");
        assert_eq!(tools_hash(&project), before);
    }

    /// A typo'd glob must not fail `--help` and TAB through the freshness
    /// check; the build reports it. Other blocks still count.
    #[test]
    fn static_hash_tolerates_bad_glob() {
        let project = django_project();
        let pyproject = format!(
            "{}\n[tool.toolr.argparse.broken]\nscan_paths = [\"apps/[/x.py\"]\n",
            std::fs::read_to_string(project.path().join("tools/pyproject.toml")).unwrap()
        );
        write(&project, "tools/pyproject.toml", &pyproject);
        assert_eq!(
            keys(&project),
            [
                "apps/billing/management/commands/migrate.py",
                "tools/dispatcher.py",
                "tools/pyproject.toml",
            ]
        );
    }

    /// Invalid TOML hashes its bytes, so fixing it rebuilds, but scans
    /// nothing.
    #[test]
    fn static_hash_tolerates_invalid_pyproject() {
        let project = django_project();
        write(&project, "tools/pyproject.toml", "not = = toml");
        let before = tools_hash(&project);
        assert_eq!(
            keys(&project),
            ["tools/dispatcher.py", "tools/pyproject.toml"]
        );
        write(&project, "tools/pyproject.toml", "not = = toml at all");
        assert_ne!(tools_hash(&project), before);
    }

    /// Only the argparse blocks of the pyproject count: a dependency bump
    /// or `uv add` must not make the manifest stale.
    #[test]
    fn static_hash_ignores_pyproject_edits_outside_argparse() {
        let project = django_project();
        let before = tools_hash(&project);
        let pyproject = std::fs::read_to_string(project.path().join("tools/pyproject.toml"))
            .unwrap()
            .replace(
                "name = \"tools\"\n",
                "name = \"tools\"\ndependencies = [\"django\"]\n",
            );
        write(&project, "tools/pyproject.toml", &pyproject);
        assert_eq!(tools_hash(&project), before);

        write(
            &project,
            "tools/pyproject.toml",
            &pyproject.replace("django = true", "django = false"),
        );
        assert_ne!(tools_hash(&project), before);
    }

    /// The #655 review shape: a dependency line mid-edit makes the whole
    /// file invalid TOML, which counts as drift until it is fixed.
    #[test]
    fn invalid_pyproject_outside_argparse_changes_the_hash() {
        let project = django_project();
        let path = project.path().join("tools/pyproject.toml");
        let valid = std::fs::read_to_string(&path).unwrap();
        let before = tools_hash(&project);
        write(
            &project,
            "tools/pyproject.toml",
            &valid.replace(
                "name = \"tools\"\n",
                "name = \"tools\"\ndependencies = [\"django\"\n",
            ),
        );
        assert_ne!(tools_hash(&project), before);
        write(&project, "tools/pyproject.toml", &valid);
        assert_eq!(tools_hash(&project), before);
    }

    /// The blocks are hashed in name order, not file order.
    #[test]
    fn static_hash_ignores_argparse_block_order() {
        let other = "[tool.toolr.argparse.other]\nscan_paths = [\"nowhere/*.py\"]\n";
        let project = setup(&[(
            "tools/pyproject.toml",
            &format!("[project]\nname = \"tools\"\n{DJANGO_BLOCK}\n{other}"),
        )]);
        let before = tools_hash(&project);
        write(
            &project,
            "tools/pyproject.toml",
            &format!("[project]\nname = \"tools\"\n{other}\n{DJANGO_BLOCK}"),
        );
        assert_eq!(tools_hash(&project), before);
    }

    /// A pyproject a `scan_paths` glob also matches is scanned as a file,
    /// so all of its bytes count.
    #[test]
    fn scanned_pyproject_is_hashed_whole() {
        let pyproject = "[tool.toolr.argparse.all]\nscan_paths = [\"tools/*\"]\n";
        let project = setup(&[("tools/pyproject.toml", pyproject)]);
        let inputs = inputs_for(project.path(), &[], &[vec!["tools/*".into()]]);
        assert_eq!(inputs.len(), 1, "{inputs:?}");
        assert_eq!(inputs[0].kind, InputKind::Scanned, "{inputs:?}");
        assert_eq!(keys(&project), ["tools/pyproject.toml"]);
        let before = tools_hash(&project);
        write(
            &project,
            "tools/pyproject.toml",
            &format!("[project]\nname = \"x\"\n{pyproject}"),
        );
        assert_ne!(tools_hash(&project), before);
    }

    #[test]
    fn scanned_file_under_tools_is_hashed_once() {
        let project = setup(&[
            (
                "tools/pyproject.toml",
                "[tool.toolr.argparse.local]\nscan_paths = [\"tools/*.py\"]\n",
            ),
            ("tools/a.py", "x"),
        ]);
        assert_eq!(keys(&project), ["tools/a.py", "tools/pyproject.toml"]);
        let a = project.path().join("tools/a.py");
        let inputs = inputs_for(project.path(), &[a], &[vec!["tools/*.py".into()]]);
        assert_eq!(inputs.len(), 1, "{inputs:?}");
        assert_eq!(
            inputs[0].kind,
            InputKind::Module,
            "the module's strict reads win: {inputs:?}"
        );
    }

    #[test]
    fn static_hash_without_pyproject() {
        let project = tools_tree(&[("a.py", "x")]);
        assert_eq!(keys(&project), ["tools/a.py"]);
        let before = tools_hash(&project);
        write(&project, "tools/a.py", "y");
        assert_ne!(tools_hash(&project), before);
    }

    /// An absolute `scan_paths` entry outside the project keeps its full
    /// path as the key.
    #[cfg(unix)]
    #[test]
    fn scanned_file_outside_the_project_is_keyed_by_full_path() {
        let elsewhere = setup(&[("cmd.py", MIGRATE_PY)]);
        let cmd = elsewhere.path().join("cmd.py");
        let project = setup(&[(
            "tools/pyproject.toml",
            &format!(
                "[tool.toolr.argparse.ext]\nscan_paths = ['{}']\n",
                cmd.display()
            ),
        )]);
        assert_eq!(
            keys(&project),
            [cmd.display().to_string(), "tools/pyproject.toml".into()]
        );
    }

    /// Run `f` while `path` is mode 000. `None` when this process can read
    /// it anyway (root), where the case can't be exercised.
    #[cfg(unix)]
    fn with_unreadable<R>(path: &Path, f: impl FnOnce() -> R) -> Option<R> {
        use std::os::unix::fs::PermissionsExt;
        let mode = |m| std::fs::set_permissions(path, std::fs::Permissions::from_mode(m)).unwrap();
        mode(0o000);
        let out = std::fs::read(path).is_err().then(f);
        mode(0o644);
        out
    }

    /// The scanner turns an unreadable scanned file into a placeholder, so
    /// the hash must not fail on it either; fixing it still rebuilds.
    #[cfg(unix)]
    #[test]
    fn unreadable_scanned_file_is_hashed_not_fatal() {
        let project = django_project();
        let migrate = project
            .path()
            .join("apps/billing/management/commands/migrate.py");
        let readable = tools_hash(&project);
        let tools = project.path().join("tools");
        if let Some(locked) = with_unreadable(&migrate, || compute_static_hash(&tools)) {
            assert_ne!(locked.unwrap(), readable);
            assert_eq!(tools_hash(&project), readable);
        }
    }

    /// The same holds for `tools/pyproject.toml`: the build reports it.
    #[cfg(unix)]
    #[test]
    fn unreadable_pyproject_is_hashed_not_fatal() {
        let project = django_project();
        let tools = project.path().join("tools");
        let locked = with_unreadable(&tools.join("pyproject.toml"), || {
            snapshot_static_inputs(&tools, None)
        });
        if let Some(records) = locked {
            let records = records.unwrap();
            let keys: Vec<_> = records.files.iter().map(|f| f.key.as_str()).collect();
            assert_eq!(keys, ["tools/dispatcher.py", "tools/pyproject.toml"]);
            assert!(
                matches!(records.files[1].content, Content::Unreadable(_)),
                "{records:?}"
            );
        }
    }

    /// An unreadable module still fails, naming the file.
    #[cfg(unix)]
    #[test]
    fn unreadable_module_fails_naming_the_file() {
        let project = django_project();
        let tools = project.path().join("tools");
        let dispatcher = tools.join("dispatcher.py");
        if let Some(result) = with_unreadable(&dispatcher, || compute_static_hash(&tools)) {
            let err = format!("{:#}", result.unwrap_err());
            let prefix = format!("reading {}: ", dispatcher.display());
            assert!(err.starts_with(&prefix), "{err}");
        }
    }

    /// A file name is repo content: the error must not carry its control characters.
    #[cfg(unix)]
    #[test]
    fn unreadable_module_error_escapes_control_characters() {
        let project = django_project();
        let tools = project.path().join("tools");
        let hostile = tools.join("a\x1b[31mb.py");
        std::fs::write(&hostile, "x = 1\n").unwrap();
        // `None` (nothing to check) when this process can read the file anyway (root).
        let checks = with_unreadable(&hostile, || compute_static_hash(&tools));
        checks.into_iter().for_each(|result| {
            let err = format!("{:#}", result.unwrap_err());
            assert!(!err.chars().any(char::is_control), "{err:?}");
            assert!(err.contains("a\\u{1b}[31mb.py"), "{err}");
        });
    }

    const AGED: std::time::Duration = std::time::Duration::from_secs(60);

    /// Set the mtime of `path` to `time`.
    fn set_mtime(path: &Path, time: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    /// Backdate every file under `project` by [`AGED`], so a manifest
    /// written now is newer than all of them.
    fn age(project: &TempDir) {
        let then = SystemTime::now() - AGED;
        for entry in WalkDir::new(project.path()).into_iter().flatten() {
            if entry.file_type().is_file() {
                set_mtime(entry.path(), then);
            }
        }
    }

    /// The manifest a persisting build would write for `project`: the
    /// filesystem clock read first, then the snapshot.
    fn written_manifest(project: &TempDir) -> crate::manifest::Manifest {
        let tools = project.path().join("tools");
        let clock = SnapshotClock::now(&tools);
        let mut input_records = snapshot_static_inputs(&tools, None).unwrap();
        clock.stamp(&mut input_records);
        manifest_of(project, input_records)
    }

    /// Write a manifest carrying `input_records` to `project`'s `tools/`.
    fn manifest_of(project: &TempDir, input_records: InputRecords) -> crate::manifest::Manifest {
        let manifest = crate::manifest::Manifest {
            static_hash: static_hash_of(&input_records.files),
            toolr_version: env!("CARGO_PKG_VERSION").to_string(),
            input_records,
            ..crate::manifest::Manifest::empty()
        };
        let path = project.path().join("tools/.toolr-manifest.json");
        crate::manifest::write_manifest(&path, &manifest).unwrap();
        manifest
    }

    fn compare_reads(
        project: &TempDir,
        manifest: &crate::manifest::Manifest,
    ) -> (
        crate::freshness::FreshnessVerdict,
        std::collections::HashMap<PathBuf, usize>,
    ) {
        let tools = project.path().join("tools");
        take_reads();
        let verdict = crate::freshness::compare(Some(manifest), &tools, None).unwrap();
        (verdict, take_reads())
    }

    fn reads_of(project: &TempDir, rels: &[&str]) -> std::collections::HashMap<PathBuf, usize> {
        rels.iter().map(|r| (project.path().join(r), 1)).collect()
    }

    fn mtime_ns(path: &Path) -> u64 {
        nanos(std::fs::metadata(path).and_then(|m| m.modified()))
    }

    const MIGRATE: &str = "apps/billing/management/commands/migrate.py";
    const ALL_INPUTS: [&str; 3] = [MIGRATE, "tools/dispatcher.py", "tools/pyproject.toml"];

    /// #656: an unchanged tree costs stats, not reads.
    #[test]
    fn fresh_compare_reads_nothing() {
        let project = django_project();
        age(&project);
        let manifest = written_manifest(&project);
        let (verdict, reads) = compare_reads(&project, &manifest);
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::Fresh);
        assert!(reads.is_empty(), "{reads:?}");
    }

    /// The filesystem clock is now, and leaves no file behind.
    #[test]
    fn fs_clock_reads_the_filesystem_and_cleans_up() {
        let project = tools_tree(&[("a.py", "x")]);
        let tools = project.path().join("tools");
        let before = nanos(Ok(SystemTime::now() - std::time::Duration::from_secs(5)));
        let clock = SnapshotClock::now(&tools);
        assert!(clock.at_ns > before);
        let a = std::fs::metadata(tools.join("a.py")).unwrap();
        assert_eq!(clock.dev, device(&a));
        let left: Vec<_> = std::fs::read_dir(&tools).unwrap().flatten().collect();
        assert_eq!(left.len(), 1, "{left:?}");
        assert_eq!(
            SnapshotClock::now(&project.path().join("missing")),
            SnapshotClock { at_ns: 0, dev: 0 }
        );
    }

    /// `build_to_persist` reads the clock before the build and stamps it on
    /// the built records; a failed build is returned as is.
    #[test]
    fn build_to_persist_stamps_a_clock_taken_before_the_build() {
        let project = tools_tree(&[
            ("pyproject.toml", "[project]\nname = \"tools\"\n"),
            ("a.py", "x = 1\n"),
        ]);
        let tools = project.path().join("tools");
        let mut built_at = 0;
        let manifest = build_to_persist(&tools, || {
            let probe = tools.join("probe.txt");
            std::fs::write(&probe, "x").unwrap();
            built_at = mtime_ns(&probe);
            std::fs::remove_file(probe).unwrap();
            crate::parser::build_static_manifest(&tools)
        })
        .unwrap()
        .manifest;
        let records = &manifest.input_records;
        assert_ne!(records.snapshot_at_ns, 0);
        assert!(records.snapshot_at_ns <= built_at, "{records:?}");
        assert_eq!(records.snapshot_dev, SnapshotClock::now(&tools).dev);
        let failed = build_to_persist(&tools, || Err::<crate::parser::BuiltManifest, _>("no"));
        assert_eq!(failed.unwrap_err(), "no");
    }

    /// Records without a bound (older manifests, or one written by a path
    /// that took none) reread every input once, the pyproject included, and
    /// are still fresh.
    #[test]
    fn compare_without_a_bound_reads_each_input_once() {
        let project = django_project();
        age(&project);
        let mut manifest = written_manifest(&project);
        manifest.input_records.snapshot_at_ns = 0;
        let (verdict, reads) = compare_reads(&project, &manifest);
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::Fresh);
        assert_eq!(reads, reads_of(&project, &ALL_INPUTS));

        manifest.input_records = Default::default();
        let (verdict, reads) = compare_reads(&project, &manifest);
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::Fresh);
        assert_eq!(reads, reads_of(&project, &ALL_INPUTS));
    }

    /// Editing one scanned file rereads only that file; the rebuild's
    /// snapshot (no reuse, as in `build_static_manifest`) then reads each
    /// input once.
    #[test]
    fn edited_scanned_file_is_the_only_read() {
        let project = django_project();
        age(&project);
        let manifest = written_manifest(&project);
        write(
            &project,
            MIGRATE,
            &format!("{MIGRATE_PY}    parser.add_argument('--database')\n"),
        );
        let (verdict, reads) = compare_reads(&project, &manifest);
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::StaticDrift);
        assert_eq!(reads, reads_of(&project, &[MIGRATE]));

        snapshot_static_inputs(&project.path().join("tools"), None).unwrap();
        assert_eq!(take_reads(), reads_of(&project, &ALL_INPUTS));
    }

    /// A new file matching a `scan_paths` glob is read and invalidates.
    #[test]
    fn new_scanned_file_invalidates() {
        let project = django_project();
        age(&project);
        let manifest = written_manifest(&project);
        let added = "apps/billing/management/commands/sync.py";
        write(&project, added, MIGRATE_PY);
        let (verdict, reads) = compare_reads(&project, &manifest);
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::StaticDrift);
        assert_eq!(reads, reads_of(&project, &[added]));
    }

    /// A removed scanned file invalidates without reading anything.
    #[test]
    fn removed_scanned_file_invalidates() {
        let project = django_project();
        age(&project);
        let manifest = written_manifest(&project);
        std::fs::remove_file(project.path().join(MIGRATE)).unwrap();
        let (verdict, reads) = compare_reads(&project, &manifest);
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::StaticDrift);
        assert!(reads.is_empty(), "{reads:?}");
    }

    /// An edited pyproject is read once, and its new globs apply.
    #[test]
    fn edited_pyproject_is_read_once_and_rescanned() {
        let project = django_project();
        age(&project);
        let manifest = written_manifest(&project);
        let pyproject = std::fs::read_to_string(project.path().join("tools/pyproject.toml"))
            .unwrap()
            .replace("apps/*/management/commands/*.py", "nowhere/*.py");
        write(&project, "tools/pyproject.toml", &pyproject);
        let tools = project.path().join("tools");
        take_reads();
        let records = snapshot_static_inputs(&tools, Some(&manifest.input_records)).unwrap();
        assert_eq!(take_reads(), reads_of(&project, &["tools/pyproject.toml"]));
        let keys: Vec<_> = records.files.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["tools/dispatcher.py", "tools/pyproject.toml"]);
        assert_eq!(records.scan_paths, [vec!["nowhere/*.py".to_string()]]);
    }

    /// A pyproject edit outside the argparse blocks rereads the pyproject
    /// once and leaves the manifest fresh.
    #[test]
    fn pyproject_edit_outside_argparse_is_read_and_fresh() {
        let project = django_project();
        age(&project);
        let manifest = written_manifest(&project);
        let pyproject = std::fs::read_to_string(project.path().join("tools/pyproject.toml"))
            .unwrap()
            .replace(
                "name = \"tools\"\n",
                "name = \"tools\"\ndependencies = [\"django\"]\n",
            );
        write(&project, "tools/pyproject.toml", &pyproject);
        let (verdict, reads) = compare_reads(&project, &manifest);
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::Fresh);
        assert_eq!(reads, reads_of(&project, &["tools/pyproject.toml"]));
    }

    /// The racy rule: a record whose mtime is not strictly older than the
    /// snapshot's bound is reread even though nothing changed.
    #[test]
    fn racy_record_is_reread() {
        let project = django_project();
        age(&project);
        let tools = project.path().join("tools");
        let mut previous = snapshot_static_inputs(&tools, None).unwrap();
        previous.snapshot_dev = SnapshotClock::now(&tools).dev;
        // `age` gave every file the same mtime.
        let mtime = mtime_ns(&project.path().join(MIGRATE));
        let mut reads_with = |bound| {
            previous.snapshot_at_ns = bound;
            take_reads();
            let records = snapshot_static_inputs(&tools, Some(&previous)).unwrap();
            assert_eq!(records.files, previous.files);
            take_reads()
        };
        assert_eq!(reads_with(mtime), reads_of(&project, &ALL_INPUTS));
        assert!(reads_with(mtime + 1).is_empty());
    }

    /// Review of #656, reproduced on a 1 s clock: the file is written, the
    /// snapshot stats it in the same tick, a same-size edit lands in that
    /// tick too, and the manifest is written a tick later. The manifest is
    /// newer than the file, but the snapshot isn't, so the file is reread.
    #[test]
    fn record_as_new_as_its_snapshot_is_reread_though_the_manifest_is_newer() {
        let project = django_project();
        age(&project);
        let tools = project.path().join("tools");
        let migrate = project.path().join(MIGRATE);
        let mut input_records = snapshot_static_inputs(&tools, None).unwrap();
        // The coarse tick both the file and the snapshot's bound fell in.
        input_records.snapshot_at_ns = mtime_ns(&migrate);
        input_records.snapshot_dev = SnapshotClock::now(&tools).dev;
        let manifest = manifest_of(&project, input_records);
        assert!(mtime_ns(&tools.join(".toolr-manifest.json")) > mtime_ns(&migrate));
        let (_, reads) = compare_reads(&project, &manifest);
        assert_eq!(reads, reads_of(&project, &ALL_INPUTS));
    }

    /// Review of #656: the bound is `tools/`'s clock, so an input on another
    /// device, whose mtimes come from another clock, is always reread.
    #[test]
    fn record_on_another_device_is_reread() {
        let project = django_project();
        age(&project);
        let mut manifest = written_manifest(&project);
        let (_, reads) = compare_reads(&project, &manifest);
        assert!(reads.is_empty(), "{reads:?}");
        manifest.input_records.snapshot_dev = manifest.input_records.snapshot_dev.wrapping_add(1);
        let (verdict, reads) = compare_reads(&project, &manifest);
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::Fresh);
        assert_eq!(reads, reads_of(&project, &ALL_INPUTS));
    }

    /// An input outside the project root is always reread, even on the same
    /// device: off Unix the device number is `0` everywhere, so this rule is
    /// all that keeps another filesystem's mtimes away from the bound.
    #[cfg(unix)]
    #[test]
    fn scanned_file_outside_the_project_root_is_reread() {
        let elsewhere = setup(&[("cmd.py", MIGRATE_PY)]);
        let cmd = elsewhere.path().join("cmd.py");
        let project = setup(&[
            (
                "tools/pyproject.toml",
                &format!(
                    "[tool.toolr.argparse.ext]\nscan_paths = ['{}']\n",
                    cmd.display()
                ),
            ),
            ("tools/a.py", "x"),
        ]);
        age(&project);
        age(&elsewhere);
        let manifest = written_manifest(&project);
        let (verdict, reads) = compare_reads(&project, &manifest);
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::Fresh);
        assert_eq!(reads, [(cmd, 1)].into_iter().collect());
    }

    /// Same size, mtime restored, manifest written well after: ctime still
    /// moved, so the edit is caught.
    #[cfg(unix)]
    #[test]
    fn same_size_edit_with_restored_mtime_is_detected() {
        let project = django_project();
        age(&project);
        let manifest = written_manifest(&project);
        let path = project.path().join(MIGRATE);
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        write(&project, MIGRATE, &MIGRATE_PY.replace("--check", "--force"));
        set_mtime(&path, mtime);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
        let (verdict, reads) = compare_reads(&project, &manifest);
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::StaticDrift);
        assert_eq!(reads, reads_of(&project, &[MIGRATE]));
    }

    /// Fixing an unreadable scanned file changes neither size nor mtime, so
    /// its record is never reused.
    #[cfg(unix)]
    #[test]
    fn unreadable_record_is_never_reused() {
        let project = django_project();
        age(&project);
        let path = project.path().join(MIGRATE);
        let Some(manifest) = with_unreadable(&path, || written_manifest(&project)) else {
            return;
        };
        let record = &manifest.input_records.files[0];
        assert_eq!(record.key, MIGRATE);
        assert!(
            matches!(record.content, Content::Unreadable(_)),
            "{record:?}"
        );
        let (verdict, reads) = compare_reads(&project, &manifest);
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::StaticDrift);
        assert_eq!(reads, reads_of(&project, &[MIGRATE]));
    }

    /// Records that are not ours (unsorted, unknown keys) are only ever
    /// missed, never misapplied.
    #[test]
    fn foreign_records_are_not_reused() {
        let project = django_project();
        age(&project);
        let mut manifest = written_manifest(&project);
        manifest.input_records.files.reverse();
        manifest.input_records.files[0].key = "tools/other.py".into();
        let (_, reads) = compare_reads(&project, &manifest);
        assert!(reads.contains_key(&project.path().join("tools/pyproject.toml")));
    }

    #[test]
    fn nanos_is_zero_before_the_epoch_or_on_error() {
        assert_eq!(nanos(Ok(UNIX_EPOCH - std::time::Duration::from_secs(1))), 0);
        assert_eq!(nanos(Err(std::io::ErrorKind::Unsupported.into())), 0);
        // Windows `SystemTime` ticks in 100 ns, so a smaller offset rounds to 0.
        assert_eq!(
            nanos(Ok(UNIX_EPOCH + std::time::Duration::from_micros(5))),
            5_000
        );
    }

    #[test]
    fn ignores_non_py_files() {
        let a = tools_tree(&[("a.py", "x"), ("readme.md", "ignored")]);
        let b = tools_tree(&[("a.py", "x")]);
        assert_eq!(tools_hash(&a), tools_hash(&b));
    }
}
