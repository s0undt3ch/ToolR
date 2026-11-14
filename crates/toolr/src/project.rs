//! Implementation of `toolr project <...>` subcommands.

use std::collections::HashSet;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context as _, Result};
use clap::ArgMatches;

use crate::init_scaffold::{ScaffoldConflictsError, analyze_scaffold, execute_scaffold, scaffold};
use crate::init_templates::{ScaffoldOptions, parse_venv_location};
use crate::invocation::Invocation;
use toolr_core::exit_status::process_exit_code;
use toolr_core::trust::{
    TrustMode, TrustStore, Trusted, require_trust_quietly, shown, trust_all_env,
};

/// `quiet` is the root `-q`, the only place toolr takes it. A handler that may install uv must honour it and get a case in
/// `tests/quiet_uv_consent.rs`. The `-q` output rule is stated on the arg in
/// `cli::root_flag_args`.
pub(crate) fn dispatch_project(
    matches: &ArgMatches,
    run: &Invocation,
    quiet: bool,
) -> Result<ExitCode> {
    match matches.subcommand() {
        Some(("init", init_m)) => project_init(init_m, run, quiet),
        Some(("venv", venv_m)) => match venv_m.subcommand() {
            Some(("path", _)) => venv_path(run, quiet),
            Some(("shell", _)) => venv_shell(run, quiet),
            Some(("run", run_m)) => venv_run(run_m, run, quiet),
            Some(("sync", sync_m)) => venv_sync(sync_m, run, quiet),
            Some(("lock", lock_m)) => venv_lock(lock_m, run, quiet),
            Some(("add", add_m)) => venv_add(add_m, run, quiet),
            Some(("remove", remove_m)) => venv_remove(remove_m, run, quiet),
            _ => unreachable!("clap enforces subcommand_required"),
        },
        Some(("manifest", manifest_m)) => match manifest_m.subcommand() {
            Some(("rebuild", _)) => manifest_rebuild(run, quiet),
            _ => unreachable!("clap enforces subcommand_required"),
        },
        Some(("trust", trust_m)) => project_trust(trust_m, run, quiet),
        Some(("untrust", untrust_m)) => project_untrust(untrust_m, run, quiet),
        _ => unreachable!("clap enforces subcommand_required"),
    }
}

/// Make the tools venv under `cwd` ready: the uv consent, uv's `--quiet` and the trust prompt
/// all follow `quiet`, and `tweak` adds what the caller needs on top.
pub(crate) fn ready_venv(
    cwd: &Path,
    quiet: bool,
    tweak: impl FnOnce(toolr_core::project::EnsureOpts) -> toolr_core::project::EnsureOpts,
) -> Result<(toolr_core::venv::ResolvedVenv, toolr_core::uv::UvBinary)> {
    let consent = toolr_core::uv::install::ConsentMode::from_env(quiet);
    let opts = toolr_core::project::EnsureOpts::default()
        .with_quiet(quiet)
        .with_trust(TrustMode {
            never_prompt: quiet,
        });
    toolr_core::project::ensure_venv_ready(cwd, consent, tweak(opts))
}

/// What a `project` handler that works on the tools venv starts from: the run's working
/// directory and its project root, looked up once.
struct ProjectCtx<'a> {
    run: &'a Invocation,
    cwd: &'a Path,
}

impl<'a> ProjectCtx<'a> {
    fn new(run: &'a Invocation) -> Self {
        Self { run, cwd: &run.cwd }
    }

    fn repo_root(&self) -> Result<PathBuf> {
        self.run.project_root()
    }
}

/// Before anything reads `tools/pyproject.toml`: toolr reads nothing of an untrusted repository.
fn trust_root(repo_root: &Path, quiet: bool) -> Result<Trusted> {
    require_trust_quietly(repo_root, quiet)
}

/// Fail unless `tools/pyproject.toml` declares every one of `packages`; `fix` ends the message.
fn require_declared(tools_dir: &Path, packages: &[String], fix: &str) -> Result<()> {
    let pyproject = tools_dir.join("pyproject.toml");
    for pkg in packages {
        if !pyproject_declares_dependency(&pyproject, pkg)? {
            anyhow::bail!(
                "package `{pkg}` is not declared in {} — {fix}",
                pyproject.display(),
            );
        }
    }
    Ok(())
}

/// `venv sync --if-project`'s exit: success, silently, when `result` failed because there
/// was nothing to sync.
fn nothing_to_sync_exit<T>(result: &Result<T>, if_project: bool) -> Option<ExitCode> {
    let e = result.as_ref().err()?;
    (if_project && toolr_core::project::is_nothing_to_sync(e)).then_some(ExitCode::SUCCESS)
}

fn root_from_path_arg(matches: &ArgMatches, run: &Invocation) -> Result<PathBuf> {
    match matches.get_one::<PathBuf>("path") {
        Some(start) => toolr_core::discovery::discover_project_root(start)
            .map_err(|e| toolr_core::project::NothingToSync::no_project(&e).into()),
        None => run.project_root(),
    }
}

/// Trust first, then sync: stdout stays the trusted root alone (the human
/// confirmation goes to stderr), and a declined sync keeps the trust and exits
/// 0.
fn project_trust(matches: &ArgMatches, run: &Invocation, quiet: bool) -> Result<ExitCode> {
    let root = root_from_path_arg(matches, run)?;
    let was_trusted = TrustStore::open_default()?.is_trusted(&root)?;
    let (record, trusted) = toolr_core::trust::grant_trust(&root)?;
    let repo = shown(&record.repo_path);
    println!("{repo}");
    if !quiet {
        if was_trusted {
            eprintln!("toolr: {repo} was already trusted");
        } else {
            eprintln!("toolr: trusted {repo}");
        }
    }
    if matches.get_flag("no-sync") {
        return Ok(ExitCode::SUCCESS);
    }
    use toolr_core::project::{SYNC_LATER_HINT, SyncAfterTrust, sync_after_trust_on_tty};
    let outcome = sync_after_trust_on_tty(&trusted, quiet)
        .context("the repository is trusted, but syncing its tools venv failed")?;
    match outcome {
        SyncAfterTrust::Synced(resolved) if !quiet => {
            eprintln!("toolr: synced venv at {}", shown(&resolved.venv_dir));
        }
        SyncAfterTrust::Declined if !quiet => eprintln!("toolr: {SYNC_LATER_HINT}"),
        _ => {}
    }
    Ok(ExitCode::SUCCESS)
}

fn project_untrust(matches: &ArgMatches, run: &Invocation, quiet: bool) -> Result<ExitCode> {
    let root = root_from_path_arg(matches, run)?;
    let store = TrustStore::open_default()?;
    let (repo, existed) = store.untrust(&root)?;
    if existed {
        println!("{}", shown(&repo));
        if !quiet {
            eprintln!("toolr: untrusted {}", shown(&repo));
        }
    } else {
        eprintln!("toolr: {} was not trusted", shown(&repo));
    }
    Ok(ExitCode::SUCCESS)
}

fn project_init(matches: &ArgMatches, run: &Invocation, quiet: bool) -> Result<ExitCode> {
    let force = matches.get_flag("force");
    let no_sync = matches.get_flag("no-sync");
    let no_example = matches.get_flag("no-example");
    let yes_all = matches.get_flag("yes");
    let venv_location_str = matches
        .get_one::<String>("venv-location")
        .map(String::as_str)
        .unwrap_or("cache");
    let venv_location = parse_venv_location(venv_location_str)?;
    let requires_python = matches
        .get_one::<String>("python")
        .cloned()
        .unwrap_or_else(detect_requires_python);

    let cwd = run.cwd.clone();
    let opts = ScaffoldOptions {
        requires_python,
        venv_location,
        include_example: !no_example,
    };

    // Non-interactive path: detect conflicts early and exit with code 2 so
    // agents can distinguish "conflicts" from generic failures (exit 1).
    if !force && !yes_all && !std::io::stdin().is_terminal() {
        let analysis = analyze_scaffold(&cwd, &opts)?;
        let conflicts = analysis.conflict_files();
        if !conflicts.is_empty() {
            let err = ScaffoldConflictsError {
                files: conflicts
                    .into_iter()
                    .map(|p| p.strip_prefix(&cwd).unwrap_or(p).to_path_buf())
                    .collect(),
            };
            eprintln!("toolr: error: {err}");
            return Ok(ExitCode::from(2));
        }
    }

    run_project_init(
        &cwd,
        &opts,
        force,
        no_sync,
        quiet,
        yes_all,
        init_trust_store().as_ref(),
        &mut |dest| prompt_overwrite_file(dest, &cwd),
    )
}

/// `None` under `TOOLR_TRUST_ALL=1`, which makes trust irrelevant, or when no
/// store can be located; init then leaves trust to the sync's own check.
fn init_trust_store() -> Option<TrustStore> {
    if trust_all_env() {
        return None;
    }
    TrustStore::open_default()
        .inspect_err(|e| eprintln!("toolr: warning: could not open the trust store: {e:#}"))
        .ok()
}

/// Inner implementation, factored out so the confirm callback can be injected
/// in tests without spinning up a full process.
#[allow(clippy::too_many_arguments)] // flat flags keep the unit-test call sites readable.
pub(crate) fn run_project_init(
    cwd: &Path,
    opts: &ScaffoldOptions,
    force: bool,
    no_sync: bool,
    quiet: bool,
    yes_all: bool,
    trust_store: Option<&TrustStore>,
    confirm: &mut dyn FnMut(&Path) -> Result<bool>,
) -> Result<ExitCode> {
    let outcome = if force {
        scaffold(cwd, opts, true)?
    } else {
        let analysis = analyze_scaffold(cwd, opts)?;
        let conflicts = analysis.conflict_files();

        let overwrite: HashSet<PathBuf> = if conflicts.is_empty() {
            HashSet::new()
        } else if yes_all {
            // --yes: approve all conflicts without prompting.
            conflicts.iter().map(|p| p.to_path_buf()).collect()
        } else {
            // Call confirm once per conflicting file; collect approved paths.
            conflicts
                .iter()
                .filter_map(|dest| match confirm(dest) {
                    Err(e) => Some(Err(e)),
                    Ok(true) => Some(Ok(dest.to_path_buf())),
                    Ok(false) => None,
                })
                .collect::<Result<_>>()?
        };

        execute_scaffold(&analysis, &overwrite)?
    };

    if !quiet {
        println!(
            "toolr: scaffolded tools/ at {}",
            outcome.tools_dir.display()
        );
        for path in &outcome.files_written {
            let rel = path.strip_prefix(cwd).unwrap_or(path).display();
            println!("toolr:   wrote {rel}");
        }
    }

    // The user is authoring this repo; trust it before the sync below, which
    // would otherwise refuse to run. The files are already written, so a
    // failure only warns and leaves the sync's own check to prompt or refuse.
    if let Some(store) = trust_store {
        match store.trust(cwd) {
            Ok(record) if !quiet => eprintln!("toolr: trusted {}", shown(&record.repo_path)),
            Ok(_) => {}
            Err(e) => eprintln!(
                "toolr: warning: could not record trust for {}: {e:#}",
                shown(cwd)
            ),
        }
    }

    if no_sync {
        if !quiet {
            println!("toolr: skipping `uv sync` (--no-sync)");
            println!("toolr: run `toolr project venv sync` when you are ready");
            println!(
                "toolr: then run commands with `toolr project venv run -- <cmd>` (e.g. pytest tests/)"
            );
        }
        return Ok(ExitCode::SUCCESS);
    }

    // Auto-sync — same path as `toolr project venv sync --force`.
    let (resolved, uv) = ready_venv(cwd, quiet, |o| o.with_force_sync(true))?;
    if !quiet {
        println!(
            "toolr: synced venv at {} using uv {}.{}.{}",
            resolved.venv_dir.display(),
            uv.version.0,
            uv.version.1,
            uv.version.2,
        );
        println!("toolr:");
        println!("toolr: next steps:");
        println!("toolr:   toolr example hello");
        println!("toolr:   toolr example commit");
        println!(
            "toolr:   toolr self completion install <bash|zsh|fish|powershell>   # optional, for tab completion"
        );
        println!(
            "toolr:   toolr project venv run -- pytest tests/            # run your tests in the managed venv"
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Prompt the user whether to overwrite a single conflicting file; EOF declines.
fn prompt_overwrite_file(dest: &Path, cwd: &Path) -> Result<bool> {
    let rel = dest.strip_prefix(cwd).unwrap_or(dest);
    Ok(toolr_core::prompt::yes_no_stdin(
        &format!("toolr: overwrite {}?", rel.display()),
        false,
    ))
}

/// Default `requires-python` value for new projects, from the `python3` in an
/// absolute `PATH` directory (never one in the repo through a relative entry).
fn detect_requires_python() -> String {
    requires_python_from(toolr_core::path_search::find_on_path("python3").as_deref())
}

/// `>=3.N` for `python`'s minor, never below the oldest Python toolr runs on.
fn requires_python_from(python: Option<&Path>) -> String {
    use toolr_core::execute::python::{MIN_PYTHON_MINOR, python3_minor};
    let minor = python
        .and_then(python3_minor)
        .map_or(MIN_PYTHON_MINOR, |m| m.max(MIN_PYTHON_MINOR));
    format!(">=3.{minor}")
}

fn manifest_rebuild(run: &Invocation, quiet: bool) -> Result<ExitCode> {
    use toolr_core::manifest_build::rebuild_manifest_full;

    let repo_root = run.project_root()?;
    // The rebuild parses `tools/*.py` and globs the venv's plugin fragments.
    trust_root(&repo_root, quiet)?;
    let resolved = toolr_core::venv::resolve_venv_path(&repo_root)?;
    let venv = resolved.usable_venv_dir(&repo_root);
    if venv.is_none() {
        eprintln!(
            "toolr: warning: leaving out its plugin commands: {}",
            crate::build_manifest_resolve::foreign_venv_hint(&resolved.venv_dir)
        );
    }
    let built = rebuild_manifest_full(&repo_root, venv)?;
    for w in built.all_warnings() {
        eprintln!("toolr: warning: {w}");
    }
    if !quiet {
        println!(
            "toolr: wrote {} groups / {} commands to {}",
            built.manifest.groups.len(),
            built.manifest.commands.len(),
            toolr_core::manifest::manifest_path(&repo_root).display(),
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn venv_sync(matches: &ArgMatches, run: &Invocation, quiet: bool) -> Result<ExitCode> {
    let ctx = ProjectCtx::new(run);
    let force = matches.get_flag("force");
    let upgrade_all = matches.get_flag("upgrade");
    let upgrade_pkgs: Vec<String> = matches
        .get_many::<String>("upgrade-package")
        .map(|vals| vals.cloned().collect())
        .unwrap_or_default();

    let if_project = matches.get_flag("if-project");
    let repo_root = ctx.repo_root();
    if let Some(code) = nothing_to_sync_exit(&repo_root, if_project) {
        return Ok(code);
    }
    let repo_root = repo_root?;
    let trusted = trust_root(&repo_root, quiet)?;
    // -U on its own skips the check because uv re-locks everything.
    require_declared(
        &repo_root.join("tools"),
        &upgrade_pkgs,
        "add it to `[project] dependencies` first",
    )?;
    let upgrade = build_upgrade_mode(upgrade_all, upgrade_pkgs);

    let result = ready_venv(ctx.cwd, quiet, |o| {
        o.with_force_sync(force)
            .with_upgrade(upgrade)
            .with_trusted(trusted)
    });

    if let Some(code) = nothing_to_sync_exit(&result, if_project) {
        return Ok(code);
    }

    let (resolved, uv) = result.map_err(|e| {
        if toolr_core::project::is_nothing_to_sync(&e) {
            anyhow::anyhow!(
                "{e:#}\nhint: pass --if-project to exit 0 when there is nothing to sync"
            )
        } else {
            e
        }
    })?;

    if !quiet {
        println!(
            "toolr: synced venv at {} using uv {}.{}.{}",
            resolved.venv_dir.display(),
            uv.version.0,
            uv.version.1,
            uv.version.2,
        );
        println!(
            "toolr: run a command in it with `toolr project venv run -- <cmd>` (e.g. pytest tests/)"
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Convert (`--upgrade`, `--upgrade-package ...`) flags into the
/// `UpgradeMode` the core layer wants. `All` wins if both are present —
/// uv accepts the combo, but the lock-side semantics are the same as
/// passing `-U` alone.
fn build_upgrade_mode(all: bool, pkgs: Vec<String>) -> toolr_core::venv::UpgradeMode {
    if all {
        return toolr_core::venv::UpgradeMode::All;
    }
    if pkgs.is_empty() {
        toolr_core::venv::UpgradeMode::None
    } else {
        toolr_core::venv::UpgradeMode::Packages(pkgs)
    }
}

fn venv_lock(matches: &ArgMatches, run: &Invocation, quiet: bool) -> Result<ExitCode> {
    let ctx = ProjectCtx::new(run);
    let upgrade_all = matches.get_flag("upgrade");
    let upgrade_pkgs: Vec<String> = matches
        .get_many::<String>("upgrade-package")
        .map(|vals| vals.cloned().collect())
        .unwrap_or_default();

    let repo_root = ctx.repo_root()?;
    let trusted = trust_root(&repo_root, quiet)?;
    let tools_dir = repo_root.join("tools");
    require_declared(
        &tools_dir,
        &upgrade_pkgs,
        "add it to `[project] dependencies` first",
    )?;

    let upgrade = build_upgrade_mode(upgrade_all, upgrade_pkgs);

    let (resolved, uv) = ready_venv(ctx.cwd, quiet, |o| o.with_trusted(trusted))?;

    let status = toolr_core::venv::run_uv_lock(&uv, &tools_dir, &resolved, &upgrade, quiet)?;
    if !status.success() {
        anyhow::bail!("`uv lock` failed with exit code {:?}", status.code(),);
    }

    if !quiet {
        println!("toolr: refreshed {}", tools_dir.join("uv.lock").display());
    }
    Ok(ExitCode::SUCCESS)
}

fn venv_add(matches: &ArgMatches, run: &Invocation, quiet: bool) -> Result<ExitCode> {
    let ctx = ProjectCtx::new(run);
    let specs: Vec<String> = matches
        .get_many::<String>("packages")
        .expect("clap marks this required")
        .cloned()
        .collect();

    let tools_dir = ctx.repo_root()?.join("tools");

    let (resolved, uv) = ready_venv(ctx.cwd, quiet, |o| o)?;

    let status = toolr_core::venv::run_uv_add(&uv, &tools_dir, &resolved, &specs, quiet)?;
    if !status.success() {
        anyhow::bail!("`uv add` failed with exit code {:?}", status.code());
    }

    if !quiet {
        println!(
            "toolr: added {} to {}",
            specs.join(", "),
            tools_dir.join("pyproject.toml").display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn venv_remove(matches: &ArgMatches, run: &Invocation, quiet: bool) -> Result<ExitCode> {
    let ctx = ProjectCtx::new(run);
    let packages: Vec<String> = matches
        .get_many::<String>("packages")
        .expect("clap marks this required")
        .cloned()
        .collect();

    let repo_root = ctx.repo_root()?;
    let trusted = trust_root(&repo_root, quiet)?;
    let tools_dir = repo_root.join("tools");
    require_declared(&tools_dir, &packages, "nothing to remove")?;
    let pyproject = tools_dir.join("pyproject.toml");

    let (resolved, uv) = ready_venv(ctx.cwd, quiet, |o| o.with_trusted(trusted))?;

    let status = toolr_core::venv::run_uv_remove(&uv, &tools_dir, &resolved, &packages, quiet)?;
    if !status.success() {
        anyhow::bail!("`uv remove` failed with exit code {:?}", status.code());
    }

    if !quiet {
        println!(
            "toolr: removed {} from {}",
            packages.join(", "),
            pyproject.display()
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Light TOML inspection: does `[project].dependencies` (or any
/// `[project.optional-dependencies.*]` list) name `package`?
/// We only need to confirm presence — version pin / extras are uv's
/// problem from here on.
fn pyproject_declares_dependency(pyproject: &Path, package: &str) -> Result<bool> {
    let text = std::fs::read_to_string(pyproject) // nosemgrep: rust.actix.path-traversal.tainted-path.tainted-path
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", pyproject.display()))?;
    let parsed: toml::Value = toml::from_str(&text)
        .map_err(|e| anyhow::anyhow!("parsing {}: {e}", pyproject.display()))?;

    let mut found = false;
    if let Some(deps) = parsed
        .get("project")
        .and_then(|p| p.get("dependencies"))
        .and_then(|v| v.as_array())
    {
        for dep in deps {
            if let Some(s) = dep.as_str()
                && dep_name_matches(s, package)
            {
                found = true;
                break;
            }
        }
    }
    if !found
        && let Some(opt) = parsed
            .get("project")
            .and_then(|p| p.get("optional-dependencies"))
            .and_then(|v| v.as_table())
    {
        for (_extra, list) in opt {
            if let Some(deps) = list.as_array() {
                for dep in deps {
                    if let Some(s) = dep.as_str()
                        && dep_name_matches(s, package)
                    {
                        found = true;
                        break;
                    }
                }
            }
        }
    }
    Ok(found)
}

/// PEP 508-light: `foo`, `foo[bar]`, `foo>=1.2`, `foo ==1.2; python_version < "3.13"`
/// all parse to the name `foo`. We just peel the first identifier-ish token and
/// compare it to `package` after PEP 503 normalisation.
fn dep_name_matches(spec: &str, package: &str) -> bool {
    let name_end = spec
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != '.')
        .unwrap_or(spec.len());
    normalize_dist_name(&spec[..name_end]) == normalize_dist_name(package)
}

/// PEP 503 name normalisation, a direct port of `packaging.utils.canonicalize_name`
/// (which pip vendors).
fn normalize_dist_name(name: &str) -> String {
    let mut value = name.to_lowercase().replace(['_', '.'], "-");
    while value.contains("--") {
        value = value.replace("--", "-");
    }
    value
}

fn venv_path(run: &Invocation, quiet: bool) -> Result<ExitCode> {
    let repo_root = run.project_root()?;
    trust_root(&repo_root, quiet)?;
    let resolved = toolr_core::venv::resolve_venv_path(&repo_root)?;
    println!("{}", resolved.venv_dir.display());
    Ok(ExitCode::SUCCESS)
}

fn venv_shell(run: &Invocation, quiet: bool) -> Result<ExitCode> {
    use std::process::Command;

    let (resolved, _) = ready_venv(&run.cwd, quiet, |o| o)?;

    let shell = default_shell_path();
    let bin_dir = venv_bin_dir(&resolved.venv_dir);
    let prepended_path = prepend_to_path(&bin_dir, std::env::var_os("PATH").as_deref())?;

    let status = Command::new(&shell)
        .env("VIRTUAL_ENV", &resolved.venv_dir)
        .env("PATH", &prepended_path)
        // Help shell prompts notice the activation.
        .env("TOOLR_VENV", &resolved.venv_dir)
        .status()?;
    Ok(ExitCode::from(process_exit_code(status)))
}

fn venv_run(matches: &ArgMatches, run: &Invocation, quiet: bool) -> Result<ExitCode> {
    let ctx = ProjectCtx::new(run);
    let no_sync = matches.get_flag("no-sync");
    let argv: Vec<String> = matches
        .get_many::<String>("command")
        .expect("clap marks command required")
        .cloned()
        .collect();
    // `venv run` takes the command with hyphen values allowed, so a toolr flag put after it
    // (`venv run -q pytest`) reaches here as the command. Say where the root `-q` goes instead
    // of running it.
    // The passive cache hint has already run by now; it is advice on stderr and harmless here.
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    if let Some(m) = crate::cli::misplaced_flag_error(&words, "project venv run", false) {
        anyhow::bail!("{}", m.message());
    }

    let resolved = if no_sync {
        // Pure path: never touch the venv. Resolve, gate on freshness,
        // validate, then run. Needs no uv / consent / network.
        let repo_root = ctx.repo_root()?;
        trust_root(&repo_root, quiet)?;
        let resolved = toolr_core::venv::resolve_venv_path(&repo_root)
            .context("resolving the tools venv path")?;
        let tools = repo_root.join("tools");
        match toolr_core::venv::check_freshness(&resolved, &tools) {
            toolr_core::venv::Freshness::Missing => anyhow::bail!(
                "the tools venv hasn't been created yet — run `toolr project venv sync`"
            ),
            toolr_core::venv::Freshness::Stale => anyhow::bail!(
                "the tools venv is out of date with {} — run `toolr project venv sync` (or drop --no-sync)",
                toolr_core::venv::SYNC_INPUTS_DESCRIPTION
            ),
            toolr_core::venv::Freshness::Broken => anyhow::bail!(
                "the tools venv's Python interpreter no longer exists (was it uninstalled?) — run `toolr project venv sync` to rebuild it (or drop --no-sync)"
            ),
            toolr_core::venv::Freshness::Foreign => anyhow::bail!(
                "{} (or drop --no-sync)",
                crate::build_manifest_resolve::foreign_venv_hint(&resolved.venv_dir)
            ),
            toolr_core::venv::Freshness::Fresh => {}
        }
        toolr_core::venv::validate_venv(&resolved.venv_dir, &resolved.python)
            .map_err(|e| anyhow::anyhow!("validating the tools venv: {e}"))?;
        resolved
    } else {
        // Default: freshness-gated auto-sync (same path as `venv sync`,
        // and as `venv shell`), then run. `--quiet` forwards to uv's
        // --quiet inside the sync.
        ready_venv(ctx.cwd, quiet, |o| o)?.0
    };

    run_command_in_venv(&resolved.venv_dir, &argv)
}

/// Activate `venv_dir` (VIRTUAL_ENV + TOOLR_VENV + PATH prepend) and run
/// `argv` in it, inheriting stdio and passing the child's exit code
/// through. A not-found spawn error becomes an actionable nudge (exit
/// 127) instead of a raw OS error. No command echo — the child owns
/// stdout/stderr (parity with `uv run`).
fn run_command_in_venv(venv_dir: &Path, argv: &[String]) -> Result<ExitCode> {
    use std::process::Command;

    let bin_dir = venv_bin_dir(venv_dir);
    let prepended_path = prepend_to_path(&bin_dir, std::env::var_os("PATH").as_deref())?;
    let (program, rest) = argv
        .split_first()
        .expect("clap enforces at least one command token");

    let result = Command::new(program) // nosemgrep: rust.actix.command-injection.rust-actix-command-injection.rust-actix-command-injection
        .args(rest)
        .env("VIRTUAL_ENV", venv_dir)
        .env("TOOLR_VENV", venv_dir)
        .env("PATH", &prepended_path)
        .status();

    match result {
        Ok(status) => Ok(ExitCode::from(process_exit_code(status))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("toolr: couldn't find `{program}` in the tools venv.");
            eprintln!(
                "hint: did you forget to add it to tools/pyproject.toml (then `toolr project venv sync`)?"
            );
            Ok(ExitCode::from(127))
        }
        Err(e) => {
            Err(anyhow::Error::new(e).context(format!("spawning `{program}` in the tools venv")))
        }
    }
}

/// Resolve the shell binary to spawn for `toolr project venv shell`.
/// Honours `$SHELL`, falling back to a per-OS default. Extracted as a
/// pure helper so the fallback arms are unit-testable.
fn default_shell_path() -> std::path::PathBuf {
    std::env::var_os("SHELL")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(windows) {
                std::path::PathBuf::from("cmd.exe")
            } else {
                std::path::PathBuf::from("/bin/sh")
            }
        })
}

/// Resolve the `bin` (Unix) or `Scripts` (Windows) sub-directory of a
/// venv. Extracted so `venv_shell`'s PATH-prepend logic is testable.
fn venv_bin_dir(venv_dir: &std::path::Path) -> std::path::PathBuf {
    if cfg!(windows) {
        venv_dir.join("Scripts")
    } else {
        venv_dir.join("bin")
    }
}

/// Prepend `dir` to a colon-separated PATH-style value. When `existing`
/// is `None`, returns just `dir` as an `OsString`. Surfaces
/// `join_paths` errors via `?`.
fn prepend_to_path(
    dir: &std::path::Path,
    existing: Option<&std::ffi::OsStr>,
) -> Result<std::ffi::OsString> {
    match existing {
        Some(existing) => {
            let mut paths: Vec<_> = std::env::split_paths(existing).collect();
            paths.insert(0, dir.to_path_buf());
            Ok(std::env::join_paths(paths)?)
        }
        None => Ok(dir.as_os_str().to_os_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_templates::{ScaffoldOptions, VenvLocation};
    use std::fs;
    use tempfile::TempDir;

    /// Mutating SHELL / PATH and observing their effects requires
    /// process-wide env coordination — serialise so cargo's parallel
    /// runner doesn't race.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_env_var<R>(key: &str, value: Option<&str>, f: impl FnOnce() -> R) -> R {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os(key);
        // SAFETY: ENV_LOCK serialises every test in this module that
        // touches env vars.
        unsafe {
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        let r = f();
        unsafe {
            match prev {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
        r
    }

    /// A trust store outside the scaffolded repo, so tests never touch the
    /// real one.
    struct TrustDir(TempDir);

    impl TrustDir {
        fn new() -> Self {
            Self(TempDir::new().unwrap())
        }
        fn store(&self) -> TrustStore {
            TrustStore::at(self.0.path().join("trust"))
        }
    }

    fn init_opts() -> ScaffoldOptions {
        ScaffoldOptions {
            requires_python: ">=3.11".into(),
            venv_location: VenvLocation::Cache,
            include_example: true,
        }
    }

    /// run_project_init with --yes auto-approves all conflicts.
    #[test]
    fn run_project_init_yes_all_overwrites_conflicts() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("pyproject.toml"), "# stale").unwrap();
        let store = TrustDir::new();

        let opts = init_opts();
        run_project_init(
            tmp.path(),
            &opts,
            /*force=*/ false,
            /*no_sync=*/ true,
            /*quiet=*/ true,
            /*yes_all=*/ true,
            Some(&store.store()),
            &mut |_| unreachable!("confirm should not be called when yes_all=true"),
        )
        .unwrap();

        let content = fs::read_to_string(tools.join("pyproject.toml")).unwrap();
        assert!(content.contains(r#"name = "tools""#));
        assert!(store.store().is_trusted(tmp.path()).unwrap());
    }

    /// run_project_init with a confirm closure that says "no" leaves conflicts intact.
    #[test]
    fn run_project_init_confirm_no_preserves_conflicts() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("pyproject.toml"), "# stale").unwrap();
        let store = TrustDir::new();

        let opts = init_opts();
        run_project_init(
            tmp.path(),
            &opts,
            /*force=*/ false,
            /*no_sync=*/ true,
            /*quiet=*/ true,
            /*yes_all=*/ false,
            Some(&store.store()),
            &mut |_| Ok(false), // always decline
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(tools.join("pyproject.toml")).unwrap(),
            "# stale"
        );
    }

    /// run_project_init with a confirm closure records which files were approved.
    #[test]
    fn run_project_init_confirm_selectively_overwrites() {
        let tmp = TempDir::new().unwrap();
        let tools = tmp.path().join("tools");
        fs::create_dir(&tools).unwrap();
        fs::write(tools.join("pyproject.toml"), "# stale-pyproject").unwrap();
        fs::write(tools.join(".gitignore"), "# stale-gitignore").unwrap();
        let store = TrustDir::new();

        let opts = init_opts();
        run_project_init(
            tmp.path(),
            &opts,
            /*force=*/ false,
            /*no_sync=*/ true,
            /*quiet=*/ true,
            /*yes_all=*/ false,
            Some(&store.store()),
            &mut |dest| {
                // Approve only pyproject.toml.
                Ok(dest.ends_with("pyproject.toml"))
            },
        )
        .unwrap();

        let pyproject = fs::read_to_string(tools.join("pyproject.toml")).unwrap();
        assert!(
            pyproject.contains(r#"name = "tools""#),
            "should be overwritten"
        );
        assert_eq!(
            fs::read_to_string(tools.join(".gitignore")).unwrap(),
            "# stale-gitignore",
            ".gitignore should be preserved"
        );
    }

    #[test]
    fn default_shell_path_uses_shell_env_when_set() {
        let p = with_env_var("SHELL", Some("/custom/sh"), default_shell_path);
        assert_eq!(p, std::path::PathBuf::from("/custom/sh"));
    }

    #[test]
    fn default_shell_path_falls_back_to_per_os_default() {
        let p = with_env_var("SHELL", None, default_shell_path);
        if cfg!(windows) {
            assert_eq!(p, std::path::PathBuf::from("cmd.exe"));
        } else {
            assert_eq!(p, std::path::PathBuf::from("/bin/sh"));
        }
    }

    #[test]
    fn venv_bin_dir_picks_per_os_subdirectory() {
        let venv = std::path::Path::new("/tmp/venv");
        let bin = venv_bin_dir(venv);
        if cfg!(windows) {
            assert_eq!(bin, venv.join("Scripts"));
        } else {
            assert_eq!(bin, venv.join("bin"));
        }
    }

    #[test]
    fn prepend_to_path_with_existing_inserts_at_front() {
        let bin = std::path::Path::new("/venv/bin");
        let existing = if cfg!(windows) {
            "C:\\Windows;C:\\Windows\\System32"
        } else {
            "/usr/bin:/bin"
        };
        let result = prepend_to_path(bin, Some(std::ffi::OsStr::new(existing))).unwrap();
        let paths: Vec<_> = std::env::split_paths(&result).collect();
        assert_eq!(paths[0], bin);
        assert!(paths.len() >= 2);
    }

    #[test]
    fn prepend_to_path_with_no_existing_returns_bin_alone() {
        let bin = std::path::Path::new("/venv/bin");
        let result = prepend_to_path(bin, None).unwrap();
        assert_eq!(result, bin.as_os_str());
    }

    #[test]
    fn dep_name_matches_strips_version_extras_and_markers() {
        assert!(dep_name_matches("toolr-py", "toolr-py"));
        assert!(dep_name_matches("toolr-py>=0.11", "toolr-py"));
        assert!(dep_name_matches("toolr-py[extra]", "toolr-py"));
        assert!(dep_name_matches(
            "toolr-py==0.11.2; python_version < \"3.13\"",
            "toolr-py"
        ));
        assert!(!dep_name_matches("other", "toolr-py"));
    }

    /// Expected values come from `packaging.utils.canonicalize_name` (26.3).
    #[test]
    fn normalize_dist_name_matches_packaging_canonicalize_name() {
        for (name, expected) in [
            ("toolr_py", "toolr-py"),
            ("Toolr.Py", "toolr-py"),
            ("toolr--py", "toolr-py"),
            ("toolr-_.py", "toolr-py"),
            ("TOOLR-PY", "toolr-py"),
            // Full Unicode lowercasing like Python's `str.lower()`: `package`
            // comes from the CLI unfiltered, so ASCII-only would diverge.
            ("Ünicode", "ünicode"),
            ("İstanbul", "i\u{307}stanbul"),
            ("_toolr_", "-toolr-"),
            ("toolrpy", "toolrpy"),
        ] {
            assert_eq!(normalize_dist_name(name), expected, "{name:?}");
        }
    }

    #[test]
    fn dep_name_matches_normalises_separators_per_pep_503() {
        assert!(dep_name_matches("Toolr-Py>=0.11", "toolr-py"));
        assert!(dep_name_matches("toolr_py", "toolr-py"));
        assert!(dep_name_matches("Toolr.Py>=1", "toolr-py"));
        assert!(dep_name_matches("toolr--py", "toolr_py"));
        assert!(!dep_name_matches("toolrpy", "toolr-py"));
    }

    #[test]
    fn pyproject_declares_dependency_finds_direct_dep() {
        let tmp = TempDir::new().unwrap();
        let pyproject = tmp.path().join("pyproject.toml");
        fs::write(
            &pyproject,
            r#"[project]
name = "tools"
dependencies = [
    "toolr-py>=0.11",
    "requests",
]
"#,
        )
        .unwrap();
        assert!(pyproject_declares_dependency(&pyproject, "toolr-py").unwrap());
        assert!(pyproject_declares_dependency(&pyproject, "requests").unwrap());
        assert!(!pyproject_declares_dependency(&pyproject, "absent").unwrap());
    }

    #[test]
    fn pyproject_declares_dependency_finds_optional_dep() {
        let tmp = TempDir::new().unwrap();
        let pyproject = tmp.path().join("pyproject.toml");
        fs::write(
            &pyproject,
            r#"[project]
name = "tools"
dependencies = []

[project.optional-dependencies]
dev = ["pytest", "ruff"]
"#,
        )
        .unwrap();
        assert!(pyproject_declares_dependency(&pyproject, "pytest").unwrap());
        assert!(pyproject_declares_dependency(&pyproject, "ruff").unwrap());
        assert!(!pyproject_declares_dependency(&pyproject, "absent").unwrap());
    }

    #[test]
    fn detect_requires_python_reads_the_real_python3() {
        // Probed separately so a broken probe script can't hide behind the
        // floor fallback.
        let floor = toolr_core::execute::python::MIN_PYTHON_MINOR;
        let detected = detect_requires_python();
        let minor: u32 = detected
            .strip_prefix(">=3.")
            .and_then(|m| m.parse().ok())
            .unwrap_or_else(|| panic!("not a >=3.N floor: {detected}"));
        assert!(minor >= floor, "{detected} is below the floor");
        let real = std::process::Command::new("python3")
            .args(["-c", "import sys; print(sys.version_info.minor)"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u32>()
                    .ok()
            });
        let real = real.filter(|&m| m >= floor).unwrap_or(minor);
        assert_eq!(minor, real, "{detected} doesn't match python3 3.{real}");
    }

    #[test]
    fn requires_python_falls_back_to_the_supported_floor() {
        let s = requires_python_from(Some(Path::new("definitely-not-a-real-python-toolr-test")));
        assert_eq!(
            s,
            format!(">=3.{}", toolr_core::execute::python::MIN_PYTHON_MINOR)
        );
    }

    #[test]
    #[cfg(unix)]
    fn requires_python_raises_a_too_old_interpreter_to_the_supported_floor() {
        let bin = tempfile::TempDir::new().unwrap();
        let python = stub_python(bin.path(), "3.9.18");
        assert_eq!(
            requires_python_from(Some(&python)),
            format!(">=3.{}", toolr_core::execute::python::MIN_PYTHON_MINOR)
        );
    }

    #[test]
    #[cfg(unix)]
    fn requires_python_keeps_a_newer_interpreter_version() {
        let bin = tempfile::TempDir::new().unwrap();
        let python = stub_python(bin.path(), "3.99.0");
        assert_eq!(requires_python_from(Some(&python)), ">=3.99");
    }

    /// A `python` that reports `Python {version}` for whatever it is asked to run.
    #[cfg(unix)]
    fn stub_python(dir: &std::path::Path, version: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("python");
        std::fs::write(&path, format!("#!/bin/sh\necho 'Python {version}'\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn build_upgrade_mode_none_when_nothing_set() {
        use toolr_core::venv::UpgradeMode;
        let m = build_upgrade_mode(false, vec![]);
        assert!(matches!(m, UpgradeMode::None));
    }

    #[test]
    fn build_upgrade_mode_all_when_upgrade_flag_set() {
        use toolr_core::venv::UpgradeMode;
        let m = build_upgrade_mode(true, vec![]);
        assert!(matches!(m, UpgradeMode::All));
    }

    #[test]
    fn build_upgrade_mode_all_wins_over_packages_when_both_set() {
        use toolr_core::venv::UpgradeMode;
        let m = build_upgrade_mode(true, vec!["foo".into()]);
        assert!(matches!(m, UpgradeMode::All));
    }

    #[test]
    fn build_upgrade_mode_packages_when_only_dash_p_set() {
        use toolr_core::venv::UpgradeMode;
        let m = build_upgrade_mode(false, vec!["foo".into(), "bar".into()]);
        match m {
            UpgradeMode::Packages(p) => assert_eq!(p, vec!["foo".to_string(), "bar".to_string()]),
            other => panic!("expected Packages, got {other:?}"),
        }
    }
}
