//! Tracing instrumentation helpers.
//!
//! Per spec §7 commitment 3: never log SQL text (potential PII; P1 Sentry
//! redaction skips it). Wrap span construction so contributors don't have to
//! remember `skip_all` + `fields(sql_len)` every time.

/// Returns the byte length of the SQL string, for use as a span field.
/// SQL text itself is never logged.
#[inline]
#[allow(dead_code)] // First call site lands in T7 (execute family).
pub(crate) fn sql_len(sql: &str) -> usize {
    sql.len()
}
