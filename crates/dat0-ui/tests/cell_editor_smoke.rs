//! What the cell editor is, before anything drives it: the right widget for
//! the column, the cell's value already in it, every gesture wired, and the
//! caret in it on arrival.
//!
//! The GPUI suite this replaces could only assert construction. `InputState`
//! and `SelectState` both needed a `&mut Window` no headless test could
//! produce, so the widgets were lazy-built on the first `render()` and the
//! tests were reduced to "does `new` panic" plus a count of stored
//! `Subscription` handles standing in for "are the callbacks alive". A
//! `<input>` and a `<select>` need none of that: the widget is in the tree on
//! the first pass and its listeners are visible, so the stand-in becomes the
//! real assertion.

mod support;

use dioxus::prelude::*;

use dat0_core::grid::selection::CellCoord;
use dat0_core::view::filter_popover::ColumnType;
use dat0_ui::components::grid::ROW_H;
use dat0_ui::components::grid::cell_editor::{CellEditor, EditOutcome};
use support::Harness;

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    column_type: ColumnType,
    initial: String,
    cell: CellCoord,
}

/// Mounts the editor alone and reports its outcome as text, because the
/// harness reads a tree, not Rust state.
#[component]
fn Host(props: HostProps) -> Element {
    let mut done = use_signal(Vec::<String>::new);
    rsx! {
        CellEditor {
            cell: props.cell,
            initial: props.initial.clone(),
            column_type: props.column_type,
            widths: vec![80.0, 120.0, 60.0],
            on_done: move |o: EditOutcome| {
                done.write().push(match o {
                    EditOutcome::Commit { value, move_by } => {
                        format!("commit {value} {},{}", move_by.0, move_by.1)
                    }
                    EditOutcome::Cancel => "cancel".to_string(),
                });
            },
        }
        div { "data-a11y-id": "done", "{done.read().join(\"|\")}" }
    }
}

fn editor(column_type: ColumnType, initial: &str) -> Harness {
    Harness::new(
        Host,
        HostProps {
            column_type,
            initial: initial.to_string(),
            cell: CellCoord { row: 0, col: 0 },
        },
    )
}

/// Every column type gets an editor.
#[test]
fn no_column_type_is_left_uneditable() {
    for ct in [
        ColumnType::Numeric,
        ColumnType::String,
        ColumnType::Bool,
        ColumnType::Date,
        ColumnType::Timestamp,
    ] {
        let h = editor(ct, "");
        assert!(
            h.by_a11y_id("cell-editor").is_some(),
            "{ct:?} must mount an editor"
        );
    }
}

/// A boolean is picked, not typed; everything else is typed.
#[test]
fn the_column_type_decides_the_widget() {
    let bool_tag = {
        let h = editor(ColumnType::Bool, "true");
        h.dom()
            .get(h.by_a11y_id("cell-editor").unwrap())
            .tag()
            .unwrap()
            .to_string()
    };
    assert_eq!(
        bool_tag, "select",
        "a bool column offers its two values rather than asking the user to \
         spell one of them"
    );

    for ct in [
        ColumnType::Numeric,
        ColumnType::String,
        ColumnType::Date,
        ColumnType::Timestamp,
    ] {
        let h = editor(ct, "");
        let tag = h
            .dom()
            .get(h.by_a11y_id("cell-editor").unwrap())
            .tag()
            .unwrap()
            .to_string();
        assert_eq!(tag, "input", "{ct:?} is typed");
    }
}

/// The picker offers exactly the two values a boolean has, and opens on the
/// one the cell holds.
#[test]
fn the_bool_picker_offers_both_values_and_opens_on_the_current_one() {
    for (stored, expect_true) in [("true", true), ("false", false), ("t", true), ("0", false)] {
        let h = editor(ColumnType::Bool, stored);
        let select = h.by_a11y_id("cell-editor").unwrap();
        let options: Vec<_> = h
            .dom()
            .get(select)
            .children
            .iter()
            .map(|c| {
                let n = h.dom().get(*c);
                (
                    n.attr("value").unwrap_or_default().to_string(),
                    n.attr("selected").map(str::to_string),
                )
            })
            .collect();
        assert_eq!(options.len(), 2, "a boolean has two values");
        assert_eq!(options[0].0, "true");
        assert_eq!(options[1].0, "false");
        let selected_true = options[0].1.as_deref() == Some("true");
        assert_eq!(
            selected_true, expect_true,
            "{stored:?} must open the picker on {expect_true}"
        );
    }
}

/// The editor starts on the cell's current value, so an edit is a correction
/// rather than a retype.
#[test]
fn the_editor_opens_holding_the_cells_value() {
    let h = editor(ColumnType::String, "alpha");
    assert_eq!(
        h.attr(h.by_a11y_id("cell-editor").unwrap(), "value")
            .as_deref(),
        Some("alpha")
    );
}

/// Replaces the GPUI `subscription_count()` guard. There the handles were
/// counted because the callbacks were invisible and a dropped `Subscription`
/// silently deregistered; here the listeners are in the tree, so the guard is
/// against the real thing rather than a proxy for it.
#[test]
fn every_gesture_that_ends_an_edit_is_wired() {
    let text = editor(ColumnType::String, "x");
    let input = text.by_a11y_id("cell-editor").unwrap();
    for ev in ["input", "keydown", "blur"] {
        assert!(
            text.has_listener(input, ev),
            "a text editor with no {ev} listener silently discards edits"
        );
    }

    let boolean = editor(ColumnType::Bool, "true");
    let select = boolean.by_a11y_id("cell-editor").unwrap();
    for ev in ["change", "keydown", "blur"] {
        assert!(
            boolean.has_listener(select, ev),
            "a bool editor with no {ev} listener silently discards edits"
        );
    }
}

/// The GPUI editor carried a `focus_handle()` so it could take the caret on
/// mount; the accessor's stability was all a headless test could check. Here
/// the guarantee is one attribute, and it is the guarantee itself: an editor
/// that opens without focus makes the user click into the box they just asked
/// for.
#[test]
fn the_editor_takes_the_caret_on_arrival() {
    for ct in [ColumnType::String, ColumnType::Bool] {
        let h = editor(ct, "true");
        assert_eq!(
            h.attr(h.by_a11y_id("cell-editor").unwrap(), "autofocus")
                .as_deref(),
            Some("true"),
            "{ct:?}"
        );
    }
}

/// A screen reader must be told what the box over the grid is.
#[test]
fn the_editor_names_itself_to_a_reader() {
    let h = editor(ColumnType::String, "x");
    assert!(h.has_label(&dat0_i18n::t("grid.edit_cell")));
}

/// Position comes from the cell's coordinates, so an editor mounted on a
/// different cell lands somewhere else. Column widths here are uneven on
/// purpose: with equal widths a left offset that used the wrong column would
/// still land in the right place.
#[test]
fn the_editor_is_placed_from_the_cell_it_edits() {
    for (col, left, width) in [(0usize, 0.0, 80.0), (1, 80.0, 120.0), (2, 200.0, 60.0)] {
        for row in [0usize, 3, 17] {
            let h = Harness::new(
                Host,
                HostProps {
                    column_type: ColumnType::String,
                    initial: String::new(),
                    cell: CellCoord { row, col },
                },
            );
            let style = h
                .attr(h.by_a11y_id("cell-editor").unwrap(), "style")
                .unwrap_or_default();
            assert_eq!(
                style,
                format!(
                    "left: {left}px; top: {}px; width: {width}px;",
                    row as f64 * ROW_H
                ),
                "cell ({row},{col})"
            );
        }
    }
}
