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
use std::os::unix::io::{FromRawFd, OwnedFd, RawFd};

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

#[cfg(windows)]
#[derive(Debug, Clone)]
pub struct ThreadSafeHandle {
    raw_handle: RawHandle,
}

#[cfg(windows)]
unsafe impl Send for ThreadSafeHandle {}

#[cfg(windows)]
unsafe impl Sync for ThreadSafeHandle {}

#[cfg(windows)]
impl ThreadSafeHandle {
    pub fn new(handle: RawHandle) -> Self {
        Self { raw_handle: handle }
    }

    pub fn raw(&self) -> RawHandle {
        self.raw_handle
    }
}

// Pure Rust implementation - available always
pub struct CommandConfig {
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub input: Option<Vec<u8>>,
    #[cfg(unix)]
    pub stdout_fd: Option<RawFd>, // For capturing output to file
    #[cfg(unix)]
    pub stderr_fd: Option<RawFd>, // For capturing output to file
    #[cfg(unix)]
    pub sys_stdout_fd: Option<RawFd>, // For streaming to output
    #[cfg(unix)]
    pub sys_stderr_fd: Option<RawFd>, // For streaming to error
    #[cfg(windows)]
    pub stdout_fd: Option<ThreadSafeHandle>, // For capturing output to file
    #[cfg(windows)]
    pub stderr_fd: Option<ThreadSafeHandle>, // For capturing output to file
    #[cfg(windows)]
    pub sys_stdout_fd: Option<ThreadSafeHandle>, // For streaming to output
    #[cfg(windows)]
    pub sys_stderr_fd: Option<ThreadSafeHandle>, // For streaming to error
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
            #[cfg(unix)]
            stdout_fd: None,
            #[cfg(unix)]
            stderr_fd: None,
            #[cfg(unix)]
            sys_stdout_fd: None,
            #[cfg(unix)]
            sys_stderr_fd: None,
            #[cfg(windows)]
            stdout_fd: None,
            #[cfg(windows)]
            stderr_fd: None,
            #[cfg(windows)]
            sys_stdout_fd: None,
            #[cfg(windows)]
            sys_stderr_fd: None,
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

#[cfg(unix)]
type Sink = RawFd;
#[cfg(windows)]
type Sink = ThreadSafeHandle;

/// Duplicate a caller-owned fd (or handle) so the reader thread owns and closes its copy.
fn own_sink(sink: Option<&Sink>) -> io::Result<Option<File>> {
    // `fcntl` rather than `BorrowedFd`, so a closed fd is an error, not a broken contract.
    #[cfg(unix)]
    let owned = |fd: &RawFd| match unsafe { libc::fcntl(*fd, libc::F_DUPFD_CLOEXEC, 0) } {
        -1 => Err(io::Error::last_os_error()),
        dup => Ok(unsafe { OwnedFd::from_raw_fd(dup) }),
    };
    #[cfg(windows)]
    let owned =
        |h: &ThreadSafeHandle| unsafe { BorrowedHandle::borrow_raw(h.raw()) }.try_clone_to_owned();
    sink.map(|sink| owned(sink).map(File::from)).transpose()
}

/// State the reader threads share with the call.
struct ReaderState {
    /// When a reader last read or delivered output.
    last_output: Mutex<Instant>,
    /// Readers busy writing to a sink: a slow sink isn't the child running late.
    writing: AtomicUsize,
    /// Set when the call gives up on the readers; each stops at its next read.
    detached: AtomicBool,
}

impl ReaderState {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            last_output: Mutex::new(Instant::now()),
            writing: AtomicUsize::new(0),
            detached: AtomicBool::new(false),
        })
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

/// Copy `pipe` into each sink until EOF, stamping the output clock after each read and write. A
/// failed write stops the copy. A detached reader stops at its next read and drops the
/// pipe, so a writer that escaped the kill gets EPIPE rather than feeding it forever.
fn spawn_reader<R: Read + Send + 'static>(
    mut pipe: R,
    sinks: [Option<File>; 2],
    state: Arc<ReaderState>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let mut sinks: Vec<File> = sinks.into_iter().flatten().collect();
        let mut buffer = [0; 8192];
        while let Ok(n @ 1..) = pipe.read(&mut buffer) {
            state.writing.fetch_add(1, Ordering::SeqCst);
            state.stamp();
            let failed = state.detached.load(Ordering::SeqCst)
                || sinks.iter_mut().any(|sink| {
                    sink.write_all(&buffer[..n])
                        .and_then(|()| sink.flush())
                        .is_err()
                });
            state.stamp();
            state.writing.fetch_sub(1, Ordering::SeqCst);
            if failed {
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

    // Owned copies, made before the spawn: a thread detached on timeout keeps writing to its
    // own fd, never to a number the caller has closed and the OS has handed out again.
    let own = |sink| {
        own_sink(sink).map_err(|e| {
            Box::new(CommandExecutionError::new(format!(
                "Failed to duplicate an output handle: {e}"
            )))
        })
    };
    let stdout_sinks = [
        own(config.stdout_fd.as_ref())?,
        own(config.sys_stdout_fd.as_ref())?,
    ];
    let stderr_sinks = [
        own(config.stderr_fd.as_ref())?,
        own(config.sys_stderr_fd.as_ref())?,
    ];

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
    let stdout_thread = child
        .stdout
        .take()
        .map(|pipe| spawn_reader(pipe, stdout_sinks, Arc::clone(&readers)));
    let stderr_thread = child
        .stderr
        .take()
        .map(|pipe| spawn_reader(pipe, stderr_sinks, Arc::clone(&readers)));

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

    // Returned only now, so a write error doesn't leave the readers unjoined.
    stdin_result.map_err(Box::new)?;

    Ok(crate::exit_status::returncode(status))
}

#[cfg(test)]
mod tests;
