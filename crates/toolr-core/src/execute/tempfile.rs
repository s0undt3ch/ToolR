//! Write an [`ExecutionSpec`] to a private tempfile that auto-deletes
//! when the returned handle is dropped (including on panic).

use std::io::{self, Write};

use tempfile::{Builder, NamedTempFile};

use super::spec::ExecutionSpec;

/// Write `spec` to a fresh tempfile and return its handle. The caller
/// must keep the handle alive for as long as the path is needed —
/// dropping it deletes the file.
pub fn write_spec_to_tempfile(spec: &ExecutionSpec) -> io::Result<NamedTempFile> {
    let mut file = Builder::new()
        .prefix("toolr-spec-")
        .suffix(".json")
        .rand_bytes(12)
        .tempfile()?;
    let bytes = serde_json::to_vec(spec).map_err(io::Error::other)?;
    file.write_all(&bytes)?;
    file.flush()?;
    Ok(file)
}

#[cfg(test)]
mod tests;
