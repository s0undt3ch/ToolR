use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use std::fs::File;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::env;

#[cfg(unix)]
use std::os::unix::io::{RawFd, FromRawFd};

#[cfg(windows)]
use std::os::windows::io::{RawHandle, FromRawHandle};

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
        Self { message: message.into() }
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
        Self { message: message.into() }
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
        Self { message: message.into() }
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
    pub stdout_fd: Option<RawFd>,     // For capturing output to file
    #[cfg(unix)]
    pub stderr_fd: Option<RawFd>,     // For capturing output to file
    #[cfg(unix)]
    pub sys_stdout_fd: Option<RawFd>, // For streaming to output
    #[cfg(unix)]
    pub sys_stderr_fd: Option<RawFd>, // For streaming to error
    #[cfg(windows)]
    pub stdout_fd: Option<ThreadSafeHandle>,     // For capturing output to file
    #[cfg(windows)]
    pub stderr_fd: Option<ThreadSafeHandle>,     // For capturing output to file
    #[cfg(windows)]
    pub sys_stdout_fd: Option<ThreadSafeHandle>, // For streaming to output
    #[cfg(windows)]
    pub sys_stderr_fd: Option<ThreadSafeHandle>, // For streaming to error
    pub timeout_secs: Option<f64>,
    pub no_output_timeout_secs: Option<f64>,
    pub cwd: Option<PathBuf>,       // Current working directory
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
            cwd: env::current_dir().ok(),  // Default to current working directory
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

// Core implementation used by both Rust and Python
pub fn run_command_internal(config: CommandConfig) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
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

    // For stdout and stderr, we'll always use pipes
    // This allows us to handle them properly for timeout detection and redirection
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    // Start the command
    let mut child = command.spawn().map_err(|e| {
        Box::new(CommandExecutionError::new(format!("Failed to execute command: {e}")))
    })?;

    // If there's input, write it to stdin
    if let Some(input_data) = &config.input {
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(input_data).map_err(|e| {
                Box::new(CommandExecutionError::new(format!("Failed to write to stdin: {e}")))
            })?;
            // Explicitly drop stdin to close it
            drop(stdin);
        }
    }

    // Take stdout and stderr pipes
    let stdout = child.stdout.take().expect("Failed to get stdout handle");
    let stderr = child.stderr.take().expect("Failed to get stderr handle");

    // Track last output time for no_output_timeout_secs
    let last_output = Arc::new(Mutex::new(Instant::now()));

    // Setup stdout handling
    let last_output_clone = Arc::clone(&last_output);

    // Clone these Option values so they can be moved into the thread
    #[cfg(windows)]
    let stdout_fd_clone = config.stdout_fd.clone();
    #[cfg(windows)]
    let sys_stdout_fd_clone = config.sys_stdout_fd.clone();

    #[cfg(unix)]
    let stdout_fd = config.stdout_fd;
    #[cfg(unix)]
    let sys_stdout_fd = config.sys_stdout_fd;

    let stdout_thread = thread::spawn(move || {
        let mut buffer = [0; 8192];
        let mut reader = stdout;

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    // Update last output time
                    if let Ok(mut last) = last_output_clone.lock() {
                        *last = Instant::now();
                    }

                    // Write to capture file if requested
                    #[cfg(unix)]
                    if let Some(fd) = stdout_fd {
                        let mut file = unsafe {
                            File::from_raw_fd(fd)
                        };
                        if file.write_all(&buffer[0..n]).is_err() {
                            break;
                        }
                        if file.flush().is_err() {
                            break;
                        }
                        // Don't close the file descriptor - it's owned by Python
                        std::mem::forget(file);
                    }

                    #[cfg(windows)]
                    if let Some(ref handle) = stdout_fd_clone {
                        let mut file = unsafe {
                            File::from_raw_handle(handle.raw())
                        };
                        if file.write_all(&buffer[0..n]).is_err() {
                            break;
                        }
                        if file.flush().is_err() {
                            break;
                        }
                        // Don't close the handle - it's owned by Python
                        std::mem::forget(file);
                    }

                    // Stream to stdout if requested
                    #[cfg(unix)]
                    if let Some(fd) = sys_stdout_fd {
                        let mut file = unsafe {
                            File::from_raw_fd(fd)
                        };
                        if file.write_all(&buffer[0..n]).is_err() {
                            break;
                        }
                        if file.flush().is_err() {
                            break;
                        }
                        // Don't close the file descriptor - it's owned by Python
                        std::mem::forget(file);
                    }

                    #[cfg(windows)]
                    if let Some(ref handle) = sys_stdout_fd_clone {
                        let mut file = unsafe {
                            File::from_raw_handle(handle.raw())
                        };
                        if file.write_all(&buffer[0..n]).is_err() {
                            break;
                        }
                        if file.flush().is_err() {
                            break;
                        }
                        // Don't close the handle - it's owned by Python
                        std::mem::forget(file);
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Setup stderr handling - similar to stdout
    let last_output_clone = Arc::clone(&last_output);

    // Clone these Option values so they can be moved into the thread
    #[cfg(windows)]
    let stderr_fd_clone = config.stderr_fd.clone();
    #[cfg(windows)]
    let sys_stderr_fd_clone = config.sys_stderr_fd.clone();

    #[cfg(unix)]
    let stderr_fd = config.stderr_fd;
    #[cfg(unix)]
    let sys_stderr_fd = config.sys_stderr_fd;

    let stderr_thread = thread::spawn(move || {
        let mut buffer = [0; 8192];
        let mut reader = stderr;

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    // Update last output time
                    if let Ok(mut last) = last_output_clone.lock() {
                        *last = Instant::now();
                    }

                    // Write to capture file if requested
                    #[cfg(unix)]
                    if let Some(fd) = stderr_fd {
                        let mut file = unsafe {
                            File::from_raw_fd(fd)
                        };
                        if file.write_all(&buffer[0..n]).is_err() {
                            break;
                        }
                        if file.flush().is_err() {
                            break;
                        }
                        // Don't close the file descriptor - it's owned by Python
                        std::mem::forget(file);
                    }

                    #[cfg(windows)]
                    if let Some(ref handle) = stderr_fd_clone {
                        let mut file = unsafe {
                            File::from_raw_handle(handle.raw())
                        };
                        if file.write_all(&buffer[0..n]).is_err() {
                            break;
                        }
                        if file.flush().is_err() {
                            break;
                        }
                        // Don't close the handle - it's owned by Python
                        std::mem::forget(file);
                    }

                    // Stream to stderr if requested
                    #[cfg(unix)]
                    if let Some(fd) = sys_stderr_fd {
                        let mut file = unsafe {
                            File::from_raw_fd(fd)
                        };
                        if file.write_all(&buffer[0..n]).is_err() {
                            break;
                        }
                        if file.flush().is_err() {
                            break;
                        }
                        // Don't close the file descriptor - it's owned by Python
                        std::mem::forget(file);
                    }

                    #[cfg(windows)]
                    if let Some(ref handle) = sys_stderr_fd_clone {
                        let mut file = unsafe {
                            File::from_raw_handle(handle.raw())
                        };
                        if file.write_all(&buffer[0..n]).is_err() {
                            break;
                        }
                        if file.flush().is_err() {
                            break;
                        }
                        // Don't close the handle - it's owned by Python
                        std::mem::forget(file);
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Wait for command completion with timeout handling
    let start_time = Instant::now();
    let status = loop {
        // Check for command completion
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                // Check for timeout
                if let Some(timeout) = config.timeout_secs {
                    // Convert float seconds to Duration
                    let timeout_duration = Duration::from_secs_f64(timeout);
                    if start_time.elapsed() > timeout_duration {
                        let _ = child.kill();
                        let _ = child.wait();  // Make sure it's dead

                        return Err(Box::new(CommandTimeoutExceededError::new(
                            format!("Command timed out after {timeout:.2} seconds")
                        )));
                    }
                }

                // Check for no-output timeout - when monitoring for output
                if let Some(no_output_timeout) = config.no_output_timeout_secs {
                    let last_output_time = *last_output.lock().unwrap();

                    // Convert float seconds to Duration
                    let no_output_duration = Duration::from_secs_f64(no_output_timeout);
                    if last_output_time.elapsed() > no_output_duration {
                        let _ = child.kill();
                        let _ = child.wait();  // Make sure it's dead

                        return Err(Box::new(CommandNoOutputTimeoutError::new(
                            format!("Command timed out due to no output for {no_output_timeout:.2} seconds")
                        )));
                    }
                }

                thread::sleep(Duration::from_millis(10));
            },
            Err(e) => {
                return Err(Box::new(CommandExecutionError::new(
                    format!("Failed to wait for process: {e}")
                )));
            }
        }
    };

    // Wait for stdout/stderr threads to finish
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();

    // Get exit code and return it, regardless of value
    let returncode = status.code().unwrap_or(-1);
    Ok(returncode)
}

#[cfg(test)]
mod tests;
