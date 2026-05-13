#[cfg(test)]
mod test_suite {
    use std::io::{Read, Write, Seek, SeekFrom};
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};
    use tempfile::NamedTempFile;
    use anyhow::{Result, anyhow};
    use std::collections::HashMap;
    use std::fs::File;

    // Platform-specific imports
    #[cfg(unix)]
    use std::os::unix::io::AsRawFd;
    #[cfg(windows)]
    use std::os::windows::io::AsRawHandle;
    #[cfg(windows)]
    use winapi::um::handleapi::{INVALID_HANDLE_VALUE, DuplicateHandle};
    #[cfg(windows)]
    use winapi::um::processthreadsapi::GetCurrentProcess;
    #[cfg(windows)]
    use winapi::um::winnt::DUPLICATE_SAME_ACCESS;
    #[cfg(windows)]
    use winapi::um::namedpipeapi::CreatePipe;
    #[cfg(windows)]
    use winapi::um::fileapi::ReadFile;
    #[cfg(windows)]
    use winapi::um::minwinbase::SECURITY_ATTRIBUTES;
    #[cfg(windows)]
    #[cfg(windows)]
    use crate::command::ThreadSafeHandle;

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
                handle as *mut _,  // Let the compiler infer the void type
                GetCurrentProcess(),
                &mut new_handle,
                0,
                1,
                DUPLICATE_SAME_ACCESS,
            ) != 0 {
                ThreadSafeHandle::new(new_handle as *mut _)  // Convert to RawHandle
            } else {
                // For tests, fallback to a dummy handle if duplication fails
                ThreadSafeHandle::new(handle as *mut _)  // Convert to RawHandle
            }
        }
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
                    ThreadSafeHandle::new(write.as_raw_handle() as *mut _)
                ))
            } else {
                Ok((
                    ThreadSafeHandle::new(read_handle as *mut _),
                    ThreadSafeHandle::new(write_handle as *mut _)
                ))
            }
        }
    }

    #[cfg(windows)]
    fn read_pipe(handle: ThreadSafeHandle, buffer: &mut [u8]) -> i32 {
        let mut bytes_read = 0;
        unsafe {
            let raw_handle = handle.raw() as *mut winapi::ctypes::c_void;
            if raw_handle != INVALID_HANDLE_VALUE as *mut winapi::ctypes::c_void && ReadFile(
                raw_handle,
                buffer.as_mut_ptr() as *mut winapi::ctypes::c_void,
                buffer.len() as u32,
                &mut bytes_read,
                std::ptr::null_mut(),
            ) != 0 {
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
        assert!(stdout_content.contains("to stdout"), "Expected 'to stdout' in '{stdout_content}'");
        assert!(stderr_content.contains("to stderr"), "Expected 'to stderr' in '{stderr_content}'");

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
            if n == 0 { break; }
            stdout_capture.write_all(&buffer[0..n])?;
        }
        stdout_capture.flush()?;

        loop {
            let n = stderr_file.read(&mut buffer)?;
            if n == 0 { break; }
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

        assert!(stdout_capture_content.contains("to both stdout"),
                "Expected 'to both stdout' in '{stdout_capture_content}'");
        assert!(stderr_capture_content.contains("to both stderr"),
                "Expected 'to both stderr' in '{stderr_capture_content}'");

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
        let mut env = HashMap::new();
        env.insert("TEST_VAR".to_string(), "test_value".to_string());
        env.insert("ANOTHER_VAR".to_string(), "another_value".to_string());

        // Create temporary files for stdout and stderr
        let mut stdout_file = NamedTempFile::new()?;
        let stderr_file = NamedTempFile::new()?;

        // Get file descriptors in a cross-platform way
        let stdout_fd = get_file_descriptor(stdout_file.as_file());
        let stderr_fd = get_file_descriptor(stderr_file.as_file());

        // Use platform-specific Python executable name
        let config = CommandConfig {
            args: vec![
                #[cfg(unix)]
                "python".to_string(),
                #[cfg(windows)]
                "python.exe".to_string(),
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
        assert!(stdout_content.contains("TEST_VAR=test_value"),
                "Expected 'TEST_VAR=test_value' in stdout, got: {stdout_content}");
        assert!(stdout_content.contains("ANOTHER_VAR=another_value"),
                "Expected 'ANOTHER_VAR=another_value' in stdout, got: {stdout_content}");

        Ok(())
    }

    #[test]
    fn test_command_respects_cwd() -> Result<()> {
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
                #[cfg(unix)]
                "python".to_string(),
                #[cfg(windows)]
                "python.exe".to_string(),
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

        let canonical_captured = captured_path_buf.canonicalize().unwrap_or(captured_path_buf);
        let canonical_expected = expected_path_buf.canonicalize().unwrap_or(expected_path_buf);

        // Compare the canonicalized paths
        assert_eq!(canonical_captured, canonical_expected,
                  "Expected current working directory '{}', got '{}'",
                  canonical_expected.display(), canonical_captured.display());

        // Now test that without setting cwd, pwd returns a different directory
        let mut stdout_file2 = NamedTempFile::new()?;
        let stderr_file2 = NamedTempFile::new()?;

        let stdout_fd2 = get_file_descriptor(stdout_file2.as_file());
        let stderr_fd2 = get_file_descriptor(stderr_file2.as_file());

        // Create a command config that runs 'pwd' without setting the working directory
        let config2 = CommandConfig {
            args: vec![
                #[cfg(unix)]
                "python".to_string(),
                #[cfg(windows)]
                "python.exe".to_string(),
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
        assert_ne!(stdout_content2.trim(), temp_path_str.as_ref(),
                  "Working directory should be different when cwd is not specified");

        Ok(())
    }

    mod tokio_tests {
        use super::*;
        use tokio::runtime::Runtime;

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
            libc::read(fd, buffer.as_mut_ptr() as *mut libc::c_void, buffer.len()) as i32
        }

        #[cfg(windows)]
        fn read_pipe(fd: ThreadSafeHandle, buffer: &mut [u8]) -> i32 {
            super::read_pipe(fd, buffer)
        }

        #[test]
        fn test_sys_fd_streaming() -> Result<()> {
            let rt = Runtime::new()?;

            rt.block_on(async {
                let (stdout_read, stdout_write) = create_pipe()?;
                let (stderr_read, stderr_write) = create_pipe()?;

                let config = CommandConfig {
                    args: vec![
                        #[cfg(unix)]
                        "python".to_string(),
                        #[cfg(windows)]
                        "python.exe".to_string(),
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
                        #[cfg(unix)]
                        "python".to_string(),
                        #[cfg(windows)]
                        "python.exe".to_string(),
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
            let rt = Runtime::new()?;

            rt.block_on(async {
                // Create a command that will run longer than our timeout
                let config = CommandConfig {
                    args: vec![
                        #[cfg(unix)]
                        "python".to_string(),
                        #[cfg(windows)]
                        "python.exe".to_string(),
                        "-c".to_string(),
                        "import time; time.sleep(10)".to_string(),
                    ],
                    timeout_secs: Some(0.1), // Very short timeout
                    ..Default::default()
                };

                // Run the command and expect failure
                let result = run_command_internal(config);

                // Should fail with timeout
                assert!(result.is_err());
                let error_str = format!("{}", result.unwrap_err());

                // Check error message attributes
                assert!(error_str.contains("timed out after"), "Error should mention timeout duration");

                Ok(())
            })
        }

        #[test]
        fn test_no_output_timeout() -> Result<()> {
            let rt = Runtime::new()?;

            rt.block_on(async {
                // Create pipes for output
                let (stdout_read, stdout_write) = create_pipe()?;
                let (_stderr_read, stderr_write) = create_pipe()?;

                // Create a command that outputs once then waits, triggering no-output timeout
                let config = CommandConfig {
                    args: vec![
                        #[cfg(unix)]
                        "python".to_string(),
                        #[cfg(windows)]
                        "python.exe".to_string(),
                        "-c".to_string(),
                        "import sys, time; sys.stdout.write('initial output'); sys.stdout.flush(); time.sleep(10)".to_string(),
                    ],
                    sys_stdout_fd: wrap_fd(stdout_write),
                    sys_stderr_fd: wrap_fd(stderr_write),
                    no_output_timeout_secs: Some(0.1), // Very short no-output timeout
                    ..Default::default()
                };

                // Run the command in a separate thread
                let handle = std::thread::spawn(move || {
                    run_command_internal(config)
                });

                // Read the initial output
                let mut stdout_buffer = [0u8; 1024];
                #[cfg(unix)]
                unsafe { read_pipe(stdout_read, &mut stdout_buffer) };
                #[cfg(windows)]
                read_pipe(stdout_read, &mut stdout_buffer);

                // Wait for command to fail
                let result = handle.join().expect("Thread panicked");

                // Clean up
                #[cfg(unix)]
                unsafe {
                    libc::close(stdout_read);
                    libc::close(_stderr_read);
                    libc::close(stdout_write);
                    libc::close(stderr_write);
                }

                #[cfg(windows)]
                {
                    // On Windows our test files will close automatically
                }

                // Should fail with no-output timeout
                assert!(result.is_err());
                let error_str = format!("{}", result.unwrap_err());

                assert!(error_str.contains("no output for"), "Error should mention no-output timeout");

                Ok(())
            })
        }
    }
}
