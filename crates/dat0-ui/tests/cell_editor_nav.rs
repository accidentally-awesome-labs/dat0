//! Editing a cell through the mounted grid: where the editor appears, what it
//! refuses, and what a commit actually does to the data.
//!
//! `tests/grid_edit.rs` covers the gestures — Enter opens, Enter commits and
//! steps down, Escape cancels, Tab and Shift-Tab step sideways. This file is
//! the rest of the GPUI suite's contract, which is about correctness rather
//! than grammar:
//!
//! * **The editor is over its cell.** The GPUI editor was a fixed-position
//!   overlay (`window/render.rs:199-231`) that appeared in the same place no
//!   matter which cell was being edited. `grid_edit.rs` pins one cell; this
//!   pins that the position *tracks* the cell, with uneven column widths so a
//!   left offset taken from the wrong column cannot pass.
//! * **A value the column cannot hold is refused.** Typing `abc` into a
//!   numeric column commits nothing, does not move the cursor, and leaves the
//!   editor open on the text to fix — GPUI's `parse_text` rejection, now
//!   `dat0_core::grid::edit_ops::parse_cell_text`.
//! * **A boolean is picked, not spelled.** GPUI mounted a `SelectState` here
//!   and could only assert the mount, because `set_selected_value` emitted no
//!   `Confirm`. A `<select>` can be driven, so the commit is covered too.
//! * **A commit reaches the data.** The one deep proof: the text the editor
//!   emitted, taken through the real `ViewModel` → engine → rebind path, reads
//!   back out of the grid.

mod support;

use std::sync::Arc;

use dioxus::prelude::*;
use tempfile::TempDir;

use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::edit_ops::parse_cell_text;
use dat0_core::grid::selection::SelectionModel;
use dat0_core::view::ViewModel;
use dat0_core::view::filter_popover::ColumnType;
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{
    CellEdit, DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts, RowKey, Scalar,
};
use dat0_ui::components::grid::{COL_W_DEFAULT, Grid, ROW_H};
use support::{Harness, Key, Modifiers};

// ── fixture ─────────────────────────────────────────────────────────────────

/// A three-row table with one column of each kind the editor treats
/// differently: numeric (parses or is refused), boolean (picked), text (takes
/// anything).
///
/// Registered from a CSV rather than created with `CREATE TABLE AS`, because
/// only `register_file_as_table` materialises the `__dat0_rowid` surrogate that
/// an `Edit` transform binds against — without it there is nothing to round
/// trip.
async fn base(tmp: &TempDir) -> (Arc<DuckDBEngine>, String) {
    let csv = tmp.path().join("cells.csv");
    std::fs::write(
        &csv,
        "n,flag,label\n1,true,alpha\n2,false,bravo\n3,true,charlie\n",
    )
    .unwrap();

    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file_as_table(&csv, RegisterOpts::default())
        .await
        .unwrap();
    (Arc::new(engine), info.name)
}

/// Bind a grid data source over `relation` with page 0 resident, so the first
/// render paints real values rather than the `—` placeholder.
async fn bind(
    engine: &Arc<DuckDBEngine>,
    relation: &str,
) -> (Arc<GridDataSource>, Vec<ProjectionColumn>) {
    let ds = GridDataSource::new(Arc::clone(engine), relation.to_string())
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

#[derive(Clone, Props)]
struct HostProps {
    source: Arc<GridDataSource>,
    columns: Vec<ProjectionColumn>,
    widths: Vec<f64>,
}

impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source)
            && self.columns == other.columns
            && self.widths == other.widths
    }
}

#[component]
fn Host(props: HostProps) -> Element {
    let rows = props.source.row_count as usize;
    let selection = use_signal(|| SelectionModel::new(rows, props.columns.len()));
    let widths = use_signal(|| props.widths.clone());
    let mut edits = use_signal(Vec::<String>::new);

    rsx! {
        Grid {
            source: props.source.clone(),
            selection,
            columns: props.columns.clone(),
            widths,
            on_edit: move |(c, v): (dat0_core::grid::selection::CellCoord, String)| {
                edits.write().push(format!("{},{}={v}", c.row, c.col));
            },
        }
        div { "data-a11y-id": "edits", "{edits.read().join(\"|\")}" }
        div { "data-a11y-id": "active", "{selection.read().active().row},{selection.read().active().col}" }
    }
}

async fn mount(engine: &Arc<DuckDBEngine>, relation: &str, widths: Vec<f64>) -> Harness {
    let (source, columns) = bind(engine, relation).await;
    Harness::new(
        Host,
        HostProps {
            source,
            columns,
            widths,
        },
    )
}

fn even() -> Vec<f64> {
    vec![COL_W_DEFAULT; 3]
}

/// A key at the grid viewport — the grid's own cursor grammar.
fn grid_key(h: &mut Harness, k: Key, mods: Modifiers) {
    let vp = h.by_a11y_id("grid-viewport").unwrap();
    h.key(vp, k, mods);
}

fn type_into_editor(h: &mut Harness, text: &str) {
    let ed = h.by_a11y_id("cell-editor").expect("the editor is open");
    h.dispatch(ed, "input", form(text));
}

fn form(value: &str) -> dioxus::html::SerializedFormData {
    dioxus::html::SerializedFormData::new(value.to_string(), Vec::new())
}

fn read(h: &Harness, id: &str) -> String {
    h.text_of(h.by_a11y_id(id).unwrap())
}

/// Move the cursor to `(row, col)` from the origin.
fn go(h: &mut Harness, row: usize, col: usize) {
    for _ in 0..row {
        grid_key(h, Key::ArrowDown, Modifiers::empty());
    }
    for _ in 0..col {
        grid_key(h, Key::ArrowRight, Modifiers::empty());
    }
    assert_eq!(read(h, "active"), format!("{row},{col}"));
}

// ── anchoring ───────────────────────────────────────────────────────────────

/// The GPUI editor was a fixed overlay: it appeared in the same place for
/// every cell, so editing row 40 put the box over row 1. Uneven widths here
/// are load-bearing — with three equal columns, an offset computed from the
/// wrong column index still lands somewhere plausible.
#[tokio::test]
async fn the_editor_follows_the_cell_rather_than_sitting_in_one_place() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;
    let widths = vec![70.0, 130.0, 90.0];
    let expected = [(0usize, 0.0, 70.0), (1, 70.0, 130.0), (2, 200.0, 90.0)];

    let mut seen = Vec::new();
    for (col, left, w) in expected {
        for row in [0usize, 2] {
            let mut h = mount(&engine, &table, widths.clone()).await;
            go(&mut h, row, col);
            grid_key(&mut h, Key::Enter, Modifiers::empty());

            let style = h
                .attr(h.by_a11y_id("cell-editor").unwrap(), "style")
                .unwrap_or_default();
            assert_eq!(
                style,
                format!(
                    "left: {left}px; top: {}px; width: {w}px;",
                    row as f64 * ROW_H
                ),
                "the editor for cell ({row},{col}) is not on it"
            );
            seen.push(style);
        }
    }
    // The defect stated positively: no two cells share a position.
    let unique: std::collections::BTreeSet<_> = seen.iter().collect();
    assert_eq!(
        unique.len(),
        seen.len(),
        "two different cells opened the editor in the same place"
    );
}

/// The editor also carries the cell's own value, so opening on row 2 does not
/// show row 0's text.
#[tokio::test]
async fn the_editor_opens_on_the_value_of_the_cell_it_covers() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;

    for (row, col, expect) in [(0usize, 0usize, "1"), (2, 0, "3"), (1, 2, "bravo")] {
        let mut h = mount(&engine, &table, even()).await;
        go(&mut h, row, col);
        grid_key(&mut h, Key::Enter, Modifiers::empty());
        assert_eq!(
            h.attr(h.by_a11y_id("cell-editor").unwrap(), "value")
                .as_deref(),
            Some(expect),
            "cell ({row},{col})"
        );
    }
}

// ── refusing a value the column cannot hold ─────────────────────────────────

#[tokio::test]
async fn a_number_column_refuses_text_and_keeps_the_editor_open() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;
    let mut h = mount(&engine, &table, even()).await;

    grid_key(&mut h, Key::Enter, Modifiers::empty());
    type_into_editor(&mut h, "abc");
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.key(ed, Key::Enter, Modifiers::empty());

    assert_eq!(read(&h, "edits"), "", "nothing may be written");
    assert_eq!(
        read(&h, "active"),
        "0,0",
        "a refused commit must not move the cursor"
    );
    let ed = h
        .by_a11y_id("cell-editor")
        .expect("the editor stays open so the value can be fixed");
    assert_eq!(h.attr(ed, "aria-invalid").as_deref(), Some("true"));
    assert!(
        h.by_a11y_id("cell-editor-invalid").is_some(),
        "an editor that refuses silently looks broken"
    );
}

/// The positive control the GPUI suite needed a whole extra test for: if the
/// refusal above were a dead branch that swallowed every commit, this would
/// fail too.
#[tokio::test]
async fn correcting_the_value_lets_the_same_commit_through() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;
    let mut h = mount(&engine, &table, even()).await;

    grid_key(&mut h, Key::Enter, Modifiers::empty());
    type_into_editor(&mut h, "abc");
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.key(ed, Key::Enter, Modifiers::empty());
    assert_eq!(read(&h, "edits"), "");

    // Typing is the correction, so the complaint clears with it.
    type_into_editor(&mut h, "99");
    assert_eq!(
        h.attr(h.by_a11y_id("cell-editor").unwrap(), "aria-invalid")
            .as_deref(),
        Some("false")
    );
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.key(ed, Key::Enter, Modifiers::empty());

    assert_eq!(read(&h, "edits"), "0,0=99");
    assert_eq!(read(&h, "active"), "1,0");
}

/// Blur is the weakest commit gesture, and it went through the same parse in
/// GPUI. A click away must not be a back door for a value Enter refused.
#[tokio::test]
async fn clicking_away_from_a_refused_value_writes_nothing_either() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;
    let mut h = mount(&engine, &table, even()).await;

    grid_key(&mut h, Key::Enter, Modifiers::empty());
    type_into_editor(&mut h, "not a number");
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.dispatch(ed, "blur", dioxus::html::SerializedFocusData {});

    assert_eq!(read(&h, "edits"), "");
    assert!(h.by_a11y_id("cell-editor").is_some());
}

/// Tab commits, so it is refused on the same terms as Enter — otherwise the
/// sideways step is a way to smuggle a bad value in.
#[tokio::test]
async fn tab_is_refused_on_the_same_terms_as_enter() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;
    let mut h = mount(&engine, &table, even()).await;

    grid_key(&mut h, Key::Enter, Modifiers::empty());
    type_into_editor(&mut h, "abc");
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.key(ed, Key::Tab, Modifiers::empty());

    assert_eq!(read(&h, "edits"), "");
    assert_eq!(read(&h, "active"), "0,0");
    assert!(h.by_a11y_id("cell-editor").is_some());
}

/// A text column has nothing to refuse, so the same keystrokes commit.
#[tokio::test]
async fn a_text_column_takes_the_value_a_number_column_refused() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;
    let mut h = mount(&engine, &table, even()).await;

    go(&mut h, 0, 2);
    grid_key(&mut h, Key::Enter, Modifiers::empty());
    type_into_editor(&mut h, "abc");
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.key(ed, Key::Enter, Modifiers::empty());

    assert_eq!(read(&h, "edits"), "0,2=abc");
}

// ── PD-020: Tab walks the row ───────────────────────────────────────────────

/// Tab moves the active cell, and stops at the edges rather than wrapping onto
/// another row or out of the grid entirely.
#[tokio::test]
async fn tab_walks_the_row_and_stops_at_its_ends() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;

    // Right edge: the label column is last, so a commit there stays put.
    let mut h = mount(&engine, &table, even()).await;
    go(&mut h, 1, 2);
    grid_key(&mut h, Key::Enter, Modifiers::empty());
    type_into_editor(&mut h, "edge");
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.key(ed, Key::Tab, Modifiers::empty());
    assert_eq!(read(&h, "edits"), "1,2=edge", "the commit still happens");
    assert_eq!(
        read(&h, "active"),
        "1,2",
        "Tab past the last column must not wrap onto the next row"
    );

    // Left edge.
    let mut h = mount(&engine, &table, even()).await;
    grid_key(&mut h, Key::Enter, Modifiers::empty());
    type_into_editor(&mut h, "7");
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.key(ed, Key::Tab, Modifiers::SHIFT);
    assert_eq!(read(&h, "edits"), "0,0=7");
    assert_eq!(read(&h, "active"), "0,0");
}

// ── the boolean picker ──────────────────────────────────────────────────────

/// GPUI's proof of the second widget path, ported: a bool column must not put
/// a text box over the cell.
#[tokio::test]
async fn a_bool_column_opens_a_picker_not_a_text_box() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;
    let mut h = mount(&engine, &table, even()).await;

    go(&mut h, 0, 1);
    grid_key(&mut h, Key::Enter, Modifiers::empty());
    let ed = h.by_a11y_id("cell-editor").expect("the editor opened");
    assert_eq!(h.dom().get(ed).tag(), Some("select"));

    // And the other columns still do not.
    let mut h = mount(&engine, &table, even()).await;
    grid_key(&mut h, Key::Enter, Modifiers::empty());
    let ed = h.by_a11y_id("cell-editor").unwrap();
    assert_eq!(h.dom().get(ed).tag(), Some("input"));
}

/// New coverage. The GPUI suite explicitly gave this up — `set_selected_value`
/// did not emit `Confirm`, so no headless test could drive the confirm and the
/// commit stayed on a unit test of the parser. A `<select>` has an `onchange`.
#[tokio::test]
async fn picking_a_boolean_commits_it_and_walks_down() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;
    let mut h = mount(&engine, &table, even()).await;

    go(&mut h, 0, 1);
    grid_key(&mut h, Key::Enter, Modifiers::empty());
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.dispatch(ed, "change", form("false"));

    assert_eq!(read(&h, "edits"), "0,1=false");
    assert_eq!(read(&h, "active"), "1,1");
    assert!(h.by_a11y_id("cell-editor").is_none());
}

/// Escape out of the picker leaves the cell as it was, the same as out of the
/// text box.
#[tokio::test]
async fn escaping_the_picker_changes_nothing() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;
    let mut h = mount(&engine, &table, even()).await;

    go(&mut h, 0, 1);
    grid_key(&mut h, Key::Enter, Modifiers::empty());
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.key(ed, Key::Escape, Modifiers::empty());

    assert_eq!(read(&h, "edits"), "");
    assert_eq!(read(&h, "active"), "0,1");
    assert!(h.by_a11y_id("cell-editor").is_none());
}

// ── the deep proof ──────────────────────────────────────────────────────────

/// The one end-to-end run: what the editor emitted, applied through the real
/// `ViewModel` → `Transformation::Edit` → engine view, reads back out of a
/// grid bound to that view.
///
/// The write is applied in the test body rather than inside `on_edit` because
/// the grid deliberately does not own it — "the grid never touches the engine
/// itself, one place decides what a write means" — so the shell's job is
/// modelled here explicitly: take the emitted `(cell, text)`, resolve the row's
/// surrogate key and the column's source name, parse the text for the column's
/// type, and hand the engine one `CellEdit`.
#[tokio::test]
async fn a_committed_value_reaches_the_data_and_reads_back() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;
    let (source, columns) = bind(&engine, &table).await;

    let mut h = Harness::new(
        Host,
        HostProps {
            source: Arc::clone(&source),
            columns: columns.clone(),
            widths: even(),
        },
    );
    assert_eq!(
        h.text_of(h.by_a11y_id("cell-0-0").unwrap()),
        "1",
        "seed: the first cell is 1 before the edit"
    );

    grid_key(&mut h, Key::Enter, Modifiers::empty());
    type_into_editor(&mut h, "42");
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.key(ed, Key::Enter, Modifiers::empty());
    assert_eq!(read(&h, "edits"), "0,0=42");

    // What the shell does with that: one typed CellEdit against the row's
    // surrogate key.
    let row_id = source
        .row_key(0)
        .expect("the base table carries a surrogate");
    let column = columns[0].source.clone();
    let value = parse_cell_text(ColumnType::Numeric, "42").expect("42 is a number");
    let mut vm = ViewModel::new("tab".into(), format!("\"{}\"", table.replace('"', "\"\"")));
    let change = vm.edit_cells(vec![CellEdit {
        row: RowKey::Surrogate { id: row_id },
        column,
        value,
    }]);
    let view = change.new_active_view.clone().expect("an edit rebinds");
    engine
        .create_or_replace_view(&view, change.sql.as_ref().expect("edit SQL"))
        .await
        .unwrap();

    // Rebind the grid to the view the edit produced.
    let (rebound, rebound_cols) = bind(&engine, &view).await;
    let h = Harness::new(
        Host,
        HostProps {
            source: rebound,
            columns: rebound_cols,
            widths: even(),
        },
    );
    assert_eq!(
        h.text_of(h.by_a11y_id("cell-0-0").unwrap()),
        "42",
        "the edited value must survive the engine round trip"
    );
    assert_eq!(
        h.text_of(h.by_a11y_id("cell-1-0").unwrap()),
        "2",
        "and must not have touched any other row"
    );
}

/// The bool commit reaches the data too, on the same path — as a
/// `Scalar::Bool`, not the string `"false"`, which is the difference between
/// a boolean column and a corrupted one.
///
/// Read back through the engine rather than the grid: `render_cell` has no
/// `DataType::Boolean` arm, so every boolean cell paints the literal
/// `(Boolean)` — a pre-existing dat0 limitation carried over unchanged from
/// the GPUI build, and one this assertion must not depend on either way.
#[tokio::test]
async fn a_picked_boolean_reaches_the_data_as_a_boolean() {
    let tmp = TempDir::new().unwrap();
    let (engine, table) = base(&tmp).await;
    let (source, columns) = bind(&engine, &table).await;

    let mut h = Harness::new(
        Host,
        HostProps {
            source: Arc::clone(&source),
            columns: columns.clone(),
            widths: even(),
        },
    );
    go(&mut h, 0, 1);
    grid_key(&mut h, Key::Enter, Modifiers::empty());
    let ed = h.by_a11y_id("cell-editor").unwrap();
    h.dispatch(ed, "change", form("false"));
    assert_eq!(read(&h, "edits"), "0,1=false");

    let row_id = source.row_key(0).unwrap();
    let value = parse_cell_text(ColumnType::Bool, "false").expect("false is a boolean");
    assert_eq!(value, Scalar::Bool(false), "not the string \"false\"");
    let mut vm = ViewModel::new("tab".into(), format!("\"{}\"", table.replace('"', "\"\"")));
    let change = vm.edit_cells(vec![CellEdit {
        row: RowKey::Surrogate { id: row_id },
        column: columns[1].source.clone(),
        value,
    }]);
    let view = change.new_active_view.clone().unwrap();
    engine
        .create_or_replace_view(&view, change.sql.as_ref().unwrap())
        .await
        .unwrap();

    let count = |sql: String| {
        let engine = Arc::clone(&engine);
        async move {
            engine
                .execute_paged(&sql, 0, 100)
                .await
                .unwrap()
                .batches
                .iter()
                .map(|b| b.num_rows())
                .sum::<usize>()
        }
    };
    assert_eq!(
        count(format!("SELECT n FROM \"{view}\" WHERE flag = false")).await,
        2,
        "row 1 was already false and row 0 has just been flipped"
    );
    assert_eq!(
        count(format!("SELECT n FROM \"{view}\" WHERE flag = true")).await,
        1,
        "and the third row is untouched"
    );
}
