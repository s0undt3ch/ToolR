use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::io::{FromRawFd, OwnedFd};

#[cfg(windows)]
use std::os::windows::io::{BorrowedHandle, RawHandle};

mod tree;

/// How long the I/O threads get to finish once the tree is killed. Only an escaped process holds
/// them past EOF, and a detached reader lets go of the pipe at its next read, so this only buys
/// time to flush what the dead tree already wrote.
const KILL_GRACE: Duration = Duration::from_millis(500);

/// How long the wait loops sleep between checks.
const POLL: Duration = Duration::from_millis(10);

// Custom error types
#[derive(Debug)]
pub struct CommandExecutionError {
    message: String,
}

impl fmt::Display for CommandExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for CommandExecutionError {}

impl CommandExecutionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug)]
pub struct CommandTimeoutExceededError {
    message: String,
}

impl fmt::Display for CommandTimeoutExceededError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for CommandTimeoutExceededError {}

impl CommandTimeoutExceededError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug)]
pub struct CommandNoOutputTimeoutError {
    message: String,
}

impl fmt::Display for CommandNoOutputTimeoutError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for CommandNoOutputTimeoutError {}

impl CommandNoOutputTimeoutError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

// Pure Rust implementation - available always
pub struct CommandConfig {
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub input: Option<Vec<u8>>,
    /// Capture file for the child's stdout, set for `capture_output=True`.
    pub stdout_capture: Option<File>,
    /// Capture file for the child's stderr, set for `capture_output=True`.
    pub stderr_capture: Option<File>,
    /// Where the child's stdout streams to: toolr's own stdout.
    pub stdout_stream: Option<File>,
    /// Where the child's stderr streams to: toolr's own stderr.
    pub stderr_stream: Option<File>,
    pub timeout_secs: Option<f64>,
    pub no_output_timeout_secs: Option<f64>,
    pub cwd: Option<PathBuf>, // Current working directory
    /// Inherit stdin/stdout/stderr directly from the parent instead of
    /// piping them — required for interactive children (editors, prompts)
    /// that check `isatty()` on any of the three streams.
    pub interactive: bool,
}

impl Default for CommandConfig {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            env: HashMap::new(),
            input: None,
            stdout_capture: None,
            stderr_capture: None,
            stdout_stream: None,
            stderr_stream: None,
            timeout_secs: None,
            no_output_timeout_secs: None,
            cwd: env::current_dir().ok(), // Default to current working directory
            interactive: false,
        }
    }
}

impl CommandConfig {
    // Builder-style constructor
    pub fn new(args: Vec<String>) -> Self {
        Self {
            args,
            ..Self::default()
        }
    }

    // Optional builder methods for a more fluent API
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    pub fn with_input(mut self, input: Vec<u8>) -> Self {
        self.input = Some(input);
        self
    }

    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    // Add other builder methods as needed...
}

/// Feed `input` to the child from its own thread, dropping `stdin` afterwards to send EOF.
///
/// `BrokenPipe` means the child stopped reading; its exit status says why, as with
/// `subprocess.communicate`.
fn spawn_stdin_writer<W: Write + Send + 'static>(
    mut stdin: W,
    input: Vec<u8>,
) -> JoinHandle<io::Result<()>> {
    thread::spawn(move || match stdin.write_all(&input) {
        Err(e) if e.kind() == ErrorKind::BrokenPipe => Ok(()),
        result => result,
    })
}

/// Duplicate a C-runtime file descriptor, so the caller owns and closes its own copy.
///
/// The copy shares the file description, offset included, so output written through it is
/// visible to whoever holds the original. On Windows a closed fd aborts the process in the CRT's
/// invalid-parameter handler, so pass only fds just taken from an open file.
pub fn file_from_fd(fd: i32) -> io::Result<File> {
    // `fcntl` rather than `BorrowedFd`, so a closed fd is an error, not a broken contract.
    #[cfg(unix)]
    let owned = match unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) } {
        -1 => return Err(io::Error::last_os_error()),
        dup => unsafe { OwnedFd::from_raw_fd(dup) },
    };
    #[cfg(windows)]
    let owned = {
        // A negative fd would abort in the CRT's invalid-parameter handler; reject it up front.
        let handle = if fd < 0 {
            -1
        } else {
            unsafe { libc::get_osfhandle(fd) }
        };
        // `-1` for a bad fd, `-2` for a std stream with no console: neither is a handle.
        if handle < 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "Bad file descriptor",
            ));
        }
        unsafe { BorrowedHandle::borrow_raw(handle as RawHandle) }.try_clone_to_owned()?
    };
    Ok(File::from(owned))
}

/// State the reader threads share with the call.
struct ReaderState {
    /// When a reader last read or delivered output.
    last_output: Mutex<Instant>,
    /// Readers busy writing to a sink: a slow sink isn't the child running late.
    writing: AtomicUsize,
    /// Set when the call gives up on the readers; each stops at its next read.
    detached: AtomicBool,
    /// The first failed write a reader hit, returned once the readers are joined.
    error: Mutex<Option<String>>,
}

impl ReaderState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            last_output: Mutex::new(Instant::now()),
            writing: AtomicUsize::new(0),
            detached: AtomicBool::new(false),
            error: Mutex::new(None),
        })
    }

    fn record(&self, msg: String) {
        self.error.lock().unwrap().get_or_insert(msg);
    }

    fn stamp(&self) {
        *self.last_output.lock().unwrap() = Instant::now();
    }

    fn quiet_for(&self) -> Duration {
        self.last_output.lock().unwrap().elapsed()
    }

    fn idle(&self) -> bool {
        self.writing.load(Ordering::SeqCst) == 0
    }
}

/// Detaches the readers however the call returns; on success they've already finished.
struct DetachOnDrop(Arc<ReaderState>);

impl Drop for DetachOnDrop {
    fn drop(&mut self) {
        self.0.detached.store(true, Ordering::SeqCst);
    }
}

/// Copy `pipe` into the capture and stream files until EOF. With a capture the pipe is drained to
/// EOF even after a failed write, so the child finishes; a lost stream with no capture, or a
/// detached reader, drops the pipe so the writer gets EPIPE rather than feeding it forever.
fn spawn_reader<R: Read + Send + 'static>(
    mut pipe: R,
    name: &'static str,
    mut capture: Option<File>,
    mut stream: Option<File>,
    state: Arc<ReaderState>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        // A reader with no sinks also drains, so the no-output timeout keeps seeing output.
        let drain_to_eof = capture.is_some() || stream.is_none();
        let mut buffer = [0; 8192];
        while let Ok(n @ 1..) = pipe.read(&mut buffer)
            && !state.detached.load(Ordering::SeqCst)
        {
            state.writing.fetch_add(1, Ordering::SeqCst);
            state.stamp();
            let write = |sink: &mut File| sink.write_all(&buffer[..n]).and_then(|()| sink.flush());
            if let Some(Err(e)) = capture.as_mut().map(write) {
                state.record(format!("Failed to write captured {name}: {e}"));
                capture = None;
            }
            if let Some(Err(e)) = stream.as_mut().map(write) {
                // The reader of toolr's output went away, as after `| head`: not an error.
                if e.kind() != ErrorKind::BrokenPipe {
                    state.record(format!("Failed to write {name}: {e}"));
                }
                stream = None;
            }
            state.stamp();
            state.writing.fetch_sub(1, Ordering::SeqCst);
            if !drain_to_eof && stream.is_none() {
                break;
            }
        }
    })
}

/// The two limits, enforced in three phases of `run_command_internal`:
///
/// - **Running:** `total` counts from the spawn whatever the readers are doing; `no_output` only
///   while no reader is writing to a sink.
/// - **After exit**, while the readers drain: both count only while no reader is writing, so a
///   slow sink isn't mistaken for a background process holding the pipe.
/// - **After a kill:** the readers get a fixed `KILL_GRACE`, then are detached.
///
/// `expired` applies the first two; the third is the grace loop after `tree::kill_tree`.
struct Timeouts {
    total: Option<f64>,
    no_output: Option<f64>,
}

impl Timeouts {
    /// The error for whichever limit has run out, if any.
    fn expired(
        &self,
        start: Instant,
        readers: &ReaderState,
        child_running: bool,
    ) -> Option<Box<dyn Error + Send + Sync>> {
        let idle = readers.idle();
        if let Some(timeout) = self.total
            && (child_running || idle)
            && start.elapsed() > Duration::from_secs_f64(timeout)
        {
            return Some(Box::new(CommandTimeoutExceededError::new(format!(
                "Command timed out after {timeout:.2} seconds"
            ))));
        }
        if let Some(timeout) = self.no_output
            && idle
            && readers.quiet_for() > Duration::from_secs_f64(timeout)
        {
            return Some(Box::new(CommandNoOutputTimeoutError::new(format!(
                "Command timed out due to no output for {timeout:.2} seconds"
            ))));
        }
        None
    }
}

fn join_stdin_writer(handle: JoinHandle<io::Result<()>>) -> Result<(), CommandExecutionError> {
    match handle.join() {
        Ok(Err(e)) => Err(CommandExecutionError::new(format!(
            "Failed to write to stdin: {e}"
        ))),
        _ => Ok(()),
    }
}

// Core implementation used by both Rust and Python
pub fn run_command_internal(
    mut config: CommandConfig,
) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    // Create a command with the given arguments and environment variables
    let mut command = Command::new(&config.args[0]);
    command.args(&config.args[1..]);

    // Set environment variables
    for (key, value) in &config.env {
        command.env(key, value);
    }

    // Set current working directory if specified
    if let Some(ref dir) = config.cwd {
        command.current_dir(dir);
    }

    // Configure stdin:
    // - If input is provided, we'll use a pipe and write the input data
    // - Otherwise, inherit the stdin from the parent process
    if config.input.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::inherit());
    }

    // In interactive mode the child gets the parent's real stdout/stderr
    // (so `isatty()` succeeds for editors/prompts) instead of pipes routed
    // through the capture/streaming machinery below.
    if config.interactive {
        command.stdout(Stdio::inherit());
        command.stderr(Stdio::inherit());
    } else {
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
    }

    let (stdout_capture, stdout_stream) =
        (config.stdout_capture.take(), config.stdout_stream.take());
    let (stderr_capture, stderr_stream) =
        (config.stderr_capture.take(), config.stderr_stream.take());

    // Start the command
    let mut child = command.spawn().map_err(|e| {
        Box::new(CommandExecutionError::new(format!(
            "Failed to execute command: {e}"
        )))
    })?;

    // Written from its own thread once the readers run: a child that fills its
    // stdout pipe before reading stdin would otherwise deadlock us.
    let stdin_payload = child.stdin.take().zip(config.input.take());

    let readers = ReaderState::new();
    let _detach = DetachOnDrop(Arc::clone(&readers));

    // `None` in interactive mode, where stdio was inherited above rather than piped.
    let stdout_thread = child.stdout.take().map(|pipe| {
        spawn_reader(
            pipe,
            "stdout",
            stdout_capture,
            stdout_stream,
            Arc::clone(&readers),
        )
    });
    let stderr_thread = child.stderr.take().map(|pipe| {
        spawn_reader(
            pipe,
            "stderr",
            stderr_capture,
            stderr_stream,
            Arc::clone(&readers),
        )
    });

    // Spawned after the readers. On timeout the kill closes the pipe and the
    // thread ends on its own, or is detached after `KILL_GRACE`.
    let stdin_thread = stdin_payload.map(|(stdin, input)| spawn_stdin_writer(stdin, input));

    let all_finished = || {
        stdout_thread.as_ref().is_none_or(JoinHandle::is_finished)
            && stderr_thread.as_ref().is_none_or(JoinHandle::is_finished)
            && stdin_thread.as_ref().is_none_or(JoinHandle::is_finished)
    };
    let timeouts = Timeouts {
        total: config.timeout_secs,
        no_output: config.no_output_timeout_secs,
    };

    // Wait for command completion with timeout handling
    let start_time = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if let Some(err) = timeouts.expired(start_time, &readers, true) {
                    tree::kill_tree(&mut child);
                    let grace_ends = Instant::now() + KILL_GRACE;
                    while !all_finished() && Instant::now() < grace_ends {
                        thread::sleep(POLL);
                    }
                    return Err(err);
                }
                thread::sleep(POLL);
            }
            Err(e) => {
                return Err(Box::new(CommandExecutionError::new(format!(
                    "Failed to wait for process: {e}"
                ))));
            }
        }
    };

    // The child has exited, but something it started may still hold the pipes: the
    // timeouts cover the whole call, so they keep running until the threads finish.
    while !all_finished() {
        if let Some(err) = timeouts.expired(start_time, &readers, false) {
            return Err(err);
        }
        thread::sleep(POLL);
    }

    let stdin_result = stdin_thread.map_or(Ok(()), join_stdin_writer);

    // Wait for stdout/stderr threads to finish (absent in interactive mode)
    if let Some(t) = stdout_thread {
        let _ = t.join();
    }
    if let Some(t) = stderr_thread {
        let _ = t.join();
    }

    // Returned only now, so a write error doesn't leave the readers unjoined. Lost output
    // matters more than a stdin the child stopped reading; a timeout returned earlier and wins.
    if let Some(msg) = readers.error.lock().unwrap().take() {
        return Err(Box::new(CommandExecutionError::new(msg)));
    }
    stdin_result.map_err(Box::new)?;

    Ok(crate::exit_status::returncode(status))
}

#[cfg(test)]
mod tests;
