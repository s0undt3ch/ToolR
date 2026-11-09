//! The PowerShell completion script, driven through a real `TabExpansion2`: candidate values stay
//! inert, replies parse back to exactly the value, and the engine sees each word as PowerShell
//! would pass it. Every test runs under each PowerShell found: pwsh everywhere, plus Windows
//! PowerShell 5.1 on Windows.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use serde_json::{Value, json};
use tempfile::TempDir;

/// `S1` and `S2` stand for sentinel paths the fixture fills in.
const VALUES: [&str; 16] = [
    "dry run",
    "dry walk",
    "it's",
    "a\"b",
    "a`b",
    "$(New-Item S1)",
    "@(New-Item S2)",
    "a,b",
    "x;y",
    // The engine never offers a value starting with `-`, so `--%` itself can't be a candidate.
    "50%",
    "~root",
    "#c",
    "\u{2018}q\u{2019}",
    "\u{e9}t\u{e9}",
    "e\\f",
    "0x10",
];

struct Fixture {
    tmp: TempDir,
    /// Holds the printed script and the driver, outside the project.
    work: TempDir,
    sentinels: [PathBuf; 2],
}

impl Fixture {
    fn values(&self) -> Vec<String> {
        VALUES
            .iter()
            .map(|v| {
                v.replace("S1", self.sentinels[0].to_str().unwrap())
                    .replace("S2", self.sentinels[1].to_str().unwrap())
            })
            .collect()
    }

    fn assert_inert(&self) {
        for s in &self.sentinels {
            assert!(!s.exists(), "{} was created", s.display());
        }
    }
}

fn fixture() -> Fixture {
    let tmp = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();
    let sentinels = [
        tmp.path().join("sentinel-one"),
        tmp.path().join("sentinel-two"),
    ];
    common::write_project(tmp.path());
    std::fs::create_dir_all(tmp.path().join("files/sub")).unwrap();
    for name in ["files/with space.txt", "files/it's.txt"] {
        std::fs::write(tmp.path().join(name), "").unwrap();
    }
    // Names PowerShell's own path completion wildcard-escapes; `*` isn't allowed on Windows.
    for dir in ["odd/d[1]", "odd/e`f"] {
        std::fs::create_dir_all(tmp.path().join(dir)).unwrap();
    }
    for name in ["odd/a[1].txt", "odd/a`b.txt", "odd/a``c.txt"] {
        std::fs::write(tmp.path().join(name), "").unwrap();
    }
    if !cfg!(windows) {
        std::fs::write(tmp.path().join("odd/x*y.txt"), "").unwrap();
    }
    let fx = Fixture {
        tmp,
        work,
        sentinels,
    };
    let literal = fx
        .values()
        .iter()
        .map(|v| format!("{v:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let src = format!(
        r#""""CI utilities."""
import pathlib
from typing import Literal

from toolr.types import DirectoryPath

group = command_group("ci", "CI utilities", docstring=__doc__)

@group.command
def lint(
    ctx,
    mode: Literal[{literal}] = "dry run",
    cfg: pathlib.Path | None = None,
    out: DirectoryPath | None = None,
    verbose: bool = False,
):
    """Lint things.

    Args:
        mode: Lint mode.
        cfg: Config path.
        out: Output directory.
        verbose: Say more.
    """
    pass

@group.command
def pick(ctx, first: str, second: Literal["x1", "x2"]):
    """Pick things.

    Args:
        first: Anything.
        second: The second.
    """
    pass
"#
    );
    std::fs::write(fx.tmp.path().join("tools/ci.py"), src).unwrap();
    common::toolr()
        .current_dir(fx.tmp.path())
        .arg("__build-static-manifest")
        .assert()
        .success();
    let script = common::toolr()
        .args(["self", "completion", "print", "powershell"])
        .output()
        .unwrap();
    assert!(script.status.success());
    std::fs::write(fx.work.path().join("toolr.ps1"), script.stdout).unwrap();
    fx
}

/// pwsh when on `PATH`, plus Windows PowerShell on Windows. On CI a missing one fails the tests.
fn powershells() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut wanted = vec!["pwsh"];
    if cfg!(windows) {
        wanted.push("powershell");
    }
    for name in wanted {
        match which::which(name) {
            Ok(path) => found.push(path),
            Err(_) => {
                assert!(
                    std::env::var_os("CI").is_none(),
                    "{name} is not installed; CI must provide it so the PowerShell completion tests run"
                );
                eprintln!("skipping {name}: not installed");
            }
        }
    }
    found
}

fn path_with_toolr() -> std::ffi::OsString {
    let toolr_dir = assert_cmd::cargo::cargo_bin("toolr")
        .parent()
        .unwrap()
        .to_path_buf();
    let rest = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(toolr_dir).chain(std::env::split_paths(&rest))).unwrap()
}

/// Runs each request through `TabExpansion2` and reports, per match, its text, list text,
/// tooltip, result type, and what PowerShell's parser reads back from the text.
const DRIVER: &str = r#"
param([string]$Script, [string]$Requests, [string]$Output, [string]$Cwd)
. $Script
# PRELUDE
# Deliberately re-implements _toolr_parse_word, so the script is checked against an independent oracle.
function Read-Word([string]$Text) {
    $tokens = $null; $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput("x $Text", [ref]$tokens, [ref]$errors)
    if ($errors.Count -gt 0 -or $ast.EndBlock.Statements.Count -ne 1) { return $null }
    $elements = $ast.EndBlock.Statements[0].PipelineElements[0].CommandElements
    if ($elements.Count -ne 2) { return $null }
    $e = $elements[1]
    if ($e -is [System.Management.Automation.Language.StringConstantExpressionAst]) { return $e.Value }
    if ($e -is [System.Management.Automation.Language.ExpandableStringExpressionAst] -and $e.NestedExpressions.Count -eq 0) { return $e.Value }
    if ($e -is [System.Management.Automation.Language.CommandParameterAst] -or $e -is [System.Management.Automation.Language.ConstantExpressionAst]) { return $e.Extent.Text }
    return $null
}
$utf8 = New-Object System.Text.UTF8Encoding $false
$queue = ConvertFrom-Json -InputObject ([System.IO.File]::ReadAllText($Requests, $utf8))
Set-Location -LiteralPath $Cwd
# CHDIR
$results = @()
foreach ($request in $queue) {
    $r = TabExpansion2 -inputScript $request.line -cursorColumn $request.cursor
    $found = @()
    foreach ($m in $r.CompletionMatches) {
        $found += @{
            text = $m.CompletionText
            list = $m.ListItemText
            tip = $m.ToolTip
            kind = [string]$m.ResultType
            parsed = (Read-Word $m.CompletionText)
        }
    }
    $results += ,@{ index = $r.ReplacementIndex; length = $r.ReplacementLength; matches = $found }
}
[System.IO.File]::WriteAllText($Output, (ConvertTo-Json -Depth 5 -InputObject @($results)), $utf8)
"#;

#[derive(Debug)]
struct Match {
    text: String,
    list: String,
    tip: String,
    kind: String,
    /// The text read back through PowerShell's parser; `None` if it isn't one plain word.
    parsed: Option<String>,
}

#[derive(Debug)]
struct TabResult {
    /// `None` when PowerShell reports no replacement (no matches at all).
    index: Option<usize>,
    length: Option<usize>,
    matches: Vec<Match>,
}

impl TabResult {
    fn texts(&self) -> Vec<&str> {
        self.matches.iter().map(|m| m.text.as_str()).collect()
    }

    fn parsed(&self) -> Vec<String> {
        self.matches
            .iter()
            .map(|m| {
                m.parsed
                    .clone()
                    .unwrap_or_else(|| format!("<unparsed {}>", m.text))
            })
            .collect()
    }
}

struct Session<'a> {
    ps: &'a Path,
    fx: &'a Fixture,
    prelude: &'a str,
    /// A `Set-Location` target replacing the fixture root (PowerShell syntax).
    location: Option<&'a str>,
}

impl<'a> Session<'a> {
    fn new(ps: &'a Path, fx: &'a Fixture) -> Self {
        Self {
            ps,
            fx,
            prelude: "",
            location: None,
        }
    }

    /// Complete each line with the cursor at its end.
    fn tab_all(&self, lines: &[&str]) -> Vec<TabResult> {
        let requests: Vec<(String, usize)> = lines
            .iter()
            .map(|l| (l.to_string(), l.encode_utf16().count()))
            .collect();
        self.run(&requests)
    }

    fn tab(&self, line: &str) -> TabResult {
        self.tab_all(&[line]).pop().unwrap()
    }

    fn run(&self, requests: &[(String, usize)]) -> Vec<TabResult> {
        let (results, stderr) = self.run_raw(requests);
        assert!(stderr.trim().is_empty(), "{}: {stderr}", self.ps.display());
        results
    }

    fn run_raw(&self, requests: &[(String, usize)]) -> (Vec<TabResult>, String) {
        let work = self.fx.work.path();
        for marker in ["# PRELUDE", "# CHDIR"] {
            assert!(DRIVER.contains(marker), "DRIVER lost its {marker} marker");
        }
        let driver = DRIVER.replace("# PRELUDE", self.prelude).replace(
            "# CHDIR",
            &self
                .location
                .map(|l| format!("Set-Location {l}"))
                .unwrap_or_default(),
        );
        // A BOM, so Windows PowerShell 5.1 doesn't read the driver in the ANSI code page.
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(driver.as_bytes());
        std::fs::write(work.join("driver.ps1"), bytes).unwrap();
        let body: Vec<Value> = requests
            .iter()
            .map(|(line, cursor)| json!({"line": line, "cursor": cursor}))
            .collect();
        std::fs::write(work.join("requests.json"), Value::Array(body).to_string()).unwrap();
        let output = work.join("output.json");
        let _ = std::fs::remove_file(&output);
        let out = StdCommand::new(self.ps)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(work.join("driver.ps1"))
            .arg("-Script")
            .arg(work.join("toolr.ps1"))
            .arg("-Requests")
            .arg(work.join("requests.json"))
            .arg("-Output")
            .arg(&output)
            .arg("-Cwd")
            .arg(self.fx.tmp.path())
            .env("PATH", path_with_toolr())
            .env("XDG_CACHE_HOME", common::test_cache_home())
            .envs(home_env(self.fx.tmp.path()))
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(
            out.status.success(),
            "{} failed:\n{}\n{stderr}",
            self.ps.display(),
            String::from_utf8_lossy(&out.stdout)
        );
        let text = std::fs::read_to_string(&output).unwrap();
        let parsed: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
        (
            parsed.as_array().unwrap().iter().map(tab_result).collect(),
            stderr,
        )
    }
}

/// Point PowerShell's home at `home`, so `~` resolves inside the fixture and never the real home.
/// Windows PowerShell 5.1 takes its home from `HOMEDRIVE` and `HOMEPATH`; pwsh from `USERPROFILE`.
fn home_env(home: &Path) -> Vec<(&'static str, std::ffi::OsString)> {
    let mut env = vec![("HOME", home.as_os_str().to_owned())];
    if cfg!(windows) {
        // Assumes a drive-letter path (`C:\...`), so the first two characters are the drive.
        let text = home.to_str().unwrap();
        env.push(("USERPROFILE", home.as_os_str().to_owned()));
        env.push(("HOMEDRIVE", text[..2].into()));
        env.push(("HOMEPATH", text[2..].into()));
    }
    env
}

/// 5.1's `ConvertTo-Json` writes a one-element array as the element itself.
fn as_list(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(a) => a.clone(),
        Value::Null => Vec::new(),
        other => vec![other.clone()],
    }
}

fn tab_result(v: &Value) -> TabResult {
    let s = |m: &Value, k: &str| m[k].as_str().unwrap_or_default().to_string();
    TabResult {
        index: v["index"].as_u64().map(|i| i as usize),
        length: v["length"].as_u64().map(|l| l as usize),
        matches: as_list(&v["matches"])
            .iter()
            .map(|m| Match {
                text: s(m, "text"),
                list: s(m, "list"),
                tip: s(m, "tip"),
                kind: s(m, "kind"),
                parsed: m["parsed"].as_str().map(str::to_string),
            })
            .collect(),
    }
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

#[test]
fn powershell_empty_word_offers_every_value() {
    let fx = fixture();
    for ps in powershells() {
        let r = Session::new(&ps, &fx).tab("toolr ci lint --mode ");
        let lists: Vec<String> = r.matches.iter().map(|m| m.list.clone()).collect();
        assert_eq!(sorted(lists), sorted(fx.values()), "{}", ps.display());
        assert_eq!(sorted(r.parsed()), sorted(fx.values()), "{}", ps.display());
        assert!(
            r.matches.iter().all(|m| m.kind == "ParameterValue"),
            "{r:?}"
        );
        fx.assert_inert();
    }
}

#[test]
fn powershell_values_are_inert() {
    let fx = fixture();
    for ps in powershells() {
        let mut lines = Vec::new();
        for value in fx.values() {
            for open in ["", "'", "\""] {
                for end in value.char_indices().map(|(i, _)| i).chain([value.len()]) {
                    lines.push(format!("toolr ci lint --mode {open}{}", &value[..end]));
                }
            }
        }
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        Session::new(&ps, &fx).tab_all(&refs);
        fx.assert_inert();
    }
}

#[test]
fn powershell_replies_round_trip_through_the_parser() {
    let fx = fixture();
    for ps in powershells() {
        let session = Session::new(&ps, &fx);
        let values = fx.values();
        for open in ["", "'", "\""] {
            // Unquoted, a leading `$`, `@`, `#` or smart quote is PowerShell syntax, not a partial.
            let values: Vec<&String> = values
                .iter()
                .filter(|v| !open.is_empty() || v.starts_with(|c: char| c.is_alphanumeric()))
                .collect();
            let lines: Vec<String> = values
                .iter()
                .map(|v| {
                    let first: String = v.chars().take(1).collect();
                    format!("toolr ci lint --mode {open}{first}")
                })
                .collect();
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            for ((line, value), r) in lines.iter().zip(values).zip(session.tab_all(&refs)) {
                assert!(
                    r.parsed().contains(value),
                    "{}: {line:?} -> {:?}, want {value:?}",
                    ps.display(),
                    r.texts()
                );
                for m in &r.matches {
                    assert_eq!(
                        m.parsed.as_deref(),
                        Some(m.list.as_str()),
                        "{line:?}: {m:?}"
                    );
                }
            }
        }
        fx.assert_inert();
    }
}

#[test]
fn powershell_quoted_and_escaped_partials_match() {
    let fx = fixture();
    for ps in powershells() {
        let session = Session::new(&ps, &fx);
        let results = session.tab_all(&[
            "toolr ci lint --mode 'dry r",
            "toolr ci lint --mode \"dry r",
            "toolr ci lint --mode dry` r",
            "toolr ci lint --mode \u{2018}dry r",
            "toolr ci lint --mode 'it''",
        ]);
        let want = [
            vec!["dry run"],
            vec!["dry run"],
            vec!["dry run"],
            vec!["dry run"],
            vec!["it's"],
        ];
        for (r, want) in results.iter().zip(want) {
            assert_eq!(r.parsed(), want, "{}: {r:?}", ps.display());
        }
        assert_eq!(results[0].texts(), ["'dry run'"]);
        assert_eq!(results[2].texts(), ["'dry run'"]);
        assert_eq!(results[4].texts(), ["'it''s'"]);
    }
}

#[test]
fn powershell_double_quoted_partial_keeps_double_quotes() {
    let fx = fixture();
    for ps in powershells() {
        let session = Session::new(&ps, &fx);
        let results = session.tab_all(&[
            "toolr ci lint --mode \"dry r",
            "toolr ci lint --mode \"a",
            "toolr ci lint --mode \"$",
        ]);
        assert_eq!(results[0].texts(), ["\"dry run\""], "{}", ps.display());
        for m in &results[1].matches {
            assert!(m.text.starts_with('"'), "{m:?}");
            assert_eq!(m.parsed.as_deref(), Some(m.list.as_str()), "{m:?}");
        }
        assert!(results[1].parsed().contains(&"a\"b".to_string()));
        assert!(results[1].parsed().contains(&"a`b".to_string()));
        assert_eq!(results[2].texts().len(), 1, "{:?}", results[2]);
        assert_eq!(results[2].matches[0].parsed, Some(fx.values()[5].clone()));
        fx.assert_inert();
    }
}

#[test]
fn powershell_replacement_covers_the_whole_token() {
    let fx = fixture();
    for ps in powershells() {
        let line = "toolr ci lint --mode 'dry r";
        let r = Session::new(&ps, &fx).tab(line);
        assert_eq!(r.index, Some("toolr ci lint --mode ".len()), "{r:?}");
        assert_eq!(r.length, Some("'dry r".len()), "{r:?}");
    }
}

#[test]
fn powershell_earlier_quoted_words_reach_the_engine_unquoted() {
    let fx = fixture();
    for ps in powershells() {
        let results = Session::new(&ps, &fx).tab_all(&[
            "toolr 'ci' \"lint\" --mode dry",
            "toolr ci lint --verbose --mode dry",
        ]);
        for r in results {
            assert_eq!(r.parsed(), ["dry run", "dry walk"], "{}", ps.display());
        }
    }
}

#[test]
fn powershell_attached_option_value_keeps_the_option() {
    let fx = fixture();
    for ps in powershells() {
        let results = Session::new(&ps, &fx).tab_all(&[
            "toolr ci lint --mode=dry r",
            "toolr ci lint --mode='dry r",
            "toolr ci lint --mode=\"dry r",
            "toolr ci lint --mode=it",
        ]);
        assert_eq!(results[0].texts(), Vec::<&str>::new(), "{}", ps.display());
        assert_eq!(results[1].texts(), ["'--mode=dry run'"], "{}", ps.display());
        assert_eq!(results[1].parsed(), ["--mode=dry run"]);
        assert_eq!(
            results[2].texts(),
            ["\"--mode=dry run\""],
            "{}",
            ps.display()
        );
        assert_eq!(results[3].texts(), ["'--mode=it''s'"], "{}", ps.display());
        assert_eq!(results[3].parsed(), ["--mode=it's"]);
    }
}

/// Windows PowerShell 5.1, as opposed to pwsh.
fn is_windows_powershell(ps: &Path) -> bool {
    ps.file_stem()
        .is_some_and(|s| s.eq_ignore_ascii_case("powershell"))
}

#[test]
fn powershell_flags_complete_with_descriptions() {
    let fx = fixture();
    for ps in powershells() {
        let results = Session::new(&ps, &fx).tab_all(&[
            "toolr ci lint --",
            "toolr ci lint -",
            "toolr ci lint --m",
        ]);
        if is_windows_powershell(&ps) {
            // 5.1 never calls a native completer for a word PowerShell tokenises as a parameter or
            // operator (`-`, `--`); PowerShell 6 fixed both (PowerShell/PowerShell#2912 via #2919, and #3633).
            assert!(results[0].matches.is_empty(), "{:?}", results[0]);
            assert!(results[1].matches.is_empty(), "{:?}", results[1]);
        } else {
            for r in &results[..2] {
                for flag in ["--mode", "--cfg", "--out", "--verbose"] {
                    assert!(r.texts().contains(&flag), "{}: {r:?}", ps.display());
                }
            }
        }
        // `--m` scans as a generic word, which every version passes to the completer.
        let mode = results[2].matches.iter().find(|m| m.text == "--mode");
        assert_eq!(mode.unwrap().tip, "Lint mode.", "{}", ps.display());
        assert_eq!(results[2].texts(), ["--mode"], "{}", ps.display());
    }
}

#[test]
fn powershell_double_dash_and_root_flags_pass_through() {
    let fx = fixture();
    for ps in powershells() {
        let results = Session::new(&ps, &fx).tab_all(&[
            "toolr --quiet ci li",
            "toolr ci lint --mode 0",
            "toolr -- ",
        ]);
        assert_eq!(results[0].texts(), ["lint"], "{}", ps.display());
        assert_eq!(results[1].texts(), ["0x10"], "{}", ps.display());
        // The engine sees a leading `--` and offers no group, so no fixture value either.
        assert!(!results[2].texts().contains(&"ci"), "{:?}", results[2]);
    }
}

#[test]
fn powershell_variable_or_subexpression_word_offers_nothing() {
    let fx = fixture();
    for ps in powershells() {
        let session = Session::new(&ps, &fx);
        let s1 = fx.sentinels[0].display();
        let lines = [
            "toolr ci lint --mode $x".to_string(),
            "toolr ci lint --mode \"a$(x)".to_string(),
            "toolr ci lint --mode a,b".to_string(),
            format!("toolr ci lint --mode $(New-Item '{s1}')"),
            "toolr $x lint --mode ".to_string(),
            format!("toolr ci $(New-Item '{s1}') --mode "),
            "toolr ci lint,x --mode ".to_string(),
            // After `--%` PowerShell passes the rest of the line verbatim.
            "toolr --% ci lint --mode ".to_string(),
        ];
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let values = fx.values();
        for (line, r) in lines.iter().zip(session.tab_all(&refs)) {
            for m in &r.matches {
                assert!(
                    !values.contains(&m.list),
                    "{}: {line:?} offered {m:?}",
                    ps.display()
                );
            }
        }
        fx.assert_inert();
    }
}

fn sep() -> char {
    std::path::MAIN_SEPARATOR
}

#[test]
fn powershell_path_slot_completes_files() {
    let fx = fixture();
    for ps in powershells() {
        let results = Session::new(&ps, &fx)
            .tab_all(&["toolr ci lint --cfg fi", "toolr ci lint --cfg files/w"]);
        assert!(
            results[0]
                .matches
                .iter()
                .any(|m| m.kind == "ProviderContainer" && m.list == "files"),
            "{}: {:?}",
            ps.display(),
            results[0]
        );
        let want = format!(".{0}files{0}with space.txt", sep());
        assert_eq!(
            results[1].parsed(),
            [want],
            "{}: {:?}",
            ps.display(),
            results[1]
        );
    }
}

#[test]
fn powershell_directory_slot_completes_only_directories() {
    let fx = fixture();
    for ps in powershells() {
        let results =
            Session::new(&ps, &fx).tab_all(&["toolr ci lint --out files/", "toolr ci lint --out "]);
        let lists: Vec<&str> = results[0].matches.iter().map(|m| m.list.as_str()).collect();
        assert_eq!(lists, ["sub"], "{}: {:?}", ps.display(), results[0]);
        let r = &results[1];
        assert!(
            r.matches.iter().any(|m| m.list == "files")
                && r.matches.iter().all(|m| m.kind == "ProviderContainer"),
            "{}: {r:?}",
            ps.display()
        );
    }
}

#[test]
fn powershell_attached_path_keeps_the_option() {
    let fx = fixture();
    for ps in powershells() {
        let results = Session::new(&ps, &fx).tab_all(&[
            "toolr ci lint --cfg=files/w",
            "toolr ci lint --cfg=files/it",
            "toolr ci lint --out=files/",
        ]);
        let want = |name: &str| format!("--cfg=.{0}files{0}{name}", sep());
        assert_eq!(
            results[0].parsed(),
            [want("with space.txt")],
            "{}",
            ps.display()
        );
        assert_eq!(results[1].parsed(), [want("it's.txt")], "{}", ps.display());
        let dirs: Vec<&str> = results[2].matches.iter().map(|m| m.list.as_str()).collect();
        assert_eq!(dirs, ["sub"], "{}: {:?}", ps.display(), results[2]);
    }
}

/// Native commands get the path itself, not the wildcard-escaped text `-Path` parameters take.
#[test]
fn powershell_wildcard_characters_in_paths_stay_literal() {
    let fx = fixture();
    let path = |name: &str| format!(".{0}odd{0}{name}", sep());
    let files = [path("a[1].txt"), path("a`b.txt"), path("a``c.txt")];
    let dirs = sorted(vec![path("d[1]"), path("e`f")]);
    let mut lines = Vec::new();
    let mut want = Vec::new();
    for open in ["", "'", "\""] {
        for prefix in ["", "--cfg="] {
            lines.push(format!("toolr ci lint --cfg {open}{prefix}odd/a"));
            want.push(sorted(
                files.iter().map(|f| format!("{prefix}{f}")).collect(),
            ));
            lines.push(format!("toolr ci lint --cfg {open}{prefix}odd/a["));
            want.push(vec![format!("{prefix}{}", path("a[1].txt"))]);
            if open != "'" {
                // A doubled backtick is one literal backtick outside single quotes.
                lines.push(format!("toolr ci lint --cfg {open}{prefix}odd/a``"));
                want.push(sorted(vec![
                    format!("{prefix}{}", path("a`b.txt")),
                    format!("{prefix}{}", path("a``c.txt")),
                ]));
            }
        }
        for prefix in ["", "--out="] {
            lines.push(format!("toolr ci lint --out {open}{prefix}odd/"));
            want.push(dirs.iter().map(|d| format!("{prefix}{d}")).collect());
        }
    }
    lines.push("toolr ci lint --cfg odd/a[1".to_string());
    want.push(vec![path("a[1].txt")]);
    if !cfg!(windows) {
        // `*` is a wildcard in the partial, as in PowerShell's own native-command completion.
        lines.push("toolr ci lint --cfg odd/x*".to_string());
        want.push(vec![path("x*y.txt")]);
        lines.push("toolr ci lint --cfg=odd/x".to_string());
        want.push(vec![format!("--cfg={}", path("x*y.txt"))]);
    }
    for ps in powershells() {
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let results = Session::new(&ps, &fx).tab_all(&lines);
        for ((line, want), r) in lines.iter().zip(&want).zip(&results) {
            assert_eq!(
                &sorted(r.parsed()),
                want,
                "{} `{line}`: {r:?}",
                ps.display()
            );
        }
    }
}

/// PowerShell doesn't expand a quoted `~`, so a path completed from `~/` names the home itself.
#[test]
fn powershell_tilde_path_expands_to_the_home() {
    let fx = fixture();
    let home = fx.tmp.path().to_str().unwrap();
    let files = format!("{home}{}files", sep());
    for ps in powershells() {
        let results = Session::new(&ps, &fx).tab_all(&[
            "toolr ci lint --cfg ~/",
            "toolr ci lint --cfg=~/",
            "toolr ci lint --cfg ~/fi",
        ]);
        for (prefix, r) in [("", &results[0]), ("--cfg=", &results[1])] {
            let parsed = r.parsed();
            assert!(
                parsed.contains(&format!("{prefix}{files}")),
                "{}: {r:?}",
                ps.display()
            );
            assert!(
                parsed
                    .iter()
                    .all(|p| p.starts_with(&format!("{prefix}{home}"))),
                "{}: {r:?}",
                ps.display()
            );
        }
        assert_eq!(
            results[2].parsed(),
            std::slice::from_ref(&files),
            "{}",
            ps.display()
        );
    }
}

#[test]
fn powershell_cursor_mid_word_completes_the_whole_word() {
    let fx = fixture();
    for ps in powershells() {
        let line = "toolr ci lint --mode dryX";
        let r = Session::new(&ps, &fx).run(&[(line.to_string(), line.len() - 1)]);
        // The engine sees `dryX`, which matches nothing; PowerShell replaces the whole token.
        let values = fx.values();
        assert!(
            r[0].matches.iter().all(|m| !values.contains(&m.list)),
            "{}: {:?}",
            ps.display(),
            r[0]
        );
        let line = "toolr ci lint --mode dr";
        let r = Session::new(&ps, &fx).run(&[(line.to_string(), line.len() - 1)]);
        assert_eq!(r[0].parsed(), ["dry run", "dry walk"], "{}", ps.display());
    }
}

#[test]
fn powershell_no_tools_dir_completes_builtins() {
    let fx = fixture();
    let outside = TempDir::new().unwrap();
    let location = format!("-LiteralPath '{}'", outside.path().display());
    for ps in powershells() {
        let mut session = Session::new(&ps, &fx);
        session.location = Some(&location);
        let results = session.tab_all(&["toolr se", "toolr.exe se"]);
        for r in results {
            assert_eq!(r.texts(), ["self"], "{}", ps.display());
        }
    }
}

#[test]
fn powershell_unicode_and_backslash_partials_reach_the_engine() {
    let fx = fixture();
    for ps in powershells() {
        let results = Session::new(&ps, &fx).tab_all(&[
            "toolr ci lint --mode \u{e9}",
            "toolr ci lint --mode 'e\\",
            "toolr ci lint --mode 'a\"",
            "toolr ci lint --mode ''",
            "toolr ci lint --mode 'dry ",
        ]);
        assert_eq!(results[0].parsed(), ["\u{e9}t\u{e9}"], "{}", ps.display());
        assert_eq!(results[1].parsed(), ["e\\f"], "{}", ps.display());
        assert_eq!(results[2].parsed(), ["a\"b"], "{}", ps.display());
        assert_eq!(
            sorted(results[3].parsed()),
            sorted(fx.values()),
            "{}",
            ps.display()
        );
        assert_eq!(
            results[4].parsed(),
            ["dry run", "dry walk"],
            "{}",
            ps.display()
        );
    }
}

#[test]
fn powershell_empty_earlier_word_reaches_the_engine() {
    let fx = fixture();
    for ps in powershells() {
        let r = Session::new(&ps, &fx).tab("toolr ci pick '' ");
        assert_eq!(r.texts(), ["x1", "x2"], "{}", ps.display());
    }
}

#[test]
fn powershell_legacy_argument_passing_round_trips() {
    let fx = fixture();
    for ps in powershells() {
        let mut session = Session::new(&ps, &fx);
        session.prelude = "$PSNativeCommandArgumentPassing = 'Legacy'";
        let results = session.tab_all(&[
            "toolr ci pick '' ",
            "toolr ci lint --mode 'a\"",
            "toolr ci lint --mode 'e\\",
            "toolr ci lint --mode 'dry ",
        ]);
        assert_eq!(results[0].texts(), ["x1", "x2"], "{}", ps.display());
        assert_eq!(results[1].parsed(), ["a\"b"], "{}", ps.display());
        assert_eq!(results[2].parsed(), ["e\\f"], "{}", ps.display());
        assert_eq!(
            results[3].parsed(),
            ["dry run", "dry walk"],
            "{}",
            ps.display()
        );
    }
}

#[test]
fn powershell_hostile_session_settings_stay_quiet() {
    let fx = fixture();
    for ps in powershells() {
        let mut session = Session::new(&ps, &fx);
        session.prelude = "$ErrorActionPreference = 'Stop'\n\
             Set-StrictMode -Version Latest\n\
             $PSNativeCommandUseErrorActionPreference = $true";
        let results = session.tab_all(&[
            "toolr ci lint --mode dr",
            "toolr ci lint --cfg files/w",
            "toolr nope --mode ",
            "toolr ci lint --mode $x",
        ]);
        assert_eq!(
            results[0].parsed(),
            ["dry run", "dry walk"],
            "{}",
            ps.display()
        );
        assert_eq!(
            results[1].matches.len(),
            1,
            "{}: {:?}",
            ps.display(),
            results[1]
        );
    }
}

#[test]
fn powershell_user_function_named_toolr_does_not_recurse() {
    let fx = fixture();
    for ps in powershells() {
        let mut session = Session::new(&ps, &fx);
        session.prelude = "function global:toolr { throw 'called' }";
        // The function wins command lookup for `toolr`; `toolr.exe` still reaches the binary
        // on Windows, and the script must never call the function either way.
        let (results, stderr) = session.run_raw(&[
            ("toolr ci lint --mode dr".to_string(), 23),
            ("toolr.exe se".to_string(), 12),
        ]);
        assert!(!stderr.contains("called"), "{}: {stderr}", ps.display());
        if cfg!(windows) {
            assert_eq!(results[1].texts(), ["self"], "{}", ps.display());
        }
    }
}

#[test]
fn powershell_non_filesystem_location_offers_no_values() {
    let fx = fixture();
    for ps in powershells() {
        let mut session = Session::new(&ps, &fx);
        session.location = Some("Env:");
        let r = session.tab("toolr ci lint --mode ");
        let values = fx.values();
        assert!(
            r.matches.iter().all(|m| !values.contains(&m.list)),
            "{}: {r:?}",
            ps.display()
        );
    }
}

#[test]
fn powershell_version_guard_wraps_the_whole_script() {
    let fx = fixture();
    let check = r#"
param([string]$Script)
$tokens = $null; $errors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile($Script, [ref]$tokens, [ref]$errors)
if ($errors.Count -gt 0) { throw "parse errors: $errors" }
$statements = $ast.EndBlock.Statements
if ($statements.Count -ne 1) { throw "expected one top-level statement, got $($statements.Count)" }
$if = $statements[0]
if ($if -isnot [System.Management.Automation.Language.IfStatementAst]) { throw 'not an if' }
if ($if.Clauses.Count -ne 1) { throw 'expected one clause' }
$condition = $if.Clauses[0].Item1.Extent.Text
if ($condition -cne "`$PSVersionTable.PSVersion -lt [version]'5.1'") { throw "condition: $condition" }
$body = $if.Clauses[0].Item2.Statements
if ($body.Count -ne 1 -or $body[0].Extent.Text -cne "Write-Warning 'toolr completion needs PowerShell 5.1 or newer'") {
    throw "if body: $($if.Clauses[0].Item2.Extent.Text)"
}
if ($null -eq $if.ElseClause -or -not $if.ElseClause.Extent.Text.Contains('Register-ArgumentCompleter -Native')) {
    throw 'no registration in the else'
}
'ok'
"#;
    std::fs::write(fx.work.path().join("check.ps1"), check).unwrap();
    for ps in powershells() {
        let out = StdCommand::new(&ps)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(fx.work.path().join("check.ps1"))
            .arg(fx.work.path().join("toolr.ps1"))
            .output()
            .unwrap();
        assert!(
            out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "ok",
            "{}: {}\n{}",
            ps.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
