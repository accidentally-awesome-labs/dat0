//! MX2/MX3: the perf gate.
//!
//! Runs each scenario as a subprocess, parses the one JSON line it prints, and
//! compares the measurement against two independent things:
//!
//! - **The budget** — the numbers `docs/specs/2026-04-26-dat0-design.md` commits
//!   to and the marketing page repeats. Absolute, host-independent, and the only
//!   arm that can fail on a machine nobody has recorded.
//! - **The recorded baseline for this host** — a regression tripwire that only
//!   engages once someone has committed that host's numbers. A new machine
//!   (including CI's first run) records rather than fails, because a red gate on
//!   first contact is a gate people delete.
//!
//! The host key is deliberately not just OS+ARCH: a virtualized CI runner and an
//! M1 Max are both `macos-aarch64` and comparing one against the other would
//! produce noise indistinguishable from a real regression.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

/// Every scenario `xtask perf` knows how to run, in default execution order.
pub const SCENARIOS: &[&str] = &[
    "scroll_1m",
    "scroll_10m",
    "open_csv_10gb",
    "open_parquet_1gb",
    "cold_launch",
    "idle_rss",
];

/// Where the committed budgets and per-host baselines live.
pub const BASELINE_PATH: &str = "docs/internal/perf-baselines.json";

/// The one metric a scenario is judged on. Named so the JSON stays readable and
/// a typo is a parse error rather than a silently-skipped comparison.
/// `Eq` is deliberately absent: `max` is an `f64` and does not implement it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Budget {
    pub metric: String,
    pub max: f64,
}

/// One host's recorded run. Extra keys are the per-scenario metric maps.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct HostEntry {
    pub recorded: String,
    pub rustc: String,
    #[serde(flatten)]
    pub scenarios: BTreeMap<String, BTreeMap<String, f64>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Baselines {
    pub schema: u32,
    pub budgets: BTreeMap<String, Budget>,
    #[serde(default)]
    pub hosts: BTreeMap<String, HostEntry>,
}

/// One scenario's measured output — exactly the JSON line the harness prints.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Measurement {
    pub scenario: String,
    #[serde(default)]
    pub rows: Option<u64>,
    #[serde(default)]
    pub frames: Option<u64>,
    #[serde(default)]
    pub p50_ms: Option<f64>,
    #[serde(default)]
    pub p95_ms: Option<f64>,
    #[serde(default)]
    pub p99_ms: Option<f64>,
    #[serde(default)]
    pub rss_peak_bytes: Option<u64>,
    #[serde(default)]
    pub wall_ms: Option<f64>,
}

impl Measurement {
    /// The value a budget names, or `None` when the harness did not report it.
    pub fn metric(&self, name: &str) -> Option<f64> {
        match name {
            "p50_ms" => self.p50_ms,
            "p95_ms" => self.p95_ms,
            "p99_ms" => self.p99_ms,
            "wall_ms" => self.wall_ms,
            "rss_peak_bytes" => self.rss_peak_bytes.map(|b| b as f64),
            "frames" => self.frames.map(|f| f as f64),
            _ => None,
        }
    }
}

/// How much worse than the recorded baseline counts as a regression.
///
/// 1.20 rather than something tighter because frame timing on a shared machine
/// genuinely varies by a few percent between runs, and a gate that fires on
/// noise gets disabled within a week.
pub const REGRESSION_FACTOR: f64 = 1.20;

/// The whole `--check` contract. One variant per row of MX2's decision table.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Measurement breaches the absolute budget. Exit 1.
    FailBudget { metric: String, got: f64, max: f64 },
    /// Measurement is more than [`REGRESSION_FACTOR`] worse than this host's
    /// recorded value. Exit 1.
    FailRegression {
        metric: String,
        got: f64,
        recorded: f64,
    },
    /// Host key absent from `hosts`: print the value, do not compare. Exit 0.
    Recorded { metric: String, got: f64 },
    /// Within budget and within tolerance of the recorded value. Exit 0.
    Pass { metric: String, got: f64 },
    /// Fixture missing, so the scenario did not run. Exit 0.
    Skipped { reason: String },
    /// The scenario ran but did not report the metric its budget names — a
    /// harness bug, not a perf result, and it must not pass silently. Exit 1.
    Missing { metric: String },
}

impl Verdict {
    pub fn is_failure(&self) -> bool {
        matches!(
            self,
            Verdict::FailBudget { .. } | Verdict::FailRegression { .. } | Verdict::Missing { .. }
        )
    }
}

/// Judge one measurement. Pure, so every row of the decision table is a unit
/// test rather than something only observable by running a real window.
///
/// Order matters: the absolute budget is checked BEFORE the per-host
/// regression. A host whose recorded baseline is already over budget must keep
/// failing on the budget, not report a comfortable no-regression pass.
pub fn evaluate(budget: &Budget, host: Option<&HostEntry>, m: &Measurement) -> Verdict {
    let Some(got) = m.metric(&budget.metric) else {
        return Verdict::Missing {
            metric: budget.metric.clone(),
        };
    };
    if got > budget.max {
        return Verdict::FailBudget {
            metric: budget.metric.clone(),
            got,
            max: budget.max,
        };
    }
    let recorded = host
        .and_then(|h| h.scenarios.get(&m.scenario))
        .and_then(|s| s.get(&budget.metric))
        .copied();
    match recorded {
        None => Verdict::Recorded {
            metric: budget.metric.clone(),
            got,
        },
        Some(base) if got > base * REGRESSION_FACTOR => Verdict::FailRegression {
            metric: budget.metric.clone(),
            got,
            recorded: base,
        },
        Some(_) => Verdict::Pass {
            metric: budget.metric.clone(),
            got,
        },
    }
}

/// This machine's baseline key.
///
/// `$DAT0_PERF_HOST` distinguishes a virtualized runner from a dev box; without
/// it every `macos-aarch64` machine would share one entry and CI would be
/// compared against hardware it has nothing in common with. CI sets
/// `DAT0_PERF_HOST=ci-hosted`.
pub fn host_key() -> String {
    let id = std::env::var("DAT0_PERF_HOST").unwrap_or_else(|_| "dev".to_string());
    format!("{}-{}-{}", std::env::consts::OS, std::env::consts::ARCH, id)
}

pub fn load_baselines(root: &Path) -> Result<Baselines> {
    let path = root.join(BASELINE_PATH);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("read baselines {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse baselines {}", path.display()))
}

/// Extract the single JSON object line from a scenario's stdout.
///
/// Scoped to the LAST line that parses rather than the last line outright: a
/// real window emits `tracing` output on the way up, and a harness that
/// demanded clean stdout would break the first time someone raised a log level.
pub fn parse_measurement(stdout: &str) -> Option<Measurement> {
    stdout
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Measurement>(l.trim()).ok())
        .find(|m| !m.scenario.is_empty())
}

/// Run one scenario, returning `(measurement, stderr)`.
///
/// `None` means the harness declined to measure and said why on stderr — a
/// missing fixture, or a process with no display link. It signals that by
/// exiting 0 with no JSON line, so the two cases share one path here and the
/// reason is forwarded rather than guessed at.
fn run_scenario(root: &Path, scenario: &str) -> Result<(Option<Measurement>, String)> {
    let mut cmd = if scenario == "cold_launch" {
        // Deliberately the REAL binary: `cold_launch` is what a user
        // double-clicks, and an example with a different link line, different
        // features and no `main.rs` would measure something else.
        let bin = root.join("target/release/dat0");
        anyhow::ensure!(
            bin.exists(),
            "cold_launch needs {} — build with `cargo build --release` in crates/dat0-ui",
            bin.display()
        );
        let mut c = Command::new(bin);
        c.env("DAT0_PERF_COLD_LAUNCH", "1");
        c
    } else {
        let bin = root.join("target/release/examples/perf_harness");
        anyhow::ensure!(
            bin.exists(),
            "missing {} — build with `cargo build --release --features perf-harness \
             --example perf_harness` in crates/dat0-ui",
            bin.display()
        );
        let mut c = Command::new(bin);
        c.arg(scenario);
        c
    };
    let out = cmd
        .output()
        .with_context(|| format!("run perf scenario {scenario}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        anyhow::bail!("scenario {scenario} exited {}: {stderr}", out.status);
    }
    Ok((parse_measurement(&stdout), stderr))
}

/// The harness's own one-line explanation for declining to measure.
///
/// It prints `SKIP <scenario>: <reason>` to stderr. Echoing that verbatim beats
/// `xtask` inventing a cause it cannot know — the old text said "fixture
/// missing" for a window that had no vsync, which sent the reader looking in
/// entirely the wrong place.
fn skip_reason(scenario: &str, stderr: &str) -> String {
    stderr
        .lines()
        .find(|l| l.starts_with("SKIP "))
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| format!("SKIP {scenario}: no measurement emitted"))
}

/// Append a line to `$GITHUB_STEP_SUMMARY` when running under Actions.
fn step_summary(line: &str) {
    if let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY") {
        use std::io::Write as _;
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

pub struct Options {
    pub scenarios: Vec<String>,
    pub check: bool,
    pub update_baseline: bool,
}

pub fn run(opts: Options) -> Result<i32> {
    let root = workspace_root();
    let scenarios = if opts.scenarios.is_empty() {
        SCENARIOS.iter().map(|s| (*s).to_string()).collect()
    } else {
        opts.scenarios.clone()
    };
    for s in &scenarios {
        anyhow::ensure!(
            SCENARIOS.contains(&s.as_str()),
            "unknown scenario {s}; known: {}",
            SCENARIOS.join(", ")
        );
    }

    let mut baselines = load_baselines(&root)?;
    let key = host_key();
    let mut failed = false;
    let mut measured: Vec<Measurement> = Vec::new();

    for scenario in &scenarios {
        let (measured_one, stderr) = run_scenario(&root, scenario)?;
        let Some(m) = measured_one else {
            let line = skip_reason(scenario, &stderr);
            println!("{line}");
            step_summary(&line);
            continue;
        };
        println!(
            "{}",
            serde_json::to_string(&m)
                .unwrap_or_else(|_| format!("{{\"scenario\":\"{scenario}\"}}"))
        );

        if opts.check {
            let Some(budget) = baselines.budgets.get(scenario) else {
                anyhow::bail!("no budget for scenario {scenario} in {BASELINE_PATH}");
            };
            let verdict = evaluate(budget, baselines.hosts.get(&key), &m);
            let line = describe(scenario, &verdict);
            println!("{line}");
            step_summary(&line);
            failed |= verdict.is_failure();
        }
        measured.push(m);
    }

    if opts.update_baseline {
        let entry = baselines.hosts.entry(key.clone()).or_default();
        entry.recorded = now_iso();
        entry.rustc = rustc_version();
        for m in &measured {
            let slot = entry.scenarios.entry(m.scenario.clone()).or_default();
            for metric in ["p50_ms", "p95_ms", "p99_ms", "wall_ms", "rss_peak_bytes"] {
                if let Some(v) = m.metric(metric) {
                    slot.insert(metric.to_string(), v);
                }
            }
        }
        let path = root.join(BASELINE_PATH);
        let json = serde_json::to_string_pretty(&baselines)?;
        std::fs::write(&path, format!("{json}\n"))
            .with_context(|| format!("write {}", path.display()))?;
        println!("updated baseline for {key} in {BASELINE_PATH}");
    }

    Ok(i32::from(failed))
}

pub fn describe(scenario: &str, v: &Verdict) -> String {
    match v {
        Verdict::FailBudget { metric, got, max } => {
            format!("FAIL {scenario}: {metric} {got:.2} over budget {max:.2}")
        }
        Verdict::FailRegression {
            metric,
            got,
            recorded,
        } => format!(
            "FAIL {scenario}: {metric} {got:.2} is >{:.0}% worse than the recorded {recorded:.2}",
            (REGRESSION_FACTOR - 1.0) * 100.0
        ),
        Verdict::Recorded { metric, got } => {
            format!("RECORD {scenario}: {metric} {got:.2} (no baseline for this host)")
        }
        Verdict::Pass { metric, got } => format!("PASS {scenario}: {metric} {got:.2}"),
        Verdict::Skipped { reason } => format!("SKIP {scenario}: {reason}"),
        Verdict::Missing { metric } => {
            format!("FAIL {scenario}: harness reported no {metric}")
        }
    }
}

fn workspace_root() -> PathBuf {
    // `xtask` lives one level under the workspace root, and cargo runs it from
    // the root, so the manifest dir's parent is the root either way.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn now_iso() -> String {
    // No chrono in xtask; seconds-since-epoch is unambiguous and this field is
    // provenance, not something anything parses.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("epoch:{secs}")
}

fn rustc_version() -> String {
    Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
