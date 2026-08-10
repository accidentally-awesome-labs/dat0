//! MX2: the perf gate's decision table, one test per row.
//!
//! `evaluate` is pure on purpose. The alternative — asserting the gate only by
//! running a real 1440x900 window — would make the contract observable exactly
//! once per two-minute build, on one machine, and the regression arm would
//! never be exercised at all because a dev box always has a baseline.

use xtask::perf::{Budget, HostEntry, Measurement, REGRESSION_FACTOR, Verdict, evaluate};

fn budget(metric: &str, max: f64) -> Budget {
    Budget {
        metric: metric.to_string(),
        max,
    }
}

fn measurement(scenario: &str) -> Measurement {
    Measurement {
        scenario: scenario.to_string(),
        ..Default::default()
    }
}

fn host_with(scenario: &str, metric: &str, value: f64) -> HostEntry {
    let mut h = HostEntry::default();
    h.scenarios
        .entry(scenario.to_string())
        .or_default()
        .insert(metric.to_string(), value);
    h
}

/// Row 1: over the absolute budget → fail, regardless of any baseline.
#[test]
fn a_budget_breach_fails() {
    let mut m = measurement("scroll_1m");
    m.p95_ms = Some(20.0);
    let v = evaluate(&budget("p95_ms", 16.67), None, &m);
    assert!(matches!(v, Verdict::FailBudget { .. }), "{v:?}");
    assert!(v.is_failure());
}

/// The budget arm must win even when the host's own recorded value is worse.
/// Otherwise a host that was already over budget would report a comfortable
/// no-regression pass forever.
#[test]
fn the_budget_is_checked_before_the_regression() {
    let mut m = measurement("scroll_1m");
    m.p95_ms = Some(20.0);
    let host = host_with("scroll_1m", "p95_ms", 25.0); // baseline already over
    let v = evaluate(&budget("p95_ms", 16.67), Some(&host), &m);
    assert!(
        matches!(v, Verdict::FailBudget { .. }),
        "an over-budget host baseline must not launder an over-budget run: {v:?}"
    );
}

/// Row 2: within budget but more than REGRESSION_FACTOR worse than recorded.
#[test]
fn a_regression_against_this_hosts_baseline_fails() {
    let mut m = measurement("scroll_1m");
    m.p95_ms = Some(10.0);
    let host = host_with("scroll_1m", "p95_ms", 8.0); // 10.0 > 8.0 * 1.20 = 9.6
    let v = evaluate(&budget("p95_ms", 16.67), Some(&host), &m);
    match v {
        Verdict::FailRegression { got, recorded, .. } => {
            assert_eq!(got, 10.0);
            assert_eq!(recorded, 8.0);
        }
        other => panic!("expected FailRegression, got {other:?}"),
    }
}

/// Exactly at the tolerance is a pass — the boundary must not be a coin flip.
#[test]
fn the_regression_tolerance_boundary_passes() {
    let mut m = measurement("scroll_1m");
    m.p95_ms = Some(8.0 * REGRESSION_FACTOR);
    let host = host_with("scroll_1m", "p95_ms", 8.0);
    let v = evaluate(&budget("p95_ms", 16.67), Some(&host), &m);
    assert!(matches!(v, Verdict::Pass { .. }), "{v:?}");
}

/// Row 3: no entry for this host → record, do not compare. This is what keeps
/// a new machine (and CI's first run) from being red on first contact.
#[test]
fn an_unknown_host_records_rather_than_failing() {
    let mut m = measurement("scroll_1m");
    m.p95_ms = Some(9.9);
    let v = evaluate(&budget("p95_ms", 16.67), None, &m);
    assert!(matches!(v, Verdict::Recorded { .. }), "{v:?}");
    assert!(!v.is_failure());
}

/// A host that exists but has never recorded THIS scenario is still unknown for
/// this comparison — adding a scenario must not make every host red.
#[test]
fn a_known_host_missing_this_scenario_records() {
    let mut m = measurement("scroll_10m");
    m.p95_ms = Some(12.0);
    let host = host_with("scroll_1m", "p95_ms", 4.0);
    let v = evaluate(&budget("p95_ms", 16.67), Some(&host), &m);
    assert!(matches!(v, Verdict::Recorded { .. }), "{v:?}");
}

/// A harness that ran but reported nothing for the budgeted metric is a bug,
/// and must not be indistinguishable from a pass.
#[test]
fn a_missing_metric_is_a_failure_not_a_pass() {
    let m = measurement("cold_launch"); // wall_ms is None
    let v = evaluate(&budget("wall_ms", 1000.0), None, &m);
    assert!(matches!(v, Verdict::Missing { .. }), "{v:?}");
    assert!(v.is_failure());
}

/// `rss_peak_bytes` is an integer on the wire but compared as a float; the
/// conversion must not silently drop the metric.
#[test]
fn integer_metrics_are_comparable() {
    let mut m = measurement("idle_rss");
    m.rss_peak_bytes = Some(150 * 1024 * 1024);
    let v = evaluate(&budget("rss_peak_bytes", 209_715_200.0), None, &m);
    assert!(matches!(v, Verdict::Recorded { .. }), "{v:?}");

    m.rss_peak_bytes = Some(300 * 1024 * 1024);
    let v = evaluate(&budget("rss_peak_bytes", 209_715_200.0), None, &m);
    assert!(matches!(v, Verdict::FailBudget { .. }), "{v:?}");
}

/// The harness prints one JSON line, but a real window also emits `tracing`
/// output on the way up. The parser must find the measurement anyway.
#[test]
fn the_measurement_line_is_found_among_log_noise() {
    let stdout = concat!(
        "2026-08-08T18:00:45Z  INFO telemetry submission disabled (opt-in off)\n",
        "2026-08-08T18:00:45Z  INFO dat0 starting\n",
        r#"{"scenario":"scroll_1m","rows":1000000,"frames":600,"p50_ms":4.1,"p95_ms":9.7,"p99_ms":14.2,"rss_peak_bytes":734003200,"wall_ms":null}"#,
        "\n",
    );
    let m = xtask::perf::parse_measurement(stdout).expect("measurement line");
    assert_eq!(m.scenario, "scroll_1m");
    assert_eq!(m.frames, Some(600));
    assert_eq!(m.p95_ms, Some(9.7));
    assert_eq!(m.wall_ms, None);
    assert_eq!(m.metric("p95_ms"), Some(9.7));
}

/// No JSON at all is how a scenario signals a missing fixture.
#[test]
fn stdout_with_no_json_yields_no_measurement() {
    assert!(xtask::perf::parse_measurement("nothing here\n").is_none());
    assert!(xtask::perf::parse_measurement("").is_none());
    // A JSON object that is not a measurement must not be mistaken for one.
    assert!(xtask::perf::parse_measurement(r#"{"hello":"world"}"#).is_none());
}

/// The committed baselines file is what the gate reads on every run; a typo in
/// it would disable the gate silently.
#[test]
fn the_committed_baselines_cover_every_scenario() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let b = xtask::perf::load_baselines(root).expect("baselines parse");
    assert_eq!(b.schema, 1);
    for s in xtask::perf::SCENARIOS {
        let budget = b
            .budgets
            .get(*s)
            .unwrap_or_else(|| panic!("no budget for scenario {s}"));
        assert!(
            budget.max > 0.0,
            "{s}: budget max must be positive, got {}",
            budget.max
        );
        // A budget naming a metric `Measurement` cannot produce would make
        // every run report `Missing` — a gate that only ever fails is as
        // useless as one that only ever passes.
        let probe = Measurement {
            p50_ms: Some(1.0),
            p95_ms: Some(1.0),
            p99_ms: Some(1.0),
            wall_ms: Some(1.0),
            rss_peak_bytes: Some(1),
            ..Default::default()
        };
        assert!(
            probe.metric(&budget.metric).is_some(),
            "{s}: budget names unknown metric {:?}",
            budget.metric
        );
    }
}

/// The two frame-rate budgets are the 60 fps claim, stated once.
#[test]
fn the_scroll_budgets_are_one_frame_at_sixty_hz() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root");
    let b = xtask::perf::load_baselines(root).expect("baselines parse");
    for s in ["scroll_1m", "scroll_10m"] {
        let budget = &b.budgets[s];
        assert_eq!(budget.metric, "p95_ms");
        assert!(
            (budget.max - 1000.0 / 60.0).abs() < 0.01,
            "{s} budget {} is not one 60 Hz frame",
            budget.max
        );
    }
}
