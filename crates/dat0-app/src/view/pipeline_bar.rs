//! PipelineBar: visualizes the active transform stack (pills / timeline) with
//! a scrubber (jump-to-state) + per-transform remove. The pill LABEL logic is
//! pure + unit-tested; the GPUI render mounts on the WorkspaceShell.

use crate::a11y::A11yExt as _;
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
/// `expanded` is `false` by default; clicking `⌄` flips it to show the
/// vertical timeline; clicking `⌃` collapses back to the pill strip.
#[derive(Debug, Default, Clone)]
pub struct PipelineBarState {
    pub expanded: bool,
}

/// Render the PipelineBar — collapsed pill strip or expanded vertical timeline,
/// depending on `state.expanded`.
///
/// **Collapsed:** horizontal flex of pills: a leading `▣ base` chip (clickable
/// → `jump_to(0)`), then one chip per `describe_transform(t)` with `›`
/// separators, and a trailing `⌄` toggle. Clicking a pill calls
/// `ws.pipeline_jump_to(i + 1)`.
///
/// **Expanded:** a vertical list with one row per active transform. Each row
/// shows `[icon] describe_transform(t)` (clickable → `jump_to(i + 1)`) and a
/// trailing `✕` remove button (`ws.pipeline_remove_at(i)`). Above the rows a
/// `▣ base` entry (clickable → `jump_to(0)`) anchors the timeline. A `⌃`
/// toggle collapses back to the pill strip.
///
/// Returns `None` when the stack is empty (no bar shown until a transform is
/// applied), so the caller can use `.children(render_pipeline_bar(...))`.
pub fn render_pipeline_bar(
    stack: &[Transformation],
    state: &mut PipelineBarState,
    cx: &mut gpui::Context<crate::window::WorkspaceShell>,
) -> Option<gpui::AnyElement> {
    // Only render when there is at least one transform on the stack.
    if stack.is_empty() {
        return None;
    }

    use gpui::div;

    if state.expanded {
        // ── EXPANDED: vertical timeline ───────────────────────────────────────

        // `▣ base` row — clickable → jump_to(0)
        let base_row = div()
            .px_3()
            .py_1()
            .flex()
            .items_center()
            .gap_2()
            .cursor_pointer()
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgba(0x6b72_80ff)) // gray-500
                    .child("▣"),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(gpui::rgba(0x3b82_f6ff)) // blue-500
                    .child("base"),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|ws, _ev, _window, cx| {
                    ws.pipeline_jump_to(0, cx);
                }),
            );

        // One row per transform.
        let rows: Vec<gpui::AnyElement> = stack
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let label = describe_transform(t);
                let jump_k = i + 1;

                // Row body (icon + label) → jump_to(i+1)
                let row_body = div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .child(
                        div()
                            .text_sm()
                            .text_color(gpui::rgba(0x6b72_80ff)) // gray-500
                            .child("›"),
                    )
                    .child(div().text_sm().child(label))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(move |ws, _ev, _window, cx| {
                            ws.pipeline_jump_to(jump_k, cx);
                        }),
                    );

                // ✕ remove button → pipeline_remove_at(i)
                let remove_btn = div()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .text_sm()
                    .text_color(gpui::rgba(0xef44_44ff)) // red-500
                    .cursor_pointer()
                    .a11y_label(
                        crate::a11y::AccessRole::Label,
                        dat0_i18n::t("pipeline.remove_step"),
                    )
                    .child(gpui_component::Icon::new(gpui_component::IconName::Close))
                    .on_mouse_up(
                        gpui::MouseButton::Left,
                        cx.listener(move |ws, _ev, _window, cx| {
                            ws.pipeline_remove_at(i, cx);
                        }),
                    );

                div()
                    .px_3()
                    .py_0p5()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(row_body)
                    .child(remove_btn)
                    .into_any_element()
            })
            .collect();

        // Footer: `Save as Table…` pill (P5b T11) + `⌃` collapse toggle.
        let save_pill = div()
            .id("pipeline-save-table")
            .px_2()
            .py_0p5()
            .rounded_md()
            .text_sm()
            .bg(gpui::rgba(0x3b82_f640)) // blue-500/25
            .cursor_pointer()
            .child(gpui::SharedString::from(dat0_i18n::t("view.save_as_table")))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|ws, _ev, window, cx| {
                    ws.open_save_view_as_table(window, cx);
                }),
            );
        let collapse_btn = div()
            .px_2()
            .py_0p5()
            .rounded_md()
            .text_sm()
            .cursor_pointer()
            .child("⌃")
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|ws, _ev, _window, _cx| {
                    ws.pipeline_bar_state.expanded = false;
                }),
            );
        let toggle_btn = div()
            .px_3()
            .py_1()
            .flex()
            .justify_end()
            .items_center()
            .gap_2()
            .child(save_pill)
            .child(collapse_btn);

        let bar = div()
            .w_full()
            .border_b_1()
            .child(base_row)
            .children(rows)
            .child(toggle_btn)
            .into_any_element();

        Some(bar)
    } else {
        // ── COLLAPSED: horizontal pill strip ─────────────────────────────────

        // ▣ base chip — clickable → jump_to(0)
        let base_chip = div()
            .px_2()
            .py_0p5()
            .rounded_md()
            .text_sm()
            .bg(gpui::rgba(0x3b82_f640)) // blue-500/25
            .cursor_pointer()
            .child("▣ base")
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|ws, _ev, _window, cx| {
                    ws.pipeline_jump_to(0, cx);
                }),
            );

        // Transform pills
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
            let jump_k = i + 1;
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
                        ws.pipeline_jump_to(jump_k, cx);
                    }),
                );
            pill_children.push(pill.into_any_element());
        }

        // `Save as Table…` pill (P5b T11) — promotes the active transform stack
        // to a derived table via `create_table(.., DerivedOrigin::Transform)`.
        // The bar only renders when the stack is non-empty (callsite-guarded on
        // `view_model` + non-empty `active()`), so this pill is inherently
        // gated on an active view with at least one transform.
        let save_pill = div()
            .id("pipeline-save-table")
            .ml_2()
            .px_2()
            .py_0p5()
            .rounded_md()
            .text_sm()
            .bg(gpui::rgba(0x3b82_f640)) // blue-500/25
            .cursor_pointer()
            .child(gpui::SharedString::from(dat0_i18n::t("view.save_as_table")))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|ws, _ev, window, cx| {
                    ws.open_save_view_as_table(window, cx);
                }),
            );

        // `⌄` expand toggle
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
                cx.listener(|ws, _ev, _window, _cx| {
                    ws.pipeline_bar_state.expanded = true;
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
            .child(save_pill)
            .child(toggle_btn)
            .into_any_element();

        Some(bar)
    }
}
