//! The perf harness's toolkit-free half: what to measure, and the fixtures.
//!
//! The runner that opens a window lives in the UI crate, because only it knows
//! what a window is. Everything here — the scenario list, the budgets, the
//! fixture paths, the watchdog — is shared by whichever shell runs it and by
//! `xtask perf`, which reads the same names.
//!
//! ## What this measures, and what it does not
//!
//! It opens a real 1440x900 window with a real `WorkspaceShell`, so the numbers
//! include layout, the gpui-component `Table`, the LRU page cache and DuckDB —
//! the whole stack a user's scroll goes through. The existing
//! `benches/grid_scroll.rs` measures `render_cell` in a loop and its own doc
//! block calls that "evidence of nothing"; this is the instrument that replaces
//! the guess.
//!
//! It does **not** pre-warm the page cache. A frame whose page misses the LRU
//! paints em-dashes rather than blocking (`grid/mod.rs`), and that is real
//! product behaviour. Warming it would measure a grid the user never sees.

use std::path::PathBuf;

use anyhow::{Context, Result};

use super::{DriveState, FrameClock};

/// How many frames each scroll scenario drives. Chosen so a p99 has ~6 samples
/// to stand on at 60 fps, which is the point where the number stops being one
/// unlucky frame.
pub const SCROLL_FRAMES: usize = 600;

/// Rows for the two scroll scenarios.
pub const SCROLL_1M_ROWS: usize = 1_000_000;
pub const SCROLL_10M_ROWS: usize = 10_000_000;

/// Seconds `idle_rss` lets the process settle before sampling.
pub const IDLE_SETTLE_SECS: u64 = 3;

/// DuckDB memory limit for every harness run.
///
/// Fixed rather than read from `settings.toml`: a budget that varies with the
/// developer's config would make two runs on the same machine incomparable, and
/// the whole point of the baseline is that they are comparable. 4 GiB is well
/// above what `scroll_10m` needs to page and well below what a CI runner has.
pub const HARNESS_MEMORY_BUDGET_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Hard ceiling on a single scenario, in seconds. Overridable with
/// `DAT0_PERF_TIMEOUT_SECS`.
///
/// A perf harness that can hang is worse than no perf harness: it wedges a CI
/// job for the runner's whole timeout and reports nothing. On expiry the
/// watchdog prints WHY — the observed frame count separates "the window never
/// painted" from "it painted but the drive never finished" — and exits 3, which
/// `xtask` surfaces as a scenario error rather than a budget breach.
pub const DEFAULT_TIMEOUT_SECS: u64 = 180;

pub fn timeout_secs() -> u64 {
    std::env::var("DAT0_PERF_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SECS)
}

/// How long to wait before deciding this process is getting no vsync.
pub const NO_DISPLAY_GRACE_SECS: u64 = 10;

/// Set by the UI harness as soon as its in-window driver reports for duty.
///
/// The liveness signal, replacing the GPUI frame counter this used to read.
/// GPUI redrew only from the platform display link, so "frames painted" told
/// you whether there was a GUI session; a WebView animates from its own timers
/// and paints regardless, so frame count says nothing. What does say something
/// is whether the driver inside the window ever ran at all — no display server,
/// no webview, no ping.
pub static DRIVER_ALIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Arm the watchdog. Never joined — it either fires or the process exits first.
///
/// Two arms, and the distinction matters:
///
/// - **No window.** With no display server there is no WebView, so the driver
///   inside the window never runs and [`DRIVER_ALIVE`] stays false. A
///   frame-interval number is then not slow — it is *unmeasurable*, and
///   inventing one would be worse than reporting none. The harness exits 0
///   printing NO JSON line, which is exactly how `xtask perf` recognises a
///   skip.
/// - **Genuine hang.** The driver is running but the scenario's completion
///   condition never became true. That is a bug, and it exits 3.
pub fn spawn_watchdog(scenario: &'static str) {
    let limit = timeout_secs();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(NO_DISPLAY_GRACE_SECS));
        if !DRIVER_ALIVE.load(std::sync::atomic::Ordering::Relaxed) {
            eprintln!(
                "SKIP {scenario}: the in-window driver did not report within \
                 {NO_DISPLAY_GRACE_SECS}s.\n\
                 That means no WebView came up — almost always a process with no \
                 display server. Run this on a desktop session, or leave the \
                 scenario to the `perf-gate` job on dedicated hardware (D-032)."
            );
            std::process::exit(0)
        }
        std::thread::sleep(std::time::Duration::from_secs(
            limit.saturating_sub(NO_DISPLAY_GRACE_SECS),
        ));
        eprintln!(
            "perf harness: {scenario} did not finish within {limit}s.\n\
             The driver DID report, so this is the scenario's completion condition \
             never becoming true, not a missing display."
        );
        std::process::exit(3)
    });
}

/// Every scenario the harness itself can run. `cold_launch` is absent on
/// purpose: it measures the REAL `dat0` binary, which has a different link
/// line, different features and a `main.rs` this example does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scenario {
    Scroll1M,
    Scroll10M,
    OpenCsv10Gb,
    OpenParquet1Gb,
    IdleRss,
}

impl Scenario {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "scroll_1m" => Self::Scroll1M,
            "scroll_10m" => Self::Scroll10M,
            "open_csv_10gb" => Self::OpenCsv10Gb,
            "open_parquet_1gb" => Self::OpenParquet1Gb,
            "idle_rss" => Self::IdleRss,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Scroll1M => "scroll_1m",
            Self::Scroll10M => "scroll_10m",
            Self::OpenCsv10Gb => "open_csv_10gb",
            Self::OpenParquet1Gb => "open_parquet_1gb",
            Self::IdleRss => "idle_rss",
        }
    }

    /// Rows the scenario's fixture carries, when it has one.
    pub fn rows(self) -> Option<usize> {
        match self {
            Self::Scroll1M => Some(SCROLL_1M_ROWS),
            Self::Scroll10M => Some(SCROLL_10M_ROWS),
            Self::OpenCsv10Gb | Self::OpenParquet1Gb | Self::IdleRss => None,
        }
    }

    /// Whether the scenario measures frame intervals (rather than wall time or
    /// resident memory). Only these arm the drive loop.
    pub fn drives_frames(self) -> bool {
        matches!(self, Self::Scroll1M | Self::Scroll10M)
    }

    /// Whether the scenario needs a table to measure at all.
    ///
    /// `idle_rss` does not — it measures a window with nothing open, which is
    /// the point. Everything else without its fixture must SKIP rather than
    /// report a zero.
    pub fn needs_fixture(self) -> bool {
        !matches!(self, Self::IdleRss)
    }
}

/// Where generated fixtures live. Under `target/` so a `git status` stays clean
/// and CI's cache eviction can reclaim ten gigabytes without anyone noticing.
pub fn fixture_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("DAT0_PERF_FIXTURE_DIR")
            .unwrap_or_else(|| std::ffi::OsString::from("target/perf-fixtures")),
    )
}

/// Generate (or reuse) the CSV a scroll scenario reads.
///
/// `gen_filter_fixture` is already deterministic and already skips a file that
/// exists, so a repeat run costs a `stat`. A 10 M-row CSV is ~450 MB and takes
/// minutes to write — regenerating it per run would make the gate too slow to
/// keep.
#[cfg(feature = "perf-harness")]
pub fn ensure_scroll_fixture(rows: usize) -> Result<PathBuf> {
    let dir = fixture_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    dat0_fixtures::filter::gen_filter_fixture(&dir, rows, 0xD470)
        .with_context(|| format!("generate {rows}-row perf fixture"))
}

/// A pre-generated large fixture, or `None` when it is absent.
///
/// The two multi-gigabyte scenarios deliberately do NOT generate their own
/// input: a 10 GB CSV takes long enough that a developer running
/// `cargo xtask perf` by accident would think the tool had hung. They skip
/// instead, and `xtask` reports `SKIP` with exit 0.
pub fn optional_large_fixture(name: &str) -> Option<PathBuf> {
    let p = PathBuf::from("tests/fixtures/large").join(name);
    p.exists().then_some(p)
}

/// Arm `clock` to drive `frames` scripted frames from row 0.
///
/// Also widens the sample window to cover the whole run, so a stall in the
/// first third cannot age out of the percentile before the budget is checked.
pub fn arm_drive(clock: &mut FrameClock, frames: usize) {
    clock.set_window(frames + 1);
    clock.drive(Some(DriveState {
        next_row: 0,
        frames_left: frames,
    }));
}

/// Whether the scripted scroll has finished.
pub fn drive_finished(clock: &mut FrameClock) -> bool {
    clock.drive_state().is_none_or(|s| s.frames_left == 0)
}
