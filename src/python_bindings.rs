use std::collections::HashMap;
use std::path::PathBuf;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use crate::command::{
    CommandConfig, run_command_internal,
    CommandExecutionError, CommandTimeoutExceededError, CommandNoOutputTimeoutError
};

#[cfg(windows)]
use std::os::windows::io::RawHandle;
#[cfg(windows)]
use crate::command::ThreadSafeHandle;

// Define Python-specific exceptions
pyo3::create_exception!(_command, CommandError, PyException);
pyo3::create_exception!(_command, CommandTimeoutError, CommandError);
pyo3::create_exception!(_command, CommandTimeoutNoOutputError, CommandError);

#[cfg(windows)]
fn fd_to_handle(fd: i32) -> RawHandle {
    unsafe { libc::get_osfhandle(fd) as RawHandle }
}

#[pyfunction]
#[pyo3(signature = (
    args,
    env = HashMap::new(),
    input = None,
    stdout_fd = None,
    stderr_fd = None,
    sys_stdout_fd = None,
    sys_stderr_fd = None,
    timeout_secs = None,
    no_output_timeout_secs = None,
    cwd = None
))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_command_impl(
    args: Vec<String>,
    env: HashMap<String, String>,
    input: Option<&[u8]>,
    stdout_fd: Option<i32>,
    stderr_fd: Option<i32>,
    sys_stdout_fd: Option<i32>,
    sys_stderr_fd: Option<i32>,
    timeout_secs: Option<f64>,
    no_output_timeout_secs: Option<f64>,
    cwd: Option<String>,
) -> PyResult<i32> {
    // If one of the file descriptors is provided, both must be
    if (stdout_fd.is_some() && stderr_fd.is_none()) ||
       (stdout_fd.is_none() && stderr_fd.is_some()) {
        return Err(CommandError::new_err(
            "Both stdout_fd and stderr_fd must be provided together"
        ));
    }

    // If one of the sys file descriptors is provided, both must be
    if (sys_stdout_fd.is_some() && sys_stderr_fd.is_none()) ||
       (sys_stdout_fd.is_none() && sys_stderr_fd.is_some()) {
        return Err(CommandError::new_err(
            "Both sys_stdout_fd and sys_stderr_fd must be provided together"
        ));
    }

    let input_vec = input.map(|data| data.to_vec());

    // Convert cwd string to PathBuf if provided
    let cwd_path = cwd.map(PathBuf::from);

    let config = CommandConfig {
        args,
        env,
        input: input_vec,
        #[cfg(unix)]
        stdout_fd,
        #[cfg(unix)]
        stderr_fd,
        #[cfg(unix)]
        sys_stdout_fd,
        #[cfg(unix)]
        sys_stderr_fd,
        #[cfg(windows)]
        stdout_fd: stdout_fd.map(|fd| {
            ThreadSafeHandle::new(fd_to_handle(fd))
        }),
        #[cfg(windows)]
        stderr_fd: stderr_fd.map(|fd| {
            ThreadSafeHandle::new(fd_to_handle(fd))
        }),
        #[cfg(windows)]
        sys_stdout_fd: sys_stdout_fd.map(|fd| {
            ThreadSafeHandle::new(fd_to_handle(fd))
        }),
        #[cfg(windows)]
        sys_stderr_fd: sys_stderr_fd.map(|fd| {
            ThreadSafeHandle::new(fd_to_handle(fd))
        }),
        timeout_secs,
        no_output_timeout_secs,
        cwd: cwd_path,
    };

    // Call the internal function and map errors to appropriate Python exceptions
    match run_command_internal(config) {
        Ok(exit_code) => Ok(exit_code),
        Err(err) => {
            // Try to downcast to specific error types
            if let Some(timeout_err) = err.downcast_ref::<CommandTimeoutExceededError>() {
                Err(CommandTimeoutError::new_err(timeout_err.to_string()))
            } else if let Some(no_output_err) = err.downcast_ref::<CommandNoOutputTimeoutError>() {
                Err(CommandTimeoutNoOutputError::new_err(no_output_err.to_string()))
            } else if let Some(cmd_err) = err.downcast_ref::<CommandExecutionError>() {
                Err(CommandError::new_err(cmd_err.to_string()))
            } else {
                // Fall back to generic error message for unknown error types
                Err(CommandError::new_err(format!("Command failed: {err}")))
            }
        }
    }
}

// Python module definition
#[pymodule]
pub fn _command(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Register our function
    m.add_function(wrap_pyfunction!(run_command_impl, m)?)?;

    // Register our exceptions
    m.add("CommandError", m.py().get_type::<CommandError>())?;
    m.add("CommandTimeoutError", m.py().get_type::<CommandTimeoutError>())?;
    m.add("CommandTimeoutNoOutputError", m.py().get_type::<CommandTimeoutNoOutputError>())?;

    Ok(())
}
