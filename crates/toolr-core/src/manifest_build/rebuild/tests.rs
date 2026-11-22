use super::*;
use crate::test_env::EnvGuard;
use tempfile::TempDir;

#[test]
fn full_rebuild_writes_static_manifest() {
    let (_env, _cache_home) = EnvGuard::temp_cache_home();
    let tmp = TempDir::new().unwrap();
    let project = tmp.path();
    let tools = project.join("tools");
    std::fs::create_dir(&tools).unwrap();
    std::fs::write(
        tools.join("ci.py"),
        "\"\"\"CI utilities.\"\"\"\ngroup = command_group(\"ci\", \"CI utilities\", \"CI utilities.\")\n@group.command\ndef hello(ctx):\n    \"\"\"Say hello.\"\"\"\n    pass\n",
    )
    .unwrap();
    let venv = project.join("venv");
    std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/foo-1.0.0.dist-info")).unwrap();
    rebuild_manifest_full(project, Some(&venv)).unwrap();
    let m = load_manifest(&manifest_path(project)).unwrap();
    let group_names: Vec<_> = m.groups.iter().map(|g| g.name.as_str()).collect();
    assert!(group_names.contains(&"ci"));
    assert!(!m.third_party_hash.is_empty());
}

#[test]
fn full_rebuild_reports_plugin_warnings() {
    let (_env, _cache_home) = EnvGuard::temp_cache_home();
    let tmp = TempDir::new().unwrap();
    let project = tmp.path();
    std::fs::create_dir(project.join("tools")).unwrap();
    let venv = project.join("venv");
    let pkg = venv.join("lib/python3.13/site-packages/demo");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("toolr-manifest.json"),
        r#"{"toolr_schema_version": 1, "package": "demo"}"#,
    )
    .unwrap();
    let built = rebuild_manifest_full(project, Some(&venv)).unwrap();
    assert_eq!(
        built.all_warnings().collect::<Vec<_>>(),
        [
            "skipping plugin demo: built with toolr schema 1, this toolr needs >= 2. \
          Rebuild the plugin with toolr >= 0.34.0."
        ]
    );
}

/// `project manifest rebuild` reports the local build's warnings too.
#[test]
fn full_rebuild_reports_build_warnings() {
    let (_env, _cache_home) = EnvGuard::temp_cache_home();
    let tmp = TempDir::new().unwrap();
    let project = tmp.path();
    let tools = project.join("tools");
    std::fs::create_dir(&tools).unwrap();
    for (module, title) in [("m1", "Grp"), ("m2", "Other")] {
        std::fs::write(
            tools.join(format!("{module}.py")),
            format!("g = command_group(\"grp\", \"{title}\", \"Grp\")\n"),
        )
        .unwrap();
    }
    let venv = project.join("venv");
    std::fs::create_dir(&venv).unwrap();
    let built = rebuild_manifest_full(project, Some(&venv)).unwrap();
    assert_eq!(
        built.all_warnings().collect::<Vec<_>>(),
        [
            "group `grp` is declared more than once; keeping the title from tools.m1 and \
          ignoring the different title in tools.m2"
        ]
    );
}

/// A worker that isn't blocked finishes well within this; too short a
/// wait can only make [`while_locked`] pass falsely, never fail falsely.
const BLOCKED_FOR: std::time::Duration = std::time::Duration::from_millis(200);

/// Run `work` on a thread while `project_root`'s rebuild lock is held,
/// assert it's still waiting after [`BLOCKED_FOR`], run `meanwhile`, then
/// release the lock and return what `work` returned.
fn while_locked<T: Send + 'static>(
    project_root: &Path,
    work: impl FnOnce() -> T + Send + 'static,
    meanwhile: impl FnOnce(),
) -> T {
    let held = acquire_rebuild_lock(&manifest_path(project_root));
    let worker = std::thread::spawn(work);
    std::thread::sleep(BLOCKED_FOR);
    assert!(!worker.is_finished(), "the worker didn't wait for the lock");
    meanwhile();
    drop(held);
    worker.join().unwrap()
}

/// An explicit rebuild serialises with auto-rebuilds on the repo's lock:
/// it waits while another holder has it, then writes.
#[test]
fn full_rebuild_waits_for_the_rebuild_lock() {
    let (_env, _cache_home) = EnvGuard::temp_cache_home();
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().to_path_buf();
    std::fs::create_dir(project.join("tools")).unwrap();
    let path = manifest_path(&project);
    let (root, venv) = (project.clone(), project.join("venv"));
    let rebuilt = while_locked(
        &project,
        move || rebuild_manifest_full(&root, Some(&venv)).map(|_| ()),
        || assert!(!path.exists()),
    );
    rebuilt.unwrap();
    assert!(path.is_file());
}

/// A fresh tree whose manifest another process writes while this one
/// waits on the lock: a failed build hands back that manifest.
#[test]
fn refresh_serves_a_manifest_written_while_waiting_on_the_lock() {
    let (_env, _cache_home) = EnvGuard::temp_cache_home();
    let tmp = TempDir::new().unwrap();
    let path = tools_tree(&tmp);
    std::fs::write(tmp.path().join("tools/broken.py"), "def not closed(\n").unwrap();
    let mut stale = manifest_with(vec![]);
    stale.toolr_version = "0.0.0-stale".into();
    let root = tmp.path().to_path_buf();
    let served = while_locked(
        tmp.path(),
        move || refresh(&root, None, Persist::Write).unwrap().refresh,
        || write_manifest(&path, &stale).unwrap(),
    );
    assert!(
        matches!(&served, Refresh::Failed { cached: Some(c), .. } if *c == stale),
        "{served:?}"
    );
}

fn group(name: &str, parent: &str, origin: Origin) -> crate::manifest::Group {
    crate::manifest::Group {
        name: name.into(),
        title: name.into(),
        description: String::new(),
        parent: Some(parent.into()),
        origin,
    }
}

fn manifest_with(groups: Vec<crate::manifest::Group>) -> Manifest {
    Manifest {
        third_party_hash: "cached-hash".into(),
        groups,
        plugin_warnings: vec![crate::manifest::PluginWarning {
            package: "demo".into(),
            path: "site-packages/demo/toolr-manifest.json".into(),
            kind: crate::manifest::PluginWarningKind::Shadowed,
            message: "tools/ci.py defines ci lint, hiding the one from demo".into(),
        }],
        ..Manifest::empty()
    }
}

#[test]
fn carry_forward_keeps_same_leaf_groups_with_different_parents() {
    let mut fresh = manifest_with(vec![group("image", "ci", Origin::Static)]);
    fresh.plugin_warnings.clear();
    let cached = manifest_with(vec![
        group("image", "docker", Origin::ThirdParty),
        group("image", "ci", Origin::ThirdParty),
        group("old", "ci", Origin::Static),
    ]);
    carry_forward(&mut fresh, &cached);
    let paths: Vec<String> = fresh
        .groups
        .iter()
        .map(crate::manifest::Group::full_path)
        .collect();
    assert_eq!(paths, ["ci.image", "docker.image"]);
    assert_eq!(fresh.plugin_warnings, cached.plugin_warnings);
    assert_eq!(fresh.third_party_hash, "cached-hash");
}

/// A tools tree with one module, and its manifest path.
fn tools_tree(tmp: &TempDir) -> std::path::PathBuf {
    let tools = tmp.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    std::fs::write(tools.join("demo.py"), "\"\"\"Demo.\"\"\"\n").unwrap();
    manifest_path(tmp.path())
}

/// Without a venv, a stale manifest's plugin state carries forward; a
/// missing one gets the empty-venv hash.
#[test]
fn refresh_without_a_venv_keeps_the_cached_plugin_state() {
    let (_env, cache_home) = EnvGuard::temp_cache_home();
    let tmp = TempDir::new().unwrap();
    let path = tools_tree(&tmp);
    let r = refresh(tmp.path(), None, Persist::InMemory)
        .unwrap()
        .refresh;
    let Refresh::Built(first) = r else { panic!() };
    assert_eq!(first.manifest.third_party_hash, empty_third_party_hash());
    let mut cached = manifest_with(vec![group("image", "docker", Origin::ThirdParty)]);
    cached.toolr_version = "0.0.0-stale".into();
    write_manifest(&path, &cached).unwrap();
    let r = refresh(tmp.path(), None, Persist::InMemory)
        .unwrap()
        .refresh;
    let Refresh::Built(built) = r else { panic!() };
    assert_eq!(built.manifest.third_party_hash, "cached-hash");
    assert_eq!(built.manifest.plugin_warnings, cached.plugin_warnings);
    assert_eq!(built.manifest.groups.len(), 1);
    assert_eq!(load_manifest(&path).unwrap().toolr_version, "0.0.0-stale");
    // Not persisting takes no lock either.
    let lock = crate::manifest::lock_path_for_repo(&cache_home.path().join("toolr"), tmp.path());
    assert!(!lock.exists());
}

/// With a venv, the plugin state comes from the venv, not the cache.
#[test]
fn refresh_with_a_venv_drops_the_cached_plugin_state() {
    let tmp = TempDir::new().unwrap();
    let path = tools_tree(&tmp);
    let mut cached = manifest_with(vec![group("image", "docker", Origin::ThirdParty)]);
    cached.toolr_version = "0.0.0-stale".into();
    write_manifest(&path, &cached).unwrap();
    let venv = tmp.path().join("venv");
    let r = refresh(tmp.path(), Some(&venv), Persist::InMemory)
        .unwrap()
        .refresh;
    let Refresh::Built(built) = r else { panic!() };
    assert!(built.manifest.groups.is_empty());
    assert!(built.manifest.plugin_warnings.is_empty());
    assert_eq!(
        built.manifest.third_party_hash,
        compute_third_party_hash(&venv).unwrap()
    );
}

/// Persisting writes the build under the lock, and the written manifest
/// is fresh for the next call.
#[test]
fn refresh_persists_a_manifest_that_stays_fresh() {
    let (_env, cache_home) = EnvGuard::temp_cache_home();
    let tmp = TempDir::new().unwrap();
    let path = tools_tree(&tmp);
    let venv = tmp.path().join("venv");
    let first = refresh(tmp.path(), Some(&venv), Persist::Write).unwrap();
    assert!(matches!(first.refresh, Refresh::Built(_)));
    assert!(first.unreadable.is_none());
    let lock = crate::manifest::lock_path_for_repo(&cache_home.path().join("toolr"), tmp.path());
    assert!(lock.is_file());
    let written = load_manifest(&path).unwrap();
    let r = refresh(tmp.path(), Some(&venv), Persist::Write)
        .unwrap()
        .refresh;
    let Refresh::Fresh(served) = r else { panic!() };
    assert_eq!(served, written);
}

/// An unreadable manifest is reported and rebuilt.
#[test]
fn refresh_reports_an_unreadable_manifest() {
    let tmp = TempDir::new().unwrap();
    let path = tools_tree(&tmp);
    std::fs::write(&path, "not json").unwrap();
    let refreshed = refresh(tmp.path(), None, Persist::InMemory).unwrap();
    assert!(matches!(refreshed.refresh, Refresh::Built(_)));
    assert!(
        matches!(refreshed.unreadable, Some(ManifestError::Json(_))),
        "{refreshed:?}"
    );
}

/// A failed build hands back the cache; nothing is written.
#[test]
fn refresh_failure_keeps_the_cache() {
    let (_env, _cache_home) = EnvGuard::temp_cache_home();
    let tmp = TempDir::new().unwrap();
    let path = tools_tree(&tmp);
    let mut cached = manifest_with(vec![]);
    cached.toolr_version = "0.0.0-stale".into();
    write_manifest(&path, &cached).unwrap();
    std::fs::write(tmp.path().join("tools/broken.py"), "def not closed(\n").unwrap();
    let r = refresh(tmp.path(), None, Persist::Write).unwrap().refresh;
    assert!(
        matches!(&r, Refresh::Failed { error, cached: Some(c) }
            if *c == cached && error.to_string().starts_with("parsing ")),
        "{r:?}"
    );
    assert_eq!(load_manifest(&path).unwrap().toolr_version, "0.0.0-stale");
}
