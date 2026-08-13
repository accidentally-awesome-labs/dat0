//! MX1: real frame-interval instrumentation for the workspace window.
//!
//! Every "60 fps" claim dat0 makes — `README.md`, `docs/design/onboarding-v1.md`,
//! the marketing page — was unbacked before this module existed, and the only
//! bench in the tree says so about itself: `benches/grid_scroll.rs` calls
//! `render_cell` in a plain loop, never builds a `Window`, and its own doc block
//! calls the readings "evidence of nothing".
//!
//! This is the instrument that replaces the guess. It records nothing but wall
//! clock, costs one `Instant::now()` and at most one `VecDeque` eviction per
//! frame, and has no dependency beyond `std`.
//!
//! **Frame interval is not GPU time.** It is the gap between consecutive paints,
//! which is exactly what a claimed frame rate means and is the only quantity a
//! `Render` impl can honestly observe. Nothing here may be described as GPU or
//! draw-call time.

#[cfg(any(test, feature = "perf-harness"))]
pub mod harness;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Process-wide count of paints, incremented by every [`FrameClock::tick_at`].
///
/// Exists so the harness watchdog can say *why* a scenario stalled — "0 frames
/// in 120 s" (the window never painted) and "600 frames but never finished"
/// (the drive logic is wrong) are completely different bugs, and a bare
/// timeout cannot tell them apart. One relaxed atomic add per frame.
pub static FRAMES_TICKED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many paint instants to retain. At a sustained 60 fps this is four
/// seconds of history, which is long enough for a p99 over a scroll gesture and
/// short enough that a stall ages out of the readout while the user is still
/// looking at it.
pub const FRAME_WINDOW: usize = 240;

/// Frames older than this make the readout stale.
///
/// gpui paints on demand, so an idle window simply stops calling `render`. A
/// frame rate computed across the resulting gap would describe the user's
/// coffee break, not the renderer, so [`FrameClock::fps`] reports `None` and the
/// HUD renders an em-dash. It must never render `0`.
pub const IDLE_AFTER: Duration = Duration::from_millis(500);

/// Percentile window for [`FrameClock::percentile_ms`].
///
/// Deliberately shorter than [`FRAME_WINDOW`]: a percentile is a claim about
/// what the app is doing *now*, and mixing in three-second-old frames from
/// before the user started scrolling flatters the number.
const PERCENTILE_WINDOW: Duration = Duration::from_secs(1);

/// Set while a perf scenario is running: keeps `render` requesting the next
/// frame and advances the scripted scroll. `None` in normal use, so a shipped
/// window never spins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriveState {
    /// Row the next driven frame scrolls to.
    pub next_row: usize,
    /// Frames still owed. At zero the scenario stops advancing and the harness
    /// reads [`FrameClock::report`].
    pub frames_left: usize,
}

/// Rolling frame-interval recorder. One per window, owned by `WorkspaceShell`.
#[derive(Debug)]
pub struct FrameClock {
    /// Paint instants, oldest first. Bounded by [`Self::window`].
    samples: VecDeque<Instant>,
    /// Retention, in samples. [`FRAME_WINDOW`] in a normal window; the perf
    /// harness raises it so a whole scenario fits and a stall in the first
    /// third of a run cannot age out before the budget is checked.
    window: usize,
    drive: Option<DriveState>,
}

impl Default for FrameClock {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameClock {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(FRAME_WINDOW),
            window: FRAME_WINDOW,
            drive: None,
        }
    }

    /// Raise (or lower) the retention. Clamped to at least 2 — one sample
    /// defines no interval, so a window of 1 could never report anything.
    ///
    /// Exists for the perf harness: a 600-frame scenario whose percentiles were
    /// computed over only the last 240 paints would hide a stall in the first
    /// 360, and hiding stalls is the one thing a perf gate must not do.
    pub fn set_window(&mut self, frames: usize) {
        self.window = frames.max(2);
        while self.samples.len() > self.window {
            self.samples.pop_front();
        }
    }

    /// Call once at the top of `render`. Evicts samples beyond the window.
    pub fn tick(&mut self) {
        self.tick_at(Instant::now());
    }

    /// [`tick`](Self::tick) with the clock injected.
    ///
    /// Public because `tests/frame_clock.rs` is an integration binary and a
    /// rolling-window percentile asserted against `Instant::now()` would be a
    /// flaky test, which is the kind of test that gets deleted.
    pub fn tick_at(&mut self, now: Instant) {
        FRAMES_TICKED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        while self.samples.len() >= self.window {
            self.samples.pop_front();
        }
        self.samples.push_back(now);
    }

    /// Frames per second, from the mean of the recent real frame intervals.
    ///
    /// `None` when the newest sample is older than [`IDLE_AFTER`] or no real
    /// interval exists — one paint defines no interval.
    pub fn fps(&self) -> Option<f32> {
        self.fps_at(Instant::now())
    }

    /// [`fps`](Self::fps) with the clock injected. See [`tick_at`](Self::tick_at).
    pub fn fps_at(&self, now: Instant) -> Option<f32> {
        let gaps = self.gaps_ms(now, Some(PERCENTILE_WINDOW))?;
        let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
        if mean <= 0.0 {
            return None;
        }
        // Mean interval, not `count / span`: the span between the oldest and
        // newest sample includes any idle stretch, and dividing by it would
        // report 3 fps for a window the user simply stopped touching.
        Some(1000.0 / mean)
    }

    /// Percentile over real frame intervals in the last second.
    /// `p` in `0.0..=1.0`; nearest-rank, so `percentile_ms(1.0)` is the worst
    /// frame and `percentile_ms(0.0)` the best.
    pub fn percentile_ms(&self, p: f32) -> Option<f32> {
        self.percentile_ms_at(p, Instant::now())
    }

    /// [`percentile_ms`](Self::percentile_ms) with the clock injected.
    pub fn percentile_ms_at(&self, p: f32, now: Instant) -> Option<f32> {
        let gaps = self.gaps_ms(now, Some(PERCENTILE_WINDOW))?;
        Some(nearest_rank(&gaps, p))
    }

    /// Real frame intervals, in milliseconds, sorted ascending.
    ///
    /// `recency` bounds how far back a gap may end; `None` uses the whole
    /// retained window. Two exclusions, both load-bearing:
    ///
    /// - The newest sample must be within [`IDLE_AFTER`] of `now`, or the
    ///   readout is stale and the caller gets `None` rather than a number
    ///   describing the past.
    /// - Any gap longer than [`IDLE_AFTER`] is an **idle period, not a frame**.
    ///   gpui paints on demand, so the first paint after a two-second pause
    ///   would otherwise enter the sample set as a 2000 ms "frame" and put p99
    ///   two orders of magnitude off. Measured: this is what
    ///   `percentile_ignores_gaps_older_than_one_second` caught.
    fn gaps_ms(&self, now: Instant, recency: Option<Duration>) -> Option<Vec<f32>> {
        let (_, &newest) = self.span()?;
        if now.saturating_duration_since(newest) > IDLE_AFTER {
            return None;
        }
        let cutoff = recency.and_then(|r| newest.checked_sub(r));
        let mut gaps: Vec<f32> = self
            .samples
            .iter()
            .zip(self.samples.iter().skip(1))
            // A gap belongs to the window when the frame that *ended* it is
            // recent. Keying on the earlier instant would drop the gap that
            // straddles the cutoff, which is usually the slow one.
            .filter(|(_, end)| cutoff.is_none_or(|c| **end >= c))
            .map(|(start, end)| end.saturating_duration_since(*start))
            .filter(|gap| *gap <= IDLE_AFTER)
            .map(|gap| gap.as_secs_f32() * 1000.0)
            .collect();
        if gaps.is_empty() {
            return None;
        }
        gaps.sort_by(f32::total_cmp);
        Some(gaps)
    }

    pub fn drive(&mut self, state: Option<DriveState>) {
        self.drive = state;
    }

    pub fn drive_state(&mut self) -> Option<&mut DriveState> {
        self.drive.as_mut()
    }

    /// Snapshot for the harness: `(intervals, p50_ms, p95_ms, p99_ms)`.
    ///
    /// Two deliberate differences from [`percentile_ms`](Self::percentile_ms):
    ///
    /// - It spans the **whole retained window**, not the last second. A gate
    ///   asserting a p95 over a 600-frame scroll must see all 600; the live HUD
    ///   wants the opposite, a number that tracks what the user is doing now.
    /// - It anchors "now" on the newest sample, so a report taken a moment
    ///   after the last driven frame is not suppressed by [`IDLE_AFTER`].
    ///
    /// `intervals` is the count of **real frame intervals** the percentiles
    /// were computed from — always at most `samples - 1`, and fewer when the
    /// run contained an idle stretch. It is not the number of frames the
    /// scenario asked for; the harness reports that itself.
    pub fn report(&self) -> Option<(usize, f32, f32, f32)> {
        let (_, &newest) = self.span()?;
        let gaps = self.gaps_ms(newest, None)?;
        Some((
            gaps.len(),
            nearest_rank(&gaps, 0.50),
            nearest_rank(&gaps, 0.95),
            nearest_rank(&gaps, 0.99),
        ))
    }

    /// `(oldest, newest)`, or `None` below two samples.
    fn span(&self) -> Option<(&Instant, &Instant)> {
        if self.samples.len() < 2 {
            return None;
        }
        Some((self.samples.front()?, self.samples.back()?))
    }
}

/// Nearest-rank percentile over an ascending slice. `sorted` must be non-empty.
///
/// Nearest-rank rather than linear interpolation because it always returns a
/// value that was actually measured: a p99 of 14.2 ms means some frame really
/// took 14.2 ms. It is also the convention every latency dashboard uses, so a
/// p95 here is comparable to a p95 anywhere else.
fn nearest_rank(sorted: &[f32], p: f32) -> f32 {
    let rank = (p.clamp(0.0, 1.0) * sorted.len() as f32).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// One scenario's result, serialized as the single JSON line the harness prints.
///
/// `frames` is the number of frames the scenario DROVE, not the number of
/// intervals `FrameClock::report` measured over — the two differ by one, and by
/// however many idle stretches the run contained. The harness owns the former;
/// only it knows what it asked for.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Report {
    pub scenario: &'static str,
    pub rows: Option<u64>,
    pub frames: Option<u64>,
    pub p50_ms: Option<f32>,
    pub p95_ms: Option<f32>,
    pub p99_ms: Option<f32>,
    pub rss_peak_bytes: Option<u64>,
    pub wall_ms: Option<f64>,
}

impl Report {
    pub fn new(scenario: &'static str) -> Self {
        Self {
            scenario,
            ..Default::default()
        }
    }

    /// Fill the percentile fields from a driven clock.
    pub fn with_clock(mut self, clock: &FrameClock) -> Self {
        if let Some((_, p50, p95, p99)) = clock.report() {
            self.p50_ms = Some(p50);
            self.p95_ms = Some(p95);
            self.p99_ms = Some(p99);
        }
        self
    }

    /// Print the one line `xtask perf` parses, then flush.
    ///
    /// `println!` alone is not enough: the harness exits via
    /// `std::process::exit`, which does not run destructors and therefore does
    /// not flush a line-buffered stdout that was redirected into a pipe — which
    /// is exactly how `xtask` invokes it.
    pub fn emit(&self) {
        use std::io::Write as _;
        let line = serde_json::to_string(self)
            .unwrap_or_else(|_| format!(r#"{{"scenario":"{}"}}"#, self.scenario));
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

/// Milliseconds since `start`, as the JSON's `wall_ms`.
pub fn wall_ms_since(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

/// `wall_ms` for the cold-launch scenario, measured from `PROCESS_START`.
///
/// `None` when the cell was never set, which can only happen if something ran
/// before `main`'s first statement — a real bug, and reporting `None` makes
/// `xtask` fail with `Missing` rather than silently record a zero.
pub fn cold_launch_wall_ms() -> Option<f64> {
    PROCESS_START.get().map(|s| wall_ms_since(*s))
}

/// Whether the real binary was launched in cold-launch measurement mode.
pub fn cold_launch_requested() -> bool {
    std::env::var_os(COLD_LAUNCH_ENV).is_some_and(|v| !v.is_empty())
}

/// Emit the cold-launch line and exit.
///
/// Called from `WorkspaceShell::render` at the end of its FIRST completed
/// frame: that is the instant the user can see something, which is what a
/// launch-time claim means. `std::process::exit` rather than a graceful
/// shutdown because a graceful shutdown would run DuckDB's close path and add
/// seconds to a measurement that has already been taken.
pub fn emit_cold_launch_and_exit() -> ! {
    let mut r = Report::new("cold_launch");
    r.wall_ms = cold_launch_wall_ms();
    r.rss_peak_bytes = crate::platform::rss_bytes();
    r.frames = Some(1);
    r.emit();
    std::process::exit(0)
}

/// Process start, set by `main.rs` as its first statement and read by the
/// `cold_launch` scenario.
///
/// `OnceLock`, not `LazyLock`: the value must be the instant *`main` began*.
/// A `LazyLock<Instant>::new(Instant::now)` would capture the instant of first
/// *access*, which is after window creation — precisely the interval being
/// measured. The initializer is genuinely runtime input, so the cell stays
/// externally `set`.
pub static PROCESS_START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Env var that puts the real `dat0` binary into cold-launch measurement mode:
/// the first completed frame prints one JSON line and exits.
pub const COLD_LAUNCH_ENV: &str = "DAT0_PERF_COLD_LAUNCH";
