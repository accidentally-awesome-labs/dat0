//! Catalog left-dock panel (P6a §4, hierarchy since the catalog-tree slice).
//! Renders the pure [`CatalogTree`](crate::catalog::CatalogTree) as four
//! sections (Sources / Cloud / Tables / Derived); attach parents paint a
//! chevron row whose children indent beneath, and every table row is clickable
//! (opens into the main grid via [`WorkspaceShell::open_table_tab`]).
//!
//! Like [`crate::connections::panel`], this is a *free function* — not a GPUI
//! `Render`/`EventEmitter` entity — so every row's `on_click` can reach
//! `WorkspaceShell` directly via `cx.listener(|ws, …| …)`. The render is a pure
//! function of the supplied `tree` + collapse/nav state; the live tree is
//! rebuilt by `WorkspaceShell::refresh_catalog` whenever the catalog could
//! change.

use crate::a11y::A11yExt as _;
use crate::a11y::FocusStopExt as _;
use crate::catalog::nav::{RowKind, visible_rows};
use crate::theme::tokens::Dat0Theme as _;
use crate::window::WorkspaceShell;
use gpui::prelude::*;
use gpui::{Context, SharedString, div};
use gpui_component::{ActiveTheme as _, Icon, IconName};
use std::collections::HashSet;

/// A section header label, e.g. `section_label("Tables", 3) == "Tables (3)"`.
/// The section titles ("Sources"/"Tables"/"Derived") are structural group names,
/// not user-facing toggle strings, so they are passed as literals (not i18n).
pub(crate) fn section_label(name: &str, n: usize) -> String {
    format!("{name} ({n})")
}

/// Render the catalog dock from the current tree + collapse/nav state. Called
/// from `WorkspaceShell::render`. The row list comes from the pure
/// [`visible_rows`] flatten — the SAME Vec the keyboard handlers use, so the
/// paint order and the nav index cannot drift.
pub fn render_catalog(
    tree: &crate::catalog::CatalogTree,
    collapsed: &HashSet<String>,
    active: usize,
    fh: &gpui::FocusHandle,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement {
    let rows = visible_rows(tree, collapsed);
    let active = active.min(rows.len().saturating_sub(1));

    // ↑/↓/←/→: a SECOND on_key_down chained after focus_stop's own (gpui
    // pushes key-down listeners; both fire — T0-proven on this surface).
    let arrows = cx.listener(|ws, ev: &gpui::KeyDownEvent, window, cx| {
        if matches!(ev.keystroke.key.as_str(), "up" | "down" | "left" | "right") {
            let key = ev.keystroke.key.clone();
            ws.catalog_nav_key(&key, window, cx);
        }
    });
    // Enter/Space (focus_stop routes only those two here).
    let activate = cx.listener(|ws, ev: &gpui::KeyDownEvent, window, cx| {
        let key = ev.keystroke.key.clone();
        ws.catalog_nav_key(&key, window, cx);
    });

    let ring = cx.theme().d0().focus_ring;

    let mut root = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .focus_stop("catalog-tree", fh, 0, ring, activate)
        .on_key_down(arrows)
        .a11y(
            "catalog-tree",
            crate::a11y::AccessRole::Button,
            dat0_i18n::t("catalog.title"),
        );
    // B7: the "Catalog" title row moved into `CatalogPanel::title` — the dock
    // paints a 30px title bar above this body, so keeping both would show the
    // word twice. The `.a11y` above STAYS: it is the tree's accessible name and
    // `catalog_nav` resolves the focus oracle through it.

    // Header `(n)` = TOP-LEVEL node count (an attach = 1 parent), collapse-
    // independent — keeps the Slice-5 "Cloud (1)" teeth semantics.
    let sections: [(String, &'static str, usize); 4] = [
        ("Sources".to_string(), "Sources", tree.sources.len()),
        (dat0_i18n::t("catalog.cloud"), "Cloud", tree.cloud.len()),
        ("Tables".to_string(), "Tables", tree.tables.len()),
        ("Derived".to_string(), "Derived", tree.derived.len()),
    ];

    let mut iter = rows.iter().enumerate().peekable();
    for (label, id, n) in sections {
        let header = section_label(&label, n);
        let mut section = div().flex().flex_col().gap_1().child(
            div()
                .a11y_label(crate::a11y::AccessRole::Label, header.clone())
                .child(SharedString::from(header)),
        );
        while iter.peek().is_some_and(|(_, r)| r.section == id) {
            let (i, row) = iter.next().expect("peeked");
            section = section.child(match &row.kind {
                RowKind::Parent {
                    alias,
                    expanded,
                    n_children,
                } => parent_row(id, alias, *expanded, *n_children, i == active, cx),
                RowKind::Leaf { name, depth } => catalog_row(id, name, *depth, i == active, cx),
            });
        }
        root = root.child(section);
    }

    debug_assert!(
        iter.next().is_none(),
        "visible_rows section order drifted from render_catalog's section list"
    );

    root.into_any_element()
}

/// An attach-parent row: chevron + alias + child count. Click toggles
/// expand/collapse via `toggle_catalog_parent` — the SAME method the keyboard
/// Toggle arm calls (single source of truth).
fn parent_row(
    section: &str,
    alias: &str,
    expanded: bool,
    n_children: usize,
    is_active: bool,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::Stateful<gpui::Div> {
    let d0 = cx.theme().d0();
    let (ring, hover) = (d0.focus_ring, d0.hover_tint);
    // A6d: the chevron is a real icon, not a glyph folded into the label. The
    // accessible name deliberately drops it — a screen reader should announce
    // "main (3)", not "▾ main (3)"; expand/collapse state is the icon's job.
    let chev = if expanded {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    let text = format!("{alias} ({n_children})");
    let alias_owned = alias.to_string();
    // `attach-` infix: a parent id can never collide with a same-named table
    // row id within the section.
    let mut row = div()
        .id(SharedString::from(format!("cat-{section}-attach-{alias}")))
        .px_2()
        .py_1()
        .cursor_pointer()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .hover(move |s| s.bg(hover))
        .child(Icon::new(chev))
        .child(SharedString::from(text.clone()))
        .a11y_label(crate::a11y::AccessRole::Label, text)
        .on_click(cx.listener(move |ws, _ev, _window, cx| {
            ws.toggle_catalog_parent(alias_owned.clone(), cx);
        }));
    if is_active {
        row = row.border_2().border_color(ring);
    }
    row
}

/// A clickable table row that opens `name` into the main grid. `depth == 1`
/// (child of an attach parent) indents. Active row paints the nav ring
/// (decoupled from gpui focus — grid `is_active` idiom).
fn catalog_row(
    section: &str,
    name: &str,
    depth: u8,
    is_active: bool,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::Stateful<gpui::Div> {
    let d0 = cx.theme().d0();
    let (ring, hover) = (d0.focus_ring, d0.hover_tint);
    let name = name.to_string();
    let mut row = div()
        .id(SharedString::from(format!("cat-{section}-{name}")))
        .px_2()
        .py_1()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .child(SharedString::from(name.clone()))
        .a11y_label(crate::a11y::AccessRole::Label, name.clone())
        .on_click(cx.listener(move |ws, _ev, window, cx| {
            ws.open_table_tab(name.clone(), window, cx);
        }));
    if depth == 1 {
        row = row.pl_4();
    }
    if is_active {
        row = row.border_2().border_color(ring);
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn section_header_label_counts_nodes() {
        assert_eq!(section_label("Tables", 3), "Tables (3)");
    }

    #[test]
    fn cloud_section_label_is_i18n_resolved() {
        // t() echoes the key when missing, so this also asserts the key exists.
        assert_eq!(dat0_i18n::t("catalog.cloud"), "Cloud");
    }
}
