//! Clock helpers.
//!
//! Extracted from `window/workspace_ops.rs` during the dat0-core split: the
//! package writer and the workspace-manifest writer both need a timestamp and
//! neither is a UI concern.

/// Epoch-seconds timestamp string used as the `now_rfc3339` argument to
/// `promote_files`. dat0 carries no time/chrono/jiff dependency; an integer
/// seconds string is acceptable because the manifest timestamp is
/// informational, not load-bearing.
pub fn now_epoch_secs() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_epoch_secs_is_a_plausible_unix_timestamp() {
        let s = now_epoch_secs();
        let n: u64 = s.parse().expect("digits only");
        // 2020-01-01 .. 2200-01-01: catches a unit slip (millis/nanos) or a
        // zeroed clock without pinning the test to a wall-clock date.
        assert!((1_577_836_800..7_258_118_400).contains(&n), "{s}");
    }
}
