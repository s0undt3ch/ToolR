//! Stable hashing over the inputs of the static manifest build.

use std::path::{Path, PathBuf};

use anyhow::Result;
use blake3::Hasher;
use walkdir::WalkDir;

use crate::text::escape_control_chars;

/// One file that feeds `static_hash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticInput {
    /// The path relative to the project root, components joined with `/`;
    /// a file outside the project root keeps its full path.
    pub key: String,
    pub path: PathBuf,
    /// A `tools/**/*.py` module. Only a module's read errors fail the hash;
    /// see [`hash_paths`].
    pub module: bool,
    /// `tools/pyproject.toml`: only its `[tool.toolr.argparse.*]` blocks
    /// are hashed; see [`hash_paths`].
    pub config: bool,
}

/// The manifest's `static_hash`: every input of the static build, hashed in
/// key order so the result is reproducible across runs and machines.
///
/// Fails unwrapped with [`SymlinkedModules`] like [`list_python_files`].
pub fn compute_static_hash(tools_dir: &Path) -> Result<String> {
    hash_paths(&static_inputs(tools_dir)?)
}

/// Every input of the static build: `tools/**/*.py`, `tools/pyproject.toml`
/// (for its `[tool.toolr.argparse.*]` blocks) and the files those blocks
/// scan. Sorted by key, without duplicate keys.
pub fn static_inputs(tools_dir: &Path) -> Result<Vec<StaticInput>> {
    Ok(static_inputs_from(
        tools_dir,
        &list_python_files(tools_dir)?,
    ))
}

/// [`static_inputs`] with `tools/**/*.py` already listed, so the build walks
/// `tools/` once for both the parser and the hash.
///
/// The argparse config never fails the hash, or a typo in it would fail
/// `--help`, dispatch and TAB through the freshness check. A pyproject
/// that doesn't parse, or a block with a bad glob, adds no scanned files;
/// the pyproject itself is still an input, so fixing it rebuilds, and the
/// build reports the error.
pub(crate) fn static_inputs_from(tools_dir: &Path, py_files: &[PathBuf]) -> Vec<StaticInput> {
    // Same root and pyproject as `argparse::run_for_project`.
    let project_root = tools_dir.parent().unwrap_or(tools_dir);
    let mut paths: Vec<(PathBuf, bool)> = py_files.iter().map(|p| (p.clone(), true)).collect();
    let pyproject = project_root.join("tools").join("pyproject.toml");
    let has_pyproject = pyproject.is_file();
    if has_pyproject {
        let blocks = read_input(&pyproject)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| crate::argparse::parse_blocks(&text).ok())
            .unwrap_or_default();
        for block in blocks {
            if let Ok(scanned) =
                crate::argparse::scan::expand_scan_paths(project_root, &block.scan_paths)
            {
                paths.extend(scanned.into_iter().map(|p| (p, false)));
            }
        }
    }
    let mut inputs: Vec<StaticInput> = paths
        .into_iter()
        .map(|(path, module)| StaticInput {
            key: input_key(project_root, &path),
            path,
            module,
            config: false,
        })
        .collect();
    if has_pyproject {
        inputs.push(StaticInput {
            key: input_key(project_root, &pyproject),
            path: pyproject,
            module: false,
            config: true,
        });
    }
    // A module also matched by a `scan_paths` glob keeps its strict reads,
    // and a scanned pyproject its full bytes: the scanner reads all of it.
    inputs.sort_by(|a, b| {
        a.key
            .cmp(&b.key)
            .then(b.module.cmp(&a.module))
            .then(a.config.cmp(&b.config))
    });
    inputs.dedup_by(|a, b| a.key == b.key);
    inputs
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

/// Hash each input as `key || 0x00 || len(contents) as u64 LE || contents`.
///
/// The pyproject is hashed as `key || 0x02 || len || <argparse blocks>`
/// instead, so edits outside `[tool.toolr.argparse.*]` (a dependency bump,
/// `uv add`) leave the hash alone; see [`argparse_digest`]. A pyproject
/// the build can't read its blocks from keeps the raw-bytes form, so
/// fixing it still changes the hash.
///
/// A module that can't be read fails the hash, naming the file. Any other
/// input that can't be read is hashed as `key || 0x01 || <error kind>`
/// instead: the scanner turns such a file into a placeholder with a
/// warning, and the pyproject's read error is the build's to report, so
/// neither may fail `--help`, dispatch and TAB through the freshness
/// check. Fixing the permission still changes the hash.
pub(crate) fn hash_paths(inputs: &[StaticInput]) -> Result<String> {
    let mut hasher = Hasher::new();
    for StaticInput {
        key,
        path,
        module,
        config,
    } in inputs
    {
        let bytes = match read_input(path) {
            Ok(bytes) => bytes,
            // Deleted since the walk listed it: hash the tree as it now is.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) if !module => {
                hasher.update(key.as_bytes());
                hasher.update(b"\x01");
                hasher.update(e.kind().to_string().as_bytes());
                continue;
            }
            Err(e) => {
                return Err(anyhow::Error::new(e).context(format!("reading {}", path.display())));
            }
        };
        hasher.update(key.as_bytes());
        let (tag, bytes) = match config.then(|| argparse_digest(&bytes)).flatten() {
            Some(digest) => (b"\x02", digest),
            None => (b"\0", bytes),
        };
        hasher.update(tag);
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// The pyproject's `[tool.toolr.argparse.*]` blocks as the build parses
/// them, serialised in name order: the only part of the file the build
/// reads. `None` when they don't parse, so the caller hashes the raw bytes.
fn argparse_digest(bytes: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let blocks = crate::argparse::parse_blocks(text).ok()?;
    serde_json::to_vec(&blocks).ok()
}

fn read_input(path: &Path) -> std::io::Result<Vec<u8>> {
    #[cfg(test)]
    tests::count_read(path);
    // `path` is listed by `static_inputs` from the project's own config, not untrusted input.
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
            module: true,
            config: false,
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
                module: true,
                config: false,
            },
            StaticInput {
                key: "pkg.py".into(),
                path: dir.clone(),
                module: true,
                config: false,
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

    /// F-16 layout: a dispatcher in `tools/` and a management command its
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
        static_inputs(&project.path().join("tools"))
            .unwrap()
            .into_iter()
            .map(|i| i.key)
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

    /// The F-16 review shape: a dependency line mid-edit makes the whole
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

    /// A pyproject a `scan_paths` glob also matches is scanned as a file,
    /// so all of its bytes count.
    #[test]
    fn scanned_pyproject_is_hashed_whole() {
        let pyproject = "[tool.toolr.argparse.all]\nscan_paths = [\"tools/*\"]\n";
        let project = setup(&[("tools/pyproject.toml", pyproject)]);
        let inputs = static_inputs(&project.path().join("tools")).unwrap();
        assert_eq!(inputs.len(), 1, "{inputs:?}");
        assert!(!inputs[0].config, "{inputs:?}");
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
        let inputs = static_inputs(&project.path().join("tools")).unwrap();
        assert!(
            inputs[0].module,
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
            (static_inputs(&tools).unwrap(), compute_static_hash(&tools))
        });
        if let Some((inputs, hash)) = locked {
            let keys: Vec<_> = inputs.into_iter().map(|i| i.key).collect();
            assert_eq!(keys, ["tools/dispatcher.py", "tools/pyproject.toml"]);
            assert!(hash.is_ok());
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

    /// Baseline for the stat gate F-30 adds: a freshness check on an
    /// unchanged tree reads each scanned file and module once, and the
    /// pyproject twice (parse, then hash).
    #[test]
    fn fresh_compare_reads_each_input_once() {
        let project = django_project();
        let tools = project.path().join("tools");
        let manifest = crate::manifest::Manifest {
            schema_version: crate::manifest::SCHEMA_VERSION,
            static_hash: compute_static_hash(&tools).unwrap(),
            third_party_hash: String::new(),
            toolr_version: env!("CARGO_PKG_VERSION").to_string(),
            groups: vec![],
            commands: vec![],
            plugin_warnings: Vec::new(),
        };
        take_reads();
        let verdict = crate::freshness::compare(Some(&manifest), &tools, None).unwrap();
        assert_eq!(verdict, crate::freshness::FreshnessVerdict::Fresh);
        let reads = take_reads();
        let count = |rel: &str| reads.get(&project.path().join(rel)).copied();
        assert_eq!(
            count("apps/billing/management/commands/migrate.py"),
            Some(1)
        );
        assert_eq!(count("tools/dispatcher.py"), Some(1));
        assert_eq!(count("tools/pyproject.toml"), Some(2));
        assert_eq!(reads.len(), 3, "{reads:?}");
    }

    #[test]
    fn ignores_non_py_files() {
        let a = tools_tree(&[("a.py", "x"), ("readme.md", "ignored")]);
        let b = tools_tree(&[("a.py", "x")]);
        assert_eq!(tools_hash(&a), tools_hash(&b));
    }
}
