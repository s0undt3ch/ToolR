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
    let completion = || clap::Command::new("completion").subcommand(clap::Command::new("other"));
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
