//! Catalog left-dock panel (P6a §4). Renders the pure [`CatalogTree`](crate::catalog::CatalogTree)
//! as three sections (Sources / Tables / Derived); each node is a clickable row
//! that opens that table into the main grid via [`WorkspaceShell::open_table_tab`].
//!
//! Like [`crate::connections::panel`], this is a *free function* — not a GPUI
//! `Render`/`EventEmitter` entity — so every row's `on_click` can reach
//! `WorkspaceShell` directly via `cx.listener(|ws, …| …)`. The render is a pure
//! function of the supplied `tree`; the live tree is rebuilt by
//! `WorkspaceShell::refresh_catalog` whenever the catalog could change.

use crate::window::WorkspaceShell;
use gpui::prelude::*;
use gpui::{Context, SharedString, div};

/// A section header label, e.g. `section_label("Tables", 3) == "Tables (3)"`.
/// The section titles ("Sources"/"Tables"/"Derived") are structural group names,
/// not user-facing toggle strings, so they are passed as literals (not i18n).
pub(crate) fn section_label(name: &str, n: usize) -> String {
    format!("{name} ({n})")
}

/// Render the catalog dock from the current tree. Called from
/// `WorkspaceShell::render`. A pure function of `tree` — the only state read is
/// `tree.sources` / `tree.tables` / `tree.derived`.
pub fn render_catalog(
    tree: &crate::catalog::CatalogTree,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement {
    let sections = [
        ("Sources", &tree.sources),
        ("Tables", &tree.tables),
        ("Derived", &tree.derived),
    ];

    let mut root = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .child(div().child(SharedString::from(dat0_i18n::t("catalog.title"))));

    for (title, nodes) in sections {
        let mut section = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(div().child(SharedString::from(section_label(title, nodes.len()))));
        for node in nodes {
            section = section.child(catalog_row(title, &node.name, cx));
        }
        root = root.child(section);
    }

    root.into_any_element()
}

/// A clickable catalog row that opens `name` into the main grid. Mirrors the
/// `action_button` idiom in [`crate::connections::panel`]
/// (`div().id(..).cursor_pointer().on_click(cx.listener(..))`).
fn catalog_row(section: &str, name: &str, cx: &mut Context<WorkspaceShell>) -> gpui::Stateful<gpui::Div> {
    let name = name.to_string();
    // ElementId must be unique within the render pass: a table name can recur
    // across sections (e.g. an attached `events` and a local derived `events`),
    // so the section qualifies the id to avoid GPUI click/hover cross-talk.
    div()
        .id(SharedString::from(format!("cat-{section}-{name}")))
        .px_2()
        .py_1()
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgba(0x80808022)))
        .child(SharedString::from(name.clone()))
        .on_click(cx.listener(move |ws, _ev, window, cx| {
            ws.open_table_tab(name.clone(), window, cx);
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn section_header_label_counts_nodes() {
        assert_eq!(section_label("Tables", 3), "Tables (3)");
    }
}
