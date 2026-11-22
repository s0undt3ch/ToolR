use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

use super::*;

/// git can't create directories under a verbatim (`\\?\`) path on Windows.
fn git_path(path: &Path) -> &str {
    let s = path.to_str().unwrap();
    s.strip_prefix(r"\\?\").unwrap_or(s)
}

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
    let (at, stderr) = (dir.display(), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "git {args:?} in {at}: {stderr}");
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
    git(repo, &["worktree", "add", "-q", "--detach", git_path(wt)]);
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

/// Resolves on another thread, so a read that blocks fails the test instead
/// of hanging the suite.
fn resolve_promptly(path: &Path) -> RepoKey {
    let path = path.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(resolve(&path));
    });
    rx.recv_timeout(std::time::Duration::from_secs(10))
        .expect("resolving blocked")
}

/// Asserts `path` keys on itself and returns its repair hint.
fn per_path_hint(path: &Path) -> Option<RepairHint> {
    let got = resolve_promptly(path);
    let root = canon(path);
    assert_eq!(got.key, root);
    assert_eq!(got.kind, KeyKind::PerPath);
    assert_eq!(got.display, root);
    assert_eq!(got.project_root, root);
    assert_eq!(
        got.worktree_top,
        got.repair.as_ref().map(|h| h.worktree_top.clone())
    );
    got.repair
}

fn assert_per_path(path: &Path) {
    assert_eq!(per_path_hint(path), None);
}

fn hint(top: &Path, repo: &Path, worktree: &str) -> Option<RepairHint> {
    Some(RepairHint {
        worktree_top: top.to_path_buf(),
        repo: repo.to_path_buf(),
        worktree: PathBuf::from(worktree),
    })
}

#[test]
fn main_checkout_keys_on_dot_git() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp.path().join("repo"));
    let got = resolve(&repo);
    assert_eq!(got.key, repo.join(".git"));
    assert_eq!(got.kind, KeyKind::Repository);
    assert_eq!(got.display, repo);
    assert_eq!(got.project_root, repo);
    assert_eq!(got.worktree_top, Some(repo));
    assert_eq!(got.repair, None);
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
        assert_eq!(got.kind, KeyKind::Repository);
        assert_eq!(got.display, repo);
        assert_eq!(got.project_root, wt);
        assert_eq!(got.repair, None);
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
    assert_eq!(got.worktree_top, Some(wt));
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
    assert_eq!(got.repair, None);
}

#[test]
fn main_moved_then_repair_restores_key() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp.path().join("repo"));
    let wt = add_worktree(&repo, &tmp.path().join("wt"));
    fs::rename(&repo, tmp.path().join("repo2")).unwrap();
    let moved = canon(&tmp.path().join("repo2"));
    assert_per_path(&wt);
    git(&moved, &["worktree", "repair", git_path(&wt)]);
    assert_eq!(resolve(&wt).key, moved.join(".git"));
    assert_eq!(resolve(&wt).display, moved);
}

#[test]
fn plain_mv_falls_back_with_hint() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp.path().join("repo"));
    let wt = add_worktree(&repo, &tmp.path().join("wt"));
    fs::rename(&wt, tmp.path().join("moved")).unwrap();
    let moved = canon(&tmp.path().join("moved"));
    assert_eq!(per_path_hint(&moved), hint(&moved, &repo, "wt"));
}

#[test]
fn nested_project_hint_names_worktree_top() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp.path().join("repo"));
    let wt = add_worktree(&repo, &tmp.path().join("wt"));
    fs::create_dir(wt.join("sub")).unwrap();
    fs::rename(&wt, tmp.path().join("moved")).unwrap();
    let moved = canon(&tmp.path().join("moved"));
    assert_eq!(per_path_hint(&moved.join("sub")), hint(&moved, &repo, "wt"));
}

#[test]
fn deleted_unpruned_worktree_claimed_elsewhere_hints() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp.path().join("repo"));
    let wt = add_worktree(&repo, &tmp.path().join("feature"));
    fs::remove_dir_all(&wt).unwrap();
    let evil = tmp.path().join("Downloads/evil");
    fs::create_dir_all(&evil).unwrap();
    let gitdir = repo.join(".git/worktrees/feature");
    fs::write(evil.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    let evil = canon(&evil);
    assert_eq!(per_path_hint(&evil), hint(&evil, &repo, "feature"));
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
    assert_eq!(got.kind, KeyKind::Repository);
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
            git_path(&src),
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
    assert_per_path(&sup.join("mod"));
}

#[test]
fn submodule_worktree_keys_on_module_dir() {
    let tmp = TempDir::new().unwrap();
    let sup = superproject(tmp.path());
    let wt = add_worktree(&sup.join("mod"), &tmp.path().join("mod-wt"));
    let got = resolve(&wt);
    assert_eq!(got.key, sup.join(".git/modules/mod"));
    assert_eq!(got.kind, KeyKind::Repository);
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
    assert_per_path(&tmp.path().join("main"));
}

/// The owner's repository with one linked worktree, plus an attacker's directory.
struct Crafted {
    _tmp: TempDir,
    owner_repo: PathBuf,
    live_wt: PathBuf,
    attacker: PathBuf,
}

impl Crafted {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let owner_repo = init_repo(&tmp.path().join("owner_repo"));
        let live_wt = add_worktree(&owner_repo, &tmp.path().join("live"));
        fs::create_dir(tmp.path().join("attacker")).unwrap();
        let attacker = canon(&tmp.path().join("attacker"));
        Self {
            _tmp: tmp,
            owner_repo,
            live_wt,
            attacker,
        }
    }
    fn owner_key(&self) -> PathBuf {
        self.owner_repo.join(".git")
    }
    fn write(&self, rel: &str, content: &str) {
        let path = self.attacker.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
}

#[test]
fn crafted_gitfile_to_owner_dot_git() {
    let c = Crafted::new();
    c.write(".git", &format!("gitdir: {}\n", c.owner_key().display()));
    let got = resolve(&c.attacker);
    assert_ne!(got.key, c.owner_key());
    assert_per_path(&c.attacker);
}

#[test]
fn crafted_gitfile_to_live_worktree_gitdir() {
    let c = Crafted::new();
    let live_gitdir = fs::read_to_string(c.live_wt.join(".git")).unwrap();
    c.write(".git", &live_gitdir);
    assert_eq!(resolve(&c.live_wt).key, c.owner_key());
    let got = resolve(&c.attacker);
    assert_ne!(got.key, c.owner_key());
    assert_eq!(got.key, c.attacker);
    assert_eq!(got.kind, KeyKind::PerPath);
    assert_eq!(got.repair, None);
}

#[test]
fn crafted_self_contained_worktree() {
    let c = Crafted::new();
    c.write(".git", "gitdir: ./g\n");
    c.write(
        "g/gitdir",
        &format!("{}\n", c.attacker.join(".git").display()),
    );
    c.write("g/commondir", &format!("{}\n", c.owner_key().display()));
    let got = resolve(&c.attacker);
    assert_ne!(got.key, c.owner_key());
    assert_per_path(&c.attacker);
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
        &format!("{}\n", c.owner_key().display()),
    );
    let got = resolve(&c.attacker);
    assert_ne!(got.key, c.owner_key());
    assert_eq!(got.key, c.attacker.join("x"));
    assert_eq!(got.kind, KeyKind::Repository);
}

#[test]
fn forged_worktree_under_per_path_dir_is_a_different_kind() {
    let tmp = TempDir::new().unwrap();
    let per_path_dir = tmp.path().join("p");
    fs::create_dir_all(per_path_dir.join("worktrees/proj")).unwrap();
    fs::create_dir_all(per_path_dir.join("worktrees/g")).unwrap();
    fs::write(per_path_dir.join("worktrees/proj/.git"), "gitdir: ../g\n").unwrap();
    fs::write(per_path_dir.join("worktrees/g/gitdir"), "../proj/.git\n").unwrap();
    let per_path = resolve(&per_path_dir);
    assert_eq!(per_path.kind, KeyKind::PerPath);
    let forged = resolve(&per_path_dir.join("worktrees/proj"));
    assert_eq!(forged.key, per_path.key);
    assert_eq!(forged.kind, KeyKind::Repository);
    assert_ne!(forged.kind, per_path.kind);
}

#[test]
fn deleted_worktree_recreated_at_same_path_keeps_key() {
    // The spec's accepted residual: stale bookkeeping until `git worktree prune`.
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp.path().join("repo"));
    let wt = add_worktree(&repo, &tmp.path().join("wt"));
    let gitfile = fs::read_to_string(wt.join(".git")).unwrap();
    fs::remove_dir_all(&wt).unwrap();
    fs::create_dir(&wt).unwrap();
    fs::write(wt.join(".git"), gitfile).unwrap();
    let got = resolve(&wt);
    assert_eq!(got.key, repo.join(".git"));
    assert_eq!(got.kind, KeyKind::Repository);
}

#[test]
fn per_path_key_named_dot_git_displays_itself() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("x/.git");
    let got = resolve(&missing);
    assert_eq!(got.kind, KeyKind::PerPath);
    assert_eq!(got.display, missing);
}

#[test]
fn planted_commondir_in_dot_git_dir_ignored() {
    let c = Crafted::new();
    init_repo(&c.attacker);
    fs::write(
        c.attacker.join(".git/commondir"),
        format!("{}\n", c.owner_key().display()),
    )
    .unwrap();
    let got = resolve(&c.attacker);
    assert_ne!(got.key, c.owner_key());
    assert_eq!(got.key, c.attacker.join(".git"));
}

#[test]
fn malformed_gitfile_is_per_path() {
    let c = Crafted::new();
    c.write(".git", &format!("{}\n", c.owner_key().display()));
    assert_per_path(&c.attacker);
    c.write(".git", "gitdir:   \n");
    assert_per_path(&c.attacker);
    c.write(".git", "gitdir: does/not/exist\n");
    assert_per_path(&c.attacker);
}

#[test]
fn worktree_without_back_link_is_per_path_without_hint() {
    let c = Crafted::new();
    let gitdir = c.owner_repo.join(".git/worktrees/live");
    fs::remove_file(gitdir.join("gitdir")).unwrap();
    assert_per_path(&c.live_wt);
}

#[test]
fn back_link_must_name_dot_git() {
    let c = Crafted::new();
    let gitdir = c.owner_repo.join(".git/worktrees/live");
    fs::write(
        gitdir.join("gitdir"),
        format!("{}\n", c.live_wt.join("other").display()),
    )
    .unwrap();
    assert_eq!(
        per_path_hint(&c.live_wt),
        hint(&c.live_wt, &c.owner_repo, "live")
    );
}

#[cfg(unix)]
#[test]
fn symlinked_dot_git_dir_and_file_are_per_path() {
    use std::os::unix::fs::symlink;
    let c = Crafted::new();
    symlink(c.owner_key(), c.attacker.join(".git")).unwrap();
    assert_per_path(&c.attacker);

    let other = c.attacker.join("other");
    fs::create_dir(&other).unwrap();
    symlink(c.live_wt.join(".git"), other.join(".git")).unwrap();
    assert_per_path(&other);
}

#[cfg(unix)]
#[test]
fn special_file_dot_git_is_per_path() {
    let c = Crafted::new();
    let _socket = std::os::unix::net::UnixListener::bind(c.attacker.join(".git")).unwrap();
    assert_per_path(&c.attacker);
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
    assert_eq!(got.kind, KeyKind::PerPath);
    assert_eq!(got.repair, None);
    let got = resolve_as(&repo.join(".git"), None, Some(euid.wrapping_add(1)));
    assert_eq!(got.key, repo.join(".git"));
    assert_eq!(got.display, repo.join(".git"));
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
    assert!(got.is_none_or(|got| got.key == sub && got.repair.is_none()));
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
    assert_per_path(tmp.path());
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
    assert_eq!(got.repair, None);
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
    assert_eq!(got.repair, None);
}

#[test]
fn spaces_and_unicode_in_paths() {
    let tmp = TempDir::new().unwrap();
    let repo = init_repo(&tmp.path().join("repo"));
    let wt = add_worktree(&repo, &tmp.path().join("odd dir/wörk tree"));
    assert_eq!(resolve(&wt).key, repo.join(".git"));
}

#[cfg(unix)]
fn mkfifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `path` is a valid NUL-terminated string.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
}

impl Crafted {
    fn live_back_link(&self) -> PathBuf {
        self.owner_repo.join(".git/worktrees/live/gitdir")
    }
}

#[cfg(unix)]
#[test]
fn fifo_dot_git_file_is_per_path() {
    let c = Crafted::new();
    mkfifo(&c.attacker.join(".git"));
    assert_per_path(&c.attacker);
}

#[cfg(unix)]
#[test]
fn fifo_back_link_is_per_path() {
    let c = Crafted::new();
    fs::remove_file(c.live_back_link()).unwrap();
    mkfifo(&c.live_back_link());
    assert_per_path(&c.live_wt);
}

#[cfg(unix)]
#[test]
fn fifo_back_link_target_is_per_path_without_hint() {
    let c = Crafted::new();
    mkfifo(&c.attacker.join("pipe"));
    fs::write(
        c.live_back_link(),
        format!("{}\n", c.attacker.join("pipe").display()),
    )
    .unwrap();
    assert_per_path(&c.live_wt);
}

#[cfg(unix)]
#[test]
fn dev_zero_back_link_is_per_path() {
    use std::os::unix::fs::symlink;
    let c = Crafted::new();
    fs::remove_file(c.live_back_link()).unwrap();
    symlink("/dev/zero", c.live_back_link()).unwrap();
    assert_per_path(&c.live_wt);
}

#[cfg(unix)]
#[test]
fn dev_zero_back_link_target_is_per_path_without_hint() {
    use std::os::unix::fs::symlink;
    let c = Crafted::new();
    symlink("/dev/zero", c.attacker.join("zero")).unwrap();
    fs::write(
        c.live_back_link(),
        format!("{}\n", c.attacker.join("zero").display()),
    )
    .unwrap();
    assert_per_path(&c.live_wt);
}

#[test]
fn oversized_dot_git_file_is_per_path() {
    let c = Crafted::new();
    let gitfile = fs::read_to_string(c.live_wt.join(".git")).unwrap();
    let padded = format!("{gitfile}{}", "\n".repeat(64 * 1024));
    fs::write(c.live_wt.join(".git"), padded).unwrap();
    assert_per_path(&c.live_wt);
}

#[test]
fn non_utf8_dot_git_file_is_per_path() {
    let c = Crafted::new();
    fs::write(c.attacker.join(".git"), b"gitdir: \xff\n").unwrap();
    assert_per_path(&c.attacker);
}

#[test]
fn read_git_file_classifies_what_it_cannot_read() {
    let tmp = TempDir::new().unwrap();
    let full = tmp.path().join("full");
    fs::write(&full, "x".repeat(64 * 1024)).unwrap();
    assert_eq!(read_git_file(&full).unwrap().len(), 64 * 1024);
    assert_eq!(read_git_file(tmp.path()), Err(Unreadable::Absent));
    assert_eq!(
        read_git_file(&tmp.path().join("missing")),
        Err(Unreadable::Absent)
    );
    #[cfg(unix)]
    {
        mkfifo(&tmp.path().join("pipe"));
        assert_eq!(
            read_git_file(&tmp.path().join("pipe")),
            Err(Unreadable::Hostile)
        );
        let got = crate::test_fs::with_mode(&full, 0o000, || read_git_file(&full));
        assert!(got.is_none_or(|got| got == Err(Unreadable::Hostile)));
    }
}
