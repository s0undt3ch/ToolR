//! What one run of toolr reads before and around clap: its argv, split where the leading run of
//! root flags ends, and the project root, each worked out once.

use std::cell::OnceCell;
use std::path::{Path, PathBuf};

use clap::ArgAction;
use toolr_core::discovery::{DiscoveryError, discover_project_root};

use crate::cli::spellings;
use crate::help::HelpMode;

/// The process argv and where its leading run of root flags ends. Root flags are not global,
/// so only that run is toolr's; the flag table and built-in names come from `cli`.
pub(crate) struct RootArgv {
    words: Vec<String>,
    /// Index of the first word that is neither a root flag nor a root option's value.
    command_at: usize,
}

impl RootArgv {
    /// Root options that take a value must have no short spelling: `takes_value` matches whole
    /// words, so `-qt 5` or `-t5` would leave the value as the command word. Handle clusters
    /// here before giving one a short flag.
    pub(crate) fn new(words: Vec<String>) -> Self {
        let root = crate::cli::root_command();
        let takes_value = |word: &str| {
            root.get_arguments()
                .any(|a| a.get_action().takes_values() && spellings(a).iter().any(|s| s == word))
        };
        let mut first = 1;
        // Like clap, stop at a `--` separator: nothing after it is a root flag or command.
        while let Some(word) = words
            .get(first)
            .filter(|w| w.starts_with('-') && w.as_str() != "--")
        {
            first += if takes_value(word) { 2 } else { 1 };
        }
        let command_at = first.min(words.len());
        Self { words, command_at }
    }

    /// The whole argv, `toolr` included, as clap gets it.
    pub(crate) fn words(&self) -> &[String] {
        &self.words
    }

    fn first_word(&self) -> Option<&str> {
        self.words.get(self.command_at).map(String::as_str)
    }

    fn is_builtin(&self) -> bool {
        self.first_word()
            .is_some_and(|w| crate::cli::builtin_names().iter().any(|b| b == w))
    }

    /// Whether any root flag comes before the first command word.
    pub(crate) fn has_leading_root_flags(&self) -> bool {
        self.command_at > 1
    }

    /// `--version` or `-V` (alone or in a short cluster) among the root flags: clap prints the
    /// version and exits.
    pub(crate) fn wants_version(&self) -> bool {
        let version = crate::cli::root_command()
            .get_arguments()
            .find(|a| matches!(a.get_action(), ArgAction::Version))
            .expect("the root declares --version");
        let short = version.get_short();
        self.words[1..self.command_at].iter().any(|w| {
            spellings(version).contains(w)
                || w.strip_prefix('-')
                    .filter(|c| !c.starts_with('-'))
                    .is_some_and(|c| c.chars().any(|ch| Some(ch) == short))
        })
    }

    /// The run needs no user manifest: `--version`, or one of toolr's own commands, which
    /// manage their own state. Bare `toolr` and `--help` build it, so help shows user commands.
    /// The build runs no repository code, so this is latency only, never a safety gate.
    pub(crate) fn skips_manifest_build(&self) -> bool {
        self.wants_version() || self.is_builtin()
    }

    /// The first command word is not one of toolr's own, so only the manifest can supply it.
    pub(crate) fn names_user_command(&self) -> bool {
        self.first_word().is_some() && !self.is_builtin()
    }

    /// A shell's Tab request (`toolr __complete …`).
    pub(crate) fn is_tab_completion(&self) -> bool {
        self.first_word() == Some("__complete")
    }

    /// The help form asked for before any `--`: `--help` wins over `-h`.
    pub(crate) fn help_mode(&self) -> Option<HelpMode> {
        let words: Vec<&str> = self
            .words
            .iter()
            .skip(1)
            .map(String::as_str)
            .take_while(|w| *w != "--")
            .collect();
        if words.contains(&"--help") {
            Some(HelpMode::Long)
        } else {
            words.contains(&"-h").then_some(HelpMode::Short)
        }
    }

    /// The user's words after `__complete <cwd>`, read from argv because clap drops a bare `--`
    /// that the shell passes through as the word being completed.
    pub(crate) fn complete_tokens(&self) -> &[String] {
        self.words.get(self.command_at + 2..).unwrap_or_default()
    }

    /// The subcommands clap matches from the first command word, and the deepest one's command.
    pub(crate) fn command_path<'c>(
        &self,
        root: &'c clap::Command,
    ) -> (&'c clap::Command, Vec<&str>) {
        let mut cmd = root;
        let mut names = Vec::new();
        for word in self.words.get(self.command_at..).unwrap_or_default() {
            let Some(sub) = cmd.find_subcommand(word) else {
                break;
            };
            cmd = sub;
            names.push(word.as_str());
        }
        (cmd, names)
    }
}

/// One run: its argv, its working directory and the project root above it.
pub(crate) struct Invocation {
    pub(crate) argv: RootArgv,
    pub(crate) cwd: PathBuf,
    /// Looked up on first use, so Tab completion and `--version` never walk the tree.
    root: OnceCell<Result<PathBuf, DiscoveryError>>,
}

impl Invocation {
    pub(crate) fn new(argv: RootArgv, cwd: PathBuf) -> Self {
        Self {
            argv,
            cwd,
            root: OnceCell::new(),
        }
    }

    fn resolved(&self) -> &Result<PathBuf, DiscoveryError> {
        self.root.get_or_init(|| discover_project_root(&self.cwd))
    }

    /// The project root, or why there is none.
    pub(crate) fn root(&self) -> Result<&Path, &DiscoveryError> {
        self.resolved().as_ref().map(PathBuf::as_path)
    }

    /// The working directory and the owned lookup result: `?` on the owned error keeps its source chain.
    pub(crate) fn into_cwd_and_root(self) -> (PathBuf, Result<PathBuf, DiscoveryError>) {
        self.resolved();
        let root = self.root.into_inner().expect("resolved above");
        (self.cwd, root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> RootArgv {
        RootArgv::new(
            std::iter::once("toolr")
                .chain(args.iter().copied())
                .map(String::from)
                .collect(),
        )
    }

    #[test]
    fn no_root_option_that_takes_a_value_has_a_short_flag() {
        for arg in crate::cli::root_command().get_arguments() {
            assert!(
                !(arg.get_action().takes_values() && arg.get_short().is_some()),
                "`{}` takes a value and has a short flag; RootArgv::new, `cli::root_flag_spelling` and \
                 the completion scan read whole words and must learn short clusters (`-qt 5`) and \
                 attached values (`-t5`) first",
                arg.get_id()
            );
        }
    }

    #[test]
    fn nothing_after_the_separator_is_classified() {
        for args in [&["--", "self"][..], &["--", "-V"], &["--", "__complete"]] {
            let a = argv(args);
            assert!(!a.skips_manifest_build(), "{args:?}");
            assert!(!a.is_tab_completion() && !a.wants_version(), "{args:?}");
        }
        assert_eq!(argv(&["-q", "--", "self"]).first_word(), Some("--"));
    }

    #[test]
    fn root_option_values_are_not_the_command() {
        for args in [
            &["--timeout-secs", "5", "self", "--help"][..],
            &["--timeout", "5", "--nots", "7", "self"][..],
            &["--no-output-timeout-secs", "5", "-d", "self"][..],
            &["--timeout-secs=5", "self"][..],
        ] {
            let a = argv(args);
            assert_eq!(a.first_word(), Some("self"), "{args:?}");
            assert!(
                a.skips_manifest_build() && a.has_leading_root_flags(),
                "{args:?}"
            );
        }
        assert_eq!(argv(&["--timeout-secs"]).first_word(), None);
        assert!(!argv(&["self"]).has_leading_root_flags());
    }

    #[test]
    fn every_built_in_skips_the_build_and_names_no_user_command() {
        for name in ["project", "self", "__complete", "__install-uv-now"] {
            let a = argv(&[name, "x"]);
            assert!(
                a.skips_manifest_build() && !a.names_user_command(),
                "{name}"
            );
        }
    }

    #[test]
    fn help_bare_toolr_and_user_commands_build_the_manifest() {
        for args in [
            &["--help"][..],
            &["-h"],
            &[],
            &["init", "hello"],
            &["--debug", "django", "migrate"],
            &["rel", "bump", "--version", "1.2"],
            &["rel", "-qV"],
        ] {
            assert!(!argv(args).skips_manifest_build(), "{args:?}");
        }
        assert!(argv(&["init", "hello"]).names_user_command());
        assert!(!argv(&["-q"]).names_user_command());
    }

    #[test]
    fn version_counts_only_among_the_root_flags() {
        for args in [
            &["--version"][..],
            &["-V"],
            &["-q", "-V"],
            &["-qV"],
            &["-V", "ci"],
        ] {
            assert!(argv(args).skips_manifest_build(), "{args:?}");
        }
    }

    #[test]
    fn tab_completion_is_the_first_command_word() {
        let a = argv(&["-q", "__complete", "/tmp", "--", "ci", ""]);
        assert!(a.is_tab_completion());
        assert_eq!(a.complete_tokens(), ["--", "ci", ""]);
        assert!(!argv(&["project", "__complete"]).is_tab_completion());
        assert!(argv(&["__complete"]).complete_tokens().is_empty());
    }

    #[test]
    fn long_help_wins_and_nothing_after_the_separator_counts() {
        assert_eq!(
            argv(&["ci", "-h", "--help"]).help_mode(),
            Some(HelpMode::Long)
        );
        assert_eq!(argv(&["ci", "-h"]).help_mode(), Some(HelpMode::Short));
        assert_eq!(argv(&["run", "--", "pytest", "--help"]).help_mode(), None);
        assert_eq!(argv(&["ci"]).help_mode(), None);
    }

    #[test]
    fn command_path_walks_from_the_first_command_word() {
        let root = crate::cli::build_command(&toolr_core::manifest::Manifest::empty());
        let a = argv(&["-q", "project", "venv", "sync", "--force"]);
        let (cmd, names) = a.command_path(&root);
        assert_eq!(names, ["project", "venv", "sync"]);
        assert_eq!(cmd.get_name(), "sync");
        let a = argv(&["--timeout-secs", "5"]);
        let (cmd, names) = a.command_path(&root);
        assert!(names.is_empty());
        assert_eq!(cmd.get_name(), "toolr");
    }

    #[test]
    fn the_root_is_looked_up_once_and_handed_over_owned() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("tools")).unwrap();
        std::fs::write(tmp.path().join("tools/pyproject.toml"), "").unwrap();
        let run = Invocation::new(argv(&[]), tmp.path().to_path_buf());
        let found = run.root().unwrap().to_path_buf();
        std::fs::remove_dir_all(tmp.path().join("tools")).unwrap();
        // The cached answer stands even though the project is gone now.
        assert_eq!(run.root().unwrap(), found);
        let (cwd, root) = run.into_cwd_and_root();
        assert_eq!((cwd.as_path(), root.unwrap()), (tmp.path(), found));

        let none = tempfile::TempDir::new().unwrap();
        let run = Invocation::new(argv(&[]), none.path().to_path_buf());
        assert!(run.into_cwd_and_root().1.is_err());
    }
}
