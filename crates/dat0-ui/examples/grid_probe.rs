//! Real-window probe for the grid's reach (PD-026).
//!
//! The grid's canvas used to be `rows × 26px` tall, and WebKit clamps a layout
//! length near 33.5M px: every row past ~1,290,555 was out of reach, and
//! Ctrl+End moved the cursor to the last row without showing it. The headless
//! harness has no layout engine and could see neither. This lays out a
//! 2,000,000-row table in a real window and checks, in WebKit's own geometry,
//! that
//!
//! - the canvas is laid out at the height the grid gives it, under the clamp;
//! - scrolled to the bottom, the last row is on screen;
//! - Ctrl/Cmd+Down moves to the last row *and* scrolls it into view, and
//!   Ctrl/Cmd+Up brings back the first.
//!
//! ```text
//! cargo run --release -p dat0-ui --example grid_probe
//! ```
//!
//! Exits 0 when every check passes, 1 otherwise.

use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::selection::SelectionModel;
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};
use dat0_ui::components::grid::scroll::MAX_CANVAS_H;
use dat0_ui::components::grid::{COL_W_DEFAULT, Grid};
use dat0_ui::theme::{Theme, ThemeStyle};

const ROWS: u64 = 2_000_000;

/// Measured in the page, reported to Rust. Timers rather than
/// `requestAnimationFrame`, which an unfocused window never runs (see
/// `shell_probe`).
const PROBE: &str = r#"
const settle = () => new Promise((r) => setTimeout(r, 4));
async function waitFor(pred, tries = 1500) {
  for (let i = 0; i < tries; i++) {
    if (pred()) return true;
    await settle();
  }
  return false;
}
const guard = setTimeout(() => {
  dioxus.send({ error: "probe did not finish within 60s" });
}, 60000);

try {
  const q = (id) => document.querySelector(`[data-a11y-id="${id}"]`);
  if (!(await waitFor(() => q("grid-viewport") && q("row-0")))) {
    throw new Error("the grid never mounted");
  }
  const vp = q("grid-viewport");
  // The stylesheet arrives over the asset protocol after the first render;
  // until it applies the viewport is not a scroller, and a scroll is ignored.
  if (!(await waitFor(() =>
    getComputedStyle(vp).overflowY === "auto" &&
    vp.clientHeight > 0 &&
    vp.clientHeight < vp.scrollHeight))) {
    throw new Error("the grid never became a scroller");
  }
  const canvas = vp.querySelector(".d0-grid-canvas");
  const last = "row-LAST";
  const inView = (id) => {
    const el = q(id);
    if (!el) return false;
    const r = el.getBoundingClientRect();
    const v = vp.getBoundingClientRect();
    return r.top >= v.top - 0.5 && r.bottom <= v.bottom + 0.5;
  };

  const canvas_h = canvas.getBoundingClientRect().height;

  vp.scrollTop = vp.scrollHeight;
  const bottom = await waitFor(() => inView(last));
  // What the bottom looked like, for a failure to name.
  const rows = [...vp.querySelectorAll('[role="row"]')];
  const tail = rows[rows.length - 1];
  const at_bottom = {
    scroll_top: vp.scrollTop,
    scroll_height: vp.scrollHeight,
    client_height: vp.clientHeight,
    view_bottom: vp.getBoundingClientRect().bottom,
    last_rendered: tail ? tail.getAttribute("data-a11y-id") : null,
    last_rendered_bottom: tail ? tail.getBoundingClientRect().bottom : null,
    rendered: rows.length,
  };

  vp.scrollTop = 0;
  const top = await waitFor(() => inView("row-0"));

  // The platform's jump chord, as a keystroke at the focused grid.
  vp.focus();
  const mac = navigator.platform.includes("Mac");
  const press = (key) =>
    vp.dispatchEvent(
      new KeyboardEvent("keydown", {
        key,
        code: key,
        ctrlKey: !mac,
        metaKey: mac,
        bubbles: true,
        cancelable: true,
      }),
    );
  press("ArrowDown");
  const jumped = await waitFor(() => inView(last));
  press("ArrowUp");
  const back = await waitFor(() => inView("row-0"));

  dioxus.send({ canvas_h, bottom, top, jumped, back, at_bottom: JSON.stringify(at_bottom) });
} catch (e) {
  dioxus.send({ error: String(e) });
}
clearTimeout(guard);
await dioxus.recv();
"#;

#[derive(serde::Deserialize, Debug, Default)]
struct Report {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    canvas_h: f64,
    #[serde(default)]
    bottom: bool,
    #[serde(default)]
    top: bool,
    #[serde(default)]
    jumped: bool,
    #[serde(default)]
    back: bool,
    /// The viewport and its last rendered row, scrolled to the bottom.
    #[serde(default)]
    at_bottom: String,
}

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(dat0_ui::launch::config())
        .launch(Probe);
}

/// Two million rows, generated in DuckDB: the count is the point, not the
/// ingest path.
async fn table() -> Result<(Arc<GridDataSource>, Vec<ProjectionColumn>), String> {
    let dir = scratch();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let engine = DuckDBEngine::new(
        dir.join("scratch.duckdb"),
        MemoryBudget {
            bytes: 512 * 1024 * 1024,
        },
    )
    .map_err(|e| e.to_string())?;
    engine.init().await.map_err(|e| e.to_string())?;
    let sql = format!("SELECT i AS id, 'row ' || i AS label FROM range({ROWS}) t(i)");
    engine
        .create_table("big", &sql, DerivedOrigin::Sql(sql.clone()))
        .await
        .map_err(|e| e.to_string())?;
    let source = GridDataSource::new(Arc::new(engine), "big".to_string())
        .await
        .map_err(|e| e.to_string())?;
    let columns = source
        .visible_column_names()
        .into_iter()
        .map(|n| ProjectionColumn {
            source: n.clone(),
            display: n,
        })
        .collect();
    Ok((Arc::new(source), columns))
}

/// Where the table lives: removed on exit, whatever the outcome.
fn scratch() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("dat0-grid-probe-{}", std::process::id()))
}

fn exit(code: i32) -> ! {
    let _ = std::fs::remove_dir_all(scratch());
    std::process::exit(code)
}

#[component]
fn Probe() -> Element {
    Theme::provide(None);
    dioxus::desktop::use_asset_handler("dat0", dat0_ui::protocol::serve);
    let built = use_resource(table);
    let got = built.read().clone();
    match got {
        None => rsx! { ThemeStyle {} },
        Some(Err(e)) => {
            println!("probe error: could not build the table: {e}");
            exit(1);
        }
        Some(Ok((source, columns))) => rsx! {
            ThemeStyle {}
            Mounted { source, columns }
        },
    }
}

#[derive(Clone, Props)]
struct MountedProps {
    source: Arc<GridDataSource>,
    columns: Vec<ProjectionColumn>,
}

impl PartialEq for MountedProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source)
    }
}

#[component]
fn Mounted(props: MountedProps) -> Element {
    let rows = usize::try_from(props.source.row_count).unwrap_or(usize::MAX);
    let cols = props.columns.len();
    let selection = use_signal(|| SelectionModel::new(rows, cols.max(1)));
    let widths = use_signal(|| vec![COL_W_DEFAULT; cols]);

    // After mount, when the document provider exists.
    use_effect(move || {
        spawn(async move {
            let script = PROBE.replace("row-LAST", &format!("row-{}", ROWS - 1));
            let mut eval = document::eval(&script);
            let got = eval.recv::<Report>().await;
            let _ = eval.send(true);
            match got {
                Ok(r) => report(r),
                Err(e) => {
                    println!("probe channel failed: {e}");
                    exit(2);
                }
            }
        });
    });

    rsx! {
        div { class: "d0-window",
            Grid {
                source: props.source.clone(),
                selection,
                columns: props.columns.clone(),
                widths,
            }
        }
    }
}

fn report(r: Report) {
    println!("--- dat0 grid probe ({ROWS} rows) ---");
    if let Some(e) = r.error {
        println!("probe error: {e}");
        exit(1);
    }
    println!("  at the bottom: {}", r.at_bottom);
    let checks = [
        (
            "canvas laid out at the cap",
            format!("{}", r.canvas_h),
            (r.canvas_h - MAX_CANVAS_H).abs() <= 1.0,
        ),
        (
            "last row shown at the bottom",
            r.bottom.to_string(),
            r.bottom,
        ),
        ("first row shown at the top", r.top.to_string(), r.top),
        (
            "Ctrl/Cmd+Down shows the last row",
            r.jumped.to_string(),
            r.jumped,
        ),
        ("Ctrl/Cmd+Up shows the first", r.back.to_string(), r.back),
    ];
    let mut failed = 0;
    for (name, got, ok) in &checks {
        println!(
            "  {name:<34} {got:<14} {}",
            if *ok { "ok" } else { "MISMATCH" }
        );
        if !ok {
            failed += 1;
        }
    }
    if failed == 0 {
        println!("PASS");
        exit(0);
    }
    println!("FAIL ({failed} checks)");
    exit(1);
}
