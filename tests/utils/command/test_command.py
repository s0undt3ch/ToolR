from __future__ import annotations

import os
import pathlib
import tempfile
import threading
import time

import pytest

from toolr.utils.command import CommandError
from toolr.utils.command import CommandTimeoutError
from toolr.utils.command import CommandTimeoutNoOutputError
from toolr.utils.command import run_command
from toolr.utils.command import run_command_impl


def test_simple_command():
    """Test basic command execution"""
    result = run_command(["echo", "Hello, World!"], capture_output=True)
    assert result.returncode == 0
    result.stdout.seek(0)  # Reset position to start of file
    assert "Hello, World!" in result.stdout.read()


def test_with_environment():
    """Test command execution with custom environment variables"""
    result = run_command(
        ["sh", "-c", "echo $CUSTOM_VAR"],
        env={"CUSTOM_VAR": "test_value"},
        capture_output=True,
    )
    assert result.returncode == 0
    result.stdout.seek(0)
    assert "test_value" in result.stdout.read()


def test_input_string():
    """Test providing input as string"""
    result = run_command(["cat"], input="hello from stdin", capture_output=True)
    assert result.returncode == 0
    result.stdout.seek(0)
    assert "hello from stdin" in result.stdout.read()


def test_input_bytes():
    """Test providing input as bytes"""
    result = run_command(["cat"], input=b"hello bytes", capture_output=True, text=False)
    assert result.returncode == 0
    result.stdout.seek(0)
    assert b"hello bytes" in result.stdout.read()


def test_text_mode():
    """Test text mode output handling"""
    result = run_command(["echo", "text mode test"], capture_output=True, text=True)
    assert result.returncode == 0
    result.stdout.seek(0)
    content = result.stdout.read()
    # In text mode, stdout should contain a string
    assert isinstance(content, str)
    assert "text mode test" in content


def test_bytes_mode():
    """Test bytes mode output handling"""
    result = run_command(["echo", "bytes mode test"], capture_output=True, text=False)
    assert result.returncode == 0
    result.stdout.seek(0)
    content = result.stdout.read()
    # In bytes mode, stdout should contain bytes
    assert isinstance(content, bytes)
    assert b"bytes mode test" in content


def test_command_timeout():
    """Test command timeout functionality"""
    with pytest.raises(CommandTimeoutError):
        run_command(["sleep", "1"], timeout_secs=0.5)


def test_no_output_timeout_secs():
    """Test no-output timeout functionality with stream_output=True"""
    with pytest.raises(CommandTimeoutNoOutputError):
        run_command(
            ["sleep", "1"],  # This command produces no output
            stream_output=True,  # Required for no_output_timeout_secs to work
            no_output_timeout_secs=0.5,
        )


def test_capture_output():
    """Test capture_output functionality"""
    result = run_command(["echo", "captured output"], capture_output=True)
    assert result.stdout is not None
    result.stdout.seek(0)
    content = result.stdout.read()
    assert "captured output" in content


def test_with_tmp_path(tmp_path):
    """Test using the tmp_path fixture"""
    # Create a file in the temporary directory
    test_file = tmp_path / "test.txt"
    test_file.write_text("test content")

    # Run a command that reads the file
    result = run_command(["cat", str(test_file)], capture_output=True)
    assert result.returncode == 0
    result.stdout.seek(0)
    assert "test content" in result.stdout.read()


def test_stream_and_capture(capfd):
    """Test streaming and capturing output simultaneously"""
    result = run_command(
        ["echo", "should be streamed and captured"],
        stream_output=True,
        capture_output=True,
    )

    # Check that output was streamed (captured by pytest's capfd)
    captured = capfd.readouterr()
    assert "should be streamed and captured" in captured.out

    # Check that output was also captured to the result
    result.stdout.seek(0)
    assert "should be streamed and captured" in result.stdout.read()


def test_float_timeout_secs():
    """Test that float timeout_secs works properly"""
    start_time = pytest.importorskip("time").time()

    with pytest.raises(CommandTimeoutError):
        run_command(
            ["sleep", "3"],
            timeout_secs=0.5,  # Half second timeout
        )

    elapsed = pytest.importorskip("time").time() - start_time
    assert elapsed < 2.0  # Verify timeout happened quickly


def test_float_no_output_timeout_secs():
    """Test that float no_output_timeout_secs works properly"""
    start_time = pytest.importorskip("time").time()

    with pytest.raises(CommandTimeoutNoOutputError):
        run_command(
            ["sleep", "3"],
            stream_output=True,  # Required for no_output_timeout_secs
            no_output_timeout_secs=0.5,  # Half second timeout
        )

    elapsed = pytest.importorskip("time").time() - start_time
    assert elapsed < 2.0  # Verify timeout happened quickly


def test_environ_inheritance():
    """Test that os.environ is used when env=None"""
    # Set a unique environment variable
    test_var = "TOOLR_TEST_VAR"
    test_value = "test_value_" + str(os.getpid())
    os.environ[test_var] = test_value

    try:
        # Run a command without specifying env
        result = run_command(["sh", "-c", f"echo ${test_var}"], capture_output=True)

        # Should inherit the environment variable
        result.stdout.seek(0)
        assert test_value in result.stdout.read()
    finally:
        # Clean up
        if test_var in os.environ:
            del os.environ[test_var]


def test_stream_output_text_only():
    """Test that stream_output=True requires text=True"""
    with pytest.raises(ValueError, match="stream_output=True requires text=True"):
        run_command(["echo", "test"], stream_output=True, text=False)


def test_stream_output_both_fd_required():
    """Test that sys_stdout_fd and sys_stderr_fd must both be provided"""
    # Directly access the low-level implementation to test the requirement

    with pytest.raises(CommandError, match="Both sys_stdout_fd and sys_stderr_fd must be provided together"):
        # Mock case where only stdout fd is provided
        run_command_impl(
            ["echo", "test"],
            sys_stdout_fd=1,  # stdout fd
            sys_stderr_fd=None,
        )

    with pytest.raises(CommandError, match="Both sys_stdout_fd and sys_stderr_fd must be provided together"):
        # Mock case where only stderr fd is provided
        run_command_impl(
            ["echo", "test"],
            sys_stdout_fd=None,
            sys_stderr_fd=2,  # stderr fd
        )


def test_fd_streaming_works():
    """Test that streaming with file descriptors works correctly"""
    import os

    # Create pipe pairs for stdout and stderr
    r_stdout, w_stdout = os.pipe()
    r_stderr, w_stderr = os.pipe()

    # Run command with these file descriptors
    # Run in a separate thread so we don't block

    result_code = None
    exception = None

    def run_cmd():
        nonlocal result_code, exception
        try:
            result_code = run_command_impl(
                ["bash", "-c", "echo 'to stdout'; echo 'to stderr' >&2"],
                sys_stdout_fd=w_stdout,
                sys_stderr_fd=w_stderr,
            )
        except Exception as e:
            exception = e

    thread = threading.Thread(target=run_cmd)
    thread.start()

    # Read from the pipes
    os.close(w_stdout)  # Close write end in this process
    os.close(w_stderr)  # Close write end in this process

    stdout_reader = os.fdopen(r_stdout, "r")
    stderr_reader = os.fdopen(r_stderr, "r")

    stdout_content = stdout_reader.read()
    stderr_content = stderr_reader.read()

    # Wait for thread to finish
    thread.join(timeout=2.0)
    assert not thread.is_alive(), "Command did not complete"

    # Check results
    assert exception is None, f"Got exception: {exception}"
    assert result_code == 0, f"Expected exit code 0, got {result_code}"
    assert "to stdout" in stdout_content
    assert "to stderr" in stderr_content


def test_timeout():
    start = time.time()
    with pytest.raises(CommandTimeoutError):
        run_command(["sleep", "1"], timeout_secs=0.1)
    elapsed = time.time() - start
    assert elapsed < 1.0  # Verify timeout happened quickly


def test_no_output_timeout():
    start = time.time()
    with pytest.raises(CommandTimeoutNoOutputError):
        run_command(["sleep", "1"], stream_output=True, no_output_timeout_secs=0.1)
    elapsed = time.time() - start
    assert elapsed < 1.0  # Verify timeout happened quickly


def test_specific_fd_with_capture():
    """Test streaming to specific file descriptors while also capturing."""
    # Create pipe pairs for custom stdout and stderr
    r_stdout, w_stdout = os.pipe()
    r_stderr, w_stderr = os.pipe()

    # Create temp files for capturing
    with tempfile.TemporaryFile() as stdout_capture, tempfile.TemporaryFile() as stderr_capture:
        result_code = None
        exception = None

        def run_cmd():
            nonlocal result_code, exception
            try:
                result_code = run_command_impl(
                    ["bash", "-c", "echo 'to both stdout'; echo 'to both stderr' >&2"],
                    sys_stdout_fd=w_stdout,
                    sys_stderr_fd=w_stderr,
                    stdout_fd=stdout_capture.fileno(),
                    stderr_fd=stderr_capture.fileno(),
                )
            except Exception as e:
                exception = e

        thread = threading.Thread(target=run_cmd)
        thread.start()

        # Read from the pipes (for streaming)
        os.close(w_stdout)
        os.close(w_stderr)

        stdout_reader = os.fdopen(r_stdout, "r")
        stderr_reader = os.fdopen(r_stderr, "r")

        stdout_streamed = stdout_reader.read()
        stderr_streamed = stderr_reader.read()

        # Wait for thread to finish
        thread.join(timeout=2.0)
        assert not thread.is_alive(), "Command did not complete"
        assert exception is None, f"Got exception: {exception}"

        # Read from the capture files
        stdout_capture.seek(0)
        stderr_capture.seek(0)

        stdout_captured = stdout_capture.read().decode("utf-8")
        stderr_captured = stderr_capture.read().decode("utf-8")

        # Verify both streaming and capturing worked
        assert "to both stdout" in stdout_streamed
        assert "to both stderr" in stderr_streamed
        assert "to both stdout" in stdout_captured
        assert "to both stderr" in stderr_captured


def test_command_with_cwd(tmp_path):
    """Test that commands execute in the specified working directory."""

    # Create a unique marker file in the temporary directory
    marker_file = tmp_path / "marker.txt"
    marker_content = f"Test marker content {os.getpid()}"
    marker_file.write_text(marker_content)

    # 1. Run 'cat marker.txt' in the temp directory - should succeed
    result = run_command(["cat", "marker.txt"], cwd=str(tmp_path), capture_output=True)

    assert result.returncode == 0
    result.stdout.seek(0)
    content = result.stdout.read()
    assert marker_content in content, f"Expected content '{marker_content}' in output: '{content}'"

    # 2. Try running without cwd - should fail because the file doesn't exist in the current directory
    result = run_command(["cat", "marker.txt"], capture_output=True)
    assert result.returncode != 0, "Expected command to fail without the right cwd"

    # 3. Test with pwd to verify we get back the expected directory
    result = run_command(["pwd"], cwd=str(tmp_path), capture_output=True)
    assert result.returncode == 0
    result.stdout.seek(0)
    pwd_output = result.stdout.read().strip()

    # Convert paths to resolved Path objects to handle symlinks
    resolved_tmp_path = tmp_path.resolve()
    resolved_pwd_path = pathlib.Path(pwd_output).resolve()

    assert resolved_pwd_path == resolved_tmp_path, f"Expected directory {resolved_tmp_path}, got {resolved_pwd_path}"

    # 4. Test relative path handling with cwd
    # Create a subdirectory in tmp_path
    subdir = tmp_path / "subdir"
    subdir.mkdir()

    # Create a file in the subdirectory
    sub_file = subdir / "sub_marker.txt"
    sub_content = f"Subdirectory content {os.getpid()}"
    sub_file.write_text(sub_content)

    # Run a command with relative path from tmp_path
    result = run_command(["cat", "subdir/sub_marker.txt"], cwd=str(tmp_path), capture_output=True)

    assert result.returncode == 0
    result.stdout.seek(0)
    content = result.stdout.read()
    assert sub_content in content, f"Expected content '{sub_content}' in output: '{content}'"
