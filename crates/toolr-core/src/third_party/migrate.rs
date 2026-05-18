//! Schema-version migration framework for third-party manifest fragments.
//!
//! Adding a future migration is mechanical:
//!
//! 1. Bump `FRAGMENT_SCHEMA_VERSION` in `model.rs`.
//! 2. Add a `migrate_vN_to_vN_plus_1` function below.
//! 3. Register it in `step(..)`'s match arm.
//! 4. Update existing fragment-model field defaults / `#[serde(default)]`
//!    so v1-shaped input still deserializes after step-up.
//! 5. Add a unit test exercising a v(N) fixture through migration to vCurrent.

use serde_json::Value;

use super::model::FRAGMENT_SCHEMA_VERSION;

/// Migrate `raw` JSON forward from `from_version` to
/// `FRAGMENT_SCHEMA_VERSION`, applying registered migrations in order.
///
/// Returns the migrated JSON value on success, or a human-readable
/// reason string on failure.
pub fn migrate_to_current(raw: Value, from_version: u32) -> Result<Value, String> {
    let mut value = raw;
    let mut v = from_version;
    while v < FRAGMENT_SCHEMA_VERSION {
        value = step(value, v)?;
        v += 1;
    }
    Ok(value)
}

/// Apply a single version step `v -> v+1`. Add a match arm for each new
/// migration as the schema evolves.
///
/// No migrations exist yet — `FRAGMENT_SCHEMA_VERSION == 1` means
/// `migrate_to_current` is a no-op. The shape preserved here is for
/// future migrations like:
///
/// ```ignore
/// match v {
///     1 => migrate_v1_to_v2(value),
///     2 => migrate_v2_to_v3(value),
///     _ => Err(format!("no migration registered for v{v} -> v{}", v + 1)),
/// }
/// ```
#[allow(clippy::match_single_binding)]
fn step(_value: Value, v: u32) -> Result<Value, String> {
    match v {
        _ => Err(format!("no migration registered for v{v} -> v{}", v + 1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn v1_to_current_is_identity() {
        // At FRAGMENT_SCHEMA_VERSION == 1, this should be a no-op.
        let input = json!({
            "toolr_schema_version": 1,
            "package": "my_pkg",
            "groups": [],
            "commands": []
        });
        let out = migrate_to_current(input.clone(), 1).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn unregistered_step_returns_error() {
        // Force a step from "99" — there's no migration for it.
        let err = super::step(json!({}), 99).unwrap_err();
        assert!(err.contains("no migration registered"));
    }
}
