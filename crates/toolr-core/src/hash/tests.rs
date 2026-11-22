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

const MIGRATE_PY: &str =
    "def add_arguments(self, parser):\n    parser.add_argument('--check', action='store_true')\n";

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
