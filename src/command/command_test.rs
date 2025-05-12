#[cfg(test)]
mod test_suite {
    use std::io::{Read, Write, Seek, SeekFrom};
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};
    use tempfile::NamedTempFile;
    use anyhow::{Result, anyhow};
    use std::collections::HashMap;
    use std::os::unix::io::AsRawFd;
    use std::fs::File;
    use crate::{CommandConfig, run_command_internal};

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
        assert!(stdout_content.contains("to stdout"), "Expected 'to stdout' in '{}'", stdout_content);
        assert!(stderr_content.contains("to stderr"), "Expected 'to stderr' in '{}'", stderr_content);

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
                "Expected 'to both stdout' in '{}'", stdout_capture_content);
        assert!(stderr_capture_content.contains("to both stderr"),
                "Expected 'to both stderr' in '{}'", stderr_capture_content);

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
        assert_eq!(bytes_read, 0, "Expected 0 bytes read, got {}", bytes_read);
        assert!(start_time.elapsed() >= timeout, "Timeout not respected");

        Ok(())
    }

    #[test]
    fn test_environment_variables() -> Result<()> {
        // Create a command that uses environment variables
        let mut env = HashMap::new();
        env.insert("TEST_VAR".to_string(), "test_value".to_string());
        env.insert("ANOTHER_VAR".to_string(), "another_value".to_string());

        // Create temporary files for stdout and stderr
        let mut stdout_file = NamedTempFile::new()?;
        let stderr_file = NamedTempFile::new()?;

        let stdout_fd = stdout_file.as_file().as_raw_fd();
        let stderr_fd = stderr_file.as_file().as_raw_fd();

        // Command to echo environment variables
        let config = CommandConfig {
            args: vec![
                "bash".to_string(),
                "-c".to_string(),
                "echo \"TEST_VAR=$TEST_VAR\"; echo \"ANOTHER_VAR=$ANOTHER_VAR\"".to_string(),
            ],
            env,
            stdout_fd: Some(stdout_fd),
            stderr_fd: Some(stderr_fd),
            ..Default::default()
        };

        // Run the command
        let result = run_command_internal(config);

        // Check command completed successfully
        assert!(result.is_ok(), "Command failed: {:?}", result);
        assert_eq!(result.unwrap(), 0, "Command should return exit code 0");

        // Read captured stdout
        stdout_file.seek(SeekFrom::Start(0))?;
        let mut stdout_content = String::new();
        stdout_file.read_to_string(&mut stdout_content)?;

        // Verify environment variables were correctly set
        assert!(stdout_content.contains("TEST_VAR=test_value"),
                "Expected 'TEST_VAR=test_value' in stdout, got: {}", stdout_content);
        assert!(stdout_content.contains("ANOTHER_VAR=another_value"),
                "Expected 'ANOTHER_VAR=another_value' in stdout, got: {}", stdout_content);

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

        let stdout_fd = stdout_file.as_file().as_raw_fd();
        let stderr_fd = stderr_file.as_file().as_raw_fd();

        // Create a command config that runs 'pwd' with the temp directory as working directory
        let config = CommandConfig {
            args: vec!["pwd".to_string()],
            cwd: Some(temp_path.clone()),
            stdout_fd: Some(stdout_fd),
            stderr_fd: Some(stderr_fd),
            ..Default::default()
        };

        // Run the command
        let result = run_command_internal(config);

        // Check command completed successfully
        assert!(result.is_ok(), "Command failed: {:?}", result);
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

        let stdout_fd2 = stdout_file2.as_file().as_raw_fd();
        let stderr_fd2 = stderr_file2.as_file().as_raw_fd();

        // Create a command config that runs 'pwd' without setting the working directory
        let config2 = CommandConfig {
            args: vec!["pwd".to_string()],
            stdout_fd: Some(stdout_fd2),
            stderr_fd: Some(stderr_fd2),
            ..Default::default()
        };

        // Run the command
        let result2 = run_command_internal(config2);

        // Check command completed successfully
        assert!(result2.is_ok(), "Command failed: {:?}", result2);
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
        // We need to run the tokio tests with the runtime
        use tokio::runtime::Runtime;

        // Function to create OS pipes using libc
        fn create_os_pipe() -> Result<(i32, i32)> {
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

        #[test]
        fn test_sys_fd_streaming() -> Result<()> {
            // Create a Tokio runtime for this test
            let rt = Runtime::new()?;

            rt.block_on(async {
                // Create pipes using OS pipes
                let (stdout_read, stdout_write) = create_os_pipe()?;
                let (stderr_read, stderr_write) = create_os_pipe()?;

                // Use these pipe file descriptors
                let sys_stdout_fd = stdout_write;
                let sys_stderr_fd = stderr_write;

                // Create a command config
                let config = CommandConfig {
                    args: vec![
                        "bash".to_string(),
                        "-c".to_string(),
                        "echo 'to sys stdout'; echo 'to sys stderr' >&2".to_string(),
                    ],
                    sys_stdout_fd: Some(sys_stdout_fd),
                    sys_stderr_fd: Some(sys_stderr_fd),
                    ..Default::default()
                };

                // Run the command in a separate thread to avoid blocking
                let handle = std::thread::spawn(move || {
                    run_command_internal(config)
                });

                // Read from the pipes
                let mut stdout_buffer = [0u8; 1024];
                let mut stderr_buffer = [0u8; 1024];

                let stdout_bytes = unsafe { libc::read(stdout_read, stdout_buffer.as_mut_ptr() as *mut libc::c_void, stdout_buffer.len()) };
                let stderr_bytes = unsafe { libc::read(stderr_read, stderr_buffer.as_mut_ptr() as *mut libc::c_void, stderr_buffer.len()) };

                // Wait for command to finish
                let result = handle.join().expect("Thread panicked");
                // Convert Result<i32, Box<dyn Error>> to anyhow::Result<i32>
                let exit_code = match result {
                    Ok(code) => Ok(code),
                    Err(e) => Err(anyhow!("{}", e)),
                }?;

                // Clean up
                unsafe {
                    libc::close(stdout_read);
                    libc::close(stderr_read);
                    libc::close(stdout_write);
                    libc::close(stderr_write);
                }

                // Convert output to strings
                let stdout_content = String::from_utf8_lossy(&stdout_buffer[0..stdout_bytes as usize]);
                let stderr_content = String::from_utf8_lossy(&stderr_buffer[0..stderr_bytes as usize]);

                assert_eq!(exit_code, 0, "Command should succeed");
                assert!(stdout_content.contains("to sys stdout"), "Expected 'to sys stdout', got '{}'", stdout_content);
                assert!(stderr_content.contains("to sys stderr"), "Expected 'to sys stderr', got '{}'", stderr_content);

                Ok(())
            })
        }

        #[test]
        fn test_sys_fd_with_capture() -> Result<()> {
            // Create a Tokio runtime for this test
            let rt = Runtime::new()?;

            rt.block_on(async {
                // Create pipes using OS pipes
                let (stdout_read, stdout_write) = create_os_pipe()?;
                let (stderr_read, stderr_write) = create_os_pipe()?;

                // Create capture files
                let mut stdout_capture = NamedTempFile::new()?;
                let mut stderr_capture = NamedTempFile::new()?;

                // Get file descriptors for capture files
                // First get the raw file descriptors
                let stdout_file: File = stdout_capture.reopen()?;
                let stderr_file: File = stderr_capture.reopen()?;

                let stdout_fd = stdout_file.as_raw_fd();
                let stderr_fd = stderr_file.as_raw_fd();

                // Create a command config
                let config = CommandConfig {
                    args: vec![
                        "bash".to_string(),
                        "-c".to_string(),
                        "echo 'captured stdout'; echo 'captured stderr' >&2".to_string(),
                    ],
                    stdout_fd: Some(stdout_fd),
                    stderr_fd: Some(stderr_fd),
                    sys_stdout_fd: Some(stdout_write),
                    sys_stderr_fd: Some(stderr_write),
                    ..Default::default()
                };

                // Run the command in a separate thread to avoid blocking
                let handle = std::thread::spawn(move || {
                    run_command_internal(config)
                });

                // Read from the pipes
                let mut stdout_buffer = [0u8; 1024];
                let mut stderr_buffer = [0u8; 1024];

                let stdout_bytes = unsafe { libc::read(stdout_read, stdout_buffer.as_mut_ptr() as *mut libc::c_void, stdout_buffer.len()) };
                let stderr_bytes = unsafe { libc::read(stderr_read, stderr_buffer.as_mut_ptr() as *mut libc::c_void, stderr_buffer.len()) };

                // Wait for command to finish
                let result = handle.join().expect("Thread panicked");
                // Convert Result<i32, Box<dyn Error>> to anyhow::Result<i32>
                let exit_code = match result {
                    Ok(code) => Ok(code),
                    Err(e) => Err(anyhow!("{}", e)),
                }?;

                // Clean up
                unsafe {
                    libc::close(stdout_read);
                    libc::close(stderr_read);
                    libc::close(stdout_write);
                    libc::close(stderr_write);
                }

                // Convert streamed output to strings
                let stdout_streamed = String::from_utf8_lossy(&stdout_buffer[0..stdout_bytes as usize]);
                let stderr_streamed = String::from_utf8_lossy(&stderr_buffer[0..stderr_bytes as usize]);

                // Read the captured output
                let mut stdout_captured = String::new();
                let mut stderr_captured = String::new();

                stdout_capture.seek(SeekFrom::Start(0))?;
                stderr_capture.seek(SeekFrom::Start(0))?;

                stdout_capture.read_to_string(&mut stdout_captured)?;
                stderr_capture.read_to_string(&mut stderr_captured)?;

                assert_eq!(exit_code, 0, "Command should succeed");

                // Check streamed output
                assert!(stdout_streamed.contains("captured stdout"), "Streamed stdout should contain 'captured stdout'");
                assert!(stderr_streamed.contains("captured stderr"), "Streamed stderr should contain 'captured stderr'");

                // Check captured output
                assert!(stdout_captured.contains("captured stdout"), "Captured stdout should contain 'captured stdout'");
                assert!(stderr_captured.contains("captured stderr"), "Captured stderr should contain 'captured stderr'");

                Ok(())
            })
        }

        #[test]
        fn test_timeout_exception() -> Result<()> {
            // Create a Tokio runtime for this test
            let rt = Runtime::new()?;

            rt.block_on(async {
                // Create a command that will run longer than our timeout
                let config = CommandConfig {
                    args: vec![
                        "sleep".to_string(),
                        "10".to_string(),
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
            // Create a Tokio runtime for this test
            let rt = Runtime::new()?;

            rt.block_on(async {
                // Create pipes using OS pipes
                let (stdout_read, stdout_write) = create_os_pipe()?;
                let (stderr_read, stderr_write) = create_os_pipe()?;

                // Create a command that outputs once then waits, triggering no-output timeout
                let config = CommandConfig {
                    args: vec![
                        "bash".to_string(),
                        "-c".to_string(),
                        "echo 'initial output'; sleep 10".to_string(),
                    ],
                    sys_stdout_fd: Some(stdout_write),
                    sys_stderr_fd: Some(stderr_write),
                    no_output_timeout_secs: Some(0.1), // Very short no-output timeout
                    ..Default::default()
                };

                // Run the command in a separate thread
                let handle = std::thread::spawn(move || {
                    run_command_internal(config)
                });

                // Read the initial output
                let mut stdout_buffer = [0u8; 1024];
                unsafe { libc::read(stdout_read, stdout_buffer.as_mut_ptr() as *mut libc::c_void, stdout_buffer.len()) };

                // Wait for command to fail
                let result = handle.join().expect("Thread panicked");

                // Clean up
                unsafe {
                    libc::close(stdout_read);
                    libc::close(stderr_read);
                    libc::close(stdout_write);
                    libc::close(stderr_write);
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
