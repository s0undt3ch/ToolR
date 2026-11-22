//! Maps a project root to the git repository whose trust covers it. Reads git's
//! files directly: no `git` process, and no `GIT_*` environment variable moves
//! the key.

use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Git writes these files a line at a time; anything larger is not git's.
const MAX_GIT_FILE: u64 = 64 * 1024;

/// Records match only within a kind: a forged `worktrees/` layout inside a
/// directory trusted per-path resolves to that directory's path as a
/// repository key, which must not match its per-path record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyKind {
    Repository,
    PerPath,
}

/// Where a project root's trust is recorded and what to show for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoKey {
    /// The canonical git common dir, or the canonical project root (per-path).
    pub key: PathBuf,
    pub kind: KeyKind,
    /// The repository path shown to the user.
    pub display: PathBuf,
    /// The canonical project root, or the input unchanged when it can't be
    /// canonicalised.
    pub project_root: PathBuf,
    /// The directory holding the `.git` entry the key came from, which may sit
    /// above the project root.
    pub worktree_top: Option<PathBuf>,
    /// Set when `.git` is a linked worktree's file whose back-link names a
    /// `.git` that no longer points at the same gitdir: `git worktree repair`
    /// would restore the repository key without taking it from a live worktree.
    pub repair: Option<RepairHint>,
}

/// What `git worktree repair` would bind, for the refusal to spell out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairHint {
    /// The directory holding the `.git` file: the path `git worktree repair`
    /// takes.
    pub worktree_top: PathBuf,
    /// The repository the `.git` file claims, as shown to the user.
    pub repo: PathBuf,
    /// The worktree's name: its gitdir's directory name under `worktrees/`.
    pub worktree: PathBuf,
}

enum Lookup {
    Repo { key: PathBuf, top: PathBuf },
    PerPath,
    StaleBackLink(RepairHint),
}

impl RepoKey {
    /// `home` is the canonical home directory: a `.git` there covers only the
    /// home directory itself.
    pub fn resolve(project_root: &Path, home: Option<&Path>) -> RepoKey {
        #[cfg(unix)]
        // SAFETY: geteuid has no preconditions.
        let euid = Some(unsafe { libc::geteuid() });
        #[cfg(not(unix))]
        let euid = None;
        resolve_as(project_root, home, euid)
    }
}

fn resolve_as(project_root: &Path, home: Option<&Path>, euid: Option<u32>) -> RepoKey {
    let (project_root, lookup) = match fs::canonicalize(project_root) {
        Ok(root) => {
            let lookup = find(&root, home, euid);
            (root, lookup)
        }
        Err(_) => (project_root.to_path_buf(), Lookup::PerPath),
    };
    let (key, kind, worktree_top, repair) = match lookup {
        Lookup::Repo { key, top } => (key, KeyKind::Repository, Some(top), None),
        Lookup::PerPath => (project_root.clone(), KeyKind::PerPath, None, None),
        Lookup::StaleBackLink(hint) => (
            project_root.clone(),
            KeyKind::PerPath,
            Some(hint.worktree_top.clone()),
            Some(hint),
        ),
    };
    let display = match kind {
        KeyKind::Repository => repo_display(&key),
        KeyKind::PerPath => key.clone(),
    };
    RepoKey {
        key,
        kind,
        display,
        project_root,
        worktree_top,
        repair,
    }
}

/// A common dir named `.git` is shown as the checkout holding it.
fn repo_display(common_dir: &Path) -> PathBuf {
    match (common_dir.file_name(), common_dir.parent()) {
        (Some(name), Some(parent)) if name == ".git" => parent.to_path_buf(),
        _ => common_dir.to_path_buf(),
    }
}

fn find(root: &Path, home: Option<&Path>, euid: Option<u32>) -> Lookup {
    for dir in root.ancestors() {
        if home == Some(dir) && dir != root {
            return Lookup::PerPath;
        }
        let dot_git = dir.join(".git");
        match fs::symlink_metadata(&dot_git) {
            Ok(meta) => return classify(dir, &dot_git, &meta, euid),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            // A `.git` here can't be ruled out; walking past it could borrow an
            // outer repository's trust.
            Err(_) => return Lookup::PerPath,
        }
    }
    Lookup::PerPath
}

fn classify(dir: &Path, dot_git: &Path, meta: &fs::Metadata, euid: Option<u32>) -> Lookup {
    if meta.file_type().is_symlink() {
        return Lookup::PerPath;
    }
    #[cfg(unix)]
    if let Some(euid) = euid
        && std::os::unix::fs::MetadataExt::uid(meta) != euid
    {
        return Lookup::PerPath;
    }
    #[cfg(not(unix))]
    let _ = euid;
    if meta.is_dir() {
        // A planted `commondir` inside is deliberately ignored.
        return fs::canonicalize(dot_git).map_or(Lookup::PerPath, |key| Lookup::Repo {
            key,
            top: dir.to_path_buf(),
        });
    }
    if meta.is_file() {
        return linked_worktree(dir, dot_git);
    }
    Lookup::PerPath
}

fn linked_worktree(dir: &Path, dot_git: &Path) -> Lookup {
    let Ok(Some(gitdir)) = gitfile_target(dot_git) else {
        return Lookup::PerPath;
    };
    // Git keeps every linked worktree's gitdir in `<common dir>/worktrees/`,
    // where an archive can't put one for another repository.
    let common_dir = gitdir
        .parent()
        .filter(|parent| parent.file_name() == Some("worktrees".as_ref()))
        .and_then(Path::parent);
    let Some(common_dir) = common_dir else {
        return Lookup::PerPath;
    };
    // The back-link git writes when it creates a worktree. Without checking it
    // a crafted `.git` file could name a live worktree's gitdir.
    let Ok(content) = read_git_file(&gitdir.join("gitdir")) else {
        return Lookup::PerPath;
    };
    let link = gitdir.join(content.trim());
    let names_this_checkout = link.file_name() == Some(".git".as_ref())
        && link
            .parent()
            .and_then(|parent| fs::canonicalize(parent).ok())
            .as_deref()
            == Some(dir);
    if names_this_checkout {
        return Lookup::Repo {
            key: common_dir.to_path_buf(),
            top: dir.to_path_buf(),
        };
    }
    match gitfile_target(&link) {
        // A copy of a live worktree's `.git`: repairing would hand it that
        // worktree's trust.
        Ok(Some(target)) if target == gitdir => Lookup::PerPath,
        // A back-link that names a FIFO or a device was planted; no hint.
        Err(Unreadable::Hostile) => Lookup::PerPath,
        _ => Lookup::StaleBackLink(RepairHint {
            worktree_top: dir.to_path_buf(),
            repo: repo_display(common_dir),
            worktree: gitdir.file_name().map(PathBuf::from).unwrap_or_default(),
        }),
    }
}

/// Why a git file couldn't be used. A moved worktree's back-link names an
/// `Absent` file, so only that still earns a repair hint.
#[derive(Debug, PartialEq, Eq)]
enum Unreadable {
    /// Missing, or a directory: nothing git would read as a gitfile.
    Absent,
    /// A FIFO, a device, oversized, not UTF-8 or unreadable.
    Hostile,
}

/// The canonical gitdir a `gitdir: <path>` file names, relative paths resolved
/// against the file's directory. `Ok(None)` when the file isn't a gitfile or
/// its target doesn't exist.
fn gitfile_target(gitfile: &Path) -> Result<Option<PathBuf>, Unreadable> {
    let content = read_git_file(gitfile)?;
    let Some(path) = content.strip_prefix("gitdir: ").map(str::trim) else {
        return Ok(None);
    };
    if path.is_empty() {
        return Ok(None);
    }
    Ok(gitfile
        .parent()
        .and_then(|parent| fs::canonicalize(parent.join(path)).ok()))
}

/// Reads a file the repository controls without letting it block or exhaust
/// memory: a FIFO would block `open()` and `/dev/zero` never ends.
fn read_git_file(path: &Path) -> Result<String, Unreadable> {
    let hostile = |e: std::io::Error| match e.kind() {
        ErrorKind::NotFound => Unreadable::Absent,
        _ => Unreadable::Hostile,
    };
    let regular = |meta: fs::Metadata| match meta {
        m if m.is_file() => Ok(()),
        m if m.is_dir() => Err(Unreadable::Absent),
        _ => Err(Unreadable::Hostile),
    };
    // Windows has no `O_NONBLOCK`, so it checks the path before opening.
    #[cfg(not(unix))]
    regular(fs::metadata(path).map_err(hostile)?)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut options, libc::O_NONBLOCK);
    let file = options.open(path).map_err(hostile)?;
    regular(file.metadata().map_err(hostile)?)?;
    let mut bytes = Vec::new();
    file.take(MAX_GIT_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(hostile)?;
    if bytes.len() as u64 > MAX_GIT_FILE {
        return Err(Unreadable::Hostile);
    }
    String::from_utf8(bytes).map_err(|_| Unreadable::Hostile)
}

#[cfg(test)]
mod tests;
