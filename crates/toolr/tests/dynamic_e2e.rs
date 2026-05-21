//! End-to-end: static + dynamic layers merged into a single manifest.

#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use toolr_core::dynamic::rebuild_manifest_full;
use toolr_core::manifest::{Origin, load_manifest};
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn make_fake_python(at: &Path, payload: &str) {
    let mut f = std::fs::File::create(at).unwrap();
    writeln!(f, "#!/bin/sh").unwrap();
    writeln!(f, "cat <<'__EOF__'").unwrap();
    writeln!(f, "{payload}").unwrap();
    writeln!(f, "__EOF__").unwrap();
    drop(f);
    let mut perms = std::fs::metadata(at).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(at, perms).unwrap();
}

#[test]
fn full_rebuild_merges_static_and_dynamic_entries() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path();

    // ---- Static side: a tools/ci.py the static parser can see.
    write(
        &project.join("tools").join("ci.py"),
        "\"\"\"CI utilities.\"\"\"\ngroup = command_group(\"ci\", \"CI utilities\")\n@group.command\ndef hello(ctx):\n    \"\"\"Say hello.\"\"\"\n    pass\n",
    );

    // ---- Dynamic side: a fake-python that emits a payload announcing
    //      one legacy group + command, plus a conflict on (ci, hello)
    //      that must lose to the static entry.
    let py = project.join("python");
    make_fake_python(
        &py,
        r#"{"payload_schema_version":1,"groups":[{"name":"legacy","title":"Legacy","description":"","origin":"static"},{"name":"ci","title":"FROM DYNAMIC","description":"","origin":"static"}],"commands":[{"name":"widget","group":"legacy","module":"third","function":"widget","summary":"Widget.","description":"","arguments":[],"imports":[],"origin":"static"},{"name":"hello","group":"ci","module":"FROM DYNAMIC","function":"hello","summary":"FROM DYNAMIC","description":"","arguments":[],"imports":[],"origin":"static"}],"warnings":["module foo failed: bar"]}"#,
    );

    // ---- Fake venv with a toolr-manifest.json so compute_third_party_hash
    //      hashes a non-empty input and third_party_hash is non-empty.
    let venv = project.join("venv");
    write(
        &venv.join("lib/python3.13/site-packages/some-pkg/toolr-manifest.json"),
        "{}",
    );

    let outcome = rebuild_manifest_full(project, &py, &venv).expect("rebuild");

    assert!(outcome.manifest_path.is_file());
    assert_eq!(outcome.warnings.len(), 1);

    let m = load_manifest(&outcome.manifest_path).unwrap();
    // Both groups present.
    let by_name: std::collections::HashMap<_, _> =
        m.groups.iter().map(|g| (g.name.clone(), g)).collect();
    assert_eq!(by_name.len(), 2);
    assert_eq!(by_name["ci"].origin, Origin::Static);
    assert_eq!(by_name["legacy"].origin, Origin::Dynamic);
    // Conflict resolution: static `ci.hello` survived.
    let hello = m
        .commands
        .iter()
        .find(|c| c.group == "ci" && c.name == "hello")
        .expect("hello present");
    assert_eq!(hello.origin, Origin::Static);
    assert_ne!(hello.module, "FROM DYNAMIC");
    // Dynamic legacy.widget came through.
    let widget = m
        .commands
        .iter()
        .find(|c| c.group == "legacy" && c.name == "widget")
        .expect("widget present");
    assert_eq!(widget.origin, Origin::Dynamic);
    // Dynamic hash stamped.
    assert!(!m.third_party_hash.is_empty());
}
