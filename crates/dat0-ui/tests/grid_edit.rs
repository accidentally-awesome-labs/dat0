//! Cell editing, the context menu, and the grid's keyboard grammar, driven
//! through the mounted component.
//!
//! Two of these close deferrals outright:
//!
//! * **PD-020** — Tab did nothing in the cell editor, because
//!   `gpui-component`'s `Input` swallowed it. A plain `<input>` does not.
//! * The editor was a **fixed-position overlay** that appeared in the same
//!   place regardless of which cell was being edited. It is now positioned from
//!   the same arithmetic that places the cell.

mod support;

use std::sync::Arc;

use dioxus::prelude::*;
use tempfile::TempDir;

use dat0_core::actions::builtin::ids;
use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::selection::SelectionModel;
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};
use dat0_ui::components::grid::{COL_W_DEFAULT, Grid, ROW_H};
use support::{Harness, Key, Modifiers};

async fn fixture() -> (Arc<GridDataSource>, Vec<ProjectionColumn>, TempDir) {
    let tmp = TempDir::new().unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    const SQL: &str = "SELECT * FROM (VALUES \
        (1, 'alpha', 10), (2, 'bravo', 20), (3, 'charlie', 30)) v(id, name, score)";
    engine
        .create_table("t", SQL, DerivedOrigin::Sql(SQL.into()))
        .await
        .unwrap();
    let engine = Arc::new(engine);
    let ds = GridDataSource::new(Arc::clone(&engine), "t".to_string())
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
    (Arc::new(ds), columns, tmp)
}

#[derive(Clone, Props)]
struct HostProps {
    source: Arc<GridDataSource>,
    columns: Vec<ProjectionColumn>,
    read_only: bool,
}

impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source)
            && self.columns == other.columns
            && self.read_only == other.read_only
    }
}

#[component]
fn Host(props: HostProps) -> Element {
    let cols = props.columns.len();
    let selection = use_signal(|| SelectionModel::new(3, cols));
    let widths = use_signal(|| vec![COL_W_DEFAULT; cols]);
    // Readback surfaces: the harness sees text, not Rust state.
    let mut edits = use_signal(Vec::<String>::new);
    let mut actions = use_signal(Vec::<String>::new);

    rsx! {
        Grid {
            source: props.source.clone(),
            selection,
            columns: props.columns.clone(),
            widths,
            read_only: props.read_only,
            on_edit: move |(c, v): (dat0_core::grid::selection::CellCoord, String)| {
                edits.write().push(format!("{},{}={v}", c.row, c.col));
            },
            on_action: move |(id, c): (&'static str, dat0_core::grid::selection::CellCoord)| {
                actions.write().push(format!("{id}@{},{}", c.row, c.col));
            },
        }
        div { "data-a11y-id": "edits", "{edits.read().join(\"|\")}" }
        div { "data-a11y-id": "actions", "{actions.read().join(\"|\")}" }
        div { "data-a11y-id": "active", "{selection.read().active().row},{selection.read().active().col}" }
        div { "data-a11y-id": "count", "{selection.read().selected_cell_count()}" }
    }
}

async fn mount(read_only: bool) -> (Harness, TempDir) {
    let (source, columns, tmp) = fixture().await;
    (
        Harness::new(
            Host,
            HostProps {
                source,
                columns,
                read_only,
            },
        ),
        tmp,
    )
}

fn key(h: &mut Harness, k: Key, mods: Modifiers) {
    let vp = h.by_a11y_id("grid-viewport").unwrap();
    h.key(vp, k, mods);
}

// ── keyboard ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn arrows_move_the_active_cell() {
    let (mut h, _t) = mount(false).await;
    assert_eq!(h.text_of(h.by_a11y_id("active").unwrap()), "0,0");

    key(&mut h, Key::ArrowDown, Modifiers::empty());
    key(&mut h, Key::ArrowDown, Modifiers::empty());
    key(&mut h, Key::ArrowRight, Modifiers::empty());
    assert_eq!(h.text_of(h.by_a11y_id("active").unwrap()), "2,1");
}

#[tokio::test]
async fn shift_arrow_extends_the_selection() {
    let (mut h, _t) = mount(false).await;
    key(&mut h, Key::ArrowDown, Modifiers::empty());
    key(&mut h, Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(h.text_of(h.by_a11y_id("count").unwrap()), "2");
}

#[tokio::test]
async fn select_all_then_escape_clears() {
    let (mut h, _t) = mount(false).await;
    let jump = if cfg!(target_os = "macos") {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    };
    key(&mut h, Key::Character("a".into()), jump);
    // 3 rows x 3 columns.
    assert_eq!(h.text_of(h.by_a11y_id("count").unwrap()), "9");

    key(&mut h, Key::Escape, Modifiers::empty());
    assert_eq!(h.text_of(h.by_a11y_id("count").unwrap()), "0");
}

// ── the editor ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn enter_opens_the_editor_over_the_active_cell() {
    let (mut h, _t) = mount(false).await;
    assert!(h.by_a11y_id("cell-editor").is_none());

    key(&mut h, Key::ArrowDown, Modifiers::empty());
    key(&mut h, Key::ArrowRight, Modifiers::empty());
    key(&mut h, Key::Enter, Modifiers::empty());

    let ed = h.by_a11y_id("cell-editor").expect("the editor opened");
    let style = h.attr(ed, "style").unwrap_or_default();
    // Cell (1,1): top = 1 * 26, left = one column width. The old editor was a
    // fixed overlay that ignored both.
    assert!(style.contains(&format!("top: {}px", ROW_H)), "{style}");
    assert!(
        style.contains(&format!("left: {COL_W_DEFAULT}px")),
        "{style}"
    );
    assert_eq!(h.attr(ed, "value").as_deref(), Some("bravo"));
}

#[tokio::test]
async fn a_read_only_workspace_refuses_to_open_the_editor() {
    let (mut h, _t) = mount(true).await;
    key(&mut h, Key::Enter, Modifiers::empty());
    assert!(h.by_a11y_id("cell-editor").is_none());
}

#[tokio::test]
async fn enter_commits_and_steps_down() {
    let (mut h, _t) = mount(false).await;
    key(&mut h, Key::Enter, Modifiers::empty());
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.dispatch(ed, "input", form("42"));
    h.key(ed, Key::Enter, Modifiers::empty());

    assert_eq!(h.text_of(h.by_a11y_id("edits").unwrap()), "0,0=42");
    assert_eq!(h.text_of(h.by_a11y_id("active").unwrap()), "1,0");
    assert!(h.by_a11y_id("cell-editor").is_none(), "the editor closed");
}

#[tokio::test]
async fn tab_commits_and_steps_right_closing_pd_020() {
    // The deferral existed only because gpui-component's Input swallowed Tab.
    let (mut h, _t) = mount(false).await;
    key(&mut h, Key::Enter, Modifiers::empty());
    let ed = h.by_a11y_id("cell-editor").unwrap();
    // `id` is numeric, so the value has to be one — the editor refuses a
    // value the column cannot hold, whichever key commits it.
    h.dispatch(ed, "input", form("7"));
    h.key(ed, Key::Tab, Modifiers::empty());

    assert_eq!(h.text_of(h.by_a11y_id("edits").unwrap()), "0,0=7");
    assert_eq!(h.text_of(h.by_a11y_id("active").unwrap()), "0,1");
}

#[tokio::test]
async fn shift_tab_steps_left() {
    let (mut h, _t) = mount(false).await;
    key(&mut h, Key::ArrowRight, Modifiers::empty());
    key(&mut h, Key::ArrowRight, Modifiers::empty());
    key(&mut h, Key::Enter, Modifiers::empty());
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.dispatch(ed, "input", form("8")); // `score` is numeric
    h.key(ed, Key::Tab, Modifiers::SHIFT);

    assert_eq!(h.text_of(h.by_a11y_id("active").unwrap()), "0,1");
}

#[tokio::test]
async fn escape_cancels_without_writing() {
    let (mut h, _t) = mount(false).await;
    key(&mut h, Key::Enter, Modifiers::empty());
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.dispatch(ed, "input", form("discard me"));
    h.key(ed, Key::Escape, Modifiers::empty());

    assert_eq!(h.text_of(h.by_a11y_id("edits").unwrap()), "");
    assert!(h.by_a11y_id("cell-editor").is_none());
    assert_eq!(
        h.text_of(h.by_a11y_id("active").unwrap()),
        "0,0",
        "a cancel must not move the cursor"
    );
}

#[tokio::test]
async fn blur_commits_in_place() {
    let (mut h, _t) = mount(false).await;
    key(&mut h, Key::Enter, Modifiers::empty());
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.dispatch(ed, "input", form("55")); // `id` is numeric
    // A blur carries focus data, not form data — the converter downcasts by
    // type and an `unwrap` inside dioxus is the only complaint you get.
    h.dispatch(ed, "blur", dioxus::html::SerializedFocusData {});

    assert_eq!(h.text_of(h.by_a11y_id("edits").unwrap()), "0,0=55");
    assert_eq!(
        h.text_of(h.by_a11y_id("active").unwrap()),
        "0,0",
        "a blur commit does not move the cursor"
    );
}

#[tokio::test]
async fn an_arrow_inside_the_editor_does_not_move_the_grid_cursor() {
    // Otherwise moving the caret through the text drags the selection with it.
    let (mut h, _t) = mount(false).await;
    key(&mut h, Key::Enter, Modifiers::empty());
    key(&mut h, Key::ArrowDown, Modifiers::empty());
    assert_eq!(h.text_of(h.by_a11y_id("active").unwrap()), "0,0");
}

// ── the context menu ────────────────────────────────────────────────────────

#[tokio::test]
async fn right_click_opens_the_menu_at_the_pointer() {
    let (mut h, _t) = mount(false).await;
    assert!(h.by_a11y_id("context-menu").is_none());

    let canvas = h
        .dom()
        .walk()
        .into_iter()
        .find(|k| {
            h.dom()
                .get(*k)
                .attr("class")
                .is_some_and(|c| c.contains("d0-grid-canvas"))
        })
        .unwrap();
    h.dispatch(canvas, "contextmenu", mouse_at(120.0, 80.0));

    let menu = h.by_a11y_id("context-menu").expect("the menu opened");
    let style = h.attr(menu, "style").unwrap_or_default();
    assert!(style.contains("left: 120px"), "{style}");
    assert!(style.contains("top: 80px"), "{style}");
}

#[tokio::test]
async fn picking_an_item_reports_the_action_and_closes() {
    let (mut h, _t) = mount(false).await;
    open_menu(&mut h);

    h.dispatch(
        h.by_a11y_id(&format!("menu-{}", ids::VIEW_COPY)).unwrap(),
        "mousedown",
        mouse_at(0.0, 0.0),
    );

    assert_eq!(
        h.text_of(h.by_a11y_id("actions").unwrap()),
        format!("{}@0,0", ids::VIEW_COPY)
    );
    assert!(h.by_a11y_id("context-menu").is_none());
}

#[tokio::test]
async fn escape_dismisses_the_menu_without_picking() {
    let (mut h, _t) = mount(false).await;
    open_menu(&mut h);
    let menu = h.by_a11y_id("context-menu").unwrap();
    h.key(menu, Key::Escape, Modifiers::empty());

    assert!(h.by_a11y_id("context-menu").is_none());
    assert_eq!(h.text_of(h.by_a11y_id("actions").unwrap()), "");
}

#[tokio::test]
async fn a_click_outside_dismisses_the_menu() {
    let (mut h, _t) = mount(false).await;
    open_menu(&mut h);
    h.dispatch(
        h.by_a11y_id("context-menu-dismiss").unwrap(),
        "mousedown",
        mouse_at(0.0, 0.0),
    );
    assert!(h.by_a11y_id("context-menu").is_none());
}

#[tokio::test]
async fn a_disabled_item_is_marked_and_does_nothing() {
    // No selection, so Delete Row(s) is present but refused.
    let (mut h, _t) = mount(false).await;
    open_menu(&mut h);

    let item = h
        .by_a11y_id(&format!("menu-{}", ids::VIEW_DELETE_ROWS))
        .unwrap();
    assert_eq!(h.attr(item, "aria-disabled").as_deref(), Some("true"));
}

fn open_menu(h: &mut Harness) {
    let canvas = h
        .dom()
        .walk()
        .into_iter()
        .find(|k| {
            h.dom()
                .get(*k)
                .attr("class")
                .is_some_and(|c| c.contains("d0-grid-canvas"))
        })
        .unwrap();
    h.dispatch(canvas, "contextmenu", mouse_at(10.0, 10.0));
}

fn mouse_at(x: f64, y: f64) -> dioxus::html::SerializedMouseData {
    use dioxus::html::geometry::{ClientPoint, Coordinates, ElementPoint, PagePoint, ScreenPoint};
    use dioxus::html::input_data::MouseButton;
    let c = Coordinates::new(
        ScreenPoint::new(x, y),
        ClientPoint::new(x, y),
        ElementPoint::new(x, y),
        PagePoint::new(x, y),
    );
    dioxus::html::SerializedMouseData::new(
        Some(MouseButton::Secondary),
        MouseButton::Secondary.into(),
        c,
        Modifiers::empty(),
    )
}

fn form(value: &str) -> dioxus::html::SerializedFormData {
    dioxus::html::SerializedFormData::new(value.to_string(), Vec::new())
}
