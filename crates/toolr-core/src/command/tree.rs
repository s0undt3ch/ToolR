//! Kill a timed-out child and everything it started, found by ppid while the child is unreaped.
//! No process group: that would cost the child Ctrl-C and stop it on terminal reads.

use std::process::Child;

/// Descendants of `root` in a `(pid, ppid)` snapshot, nearest first.
fn descendants(root: u32, table: &[(u32, u32)]) -> Vec<u32> {
    let mut found = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for &(pid, ppid) in table {
            if ppid == parent && pid != parent && pid != root && !found.contains(&pid) {
                found.push(pid);
                frontier.push(pid);
            }
        }
    }
    found
}

/// The ppid in a `/proc/<pid>/stat` line: the second field after the command name, which is in
/// parentheses and may itself contain `)`.
#[cfg(any(target_os = "linux", test))]
fn parse_stat_ppid(stat: &str) -> Option<u32> {
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(target_os = "linux")]
fn process_table() -> Vec<(u32, u32)> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            Some((pid, parse_stat_ppid(&stat)?))
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn process_table() -> Vec<(u32, u32)> {
    use std::mem::size_of;

    // With no buffer, the call returns the pid count; leave room for processes started since.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) }.max(0) as usize + 64;
    let mut pids = vec![0 as libc::pid_t; count];
    let bytes = (pids.len() * size_of::<libc::pid_t>()) as libc::c_int;
    let listed = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    pids.truncate(listed.max(0) as usize);
    pids.into_iter()
        .filter_map(|pid| {
            let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
            let size = size_of::<libc::proc_bsdinfo>() as libc::c_int;
            let info_ptr = (&raw mut info).cast();
            let got = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, info_ptr, size) };
            (got == size).then_some((pid as u32, info.pbi_ppid))
        })
        .collect()
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn process_table() -> Vec<(u32, u32)> {
    Vec::new()
}

#[cfg(unix)]
fn signal(pid: u32, sig: libc::c_int) {
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

/// Stop the tree so nothing in it can fork, then kill every process found and reap the child.
#[cfg(unix)]
pub(crate) fn kill_tree(child: &mut Child) {
    let root = child.id();
    signal(root, libc::SIGSTOP);
    let mut tree: Vec<u32> = Vec::new();
    // Re-walk until a pass finds nothing new: a process may fork before it's stopped.
    for _ in 0..5 {
        let new: Vec<u32> = descendants(root, &process_table())
            .into_iter()
            .filter(|pid| !tree.contains(pid))
            .collect();
        new.iter().for_each(|&pid| signal(pid, libc::SIGSTOP));
        if new.is_empty() {
            break;
        }
        tree.extend(new);
    }
    tree.iter().for_each(|&pid| signal(pid, libc::SIGKILL));
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
fn process_table() -> Vec<(u32, u32)> {
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::tlhelp32::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    let mut table = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return table;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry);
        while more != 0 {
            table.push((entry.th32ProcessID, entry.th32ParentProcessID));
            more = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
    }
    table
}

#[cfg(windows)]
fn creation_time(handle: winapi::um::winnt::HANDLE) -> Option<u64> {
    use winapi::shared::minwindef::FILETIME;
    use winapi::um::processthreadsapi::GetProcessTimes;

    let mut times: [FILETIME; 4] = unsafe { std::mem::zeroed() };
    let [created, exited, kernel, user] = &mut times;
    let ok = unsafe { GetProcessTimes(handle, created, exited, kernel, user) };
    (ok != 0).then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// No `SIGSTOP` on Windows, so a process started mid-walk can escape; fine for a timeout path.
#[cfg(windows)]
pub(crate) fn kill_tree(child: &mut Child) {
    use std::os::windows::io::AsRawHandle;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
    use winapi::um::winnt::{PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE};

    let born = creation_time(child.as_raw_handle().cast());
    for pid in descendants(child.id(), &process_table()) {
        unsafe {
            let handle = OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pid,
            );
            if handle.is_null() {
                continue;
            }
            // Windows keeps a dead parent's pid as the ppid, and the child may have reused that
            // pid: only a process started after the child can be its descendant.
            if let (Some(born), Some(started)) = (born, creation_time(handle))
                && started >= born
            {
                TerminateProcess(handle, 1);
            }
            CloseHandle(handle);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descendants_finds_nested_children() {
        let table = [(10, 1), (11, 10), (12, 11), (13, 1)];
        let mut found = descendants(10, &table);
        found.sort_unstable();
        assert_eq!(found, vec![11, 12]);
    }

    #[test]
    fn descendants_ignores_unrelated() {
        assert!(descendants(13, &[(10, 1), (11, 10), (13, 1)]).is_empty());
    }

    #[test]
    fn descendants_survives_cycles() {
        // pid 0 is its own parent on macOS; a stale snapshot can also hold a loop.
        let mut found = descendants(5, &[(0, 0), (6, 5), (7, 6), (6, 7), (5, 7)]);
        found.sort_unstable();
        assert_eq!(found, vec![6, 7]);
    }

    #[test]
    fn parse_stat_ppid_skips_parens_in_the_command_name() {
        assert_eq!(parse_stat_ppid("42 (a) b) S 7 42 42 0"), Some(7));
        assert_eq!(parse_stat_ppid("42 (sleep) S 1 42 42 0"), Some(1));
        assert_eq!(parse_stat_ppid("42 (sleep) S"), None);
        assert_eq!(parse_stat_ppid("garbage"), None);
    }

    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    #[test]
    fn process_table_includes_self() {
        let me = std::process::id();
        let table = process_table();
        let entry = table.iter().find(|&&(pid, _)| pid == me);
        assert!(
            matches!(entry, Some(&(_, ppid)) if ppid != 0),
            "{me} missing from {} entries",
            table.len()
        );
    }
}
