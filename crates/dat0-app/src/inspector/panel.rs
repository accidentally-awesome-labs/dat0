//! Inspector right-dock panel (P6a T9). Renders the pure
//! [`InspectorModel`](crate::inspector::InspectorModel) as a header, an overview
//! line, a Whole-table⇄View toggle button, and one card per profiled column.
//!
//! Like [`crate::catalog::panel`] / [`crate::connections::panel`], this is a
//! *free function* — not a GPUI `Render`/`EventEmitter` entity — so the toggle
//! button's `on_click` can reach `WorkspaceShell` directly via
//! `cx.listener(|ws, …| …)` and call [`WorkspaceShell::toggle_inspector_mode`]
//! with no event plumbing. The render is a pure function of `model`; the cached
//! profile is loaded off-thread by `WorkspaceShell::load_inspector_profile`.
//!
//! All card *string* logic lives in [`crate::inspector::format`]; this module
//! only arranges divs. Inline charts are a separate task (T10) — each card
//! leaves room below the stat lines but contains no chart code yet.

use crate::inspector::{InspectorModel, ProfileTargetMode, format};
use crate::window::WorkspaceShell;
use gpui::prelude::*;
use gpui::{Context, SharedString, div};

/// Render the inspector dock from the current model. Called from
/// `WorkspaceShell::render`. A pure function of `model` (+ `cx` for the toggle
/// button listener).
pub fn render_inspector(
    model: &InspectorModel,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement {
    let mut root = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .child(div().child(SharedString::from(dat0_i18n::t("inspector.title"))));

    // Overview line: target name + (rows · cols) when cached, else a placeholder.
    let overview = match (&model.target_table, model.cached()) {
        (Some(name), Some(profile)) => format!(
            "{} — {} rows · {} cols",
            name,
            profile.rows,
            profile.columns.len()
        ),
        (Some(name), None) => format!("{} — {}", name, dat0_i18n::t("inspector.loading")),
        (None, _) => dat0_i18n::t("inspector.empty").to_string(),
    };
    root = root.child(div().child(SharedString::from(overview)));

    // Whole-table ⇄ View toggle. The label reflects the *current* mode; clicking
    // flips it and re-profiles (see `WorkspaceShell::toggle_inspector_mode`).
    let mode_label = match model.mode {
        ProfileTargetMode::WholeTable => dat0_i18n::t("inspector.mode.whole"),
        ProfileTargetMode::CurrentView => dat0_i18n::t("inspector.mode.view"),
    };
    root = root.child(
        div()
            .id("inspector-mode-toggle")
            .px_2()
            .py_1()
            .border_1()
            .cursor_pointer()
            .child(SharedString::from(mode_label))
            .on_click(cx.listener(|ws, _ev, window, cx| {
                ws.toggle_inspector_mode(window, cx);
            })),
    );

    // Per-column cards (only when a profile is cached).
    if let Some(profile) = model.cached() {
        let mut cards = div().flex().flex_col().gap_2();
        for col in &profile.columns {
            cards = cards.child(column_card(col));
        }
        root = root.child(cards);
    }

    root.into_any_element()
}

/// One column card: name · type, then the three formatted stat lines. T10 adds
/// an inline chart below these lines; for now the card is text-only.
fn column_card(col: &dat0_engine::ColumnProfile) -> gpui::Div {
    let header = format!("{} · {}", col.name, col.ty);
    let stats = format::format_stats_line(col);

    let mut card = div()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .border_1()
        .child(div().child(SharedString::from(header)));

    // `format_stats_line` is empty for columns with neither numeric nor length
    // stats (booleans, all-null, …); skip the empty line for those.
    if !stats.is_empty() {
        card = card.child(div().child(SharedString::from(stats)));
    }
    card = card.child(div().child(SharedString::from(format::format_distinct(col))));
    card = card.child(div().child(SharedString::from(format::format_null(col))));

    // T10: inline chart goes here (under the stat lines).
    card
}
