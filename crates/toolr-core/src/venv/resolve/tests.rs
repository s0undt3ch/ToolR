use super::*;
use crate::test_env::EnvGuard;
use tempfile::TempDir;

fn setup_repo(body: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir(&tools).unwrap();
    std::fs::write(tools.join("pyproject.toml"), body).unwrap();
    tmp
}

#[test]
fn cache_slot_dir_keeps_in_tree_sidecar_apart_from_the_cache_venv_slot() {
    let tmp = TempDir::new().unwrap();
    let root = Path::new("/cache/toolr");
    let key = compute_repo_key(tmp.path(), "3.13").unwrap();
    let cache = cache_slot_dir(root, tmp.path(), "3.13", VenvLocation::Cache).unwrap();
    let in_tree = cache_slot_dir(root, tmp.path(), "3.13", VenvLocation::InTree).unwrap();
    assert_eq!(cache, root.join(&key));
    assert_eq!(in_tree, root.join(format!("{key}{IN_TREE_SLOT_SUFFIX}")));
}

#[test]
fn cache_default_uses_repo_key_subdir() {
    let mut env = EnvGuard::write();
    env.remove(VENV_LOCATION_ENV);
    let tmp = setup_repo("[project]\nname=\"x\"\nversion=\"0\"\n");
    let resolved = resolve_venv_path(tmp.path()).unwrap();
    assert!(resolved.venv_dir.ends_with("venv"));
    assert!(!resolved.repo_key.is_empty());
    assert!(
        resolved
            .venv_dir
            .to_string_lossy()
            .contains(&resolved.repo_key)
    );
}

#[test]
fn in_tree_lands_inside_tools_dot_venv() {
    let mut env = EnvGuard::write();
    env.remove(VENV_LOCATION_ENV);
    let tmp = setup_repo(
        "[project]\nname=\"x\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
    );
    let resolved = resolve_venv_path(tmp.path()).unwrap();
    assert_eq!(resolved.venv_dir, tmp.path().join("tools").join(".venv"));
    assert!(resolved.repo_key.is_empty());
}

/// Setting `TOOLR_VENV_LOCATION=in-tree` flips the resolution to
/// `tools/.venv` even when `pyproject.toml` requests `cache`. The
/// inverse direction (env says `cache`, file says `in-tree`) is
/// also covered to prove the override is bidirectional rather
/// than a one-way "force in-tree" flag.
#[test]
fn env_var_overrides_pyproject_venv_location() {
    let mut env = EnvGuard::write();

    let tmp = setup_repo("[project]\nname=\"x\"\nversion=\"0\"\n");
    env.set(VENV_LOCATION_ENV, "in-tree");
    let resolved = resolve_venv_path(tmp.path()).unwrap();
    assert_eq!(resolved.venv_dir, tmp.path().join("tools").join(".venv"));

    let tmp_in = setup_repo(
        "[project]\nname=\"x\"\nversion=\"0\"\n\n[tool.toolr]\nvenv-location = \"in-tree\"\n",
    );
    env.set(VENV_LOCATION_ENV, "cache");
    let resolved_cache = resolve_venv_path(tmp_in.path()).unwrap();
    // Cache mode lands under the toolr cache dir, not in-tree.
    assert!(!resolved_cache.venv_dir.starts_with(tmp_in.path()));
    assert!(!resolved_cache.repo_key.is_empty());
}

#[test]
fn env_var_invalid_value_is_an_error() {
    let mut env = EnvGuard::write();
    let tmp = setup_repo("[project]\nname=\"x\"\nversion=\"0\"\n");
    env.set(VENV_LOCATION_ENV, "bogus");
    let err = resolve_venv_path(tmp.path()).unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("TOOLR_VENV_LOCATION"), "got: {msg}");
    assert!(msg.contains("bogus"), "got: {msg}");
}
