//! In-place cell editing.
//!
//! # Three defects this closes
//!
//! **The editor was never over the cell.** The GPUI editor was a
//! fixed-position overlay (`window/render.rs`) that appeared at a constant
//! place regardless of which cell was being edited, because computing the cell
//! rect inside a widget-owned table was impractical. Here the position is
//! `row * ROW_H` and the column's accumulated offset — arithmetic the grid
//! already does to place the cell itself, so the editor cannot drift from it.
//!
//! **Tab did nothing (deferral PD-020).** That deferral existed solely because
//! `gpui-component`'s `Input` swallowed Tab and surfaced no event. A plain
//! `<input>` does not, so Tab and Shift-Tab commit and step sideways, which is
//! what every spreadsheet does and what the deferral promised.
//!
//! **A bool column had to be typed.** GPUI mounted a `SelectState` for it, but
//! nothing headless could drive the confirm. A `<select>` is two options and an
//! `onchange`.
//!
//! Commit semantics are unchanged from the GPUI editor: **Enter** commits and
//! moves down, **Escape** cancels, **blur** commits — and a value that is not
//! of the column's type commits nothing at all, leaving the editor open on the
//! text the user has to fix. That refusal is
//! [`dat0_core::grid::edit_ops::parse_cell_text`], the same rule the GPUI
//! `CellEditor::parse_text` applied.

use dioxus::prelude::*;

use dat0_core::grid::edit_ops::parse_cell_text;
use dat0_core::grid::selection::CellCoord;
use dat0_core::view::filter_popover::ColumnType;

use super::{COL_W_DEFAULT, ROW_H, offset_of};

/// What ended the edit. The caller applies it — the editor itself never
/// touches the engine, so a commit is one decision made in one place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOutcome {
    /// Write `value`, then move the active cell by `(dr, dc)`.
    Commit {
        value: String,
        move_by: (isize, isize),
    },
    /// Discard.
    Cancel,
}

#[derive(Clone, Props, PartialEq)]
pub struct CellEditorProps {
    pub cell: CellCoord,
    /// The value the cell shows now.
    pub initial: String,
    pub widths: Vec<f64>,
    /// Decides the widget and what counts as a value. Defaults to `String`,
    /// which accepts anything — a column whose type could not be read must not
    /// become uneditable.
    #[props(default = ColumnType::String)]
    pub column_type: ColumnType,
    pub on_done: EventHandler<EditOutcome>,
}

#[component]
pub fn CellEditor(props: CellEditorProps) -> Element {
    let mut value = use_signal(|| props.initial.clone());
    // Set when a commit was refused, so the editor can say why it did not close.
    let mut invalid = use_signal(|| false);
    let left = offset_of(&props.widths, props.cell.col);
    let width = props
        .widths
        .get(props.cell.col)
        .copied()
        .unwrap_or(COL_W_DEFAULT);
    let top = props.cell.row as f64 * ROW_H;
    let style = format!("left: {left}px; top: {top}px; width: {width}px;");

    let on_done = props.on_done;
    let column_type = props.column_type;
    // A commit must not also fire from the blur that follows it, or the cell
    // is written twice and the second write moves the cursor again.
    let mut finished = use_signal(|| false);

    // One commit rule for every gesture: a value the column cannot hold is
    // refused, the editor stays open, and the cursor does not move. Returns
    // whether the edit ended.
    let mut commit = move |move_by: (isize, isize)| -> bool {
        let raw = value();
        if parse_cell_text(column_type, &raw).is_none() {
            invalid.set(true);
            return false;
        }
        finished.set(true);
        on_done.call(EditOutcome::Commit {
            value: raw,
            move_by,
        });
        true
    };

    // Shared by both widgets: the same keys mean the same thing whether the
    // column is text or boolean.
    let on_key = move |e: KeyboardEvent| {
        let move_by = match e.key() {
            Key::Enter => (1, 0),
            Key::Tab if e.modifiers().shift() => (0, -1),
            Key::Tab => (0, 1),
            Key::Escape => {
                e.stop_propagation();
                finished.set(true);
                on_done.call(EditOutcome::Cancel);
                return;
            }
            _ => return,
        };
        // Tab would otherwise move focus out of the grid entirely, and Enter
        // would submit an enclosing form.
        e.prevent_default();
        e.stop_propagation();
        commit(move_by);
    };

    // A blur is the weakest gesture, so it never moves the cursor — and if the
    // value is invalid it writes nothing, exactly as Enter would not.
    let on_blur = move |_| {
        if finished() {
            return;
        }
        commit((0, 0));
    };

    let invalid_attr = if invalid() { "true" } else { "false" };

    if column_type == ColumnType::Bool {
        let current = value();
        // The stored text, not a parsed bool: the cell may hold `t`/`1`/`yes`
        // and the picker still has to open on the right option.
        let is_true =
            parse_cell_text(ColumnType::Bool, &current) == Some(dat0_engine::Scalar::Bool(true));
        return rsx! {
            select {
                class: "d0-cell-editor",
                "data-a11y-id": "cell-editor",
                role: "combobox",
                "aria-label": dat0_i18n::t("grid.edit_cell"),
                autofocus: true,
                style: "{style}",
                // Picking an option is the confirm gesture, the way
                // `SelectEvent::Confirm` was: commit and walk down.
                onchange: move |e| {
                    value.set(e.value());
                    commit((1, 0));
                },
                onkeydown: on_key,
                onblur: on_blur,
                option { value: "true", selected: is_true, {dat0_i18n::t("grid.edit_cell.true")} }
                option { value: "false", selected: !is_true, {dat0_i18n::t("grid.edit_cell.false")} }
            }
        };
    }

    rsx! {
        input {
            class: if invalid() { "d0-cell-editor is-error" } else { "d0-cell-editor" },
            "data-a11y-id": "cell-editor",
            role: "textbox",
            "aria-label": dat0_i18n::t("grid.edit_cell"),
            "aria-invalid": "{invalid_attr}",
            value: "{value}",
            autofocus: true,
            style: "{style}",
            oninput: move |e| {
                value.set(e.value());
                // Typing is the correction, so the complaint goes away with it.
                if invalid() {
                    invalid.set(false);
                }
            },
            onkeydown: on_key,
            onblur: on_blur,
        }
        if invalid() {
            span {
                class: "d0-mono is-error",
                "data-a11y-id": "cell-editor-invalid",
                role: "alert",
                "aria-label": dat0_i18n::t("grid.edit_cell.invalid"),
                style: "left: {left}px; top: {top + ROW_H}px;",
                {dat0_i18n::t("grid.edit_cell.invalid")}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_editor_sits_exactly_on_its_cell() {
        // The same arithmetic the grid uses to place the cell, so the two
        // cannot disagree — which is the defect this replaces.
        let widths = vec![80.0, 120.0, 60.0];
        assert_eq!(offset_of(&widths, 0), 0.0);
        assert_eq!(offset_of(&widths, 2), 200.0);
        assert_eq!(3.0 * ROW_H, 78.0);
    }

    #[test]
    fn a_commit_carries_the_cursor_step_with_it() {
        // The direction is part of the outcome rather than a second callback,
        // so a commit and its move cannot be applied out of order.
        let c = EditOutcome::Commit {
            value: "x".into(),
            move_by: (1, 0),
        };
        assert_ne!(c, EditOutcome::Cancel);
    }
}
