//! `toolr __complete` hands path-typed slots to the shell: exit 10 asks for
//! file system paths, exit 11 for directories, both with empty stdout.

mod common;

use tempfile::TempDir;

/// A path-typed option with an empty value used to get no candidates at all.
fn fixture() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let tools = common::write_project(tmp.path());
    std::fs::write(
        tools.join("demo.py"),
        r#""""Demo."""
import pathlib
from typing import Literal

from toolr.types import DirectoryPath, FilePath, NewPath

group = command_group("demo", "Demo", docstring=__doc__)

@group.command
def hello(
    ctx,
    src: pathlib.Path,
    cfg: FilePath | None = None,
    out: DirectoryPath | None = None,
    new: NewPath | None = None,
    mode: Literal["a", "b"] = "a",
):
    """Say hello.

    Args:
        src: Source.
        cfg: Config file.
        out: Output directory.
        new: New file.
        mode: Mode.
    """
"#,
    )
    .unwrap();
    common::toolr()
        .current_dir(tmp.path())
        .arg("__build-static-manifest")
        .assert()
        .success();
    tmp
}

fn complete(tmp: &TempDir, args: &[&str]) -> (Option<i32>, String) {
    let out = common::toolr()
        .current_dir(tmp.path())
        .arg("__complete")
        .arg(tmp.path())
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn path_slots_exit_with_the_path_kind_and_print_nothing() {
    let tmp = fixture();
    for (args, code) in [
        (&["demo", "hello", "--cfg", ""][..], 10),
        (&["demo", "hello", ""], 10),
        (&["demo", "hello", "--new", ""], 11),
        (&["demo", "hello", "--out", ""], 11),
    ] {
        let (rc, stdout) = complete(&tmp, args);
        assert_eq!(rc, Some(code), "{args:?}");
        assert_eq!(stdout, "", "{args:?}");
    }
}

#[test]
fn literal_slot_still_prints_candidates() {
    let tmp = fixture();
    let (rc, stdout) = complete(&tmp, &["demo", "hello", "--mode", ""]);
    assert_eq!(rc, Some(0));
    assert_eq!(stdout, "a\t\nb\t\n");
}

#[test]
fn built_in_path_arguments_complete_paths() {
    let tmp = fixture();
    for (args, code) in [
        (&["project", "trust", ""][..], 10),
        (&["project", "untrust", ""], 10),
        (&["self", "build-manifest", "--source-dir", ""], 11),
        (&["self", "build-manifest", "--output", ""], 10),
    ] {
        let (rc, stdout) = complete(&tmp, args);
        assert_eq!(rc, Some(code), "{args:?}");
        assert_eq!(stdout, "", "{args:?}");
    }
}
