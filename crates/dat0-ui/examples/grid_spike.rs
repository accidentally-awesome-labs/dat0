//! Phase 0.1 — grid throughput spike.
//!
//! The gate on the whole GPUI -> Dioxus migration: can a `dioxus-desktop`
//! WebView carry dat0's virtualized grid at interactive frame rates, given that
//! DOM edits cross a localhost WebSocket and `WebviewInstance::edits_in_progress`
//! blocks the VirtualDom until the flush is acked?
//!
//! Structure is the shipped grid's, not a toy:
//!   - 1,000,000 rows x 40 columns of synthetic text (mixed ints/floats/words/
//!     bools/dates, so cell formatting cost is real),
//!   - `div.viewport[overflow:auto]` -> `div.canvas[position:relative]` sized to
//!     the full virtual extent,
//!   - visible range computed in Rust from `ScrollData`, 4 rows / 2 columns of
//!     overscan,
//!   - one absolutely positioned `div` per visible row, one per visible cell,
//!     both keyed by absolute index,
//!   - the real Design-system styling: Geist Mono 12.5px served over the custom
//!     asset protocol, 26px rows, `0 8px` cell padding, a 1px row rule, hover
//!     tint, and a 40-cell selection block so the selection path is measured.
//!
//! Measurement is done in JS so there is no Rust/JS clock skew: a `scroll`
//! listener stamps the first unserviced scroll, a `MutationObserver` on the
//! canvas marks the DOM dirty, and a `requestAnimationFrame` loop samples
//! `now - stamp` on the first frame after the mutation lands. That is
//! scroll-to-repaint as the user experiences it, including the WebSocket
//! round trip and the edit-ack stall.
//!
//! Run (from `crates/dat0-ui`, which is a detached workspace during the
//! migration — see the comment at the top of its `Cargo.toml`):
//!
//! ```text
//! cargo run --release --example grid_spike
//! ```
//!
//! Prints `p50=... p95=... ms` and exits. **Acceptance: p95 <= 33 ms.**

use dioxus::desktop::tao::dpi::LogicalSize;
use dioxus::desktop::wry::http::Response;
use dioxus::desktop::{Config, WindowBuilder, use_asset_handler};
use dioxus::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

const ROWS: usize = 1_000_000;
const COLS: usize = 40;
/// Design system: grid row / header height.
const ROW_H: f64 = 26.0;
/// Today's fixed `px(100.)` column width.
const COL_W: f64 = 100.0;
const OVERSCAN_ROWS: usize = 4;
const OVERSCAN_COLS: usize = 2;

/// Seconds of programmatic scrolling before the summary is emitted.
const RUN_SECS: u32 = 10;

/// The 40-cell block that carries the `.sel` class, so selection styling is in
/// the measurement rather than a best case with none.
const SEL_ROWS: std::ops::Range<usize> = 10..18;
const SEL_COLS: std::ops::Range<usize> = 0..5;

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_window(
                    WindowBuilder::new()
                        .with_title("dat0 — grid throughput spike")
                        .with_inner_size(LogicalSize::new(1280.0, 800.0)),
                )
                .with_background_color((0xff, 0xff, 0xff, 0xff)),
        )
        .launch(app);
}

/// Deterministic synthetic cell text. Mixed shapes so the per-cell formatting
/// cost resembles what `grid::renderers::cell_render` pays on real Arrow data.
fn cell_text(r: usize, c: usize) -> String {
    const WORDS: [&str; 12] = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
        "juliett", "kilo", "lima",
    ];
    match c % 5 {
        0 => (r * 40 + c).to_string(),
        1 => WORDS[(r * 31 + c * 7) % WORDS.len()].to_string(),
        2 => format!(
            "{:.3}",
            ((r * 7919 + c * 104_729) % 1_000_000) as f64 / 1000.0
        ),
        3 => {
            if (r + c) % 2 == 0 {
                "true".to_string()
            } else {
                "false".to_string()
            }
        }
        _ => format!("2026-{:02}-{:02}", (r % 12) + 1, (r % 28) + 1),
    }
}

/// Numeric columns are right-aligned, matching `CellDisplay::alignment`.
fn is_numeric(c: usize) -> bool {
    matches!(c % 5, 0 | 2)
}

const CSS: &str = r#"
@font-face {
  font-family: 'Geist Mono';
  src: url('/dat0/fonts/GeistMono-Regular.ttf') format('truetype');
  font-weight: 400;
  font-display: block;
}
* { box-sizing: border-box; }
html, body { margin: 0; padding: 0; height: 100%; background: #fcfcfb; }
body { -webkit-font-smoothing: antialiased; }
#viewport {
  position: absolute;
  inset: 0;
  overflow: auto;
  background: #ffffff;
  will-change: scroll-position;
}
#canvas { position: relative; }
.row {
  position: absolute;
  height: 26px;
  border-bottom: 1px solid #dde2e8;
}
.row:hover { background: #eef1f4; }
.cell {
  position: absolute;
  top: 0;
  height: 26px;
  display: flex;
  align-items: center;
  padding: 0 8px;
  font-family: 'Geist Mono', ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12.5px;
  line-height: 1;
  color: #1f2328;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.cell.num { justify-content: flex-end; }
.cell.sel { background: #e8f1fb; }
"#;

/// Driver + probe. Runs entirely in the webview so the timestamps share a clock.
///
/// Correlation is through `data-top`: Rust stamps the canvas with the exact
/// `scrollTop` the current DOM was rendered for, and JS keeps a table of
/// `scrollTop -> event timestamp`. Two things are then measurable, and the
/// distinction matters because two naive framings both degenerate:
///
/// * `latency` — for each distinct DOM state, the delay from the scroll event
///   that caused it to the first animation frame that shows it. This is
///   scroll-to-repaint. (Timing an unqualified MutationObserver instead reports
///   ~0 ms, because the mutation for scroll N lands just after scroll N+1's
///   timestamp.)
/// * `staleness` — sampled every frame: the age of what is on screen. Under
///   continuous scrolling this is the number a user actually perceives.
///   (Waiting for the DOM to *equal* the newest scroll position instead never
///   settles while scrolling, because the target keeps moving.)
const DRIVER_JS: &str = r#"
const vp = document.getElementById("viewport");
const canvas = document.getElementById("canvas");

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
const staleness = [];
let frames = 0;
let unmatched = 0;
let lastDomTop = NaN;
let lastDomStamp = null;

let raf = requestAnimationFrame(function tick() {
  const now = performance.now();
  frames++;
  const domTop = parseFloat(canvas.dataset.top);
  if (Number.isFinite(domTop)) {
    if (domTop !== lastDomTop) {
      lastDomTop = domTop;
      if (stamps.has(domTop)) {
        lastDomStamp = stamps.get(domTop);
        latency.push(now - lastDomStamp);
      } else {
        lastDomStamp = null;
        unmatched++;
      }
    }
    if (lastDomStamp !== null) staleness.push(now - lastDomStamp);
  }
  raf = requestAnimationFrame(tick);
});

const started = performance.now();
const iv = setInterval(() => { vp.scrollTop += 40; }, 1000 / 120);

setTimeout(() => {
  clearInterval(iv);
  cancelAnimationFrame(raf);
  const q = (xs, p) => xs.length
    ? xs[Math.min(xs.length - 1, Math.floor(xs.length * p))]
    : -1;
  latency.sort((a, b) => a - b);
  staleness.sort((a, b) => a - b);
  dioxus.send(JSON.stringify({
    elapsed_ms: performance.now() - started,
    frames: frames,
    scroll_events: scrollEvents,
    renders: latency.length,
    unmatched: unmatched,
    lat_p50: q(latency, 0.5),
    lat_p95: q(latency, 0.95),
    lat_p99: q(latency, 0.99),
    lat_max: latency.length ? latency[latency.length - 1] : -1,
    stale_p50: q(staleness, 0.5),
    stale_p95: q(staleness, 0.95),
    stale_max: staleness.length ? staleness[staleness.length - 1] : -1,
    scroll_top: vp.scrollTop,
    dom_rows: canvas.children.length,
    dom_cells: canvas.querySelectorAll(".cell").length
  }));
}, SECS * 1000);
"#;

#[derive(serde::Deserialize, Debug)]
struct Summary {
    elapsed_ms: f64,
    frames: usize,
    scroll_events: usize,
    renders: usize,
    unmatched: usize,
    lat_p50: f64,
    lat_p95: f64,
    lat_p99: f64,
    lat_max: f64,
    stale_p50: f64,
    stale_p95: f64,
    stale_max: f64,
    scroll_top: f64,
    dom_rows: usize,
    dom_cells: usize,
}

fn app() -> Element {
    // Phase 2.3 in miniature: fonts (and later icons/css/js) come out of the
    // binary over the `dat0` custom protocol, never off disk.
    use_asset_handler("dat0", move |req, resp| {
        let path = req.uri().path().to_string();
        let body = match path.rsplit('/').next() {
            Some("GeistMono-Regular.ttf") => std::fs::read(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../dat0-app/assets/fonts/GeistMono-Regular.ttf"
            ))
            .ok(),
            _ => None,
        };
        match body {
            Some(bytes) => resp.respond(
                Response::builder()
                    .header("Content-Type", "font/ttf")
                    .header("Cache-Control", "public, max-age=31536000")
                    .body(bytes)
                    .unwrap(),
            ),
            None => resp.respond(
                Response::builder()
                    .status(404)
                    .body(Vec::<u8>::new())
                    .unwrap(),
            ),
        }
    });

    let mut scroll_top = use_signal(|| 0.0_f64);
    let mut scroll_left = use_signal(|| 0.0_f64);
    let mut client_w = use_signal(|| 1280.0_f64);
    let mut client_h = use_signal(|| 800.0_f64);

    // Rust-side cross-check: scroll event -> post-render effect tick. Kept out
    // of signals so recording a sample cannot itself schedule a render.
    let probe: Rc<RefCell<(Option<Instant>, Vec<f64>)>> =
        use_hook(|| Rc::new(RefCell::new((None, Vec::new()))));

    {
        let probe = probe.clone();
        use_effect(move || {
            // Track the scroll signals so this reruns on every scroll-driven render.
            let _ = scroll_top();
            let _ = scroll_left();
            let mut p = probe.borrow_mut();
            if let Some(t0) = p.0.take() {
                let dt = t0.elapsed().as_secs_f64() * 1000.0;
                p.1.push(dt);
            }
        });
    }

    // Kick the driver once, after mount, then wait for its summary.
    {
        let probe = probe.clone();
        use_effect(move || {
            let probe = probe.clone();
            spawn(async move {
                let mut eval = document::eval(&DRIVER_JS.replace("SECS", &RUN_SECS.to_string()));
                match eval.recv::<String>().await {
                    Ok(raw) => {
                        let s: Summary = serde_json::from_str(&raw).expect("summary json");
                        let mut rust = probe.borrow().1.clone();
                        rust.sort_by(|a, b| a.partial_cmp(b).unwrap());
                        let rq = |p: f64| -> f64 {
                            if rust.is_empty() {
                                -1.0
                            } else {
                                rust[((rust.len() as f64 * p) as usize).min(rust.len() - 1)]
                            }
                        };
                        println!("--- dat0 grid spike ({ROWS} rows x {COLS} cols) ---");
                        println!(
                            "{:.0} ms wall: {} animation frames ({:.1} fps), {} scroll events, {} distinct renders shown ({} unmatched), final scrollTop={:.0}",
                            s.elapsed_ms,
                            s.frames,
                            s.frames as f64 / (s.elapsed_ms / 1000.0),
                            s.scroll_events,
                            s.renders,
                            s.unmatched,
                            s.scroll_top
                        );
                        println!(
                            "live DOM at end: {} row nodes, {} cell nodes (of {} x {} virtual)",
                            s.dom_rows, s.dom_cells, ROWS, COLS
                        );
                        println!(
                            "scroll-to-repaint latency: p50={:.1} p95={:.1} p99={:.1} max={:.1} ms",
                            s.lat_p50, s.lat_p95, s.lat_p99, s.lat_max
                        );
                        println!(
                            "on-screen content age:     p50={:.1} p95={:.1} max={:.1} ms",
                            s.stale_p50, s.stale_p95, s.stale_max
                        );
                        println!(
                            "rust scroll-event-to-post-render-effect: n={} p50={:.1} p95={:.1} ms",
                            rust.len(),
                            rq(0.5),
                            rq(0.95)
                        );
                        let worst = s.lat_p95.max(s.stale_p95);
                        let verdict = if worst <= 33.0 { "PASS" } else { "FAIL" };
                        println!(
                            "ACCEPTANCE p95 <= 33 ms: {verdict} (repaint p95={:.1} ms, content-age p95={:.1} ms)",
                            s.lat_p95, s.stale_p95
                        );
                        std::process::exit(i32::from(worst > 33.0));
                    }
                    Err(e) => {
                        eprintln!("driver failed: {e}");
                        std::process::exit(2);
                    }
                }
            });
        });
    }

    let total_h = ROWS as f64 * ROW_H;
    let total_w = COLS as f64 * COL_W;

    let first_row = ((scroll_top() / ROW_H).floor() as usize).saturating_sub(OVERSCAN_ROWS);
    let last_row =
        (((scroll_top() + client_h()) / ROW_H).ceil() as usize + OVERSCAN_ROWS).min(ROWS);
    let first_col = ((scroll_left() / COL_W).floor() as usize).saturating_sub(OVERSCAN_COLS);
    let last_col =
        (((scroll_left() + client_w()) / COL_W).ceil() as usize + OVERSCAN_COLS).min(COLS);

    rsx! {
        style { dangerous_inner_html: CSS }
        div {
            id: "viewport",
            onscroll: move |e| {
                let d = e.data();
                scroll_top.set(d.scroll_top());
                scroll_left.set(d.scroll_left());
                client_w.set(f64::from(d.client_width()));
                client_h.set(f64::from(d.client_height()));
                let mut p = probe.borrow_mut();
                if p.0.is_none() {
                    p.0 = Some(Instant::now());
                }
            },
            div {
                id: "canvas",
                // Correlation key for the probe: the scroll position this DOM
                // state was rendered for.
                "data-top": "{scroll_top()}",
                style: "width: {total_w}px; height: {total_h}px;",
                for r in first_row..last_row {
                    div {
                        key: "{r}",
                        class: "row",
                        style: "top: {r as f64 * ROW_H}px; width: {total_w}px;",
                        for c in first_col..last_col {
                            div {
                                key: "{c}",
                                class: if SEL_ROWS.contains(&r) && SEL_COLS.contains(&c) {
                                    if is_numeric(c) { "cell num sel" } else { "cell sel" }
                                } else if is_numeric(c) {
                                    "cell num"
                                } else {
                                    "cell"
                                },
                                style: "left: {c as f64 * COL_W}px; width: {COL_W}px;",
                                {cell_text(r, c)}
                            }
                        }
                    }
                }
            }
        }
    }
}
