//! Spawn the Python runner subprocess.

use std::io;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Spawn `<python> -m toolr._runner` with:
///
/// - `TOOLR_SPEC_FILE` set to `spec_path`.
/// - stdin/stdout/stderr inherited untouched (so Rich's TTY detection,
///   tools that read stdin, etc., all work).
pub fn spawn_runner(python: &Path, spec_path: &Path) -> io::Result<Child> {
    Command::new(python)
        .arg("-m")
        .arg("toolr._runner")
        .env("TOOLR_SPEC_FILE", spec_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}

/// Captured stderr buffer drained by a background pumper thread.
///
/// Returned by [`spawn_runner_capturing_stderr`] alongside the spawned
/// child. After `wait_with_signals` returns, call [`StderrCapture::take`]
/// to retrieve the accumulated bytes.
pub struct StderrCapture {
    buf: Arc<Mutex<Vec<u8>>>,
    handle: Option<JoinHandle<()>>,
}

impl StderrCapture {
    /// Join the background pumper and return the captured stderr.
    pub fn take(mut self) -> Vec<u8> {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let guard = self.buf.lock().expect("stderr buf mutex poisoned");
        guard.clone()
    }
}

/// Like [`spawn_runner`], but pipes stderr through a background thread
/// so callers can inspect the subprocess's stderr (e.g. for
/// post-mortem ImportError interception). The pumper drains the pipe
/// continuously to avoid backpressure when the subprocess writes more
/// than a pipe-buffer's worth of stderr.
pub fn spawn_runner_capturing_stderr(
    python: &Path,
    spec_path: &Path,
) -> io::Result<(Child, StderrCapture)> {
    let mut child = Command::new(python)
        .arg("-m")
        .arg("toolr._runner")
        .env("TOOLR_SPEC_FILE", spec_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut stderr_pipe = child
        .stderr
        .take()
        .expect("stderr pipe should be present when piped");
    let buf = Arc::new(Mutex::new(Vec::new()));
    let buf_for_thread = Arc::clone(&buf);
    let handle = std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match stderr_pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut guard) = buf_for_thread.lock() {
                        guard.extend_from_slice(&chunk[..n]);
                    }
                }
                Err(_) => break,
            }
        }
    });

    Ok((
        child,
        StderrCapture {
            buf,
            handle: Some(handle),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn spawn_with_nonexistent_python_returns_io_error() {
        let bogus = PathBuf::from("/definitely/not/a/real/python-binary-xyz");
        let result = spawn_runner(&bogus, Path::new("/tmp/whatever.json"));
        assert!(result.is_err());
    }
}
