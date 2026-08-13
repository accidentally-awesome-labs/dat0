//! View / chart / SQL-console action descriptors.
//!
//! Descriptors only. Each dispatch emits `AppEvent::RunAction`; the shell's
//! action router turns the id into work against the focused window. See
//! [`super::builtin`] for why.

use super::builtin::{descriptor, ids};
use super::registry::{ActionGroup, ActionRegistry, RegisterError};

/// Register the view, chart and console descriptors onto `reg`.
pub fn register(reg: &ActionRegistry) -> Result<(), RegisterError> {
    use ActionGroup::*;

    for (id, title, group) in [
        (ids::VIEW_UNDO, "Undo".to_string(), Edit),
        (ids::VIEW_REDO, "Redo".to_string(), Edit),
        (ids::VIEW_EXPORT, "Export\u{2026}".to_string(), File),
        (
            ids::CONSOLE_TOGGLE,
            dat0_i18n::t("sql.console_toggle"),
            Navigation,
        ),
        (
            ids::SIDEBAR_TOGGLE,
            dat0_i18n::t("catalog.toggle"),
            Navigation,
        ),
        (
            ids::CHART_VISUALIZE,
            dat0_i18n::t("chart.visualize"),
            Navigation,
        ),
        // Chart export lives in the pane header, which is mouse-only; these
        // descriptors are its only keyboard path. They are deliberately not in
        // the palette's HIDDEN list — they do real work whenever a chart has
        // rendered and no-op otherwise, exactly like `view.copy` without a
        // selection.
        (
            ids::CHART_EXPORT_PNG,
            dat0_i18n::t("chart.export.png.command"),
            File,
        ),
        (
            ids::CHART_EXPORT_SVG,
            dat0_i18n::t("chart.export.svg.command"),
            File,
        ),
        (ids::AI_PANEL_OPEN, dat0_i18n::t("menu.ai_panel"), Settings),
        (ids::SQL_RUN, dat0_i18n::t("sql.run"), Edit),
        (ids::SQL_CANCEL, dat0_i18n::t("sql.cancel"), Edit),
        (ids::SQL_NEW_TAB, dat0_i18n::t("sql.new_tab"), Edit),
        (ids::SQL_CLOSE_TAB, dat0_i18n::t("sql.close_tab"), Edit),
    ] {
        reg.register(descriptor(id, title, group))?;
    }

    Ok(())
}
