//! Rendered *content*, asserted through the accessibility layer.
//!
//! Ported from `crates/dat0-app/tests/a11y_content.rs`, which existed because
//! gpui could not tell a test what a view had painted: every assertion here had
//! to go through an AccessKit tree that only existed under the `a11y-capture`
//! feature, and three of its six tests had to call a `pub` render function
//! directly because the production entry points were `pub(crate)` and therefore
//! unreachable from an integration crate.
//!
//! Both obstacles are gone. The attributes are real DOM in the release binary
//! (D-015 closed), and a Dioxus component is mounted with props rather than
//! reached through a shell, so every surface below is driven the way the app
//! drives it. What survives unchanged is the *shape* of each assertion: real
//! data in, the exact rendered strings out, and a teeth check that a value the
//! fixture never produced is absent — so a positive cannot pass by accident.
//!
//! The em-dash gotcha (PD-018) survives too: a grid cell paints `—` until its
//! page is resident, so every fixture awaits `page_for(0)` before mounting.

mod support;

use std::rc::Rc;
use std::sync::Arc;

use dioxus::prelude::*;
use serial_test::serial;
use tempfile::TempDir;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::error_ux::banner::Banner;
use dat0_core::events::AppEvents;
use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::selection::SelectionModel;
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};
use dat0_ui::components::banner::BannerHost;
use dat0_ui::components::grid::{COL_W_DEFAULT, Grid};
use dat0_ui::components::inspector::{Inspector, InspectorState};
use dat0_ui::components::shell::Shell;
use dat0_ui::state::{Status, Workspace};
use dat0_ui::theme::Theme;
use support::{Harness, dom::NodeKey};

const BUDGET: MemoryBudget = MemoryBudget {
    bytes: 128 * 1024 * 1024,
};

// ── engine fixtures ──────────────────────────────────────────────────────────

async fn engine(tmp: &TempDir) -> Arc<DuckDBEngine> {
    let e = DuckDBEngine::new(tmp.path().join("scratch.duckdb"), BUDGET).unwrap();
    e.init().await.unwrap();
    Arc::new(e)
}

/// Build a source over `table` with page 0 resident, plus its visible columns.
///
/// Page 0 must land before the first paint: `cell_render_for_source` is
/// synchronous and returns the unannotated `—` placeholder for a missing page,
/// which is exactly the trap the GPUI suite spent a hundred-iteration settle
/// loop avoiding.
async fn source_over(
    engine: &Arc<DuckDBEngine>,
    table: &str,
) -> (Arc<GridDataSource>, Vec<ProjectionColumn>) {
    let ds = GridDataSource::new(Arc::clone(engine), table.to_string())
        .await
        .unwrap();
    ds.page_for(0).await.unwrap();
    let columns = ds
        .visible_column_names()
        .into_iter()
        .map(|n| ProjectionColumn {
            source: n.clone(),
            display: n,
        })
        .collect();
    (Arc::new(ds), columns)
}

// ── the grid host ────────────────────────────────────────────────────────────

#[derive(Clone, Props)]
struct GridHostProps {
    source: Arc<GridDataSource>,
    columns: Vec<ProjectionColumn>,
}

// A data source owns an Arrow LRU and a DuckDB handle: identity is the only
// equality that means anything.
impl PartialEq for GridHostProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source) && self.columns == other.columns
    }
}

#[component]
fn GridHost(props: GridHostProps) -> Element {
    let rows = props.source.row_count as usize;
    let cols = props.columns.len();
    let selection = use_signal(|| SelectionModel::new(rows, cols));
    let widths = use_signal(|| vec![COL_W_DEFAULT; cols]);
    use_context_provider(|| selection);

    rsx! {
        Grid {
            source: props.source.clone(),
            selection,
            columns: props.columns.clone(),
            widths,
        }
    }
}

fn mount_grid(source: Arc<GridDataSource>, columns: Vec<ProjectionColumn>) -> Harness {
    Harness::new(GridHost, GridHostProps { source, columns })
}

// ── grid cell content ────────────────────────────────────────────────────────

/// The values a real import put on disk come back out of the grid as named
/// gridcells.
///
/// The original drove `file_drop::handle_drop`; here the CSV still comes off
/// the filesystem through DuckDB's own reader, so the assertion is still about
/// values that made a round trip rather than a literal the test typed twice.
#[tokio::test]
async fn grid_cells_announce_the_values_the_engine_returned() {
    let tmp = TempDir::new().unwrap();
    let csv = tmp.path().join("cells.csv");
    std::fs::write(&csv, "a,b\n1,2\n3,4\n").unwrap();

    let engine = engine(&tmp).await;
    let sql = format!("SELECT * FROM read_csv_auto('{}')", csv.display());
    engine
        .create_table("cells", &sql, DerivedOrigin::Sql(sql.clone()))
        .await
        .unwrap();
    let (source, columns) = source_over(&engine, "cells").await;
    assert_eq!(
        columns
            .iter()
            .map(|c| c.display.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"],
        "the surrogate key stays out of the visible projection"
    );

    let h = mount_grid(source, columns);

    // Every cell is a `gridcell` whose accessible name carries both the column
    // it belongs to and the value in it — the thing gpui could not extract at
    // all, and the reason the GPUI suite needed a whole capture feature.
    for (row, col, column, value) in [
        (0, 0, "a", "1"),
        (0, 1, "b", "2"),
        (1, 0, "a", "3"),
        (1, 1, "b", "4"),
    ] {
        let key = h
            .by_a11y_id(&format!("cell-{row}-{col}"))
            .unwrap_or_else(|| panic!("cell {row},{col} must render"));
        assert_eq!(h.attr(key, "role").as_deref(), Some("gridcell"));
        assert_eq!(
            h.attr(key, "aria-label").as_deref(),
            Some(format!("{column}: {value}").as_str())
        );
        assert_eq!(h.text_of(key), value);
    }

    assert_eq!(
        h.by_role("gridcell").len(),
        4,
        "two rows of two columns, and nothing invented"
    );

    // Teeth: a value the table never held must be absent, so the positives
    // above are bound to rendered content rather than always true.
    assert!(!h.has_label_contains("9999"));
    assert_eq!(h.count_label("a: 9999"), 0);
}

/// A query result is grid content like any other.
///
/// This is the surviving half of the GPUI suite's
/// `sql_console_renders_result_and_timing_content`. That test executed
/// `SELECT 1 AS x` into a view, bound the view to the console's result pane and
/// asserted two things: the timing chip's text, and the result cell's value.
/// The Dioxus console has no result pane and no timing chip yet, so the chip
/// assertion has no surface (reported, not silently dropped); the cell
/// assertion does — and the original itself recorded why it is the same
/// guarantee either way: "result cells reuse the shared grid delegate", so what
/// is being proved is that a source built over a *query result view* renders
/// its value, not that the console has a particular shape.
#[tokio::test]
async fn a_query_result_announces_its_value_as_a_gridcell() {
    let tmp = TempDir::new().unwrap();
    let engine = engine(&tmp).await;

    // Exactly what the run pipeline does: the statement becomes a view, and the
    // source is built over the view — the ctor's schema and row-count probes
    // are what actually execute the query.
    engine
        .create_or_replace_view("sql_result_pane", "SELECT 1 AS x")
        .await
        .unwrap();
    let (source, columns) = source_over(&engine, "sql_result_pane").await;
    assert_eq!(source.row_count, 1, "SELECT 1 returns one row");

    let h = mount_grid(source, columns);

    let cell = h
        .by_a11y_id("cell-0-0")
        .expect("the result cell must render");
    assert_eq!(h.attr(cell, "role").as_deref(), Some("gridcell"));
    assert_eq!(h.attr(cell, "aria-label").as_deref(), Some("x: 1"));

    // Teeth, as in the original: a value this run never produced is absent.
    assert!(!h.has_label_contains("424242"));
}

// ── the inspector ────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct InspectorHostProps {
    target: String,
    profile: dat0_engine::TableProfile,
}

#[component]
fn InspectorHost(props: InspectorHostProps) -> Element {
    Workspace::provide();
    Theme::provide(None);
    let state = InspectorState::use_new();

    {
        let seed = props.clone();
        use_hook(move || {
            state.set_target(seed.target.clone());
            let id = state.begin_load();
            state.put_profile(id, seed.profile.clone());
        });
    }

    rsx! { Inspector { state } }
}

/// The Inspector's overview line and column-card headers carry the engine's
/// real `SUMMARIZE` output.
///
/// The original could not reach `set_inspector_target` from an integration
/// crate and called the `render_inspector` pure fn directly with a hand-built
/// model. Here the panel is mounted with the state the shell gives it, but the
/// profile is still the genuine one — the types below are DuckDB's answer, not
/// a literal, which is what makes `id · BIGINT` a real assertion.
///
/// Determinism: the fixture casts both columns, so `column_type` is pinned
/// rather than left to literal inference.
#[tokio::test]
async fn the_inspector_announces_the_shape_and_types_the_engine_profiled() {
    let tmp = TempDir::new().unwrap();
    let engine = engine(&tmp).await;

    const SQL: &str = "SELECT CAST(id AS BIGINT) AS id, CAST(val AS VARCHAR) AS val \
                       FROM (VALUES (1, 'a'), (2, 'b'), (3, 'c')) AS t(id, val)";
    engine
        .create_table("probe", SQL, DerivedOrigin::Sql(SQL.into()))
        .await
        .unwrap();
    let profile = engine.profile_table("probe", None).await.unwrap();

    assert_eq!(profile.rows, 3, "fixture has 3 rows");
    // 3, not 2: `create_table` injects the `__dat0_rowid` surrogate at create
    // time and SUMMARIZE profiles it too. The overview counts the raw profile
    // list; `project_cards` filters the surrogate back out, so exactly two
    // cards render.
    assert_eq!(
        profile.columns.len(),
        3,
        "id, val, and the __dat0_rowid surrogate"
    );

    let h = Harness::new(
        InspectorHost,
        InspectorHostProps {
            target: "probe".to_string(),
            profile,
        },
    );

    let overview = h
        .by_a11y_id("inspector-overview")
        .expect("the overview line must render");
    assert_eq!(
        h.attr(overview, "aria-label").as_deref(),
        Some("probe — 3 rows · 3 cols"),
        "the overview announces the real profiled counts"
    );
    assert_eq!(h.text_of(overview), "probe — 3 rows · 3 cols");

    assert!(
        h.has_label("id · BIGINT"),
        "the id card header must announce its real profiled type"
    );
    assert!(
        h.has_label("val · VARCHAR"),
        "the val card header must announce its real profiled type"
    );

    // Teeth: neither a column that is not there nor a type the engine did not
    // report may appear.
    assert!(!h.has_label_contains("nonexistent_col_zzz"));
    assert!(
        !h.has_label("id · DOUBLE"),
        "id profiles as BIGINT, and an assertion that passes for any type is no \
         assertion"
    );
}

// ── banners ──────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct BannerHostProps {
    banners: Vec<Banner>,
}

#[component]
fn Banners(props: BannerHostProps) -> Element {
    rsx! {
        BannerHost {
            banners: props.banners.clone(),
            on_action: move |_: String| {},
            on_dismiss: move |_: usize| {},
        }
    }
}

/// A banner announces its title and its body, and only the terminal kind
/// interrupts.
///
/// The GPUI render gave every banner title `AccessRole::Alert`. The port keeps
/// the content assertion and tightens the role one: an alert asks a screen
/// reader to cut off whatever it was saying, which is right for a failed
/// session and wrong for a notice you read when you get to it.
#[test]
fn a_banner_announces_its_title_and_body() {
    const TITLE: &str = "Disk almost full";
    const BODY: &str = "Free up space before importing large files.";

    let h = Harness::new(
        Banners,
        BannerHostProps {
            banners: vec![Banner::warning_with_body(TITLE, BODY)],
        },
    );

    let banner = h.by_a11y_id("banner-0").expect("the banner must render");
    assert_eq!(
        h.attr(banner, "aria-label").as_deref(),
        Some(TITLE),
        "the title is the banner's accessible name"
    );
    // Both strings are on screen, not merely in the accessible name: a banner
    // whose title only existed as an `aria-label` would be invisible to
    // everyone not using a reader.
    let painted = h.text_of(banner);
    assert!(painted.contains(TITLE), "got {painted:?}");
    assert!(painted.contains(BODY), "got {painted:?}");

    let body = h
        .by_a11y_id("banner-0-body")
        .expect("a non-empty body must render its own node");
    assert_eq!(h.attr(body, "role").as_deref(), Some("note"));
    assert_eq!(h.attr(body, "aria-label").as_deref(), Some(BODY));

    // Teeth: content this banner never set must be absent.
    assert!(!h.has_label("A title this banner never set"));
    assert!(!h.has_label("Body text that was never set"));
}

#[test]
fn only_a_failure_interrupts_the_reader() {
    let h = Harness::new(
        Banners,
        BannerHostProps {
            banners: vec![
                Banner::warning_with_body("Disk almost full", "Free up space."),
                Banner::error("Session failed", "DuckDB would not open."),
            ],
        },
    );

    assert!(
        h.query_by_role("note", "Disk almost full"),
        "a warning is a note: you read it when you get to it"
    );
    assert!(
        h.query_by_role("alert", "Session failed"),
        "a failed session is the one kind that must cut in — the window is the \
         failure until the user retries"
    );
    assert!(
        !h.query_by_role("alert", "Disk almost full"),
        "a warning must not be announced as an alert"
    );
}

// ── the shell's own content ──────────────────────────────────────────────────

/// A fresh, already-onboarded config dir, so the shell renders its steady
/// state. `DAT0_CONFIG_DIR` is process-global, hence `#[serial]`.
fn with_settled_config<R>(f: impl FnOnce() -> R) -> R {
    let tmp = TempDir::new().unwrap();
    let store =
        dat0_core::settings::store::SettingsStore::with_path(tmp.path().join("settings.toml"));
    dat0_core::settings::set_first_run_done(&store, true).unwrap();

    let previous = std::env::var_os("DAT0_CONFIG_DIR");
    // SAFETY: `#[serial]` keeps every env-touching test off the same clock, and
    // no other thread in this binary reads the variable.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", tmp.path()) };
    let out = f();
    unsafe {
        match previous {
            Some(v) => std::env::set_var("DAT0_CONFIG_DIR", v),
            None => std::env::remove_var("DAT0_CONFIG_DIR"),
        }
    }
    out
}

#[component]
fn ShellHost() -> Element {
    let mut ws = Workspace::provide();
    Theme::provide(None);
    use_context_provider(ActionRegistry::new);
    let (events, _rx) = use_hook(|| {
        let (tx, rx) = AppEvents::channel();
        (tx, Rc::new(std::cell::RefCell::new(rx)))
    });
    use_context_provider(|| events.clone());

    rsx! {
        Shell {}
        // The grid's shape reaches the bar as telemetry, not as a data source:
        // one driver stands in for whatever produced it.
        button {
            "data-a11y-id": "drive-two-rows",
            onclick: move |_| {
                let next = Status {
                    rows: Some((1, 2, 2)),
                    ..ws.status.read().clone()
                };
                ws.status.set(next);
            },
            "rows"
        }
    }
}

fn mount_shell() -> Harness {
    let mut h = Harness::new(ShellHost, ());
    h.settle();
    h
}

/// The status bar always says what the engine and the egress counter are doing,
/// and says nothing it has not been told.
#[test]
#[serial]
fn the_status_bar_announces_only_the_segments_it_has_data_for() {
    with_settled_config(|| {
        let mut h = mount_shell();
        let bar = h.by_a11y_id("statusbar").expect("the bar is unconditional");
        assert_eq!(
            h.attr(bar, "role").as_deref(),
            Some("status"),
            "the bar is the live region a reader watches"
        );

        let before = h.text_of(bar);
        assert!(
            before.contains("engine duckdb · native"),
            "the engine segment paints before any data is loaded: got {before:?}"
        );
        assert!(
            before.contains("egress 0 B"),
            "the egress segment is always present and measured — it is the \
             privacy claim's only surface: got {before:?}"
        );
        assert!(
            !before.contains("rows"),
            "with no grid there is no row window to report: got {before:?}"
        );

        h.click("drive-two-rows");

        let after = h.text_of(h.by_a11y_id("statusbar").unwrap());
        assert!(
            after.contains("rows 1–2 / 2"),
            "the bar must report the visible window over the total: got {after:?}"
        );
        assert!(
            after.contains("engine duckdb · native"),
            "the engine segment must survive the update: got {after:?}"
        );
        assert!(
            after.contains("egress 0 B"),
            "and so must egress: got {after:?}"
        );

        // Teeth, carried over verbatim in spirit: nothing is selected and no
        // run has happened, so neither segment may appear.
        assert!(!after.contains("cells selected"));
        assert!(!after.contains("Query "));
    });
}

/// The connection identity the GPUI status bar carried as a `Local` segment now
/// lives in the titlebar pill (UI3), and it is still announced.
#[test]
#[serial]
fn the_window_announces_that_its_data_is_local() {
    with_settled_config(|| {
        let h = mount_shell();
        let pill = h
            .by_a11y_id("source-pill")
            .expect("the source pill is chrome, present in every window state");
        assert_eq!(
            h.text_of(pill),
            "local",
            "no live source and not read-only means local"
        );
    });
}

/// Every ancestor of `key`, nearest first.
fn ancestors(h: &Harness, key: NodeKey) -> Vec<NodeKey> {
    let mut out = Vec::new();
    let mut cur = h.dom().get(key).parent;
    while let Some(k) = cur {
        out.push(k);
        cur = h.dom().get(k).parent;
    }
    out
}

/// The centre body paints *through* the pane stack.
///
/// The GPUI original (B5) proved the grid centre rendered through
/// `DockArea → DockItem::Panel → GridPanel → render_grid_body`, with two
/// assertions because either alone was satisfiable by a dock nobody painted:
/// the dock was mounted, *and* a control built inside the body resolved
/// geometry through it. The dock is now a CSS grid and the panel indirection is
/// the `pane-stack` element, so the same pair holds — the stack exists, and the
/// body's own content is inside it rather than beside it.
#[test]
#[serial]
fn the_centre_body_renders_inside_the_pane_stack() {
    with_settled_config(|| {
        let h = mount_shell();
        let stack = h
            .by_a11y_id("pane-stack")
            .expect("the shell rendered without building its pane stack");

        let hero = h
            .by_a11y_id("empty-state")
            .expect("an empty workspace paints the hero as its centre body");
        assert!(
            ancestors(&h, hero).contains(&stack),
            "the centre body must render through the pane stack, not beside it \
             — a stack with nothing under it is the failure this catches"
        );

        // The banner host lives in the same stack, above the body: banners are
        // part of the centre, not floating chrome.
        assert!(
            h.by_a11y_id("banner-host").is_none(),
            "no banners were raised, so the host must not paint an empty frame"
        );
    });
}
