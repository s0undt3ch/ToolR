//! Kill a timed-out child and everything it started, found by ppid while the child is unreaped.
//! No process group: that would cost the child Ctrl-C and stop it on terminal reads.
//! Both platforms guard against pid reuse. Unix re-reads each ppid just before stopping the
//! process; Windows can't stop processes, so it vets each edge by creation time instead.

use std::process::Child;

/// Descendants of `root` in a `(pid, ppid)` snapshot, as `(pid, parent)`. `adopts(parent, pid)`
/// vets each edge; a refused process is left out with everything below it.
fn descendants(
    root: u32,
    table: &[(u32, u32)],
    adopts: impl Fn(u32, u32) -> bool,
) -> Vec<(u32, u32)> {
    let mut found: Vec<(u32, u32)> = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for &(pid, ppid) in table {
            if ppid == parent
                && pid != parent
                && pid != root
                && !found.iter().any(|&(seen, _)| seen == pid)
                && adopts(parent, pid)
            {
                found.push((pid, parent));
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
fn parent_of(pid: u32) -> Option<u32> {
    parse_stat_ppid(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

#[cfg(target_os = "linux")]
fn all_pids() -> Vec<u32> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .collect()
}

#[cfg(target_os = "macos")]
fn parent_of(pid: u32) -> Option<u32> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let info_ptr = (&raw mut info).cast();
    let got = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            info_ptr,
            size,
        )
    };
    (got == size).then_some(info.pbi_ppid)
}

#[cfg(target_os = "macos")]
fn all_pids() -> Vec<u32> {
    // With no buffer, the call returns the pid count; leave room for processes started since.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) }.max(0) as usize + 64;
    let mut pids = vec![0 as libc::pid_t; count];
    let bytes = std::mem::size_of_val(pids.as_slice()) as libc::c_int;
    let listed = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    pids.truncate(listed.max(0) as usize);
    pids.into_iter().map(|pid| pid as u32).collect()
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn parent_of(_pid: u32) -> Option<u32> {
    None
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn all_pids() -> Vec<u32> {
    Vec::new()
}

#[cfg(unix)]
fn process_table() -> Vec<(u32, u32)> {
    all_pids()
        .into_iter()
        .filter_map(|pid| Some((pid, parent_of(pid)?)))
        .collect()
}

/// Stopped processes can't fork, so each pass only finds what forked while the previous one ran.
/// Five covers any realistic fork chain; a process that slips past them is left running.
#[cfg(unix)]
const MAX_PASSES: usize = 5;

#[cfg(unix)]
fn signal(pid: u32, sig: libc::c_int) {
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

/// Kill `pid` outright; errors (already gone, no access) are ignored.
#[cfg(unix)]
pub(crate) fn terminate(pid: u32) {
    signal(pid, libc::SIGKILL);
}

#[cfg(windows)]
pub(crate) fn terminate(pid: u32) {
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
    use winapi::um::winnt::PROCESS_TERMINATE;

    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !handle.is_null() {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}

/// Stop the tree so nothing in it can fork, then kill every process found and reap the child.
#[cfg(unix)]
pub(crate) fn kill_tree(child: &mut Child) {
    let root = child.id();
    signal(root, libc::SIGSTOP);
    let mut tree: Vec<u32> = Vec::new();
    for _ in 0..MAX_PASSES {
        let new: Vec<u32> = descendants(root, &process_table(), |_, _| true)
            .into_iter()
            .filter(|&(pid, parent)| !tree.contains(&pid) && parent_of(pid) == Some(parent))
            .map(|(pid, _)| pid)
            .inspect(|&pid| signal(pid, libc::SIGSTOP))
            .collect();
        if new.is_empty() {
            break;
        }
        tree.extend(new);
    }
    tree.into_iter().for_each(terminate);
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
        if snapshot != INVALID_HANDLE_VALUE {
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snapshot, &mut entry);
            while more != 0 {
                table.push((entry.th32ProcessID, entry.th32ParentProcessID));
                more = Process32NextW(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
        }
    }
    table
}

#[cfg(windows)]
fn creation_time(pid: u32) -> Option<u64> {
    use winapi::shared::minwindef::FILETIME;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{GetProcessTimes, OpenProcess};
    use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;

    let mut times: [FILETIME; 4] = unsafe { std::mem::zeroed() };
    let [created, exited, kernel, user] = &mut times;
    let ok = unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let ok = GetProcessTimes(handle, created, exited, kernel, user);
        CloseHandle(handle);
        ok
    };
    (ok != 0).then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// Windows keeps a dead parent's pid as the ppid, and a tree member may have reused that pid: a
/// real child can't be older than its parent.
#[cfg(any(windows, test))]
fn not_older_than_parent(created: impl Fn(u32) -> Option<u64>) -> impl Fn(u32, u32) -> bool {
    move |parent, pid| matches!((created(parent), created(pid)), (Some(p), Some(c)) if c >= p)
}

/// No `SIGSTOP` on Windows, so a process started mid-walk can escape; fine for a timeout path.
#[cfg(windows)]
pub(crate) fn kill_tree(child: &mut Child) {
    let edges = descendants(
        child.id(),
        &process_table(),
        not_older_than_parent(creation_time),
    );
    edges.into_iter().for_each(|(pid, _)| terminate(pid));
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests;
