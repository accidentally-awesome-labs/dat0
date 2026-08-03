//! B7: the activity rail — a 48 px vertical icon strip that selects which left
//! panel is showing, modelled on VSCode's activity bar.
//!
//! It is a SIBLING of the `DockArea`, not a dock panel. That is what keeps it
//! visible when the dock is collapsed — the point of the model — and it keeps
//! the rail out of the dock's `.tab_group()` entirely.
//!
//! Lives in `src/view/` with every other rendered shell surface; B3 recorded
//! this after the master plan guessed a top-level module.
//!
//! ## Two independent pieces of state
//!
//! The keyboard CURSOR and which panel is OPEN are separate, and conflating them
//! is the easiest way to get this surface wrong. The cursor exists even when the
//! dock is collapsed and nothing is open. This is the same two-state model the
//! catalog tree uses for its active row versus its selection.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::{ActiveTheme as _, Icon};

use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
use crate::assets::Dat0IconName;
use crate::theme::tokens::Dat0Theme as _;
use crate::window::{LeftPanel, WorkspaceShell};

/// 48 px, VSCode's activity-bar width.
pub(crate) const RAIL_WIDTH: f32 = 48.0;

pub(crate) struct RailItem {
    pub id: &'static str,
    pub panel: LeftPanel,
    pub icon: Dat0IconName,
    /// Names the ACTION, not the panel.
    ///
    /// ⚠ A rail item labelled "Catalog" would collide with the catalog tree's
    /// own accessible name, and `A11ySnapshot::query_by_role` PANICS on a
    /// duplicate match (`tests/support/mod.rs:139`) — it would take whole suites
    /// down rather than fail one assertion. "Show Catalog" is also the more
    /// honest name: the item is a button that reveals a panel, not the panel.
    pub label_key: &'static str,
}

/// Top-to-bottom order. The index into this array is the keyboard cursor.
pub(crate) const ITEMS: &[RailItem; 3] = &[
    RailItem {
        id: "rail-catalog",
        panel: LeftPanel::Catalog,
        icon: Dat0IconName::Database,
        label_key: "rail.show_catalog",
    },
    RailItem {
        id: "rail-connections",
        panel: LeftPanel::Connections,
        icon: Dat0IconName::Plug,
        label_key: "rail.show_connections",
    },
    RailItem {
        id: "rail-ai",
        panel: LeftPanel::Ai,
        icon: Dat0IconName::Sparkles,
        label_key: "rail.show_ai",
    },
];

/// Render the rail.
///
/// `cursor` is the keyboard cursor; `open` is which panel is actually showing.
pub(crate) fn render_rail(
    cursor: usize,
    open: Option<LeftPanel>,
    fh: &gpui::FocusHandle,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement {
    let ring = cx.theme().d0().focus_ring;

    // ↑/↓: a SECOND `on_key_down` chained after `focus_stop`'s own — gpui pushes
    // key-down listeners and both fire (the catalog tree's proven idiom).
    let arrows =
        cx.listener(
            |ws, ev: &gpui::KeyDownEvent, _window, cx| match ev.keystroke.key.as_str() {
                "up" => ws.rail_move_cursor(-1, cx),
                "down" => ws.rail_move_cursor(1, cx),
                _ => {}
            },
        );
    // Enter/Space — `focus_stop` routes only those two here.
    let activate = cx.listener(|ws, _ev: &gpui::KeyDownEvent, _window, cx| {
        ws.rail_activate_cursor(cx);
    });

    let mut root = div()
        .flex()
        .flex_col()
        .gap_1()
        .p_1()
        .w(px(RAIL_WIDTH))
        .h_full()
        .border_r_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().background)
        // ONE container stop for the whole rail, not three — the listbox pattern.
        // B7 already migrates nine handles into dock chrome; adding three more
        // shell-level stops on top of that is the change this avoids.
        .focus_stop("activity-rail", fh, 0, ring, activate)
        .on_key_down(arrows)
        .a11y(
            "activity-rail",
            AccessRole::Button,
            dat0_i18n::t("rail.title"),
        );

    for (i, item) in ITEMS.iter().enumerate() {
        root = root.child(rail_item(
            i,
            item,
            cursor == i,
            open == Some(item.panel),
            cx,
        ));
    }
    root.into_any_element()
}

fn rail_item(
    index: usize,
    item: &'static RailItem,
    is_cursor: bool,
    is_open: bool,
    cx: &mut Context<WorkspaceShell>,
) -> impl IntoElement {
    let label = dat0_i18n::t(item.label_key);
    let ring = cx.theme().d0().focus_ring;
    let accent = cx.theme().primary;
    let open_bg = cx.theme().secondary;
    let radius = cx.theme().radius;
    let tooltip_label = label.clone();

    div()
        .id(item.id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(40.))
        .rounded(radius)
        .cursor_pointer()
        // The OPEN panel gets a leading accent bar plus a raised background;
        // the keyboard CURSOR gets the focus ring. Two states, two affordances.
        .when(is_open, |this| {
            this.bg(open_bg).border_l_2().border_color(accent)
        })
        .when(is_cursor, |this| this.border_1().border_color(ring))
        .a11y(item.id, AccessRole::Button, label)
        // `.tooltip` is on `StatefulInteractiveElement`, so `.id()` above is
        // required. `Tooltip` is NOT re-exported at gpui-component's crate root
        // (`lib.rs:66` is a bare `pub mod tooltip;`) — both facts measured in T0,
        // and both contradict two stale comments in `view/sql_console.rs`.
        .tooltip(move |window, app| {
            gpui_component::tooltip::Tooltip::new(tooltip_label.clone()).build(window, app)
        })
        .child(Icon::new(item.icon))
        .on_click(cx.listener(move |ws, _ev, _window, cx| ws.rail_click(index, cx)))
}
