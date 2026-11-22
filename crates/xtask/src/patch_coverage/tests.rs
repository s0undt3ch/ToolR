use super::*;

const DIFF: &str = "\
diff --git a/crates/demo/src/lib.rs b/crates/demo/src/lib.rs
--- a/crates/demo/src/lib.rs
+++ b/crates/demo/src/lib.rs
@@ -1,0 +2,3 @@ fn a()
+x
+y
+z
@@ -10 +12 @@
-old
+new
@@ -20,2 +21,0 @@
-gone
-gone
diff --git a/crates/demo/src/old.rs b/crates/demo/src/new.rs
rename from crates/demo/src/old.rs
rename to crates/demo/src/new.rs
--- a/crates/demo/src/old.rs
+++ b/crates/demo/src/new.rs
@@ -5 +5,2 @@
-a
+b
+c
diff --git a/crates/demo/src/added.rs b/crates/demo/src/added.rs
--- /dev/null
+++ b/crates/demo/src/added.rs
@@ -0,0 +1,2 @@
+one
+two
diff --git a/crates/demo/src/removed.rs b/crates/demo/src/removed.rs
--- a/crates/demo/src/removed.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-one
-two
diff --git a/crates/toolr-py/src/lib.rs b/crates/toolr-py/src/lib.rs
--- a/crates/toolr-py/src/lib.rs
+++ b/crates/toolr-py/src/lib.rs
@@ -1 +1 @@
+ignored
diff --git a/crates/toolr-core/src/complete/scripts/x.rs b/crates/toolr-core/src/complete/scripts/x.rs
--- a/crates/toolr-core/src/complete/scripts/x.rs
+++ b/crates/toolr-core/src/complete/scripts/x.rs
@@ -1 +1 @@
+ignored
diff --git a/crates/demo/src/lib/tests.rs b/crates/demo/src/lib/tests.rs
--- a/crates/demo/src/lib/tests.rs
+++ b/crates/demo/src/lib/tests.rs
@@ -1 +1 @@
+unit tests
diff --git a/crates/demo/src/lib/case_test.rs b/crates/demo/src/lib/case_test.rs
--- a/crates/demo/src/lib/case_test.rs
+++ b/crates/demo/src/lib/case_test.rs
@@ -1 +1 @@
+included tests
diff --git a/crates/demo/tests/t.rs b/crates/demo/tests/t.rs
--- a/crates/demo/tests/t.rs
+++ b/crates/demo/tests/t.rs
@@ -1 +1 @@
+not src
diff --git a/crates/demo/src/notes.md b/crates/demo/src/notes.md
--- a/crates/demo/src/notes.md
+++ b/crates/demo/src/notes.md
@@ -1 +1 @@
+not rust
";

fn set(lines: &[u32]) -> BTreeSet<u32> {
    lines.iter().copied().collect()
}

#[test]
fn parse_diff_handles_hunk_shapes_and_filters_paths() {
    let changed = parse_diff(DIFF);
    let expected: ChangedLines = [
        ("crates/demo/src/lib.rs", set(&[2, 3, 4, 12])),
        ("crates/demo/src/new.rs", set(&[5, 6])),
        ("crates/demo/src/added.rs", set(&[1, 2])),
    ]
    .into_iter()
    .map(|(p, l)| (p.to_owned(), l))
    .collect();
    assert_eq!(changed, expected);
}

#[test]
fn hunk_new_range_rejects_malformed_headers() {
    assert_eq!(hunk_new_range("@@ -1 +x,2 @@"), None);
    assert_eq!(hunk_new_range("@@ -1 +3,y @@"), None);
    assert_eq!(hunk_new_range("@@ nothing @@"), None);
    assert_eq!(hunk_new_range("@@ -1 +7 @@"), Some((7, 1)));
}

#[test]
fn parse_lcov_sums_duplicates_and_ignores_outside_root() {
    let root = Path::new("/ws");
    let text = "\
SF:/ws/crates/demo/src/lib.rs
DA:1,0
DA:2,3,checksum
DA:bad,1
end_of_record
SF:/elsewhere/crates/demo/src/lib.rs
DA:1,9
end_of_record
SF:/ws/crates/demo/src/lib.rs
DA:1,2
DA:2,0
LF:2
end_of_record
DA:5,1
";
    let hits = parse_lcov(text, root);
    assert_eq!(hits.len(), 1);
    let da = &hits["crates/demo/src/lib.rs"];
    assert_eq!(da[&1], 2);
    assert_eq!(da[&2], 3);
    assert_eq!(da.len(), 2);
}

#[test]
fn summarize_skips_uninstrumented_lines_and_reports_misses() {
    let changed: ChangedLines = [
        ("a.rs".to_owned(), set(&[1, 2, 3, 4])),
        ("unmeasured.rs".to_owned(), set(&[1])),
    ]
    .into_iter()
    .collect();
    let hits: LineHits = [("a.rs".to_owned(), [(1, 5), (2, 0), (4, 1)].into())]
        .into_iter()
        .collect();
    let summary = summarize(&changed, &hits);
    assert_eq!((summary.covered, summary.total), (2, 3));
    assert_eq!(summary.files[0].missed, vec![2]);
    assert_eq!(
        summary.render(),
        "a.rs: 2/3, missed lines [2]\npatch coverage: 2/3 = 66.7%\n"
    );
}

#[test]
fn empty_patch_is_fully_covered() {
    let summary = summarize(&ChangedLines::new(), &LineHits::new());
    assert_eq!(summary.percent(), 100.0);
    assert_eq!(summary.render(), "patch coverage: 0/0 = 100.0%\n");
}

#[test]
fn fully_covered_files_are_not_listed() {
    let changed: ChangedLines = [("a.rs".to_owned(), set(&[1]))].into_iter().collect();
    let hits: LineHits = [("a.rs".to_owned(), [(1, 1)].into())].into_iter().collect();
    assert_eq!(
        summarize(&changed, &hits).render(),
        "patch coverage: 1/1 = 100.0%\n"
    );
}
