//! Maps a project root to the git repository whose trust covers it. Reads git's
//! files directly: no `git` process, and no `GIT_*` environment variable moves
//! the key.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Where a project root's trust is recorded and what to show for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoKey {
    /// The canonical git common dir, or the canonical project root (per-path).
    pub key: PathBuf,
    /// The repository path shown to the user.
    pub display: PathBuf,
    /// The canonical project root, or the input unchanged when it can't be
    /// canonicalised.
    pub project_root: PathBuf,
    /// `.git` is a linked worktree's file whose back-link doesn't name this
    /// checkout: `git worktree repair` would restore the repository key.
    pub repair_hint: bool,
}

enum Lookup {
    Repo(PathBuf),
    PerPath,
    StaleBackLink,
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
    let (key, repair_hint) = match lookup {
        Lookup::Repo(key) => (key, false),
        Lookup::PerPath => (project_root.clone(), false),
        Lookup::StaleBackLink => (project_root.clone(), true),
    };
    let display = match key.file_name() {
        Some(name) if name == ".git" => key.parent().unwrap_or(&key).to_path_buf(),
        _ => key.clone(),
    };
    RepoKey {
        key,
        display,
        project_root,
        repair_hint,
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
        return fs::canonicalize(dot_git).map_or(Lookup::PerPath, Lookup::Repo);
    }
    if meta.is_file() {
        return linked_worktree(dir, dot_git);
    }
    Lookup::PerPath
}

fn linked_worktree(dir: &Path, dot_git: &Path) -> Lookup {
    let target = fs::read_to_string(dot_git).ok().and_then(|content| {
        let path = content.strip_prefix("gitdir: ")?.trim();
        (!path.is_empty()).then(|| dir.join(path))
    });
    let Some(gitdir) = target.and_then(|path| fs::canonicalize(path).ok()) else {
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
    if back_link_names(&gitdir, dir) {
        Lookup::Repo(common_dir.to_path_buf())
    } else {
        Lookup::StaleBackLink
    }
}

/// Whether `<gitdir>/gitdir`, the link git writes back to a worktree it
/// created, names `dir/.git`. Without it a crafted `.git` file could name a
/// live worktree's gitdir.
fn back_link_names(gitdir: &Path, dir: &Path) -> bool {
    let Ok(content) = fs::read_to_string(gitdir.join("gitdir")) else {
        return false;
    };
    let link = gitdir.join(content.trim());
    link.file_name() == Some(".git".as_ref())
        && link
            .parent()
            .and_then(|parent| fs::canonicalize(parent).ok())
            .as_deref()
            == Some(dir)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args([
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_CEILING_DIRECTORIES")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn canon(path: &Path) -> PathBuf {
        fs::canonicalize(path).unwrap()
    }

    /// `git init` plus an empty commit at `path`; returns the canonical work tree.
    fn init_repo(path: &Path) -> PathBuf {
        fs::create_dir_all(path).unwrap();
        git(path, &["init", "-q"]);
        git(path, &["commit", "-q", "--allow-empty", "-m", "init"]);
        canon(path)
    }

    fn add_worktree(repo: &Path, wt: &Path) -> PathBuf {
        git(
            repo,
            &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
        );
        canon(wt)
    }

    fn rev_parse_common_dir(dir: &Path) -> PathBuf {
        let out = git(
            dir,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        );
        canon(Path::new(out.trim()))
    }

    fn resolve(path: &Path) -> RepoKey {
        RepoKey::resolve(path, None)
    }

    fn assert_per_path(path: &Path, repair_hint: bool) {
        let got = resolve(path);
        let root = canon(path);
        assert_eq!(got.key, root);
        assert_eq!(got.display, root);
        assert_eq!(got.project_root, root);
        assert_eq!(got.repair_hint, repair_hint);
    }

    #[test]
    fn main_checkout_keys_on_dot_git() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        let got = resolve(&repo);
        assert_eq!(got.key, repo.join(".git"));
        assert_eq!(got.display, repo);
        assert_eq!(got.project_root, repo);
        assert!(!got.repair_hint);
    }

    #[test]
    fn worktree_inside_and_outside_repo_share_key() {
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        let inside = add_worktree(&repo, &repo.join(".worktrees/a"));
        let sibling = add_worktree(&repo, &outside.path().join("b"));
        for wt in [inside, sibling] {
            let got = resolve(&wt);
            assert_eq!(got.key, repo.join(".git"));
            assert_eq!(got.display, repo);
            assert_eq!(got.project_root, wt);
            assert!(!got.repair_hint);
        }
    }

    #[test]
    fn nested_project_root_in_worktree() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        let wt = add_worktree(&repo, &tmp.path().join("wt"));
        fs::create_dir(wt.join("sub")).unwrap();
        let got = resolve(&wt.join("sub"));
        assert_eq!(got.key, repo.join(".git"));
        assert_eq!(got.project_root, wt.join("sub"));
    }

    #[test]
    fn key_matches_git_rev_parse() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        let wt = add_worktree(&repo, &tmp.path().join("wt"));
        git(tmp.path(), &["clone", "-q", "--bare", "repo", "bare.git"]);
        let bare_wt = add_worktree(&tmp.path().join("bare.git"), &tmp.path().join("bare-wt"));
        for dir in [repo, wt, bare_wt] {
            assert_eq!(resolve(&dir).key, rev_parse_common_dir(&dir), "{dir:?}");
        }
    }

    #[test]
    fn relative_paths_worktree() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        git(
            &repo,
            &[
                "-c",
                "worktree.useRelativePaths=true",
                "worktree",
                "add",
                "-q",
                "--detach",
                "../wt",
            ],
        );
        let wt = canon(&tmp.path().join("wt"));
        assert!(
            !fs::read_to_string(wt.join(".git"))
                .unwrap()
                .contains(tmp.path().to_str().unwrap())
        );
        assert_eq!(resolve(&wt).key, repo.join(".git"));
        assert_eq!(resolve(&wt).key, rev_parse_common_dir(&wt));
    }

    #[test]
    fn same_basename_worktrees() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        let one = add_worktree(&repo, &tmp.path().join("p1/same"));
        let two = add_worktree(&repo, &tmp.path().join("p2/same"));
        assert_eq!(resolve(&one).key, repo.join(".git"));
        assert_eq!(resolve(&two).key, repo.join(".git"));
    }

    #[test]
    fn git_worktree_move_keeps_key() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        add_worktree(&repo, &tmp.path().join("wt"));
        git(&repo, &["worktree", "move", "../wt", "../moved"]);
        let got = resolve(&tmp.path().join("moved"));
        assert_eq!(got.key, repo.join(".git"));
        assert!(!got.repair_hint);
    }

    #[test]
    fn main_moved_then_repair_restores_key() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        let wt = add_worktree(&repo, &tmp.path().join("wt"));
        fs::rename(&repo, tmp.path().join("repo2")).unwrap();
        let moved = canon(&tmp.path().join("repo2"));
        assert_per_path(&wt, false);
        git(&moved, &["worktree", "repair", wt.to_str().unwrap()]);
        assert_eq!(resolve(&wt).key, moved.join(".git"));
        assert_eq!(resolve(&wt).display, moved);
    }

    #[test]
    fn plain_mv_falls_back_with_hint() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        let wt = add_worktree(&repo, &tmp.path().join("wt"));
        fs::rename(&wt, tmp.path().join("moved")).unwrap();
        assert_per_path(&tmp.path().join("moved"), true);
    }

    #[test]
    fn bare_repo_display_is_the_bare_dir() {
        let tmp = TempDir::new().unwrap();
        init_repo(&tmp.path().join("repo"));
        git(tmp.path(), &["clone", "-q", "--bare", "repo", "foo.git"]);
        let bare = canon(&tmp.path().join("foo.git"));
        let wt = add_worktree(&bare, &tmp.path().join("wt"));
        let got = resolve(&wt);
        assert_eq!(got.key, bare);
        assert_eq!(got.display, bare);
    }

    #[test]
    fn nested_clone_is_its_own_repo() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        git(&repo, &["clone", "-q", ".", "inner"]);
        let inner = canon(&repo.join("inner"));
        assert_eq!(resolve(&inner).key, inner.join(".git"));
    }

    fn superproject(tmp: &Path) -> PathBuf {
        init_repo(&tmp.join("sub"));
        let sup = init_repo(&tmp.join("super"));
        let src = tmp.join("sub");
        git(
            &sup,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                src.to_str().unwrap(),
                "mod",
            ],
        );
        sup
    }

    #[test]
    fn submodule_is_per_path() {
        let tmp = TempDir::new().unwrap();
        let sup = superproject(tmp.path());
        assert!(sup.join("mod/.git").is_file());
        assert_per_path(&sup.join("mod"), false);
    }

    #[test]
    fn submodule_worktree_keys_on_module_dir() {
        let tmp = TempDir::new().unwrap();
        let sup = superproject(tmp.path());
        let wt = add_worktree(&sup.join("mod"), &tmp.path().join("mod-wt"));
        let got = resolve(&wt);
        assert_eq!(got.key, sup.join(".git/modules/mod"));
        assert_eq!(got.key, rev_parse_common_dir(&wt));
    }

    #[test]
    fn separate_git_dir_main_is_per_path() {
        let tmp = TempDir::new().unwrap();
        git(
            tmp.path(),
            &["init", "-q", "--separate-git-dir", "gd", "main"],
        );
        assert!(tmp.path().join("main/.git").is_file());
        assert_per_path(&tmp.path().join("main"), false);
    }

    /// A trusted repository with one linked worktree, plus an attacker's directory.
    struct Crafted {
        _tmp: TempDir,
        trusted: PathBuf,
        live_wt: PathBuf,
        attacker: PathBuf,
    }

    impl Crafted {
        fn new() -> Self {
            let tmp = TempDir::new().unwrap();
            let trusted = init_repo(&tmp.path().join("trusted"));
            let live_wt = add_worktree(&trusted, &tmp.path().join("live"));
            fs::create_dir(tmp.path().join("attacker")).unwrap();
            let attacker = canon(&tmp.path().join("attacker"));
            Self {
                _tmp: tmp,
                trusted,
                live_wt,
                attacker,
            }
        }
        fn trusted_key(&self) -> PathBuf {
            self.trusted.join(".git")
        }
        fn write(&self, rel: &str, content: &str) {
            let path = self.attacker.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
    }

    #[test]
    fn crafted_gitfile_to_trusted_dot_git() {
        let c = Crafted::new();
        c.write(".git", &format!("gitdir: {}\n", c.trusted_key().display()));
        let got = resolve(&c.attacker);
        assert_ne!(got.key, c.trusted_key());
        assert_per_path(&c.attacker, false);
    }

    #[test]
    fn crafted_gitfile_to_live_worktree_gitdir() {
        let c = Crafted::new();
        let live_gitdir = fs::read_to_string(c.live_wt.join(".git")).unwrap();
        c.write(".git", &live_gitdir);
        assert_eq!(resolve(&c.live_wt).key, c.trusted_key());
        let got = resolve(&c.attacker);
        assert_ne!(got.key, c.trusted_key());
        assert_eq!(got.key, c.attacker);
    }

    #[test]
    fn crafted_self_contained_worktree() {
        let c = Crafted::new();
        c.write(".git", "gitdir: ./g\n");
        c.write(
            "g/gitdir",
            &format!("{}\n", c.attacker.join(".git").display()),
        );
        c.write("g/commondir", &format!("{}\n", c.trusted_key().display()));
        let got = resolve(&c.attacker);
        assert_ne!(got.key, c.trusted_key());
        assert_per_path(&c.attacker, false);
    }

    #[test]
    fn crafted_worktrees_dir_outside_git() {
        let c = Crafted::new();
        c.write(".git", "gitdir: x/worktrees/g\n");
        c.write(
            "x/worktrees/g/gitdir",
            &format!("{}\n", c.attacker.join(".git").display()),
        );
        c.write(
            "x/worktrees/g/commondir",
            &format!("{}\n", c.trusted_key().display()),
        );
        let got = resolve(&c.attacker);
        assert_ne!(got.key, c.trusted_key());
        assert_eq!(got.key, c.attacker.join("x"));
    }

    #[test]
    fn planted_commondir_in_dot_git_dir_ignored() {
        let c = Crafted::new();
        init_repo(&c.attacker);
        fs::write(
            c.attacker.join(".git/commondir"),
            format!("{}\n", c.trusted_key().display()),
        )
        .unwrap();
        let got = resolve(&c.attacker);
        assert_ne!(got.key, c.trusted_key());
        assert_eq!(got.key, c.attacker.join(".git"));
    }

    #[test]
    fn malformed_gitfile_is_per_path() {
        let c = Crafted::new();
        c.write(".git", &format!("{}\n", c.trusted_key().display()));
        assert_per_path(&c.attacker, false);
        c.write(".git", "gitdir:   \n");
        assert_per_path(&c.attacker, false);
        c.write(".git", "gitdir: does/not/exist\n");
        assert_per_path(&c.attacker, false);
    }

    #[test]
    fn worktree_without_back_link_is_per_path_with_hint() {
        let c = Crafted::new();
        let gitdir = c.trusted.join(".git/worktrees/live");
        fs::remove_file(gitdir.join("gitdir")).unwrap();
        assert_per_path(&c.live_wt, true);
    }

    #[test]
    fn back_link_must_name_dot_git() {
        let c = Crafted::new();
        let gitdir = c.trusted.join(".git/worktrees/live");
        fs::write(
            gitdir.join("gitdir"),
            format!("{}\n", c.live_wt.join("other").display()),
        )
        .unwrap();
        assert_per_path(&c.live_wt, true);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_dot_git_dir_and_file_are_per_path() {
        use std::os::unix::fs::symlink;
        let c = Crafted::new();
        symlink(c.trusted_key(), c.attacker.join(".git")).unwrap();
        assert_per_path(&c.attacker, false);

        let other = c.attacker.join("other");
        fs::create_dir(&other).unwrap();
        symlink(c.live_wt.join(".git"), other.join(".git")).unwrap();
        assert_per_path(&other, false);
    }

    #[cfg(unix)]
    #[test]
    fn special_file_dot_git_is_per_path() {
        let c = Crafted::new();
        let _socket = std::os::unix::net::UnixListener::bind(c.attacker.join(".git")).unwrap();
        assert_per_path(&c.attacker, false);
    }

    #[cfg(unix)]
    #[test]
    fn foreign_owner_is_per_path() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        // SAFETY: geteuid has no preconditions.
        let euid = unsafe { libc::geteuid() };
        assert_eq!(resolve_as(&repo, None, Some(euid)).key, repo.join(".git"));
        let got = resolve_as(&repo, None, Some(euid.wrapping_add(1)));
        assert_eq!(got.key, repo);
        assert!(!got.repair_hint);
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_directory_does_not_walk_past_it() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        let sub = repo.join("sub");
        fs::create_dir(&sub).unwrap();
        // Without search permission on `sub` its `.git` can't be checked.
        let got = crate::test_fs::with_mode(&sub, 0o600, || resolve(&sub));
        assert!(got.is_none_or(|got| got.key == sub && !got.repair_hint));
    }

    #[test]
    fn home_dotgit_does_not_cover_projects_below() {
        let tmp = TempDir::new().unwrap();
        let home = init_repo(&tmp.path().join("h"));
        fs::create_dir(home.join("x")).unwrap();
        let got = RepoKey::resolve(&home.join("x"), Some(&home));
        assert_eq!(got.key, home.join("x"));
        let foo = init_repo(&home.join("foo"));
        assert_eq!(RepoKey::resolve(&foo, Some(&home)).key, foo.join(".git"));
        assert_eq!(RepoKey::resolve(&home, Some(&home)).key, home.join(".git"));
    }

    #[test]
    fn walk_stops_at_home() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        fs::create_dir_all(repo.join("h/p")).unwrap();
        let got = RepoKey::resolve(&repo.join("h/p"), Some(&repo.join("h")));
        assert_eq!(got.key, repo.join("h/p"));
        let elsewhere = tmp.path().join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let got = RepoKey::resolve(&repo.join("h/p"), Some(&canon(&elsewhere)));
        assert_eq!(got.key, repo.join(".git"));
    }

    #[test]
    fn no_home_walks_to_root() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        fs::create_dir_all(repo.join("a/b")).unwrap();
        assert_eq!(
            RepoKey::resolve(&repo.join("a/b"), None).key,
            repo.join(".git")
        );
    }

    #[test]
    fn non_git_project_is_per_path() {
        let tmp = TempDir::new().unwrap();
        assert_per_path(tmp.path(), false);
    }

    #[test]
    fn missing_project_root_is_per_path() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        let missing = repo.join("nope");
        let got = resolve(&missing);
        assert_eq!(got.key, missing);
        assert_eq!(got.display, missing);
        assert_eq!(got.project_root, missing);
        assert!(!got.repair_hint);
    }

    #[test]
    fn gitfile_crlf_and_whitespace() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        let wt = add_worktree(&repo, &tmp.path().join("wt"));
        let gitdir = repo.join(".git/worktrees/wt");
        fs::write(wt.join(".git"), format!("gitdir: {}\r\n", gitdir.display())).unwrap();
        fs::write(
            gitdir.join("gitdir"),
            format!("{}   \n", wt.join(".git").display()),
        )
        .unwrap();
        let got = resolve(&wt);
        assert_eq!(got.key, repo.join(".git"));
        assert!(!got.repair_hint);
    }

    #[test]
    fn spaces_and_unicode_in_paths() {
        let tmp = TempDir::new().unwrap();
        let repo = init_repo(&tmp.path().join("repo"));
        let wt = add_worktree(&repo, &tmp.path().join("odd dir/wörk tree"));
        assert_eq!(resolve(&wt).key, repo.join(".git"));
    }
}
