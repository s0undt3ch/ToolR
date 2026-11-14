//! `toolr self …` and the hidden `__install-uv-now`.

use std::path::PathBuf;
use std::process::ExitCode;

use toolr_core::complete::{
    InstallOptions, InstallOutcome, PriorState, Shell as CompletionShell,
    bash_completion_installed, completion_script, install_script, powershell_quote, shell_quote,
};
use toolr_core::env::{non_empty_env_path, xdg_dir_env};
use toolr_core::trust::shown;

use crate::invocation::Invocation;

pub(super) fn run_self(
    matches: &clap::ArgMatches,
    run: &Invocation,
    quiet: bool,
) -> anyhow::Result<ExitCode> {
    match matches.subcommand() {
        Some(("build-manifest", bm_matches)) => run_self_build_manifest(bm_matches, run, quiet),
        Some(("cache", cache_matches)) => crate::self_cache::dispatch(cache_matches, quiet),
        Some(("trusted", trusted_matches)) => run_self_trusted(trusted_matches, quiet),
        Some(("completion", completion_matches)) => {
            let Some((action, action_matches)) = completion_matches.subcommand() else {
                anyhow::bail!("expected a `self completion` subcommand");
            };
            match action {
                "print" => run_completion_print(action_matches),
                "install" => run_completion_install(action_matches, quiet),
                other => anyhow::bail!("unsupported self completion subcommand: {other}"),
            }
        }
        _ => anyhow::bail!("expected a `self` subcommand"),
    }
}

/// List trusted roots, one per line, marking those that no longer exist.
/// `--prune` drops the missing ones, and venv entries whose venv or record is
/// gone, and lists what's left. The listing is the result, so `--quiet` only
/// drops the `pruned` notes.
fn run_self_trusted(matches: &clap::ArgMatches, quiet: bool) -> anyhow::Result<ExitCode> {
    let store = toolr_core::trust::TrustStore::open_default()?;
    let prune = matches.get_flag("prune");
    for record in store.list()? {
        let repo = &record.repo_path;
        if record.key.exists() {
            println!("{}", shown(repo));
        } else if prune {
            if store.untrust_key(&record.key)? && !quiet {
                eprintln!("toolr: pruned {}", shown(repo));
            }
        } else {
            println!("{} (missing)", shown(repo));
        }
    }
    if prune {
        store.prune_venv_entries()?;
    }
    Ok(ExitCode::SUCCESS)
}

fn run_self_build_manifest(
    matches: &clap::ArgMatches,
    run: &Invocation,
    quiet: bool,
) -> anyhow::Result<ExitCode> {
    let resolved = crate::build_manifest_resolve::resolve_source_and_package(matches, run, quiet)?;

    let output_path = resolve_output_path(matches, &resolved.source_dir);

    let built = toolr_core::build_fragment::build_third_party_fragment(
        &resolved.source_dir,
        &resolved.package_name,
    )?;
    let fragment = built.fragment;
    for warning in &built.warnings {
        eprintln!(
            "toolr: warning: {}",
            toolr_core::text::strip_control_chars(warning)
        );
    }
    let serialised = toolr_core::build_fragment::serialise_fragment(&fragment)?;

    if matches.get_flag("check") {
        return check_against_disk(&output_path, &serialised);
    }

    write_atomically(&output_path, &serialised)?;
    if !quiet {
        eprintln!(
            "toolr build-manifest: wrote {} group(s) / {} command(s) to {}",
            fragment.groups.len(),
            fragment.commands.len(),
            shown(&output_path),
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn resolve_output_path(matches: &clap::ArgMatches, source_dir: &std::path::Path) -> PathBuf {
    matches
        .get_one::<String>("output")
        .map(PathBuf::from)
        .unwrap_or_else(|| source_dir.join("toolr-manifest.json"))
}

fn write_atomically(path: &std::path::Path, contents: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents.as_bytes())?;
    tmp.persist(path)
        .map_err(|e| anyhow::anyhow!("persist: {e}"))?;
    Ok(())
}

fn check_against_disk(path: &std::path::Path, serialised: &str) -> anyhow::Result<ExitCode> {
    let existing = if path.is_file() {
        std::fs::read_to_string(path)? // nosemgrep: rust.actix.path-traversal.tainted-path.tainted-path
    } else {
        String::new()
    };
    if existing == serialised {
        Ok(ExitCode::SUCCESS)
    } else {
        let diff = similar::TextDiff::from_lines(existing.as_str(), serialised);
        eprintln!(
            "toolr build-manifest: {} is out of date - regenerate with `toolr self build-manifest <pkg>`",
            shown(path),
        );
        eprintln!("{}", diff.unified_diff().header("on-disk", "regenerated"));
        Ok(ExitCode::from(2))
    }
}

fn run_completion_install(matches: &clap::ArgMatches, quiet: bool) -> anyhow::Result<ExitCode> {
    let shell_str = matches
        .get_one::<String>("shell")
        .ok_or_else(|| anyhow::anyhow!("missing <shell>"))?;
    let shell: CompletionShell = shell_str.parse()?;
    let force = matches.get_flag("force");

    let home = dirs_home()?;
    let xdg_data_home = xdg_dir_env("XDG_DATA_HOME");
    let xdg_config_home = xdg_dir_env("XDG_CONFIG_HOME");
    let opts = InstallOptions {
        shell,
        xdg_data_home,
        xdg_config_home,
        home,
        data_dir: dirs::data_dir(),
        force,
        interactive: std::io::IsTerminal::is_terminal(&std::io::stdin()),
    };

    let outcome = install_script(&opts)?;
    match outcome {
        InstallOutcome::Wrote { path, prior } => {
            // --quiet drops success lines and advice (the zsh reminder, the bash hint),
            // never refusals or errors.
            if !quiet {
                let msg = match prior {
                    PriorState::None => format!(
                        "toolr: wrote {} completion script to {}",
                        shell,
                        path.display()
                    ),
                    PriorState::Identical => format!(
                        "toolr: rewrote {} completion script at {} (--force; content unchanged)",
                        shell,
                        path.display()
                    ),
                    PriorState::Differed => format!(
                        "toolr: replaced existing {} completion script at {}",
                        shell,
                        path.display()
                    ),
                };
                println!("{msg}");
                if matches!(shell, CompletionShell::Zsh) {
                    println!(
                        "toolr: ensure your ~/.zshrc includes `fpath=(~/.zfunc $fpath)` and \
                         `autoload -Uz compinit && compinit`."
                    );
                }
                print_bash_source_hint(shell, &path);
                print_powershell_profile_hint(shell, &path);
            }
            Ok(ExitCode::SUCCESS)
        }
        InstallOutcome::AlreadyInstalled { path } => {
            if !quiet {
                println!(
                    "toolr: {} completion already up to date at {}",
                    shell,
                    path.display()
                );
                print_bash_source_hint(shell, &path);
                print_powershell_profile_hint(shell, &path);
            }
            Ok(ExitCode::SUCCESS)
        }
        InstallOutcome::SkippedNeedsForce { path } => {
            eprintln!(
                "toolr: refusing to overwrite {} (use --force to replace)",
                path.display()
            );
            Ok(ExitCode::from(1))
        }
    }
}

/// Only bash-completion loads the bash install directory; without it, tell
/// the user to source the script themselves.
fn print_bash_source_hint(shell: CompletionShell, path: &std::path::Path) {
    if !matches!(shell, CompletionShell::Bash) {
        return;
    }
    // Test seam: CI hosts ship bash-completion, so tests move the probe's root.
    let sysroot = non_empty_env_path("TOOLR_BASH_COMPLETION_SYSROOT").unwrap_or_else(|| "/".into());
    // Raw on purpose: bash_completion_installed already skips empty entries.
    let xdg_data_dirs = std::env::var_os("XDG_DATA_DIRS");
    let homebrew_prefix = non_empty_env_path("HOMEBREW_PREFIX");
    if !bash_completion_installed(
        &sysroot,
        xdg_data_dirs.as_deref(),
        homebrew_prefix.as_deref(),
    ) {
        println!("{}", bash_source_hint(path));
    }
}

/// A non-UTF-8 path can't be printed losslessly, so that hint names no path
/// rather than a wrong one.
fn bash_source_hint(path: &std::path::Path) -> String {
    let action = match path.to_str() {
        Some(path) => format!("add `source {}`", shell_quote(path)),
        None => "add a `source` line for the installed script".to_string(),
    };
    format!(
        "toolr: can't find bash-completion, so bash won't load this script by itself; \
         {action} to your bash startup file (~/.bashrc)."
    )
}

/// PowerShell has no completion directory, so the installed script loads only from a profile.
fn print_powershell_profile_hint(shell: CompletionShell, path: &std::path::Path) {
    if matches!(shell, CompletionShell::PowerShell) {
        println!("{}", powershell_profile_hint(path, cfg!(windows)));
    }
}

/// `windows` is a parameter, not `cfg!`, so both texts are tested on every host.
fn powershell_profile_hint(path: &std::path::Path, windows: bool) -> String {
    let action = match path.to_str() {
        Some(path) => format!("add `. {}`", powershell_quote(path)),
        None => "add a line that dot-sources the installed script".to_string(),
    };
    let policy = if windows {
        "; the execution policy must allow local scripts \
         (`Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`)"
    } else {
        ""
    };
    format!(
        "toolr: {action} to your PowerShell profile (`$PROFILE.CurrentUserAllHosts`) to load it{policy}."
    )
}

/// `$HOME` first, so Git Bash/MSYS users and the tests' `HOME` seam keep working; then the
/// platform home, which covers a stock Windows account where `HOME` is unset.
fn dirs_home() -> anyhow::Result<PathBuf> {
    resolve_home(non_empty_env_path("HOME"), dirs::home_dir)
}

fn resolve_home(
    env_home: Option<PathBuf>,
    fallback: impl FnOnce() -> Option<PathBuf>,
) -> anyhow::Result<PathBuf> {
    env_home.or_else(fallback).ok_or_else(|| {
        anyhow::anyhow!("cannot determine the home directory; cannot pick install path")
    })
}

fn run_completion_print(matches: &clap::ArgMatches) -> anyhow::Result<ExitCode> {
    let shell_str = matches
        .get_one::<String>("shell")
        .ok_or_else(|| anyhow::anyhow!("missing <shell>"))?;
    let shell: CompletionShell = shell_str.parse()?;
    print!("{}", completion_script(shell));
    Ok(ExitCode::SUCCESS)
}

pub(super) fn run_install_uv_now() -> anyhow::Result<std::process::ExitCode> {
    let consent = toolr_core::uv::install::ConsentMode {
        yes_flag: true,
        auto_sync_env: true,
        ..Default::default()
    };
    let uv = toolr_core::uv::ensure_uv(consent).map_err(toolr_core::uv::UvError::into_anyhow)?;
    println!(
        "toolr: uv {}.{}.{} ready at {} (source: {:?})",
        uv.version.0,
        uv.version.1,
        uv.version.2,
        uv.path.display(),
        uv.source,
    );
    Ok(std::process::ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invocation::RootArgv;

    /// `run_self` given matches clap would never produce for `self` (it requires a subcommand).
    fn self_error(cmd: clap::Command, args: &[&str]) -> String {
        let matches = cmd.no_binary_name(true).try_get_matches_from(args).unwrap();
        let run = Invocation::new(RootArgv::new(vec!["toolr".into()]), std::env::temp_dir());
        run_self(&matches, &run, false).unwrap_err().to_string()
    }

    #[test]
    fn run_self_rejects_a_missing_or_unknown_subcommand() {
        let completion =
            || clap::Command::new("completion").subcommand(clap::Command::new("other"));
        let root = || clap::Command::new("self").subcommand(completion());
        assert_eq!(self_error(root(), &[]), "expected a `self` subcommand");
        assert_eq!(
            self_error(root(), &["completion"]),
            "expected a `self completion` subcommand"
        );
        assert_eq!(
            self_error(root(), &["completion", "other"]),
            "unsupported self completion subcommand: other"
        );
    }

    #[test]
    fn check_against_disk_reports_a_missing_file_as_out_of_date() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("toolr-manifest.json");
        assert_eq!(check_against_disk(&path, "{}").unwrap(), ExitCode::from(2));
        write_atomically(&path, "{}").unwrap();
        assert_eq!(check_against_disk(&path, "{}").unwrap(), ExitCode::SUCCESS);
    }

    #[test]
    fn bash_source_hint_names_the_path_and_bashrc() {
        let hint = bash_source_hint(std::path::Path::new(
            "/home/u/.local/share/bash-completion/completions/toolr",
        ));
        assert_eq!(
            hint,
            "toolr: can't find bash-completion, so bash won't load this script by itself; \
             add `source /home/u/.local/share/bash-completion/completions/toolr` \
             to your bash startup file (~/.bashrc)."
        );
    }

    #[cfg(unix)]
    #[test]
    fn bash_source_hint_names_no_path_when_it_is_not_utf8() {
        use std::os::unix::ffi::OsStrExt;
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/home/\xff/toolr"));
        assert_eq!(
            bash_source_hint(path),
            "toolr: can't find bash-completion, so bash won't load this script by itself; \
             add a `source` line for the installed script to your bash startup file (~/.bashrc)."
        );
    }

    // resolve_home takes $HOME and the platform fallback as inputs, so no test mutates the environment.
    #[test]
    fn resolve_home_prefers_env() {
        let home = resolve_home(Some("/env/home".into()), || Some("/platform".into())).unwrap();
        assert_eq!(home, PathBuf::from("/env/home"));
    }

    #[test]
    fn resolve_home_falls_back_when_env_missing() {
        let home = resolve_home(None, || Some("/platform".into())).unwrap();
        assert_eq!(home, PathBuf::from("/platform"));
    }

    #[test]
    fn resolve_home_errors_when_both_missing() {
        let err = resolve_home(None, || None).unwrap_err();
        assert!(
            err.to_string().contains("cannot pick install path"),
            "{err}"
        );
    }

    #[test]
    fn powershell_profile_hint_quotes_the_path() {
        assert_eq!(
            powershell_profile_hint(std::path::Path::new("/home/u/it's/toolr.ps1"), false),
            "toolr: add `. '/home/u/it''s/toolr.ps1'` to your PowerShell profile \
             (`$PROFILE.CurrentUserAllHosts`) to load it."
        );
    }

    #[test]
    fn powershell_profile_hint_on_windows_names_execution_policy() {
        assert_eq!(
            powershell_profile_hint(std::path::Path::new(r"C:\Users\u\toolr.ps1"), true),
            "toolr: add `. 'C:\\Users\\u\\toolr.ps1'` to your PowerShell profile \
             (`$PROFILE.CurrentUserAllHosts`) to load it; the execution policy must allow \
             local scripts (`Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`)."
        );
    }

    #[cfg(unix)]
    #[test]
    fn powershell_profile_hint_without_utf8_path_names_no_path() {
        use std::os::unix::ffi::OsStrExt;
        let path = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/home/\xff/toolr.ps1"));
        assert_eq!(
            powershell_profile_hint(path, false),
            "toolr: add a line that dot-sources the installed script to your PowerShell profile \
             (`$PROFILE.CurrentUserAllHosts`) to load it."
        );
    }
}
