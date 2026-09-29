//! Parse and validate a single third-party manifest fragment file.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use super::model::ManifestFragment;
use crate::manifest::{
    MIN_READABLE_FRAGMENT_SCHEMA, PluginWarning, PluginWarningKind, SCHEMA_VERSION,
};

/// The release that raised `MIN_READABLE_FRAGMENT_SCHEMA` to its current value.
const MIN_READABLE_FRAGMENT_RELEASE: &str = "0.34.0";

#[derive(Debug, Error)]
pub enum ThirdPartyError {
    #[error("non-UTF-8 path: {0}")]
    NonUtf8Path(PathBuf),
    #[error("glob pattern error: {0}")]
    Pattern(#[from] glob::PatternError),
    #[error("glob iteration error: {0}")]
    Glob(#[from] glob::GlobError),
    #[error("I/O error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "{path}: missing or non-integer `toolr_schema_version` key — \
         this file is not a valid toolr manifest fragment"
    )]
    MissingVersion { path: PathBuf },
    #[error(
        "duplicate command `{group}/{name}` declared by both `{first_package}` \
         and `{second_package}`"
    )]
    DuplicateCommand {
        group: String,
        name: String,
        first_package: String,
        second_package: String,
    },
}

/// The outcome of reading one fragment that is at least a well-formed, versioned fragment.
#[derive(Debug)]
pub enum ParsedFragment {
    /// Ready to merge; carries the file it came from.
    Loaded(ManifestFragment, PathBuf),
    /// The whole plugin is left out, and the warning says why.
    Skipped(PluginWarning),
}

/// Parse one fragment file. A fragment outside the load rule
/// (`MIN_READABLE_FRAGMENT_SCHEMA..=SCHEMA_VERSION`) or with an invalid argument is skipped with
/// a warning, not an error.
pub fn parse_fragment(path: &Path) -> Result<ParsedFragment, ThirdPartyError> {
    let bytes = fs::read(path).map_err(|e| ThirdPartyError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    let raw: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| ThirdPartyError::Json {
            path: path.to_path_buf(),
            source: e,
        })?;

    let version = raw
        .as_object()
        .and_then(|m| m.get("toolr_schema_version"))
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v >= 1)
        .ok_or_else(|| ThirdPartyError::MissingVersion {
            path: path.to_path_buf(),
        })?;

    // Checked on the raw value: an out-of-range fragment may not deserialise at all.
    let out_of_range = if version < MIN_READABLE_FRAGMENT_SCHEMA {
        Some(format!(
            "built with toolr schema {version}, this toolr needs >= {MIN_READABLE_FRAGMENT_SCHEMA}. \
             Rebuild the plugin with toolr >= {MIN_READABLE_FRAGMENT_RELEASE}."
        ))
    } else if version > SCHEMA_VERSION {
        Some(format!(
            "needs toolr schema {version}, this toolr supports {SCHEMA_VERSION}. Upgrade toolr."
        ))
    } else {
        None
    };
    if let Some(reason) = out_of_range {
        let package = raw_package_name(&raw, path);
        return Ok(ParsedFragment::Skipped(skipped(package, path, &reason)));
    }

    let fragment: ManifestFragment =
        serde_json::from_value(raw).map_err(|e| ThirdPartyError::Json {
            path: path.to_path_buf(),
            source: e,
        })?;

    // Validated whole before returning, so a bad command never leads to a partial merge.
    let invalid = fragment
        .commands
        .iter()
        .flat_map(|c| &c.arguments)
        .find_map(|a| a.validate().err());
    if let Some(reason) = invalid {
        let warning = skipped(fragment.package, path, &reason);
        return Ok(ParsedFragment::Skipped(warning));
    }

    Ok(ParsedFragment::Loaded(fragment, path.to_path_buf()))
}

fn skipped(package: String, path: &Path, reason: &str) -> PluginWarning {
    PluginWarning {
        message: format!("skipping plugin {package}: {reason}"),
        package,
        path: path.to_path_buf(),
        kind: PluginWarningKind::Skipped,
    }
}

/// The `package` key, or the fragment's directory name when the key is missing.
fn raw_package_name(raw: &serde_json::Value, path: &Path) -> String {
    raw.get("package")
        .and_then(serde_json::Value::as_str)
        .map(String::from)
        .or_else(|| {
            path.parent()
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}
