use std::path::{Path, PathBuf};

use super::*;

const CARGO_TOML: &str =
    "[workspace.package]\nrepository = \"https://github.com/s0undt3ch/toolr\"\n";
const PYPROJECT: &str = "[project.urls]\nDocumentation = \"https://toolr.readthedocs.io\"\n";

/// A throwaway repo root holding this repo's URL metadata, then `files`
/// (repo-relative path, body), which may override it.
fn temp_repo(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("xtask-self-contained-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let meta = [
        ("Cargo.toml", CARGO_TOML),
        ("crates/toolr/pyproject.toml", PYPROJECT),
    ];
    for (rel, body) in meta.iter().chain(files) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let git = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(git.success());
    root
}

fn lint_repo(tag: &str, files: &[(&str, &str)]) -> Result<()> {
    let root = temp_repo(tag, files);
    let out = lint(&root);
    std::fs::remove_dir_all(&root).unwrap();
    out
}

fn urls() -> Vec<String> {
    [
        "github.com/s0undt3ch/toolr",
        "raw.githubusercontent.com/s0undt3ch/toolr",
        "toolr.readthedocs.io",
    ]
    .map(String::from)
    .to_vec()
}

fn rules_in(rel: &str, body: &str, paths: &'static [&'static str]) -> Vec<&'static str> {
    lint_file(
        Path::new("s"),
        Path::new(rel),
        body,
        &exists_in(paths),
        &urls(),
    )
    .into_iter()
    .map(|v| v.rule)
    .collect()
}

fn exists_in(paths: &'static [&'static str]) -> impl Fn(&Path) -> bool {
    move |p: &Path| paths.iter().any(|x| Path::new(x) == p)
}

fn rules(body: &str, paths: &'static [&'static str]) -> Vec<&'static str> {
    lint_file(
        Path::new("s"),
        Path::new("SKILL.md"),
        body,
        &exists_in(paths),
        &urls(),
    )
    .into_iter()
    .map(|v| v.rule)
    .collect()
}

#[test]
fn relative_link_escaping_skill_is_flagged() {
    assert_eq!(rules("[a](../../docs/x.md)\n", &[]), ["link-escapes-skill"]);
}

#[test]
fn link_to_own_reference_passes() {
    assert!(rules("[a](references/x.md)\n", &["s/references/x.md"]).is_empty());
}

#[test]
fn link_to_missing_own_file_is_flagged() {
    assert_eq!(
        rules("[a](references/nope.md)\n", &[]),
        ["link-escapes-skill"]
    );
}

#[test]
fn repo_and_docs_urls_are_flagged_case_insensitively() {
    let body = "[a](https://github.com/s0undt3ch/ToolR/tree/main/skills/x)\nhttps://toolr.readthedocs.io/latest/\n";
    assert_eq!(rules(body, &[]), ["repo-url", "repo-url"]);
}

#[test]
fn third_party_urls_pass() {
    assert!(rules("[t](https://github.com/Canop/termimad)\n", &[]).is_empty());
}

#[test]
fn backticked_repo_path_outside_skill_is_flagged() {
    assert_eq!(rules("See `docs/project-config.md`.\n", &[]), ["repo-path"]);
}

#[test]
fn backticked_own_example_and_user_paths_pass() {
    let body = "`examples/tools/greet.py`, `tools/ci.py`, `.github/workflows/ci.yml`\n";
    assert!(rules(body, &["s/examples/tools/greet.py"]).is_empty());
}

#[test]
fn fenced_code_is_ignored() {
    assert!(rules("```\n[a](../../x.md) `docs/x`\n```\n", &[]).is_empty());
}

#[test]
fn wrapped_link_reports_its_own_line() {
    let body = "Intro.\n\nSome [wrapped\ntext](../out.md) here.\n";
    let v = lint_file(
        Path::new("s"),
        Path::new("SKILL.md"),
        body,
        &exists_in(&[]),
        &urls(),
    );
    assert_eq!((v[0].rule, v[0].line), ("link-escapes-skill", 3));
}

#[test]
fn reference_links_resolve_through_their_definitions() {
    let body = "[a][docs] and [b][] and [c][nowhere]\n\n[docs]: https://toolr.readthedocs.io/x\n[b]: references/x.md\n";
    assert_eq!(rules(body, &[]), ["repo-url", "link-escapes-skill"]);
}

#[test]
fn unused_definition_is_still_linted() {
    assert_eq!(rules("[x]: ../../docs/x.md\n", &[]), ["link-escapes-skill"]);
}

#[test]
fn malformed_markdown_is_reported_not_swallowed() {
    assert_eq!(rules("a ](b)\n", &[]), ["malformed-markdown"]);
}

#[test]
fn malformed_markdown_reports_the_offending_line() {
    let body = "Intro.\n\nfirst\nsecond\nthird ](b)\n";
    let v = lint_file(
        Path::new("s"),
        Path::new("SKILL.md"),
        body,
        &exists_in(&[]),
        &urls(),
    );
    assert_eq!((v[0].rule, v[0].line), ("malformed-markdown", 5));
    assert_eq!(v[0].target, "`](` without a matching `[`");
}

#[test]
fn repo_url_in_inline_code_is_ignored() {
    assert!(rules("`https://github.com/s0undt3ch/ToolR`\n", &[]).is_empty());
}

#[test]
fn repo_url_in_example_python_is_flagged() {
    let err = lint_repo(
        "py",
        &[
            ("skills/x/SKILL.md", "ok\n"),
            (
                "skills/x/examples/tools/a.py",
                "# ok\n# see https://github.com/s0undt3ch/ToolR/x\n",
            ),
        ],
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains(
            "skills/x/examples/tools/a.py:2: repo-url: https://github.com/s0undt3ch/ToolR/x"
        ),
        "{err}"
    );
}

#[test]
fn crlf_skill_files_lint_identically_to_lf() {
    let md = "Intro.\n\nSome [wrapped\ntext](../out.md) here.\n\n```\n`docs/x`\n```\nRead `docs/y.md`.\n";
    let py = "# ok\n# see https://github.com/s0undt3ch/ToolR/x\n";
    let run = |tag: &str, eol: &str| {
        lint_repo(
            tag,
            &[
                ("skills/x/SKILL.md", &md.replace('\n', eol)),
                ("skills/x/examples/a.py", &py.replace('\n', eol)),
            ],
        )
        .unwrap_err()
        .to_string()
    };
    let lf = run("eol-lf", "\n");
    assert!(lf.contains("SKILL.md:3: link-escapes-skill"), "{lf}");
    assert_eq!(run("eol-crlf", "\r\n").replace("eol-crlf", "eol-lf"), lf);
    assert_eq!(run("eol-cr", "\r").replace("eol-cr", "eol-lf"), lf);
}

#[test]
fn sibling_markdown_is_linted() {
    let err = lint_repo(
        "sibling",
        &[
            ("skills/x/SKILL.md", "ok\n"),
            ("skills/x/workflow.md", "Read `docs/x.md`.\n"),
        ],
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("skills/x/workflow.md:1: repo-path: docs/x.md"),
        "{err}"
    );
}

#[test]
fn readme_review_tests_and_binary_files_are_skipped() {
    let bad = "[a](../../docs/x.md) https://toolr.readthedocs.io/\n";
    lint_repo(
        "skipped",
        &[
            ("skills/x/SKILL.md", "ok\n"),
            ("skills/x/README.md", bad),
            ("skills/x/REVIEW.md", bad),
            ("skills/x/tests/t.md", bad),
            ("skills/x/examples/README.md", bad),
            (
                "skills/x/examples/blob.bin",
                "\0\0https://toolr.readthedocs.io/\n",
            ),
        ],
    )
    .unwrap();
}

#[test]
fn nested_tests_directory_ships_and_is_linted() {
    let err = lint_repo(
        "nested-tests",
        &[
            ("skills/x/SKILL.md", "ok\n"),
            (
                "skills/x/examples/tests/t.py",
                "# https://toolr.readthedocs.io/\n",
            ),
        ],
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("skills/x/examples/tests/t.py:1: repo-url"),
        "{err}"
    );
}

#[test]
fn gitignored_files_do_not_ship_and_are_not_linted() {
    let bad = "[a](../../docs/x.md) https://toolr.readthedocs.io/\n";
    lint_repo(
        "ignored",
        &[
            (".gitignore", ".venv/\n"),
            ("skills/x/SKILL.md", "ok\n"),
            ("skills/x/examples/.venv/lib/x.md", bad),
        ],
    )
    .unwrap();
}

#[test]
fn tracked_file_deleted_from_the_worktree_is_skipped() {
    let root = temp_repo(
        "deleted",
        &[
            ("skills/x/SKILL.md", "ok\n"),
            ("skills/x/gone.md", "[a](../../docs/x.md)\n"),
        ],
    );
    let added = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(&root)
        .status()
        .unwrap();
    assert!(added.success());
    std::fs::remove_file(root.join("skills/x/gone.md")).unwrap();
    let out = lint(&root);
    std::fs::remove_dir_all(&root).unwrap();
    out.unwrap();
}

#[cfg(unix)]
#[test]
fn symlinks_are_flagged_not_followed() {
    let root = temp_repo(
        "symlink",
        &[
            ("skills/x/SKILL.md", "ok\n"),
            ("outside/leak.md", "[a](../../docs/x.md)\n"),
        ],
    );
    let skill = root.join("skills/x");
    std::os::unix::fs::symlink("../../outside", skill.join("outside")).unwrap();
    std::os::unix::fs::symlink(".", skill.join("loop")).unwrap();
    let err = lint(&root).unwrap_err().to_string();
    std::fs::remove_dir_all(&root).unwrap();
    assert!(err.contains("skills/x/loop:1: symlink: ."), "{err}");
    assert!(
        err.contains("skills/x/outside:1: symlink: ../../outside"),
        "{err}"
    );
    assert!(!err.contains("leak.md"), "{err}");
}

#[test]
fn dot_slash_repo_path_is_flagged() {
    assert_eq!(rules("`./docs/x.md`\n", &[]), ["repo-path"]);
    assert!(rules("`./examples/a.py`\n", &["s/examples/a.py"]).is_empty());
}

#[test]
fn parent_relative_backticked_path_escaping_skill_is_flagged() {
    assert_eq!(
        rules_in("references/r.md", "`../../docs/x.md`\n", &[]),
        ["repo-path"]
    );
    assert!(
        rules_in(
            "references/r.md",
            "`../examples/a.py`\n",
            &["s/examples/a.py"]
        )
        .is_empty()
    );
}

#[test]
fn parent_relative_backticked_path_missing_inside_skill_is_flagged() {
    assert_eq!(
        rules_in("references/r.md", "`../examples/nope.py`\n", &[]),
        ["repo-path"]
    );
}

#[test]
fn only_the_first_backticked_token_is_checked() {
    assert!(rules("`cat docs/x.md`\n", &[]).is_empty());
}

#[test]
fn file_scheme_links_are_flagged() {
    assert_eq!(
        rules("[a](file:///etc/x.md)\n", &[]),
        ["link-escapes-skill"]
    );
    assert_eq!(
        rules("[a](FILE:x.md)\n", &["s/x.md"]),
        ["link-escapes-skill"]
    );
}

#[test]
fn raw_html_href_and_src_are_linted() {
    let body =
        "<a href=\"../../docs/x.md\">x</a>\n<img src='https://toolr.readthedocs.io/a.png'>\n";
    assert_eq!(rules(body, &[]), ["link-escapes-skill", "repo-url"]);
}

#[test]
fn raw_html_to_own_file_or_in_code_passes() {
    assert!(
        rules(
            "<a href=\"references/x.md\">x</a>\n",
            &["s/references/x.md"]
        )
        .is_empty()
    );
    assert!(rules("`<a href=\"../../x.md\">`\n", &[]).is_empty());
    assert!(rules("```html\n<a href=\"../../x.md\">\n```\n", &[]).is_empty());
}

#[test]
fn loose_files_directly_under_skills_are_not_a_skill() {
    lint_repo(
        "loose",
        &[
            ("skills/x/SKILL.md", "ok\n"),
            ("skills/notes.md", "[a](../../docs/x.md)\n"),
        ],
    )
    .unwrap();
}

#[test]
fn non_utf8_files_are_skipped() {
    let root = temp_repo("non-utf8", &[("skills/x/SKILL.md", "ok\n")]);
    std::fs::create_dir_all(root.join("skills/x/examples")).unwrap();
    std::fs::write(
        root.join("skills/x/examples/latin1.txt"),
        b"\xff\xfe https://toolr.readthedocs.io/\n",
    )
    .unwrap();
    let out = lint(&root);
    std::fs::remove_dir_all(&root).unwrap();
    out.unwrap();
}

#[test]
fn unmatched_backtick_run_does_not_hide_a_later_code_span() {
    assert_eq!(rules("``unclosed `docs/x.md`\n", &[]), ["repo-path"]);
}

#[test]
fn anchor_mailto_and_empty_targets_pass() {
    assert!(rules("[a](#x) [b](mailto:a@b.c) [c]()\n", &[]).is_empty());
}

#[test]
fn dot_slash_link_to_own_file_passes() {
    assert!(rules("[a](./references/x.md)\n", &["s/references/x.md"]).is_empty());
}

#[test]
fn absolute_path_link_is_flagged() {
    assert_eq!(rules("[a](/etc/x.md)\n", &[]), ["link-escapes-skill"]);
}

#[test]
fn prefixed_href_and_src_attributes_are_linted() {
    let body = "<a data-href=\"../../x.md\">x</a>\n<img data-src=../../y.png>\n<use xlink:href='../../z.svg'/>\n";
    assert_eq!(rules(body, &[]), ["link-escapes-skill"; 3]);
}

#[test]
fn srcset_and_names_merely_ending_in_href_are_not_targets() {
    assert!(
        rules(
            "<img srcset=\"../../a.png 2x\"> <a xhref=\"../../x.md\">\n",
            &[]
        )
        .is_empty()
    );
}

#[test]
fn unquoted_html_attribute_values_are_linted() {
    let body =
        "<a href=../../docs/x.md>x</a>\n<img src=https://toolr.readthedocs.io/a.png alt=x>\n";
    assert_eq!(rules(body, &[]), ["link-escapes-skill", "repo-url"]);
    assert!(rules("<a href=>x</a>\n", &[]).is_empty());
}

#[test]
fn unterminated_quoted_attribute_is_not_a_target() {
    assert!(rules("<a href=\"../../x.md\n", &[]).is_empty());
}

#[test]
fn http_prefixed_words_do_not_stop_the_url_scan() {
    assert_eq!(
        rules("httpx and https://toolr.readthedocs.io/x\n", &[]),
        ["repo-url"]
    );
}

fn urls_for(tag: &str, files: &[(&str, &str)]) -> Result<Vec<String>> {
    let root = temp_repo(tag, files);
    let out = repo_urls(&root);
    std::fs::remove_dir_all(&root).unwrap();
    out
}

#[test]
fn repo_urls_derive_from_this_repo_metadata() {
    assert_eq!(
        repo_urls(&super::super::repo_root().unwrap()).unwrap(),
        urls()
    );
}

#[test]
fn repo_urls_follow_the_project_metadata() {
    let got = urls_for(
        "urls-other",
        &[
            (
                "Cargo.toml",
                "[workspace.package]\nrepository = \"https://www.GitHub.com/Acme/Widget.git/\"\n",
            ),
            (
                "crates/toolr/pyproject.toml",
                "[project.urls]\nDocumentation = \"https://Docs.Acme.dev/widget/\"\n",
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        got,
        [
            "github.com/acme/widget",
            "raw.githubusercontent.com/acme/widget",
            "docs.acme.dev"
        ]
    );
}

#[test]
fn lint_flags_the_urls_the_metadata_names() {
    let err = lint_repo(
        "urls-lint",
        &[
            (
                "Cargo.toml",
                "[workspace.package]\nrepository = \"https://github.com/acme/widget\"\n",
            ),
            (
                "skills/x/SKILL.md",
                "https://github.com/acme/widget/x https://github.com/s0undt3ch/toolr\n",
            ),
        ],
    )
    .unwrap_err()
    .to_string();
    assert!(
        err.contains("repo-url: https://github.com/acme/widget/x"),
        "{err}"
    );
    assert!(!err.contains("s0undt3ch/toolr"), "{err}");
}

#[test]
fn repo_urls_ignore_a_fragment_or_query_in_the_metadata() {
    let got = urls_for(
        "urls-fragment",
        &[
            (
                "Cargo.toml",
                "[workspace.package]\nrepository = \"https://github.com/acme/widget#readme\"\n",
            ),
            (
                "crates/toolr/pyproject.toml",
                "[project.urls]\nDocumentation = \"https://docs.acme.dev?v=1\"\n",
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        got,
        [
            "github.com/acme/widget",
            "raw.githubusercontent.com/acme/widget",
            "docs.acme.dev"
        ]
    );
}

#[test]
fn repo_urls_match_on_a_path_segment_boundary() {
    let flagged = "https://github.com/s0undt3ch/ToolR/x https://github.com/s0undt3ch/toolr#f \
        https://github.com/s0undt3ch/ToolR.git https://toolr.readthedocs.io:443/x \
        https://raw.githubusercontent.com/s0undt3ch/ToolR/main/x\n";
    assert_eq!(rules(flagged, &[]), ["repo-url"; 5]);
    let other = "https://github.com/s0undt3ch/toolr-other https://toolr.readthedocs.io.evil.com/ \
        https://raw.githubusercontent.com/s0undt3ch/other/main/x\n";
    assert!(rules(other, &[]).is_empty());
}

#[test]
fn missing_repository_key_is_an_error_naming_file_and_key() {
    let err = format!(
        "{:#}",
        urls_for(
            "urls-no-repo",
            &[("Cargo.toml", "[workspace.package]\nversion = \"1\"\n")]
        )
        .unwrap_err()
    );
    assert!(
        err.contains("Cargo.toml: no string `workspace.package.repository`"),
        "{err}"
    );
}

#[test]
fn missing_documentation_url_is_an_error_naming_file_and_key() {
    let err = format!(
        "{:#}",
        urls_for(
            "urls-no-docs",
            &[("crates/toolr/pyproject.toml", "[project]\nname = \"x\"\n")]
        )
        .unwrap_err()
    );
    assert!(
        err.contains("crates/toolr/pyproject.toml: no string `project.urls.Documentation`"),
        "{err}"
    );
}

#[test]
fn missing_or_invalid_metadata_file_is_an_error() {
    let root = temp_repo("urls-no-file", &[]);
    std::fs::remove_file(root.join("Cargo.toml")).unwrap();
    let missing = format!("{:#}", repo_urls(&root).unwrap_err());
    std::fs::remove_dir_all(&root).unwrap();
    assert!(
        missing.contains("reading ") && missing.contains("Cargo.toml"),
        "{missing}"
    );

    let bad = format!(
        "{:#}",
        urls_for("urls-bad-toml", &[("Cargo.toml", "[x\n")]).unwrap_err()
    );
    assert!(bad.contains("parsing Cargo.toml"), "{bad}");
}

#[test]
fn repository_that_is_not_a_github_repo_url_is_an_error() {
    for url in [
        "https://gitlab.com/a/b",
        "https://github.com/a",
        "https://github.com/a/b/c",
    ] {
        let cargo = format!("[workspace.package]\nrepository = \"{url}\"\n");
        let err = format!(
            "{:#}",
            urls_for("urls-not-gh", &[("Cargo.toml", &cargo)]).unwrap_err()
        );
        assert!(
            err.contains(
                "Cargo.toml: `workspace.package.repository` is not a github.com/<owner>/<repo> URL"
            ),
            "{url}: {err}"
        );
    }
}

#[test]
fn documentation_url_without_a_host_is_an_error() {
    let err = format!(
        "{:#}",
        urls_for(
            "urls-no-host",
            &[(
                "crates/toolr/pyproject.toml",
                "[project.urls]\nDocumentation = \"https:///x\"\n"
            )]
        )
        .unwrap_err()
    );
    assert!(
        err.contains("crates/toolr/pyproject.toml: `project.urls.Documentation` has no host"),
        "{err}"
    );
}
