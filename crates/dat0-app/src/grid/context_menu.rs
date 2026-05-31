//! Selection-aware right-click context menu for the data grid (T9).
//!
//! Exposes [`build_menu`], a factory that accepts a snapshot of the current
//! [`SelectionModel`] state and returns a `PopupMenu` builder closure
//! compatible with gpui-component's [`ContextMenuExt::context_menu`].
//!
//! # Menu items
//!
//! All items dispatch through the [`crate::actions`] `ActionRegistry` (stable
//! ids registered by [`crate::actions::edit_actions::register`]).  The
//! `PopupMenuItem::on_click` handler calls the matching `WorkspaceShell` method
//! directly via the captured `WeakEntity` — this avoids the registry lookup
//! overhead and works even when no `App`-level focus is set.
//!
//! | Always shown        | Only when selection is non-empty    |
//! |---------------------|-------------------------------------|
//! | Copy                | Delete Row(s)                       |
//! | Cut                 |                                     |
//! | Paste               |                                     |
//! | ── separator ──     |                                     |
//! | Fill Down           |                                     |
//! | Set NULL            |                                     |
//!
//! # Right-click trigger wiring
//!
//! gpui-component's `ContextMenuExt::context_menu` extension method is the
//! clean hook for right-click menus.  Wiring it to the grid body (the
//! `Table<GridTableDelegate>` element or its containing `div`) requires a
//! `&mut Window` at build time — which is available only inside a
//! `Render::render` call.  Full right-click → menu-popup wiring inside
//! `WorkspaceShell::render` is therefore deferred to T11/polish (the task
//! note says "expose a `build_context_menu` style constructor + a note").
//!
//! The builder closure returned by [`build_menu`] is `'static + Clone`
//! (via `Rc` inside gpui-component) so it can be embedded in a
//! `ContextMenuExt` call when the wiring lands.  The menu items and their
//! dispatch logic are REAL and compile-verified; only the right-click trigger
//! is deferred.
//!
//! # i18n
//!
//! Menu item labels come from [`dat0_i18n::t`]:
//! `menu.copy`, `menu.cut`, `menu.paste`, `menu.fill_down`, `menu.set_null`,
//! `menu.delete_rows`.

use gpui::{Context, WeakEntity, Window};
use gpui_component::menu::PopupMenu;

use crate::grid::selection::SelectionModel;
use crate::window::WorkspaceShell;

/// Build a `PopupMenu` for the given `workspace` + `selection` state.
///
/// `selection` is an `Option<&SelectionModel>` — when `None` (no data source
/// mounted yet) the edit items are still shown but will be silent no-ops (the
/// `WorkspaceShell` handlers guard against a missing selection internally).
///
/// Returns a function compatible with [`gpui_component::menu::ContextMenuExt::context_menu`]:
///
/// ```ignore
/// some_element.context_menu(build_menu(ws_weak, selection.as_ref()))
/// ```
///
/// # T11/polish note
/// The right-click trigger is NOT yet wired in `WorkspaceShell::render`.
/// This constructor is called by neither the render path nor any other live
/// code yet — it is compile-verified only.  T11/polish will call it from
/// `render` to mount the menu on the grid body.
pub fn build_menu(
    ws: WeakEntity<WorkspaceShell>,
    selection: Option<&SelectionModel>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    // Snapshot the selection flag now (before the closure captures anything).
    // The closure is 'static, so we cannot hold a reference into `selection`.
    let has_selection = selection.map(|s| s.has_selection()).unwrap_or(false);

    let ws_copy = ws.clone();
    let ws_cut = ws.clone();
    let ws_paste = ws.clone();
    let ws_fill = ws.clone();
    let ws_null = ws.clone();
    let ws_delete = ws.clone();

    move |menu: PopupMenu, _window: &mut Window, _cx: &mut Context<PopupMenu>| {
        use gpui_component::menu::PopupMenuItem;

        let menu = menu
            .item(PopupMenuItem::new(dat0_i18n::t("menu.copy")).on_click({
                let ws = ws_copy.clone();
                move |_ev, _window, cx| {
                    if let Some(h) = ws.upgrade() {
                        h.update(cx, |ws, cx| ws.copy_selection(cx));
                    }
                }
            }))
            .item(PopupMenuItem::new(dat0_i18n::t("menu.cut")).on_click({
                let ws = ws_cut.clone();
                move |_ev, _window, cx| {
                    if let Some(h) = ws.upgrade() {
                        h.update(cx, |ws, cx| ws.cut_selection(cx));
                    }
                }
            }))
            .item(PopupMenuItem::new(dat0_i18n::t("menu.paste")).on_click({
                let ws = ws_paste.clone();
                move |_ev, _window, cx| {
                    if let Some(h) = ws.upgrade() {
                        h.update(cx, |ws, cx| ws.paste_clipboard(cx));
                    }
                }
            }))
            .separator()
            .item(
                PopupMenuItem::new(dat0_i18n::t("menu.fill_down")).on_click({
                    let ws = ws_fill.clone();
                    move |_ev, _window, cx| {
                        if let Some(h) = ws.upgrade() {
                            h.update(cx, |ws, cx| ws.fill_down(cx));
                        }
                    }
                }),
            )
            .item(PopupMenuItem::new(dat0_i18n::t("menu.set_null")).on_click({
                let ws = ws_null.clone();
                move |_ev, _window, cx| {
                    if let Some(h) = ws.upgrade() {
                        h.update(cx, |ws, cx| ws.set_null_selection(cx));
                    }
                }
            }));

        // "Delete row(s)" is always shown but may be a no-op when nothing is
        // selected. We include it unconditionally for discoverability; the
        // `WorkspaceShell::delete_selected_rows` handler is already a silent
        // no-op when the selection is empty.
        //
        // Decision (T9): `SelectionModel` doesn't distinguish "full-row" vs
        // "cell" selection — any selected cell's row is a deletion candidate
        // (see `delete_selected_rows` semantics). We show the item always and
        // disable it only when there is demonstrably nothing selected.
        menu.separator().item(
            PopupMenuItem::new(dat0_i18n::t("menu.delete_rows"))
                .disabled(!has_selection)
                .on_click({
                    let ws = ws_delete.clone();
                    move |_ev, _window, cx| {
                        if let Some(h) = ws.upgrade() {
                            h.update(cx, |ws, cx| ws.delete_selected_rows(cx));
                        }
                    }
                }),
        )
    }
}

/// Convenience re-export so callers can use the `ContextMenuExt` extension
/// without importing the full gpui-component menu module themselves.
pub use gpui_component::menu::ContextMenuExt;

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: `build_menu` returns a closure without panicking, even when
    /// selection is `None`. (The actual `PopupMenu` construction requires a
    /// GPUI `Window` so we can only test the closure builds successfully at the
    /// type level here.)
    #[test]
    fn build_menu_closure_is_static() {
        // Just verifying the type system accepts the closure as 'static.
        // No GPUI runtime needed.
        fn assert_static<F: 'static>(_: F) {}

        let weak: WeakEntity<WorkspaceShell> = gpui::WeakEntity::new_invalid();
        let closure = build_menu(weak, None);
        assert_static(closure);
    }
}
