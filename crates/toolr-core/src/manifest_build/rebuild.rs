//! The one manifest rebuild pipeline: [`refresh`] for the automatic paths
//! (dispatch, `--help`, TAB) and [`rebuild_manifest_full`] for
//! `project manifest rebuild`. Between them they own the rebuild lock, the
//! snapshot clock, the third-party hash and what a build carries forward
//! from the cache.

use std::path::Path;

use anyhow::{Context, Result};

use super::hash::{compute_third_party_hash, empty_third_party_hash};
use crate::freshness::{FreshnessVerdict, compare};
use crate::hash::{build_to_persist, context_unless_symlinked, is_symlinked_modules};
use crate::manifest::{
    Manifest, ManifestError, Origin, acquire_rebuild_lock, load_manifest, rebuild_if_stale,
    write_manifest,
};
use crate::parser::{
    BuildError, BuiltManifest, build_static_manifest, build_static_manifest_with_venv,
};

/// What [`refresh`] served.
#[derive(Debug)]
pub enum Refresh {
    /// The manifest on disk matched the live state.
    Fresh(Manifest),
    /// A new build, written when persisting.
    Built(BuiltManifest),
    /// The build failed; `cached` is the manifest on disk, stale or not,
    /// when it loaded. Symlinked modules are an `Err` from [`refresh`]
    /// instead, since the cache would hide the links again.
    Failed {
        error: anyhow::Error,
        cached: Option<Manifest>,
    },
}

/// [`refresh`]'s result, with the first error that made the manifest on
/// disk unreadable (a missing file isn't one).
#[derive(Debug)]
pub struct Refreshed {
    pub refresh: Refresh,
    pub unreadable: Option<ManifestError>,
}

/// Serve `project_root`'s manifest, rebuilding it when it's stale.
///
/// `venv` decides how a build treats third-party plugins: with one, their
/// fragments are globbed and hashed afresh; without one (TAB, or a venv
/// that doesn't resolve), the cached plugin entries, warnings and hash are
/// carried forward. When `persist`, a stale manifest is rebuilt under the
/// rebuild lock (re-checked once held) and written; otherwise it is built
/// in memory with no lock, no clock and no write.
pub fn refresh(project_root: &Path, venv: Option<&Path>, persist: bool) -> Result<Refreshed> {
    let tools = project_root.join("tools");
    let manifest_path = tools.join(".toolr-manifest.json");
    let mut unreadable = None;
    let mut fresh = None;
    // When persisting a stale manifest this runs twice, before and under the lock.
    let check = || -> Result<Option<Option<Manifest>>> {
        let cached = match load_manifest(&manifest_path) {
            Ok(m) => Some(m),
            Err(ManifestError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                unreadable.get_or_insert(e);
                None
            }
        };
        if compare(cached.as_ref(), &tools, venv)? == FreshnessVerdict::Fresh {
            fresh = cached;
            return Ok(None);
        }
        Ok(Some(cached))
    };
    let rebuild = |cached: Option<Manifest>| -> Result<Refresh> {
        let built = if persist {
            build_to_persist(&tools, || build(&tools, venv, cached.as_ref()))
        } else {
            build(&tools, venv, cached.as_ref())
        };
        match built {
            Ok(built) => {
                if persist {
                    write_manifest(&manifest_path, &built.manifest)
                        .with_context(|| format!("writing {}", manifest_path.display()))?;
                }
                Ok(Refresh::Built(built))
            }
            Err(e) if is_symlinked_modules(&e) => Err(e),
            Err(error) => Ok(Refresh::Failed { error, cached }),
        }
    };
    let rebuilt = if persist {
        rebuild_if_stale(&manifest_path, check, rebuild)?
    } else {
        let mut check = check;
        check()?.map(rebuild).transpose()?
    };
    // `compare` never calls a missing cache fresh, so one of the two is set.
    let refresh = rebuilt
        .or(fresh.map(Refresh::Fresh))
        .context("manifest neither fresh nor rebuilt")?;
    Ok(Refreshed {
        refresh,
        unreadable,
    })
}

/// Rebuild `project_root`'s manifest unconditionally from the tree and the
/// venv's plugin fragments, and write it. The lock only serialises this
/// with concurrent auto-rebuilds.
pub fn rebuild_manifest_full(project_root: &Path, venv: &Path) -> Result<BuiltManifest> {
    let tools = project_root.join("tools");
    let manifest_path = tools.join(".toolr-manifest.json");
    let _lock = acquire_rebuild_lock(&manifest_path);
    let built = build_to_persist(&tools, || build(&tools, Some(venv), None)).map_err(|e| {
        context_unless_symlinked(e, "building static manifest (incl. third-party fragments)")
    })?;
    write_manifest(&manifest_path, &built.manifest)?;
    Ok(built)
}

/// Build from `tools` and stamp the third-party hash. With a `venv`, plugin
/// entries come from its fragments; without one, from `cached`.
fn build(tools: &Path, venv: Option<&Path>, cached: Option<&Manifest>) -> Result<BuiltManifest> {
    let Some(venv) = venv else {
        let mut built = build_static_manifest(tools)?;
        match cached {
            Some(cached) => carry_forward(&mut built.manifest, cached),
            None => built.manifest.third_party_hash = empty_third_party_hash(),
        }
        return Ok(built);
    };
    let mut built =
        build_static_manifest_with_venv(tools, venv).map_err(BuildError::into_anyhow)?;
    built.manifest.third_party_hash =
        compute_third_party_hash(venv).context("hashing third-party manifests")?;
    Ok(built)
}

/// Carry `cached`'s plugin entries that no fresh entry shadows, its plugin
/// warnings (a stale warning beats a silently hidden command) and its
/// third-party hash into a build that didn't glob the venv.
fn carry_forward(fresh: &mut Manifest, cached: &Manifest) {
    for group in &cached.groups {
        if group.origin == Origin::ThirdParty
            && !fresh
                .groups
                .iter()
                .any(|g| g.full_path() == group.full_path())
        {
            fresh.groups.push(group.clone());
        }
    }
    for cmd in &cached.commands {
        if cmd.origin == Origin::ThirdParty
            && !fresh
                .commands
                .iter()
                .any(|c| c.group == cmd.group && c.name == cmd.name)
        {
            fresh.commands.push(cmd.clone());
        }
    }
    fresh.plugin_warnings.clone_from(&cached.plugin_warnings);
    fresh.third_party_hash.clone_from(&cached.third_party_hash);
}

#[cfg(test)]
mod tests {
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
        std::fs::create_dir_all(venv.join("lib/python3.13/site-packages/foo-1.0.0.dist-info"))
            .unwrap();
        rebuild_manifest_full(project, &venv).unwrap();
        let m = load_manifest(&tools.join(".toolr-manifest.json")).unwrap();
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
        let built = rebuild_manifest_full(project, &venv).unwrap();
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
        let built = rebuild_manifest_full(project, &venv).unwrap();
        assert_eq!(
            built.all_warnings().collect::<Vec<_>>(),
            [
                "group `grp` is declared more than once; keeping the title from tools.m1 and \
              ignoring the different title in tools.m2"
            ]
        );
    }

    /// An explicit rebuild serialises with auto-rebuilds on the repo's lock:
    /// it waits while another holder has it, then writes.
    #[test]
    fn full_rebuild_waits_for_the_rebuild_lock() {
        let (_env, cache_home) = EnvGuard::temp_cache_home();
        let tmp = TempDir::new().unwrap();
        let project = tmp.path().to_path_buf();
        std::fs::create_dir(project.join("tools")).unwrap();
        let manifest_path = project.join("tools/.toolr-manifest.json");
        let held = acquire_rebuild_lock(&manifest_path);
        let lock_file =
            crate::manifest::lock_path_for_repo(&cache_home.path().join("toolr"), &project);
        assert!(lock_file.is_file());
        let venv = project.join("venv");
        let worker = std::thread::spawn(move || rebuild_manifest_full(&project, &venv).map(|_| ()));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!worker.is_finished());
        assert!(!manifest_path.exists());
        drop(held);
        worker.join().unwrap().unwrap();
        assert!(manifest_path.is_file());
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
            schema_version: crate::manifest::SCHEMA_VERSION,
            static_hash: String::new(),
            third_party_hash: "cached-hash".into(),
            toolr_version: String::new(),
            groups,
            commands: vec![],
            plugin_warnings: vec![crate::manifest::PluginWarning {
                package: "demo".into(),
                path: "site-packages/demo/toolr-manifest.json".into(),
                kind: crate::manifest::PluginWarningKind::Shadowed,
                message: "tools/ci.py defines ci lint, hiding the one from demo".into(),
            }],
            input_records: Default::default(),
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
        tools.join(".toolr-manifest.json")
    }

    /// Without a venv, a stale manifest's plugin state carries forward; a
    /// missing one gets the empty-venv hash.
    #[test]
    fn refresh_without_a_venv_keeps_the_cached_plugin_state() {
        let tmp = TempDir::new().unwrap();
        let path = tools_tree(&tmp);
        let Refresh::Built(first) = refresh(tmp.path(), None, false).unwrap().refresh else {
            panic!("a missing manifest is built");
        };
        assert_eq!(first.manifest.third_party_hash, empty_third_party_hash());
        let mut cached = manifest_with(vec![group("image", "docker", Origin::ThirdParty)]);
        cached.toolr_version = "0.0.0-stale".into();
        write_manifest(&path, &cached).unwrap();
        let Refresh::Built(built) = refresh(tmp.path(), None, false).unwrap().refresh else {
            panic!("a stale manifest is rebuilt");
        };
        assert_eq!(built.manifest.third_party_hash, "cached-hash");
        assert_eq!(built.manifest.plugin_warnings, cached.plugin_warnings);
        assert_eq!(built.manifest.groups.len(), 1);
        assert_eq!(load_manifest(&path).unwrap().toolr_version, "0.0.0-stale");
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
        let Refresh::Built(built) = refresh(tmp.path(), Some(&venv), false).unwrap().refresh else {
            panic!("a stale manifest is rebuilt");
        };
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
        let first = refresh(tmp.path(), Some(&venv), true).unwrap();
        assert!(matches!(first.refresh, Refresh::Built(_)));
        assert!(first.unreadable.is_none());
        let lock =
            crate::manifest::lock_path_for_repo(&cache_home.path().join("toolr"), tmp.path());
        assert!(lock.is_file());
        let written = load_manifest(&path).unwrap();
        let Refresh::Fresh(served) = refresh(tmp.path(), Some(&venv), true).unwrap().refresh else {
            panic!("the written manifest is fresh");
        };
        assert_eq!(served, written);
    }

    /// An unreadable manifest is reported and rebuilt.
    #[test]
    fn refresh_reports_an_unreadable_manifest() {
        let tmp = TempDir::new().unwrap();
        let path = tools_tree(&tmp);
        std::fs::write(&path, "not json").unwrap();
        let refreshed = refresh(tmp.path(), None, false).unwrap();
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
        let Refresh::Failed {
            error,
            cached: kept,
        } = refresh(tmp.path(), None, true).unwrap().refresh
        else {
            panic!("a broken module fails the build");
        };
        assert!(error.to_string().starts_with("parsing "), "{error:#}");
        assert_eq!(kept, Some(cached));
        assert_eq!(load_manifest(&path).unwrap().toolr_version, "0.0.0-stale");
    }
}
