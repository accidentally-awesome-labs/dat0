//! Top-N distinct values fetch + debounce for the IN-list panel.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use dat0_engine::{DuckDBEngine, QueryEngine};
use duckdb::arrow::array::{Array, Int64Array, StringArray};

pub const TOP_N: u64 = 50;
pub const DEBOUNCE: Duration = Duration::from_millis(150);

#[derive(Debug, Clone)]
pub struct DistinctValue {
    pub value: String,
    pub count: u64,
}

/// Fetch top-N distinct values for `column` of `base_table`. Returns the
/// values in count-descending order. Also returns the *total* distinct count
/// so the caller can show the "Showing 50 of N" banner when needed.
///
/// NB: `column` and `base_table` are quoted internally.
pub async fn fetch_top_n(
    engine: Arc<DuckDBEngine>,
    base_table: &str,
    column: &str,
) -> Result<(Vec<DistinctValue>, u64)> {
    let col_q = quote(column);
    let tbl_q = quote(base_table);

    // Total distinct count first (cheap; needed for the banner).
    let total_sql = format!("SELECT COUNT(DISTINCT {})::BIGINT FROM {}", col_q, tbl_q);
    let total_result = engine.execute(&total_sql).await?;
    let total = total_result
        .batches
        .first()
        .and_then(|b| {
            b.column(0)
                .as_any()
                .downcast_ref::<Int64Array>()
                .map(|a| a.value(0) as u64)
        })
        .unwrap_or(0);

    // Top-N values by count.
    let topn_sql = format!(
        "SELECT {} AS v, COUNT(*)::BIGINT AS c FROM {} GROUP BY 1 ORDER BY 2 DESC LIMIT {}",
        col_q, tbl_q, TOP_N
    );
    let result = engine.execute(&topn_sql).await?;
    let mut values = Vec::with_capacity(TOP_N as usize);
    for batch in result.batches {
        let v_col = batch.column(0);
        let c_col = batch
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| anyhow::anyhow!("count column not Int64"))?;
        for row in 0..batch.num_rows() {
            let value = render_cell(v_col, row);
            values.push(DistinctValue {
                value,
                count: c_col.value(row) as u64,
            });
        }
    }
    Ok((values, total))
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Render an arbitrary Arrow array cell as a DuckDB-compatible literal-friendly
/// string. The IN-list ships these into FilterValue::List as Scalar::Str — the
/// engine parses them at render time per ColumnType.
fn render_cell(col: &dyn Array, row: usize) -> String {
    // Try a few common types; fall back to the formatted debug representation.
    if let Some(a) = col.as_any().downcast_ref::<StringArray>() {
        return a.value(row).to_string();
    }
    if let Some(a) = col.as_any().downcast_ref::<Int64Array>() {
        return a.value(row).to_string();
    }
    // Fallback — DuckDB Display impl on the underlying array.
    format!("{:?}", col)
}

// ---------------------------------------------------------------------------
// Debounce state machine (pure, tokio-free, testable)
// ---------------------------------------------------------------------------

/// Tracks whether a pending debounce is in progress.
///
/// The state machine is driven by two methods:
/// - `on_keystroke()` — called on each user keystroke; marks a new pending
///   fetch with the associated generation counter.
/// - `should_fire(gen)` — called by the async task after `DEBOUNCE` sleep;
///   returns `true` only when the generation has not changed (i.e. no newer
///   keystroke arrived while it was sleeping).
///
/// This design keeps the debounce logic fully synchronous and testable without
/// a real tokio runtime — tests drive the state machine directly.
#[derive(Debug, Default)]
pub struct DebounceState {
    /// Monotonically increasing generation counter. Each keystroke bumps this.
    generation: u64,
}

impl DebounceState {
    pub fn new() -> Self {
        Self { generation: 0 }
    }

    /// Called on every keystroke. Returns the generation token the spawned
    /// async task should pass to `should_fire`.
    pub fn on_keystroke(&mut self) -> u64 {
        self.generation += 1;
        self.generation
    }

    /// Returns `true` when `token` is still the current generation — i.e. no
    /// newer keystroke arrived while the debounce timer was sleeping.
    pub fn should_fire(&self, token: u64) -> bool {
        self.generation == token
    }

    /// Current generation counter. Exposed for tests.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Returns `true` when the truncation banner should be shown.
///
/// The banner surfaces when the column has more distinct values than
/// TOP_N — i.e. the returned list was truncated. The exact condition is
/// `total > TOP_N`, where `total` is the value returned by
/// `fetch_top_n`'s second return element.
pub fn banner_needed(total: u64) -> bool {
    total > TOP_N
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_fires_on_single_keystroke() {
        let mut ds = DebounceState::new();
        let tok = ds.on_keystroke();
        assert_eq!(tok, 1);
        assert!(ds.should_fire(tok), "single keystroke should fire");
    }

    #[test]
    fn debounce_only_fires_for_latest_keystroke() {
        let mut ds = DebounceState::new();
        let _g1 = ds.on_keystroke();
        let _g2 = ds.on_keystroke();
        let g3 = ds.on_keystroke();
        assert!(!ds.should_fire(1), "stale gen 1 should not fire");
        assert!(!ds.should_fire(2), "stale gen 2 should not fire");
        assert!(ds.should_fire(g3), "latest gen should fire");
    }

    #[test]
    fn debounce_generation_monotonic() {
        let mut ds = DebounceState::new();
        for i in 1..=5 {
            let tok = ds.on_keystroke();
            assert_eq!(tok, i);
        }
        assert_eq!(ds.generation(), 5);
    }

    #[test]
    fn debounce_single_fire_after_rapid_typing() {
        // Simulate: user types 10 chars rapidly, then the last debounce timer
        // fires. Only the final generation should cause a fetch.
        let mut ds = DebounceState::new();
        let mut last_tok = 0;
        for _ in 0..10 {
            last_tok = ds.on_keystroke();
        }
        // All but the last should NOT fire.
        for tok in 1..last_tok {
            assert!(
                !ds.should_fire(tok),
                "tok {tok} should not fire (superseded)"
            );
        }
        assert!(ds.should_fire(last_tok), "only the last tok fires");
    }

    #[test]
    fn banner_triggered_when_total_exceeds_top_n() {
        // Unit test for the banner gate: total > TOP_N → banner needed.
        let total_at_limit: u64 = TOP_N;
        let total_over: u64 = TOP_N + 1;
        assert!(!banner_needed(total_at_limit), "exactly TOP_N → no banner");
        assert!(banner_needed(total_over), "total > TOP_N → banner shown");
    }

    #[test]
    fn banner_not_triggered_on_empty_result() {
        assert!(!banner_needed(0), "empty result → no banner");
    }
}
