//! MX1: the frame-interval recorder's arithmetic, asserted against injected
//! instants.
//!
//! Every assertion here uses `tick_at` / `fps_at` / `percentile_ms_at` rather
//! than the wall-clock wrappers. A rolling-window percentile tested against
//! `Instant::now()` is a flaky test, and a flaky test is a deleted test —
//! `tests/view_lifecycle.rs`'s `#[ignore]`d timing case is the standing example
//! in this repo.
//!
//! The clock is the instrument every later perf claim is measured with, so its
//! own arithmetic has to be the thing that is not in doubt.

use dat0_core::perf::{DriveState, FRAME_WINDOW, FrameClock, IDLE_AFTER};
use std::time::{Duration, Instant};

/// A fixed origin, so every test's instants are exactly related to each other.
fn origin() -> Instant {
    Instant::now()
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn an_empty_clock_reports_nothing() {
    let c = FrameClock::new();
    let t0 = origin();
    assert_eq!(c.fps_at(t0), None);
    assert_eq!(c.percentile_ms_at(0.5, t0), None);
    assert_eq!(c.report(), None);
}

/// One paint defines no interval, so there is nothing to report yet. This is a
/// real state — it is every window's first frame.
#[test]
fn a_single_sample_reports_nothing() {
    let mut c = FrameClock::new();
    let t0 = origin();
    c.tick_at(t0);
    assert_eq!(c.fps_at(t0), None);
    assert_eq!(c.percentile_ms_at(0.5, t0), None);
}

/// The headline arithmetic: two paints 16 ms apart is 62.5 fps.
#[test]
fn two_samples_sixteen_ms_apart_are_sixty_two_point_five_fps() {
    let mut c = FrameClock::new();
    let t0 = origin();
    c.tick_at(t0);
    c.tick_at(t0 + ms(16));
    let fps = c
        .fps_at(t0 + ms(16))
        .expect("two samples define an interval");
    assert!(
        (fps - 62.5).abs() < 0.01,
        "expected 62.5 fps, measured {fps}"
    );
}

/// gpui paints on demand, so an idle window simply stops calling `render`. A
/// rate computed across that gap would describe the user's coffee break.
#[test]
fn a_stale_newest_sample_suppresses_the_readout() {
    let mut c = FrameClock::new();
    let t0 = origin();
    c.tick_at(t0);
    c.tick_at(t0 + ms(16));
    let newest = t0 + ms(16);

    // Just inside the window still reports.
    assert!(c.fps_at(newest + IDLE_AFTER).is_some());
    // One millisecond past it does not.
    assert_eq!(c.fps_at(newest + IDLE_AFTER + ms(1)), None);
    assert_eq!(c.percentile_ms_at(0.95, newest + IDLE_AFTER + ms(1)), None);
}

/// The window is a hard bound: memory must not grow with session length.
#[test]
fn the_sample_window_is_bounded_and_evicts_oldest_first() {
    let mut c = FrameClock::new();
    let t0 = origin();
    for i in 0..(FRAME_WINDOW as u64 + 10) {
        c.tick_at(t0 + ms(i));
    }
    let newest = t0 + ms(FRAME_WINDOW as u64 + 9);
    let (intervals, ..) = c.report().expect("a full window reports");
    assert_eq!(
        intervals,
        FRAME_WINDOW - 1,
        "a capped window of N samples yields N-1 intervals"
    );

    // Oldest-first eviction: the retained span is the LAST FRAME_WINDOW ticks,
    // which are 1 ms apart, so the rate is ~1000 fps rather than something
    // dragged down by the evicted head.
    let fps = c.fps_at(newest).expect("a full window reports");
    assert!(
        (fps - 1000.0).abs() < 1.0,
        "retained span must be the newest samples; measured {fps} fps"
    );
}

/// The harness raises retention so a stall early in a long scenario cannot age
/// out before the budget is checked.
#[test]
fn set_window_raises_retention_for_the_harness() {
    let mut c = FrameClock::new();
    c.set_window(600);
    let t0 = origin();
    let mut at = t0;
    c.tick_at(at);
    for _ in 0..599 {
        at += ms(4);
        c.tick_at(at);
    }
    let (intervals, ..) = c.report().expect("reports");
    assert_eq!(
        intervals, 599,
        "600 retained samples must yield 599 intervals, not FRAME_WINDOW-1"
    );

    // A cluster of early stalls must survive to the report. Twelve of 599
    // intervals is 2%, comfortably above the p99 rank — one lone stall would
    // legitimately sit *under* p99 and prove nothing about retention.
    // Under the default 240-sample window these would all have been evicted
    // long before frame 600.
    let mut c = FrameClock::new();
    c.set_window(600);
    let mut at = origin();
    c.tick_at(at);
    for i in 0..599 {
        at += if (40..52).contains(&i) {
            ms(120)
        } else {
            ms(4)
        };
        c.tick_at(at);
    }
    let (intervals, p50, _, p99) = c.report().expect("reports");
    assert_eq!(intervals, 599);
    assert!(
        (p50 - 4.0).abs() < 0.01,
        "the typical frame is 4 ms; p50 {p50}"
    );
    assert!(
        p99 > 100.0,
        "twelve early 120 ms stalls must still be visible at frame 600; p99 was {p99}"
    );
}

/// Nearest-rank over a known ramp. Gaps are `1..=100` × 100 µs, so the p-th
/// percentile is exactly `p * 10` ms and any off-by-one in the rank
/// calculation shows up. Micros rather than millis so all 100 gaps fit inside
/// the 1 s percentile window while preserving the exact 1..=100 ratio.
#[test]
fn percentile_over_a_known_ramp_returns_the_known_value() {
    let mut c = FrameClock::new();
    let mut at = origin();
    c.tick_at(at);
    for step in 1..=100u64 {
        at += Duration::from_micros(step * 100);
        c.tick_at(at);
    }
    let p50 = c.percentile_ms_at(0.50, at).expect("ramp reports");
    let p95 = c.percentile_ms_at(0.95, at).expect("ramp reports");
    let p99 = c.percentile_ms_at(0.99, at).expect("ramp reports");
    // Gap i is `i * 100 µs` = `i / 10` ms. Nearest-rank p50 of 100 samples is
    // the 50th smallest = 5.0 ms; p95 = 9.5 ms; p99 = 9.9 ms.
    assert!((p50 - 5.0).abs() < 0.01, "p50 was {p50}");
    assert!((p95 - 9.5).abs() < 0.01, "p95 was {p95}");
    assert!((p99 - 9.9).abs() < 0.01, "p99 was {p99}");
}

/// `percentile_ms(1.0)` is the worst frame and `(0.0)` the best — the two ends
/// a HUD reader will check first.
#[test]
fn percentile_bounds_are_the_extremes() {
    let mut c = FrameClock::new();
    let t0 = origin();
    let mut at = t0;
    c.tick_at(at);
    for gap in [4u64, 9, 2, 40, 7] {
        at += ms(gap);
        c.tick_at(at);
    }
    let best = c.percentile_ms_at(0.0, at).expect("reports");
    let worst = c.percentile_ms_at(1.0, at).expect("reports");
    assert!((best - 2.0).abs() < 0.01, "min gap was {best}");
    assert!((worst - 40.0).abs() < 0.01, "max gap was {worst}");
}

/// Two independent exclusions, asserted together because together they are the
/// reason the HUD can be trusted.
///
/// The 200 ms stall is real but old — the 1 s percentile window drops it. The
/// 2 s pause is *not a frame at all*: gpui paints on demand, so the first paint
/// after an idle stretch would otherwise enter the sample set as a 2000 ms
/// "frame" and put p99 two orders of magnitude off. That second case is a bug
/// this test caught in the first implementation.
#[test]
fn percentile_ignores_old_gaps_and_idle_stretches() {
    let mut c = FrameClock::new();
    let t0 = origin();
    // An ancient 200 ms stall…
    c.tick_at(t0);
    c.tick_at(t0 + ms(200));
    // …then a two-second quiet gap, then a run of clean 5 ms frames.
    let mut at = t0 + ms(2_200);
    c.tick_at(at);
    for _ in 0..20 {
        at += ms(5);
        c.tick_at(at);
    }
    let worst = c.percentile_ms_at(1.0, at).expect("reports");
    assert!(
        worst < 100.0,
        "worst in-window interval was {worst} ms; the 200 ms stall is outside \
         the 1 s percentile window and the 2 s pause is an idle stretch, not a frame"
    );

    // And the idle stretch must stay excluded even from the unbounded `report`,
    // which has no recency cutoff to hide behind.
    let (_, _, _, p99) = c.report().expect("reports");
    assert!(
        p99 < 500.0,
        "an idle stretch must never be counted as a frame interval; p99 was {p99}"
    );

    // fps must survive it too: 5 ms frames are 200 fps, and an idle-inflated
    // mean would report single digits.
    let fps = c.fps_at(at).expect("reports");
    assert!(
        fps > 150.0,
        "fps must be computed from real intervals only; measured {fps}"
    );
}

/// `report` anchors on the newest sample rather than the wall clock, so a
/// harness reading it a moment after the last driven frame is not suppressed
/// by the idle cutoff.
#[test]
fn report_is_not_suppressed_by_wall_clock_idleness() {
    let mut c = FrameClock::new();
    let t0 = origin();
    let mut at = t0;
    c.tick_at(at);
    for _ in 0..10 {
        at += ms(8);
        c.tick_at(at);
    }
    // Real time has moved well past IDLE_AFTER by the time this assertion runs
    // in a loaded test binary; `report` must still produce numbers.
    let (intervals, p50, p95, p99) = c.report().expect("report anchors on the newest sample");
    assert_eq!(intervals, 10, "11 samples define 10 intervals");
    assert!((p50 - 8.0).abs() < 0.01, "p50 was {p50}");
    assert!(p95 >= p50 && p99 >= p95, "percentiles must be monotonic");
}

/// The drive slot is what keeps a shipped window from spinning: it is `None`
/// unless a perf scenario set it.
#[test]
fn drive_state_is_absent_until_set_and_is_mutable_in_place() {
    let mut c = FrameClock::new();
    assert!(c.drive_state().is_none(), "a normal window never drives");

    c.drive(Some(DriveState {
        next_row: 0,
        frames_left: 2,
    }));
    let state = c.drive_state().expect("just set");
    state.frames_left -= 1;
    state.next_row = 4096;

    let state = c.drive_state().expect("still set");
    assert_eq!(state.frames_left, 1, "mutation must persist in place");
    assert_eq!(state.next_row, 4096);

    c.drive(None);
    assert!(c.drive_state().is_none());
}
