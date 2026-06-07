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

use crate::inspector::lineage::{ChainStep, EdgeKind, NodeKind};
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

    // Lineage chain (P6b): ancestors↑, the inspected table, descendants↓ — the
    // full transitive closure. Replaces the P6a flat Dependents list. Clicking a
    // node opens that table (which re-roots the Inspector via open_table_tab).
    if let Some(target) = model.target_table.clone() {
        let mut section = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().child(SharedString::from(dat0_i18n::t("inspector.lineage"))));

        // Ancestors (roots → parent), each indented by depth.
        if !model.lineage.ancestors.is_empty() {
            section = section.child(div().child(SharedString::from(dat0_i18n::t(
                "inspector.lineage.sources",
            ))));
            for step in &model.lineage.ancestors {
                section = section.child(chain_row(step, cx));
            }
        }

        // The inspected table itself (highlighted, not clickable).
        section = section.child(
            div()
                .px_1()
                .border_1()
                .child(SharedString::from(format!("▸ {target}"))),
        );

        // Descendants (child → leaf).
        let usedby = if model.lineage.descendants.is_empty() {
            dat0_i18n::t("inspector.lineage.none")
        } else {
            dat0_i18n::t("inspector.lineage.usedby")
        };
        section = section.child(div().child(SharedString::from(usedby)));
        for step in &model.lineage.descendants {
            section = section.child(chain_row(step, cx));
        }

        root = root.child(section);
    }

    // Per-column cards (only when a profile is cached).
    if let Some(profile) = model.cached() {
        let mut cards = div().flex().flex_col().gap_2();
        for col in &profile.columns {
            cards = cards.child(column_card(col, model));
        }
        root = root.child(cards);
    }

    root.into_any_element()
}

/// One column card: name · type, then the three formatted stat lines, then —
/// when its lazy data has landed (T10) — an inline chart: top-N bars for
/// low-cardinality columns, a histogram for numeric high-cardinality ones.
fn column_card(col: &dat0_engine::ColumnProfile, model: &InspectorModel) -> gpui::Div {
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

    // Inline chart (T10): top-N bars for low-card, histogram for numeric — only
    // when its lazy data has been fetched (see `load_column_extras`).
    if let Some(extra) = model.extra(&col.name) {
        if let Some(topn) = &extra.topn {
            card = card.child(crate::charts::render_topn(topn));
        } else if let Some(bins) = &extra.histogram {
            card = card.child(crate::charts::render_histogram(bins));
        }
    }
    card
}

/// A short, human-readable label for a lineage edge.
fn edge_label(edge: &EdgeKind) -> String {
    match edge {
        EdgeKind::FileImport => dat0_i18n::t("inspector.edge.file"),
        EdgeKind::SqlRef => dat0_i18n::t("inspector.edge.sql"),
        EdgeKind::Transform(n) => format!("{} ({n} ops)", dat0_i18n::t("inspector.edge.transform")),
    }
}

/// One lineage row: a per-kind glyph, the node label, and the edge label.
/// Clickable (opens + re-roots the Inspector) when the node maps to a table.
/// Every row is `Stateful` (carries an `.id()`) so the clickable/leaf branches
/// share one return type; only table-backed rows wire the `on_click`.
fn chain_row(step: &ChainStep, cx: &mut Context<WorkspaceShell>) -> gpui::Stateful<gpui::Div> {
    let glyph = match step.kind {
        NodeKind::File => "📄",
        NodeKind::External => "☁",
        NodeKind::Table => "▦",
    };
    let indent = (step.depth.min(6) as f32) * 12.0;
    let text = format!("{glyph} {}  ·  {}", step.label, edge_label(&step.edge));

    let mut row = div()
        .id(SharedString::from(format!(
            "lineage-{}-{}",
            step.depth, step.label
        )))
        .pl(gpui::px(indent))
        .child(SharedString::from(text));

    if let Some(name) = step.open_name.clone() {
        row = row
            .cursor_pointer()
            .on_click(cx.listener(move |ws, _ev, window, cx| {
                ws.open_table_tab(name.clone(), window, cx);
            }));
    }
    row
}

#[cfg(test)]
mod tests {
    use super::edge_label;
    use crate::inspector::lineage::EdgeKind;

    #[test]
    fn edge_labels_are_human_readable() {
        assert_eq!(
            edge_label(&EdgeKind::FileImport),
            dat0_i18n::t("inspector.edge.file")
        );
        assert_eq!(
            edge_label(&EdgeKind::SqlRef),
            dat0_i18n::t("inspector.edge.sql")
        );
        assert_eq!(
            edge_label(&EdgeKind::Transform(2)),
            format!("{} (2 ops)", dat0_i18n::t("inspector.edge.transform"))
        );
    }
}
