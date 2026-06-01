//! PipelineBar: visualizes the active transform stack (pills / timeline) with
//! a scrubber (jump-to-state) + per-transform remove. The pill LABEL logic is
//! pure + unit-tested; the GPUI render mounts on the WorkspaceShell.

use dat0_engine::transform::{SortDirection, Transformation};
use gpui::{IntoElement, prelude::*};
use gpui_component::h_flex;

/// Human-readable one-line label for a pill / timeline row.
pub fn describe_transform(t: &Transformation) -> String {
    match t {
        Transformation::Filter { column, .. } => format!("Filter {column}"),
        Transformation::Sort { keys } => {
            if let Some(k) = keys.first() {
                let arrow = match k.direction {
                    SortDirection::Asc => "↑",
                    SortDirection::Desc => "↓",
                };
                let more = if keys.len() > 1 {
                    format!(" +{}", keys.len() - 1)
                } else {
                    String::new()
                };
                format!("Sort {}{}{}", k.column, arrow, more)
            } else {
                "Sort".into()
            }
        }
        Transformation::Edit { cells } => format!("Edit {} cell(s)", cells.len()),
        Transformation::RowDelete { rows } => format!("Delete {} row(s)", rows.len()),
        Transformation::Reorder { .. } => "Reorder columns".into(),
        Transformation::Rename { column, to } => format!("Rename {column}→{to}"),
        Transformation::DeleteColumn { columns } => {
            format!("Delete col {}", columns.join(", "))
        }
    }
}

/// State for the PipelineBar toggle (expanded vs. collapsed).
///
/// `expanded` is `false` by default; clicking `⌄` flips it. The expanded
/// timeline view is T10 — the stub is stored here but only the collapsed strip
/// is rendered.
#[derive(Debug, Default, Clone)]
pub struct PipelineBarState {
    pub expanded: bool,
}

/// Render the collapsed PipelineBar pill strip.
///
/// Returns a horizontal flex of pills: a leading `▣ base` chip, then one chip
/// per `describe_transform(t)` with `›` separators, and a trailing `⌄` toggle.
///
/// Each transform pill is `cursor_pointer`; clicking it calls
/// `ws.pipeline_jump_to(i + 1)` via the weak handle so that pill becomes the
/// last applied transform. The `⌄` button flips `state.expanded` (the expanded
/// timeline is T10).
///
/// Returns `None` when the stack is empty (no bar shown until a transform is
/// applied), so the caller can use `.children(render_pipeline_bar(...))`.
pub fn render_pipeline_bar(
    stack: &[Transformation],
    state: &mut PipelineBarState,
    ws_weak: gpui::WeakEntity<crate::window::WorkspaceShell>,
    cx: &mut gpui::Context<crate::window::WorkspaceShell>,
) -> Option<gpui::AnyElement> {
    // Only render when there is at least one transform on the stack.
    if stack.is_empty() {
        return None;
    }

    use gpui::div;

    // ── base chip ────────────────────────────────────────────────────────────
    let base_chip = div()
        .px_2()
        .py_0p5()
        .rounded_md()
        .text_sm()
        .bg(gpui::rgba(0x3b82_f640)) // blue-500/25
        .child("▣ base");

    // ── transform pills ───────────────────────────────────────────────────────
    let mut pill_children: Vec<gpui::AnyElement> = Vec::new();
    pill_children.push(base_chip.into_any_element());

    for (i, t) in stack.iter().enumerate() {
        // Separator `›`
        pill_children.push(
            div()
                .px_1()
                .text_sm()
                .text_color(gpui::rgba(0x6b72_80ff)) // gray-500
                .child("›")
                .into_any_element(),
        );

        let label = describe_transform(t);
        let ws_weak_clone = ws_weak.clone();
        let jump_k = i + 1; // clicking pill i makes it the last applied (0-based → 1-based)
        let pill = div()
            .px_2()
            .py_0p5()
            .rounded_md()
            .text_sm()
            .bg(gpui::rgba(0xf3f4_f6ff)) // gray-100
            .cursor_pointer()
            .child(label)
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(move |ws, _ev, _window, cx| {
                    let _ = &ws_weak_clone; // silence unused-capture lint
                    ws.pipeline_jump_to(jump_k, cx);
                }),
            );
        pill_children.push(pill.into_any_element());
    }

    // ── trailing `⌄` toggle (stub — expanded view is T10) ────────────────────
    let _is_expanded = state.expanded; // stored but not yet used to switch views (T10)
    let ws_weak_for_toggle = ws_weak.clone();
    let toggle_btn = div()
        .ml_2()
        .px_2()
        .py_0p5()
        .rounded_md()
        .text_sm()
        .cursor_pointer()
        .child("⌄")
        .on_mouse_up(
            gpui::MouseButton::Left,
            cx.listener(move |ws, _ev, _window, _cx| {
                let _ = &ws_weak_for_toggle; // silence unused-capture lint
                ws.pipeline_bar_state.expanded = !ws.pipeline_bar_state.expanded;
                // T10 will render the expanded timeline here; stub for now.
            }),
        );

    let bar = h_flex()
        .w_full()
        .px_3()
        .py_1()
        .border_b_1()
        .gap_0p5()
        .items_center()
        .children(pill_children)
        .child(toggle_btn)
        .into_any_element();

    Some(bar)
}
