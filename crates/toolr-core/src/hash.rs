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
mod tests;
