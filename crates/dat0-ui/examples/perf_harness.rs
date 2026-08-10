//! Runs one perf scenario in a real window and prints one JSON line.
//!
//! ```text
//! cargo run --release --features perf-harness --example perf_harness -- scroll_1m
//! ```
//!
//! Deliberately thin: everything is in `dat0_ui::perf` so it is reachable from a
//! test. An example body is unreachable from any test — logic here would rot
//! unseen.
//!
//! `cold_launch` is NOT runnable from here. It measures the real `dat0` binary,
//! which is what a user double-clicks; `xtask perf` invokes that directly with
//! `DAT0_PERF_COLD_LAUNCH=1`.

use dat0_core::perf::harness::Scenario;

fn main() -> anyhow::Result<()> {
    let name = std::env::args().nth(1).unwrap_or_default();
    let Some(scenario) = Scenario::parse(&name) else {
        anyhow::bail!(
            "usage: perf_harness <scroll_1m|scroll_10m|open_csv_10gb|open_parquet_1gb|idle_rss>\n\
             got {name:?}"
        );
    };
    dat0_ui::perf::run_windowed(scenario)
}
