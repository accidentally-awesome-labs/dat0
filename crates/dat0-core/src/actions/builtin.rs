//! Built-in action descriptors. Registered exactly once at boot via
//! `register_all(&registry)`.
//!
//! Each id in [`ids`] is a stable string referenced by Banner `action_id`
//! values, by the command palette, and by the native menu bar (menu items are
//! created with the action id as their `muda` id, so a menu click resolves
//! straight back to a descriptor).
//!
//! # Why every dispatch body is one line
//!
//! Almost every action in dat0 means "do X to the focused window", and only the
//! shell knows which window that is. Rather than give the registry a renderer
//! handle, a descriptor emits [`AppEvent::RunAction`] carrying its own id and
//! the shell performs it. The registry, the palette and the menu bar therefore
//! need no toolkit at all, and there is exactly one place — the shell's action
//! router — that knows how an id becomes work.
//!
//! # Keybinding hints
//!
//! Descriptors carry no keybinding of their own. The command palette reads the
//! same keymap table the bindings are installed from, so a hint cannot disagree
//! with the live chord.
//!
//! Some ids are deliberately chord-less: `window.new` (see the comment on its
//! id), `sql.new_tab` and `sql.close_tab` (they would collide with the editor's
//! own keymap). `tests/keymap.rs`'s `UNBOUND` list is where each is declared.

use std::sync::Arc;

use crate::events::{AppEvent, AppEvents};

use super::registry::{
    ActionDescriptor, ActionGroup, ActionId, ActionRegistry, DispatchFn, RegisterError,
};

pub mod ids {
    pub const FILE_OPEN: &str = "file.open";
    pub const WINDOW_NEW: &str = "window.new";
    pub const THEME_TOGGLE: &str = "theme.toggle";
    pub const SETTINGS_OPEN: &str = "settings.open";
    pub const RECENTS_SHOW: &str = "recents.show";
    pub const RECOVERY_REVIEW: &str = "recovery.review";
    pub const IMPORT_CANCEL: &str = "import.cancel";
    pub const SAMPLE_DATA_RETRY_TAXI: &str = "sample_data.retry_taxi";
    /// EN4: rebuild the focused window's session after `Session::new` failed.
    /// Referenced by the `SessionSlot::Failed` banner, which is the only way to
    /// leave that state — the boot deliberately does not retry on its own.
    pub const SESSION_RETRY: &str = "session.retry";
    pub const VIEW_UNDO: &str = "view.undo";
    pub const VIEW_REDO: &str = "view.redo";
    // T9: edit/clipboard/bulk action ids.
    pub const VIEW_COPY: &str = "view.copy";
    pub const VIEW_CUT: &str = "view.cut";
    pub const VIEW_PASTE: &str = "view.paste";
    pub const VIEW_FILL_DOWN: &str = "view.fill_down";
    pub const VIEW_DELETE_ROWS: &str = "view.delete_rows";
    pub const VIEW_SET_NULL: &str = "view.set_null";
    pub const VIEW_SET_VALUE: &str = "view.set_value";
    pub const VIEW_DELETE_COLUMN: &str = "view.delete_column";
    // P4c T11: File → Export… dialog.
    pub const VIEW_EXPORT: &str = "view.export";
    // UI-redesign B6: chart export. The dock title bar's own PNG/SVG buttons
    // are forced `tab_stop(false)` by upstream (`tab_panel.rs:454`), so these
    // descriptors are chart export's only keyboard path.
    pub const CHART_EXPORT_PNG: &str = "chart.export.png";
    pub const CHART_EXPORT_SVG: &str = "chart.export.svg";
    // P9a T7: Charts → Visualize (toggle the right-dock chart panel).
    pub const CHART_VISUALIZE: &str = "chart.visualize";
    // P5a T11: SQL Console entry points (toggle / run / cancel / tab lifecycle).
    pub const CONSOLE_TOGGLE: &str = "console.toggle";
    /// S1: show or hide the catalog sidebar (⌘B).
    ///
    /// New in the Dioxus shell. The GPUI build had no such command because the
    /// left dock was a three-way mode switch driven by the activity rail, and
    /// "hide it" was not one of the three modes.
    pub const SIDEBAR_TOGGLE: &str = "sidebar.toggle";
    pub const SQL_RUN: &str = "sql.run";
    pub const SQL_CANCEL: &str = "sql.cancel";
    pub const SQL_NEW_TAB: &str = "sql.new_tab";
    pub const SQL_CLOSE_TAB: &str = "sql.close_tab";
    // P5b: SQL Console reuse/promotion actions.
    pub const SQL_SAVE_QUERY: &str = "sql.save_query";
    pub const SQL_LOAD_QUERY: &str = "sql.load_query";
    pub const SQL_HISTORY: &str = "sql.history";
    pub const SQL_SAVE_AS_TABLE: &str = "sql.save_as_table";
    pub const VIEW_SAVE_AS_TABLE: &str = "view.save_as_table";
    // P7a: Workspace open/save flows.
    pub const WORKSPACE_OPEN: &str = "workspace.open";
    pub const WORKSPACE_SAVE: &str = "workspace.save";
    // P7c T5: one-click re-import of an externally-changed source file.
    pub const LIVE_REFRESH: &str = "live.refresh";
    // P9c-1 T9: open/toggle the AI panel (BYOK secure-plumbing dock).
    pub const AI_PANEL_OPEN: &str = "ai.panel.open";
    // P11a T7: take-a-tour re-entry for command palette (carousel).
    pub const ONBOARDING_TAKE_TOUR: &str = "onboarding.take_tour";
    // MX1: toggle the per-window frame-interval HUD. Palette-only by design —
    // a chord would spend one of the few free ones on a diagnostic.
    pub const PERF_HUD_TOGGLE: &str = "perf.hud.toggle";
}

/// The dispatch body every descriptor uses: name the action, let the shell
/// perform it in whichever window has focus.
pub(super) fn run(id: &'static str) -> DispatchFn {
    Arc::new(move |events: &AppEvents| {
        events.send(AppEvent::RunAction { id, window: None });
    })
}

/// Shorthand for a descriptor whose dispatch is the standard [`run`] body.
pub(super) fn descriptor(
    id: &'static str,
    title: impl Into<String>,
    group: ActionGroup,
) -> ActionDescriptor {
    ActionDescriptor {
        id: ActionId::from(id),
        title: title.into(),
        group,
        dispatch: run(id),
    }
}

/// Register every built-in action onto `reg`. Returns an error if any id
/// collides — boot calls `.expect("...")` because two built-ins sharing an id
/// is a programmer error, not a runtime condition.
pub fn register_all(reg: &ActionRegistry) -> Result<(), RegisterError> {
    use ActionGroup::*;

    for (id, title, group) in [
        (ids::FILE_OPEN, "Open File\u{2026}".to_string(), File),
        (ids::WINDOW_NEW, "New Window".to_string(), Navigation),
        (ids::SESSION_RETRY, "Retry Session".to_string(), Navigation),
        (ids::THEME_TOGGLE, "Toggle Theme".to_string(), Theme),
        (ids::SETTINGS_OPEN, "Open Settings".to_string(), Settings),
        (ids::RECENTS_SHOW, "Show Recents".to_string(), Navigation),
        (
            ids::RECOVERY_REVIEW,
            "Review previous sessions".to_string(),
            Recovery,
        ),
        (ids::IMPORT_CANCEL, "Cancel Import".to_string(), Import),
        (
            ids::SAMPLE_DATA_RETRY_TAXI,
            "Retry NYC Taxi download".to_string(),
            File,
        ),
        (
            ids::WORKSPACE_OPEN,
            "Open Workspace\u{2026}".to_string(),
            File,
        ),
        (
            ids::WORKSPACE_SAVE,
            "Save Workspace\u{2026}".to_string(),
            File,
        ),
        (ids::LIVE_REFRESH, "Refresh from source".to_string(), File),
        (
            ids::ONBOARDING_TAKE_TOUR,
            dat0_i18n::t("menu.help.take_tour"),
            Navigation,
        ),
        (
            ids::PERF_HUD_TOGGLE,
            dat0_i18n::t("action.perf_hud"),
            Navigation,
        ),
    ] {
        reg.register(descriptor(id, title, group))?;
    }

    super::view_actions::register(reg)?;
    super::edit_actions::register(reg)?;
    super::sql_actions::register(reg)?;

    Ok(())
}
