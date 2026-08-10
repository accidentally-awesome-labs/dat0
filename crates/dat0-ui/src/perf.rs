//! The real-window perf harness.
//!
//! Lives in the library, not in `examples/perf_harness.rs`, so it is reachable
//! from a test — an example body is unreachable from any test and rots unseen.
//! The scenario list, the budgets and the fixtures are
//! [`dat0_core::perf::harness`]; only the part that needs a window is here.
//!
//! ## What it measures
//!
//! A real window with the real [`Grid`](crate::components::grid::Grid) over a
//! real DuckDB table, so the numbers include the DOM, the LRU page cache and
//! the engine — the whole stack a user's scroll goes through.
//!
//! The measurement is the one the Phase 0 spike validated (`examples/grid_spike.rs`),
//! and it is deliberately not the obvious one. Two naive framings both
//! degenerate:
//!
//! * Timing an unqualified `MutationObserver` reports ~0 ms, because the
//!   mutation for scroll N lands just after scroll N+1's timestamp — it
//!   measures the wrong pair.
//! * Waiting for the DOM to *equal* the newest scroll position never settles
//!   while scrolling, because the target keeps moving.
//!
//! So the canvas is stamped with the exact `scrollTop` its current DOM was
//! rendered for, and JS keeps a `scrollTop -> timestamp` table. That yields
//! **scroll-to-repaint** per distinct DOM state, which is what the budget is
//! written against.
//!
//! Timers, not `requestAnimationFrame`, drive the scroll: rAF fires once in a
//! window that is not compositing (measured — see the migration log), and a
//! harness that depended on it would hang on an unfocused desktop.

use std::sync::Arc;

use anyhow::{Context, Result};
use dioxus::prelude::*;

use dat0_core::perf::harness::{
    IDLE_SETTLE_SECS, SCROLL_FRAMES, Scenario, ensure_scroll_fixture, optional_large_fixture,
    spawn_watchdog,
};
use dat0_core::session::Session;

use crate::components::grid::Grid;
use crate::state::Workspace;
use crate::theme::{Theme, ThemeStyle};

/// One scenario, run to completion. Prints one JSON line and exits.
pub fn run_windowed(scenario: Scenario) -> Result<()> {
    spawn_watchdog(scenario.name());

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("perf harness tokio runtime")?;
    let _guard = rt.enter();

    // A scenario whose fixture is absent SKIPS: exit 0, print the reason, emit
    // no JSON line — which is exactly how `xtask perf` recognises a skip.
    //
    // The alternative is what this used to do by accident: open the window
    // anyway and report `wall_ms: 0`, which sails through a 30 s budget and
    // reads in the log as a pass. A gate that cannot fail is worse than no
    // gate, because it is believed.
    if scenario.needs_fixture() && fixture_for(scenario)?.is_none() {
        eprintln!(
            "SKIP {}: no pre-generated fixture under tests/fixtures/large/.\n\
             The multi-gigabyte scenarios deliberately do not generate their own \
             input — writing a 10 GB CSV would look like a hang.",
            scenario.name()
        );
        return Ok(());
    }

    let source = rt.block_on(bind_source(scenario))?;

    dioxus::LaunchBuilder::desktop()
        .with_cfg(crate::launch::config())
        .with_context(Bound {
            scenario,
            source: source.clone(),
        })
        .launch(Harness);

    Ok(())
}

/// What the window is measuring, handed to the root component.
#[derive(Clone)]
struct Bound {
    scenario: Scenario,
    source: Option<Arc<dat0_core::grid::data_source::GridDataSource>>,
}

/// Open a real session and register the scenario's fixture through the same
/// engine call a file drop uses, so the measured open includes DuckDB's sniff
/// and scan.
///
/// `None` for the scenarios that measure something other than a table:
/// `idle_rss` has no data by definition, and the two multi-gigabyte openers
/// skip when their pre-generated fixture is absent rather than spending
/// minutes writing one nobody asked for.
async fn bind_source(
    scenario: Scenario,
) -> Result<Option<Arc<dat0_core::grid::data_source::GridDataSource>>> {
    let Some(path) = fixture_for(scenario)? else {
        return Ok(None);
    };

    // A real `Session`, not a bare engine: the harness exists to measure the
    // stack the user's scroll goes through, and the session is what supplies
    // the engine handle, its budget and its scratch directory.
    let state_root = dat0_core::perf::harness::fixture_dir().join("state");
    std::fs::create_dir_all(&state_root)
        .with_context(|| format!("create {}", state_root.display()))?;
    let session = Session::new(
        &state_root,
        dat0_core::perf::harness::HARNESS_MEMORY_BUDGET_BYTES,
    )
    .await
    .context("perf harness session")?;
    let engine = session.engine.clone();

    let info = dat0_engine::QueryEngine::register_file_as_table(
        engine.as_ref(),
        &path,
        dat0_engine::types::RegisterOpts::default(),
    )
    .await
    .with_context(|| format!("register {}", path.display()))?;

    let source = dat0_core::grid::data_source::GridDataSource::new(engine, info.name)
        .await
        .context("bind the grid to the fixture")?;
    Ok(Some(Arc::new(source)))
}

fn fixture_for(scenario: Scenario) -> Result<Option<std::path::PathBuf>> {
    Ok(match scenario {
        Scenario::Scroll1M | Scenario::Scroll10M => {
            let rows = scenario.rows().expect("a scroll scenario has rows");
            Some(ensure_scroll_fixture(rows)?)
        }
        Scenario::OpenCsv10Gb => optional_large_fixture("generated.csv"),
        Scenario::OpenParquet1Gb => optional_large_fixture("generated.parquet"),
        Scenario::IdleRss => None,
    })
}

#[component]
fn Harness() -> Element {
    let bound = use_context::<Bound>();
    Theme::provide(None);
    Workspace::provide();
    dioxus::desktop::use_asset_handler("dat0", crate::protocol::serve);

    let scenario = bound.scenario;
    let source = bound.source.clone();

    // The measurement, started once the tree is mounted. Every scenario ends by
    // printing one JSON line and exiting the process — `xtask perf` reads
    // stdout, so a harness that lingered would look like a hang.
    use_future(move || async move {
        match scenario {
            Scenario::Scroll1M | Scenario::Scroll10M => drive_scroll(scenario).await,
            Scenario::IdleRss => idle_rss(scenario).await,
            Scenario::OpenCsv10Gb | Scenario::OpenParquet1Gb => open_wall(scenario).await,
        }
    });

    let Some(source) = source else {
        // A scenario with no table still needs a mounted tree: `idle_rss`
        // measures the resident set of a *running window*, not of a process
        // that never opened one.
        return rsx! {
            ThemeStyle {}
            div { class: "d0-window", "data-a11y-id": "perf-idle" }
        };
    };

    rsx! {
        ThemeStyle {}
        div { class: "d0-window",
            Grid {
                source: source.clone(),
                columns: Vec::new(),
                // Sized from the bound table: `SelectionModel::new` clamps
                // `move_active` against these, and it has no `Default` because
                // a selection over a zero-by-zero grid is not a thing.
                selection: Signal::new(dat0_core::grid::selection::SelectionModel::new(
                    usize::try_from(source.row_count).unwrap_or(usize::MAX).max(1),
                    source.visible_column_names().len().max(1),
                )),
                widths: Signal::new(Vec::new()),
            }
        }
    }
}

/// Scroll the grid on a timer and report scroll-to-repaint percentiles.
async fn drive_scroll(scenario: Scenario) {
    let script = DRIVER_JS.replace("FRAMES", &SCROLL_FRAMES.to_string());
    let mut eval = document::eval(&script);
    loop {
        match eval.recv::<serde_json::Value>().await {
            Ok(v) if v.get("alive").is_some() => {
                dat0_core::perf::harness::DRIVER_ALIVE
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            // A mounted window whose grid never appeared is a bug in the
            // harness, not an unmeasurable environment — loud, and a distinct
            // exit code from the watchdog's skip.
            Ok(v) if v.get("error").is_some() => {
                eprintln!("perf: {}", v["error"]);
                std::process::exit(3);
            }
            Ok(v) => emit(scenario, v),
            Err(e) => {
                eprintln!("perf: driver failed: {e}");
                std::process::exit(2);
            }
        }
    }
}

/// Let the window settle, then sample the resident set.
async fn idle_rss(scenario: Scenario) {
    dat0_core::perf::harness::DRIVER_ALIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    tokio::time::sleep(std::time::Duration::from_secs(IDLE_SETTLE_SECS)).await;
    let rss = dat0_core::platform::rss_bytes().unwrap_or(0);
    emit(scenario, serde_json::json!({ "rss_peak_bytes": rss }));
}

/// Report how long the open took. The work already happened in `bind_source`,
/// before the window existed, which is the honest place to measure it: a user's
/// "open" is over when the first rows are on screen, and the engine's
/// registration dominates.
async fn open_wall(scenario: Scenario) {
    dat0_core::perf::harness::DRIVER_ALIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    let wall = dat0_core::perf::PROCESS_START
        .get()
        .map(|s| s.elapsed().as_millis() as u64)
        .unwrap_or(0);
    emit(scenario, serde_json::json!({ "wall_ms": wall }));
}

/// One JSON line on stdout, then exit. `xtask perf` parses this.
fn emit(scenario: Scenario, mut body: serde_json::Value) {
    if let Some(obj) = body.as_object_mut() {
        obj.insert("scenario".into(), scenario.name().into());
        if let Some(rows) = scenario.rows() {
            obj.insert("rows".into(), (rows as u64).into());
        }
        obj.entry("rss_peak_bytes")
            .or_insert_with(|| dat0_core::platform::rss_bytes().unwrap_or(0).into());
    }
    println!("{body}");
    std::process::exit(0);
}

/// The driver + probe, running in the webview so every timestamp shares one
/// clock. See the module docs for why the correlation is through `data-top`.
const DRIVER_JS: &str = r#"
// Report for duty before anything else: the watchdog reads this to tell "no
// display server, nothing to measure" (skip, exit 0) from "the driver ran and
// never finished" (a bug, exit 3).
dioxus.send({ alive: true });

const vp = document.querySelector('[data-a11y-id="grid-viewport"]');
const canvas = document.querySelector(".d0-grid-canvas");
if (!vp || !canvas) {
  dioxus.send({ error: "grid did not mount" });
} else {
  const stamps = new Map();
  const order = [];
  let scrollEvents = 0;

  vp.addEventListener("scroll", (e) => {
    scrollEvents++;
    const top = e.target.scrollTop;
    if (!stamps.has(top)) order.push(top);
    stamps.set(top, performance.now());
    while (order.length > 512) stamps.delete(order.shift());
  }, { passive: true });

  const latency = [];
  let lastDomTop = NaN;

  // A timer, not requestAnimationFrame: rAF fires once in a window that is not
  // compositing, which is the normal state for an unfocused harness.
  const sampler = setInterval(() => {
    const domTop = parseFloat(canvas.dataset.top);
    if (Number.isFinite(domTop) && domTop !== lastDomTop) {
      lastDomTop = domTop;
      const t = stamps.get(domTop);
      if (t !== undefined) latency.push(performance.now() - t);
    }
  }, 4);

  const started = performance.now();
  const scroller = setInterval(() => { vp.scrollTop += 40; }, 1000 / 120);

  setTimeout(() => {
    clearInterval(scroller);
    clearInterval(sampler);
    latency.sort((a, b) => a - b);
    const q = (p) => latency.length
      ? latency[Math.min(latency.length - 1, Math.floor(latency.length * p))]
      : -1;
    // Key names are `xtask::perf::Measurement`'s field names, not ours: a
    // budget names `p95_ms`, and a harness that emitted `p95` would be read as
    // "reported no such metric" and skipped by `xtask perf --check`.
    dioxus.send({
      elapsed_ms: performance.now() - started,
      scroll_events: scrollEvents,
      frames: latency.length,
      p50_ms: q(0.5),
      p95_ms: q(0.95),
      p99_ms: q(0.99),
      max_ms: latency.length ? latency[latency.length - 1] : -1,
    });
  }, (FRAMES / 60) * 1000);
}
await new Promise((r) => setTimeout(r, 0));
"#;
