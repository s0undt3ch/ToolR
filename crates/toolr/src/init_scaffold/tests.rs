use super::*;
use crate::init_templates::VenvLocation;
use tempfile::TempDir;

fn opts() -> ScaffoldOptions {
    ScaffoldOptions {
        requires_python: ">=3.11".into(),
        venv_location: VenvLocation::Cache,
        include_example: true,
    }
}

#[test]
fn scaffold_writes_three_files() {
    let tmp = TempDir::new().unwrap();
    let outcome = scaffold(tmp.path(), &opts(), false).unwrap();
    assert_eq!(outcome.tools_dir, tmp.path().join("tools"));
    assert_eq!(outcome.files_written.len(), 3);
    assert!(tmp.path().join("tools/pyproject.toml").is_file());
    assert!(tmp.path().join("tools/.gitignore").is_file());
    assert!(tmp.path().join("tools/example.py").is_file());
}

#[test]
fn scaffold_without_example_writes_two_files() {
    let tmp = TempDir::new().unwrap();
    let mut o = opts();
    o.include_example = false;
    let outcome = scaffold(tmp.path(), &o, false).unwrap();
    assert_eq!(outcome.files_written.len(), 2);
    assert!(!tmp.path().join("tools/example.py").exists());
}

#[test]
fn scaffold_writes_alongside_unrelated_files() {
    // Unrelated files in tools/ are not scaffold files → no conflict,
    // scaffold proceeds and the unrelated file is left untouched.
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("my_tool.py"), "# custom").unwrap();

    let outcome = scaffold(tmp.path(), &opts(), false).unwrap();
    assert_eq!(outcome.files_written.len(), 3);
    assert_eq!(
        fs::read_to_string(tools.join("my_tool.py")).unwrap(),
        "# custom"
    );
}

#[test]
fn scaffold_returns_conflict_error_without_force() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("pyproject.toml"), "# stale").unwrap();

    let err = scaffold(tmp.path(), &opts(), false).expect_err("should conflict");
    let conflicts = err
        .downcast_ref::<ScaffoldConflictsError>()
        .expect("should be ScaffoldConflictsError");
    assert_eq!(conflicts.files.len(), 1);
    assert!(conflicts.files[0].ends_with("pyproject.toml"));
    // Existing content must be preserved.
    assert_eq!(
        fs::read_to_string(tools.join("pyproject.toml")).unwrap(),
        "# stale"
    );
}

#[test]
fn scaffold_skips_identical_files() {
    let tmp = TempDir::new().unwrap();
    // Write the scaffold once.
    scaffold(tmp.path(), &opts(), false).unwrap();
    // Run again — all files identical, nothing written.
    let outcome = scaffold(tmp.path(), &opts(), false).unwrap();
    assert_eq!(outcome.files_written.len(), 0);
}

#[test]
fn scaffold_force_overwrites() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("pyproject.toml"), "# stale").unwrap();

    scaffold(tmp.path(), &opts(), true).unwrap();
    let pyproject = fs::read_to_string(tools.join("pyproject.toml")).unwrap();
    assert!(pyproject.contains(r#"name = "tools""#));
}

#[test]
fn scaffold_accepts_an_empty_tools_dir() {
    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("tools")).unwrap();
    scaffold(tmp.path(), &opts(), false).unwrap();
    assert!(tmp.path().join("tools/pyproject.toml").is_file());
}

#[test]
fn execute_scaffold_respects_overwrite_set() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("pyproject.toml"), "# stale").unwrap();
    fs::write(tools.join(".gitignore"), "# stale").unwrap();

    let analysis = analyze_scaffold(tmp.path(), &opts()).unwrap();
    // Approve only pyproject.toml.
    let overwrite: HashSet<PathBuf> = [tools.join("pyproject.toml")].into();
    let outcome = execute_scaffold(&analysis, &overwrite).unwrap();

    // pyproject.toml overwritten, .gitignore skipped (conflict but not approved).
    assert!(
        outcome
            .files_written
            .iter()
            .any(|p| p.ends_with("pyproject.toml"))
    );
    assert!(
        !outcome
            .files_written
            .iter()
            .any(|p| p.ends_with(".gitignore"))
    );
    assert_eq!(
        fs::read_to_string(tools.join(".gitignore")).unwrap(),
        "# stale"
    );
}

#[test]
fn execute_scaffold_restores_conflict_originals_on_rollback() {
    // Build a real analysis for a single-file case so we can intercept
    // the failure by having one file succeed and then simulating a disk
    // full via a read-only directory on the second write.
    //
    // Simpler approach: use a two-conflict scenario where we approve both,
    // but make the second destination a read-only file so write_file fails,
    // then assert the first destination is restored to its original.
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir(&tools).unwrap();

    // Pre-populate both scaffold conflict files with known "original" content.
    let pyproject_original = "# pyproject original";
    let gitignore_original = "# gitignore original";
    fs::write(tools.join("pyproject.toml"), pyproject_original).unwrap();
    fs::write(tools.join(".gitignore"), gitignore_original).unwrap();

    let analysis = analyze_scaffold(tmp.path(), &opts()).unwrap();
    assert_eq!(
        analysis.conflict_files().len(),
        2,
        "expected both files to be conflicts"
    );

    // Make the tools dir itself read-only so that writing example.py
    // (a New file) fails. This triggers the rollback after pyproject.toml
    // and .gitignore have already been overwritten.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tools).unwrap().permissions();
        perms.set_mode(0o555); // read+execute, no write
        fs::set_permissions(&tools, perms).unwrap();

        let overwrite: HashSet<PathBuf> = analysis
            .conflict_files()
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let result = execute_scaffold(&analysis, &overwrite);

        // Restore permissions so TempDir can clean up.
        let mut perms = fs::metadata(&tools).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&tools, perms).unwrap();

        assert!(result.is_err(), "expected write to fail");
        // Originals must be restored, not deleted.
        assert_eq!(
            fs::read_to_string(tools.join("pyproject.toml")).unwrap(),
            pyproject_original,
            "pyproject.toml must be restored"
        );
        assert_eq!(
            fs::read_to_string(tools.join(".gitignore")).unwrap(),
            gitignore_original,
            ".gitignore must be restored"
        );
    }
    // Non-Unix: just assert the happy path so the test still compiles.
    #[cfg(not(unix))]
    {
        let _ = analysis;
    }
}

#[test]
fn scaffold_conflicts_error_display() {
    let err = ScaffoldConflictsError {
        files: vec![
            PathBuf::from("tools/pyproject.toml"),
            PathBuf::from("tools/.gitignore"),
        ],
    };
    let msg = err.to_string();
    assert!(msg.contains("would be overwritten"), "msg: {msg}");
    assert!(msg.contains("pyproject.toml"), "msg: {msg}");
    assert!(msg.contains(".gitignore"), "msg: {msg}");
}

#[test]
fn file_status_classify_new_identical_conflict() {
    let tmp = TempDir::new().unwrap();
    let tools = tmp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    fs::write(tools.join("pyproject.toml"), "# stale").unwrap();

    // Write the scaffold to get the real .gitignore content.
    let first = scaffold(tmp.path(), &opts(), true).unwrap();
    let gitignore_path = first
        .files_written
        .iter()
        .find(|p| p.ends_with(".gitignore"))
        .expect("gitignore must be written");
    let gitignore_content = fs::read_to_string(gitignore_path).unwrap();

    // Now put stale content back into pyproject.toml and keep .gitignore identical.
    fs::write(tools.join("pyproject.toml"), "# stale again").unwrap();
    // Delete example.py so it is "New" on the next analysis.
    fs::remove_file(tools.join("example.py")).unwrap();

    let analysis = analyze_scaffold(tmp.path(), &opts()).unwrap();
    let pyproject = analysis
        .files
        .iter()
        .find(|f| f.dest.ends_with("pyproject.toml"))
        .unwrap();
    let gitignore = analysis
        .files
        .iter()
        .find(|f| f.dest.ends_with(".gitignore"))
        .unwrap();
    let example = analysis
        .files
        .iter()
        .find(|f| f.dest.ends_with("example.py"))
        .unwrap();

    assert_eq!(pyproject.status, FileStatus::Conflict);
    assert_eq!(
        gitignore.status,
        FileStatus::Identical,
        "content: {gitignore_content:?}"
    );
    assert_eq!(example.status, FileStatus::New);
}

#[test]
#[cfg(unix)]
fn execute_scaffold_rejects_symlinked_tools_dir() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().unwrap();
    let real_dir = tmp.path().join("real_tools");
    let link = tmp.path().join("tools");
    fs::create_dir(&real_dir).unwrap();
    symlink(&real_dir, &link).unwrap();

    let analysis = analyze_scaffold(tmp.path(), &opts()).unwrap();
    let result = execute_scaffold(&analysis, &HashSet::new());
    assert!(result.is_err(), "expected error for symlinked tools/");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("symlink"),
        "error should mention symlink: {msg}"
    );
}

#[test]
fn tmp_filename_includes_pid() {
    let path = std::path::Path::new("/some/dir/pyproject.toml");
    let tmp = with_extension(path, "tmp");
    let name = tmp.file_name().unwrap().to_string_lossy();
    assert!(
        name.starts_with("pyproject.toml.tmp."),
        "unexpected name: {name}"
    );
    let pid_str = name.strip_prefix("pyproject.toml.tmp.").unwrap();
    assert!(
        pid_str.parse::<u32>().is_ok(),
        "suffix must be a PID: {pid_str}"
    );
}
