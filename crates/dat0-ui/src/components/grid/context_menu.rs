//! The grid's right-click menu.
//!
//! Same seven items and the same gating as the GPUI `PopupMenu`, but built out
//! of a positioned `div[role=menu]` instead of a widget-library popup — which
//! means arrow-key navigation, Escape and click-outside are dat0's, visible in
//! one file, and testable.
//!
//! Items name an [`ActionId`](dat0_core::actions::registry::ActionId); the
//! shell performs them. `view.set_value` and `view.delete_column` need an
//! argument a menu click cannot carry, so those two report their own coordinate
//! alongside the id.

use dioxus::prelude::*;

use dat0_core::actions::builtin::ids;
use dat0_core::grid::selection::CellCoord;

/// One row of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuEntry {
    pub id: &'static str,
    pub label: String,
    pub enabled: bool,
    /// A rule above this entry.
    pub separator_before: bool,
}

/// Build the menu for a given selection state.
///
/// Pure, so the gating is testable without a pointer: "Delete Row(s) is
/// disabled with an empty selection" is a rule, and a rule with no test is a
/// rule that drifts.
pub fn entries(has_selection: bool, read_only: bool) -> Vec<MenuEntry> {
    let item = |id: &'static str, key: &str, enabled: bool, sep: bool| MenuEntry {
        id,
        label: dat0_i18n::t(key),
        enabled,
        separator_before: sep,
    };
    // Every mutation is refused in a read-only workspace — the same gate
    // `mutation_blocked` applies at the call sites, surfaced here so the menu
    // says so rather than silently doing nothing.
    let w = !read_only;
    vec![
        item(ids::VIEW_COPY, "menu.copy", true, false),
        item(ids::VIEW_CUT, "menu.cut", w && has_selection, false),
        item(ids::VIEW_PASTE, "menu.paste", w, false),
        item(
            ids::VIEW_FILL_DOWN,
            "menu.fill_down",
            w && has_selection,
            true,
        ),
        item(
            ids::VIEW_SET_NULL,
            "menu.set_null",
            w && has_selection,
            false,
        ),
        item(
            ids::VIEW_DELETE_ROWS,
            "menu.delete_rows",
            w && has_selection,
            true,
        ),
        item(ids::VIEW_DELETE_COLUMN, "menu.delete_column", w, true),
    ]
}

#[derive(Clone, Props, PartialEq)]
pub struct ContextMenuProps {
    /// Where the pointer was, in client coordinates.
    pub at: (f64, f64),
    /// The cell that was right-clicked; carried so the column-scoped items
    /// know which column they mean.
    pub cell: CellCoord,
    pub has_selection: bool,
    #[props(default = false)]
    pub read_only: bool,
    /// `(action id, the right-clicked cell)`.
    pub on_pick: EventHandler<(&'static str, CellCoord)>,
    pub on_dismiss: EventHandler<()>,
}

#[component]
pub fn ContextMenu(props: ContextMenuProps) -> Element {
    let items = entries(props.has_selection, props.read_only);
    // The keyboard cursor starts on the first item a press could actually do
    // something with, so Enter is never a no-op on open.
    let first = items.iter().position(|i| i.enabled).unwrap_or(0);
    let mut cursor = use_signal(|| first);

    let on_pick = props.on_pick;
    let on_dismiss = props.on_dismiss;
    let cell = props.cell;
    let (x, y) = props.at;
    let n = items.len();
    let enabled: Vec<bool> = items.iter().map(|i| i.enabled).collect();
    let ids_list: Vec<&'static str> = items.iter().map(|i| i.id).collect();

    rsx! {
        // Click-outside. A transparent full-window layer under the menu is the
        // only way to catch a click anywhere without a document listener.
        div {
            class: "d0-menu-dismiss",
            "data-a11y-id": "context-menu-dismiss",
            onmousedown: move |_| on_dismiss.call(()),
        }
        div {
            class: "d0-menu",
            "data-a11y-id": "context-menu",
            role: "menu",
            tabindex: "0",
            autofocus: true,
            style: "left: {x}px; top: {y}px;",
            onkeydown: move |e| {
                e.stop_propagation();
                match e.key() {
                    Key::Escape => on_dismiss.call(()),
                    Key::ArrowDown => cursor.set(step(cursor(), 1, &enabled)),
                    Key::ArrowUp => cursor.set(step(cursor(), -1, &enabled)),
                    Key::Enter => {
                        if enabled.get(cursor()).copied().unwrap_or(false) {
                            on_pick.call((ids_list[cursor()], cell));
                        }
                    }
                    _ => {}
                }
            },
            for (i, entry) in items.iter().enumerate() {
                {
                    let id = entry.id;
                    let active = i == cursor() && n > 0;
                    rsx! {
                        // The separator is a child of the item rather than a
                        // sibling: rsx only allows `key` on the first node in a
                        // block, and the item is the thing with an identity.
                        div { key: "{id}", class: "d0-menu-entry",
                        if entry.separator_before {
                            div { class: "d0-menu-sep", role: "separator" }
                        }
                        button {
                            class: if active { "d0-menu-item is-active" } else { "d0-menu-item" },
                            "data-a11y-id": "menu-{id}",
                            role: "menuitem",
                            "aria-label": "{entry.label}",
                            "aria-disabled": if entry.enabled { "false" } else { "true" },
                            tabindex: "-1",
                            onmousedown: move |e| {
                                e.stop_propagation();
                                on_pick.call((id, cell));
                            },
                            "{entry.label}"
                        }
                        }
                    }
                }
            }
        }
    }
}

/// Move the cursor to the next enabled entry, wrapping.
///
/// Skipping disabled entries is why this is not `(i + 1) % n`: landing on a
/// greyed-out row and pressing Enter to no effect is how a keyboard user
/// concludes the menu is broken.
fn step(from: usize, delta: isize, enabled: &[bool]) -> usize {
    let n = enabled.len();
    if n == 0 || !enabled.iter().any(|e| *e) {
        return from;
    }
    let mut i = from;
    for _ in 0..n {
        i = ((i as isize + delta).rem_euclid(n as isize)) as usize;
        if enabled[i] {
            return i;
        }
    }
    from
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_seven_items_are_always_present() {
        // Present-but-disabled, never absent: a menu whose shape changes with
        // state is a menu users cannot build muscle memory for.
        assert_eq!(entries(false, false).len(), 7);
        assert_eq!(entries(true, true).len(), 7);
    }

    #[test]
    fn selection_scoped_items_are_disabled_without_one() {
        let none = entries(false, false);
        let find = |id| none.iter().find(|e| e.id == id).unwrap();
        assert!(!find(ids::VIEW_DELETE_ROWS).enabled);
        assert!(!find(ids::VIEW_FILL_DOWN).enabled);
        assert!(!find(ids::VIEW_SET_NULL).enabled);
        // Copy always works: copying nothing is harmless.
        assert!(find(ids::VIEW_COPY).enabled);
    }

    #[test]
    fn a_read_only_workspace_disables_every_mutation_but_not_copy() {
        let ro = entries(true, true);
        for e in &ro {
            if e.id == ids::VIEW_COPY {
                assert!(e.enabled, "copy is not a mutation");
            } else {
                assert!(!e.enabled, "{} must be refused when read-only", e.id);
            }
        }
    }

    #[test]
    fn arrow_navigation_skips_disabled_entries() {
        let enabled = vec![true, false, false, true];
        assert_eq!(step(0, 1, &enabled), 3, "down skips the two disabled rows");
        assert_eq!(step(3, 1, &enabled), 0, "and wraps");
        assert_eq!(step(0, -1, &enabled), 3, "up wraps the other way");
    }

    #[test]
    fn navigation_terminates_when_nothing_is_enabled() {
        // Every item disabled is reachable (read-only, no selection); a
        // wrap-until-enabled loop would spin forever.
        let enabled = vec![false, false, false];
        assert_eq!(step(1, 1, &enabled), 1);
    }

    #[test]
    fn separators_group_the_menu_the_way_the_gpui_one_did() {
        let e = entries(true, false);
        let sep_before: Vec<&str> = e
            .iter()
            .filter(|i| i.separator_before)
            .map(|i| i.id)
            .collect();
        assert_eq!(
            sep_before,
            vec![
                ids::VIEW_FILL_DOWN,
                ids::VIEW_DELETE_ROWS,
                ids::VIEW_DELETE_COLUMN
            ]
        );
    }
}
