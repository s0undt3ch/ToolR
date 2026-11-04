#[cfg(test)]
mod test_suite {
    use anyhow::{Result, anyhow};
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};
    use tempfile::NamedTempFile;

    // Platform-specific imports
    #[cfg(windows)]
    #[cfg(windows)]
    use crate::command::ThreadSafeHandle;
    #[cfg(unix)]
    use std::os::unix::io::AsRawFd;
    #[cfg(windows)]
    use std::os::windows::io::AsRawHandle;
    #[cfg(windows)]
    use winapi::um::fileapi::ReadFile;
    #[cfg(windows)]
    use winapi::um::handleapi::{DuplicateHandle, INVALID_HANDLE_VALUE};
    #[cfg(windows)]
    use winapi::um::minwinbase::SECURITY_ATTRIBUTES;
    #[cfg(windows)]
    use winapi::um::namedpipeapi::CreatePipe;
    #[cfg(windows)]
    use winapi::um::processthreadsapi::GetCurrentProcess;
    #[cfg(windows)]
    use winapi::um::winnt::DUPLICATE_SAME_ACCESS;

    use crate::command::tree::terminate;
    use crate::{CommandConfig, run_command_internal};

    // Helper function to get file descriptor/handle from File in a cross-platform way
    #[cfg(unix)]
    fn get_file_descriptor(file: &File) -> i32 {
        file.as_raw_fd()
    }

    #[cfg(windows)]
    fn get_file_descriptor(file: &File) -> ThreadSafeHandle {
        // Get the actual Windows handle from the file
        let handle = file.as_raw_handle();
        // Ensure we duplicate the handle so it remains valid
        unsafe {
            let mut new_handle = INVALID_HANDLE_VALUE;
            if DuplicateHandle(
                GetCurrentProcess(),
                handle as *mut _, // Let the compiler infer the void type
                GetCurrentProcess(),
                &mut new_handle,
                0,
                1,
                DUPLICATE_SAME_ACCESS,
            ) != 0
            {
                ThreadSafeHandle::new(new_handle as *mut _) // Convert to RawHandle
            } else {
                // For tests, fallback to a dummy handle if duplication fails
                ThreadSafeHandle::new(handle as *mut _) // Convert to RawHandle
            }
        }
    }

    /// Resolve a usable Python interpreter for the subprocess-based tests
    /// in this module. Tries `python3` first (the canonical mise / Debian
    /// install path) and falls back to bare `python` (older systems +
    /// some Windows installs). Returns `None` when neither is callable
    /// — callers should skip cleanly instead of hanging on a failed spawn
    /// (the no-output-timeout pipe-read path used to deadlock in this case).
    fn resolve_python_for_test() -> Option<String> {
        let candidates: &[&str] = if cfg!(windows) {
            &["python3.exe", "python.exe", "python3", "python"]
        } else {
            &["python3", "python"]
        };
        for candidate in candidates {
            let ok = Command::new(candidate)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok {
                return Some((*candidate).to_string());
            }
        }
        None
    }

    /// Skip-or-bind macro for tests that need Python. Emits a clear
    /// `skipping <name>` line on stderr when no interpreter resolves and
    /// returns `Ok(())` so the test counts as passed; binds the resolved
    /// interpreter path to `$bind` otherwise.
    macro_rules! require_python {
        ($bind:ident, $test_name:expr) => {
            let Some($bind) = resolve_python_for_test() else {
                eprintln!(concat!("skipping ", $test_name, ": no python on PATH"));
                return Ok(());
            };
        };
    }

    // Helper function for Option wrapping to handle platform differences
    #[cfg(unix)]
    fn wrap_fd(fd: i32) -> Option<i32> {
        Some(fd)
    }

    #[cfg(windows)]
    fn wrap_fd(handle: ThreadSafeHandle) -> Option<ThreadSafeHandle> {
        Some(handle)
    }

    #[cfg(windows)]
    fn create_pipe() -> Result<(ThreadSafeHandle, ThreadSafeHandle)> {
        unsafe {
            let mut read_handle = INVALID_HANDLE_VALUE;
            let mut write_handle = INVALID_HANDLE_VALUE;
            let mut sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: std::ptr::null_mut(),
                bInheritHandle: 1,
            };

            if CreatePipe(&mut read_handle, &mut write_handle, &mut sa, 0) == 0 {
                // Fallback to temp file if pipe creation fails
                let file = NamedTempFile::new()?;
                let read = file.reopen()?;
                let write = file.into_file();
                Ok((
                    ThreadSafeHandle::new(read.as_raw_handle() as *mut _),
                    ThreadSafeHandle::new(write.as_raw_handle() as *mut _),
                ))
            } else {
                Ok((
                    ThreadSafeHandle::new(read_handle as *mut _),
                    ThreadSafeHandle::new(write_handle as *mut _),
                ))
            }
        }
    }

    #[cfg(windows)]
    fn read_pipe(handle: ThreadSafeHandle, buffer: &mut [u8]) -> i32 {
        let mut bytes_read = 0;
        unsafe {
            let raw_handle = handle.raw() as *mut winapi::ctypes::c_void;
            if raw_handle != INVALID_HANDLE_VALUE as *mut winapi::ctypes::c_void
                && ReadFile(
                    raw_handle,
                    buffer.as_mut_ptr() as *mut winapi::ctypes::c_void,
                    buffer.len() as u32,
                    &mut bytes_read,
                    std::ptr::null_mut(),
                ) != 0
            {
                bytes_read as i32
            } else {
                0
            }
        }
    }

    #[test]
    fn test_command_execution() -> Result<()> {
        let output = Command::new("echo")
            .arg("Hello, World!")
            .stdout(Stdio::piped())
            .output()?;

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello, World!\n");

        Ok(())
    }

    #[test]
    fn test_fd_streaming_simulation() -> Result<()> {
        // Create files for stdout and stderr
        let mut stdout_file = NamedTempFile::new()?;
        let mut stderr_file = NamedTempFile::new()?;

        // Write test data to the files
        stdout_file.write_all(b"to stdout\n")?;
        stdout_file.flush()?;

        stderr_file.write_all(b"to stderr\n")?;
        stderr_file.flush()?;

        // Rewind to the beginning to read contents
        stdout_file.seek(SeekFrom::Start(0))?;
        stderr_file.seek(SeekFrom::Start(0))?;

        // Read back the data
        let mut stdout_content = String::new();
        let mut stderr_content = String::new();

        stdout_file.read_to_string(&mut stdout_content)?;
        stderr_file.read_to_string(&mut stderr_content)?;

        // Check content
        assert!(
            stdout_content.contains("to stdout"),
            "Expected 'to stdout' in '{stdout_content}'"
        );
        assert!(
            stderr_content.contains("to stderr"),
            "Expected 'to stderr' in '{stderr_content}'"
        );

        Ok(())
    }

    #[test]
    fn test_fd_with_capture_simulation() -> Result<()> {
        // Create files for stdout and stderr
        let mut stdout_file = NamedTempFile::new()?;
        let mut stderr_file = NamedTempFile::new()?;

        // Create temporary files for capture
        let mut stdout_capture = NamedTempFile::new()?;
        let mut stderr_capture = NamedTempFile::new()?;

        // Write test data to files
        stdout_file.write_all(b"to both stdout\n")?;
        stdout_file.flush()?;

        stderr_file.write_all(b"to both stderr\n")?;
        stderr_file.flush()?;

        // Simulate capturing by copying data to capture files
        stdout_file.seek(SeekFrom::Start(0))?;
        stderr_file.seek(SeekFrom::Start(0))?;

        let mut buffer = [0u8; 4096];
        loop {
            let n = stdout_file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            stdout_capture.write_all(&buffer[0..n])?;
        }
        stdout_capture.flush()?;

        loop {
            let n = stderr_file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            stderr_capture.write_all(&buffer[0..n])?;
        }
        stderr_capture.flush()?;

        // Verify capture files contain the data
        stdout_capture.seek(SeekFrom::Start(0))?;
        stderr_capture.seek(SeekFrom::Start(0))?;

        let mut stdout_capture_content = String::new();
        let mut stderr_capture_content = String::new();

        stdout_capture.read_to_string(&mut stdout_capture_content)?;
        stderr_capture.read_to_string(&mut stderr_capture_content)?;

        assert!(
            stdout_capture_content.contains("to both stdout"),
            "Expected 'to both stdout' in '{stdout_capture_content}'"
        );
        assert!(
            stderr_capture_content.contains("to both stderr"),
            "Expected 'to both stderr' in '{stderr_capture_content}'"
        );

        Ok(())
    }

    #[test]
    fn test_timeout_simulation() -> Result<()> {
        // Create a named pipe file for reading
        let reader = NamedTempFile::new()?;

        // Start time for timeout tracking
        let start_time = Instant::now();

        // In a real thread, we'd now wait for I/O or timeout
        let timeout = Duration::from_millis(100);

        // Simulate no data arriving (timeout)
        thread::sleep(timeout);

        // Read and verify we have no data yet
        let mut buf = [0; 10];
        let mut reader_file = reader.reopen()?;
        let bytes_read = reader_file.read(&mut buf)?;

        // Since we're using a tempfile and not a real pipe, we might get 0 or EOF
        assert_eq!(bytes_read, 0, "Expected 0 bytes read, got {bytes_read}");
        assert!(start_time.elapsed() >= timeout, "Timeout not respected");

        Ok(())
    }

    #[test]
    fn test_environment_variables() -> Result<()> {
        require_python!(python, "test_environment_variables");

        let mut env = HashMap::new();
        env.insert("TEST_VAR".to_string(), "test_value".to_string());
        env.insert("ANOTHER_VAR".to_string(), "another_value".to_string());

        // Create temporary files for stdout and stderr
        let mut stdout_file = NamedTempFile::new()?;
        let stderr_file = NamedTempFile::new()?;

        // Get file descriptors in a cross-platform way
        let stdout_fd = get_file_descriptor(stdout_file.as_file());
        let stderr_fd = get_file_descriptor(stderr_file.as_file());

        let config = CommandConfig {
            args: vec![
                python,
                "-c".to_string(),
                "import os; print('TEST_VAR=' + os.environ['TEST_VAR']); print('ANOTHER_VAR=' + os.environ['ANOTHER_VAR'])".to_string(),
            ],
            env,
            stdout_fd: wrap_fd(stdout_fd),
            stderr_fd: wrap_fd(stderr_fd),
            ..Default::default()
        };

        // Run the command
        let result = run_command_internal(config);

        // Check command completed successfully
        assert!(result.is_ok(), "Command failed: {result:?}");
        assert_eq!(result.unwrap(), 0, "Command should return exit code 0");

        // Read captured stdout
        stdout_file.seek(SeekFrom::Start(0))?;
        let mut stdout_content = String::new();
        stdout_file.read_to_string(&mut stdout_content)?;

        // Verify environment variables were correctly set
        assert!(
            stdout_content.contains("TEST_VAR=test_value"),
            "Expected 'TEST_VAR=test_value' in stdout, got: {stdout_content}"
        );
        assert!(
            stdout_content.contains("ANOTHER_VAR=another_value"),
            "Expected 'ANOTHER_VAR=another_value' in stdout, got: {stdout_content}"
        );

        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_signal_killed_child_returncode_is_negative() -> Result<()> {
        let config = CommandConfig {
            args: vec!["sh".into(), "-c".into(), "kill -TERM $$".into()],
            ..Default::default()
        };
        let returncode = run_command_internal(config).map_err(|e| anyhow!("{e}"))?;
        assert_eq!(returncode, -libc::SIGTERM);
        Ok(())
    }

    #[test]
    fn test_command_respects_cwd() -> Result<()> {
        require_python!(python, "test_command_respects_cwd");

        // Create a temporary directory to use as our working directory
        let temp_dir = tempfile::tempdir()?;
        let temp_path = temp_dir.path().to_owned();

        // Create temporary files for capturing stdout
        let mut stdout_file = NamedTempFile::new()?;
        let stderr_file = NamedTempFile::new()?;

        let stdout_fd = get_file_descriptor(stdout_file.as_file());
        let stderr_fd = get_file_descriptor(stderr_file.as_file());

        // Create a command config that runs 'pwd' with the temp directory as working directory
        let config = CommandConfig {
            args: vec![
                python.clone(),
                "-c".to_string(),
                "import os; print(os.getcwd())".to_string(),
            ],
            cwd: Some(temp_path.clone()),
            stdout_fd: wrap_fd(stdout_fd),
            stderr_fd: wrap_fd(stderr_fd),
            ..Default::default()
        };

        // Run the command
        let result = run_command_internal(config);

        // Check command completed successfully
        assert!(result.is_ok(), "Command failed: {result:?}");
        assert_eq!(result.unwrap(), 0, "Command should return exit code 0");

        // Read captured stdout
        stdout_file.seek(SeekFrom::Start(0))?;
        let mut stdout_content = String::new();
        stdout_file.read_to_string(&mut stdout_content)?;

        // Trim the output to remove any trailing newline
        let captured_path = stdout_content.trim();
        let temp_path_str = temp_path.to_string_lossy();

        // Convert both paths to canonicalized paths to handle symlinks
        // (On macOS, /var is a symlink to /private/var)
        let captured_path_buf = std::path::PathBuf::from(captured_path);
        let expected_path_buf = temp_path.clone();

        let canonical_captured = captured_path_buf
            .canonicalize()
            .unwrap_or(captured_path_buf);
        let canonical_expected = expected_path_buf
            .canonicalize()
            .unwrap_or(expected_path_buf);

        // Compare the canonicalized paths
        assert_eq!(
            canonical_captured,
            canonical_expected,
            "Expected current working directory '{}', got '{}'",
            canonical_expected.display(),
            canonical_captured.display()
        );

        // Now test that without setting cwd, pwd returns a different directory
        let mut stdout_file2 = NamedTempFile::new()?;
        let stderr_file2 = NamedTempFile::new()?;

        let stdout_fd2 = get_file_descriptor(stdout_file2.as_file());
        let stderr_fd2 = get_file_descriptor(stderr_file2.as_file());

        // Create a command config that runs 'pwd' without setting the working directory
        let config2 = CommandConfig {
            args: vec![
                python,
                "-c".to_string(),
                "import os; print(os.getcwd())".to_string(),
            ],
            stdout_fd: wrap_fd(stdout_fd2),
            stderr_fd: wrap_fd(stderr_fd2),
            ..Default::default()
        };

        // Run the command
        let result2 = run_command_internal(config2);

        // Check command completed successfully
        assert!(result2.is_ok(), "Command failed: {result2:?}");
        assert_eq!(result2.unwrap(), 0, "Command should return exit code 0");

        // Read captured stdout
        stdout_file2.seek(SeekFrom::Start(0))?;
        let mut stdout_content2 = String::new();
        stdout_file2.read_to_string(&mut stdout_content2)?;

        // Verify the working directory is different when not specified
        assert_ne!(
            stdout_content2.trim(),
            temp_path_str.as_ref(),
            "Working directory should be different when cwd is not specified"
        );

        Ok(())
    }

    mod subprocess_tests {
        use super::*;
        use std::sync::mpsc;
        use tokio::runtime::Runtime;

        /// Run `run_command_internal` on its own thread, bounded by a wall-clock
        /// timeout. Returns the inner result on completion, or an `Err` describing
        /// the wall-clock breach.
        ///
        /// The bound exists so a future regression that wedges
        /// `run_command_internal` (a stuck forwarder thread, a missed kill, etc.)
        /// fails the test fast instead of hanging the entire workspace test run.
        /// See issue #247 for the original `cargo test --workspace` hang this
        /// guards against. A plain thread, not `spawn_blocking`: a tokio runtime
        /// waits for its blocking tasks when dropped, so a wedged call would hang
        /// the test anyway. On timeout the thread is leaked.
        fn bounded_run_command(
            config: CommandConfig,
            timeout: Duration,
        ) -> Result<std::result::Result<i32, Box<dyn std::error::Error + Send + Sync>>> {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                let _ = tx.send(run_command_internal(config));
            });
            rx.recv_timeout(timeout)
                .map_err(|_| anyhow!("run_command_internal did not return within {:?}", timeout))
        }

        // Cross-platform pipe creation
        #[cfg(unix)]
        fn create_pipe() -> Result<(i32, i32)> {
            let (pipe_read, pipe_write) = unsafe {
                let mut fds = [0; 2];
                if libc::pipe(fds.as_mut_ptr()) == 0 {
                    (fds[0], fds[1])
                } else {
                    return Err(anyhow!("Failed to create pipe"));
                }
            };
            Ok((pipe_read, pipe_write))
        }

        #[cfg(windows)]
        fn create_pipe() -> Result<(ThreadSafeHandle, ThreadSafeHandle)> {
            super::create_pipe()
        }

        // Platform-specific read operation
        #[cfg(unix)]
        unsafe fn read_pipe(fd: i32, buffer: &mut [u8]) -> i32 {
            unsafe { libc::read(fd, buffer.as_mut_ptr() as *mut libc::c_void, buffer.len()) as i32 }
        }

        #[cfg(windows)]
        fn read_pipe(fd: ThreadSafeHandle, buffer: &mut [u8]) -> i32 {
            super::read_pipe(fd, buffer)
        }

        #[test]
        fn test_sys_fd_streaming() -> Result<()> {
            require_python!(python, "test_sys_fd_streaming");
            let rt = Runtime::new()?;

            rt.block_on(async {
                let (stdout_read, stdout_write) = create_pipe()?;
                let (stderr_read, stderr_write) = create_pipe()?;

                let config = CommandConfig {
                    args: vec![
                        python,
                        "-c".to_string(),
                        "import sys; sys.stdout.write('to sys stdout'); sys.stdout.flush(); sys.stderr.write('to sys stderr'); sys.stderr.flush()".to_string(),
                    ],
                    sys_stdout_fd: wrap_fd(stdout_write),
                    sys_stderr_fd: wrap_fd(stderr_write),
                    ..Default::default()
                };

                // Run the command in a separate thread to avoid blocking
                let handle = std::thread::spawn(move || {
                    run_command_internal(config)
                });

                // Read from the pipes
                let mut stdout_buffer = [0u8; 1024];
                let mut stderr_buffer = [0u8; 1024];

                #[cfg(unix)]
                let stdout_bytes = unsafe { read_pipe(stdout_read, &mut stdout_buffer) };
                #[cfg(unix)]
                let stderr_bytes = unsafe { read_pipe(stderr_read, &mut stderr_buffer) };

                #[cfg(windows)]
                let stdout_bytes = read_pipe(stdout_read, &mut stdout_buffer);
                #[cfg(windows)]
                let stderr_bytes = read_pipe(stderr_read, &mut stderr_buffer);

                // Wait for command to finish
                let result = handle.join().expect("Thread panicked");
                // Convert Result<i32, Box<dyn Error>> to anyhow::Result<i32>
                let exit_code = match result {
                    Ok(code) => Ok(code),
                    Err(e) => Err(anyhow!("{}", e)),
                }?;

                // Clean up
                #[cfg(unix)]
                unsafe {
                    libc::close(stdout_read);
                    libc::close(stderr_read);
                    libc::close(stdout_write);
                    libc::close(stderr_write);
                }

                #[cfg(windows)]
                {
                    // On Windows our test files will close automatically
                }

                // Convert output to strings
                let stdout_content = String::from_utf8_lossy(&stdout_buffer[0..stdout_bytes as usize]);
                let stderr_content = String::from_utf8_lossy(&stderr_buffer[0..stderr_bytes as usize]);

                assert_eq!(exit_code, 0, "Command should succeed");
                assert!(stdout_content.contains("to sys stdout"), "Expected 'to sys stdout', got '{stdout_content}'");
                assert!(stderr_content.contains("to sys stderr"), "Expected 'to sys stderr', got '{stderr_content}'");

                Ok(())
            })
        }

        #[test]
        fn test_sys_fd_with_capture() -> Result<()> {
            require_python!(python, "test_sys_fd_with_capture");
            let rt = Runtime::new()?;

            rt.block_on(async {
                let (stdout_read, stdout_write) = create_pipe()?;
                let (stderr_read, stderr_write) = create_pipe()?;

                let mut stdout_capture = NamedTempFile::new()?;
                let mut stderr_capture = NamedTempFile::new()?;

                let stdout_fd = get_file_descriptor(stdout_capture.as_file());
                let stderr_fd = get_file_descriptor(stderr_capture.as_file());

                let config = CommandConfig {
                    args: vec![
                        python,
                        "-c".to_string(),
                        "import sys; sys.stdout.write('captured stdout'); sys.stdout.flush(); sys.stderr.write('captured stderr'); sys.stderr.flush()".to_string(),
                    ],
                    stdout_fd: wrap_fd(stdout_fd),
                    stderr_fd: wrap_fd(stderr_fd),
                    sys_stdout_fd: wrap_fd(stdout_write),
                    sys_stderr_fd: wrap_fd(stderr_write),
                    ..Default::default()
                };

                let handle = std::thread::spawn(move || {
                    run_command_internal(config)
                });

                let mut stdout_buffer = [0u8; 1024];
                let mut stderr_buffer = [0u8; 1024];

                #[cfg(unix)]
                let stdout_bytes = unsafe { read_pipe(stdout_read, &mut stdout_buffer) };
                #[cfg(unix)]
                let stderr_bytes = unsafe { read_pipe(stderr_read, &mut stderr_buffer) };

                #[cfg(windows)]
                let stdout_bytes = read_pipe(stdout_read, &mut stdout_buffer);
                #[cfg(windows)]
                let stderr_bytes = read_pipe(stderr_read, &mut stderr_buffer);

                let result = handle.join().expect("Thread panicked");
                let exit_code = match result {
                    Ok(code) => Ok(code),
                    Err(e) => Err(anyhow!("{}", e)),
                }?;

                #[cfg(unix)]
                unsafe {
                    libc::close(stdout_read);
                    libc::close(stderr_read);
                    libc::close(stdout_write);
                    libc::close(stderr_write);
                }

                #[cfg(windows)]
                {
                    // On Windows our test files will close automatically
                }

                let stdout_streamed = String::from_utf8_lossy(&stdout_buffer[0..stdout_bytes as usize]);
                let stderr_streamed = String::from_utf8_lossy(&stderr_buffer[0..stderr_bytes as usize]);

                let mut stdout_captured = String::new();
                let mut stderr_captured = String::new();

                stdout_capture.seek(SeekFrom::Start(0))?;
                stderr_capture.seek(SeekFrom::Start(0))?;

                stdout_capture.read_to_string(&mut stdout_captured)?;
                stderr_capture.read_to_string(&mut stderr_captured)?;

                assert_eq!(exit_code, 0, "Command should succeed");

                assert!(stdout_streamed.contains("captured stdout"), "Streamed stdout should contain 'captured stdout'");
                assert!(stderr_streamed.contains("captured stderr"), "Streamed stderr should contain 'captured stderr'");

                assert!(stdout_captured.contains("captured stdout"), "Captured stdout should contain 'captured stdout'");
                assert!(stderr_captured.contains("captured stderr"), "Captured stderr should contain 'captured stderr'");

                Ok(())
            })
        }

        #[test]
        fn test_timeout_exception() -> Result<()> {
            require_python!(python, "test_timeout_exception");
            // Create a command that will run longer than our timeout
            let config = CommandConfig {
                args: vec![
                    python,
                    "-c".to_string(),
                    "import time; time.sleep(10)".to_string(),
                ],
                timeout_secs: Some(0.5), // Short timeout; 0.5s tolerates workspace contention
                ..Default::default()
            };

            // Wall-clock-bounded so the workspace test run can't hang if the
            // internal kill+wait path ever wedges. See issue #247.
            let result = bounded_run_command(config, Duration::from_secs(5))?;

            // Should fail with timeout
            assert!(result.is_err());
            let error_str = format!("{}", result.unwrap_err());

            // Check error message attributes
            assert!(
                error_str.contains("timed out after"),
                "Error should mention timeout duration"
            );

            Ok(())
        }

        #[test]
        fn test_no_output_timeout() -> Result<()> {
            require_python!(python, "test_no_output_timeout");
            // Create a command that outputs once then waits, triggering no-output timeout.
            //
            // We intentionally do not wire up a `sys_stdout_fd` pipe here: the no-output
            // watchdog updates `last_output` from the stdout-forwarder thread's read
            // against `child.stdout` (the parent-owned pipe), which works whether or not
            // a sys-fd consumer is present. Earlier revisions of this test passed a
            // test-owned pipe and called `read_pipe` *before* `handle.join()`; under
            // `cargo test --workspace` contention the watchdog could fire and kill
            // python before any byte made it to that pipe, leaving the test stuck in a
            // blocking read on a writer that nobody had closed. See issue #247.
            let config = CommandConfig {
                    args: vec![
                        python,
                        "-c".to_string(),
                        "import sys, time; sys.stdout.write('initial output'); sys.stdout.flush(); time.sleep(10)".to_string(),
                    ],
                    no_output_timeout_secs: Some(0.5),
                    ..Default::default()
                };

            // Bound the call with a wall-clock guard so any future regression that
            // wedges `run_command_internal` fails fast instead of hanging the
            // workspace. The watchdog should trip in ~0.5s; 5s is generous headroom.
            let result = bounded_run_command(config, Duration::from_secs(5))?;

            // Should fail with no-output timeout
            assert!(result.is_err());
            let error_str = format!("{}", result.unwrap_err());

            assert!(
                error_str.contains("no output for"),
                "Error should mention no-output timeout"
            );

            Ok(())
        }

        /// Must exceed the OS pipe buffer (~64 KiB), or the test passes against the deadlock.
        const PAST_PIPE_BUFFER: usize = 200_000;

        /// Child that fills its stdout pipe before it reads stdin. Shared with
        /// `test_input_larger_than_pipe_buffer_with_output_first` in
        /// `tests/utils/command/test_command.py`.
        fn output_first_script() -> String {
            format!(
                "import sys; sys.stdout.write('x'*{PAST_PIPE_BUFFER}); sys.stdout.flush(); sys.stdin.buffer.read()"
            )
        }

        #[test]
        fn input_larger_than_pipe_with_output_first_does_not_deadlock() -> Result<()> {
            require_python!(
                python,
                "input_larger_than_pipe_with_output_first_does_not_deadlock"
            );
            let mut stdout_file = NamedTempFile::new()?;
            let stderr_file = NamedTempFile::new()?;
            let stdout_fd = get_file_descriptor(stdout_file.as_file());
            let stderr_fd = get_file_descriptor(stderr_file.as_file());

            let config = CommandConfig {
                args: vec![python, "-c".to_string(), output_first_script()],
                input: Some(vec![b'y'; PAST_PIPE_BUFFER]),
                stdout_fd: wrap_fd(stdout_fd),
                stderr_fd: wrap_fd(stderr_fd),
                timeout_secs: Some(30.0),
                ..Default::default()
            };

            let result = bounded_run_command(config, Duration::from_secs(5))?;
            assert_eq!(result.map_err(|e| anyhow!("{e}"))?, 0);

            stdout_file.seek(SeekFrom::Start(0))?;
            let mut captured = Vec::new();
            stdout_file.read_to_end(&mut captured)?;
            assert_eq!(captured.len(), PAST_PIPE_BUFFER);

            Ok(())
        }

        #[test]
        fn child_closing_stdin_early_is_not_an_error() -> Result<()> {
            require_python!(python, "child_closing_stdin_early_is_not_an_error");
            let config = CommandConfig {
                args: vec![
                    python,
                    "-c".to_string(),
                    "import sys; sys.stdin.buffer.read(1)".to_string(),
                ],
                input: Some(vec![b'y'; PAST_PIPE_BUFFER]),
                ..Default::default()
            };

            let result = bounded_run_command(config, Duration::from_secs(5))?;
            assert_eq!(result.map_err(|e| anyhow!("{e}"))?, 0);

            Ok(())
        }

        #[test]
        fn timeout_fires_while_child_ignores_stdin() -> Result<()> {
            require_python!(python, "timeout_fires_while_child_ignores_stdin");
            let config = CommandConfig {
                args: vec![
                    python,
                    "-c".to_string(),
                    "import time; time.sleep(10)".to_string(),
                ],
                input: Some(vec![b'y'; PAST_PIPE_BUFFER]),
                timeout_secs: Some(0.5),
                ..Default::default()
            };

            let result = bounded_run_command(config, Duration::from_secs(5))?;
            let err = result.expect_err("the stdin write must not outlast timeout_secs");
            assert!(
                err.to_string().contains("timed out after"),
                "expected a timeout error, got: {err}"
            );

            Ok(())
        }

        // Generous bounds: these run under a loaded workspace test run.

        const TREE_BOUND: Duration = Duration::from_secs(30);
        const DEATH_WAIT: Duration = Duration::from_secs(10);

        #[cfg(unix)]
        fn process_alive(pid: u32) -> bool {
            // Killed grandchildren are reparented, so init reaps them promptly.
            unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
        }

        #[cfg(windows)]
        fn process_alive(pid: u32) -> bool {
            use winapi::um::handleapi::CloseHandle;
            use winapi::um::minwinbase::STILL_ACTIVE;
            use winapi::um::processthreadsapi::{GetExitCodeProcess, OpenProcess};
            use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;
            unsafe {
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if handle.is_null() {
                    return false;
                }
                let mut code = 0;
                let ok = GetExitCodeProcess(handle, &mut code);
                CloseHandle(handle);
                ok != 0 && code == STILL_ACTIVE
            }
        }

        fn wait_until_dead(pid: u32, within: Duration) -> bool {
            let deadline = Instant::now() + within;
            loop {
                thread::sleep(Duration::from_millis(20));
                if !process_alive(pid) || Instant::now() > deadline {
                    return !process_alive(pid);
                }
            }
        }

        /// First line of a capture file, parsed as a pid.
        fn read_pid(file: &mut (impl Read + Seek)) -> Option<u32> {
            file.seek(SeekFrom::Start(0)).ok()?;
            let mut text = String::new();
            file.read_to_string(&mut text).ok()?;
            text.lines().next()?.trim().parse().ok()
        }

        /// The pid the child prints to `file`, read while `run` is still going. Polls a
        /// reopened handle: seeking `file` itself would move the child's write offset.
        fn pid_while_running<T>(file: &NamedTempFile, run: &thread::JoinHandle<T>) -> Option<u32> {
            let mut reader = file.reopen().ok()?;
            loop {
                let finished = run.is_finished();
                let pid = read_pid(&mut reader);
                if pid.is_some() || finished {
                    return pid;
                }
                thread::sleep(Duration::from_millis(20));
            }
        }

        /// Kills a test's leftover processes when the test ends, pass or fail.
        struct KillOnDrop(Vec<u32>);

        impl Drop for KillOnDrop {
            fn drop(&mut self) {
                self.0.iter().for_each(|&pid| terminate(pid));
            }
        }

        /// Kills whatever matches `pattern` when the test ends, for processes whose
        /// pid the test never learns.
        #[cfg(unix)]
        struct KillMatching(&'static str);

        #[cfg(unix)]
        impl Drop for KillMatching {
            fn drop(&mut self) {
                pids_matching(self.0).into_iter().for_each(terminate);
            }
        }

        /// Pids whose full command line matches `pattern`.
        #[cfg(unix)]
        fn pids_matching(pattern: &str) -> Vec<u32> {
            Command::new("pgrep")
                .args(["-f", pattern])
                .output()
                .map(|out| {
                    String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .filter_map(|l| l.parse().ok())
                        .collect()
                })
                .unwrap_or_default()
        }

        #[cfg(unix)]
        fn wait_for_pids(pattern: &str, within: Duration) -> Vec<u32> {
            let deadline = Instant::now() + within;
            loop {
                let pids = pids_matching(pattern);
                if !pids.is_empty() || Instant::now() > deadline {
                    return pids;
                }
                thread::sleep(Duration::from_millis(20));
            }
        }

        /// Child that starts a long-lived Python grandchild, prints its pid and
        /// then sleeps, or exits at once when `child_exits` is set.
        /// The grandchild gets the pipes explicitly: Windows only inherits redirected handles.
        fn grandchild_script(grandchild_secs: u32, child_exits: bool) -> String {
            let tail = if child_exits { "" } else { "; time.sleep(60)" };
            format!(
                "import subprocess, sys, time; \
                 p = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep({grandchild_secs})'], \
                 stdout=sys.stdout, stderr=sys.stderr); \
                 print(p.pid, flush=True){tail}"
            )
        }

        /// Captures stdout so the test can read what the command prints.
        fn capturing_config(args: Vec<String>) -> (CommandConfig, NamedTempFile, NamedTempFile) {
            let stdout_file = NamedTempFile::new().expect("stdout temp file");
            let stderr_file = NamedTempFile::new().expect("stderr temp file");
            let config = CommandConfig {
                args,
                stdout_fd: wrap_fd(get_file_descriptor(stdout_file.as_file())),
                stderr_fd: wrap_fd(get_file_descriptor(stderr_file.as_file())),
                ..Default::default()
            };
            (config, stdout_file, stderr_file)
        }

        #[cfg(unix)]
        fn sh(script: &str) -> Vec<String> {
            vec!["sh".into(), "-c".into(), script.into()]
        }

        fn assert_timeout_error(
            result: std::result::Result<i32, Box<dyn std::error::Error + Send + Sync>>,
            expected: &str,
        ) {
            let err = result.expect_err("expected a timeout error");
            assert!(
                err.to_string().contains(expected),
                "expected {expected:?}, got: {err}"
            );
        }

        #[cfg(unix)]
        #[test]
        fn closed_capture_fd_is_an_error() -> Result<()> {
            // Past any fd limit, so no other test can have it open.
            let config = CommandConfig {
                args: vec!["true".into()],
                stdout_fd: Some(i32::MAX),
                ..Default::default()
            };
            let err = bounded_run_command(config, TREE_BOUND)?.expect_err("the fd is closed");
            assert!(err.to_string().contains("Failed to duplicate"), "{err}");
            Ok(())
        }

        /// `; true` stops `sh` exec-ing `sleep`, so `sleep` is a grandchild, and it
        /// must not outlive the timeout.
        #[cfg(unix)]
        #[test]
        fn timeout_kills_the_shells_child() -> Result<()> {
            let _cleanup = KillMatching("^sleep 999$");
            let config = CommandConfig {
                args: sh("sleep 999; true"),
                timeout_secs: Some(3.0),
                ..Default::default()
            };
            let run = thread::spawn(move || bounded_run_command(config, TREE_BOUND));
            let sleeper = wait_for_pids("^sleep 999$", DEATH_WAIT);
            assert_timeout_error(run.join().expect("run thread")?, "timed out after");
            assert!(!sleeper.is_empty(), "never saw `sleep 999` start");
            for pid in sleeper {
                assert!(
                    wait_until_dead(pid, DEATH_WAIT),
                    "sleep {pid} survived the timeout"
                );
            }
            Ok(())
        }

        /// `no_output_timeout_secs` counts from the pid print, so a slow
        /// interpreter start can't fire it before the grandchild exists.
        #[test]
        fn timeout_kills_grandchild() -> Result<()> {
            require_python!(python, "timeout_kills_grandchild");
            let (mut config, stdout_file, _stderr) =
                capturing_config(vec![python, "-c".into(), grandchild_script(60, false)]);
            config.no_output_timeout_secs = Some(5.0);

            let run = thread::spawn(move || bounded_run_command(config, TREE_BOUND));
            let pid = pid_while_running(&stdout_file, &run);
            let _cleanup = KillOnDrop(pid.into_iter().collect());
            let seen_alive = pid.is_some_and(process_alive);
            assert_timeout_error(run.join().expect("run thread")?, "no output for");
            let pid = pid.ok_or_else(|| anyhow!("the child never printed the grandchild pid"))?;
            assert!(seen_alive, "grandchild {pid} was never seen alive");
            assert!(
                wait_until_dead(pid, DEATH_WAIT),
                "grandchild {pid} survived the timeout"
            );
            Ok(())
        }

        /// The child exits 0 but its grandchild keeps stdout open: the timeout
        /// still holds, and the call reports it rather than success.
        #[test]
        fn timeout_holds_when_background_child_keeps_pipe_open() -> Result<()> {
            require_python!(
                python,
                "timeout_holds_when_background_child_keeps_pipe_open"
            );
            let (mut config, mut stdout_file, _stderr) =
                capturing_config(vec![python, "-c".into(), grandchild_script(60, true)]);
            config.timeout_secs = Some(3.0);

            let start = Instant::now();
            let result = bounded_run_command(config, TREE_BOUND);
            let elapsed = start.elapsed();
            let _cleanup = KillOnDrop(read_pid(&mut stdout_file).into_iter().collect());
            assert_timeout_error(result?, "timed out after");
            assert!(
                elapsed < Duration::from_secs(20),
                "the call waited {elapsed:?} for the background holder"
            );
            Ok(())
        }

        /// `sh` exits 0 at once and the backgrounded `sleep` holds the pipes until
        /// one of the timeouts runs out. Each caller uses its own `sleep` duration so
        /// the cleanup finds only its own leftover.
        #[cfg(unix)]
        fn background_holder_times_out(
            sleeper: &'static str,
            timeouts: (Option<f64>, Option<f64>),
            expected: &str,
        ) -> Result<()> {
            let _cleanup = KillMatching(sleeper);
            let config = CommandConfig {
                args: sh(&format!("{} &", sleeper.trim_matches(['^', '$']))),
                timeout_secs: timeouts.0,
                no_output_timeout_secs: timeouts.1,
                ..Default::default()
            };
            let start = Instant::now();
            let result = bounded_run_command(config, TREE_BOUND)?;
            let elapsed = start.elapsed();
            assert_timeout_error(result, expected);
            assert!(
                elapsed < Duration::from_secs(20),
                "the call waited {elapsed:?} for the background holder"
            );
            Ok(())
        }

        #[cfg(unix)]
        #[test]
        fn timeout_holds_when_a_background_sleep_keeps_pipe_open() -> Result<()> {
            background_holder_times_out("^sleep 301$", (Some(1.0), None), "timed out after")
        }

        #[cfg(unix)]
        #[test]
        fn no_output_timeout_holds_when_a_background_sleep_keeps_pipe_open() -> Result<()> {
            background_holder_times_out("^sleep 302$", (None, Some(1.0)), "no output for")
        }

        /// A writer that left the tree before the timeout escapes the walk. The call
        /// returns once the grace runs out, and the detached reader then lets go of the
        /// pipe, so the writer gets EPIPE instead of feeding a closed capture file forever.
        #[cfg(unix)]
        #[test]
        fn timeout_returns_when_an_escaped_process_holds_the_pipe() -> Result<()> {
            // The subshell exits at once, so the writer is reparented before the timeout.
            let (mut config, mut stdout_file, _stderr) = capturing_config(sh(
                "(sh -c 'echo $$; while echo tick; do sleep 0.1; done' &); sleep 60",
            ));
            config.timeout_secs = Some(2.0);

            let start = Instant::now();
            let result = bounded_run_command(config, TREE_BOUND);
            let elapsed = start.elapsed();
            let writer = read_pid(&mut stdout_file);
            let _cleanup = KillOnDrop(writer.into_iter().collect());
            assert_timeout_error(result?, "timed out after");
            assert!(
                elapsed < Duration::from_secs(20),
                "the call waited {elapsed:?} for the escaped process"
            );
            // The drain waited out the grace, then detached.
            let floor = Duration::from_secs(2) + crate::command::KILL_GRACE;
            assert!(elapsed >= floor, "returned after {elapsed:?}");
            let writer = writer.ok_or_else(|| anyhow!("the writer never printed its pid"))?;
            assert!(
                wait_until_dead(writer, DEATH_WAIT),
                "writer {writer} still running: the detached reader kept the pipe open"
            );
            let size = stdout_file.as_file().metadata()?.len();
            thread::sleep(Duration::from_millis(500));
            assert_eq!(
                stdout_file.as_file().metadata()?.len(),
                size,
                "capture still growing"
            );
            Ok(())
        }

        /// A pipe whose buffer is already full, so the next write to it blocks until
        /// the returned drainer reads `expected` bytes after `delay`.
        #[cfg(unix)]
        fn full_pipe(
            delay: Duration,
            expected: usize,
        ) -> Result<(i32, thread::JoinHandle<Vec<u8>>)> {
            let (read_fd, write_fd) = create_pipe()?;
            let mut filler = 0;
            unsafe {
                let flags = libc::fcntl(write_fd, libc::F_GETFL);
                libc::fcntl(write_fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
                while libc::write(write_fd, [b'f'; 1024].as_ptr().cast(), 1024) > 0 {
                    filler += 1024;
                }
                libc::fcntl(write_fd, libc::F_SETFL, flags);
            }
            let drainer = thread::spawn(move || {
                thread::sleep(delay);
                let mut reader =
                    unsafe { <File as std::os::unix::io::FromRawFd>::from_raw_fd(read_fd) };
                let mut data = vec![0; filler + expected];
                let _ = reader.read_exact(&mut data);
                data.split_off(filler)
            });
            Ok((write_fd, drainer))
        }

        /// The child finishes in time, but the sink it streams to is slow: that is not a
        /// timeout, and every byte must arrive.
        #[cfg(unix)]
        fn slow_sink_is_not_a_timeout(timeouts: (Option<f64>, Option<f64>)) -> Result<()> {
            const PAYLOAD: usize = 8000;
            let (sink, drainer) = full_pipe(Duration::from_secs(3), PAYLOAD)?;
            let config = CommandConfig {
                args: sh(&format!("printf '%0{PAYLOAD}d' 0")),
                sys_stdout_fd: Some(sink),
                timeout_secs: timeouts.0,
                no_output_timeout_secs: timeouts.1,
                ..Default::default()
            };
            let result = bounded_run_command(config, TREE_BOUND)?;
            unsafe { libc::close(sink) };
            assert_eq!(result.map_err(|e| anyhow!("{e}"))?, 0);
            let delivered = drainer.join().expect("drainer");
            assert_eq!(delivered, vec![b'0'; PAYLOAD]);
            Ok(())
        }

        #[cfg(unix)]
        #[test]
        fn slow_sink_is_not_a_timeout_secs_timeout() -> Result<()> {
            slow_sink_is_not_a_timeout((Some(1.0), None))
        }

        #[cfg(unix)]
        #[test]
        fn slow_sink_is_not_a_no_output_timeout() -> Result<()> {
            slow_sink_is_not_a_timeout((None, Some(1.0)))
        }

        /// A background writer that finishes well inside the limits (or with none set)
        /// is waited for, and its output delivered.
        #[cfg(unix)]
        fn background_writer_is_waited_for(timeouts: (Option<f64>, Option<f64>)) -> Result<()> {
            let (mut config, mut stdout_file, _stderr) =
                capturing_config(sh("(sleep 1; echo late) &"));
            config.timeout_secs = timeouts.0;
            config.no_output_timeout_secs = timeouts.1;
            let start = Instant::now();
            let result = bounded_run_command(config, TREE_BOUND)?;
            assert_eq!(result.map_err(|e| anyhow!("{e}"))?, 0);
            assert!(start.elapsed() >= Duration::from_secs(1));
            stdout_file.seek(SeekFrom::Start(0))?;
            let mut captured = String::new();
            stdout_file.read_to_string(&mut captured)?;
            assert_eq!(captured.trim(), "late");
            Ok(())
        }

        #[cfg(unix)]
        #[test]
        fn background_writer_is_waited_for_without_timeouts() -> Result<()> {
            background_writer_is_waited_for((None, None))
        }

        #[cfg(unix)]
        #[test]
        fn background_writer_is_waited_for_within_timeouts() -> Result<()> {
            background_writer_is_waited_for((Some(60.0), Some(60.0)))
        }

        /// The write end of a pipe whose read end is already closed, like toolr's stdout after
        /// `| head` exits.
        #[cfg(unix)]
        fn broken_pipe() -> Result<i32> {
            let (read_fd, write_fd) = create_pipe()?;
            unsafe { libc::close(read_fd) };
            Ok(write_fd)
        }

        fn read_all(file: &mut NamedTempFile) -> Result<Vec<u8>> {
            let mut data = Vec::new();
            file.seek(SeekFrom::Start(0))?;
            file.read_to_end(&mut data)?;
            Ok(data)
        }

        #[cfg(unix)]
        #[test]
        fn capture_kept_when_stream_pipe_breaks() -> Result<()> {
            require_python!(python, "capture_kept_when_stream_pipe_breaks");
            let script = "import sys; sys.stdout.write('x'*300000); sys.stdout.flush()";
            let (mut config, mut stdout_file, _stderr) =
                capturing_config(vec![python, "-c".into(), script.into()]);
            let (sys_stdout, sys_stderr) = (broken_pipe()?, broken_pipe()?);
            config.sys_stdout_fd = Some(sys_stdout);
            config.sys_stderr_fd = Some(sys_stderr);
            let result = bounded_run_command(config, TREE_BOUND)?;
            unsafe {
                libc::close(sys_stdout);
                libc::close(sys_stderr);
            }
            assert_eq!(result.map_err(|e| anyhow!("{e}"))?, 0);
            assert_eq!(read_all(&mut stdout_file)?.len(), 300000);
            Ok(())
        }

        /// With nothing left to write to, the reader lets go of the pipe so the child dies
        /// as it would in a shell pipeline, instead of running for ever.
        #[cfg(unix)]
        #[test]
        fn stream_only_broken_pipe_ends_child_like_a_pipeline() -> Result<()> {
            require_python!(python, "stream_only_broken_pipe_ends_child_like_a_pipeline");
            let (sys_stdout, sys_stderr) = (broken_pipe()?, broken_pipe()?);
            let config = CommandConfig {
                args: vec![
                    python,
                    "-c".into(),
                    "import sys\nwhile True: sys.stdout.write('y'*8192)".into(),
                ],
                sys_stdout_fd: Some(sys_stdout),
                sys_stderr_fd: Some(sys_stderr),
                timeout_secs: Some(8.0),
                ..Default::default()
            };
            let start = Instant::now();
            let result = bounded_run_command(config, TREE_BOUND)?;
            unsafe {
                libc::close(sys_stdout);
                libc::close(sys_stderr);
            }
            let elapsed = start.elapsed();
            assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
            assert_ne!(result.map_err(|e| anyhow!("{e}"))?, 0);
            Ok(())
        }

        /// A failed capture write is reported, and the child still runs to completion.
        #[cfg(unix)]
        #[test]
        fn capture_write_failure_is_reported() -> Result<()> {
            require_python!(python, "capture_write_failure_is_reported");
            let dir = tempfile::tempdir()?;
            let marker = dir.path().join("marker");
            let script = format!(
                // More than a pipe buffer after the failure: a dropped pipe would kill the child
                // before the marker.
                "import sys; print('hello', flush=True); sys.stdout.write('x' * 300000); \
                 sys.stdout.flush(); open({:?}, 'w').close()",
                marker.to_str().expect("utf-8 temp path")
            );
            let stdout_file = NamedTempFile::new()?;
            let read_only = File::open(stdout_file.path())?;
            let stderr_file = NamedTempFile::new()?;
            let config = CommandConfig {
                args: vec![python, "-c".into(), script],
                stdout_fd: Some(get_file_descriptor(&read_only)),
                stderr_fd: Some(get_file_descriptor(stderr_file.as_file())),
                ..Default::default()
            };
            let err = bounded_run_command(config, TREE_BOUND)?
                .expect_err("a capture write to a read-only file must fail");
            assert!(
                err.to_string().contains("Failed to write captured stdout"),
                "{err}"
            );
            assert!(marker.exists(), "the child was cut off before finishing");
            Ok(())
        }

        #[cfg(unix)]
        #[test]
        fn stream_write_failure_other_than_broken_pipe_is_reported() -> Result<()> {
            require_python!(
                python,
                "stream_write_failure_other_than_broken_pipe_is_reported"
            );
            let stream_file = NamedTempFile::new()?;
            let read_only = File::open(stream_file.path())?;
            let config = CommandConfig {
                args: vec![python, "-c".into(), "print('hello')".into()],
                sys_stdout_fd: Some(get_file_descriptor(&read_only)),
                ..Default::default()
            };
            let err = bounded_run_command(config, TREE_BOUND)?
                .expect_err("a stream write to a read-only file must fail");
            assert!(err.to_string().contains("Failed to write stdout"), "{err}");
            Ok(())
        }
    }

    // --------------------------------------------------------------------
    // Public-surface coverage for CommandConfig builder methods + error
    // constructors / Display impls. These never get exercised by the
    // existing subprocess-run tests, leaving a sizeable chunk of
    // command.rs uncovered for what is essentially trivial code.
    // --------------------------------------------------------------------

    use crate::command::{
        CommandExecutionError, CommandNoOutputTimeoutError, CommandTimeoutExceededError,
    };

    #[test]
    fn config_new_seeds_args_and_inherits_defaults() {
        let cfg = CommandConfig::new(vec!["echo".into(), "hi".into()]);
        assert_eq!(cfg.args, vec!["echo".to_string(), "hi".to_string()]);
        assert!(cfg.env.is_empty());
        assert!(cfg.input.is_none());
        assert!(cfg.timeout_secs.is_none());
        assert!(cfg.no_output_timeout_secs.is_none());
    }

    #[test]
    fn config_with_env_replaces_environment_map() {
        let mut env = HashMap::new();
        env.insert("FOO".into(), "bar".into());
        let cfg = CommandConfig::new(vec!["echo".into()]).with_env(env);
        assert_eq!(cfg.env.get("FOO"), Some(&"bar".to_string()));
    }

    #[test]
    fn config_with_input_records_stdin_payload() {
        let cfg = CommandConfig::new(vec!["cat".into()]).with_input(b"hello".to_vec());
        assert_eq!(cfg.input.as_deref(), Some(b"hello".as_ref()));
    }

    #[test]
    fn config_with_cwd_overrides_default_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = CommandConfig::new(vec!["pwd".into()]).with_cwd(tmp.path().to_path_buf());
        assert_eq!(cfg.cwd.as_deref(), Some(tmp.path()));
    }

    #[test]
    fn command_execution_error_display_propagates_message() {
        let err = CommandExecutionError::new("boom");
        assert_eq!(err.to_string(), "boom");
        // Round-trip through `dyn Error` so the `Error` impl is exercised.
        let boxed: Box<dyn std::error::Error> = Box::new(err);
        assert_eq!(boxed.to_string(), "boom");
    }

    #[test]
    fn command_timeout_error_display_propagates_message() {
        let err = CommandTimeoutExceededError::new("timed out after 1s");
        assert_eq!(err.to_string(), "timed out after 1s");
        let boxed: Box<dyn std::error::Error> = Box::new(err);
        assert!(boxed.to_string().contains("timed out"));
    }

    #[test]
    fn command_no_output_timeout_error_display_propagates_message() {
        let err = CommandNoOutputTimeoutError::new("no output for 0.5s");
        assert_eq!(err.to_string(), "no output for 0.5s");
        let boxed: Box<dyn std::error::Error> = Box::new(err);
        assert!(boxed.to_string().contains("no output"));
    }

    #[test]
    fn run_command_internal_reports_spawn_failure_via_command_execution_error() {
        // Spawning a nonexistent binary returns Err via the
        // `CommandExecutionError::new("Failed to execute command: …")`
        // arm — pin that the message has the documented prefix.
        let cfg = CommandConfig::new(vec!["definitely-not-on-path-toolr-test".into()]);
        let err = run_command_internal(cfg).expect_err("spawn should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("Failed to execute command"),
            "expected spawn-failure prefix, got: {msg}",
        );
    }

    #[test]
    fn stdin_write_errors_other_than_broken_pipe_are_reported() -> Result<()> {
        use crate::command::{join_stdin_writer, spawn_stdin_writer};

        // A read-only handle rejects writes with an error that is not `BrokenPipe`.
        let file = NamedTempFile::new()?;
        let read_only = File::open(file.path())?;
        let err = join_stdin_writer(spawn_stdin_writer(read_only, b"data".to_vec()))
            .expect_err("a write to a read-only handle must fail");
        assert!(
            err.to_string().starts_with("Failed to write to stdin: "),
            "unexpected message: {err}"
        );
        Ok(())
    }

    #[test]
    fn interactive_mode_defaults_to_false() {
        let cfg = CommandConfig::new(vec!["echo".into()]);
        assert!(!cfg.interactive);
    }

    #[test]
    fn interactive_mode_ignores_capture_fds() -> Result<()> {
        // #485: when `interactive` is set, the child's stdio is inherited
        // directly from the parent — capture fds must be left untouched
        // rather than fed through the pipe-and-copy path.
        require_python!(python, "interactive_mode_ignores_capture_fds");

        let mut stdout_file = NamedTempFile::new()?;
        let stderr_file = NamedTempFile::new()?;
        let stdout_fd = get_file_descriptor(stdout_file.as_file());
        let stderr_fd = get_file_descriptor(stderr_file.as_file());

        let config = CommandConfig {
            args: vec![python, "-c".to_string(), "print('hello')".to_string()],
            interactive: true,
            stdout_fd: wrap_fd(stdout_fd),
            stderr_fd: wrap_fd(stderr_fd),
            ..Default::default()
        };

        let result = run_command_internal(config);
        assert!(result.is_ok(), "Command failed: {result:?}");
        assert_eq!(result.unwrap(), 0, "Command should return exit code 0");

        stdout_file.seek(SeekFrom::Start(0))?;
        let mut stdout_content = String::new();
        stdout_file.read_to_string(&mut stdout_content)?;
        assert!(
            stdout_content.is_empty(),
            "interactive mode must not write to the capture fd, got: {stdout_content:?}"
        );

        Ok(())
    }
}
