//! What one run of toolr reads before and around clap: its argv, split where the leading run of
//! root flags ends, and the project root, each worked out once.

use std::cell::OnceCell;
use std::path::{Path, PathBuf};

use clap::ArgAction;
use toolr_core::discovery::{DiscoveryError, discover_project_root};
use toolr_core::project::NothingToSync;

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
        self.root_flag_given(version)
    }

    /// `-q` or `--quiet` among the root flags.
    pub(crate) fn is_quiet(&self) -> bool {
        let root = crate::cli::root_command();
        let quiet = root
            .get_arguments()
            .find(|a| a.get_id() == "quiet")
            .expect("the root declares --quiet");
        self.root_flag_given(quiet)
    }

    /// Whether `flag`, by any spelling or in a short cluster, comes before the command word.
    fn root_flag_given(&self, flag: &clap::Arg) -> bool {
        let short = flag.get_short();
        self.words[1..self.command_at].iter().any(|w| {
            spellings(flag).contains(w)
                || w.strip_prefix('-')
                    .filter(|c| !c.starts_with('-'))
                    .is_some_and(|c| c.chars().any(|ch| Some(ch) == short))
        })
    }

    /// No command word at all: a bare `toolr`, with or without root flags.
    pub(crate) fn is_bare(&self) -> bool {
        self.first_word().is_none()
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

    /// The help form asked for before any `--`: `--help` wins over `-h`, which also counts in
    /// a short cluster of root flags (`-qh`), never in a user command's own words.
    pub(crate) fn help_mode(&self) -> Option<HelpMode> {
        let words: Vec<&str> = self
            .words
            .iter()
            .skip(1)
            .map(String::as_str)
            .take_while(|w| *w != "--")
            .collect();
        let root_cluster = self.words[1..self.command_at].iter().any(|w| {
            w.strip_prefix('-')
                .is_some_and(|c| !c.starts_with('-') && c.contains('h'))
        });
        if words.contains(&"--help") {
            Some(HelpMode::Long)
        } else {
            (root_cluster || words.contains(&"-h")).then_some(HelpMode::Short)
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
    /// Whether the root is trusted, read from the trust store once per run.
    trusted: OnceCell<bool>,
}

impl Invocation {
    pub(crate) fn new(argv: RootArgv, cwd: PathBuf) -> Self {
        Self {
            argv,
            cwd,
            root: OnceCell::new(),
            trusted: OnceCell::new(),
        }
    }

    /// The discovered project root when it is not trusted: toolr reads nothing of such a
    /// repository.
    pub(crate) fn untrusted_root(&self) -> Option<&Path> {
        let root = self.root().ok()?;
        let trusted = *self
            .trusted
            .get_or_init(|| toolr_core::trust::is_trusted_now(root));
        (!trusted).then_some(root)
    }

    /// Prove the project root trusted, asking on a terminal unless `-q`, and remember it so no
    /// later step reads the store. `None` when there is no project.
    pub(crate) fn require_trust(&self) -> anyhow::Result<Option<toolr_core::trust::Trusted>> {
        let Ok(root) = self.root() else {
            return Ok(None);
        };
        let proof = toolr_core::trust::require_trust_quietly(root, self.argv.is_quiet())?;
        let _ = self.trusted.set(true);
        Ok(Some(proof))
    }

    fn resolved(&self) -> &Result<PathBuf, DiscoveryError> {
        self.root.get_or_init(|| discover_project_root(&self.cwd))
    }

    /// The project root, or why there is none.
    pub(crate) fn root(&self) -> Result<&Path, &DiscoveryError> {
        self.resolved().as_ref().map(PathBuf::as_path)
    }

    /// The project root, or the one "no project" error every built-in reports.
    pub(crate) fn project_root(&self) -> anyhow::Result<PathBuf> {
        match self.root() {
            Ok(root) => Ok(root.to_path_buf()),
            Err(e) => Err(NothingToSync::no_project(e).into()),
        }
    }

    /// The working directory and the owned lookup result: `?` on the owned error keeps its source chain.
    pub(crate) fn into_cwd_and_root(self) -> (PathBuf, Result<PathBuf, DiscoveryError>) {
        self.resolved();
        let root = self.root.into_inner().expect("resolved above");
        (self.cwd, root)
    }
}

#[cfg(test)]
mod tests;
