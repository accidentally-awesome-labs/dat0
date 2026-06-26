//! Settings panel UI (P1.T16) — sidebar list of section labels on the
//! left, active section's content pane on the right.
//!
//! `SettingsView` is the GPUI `Render` entity. The plan (P1.T21 / boot
//! orchestration) wires it to the View → Settings… menu action; until
//! then this module is scaffolded but not yet opened from the running
//! app. The two integration tests in `tests/settings_ui.rs` exercise the
//! `sections` registry directly so the panel stays testable independent
//! of the GPUI window lifecycle.
//!
//! Layout follows the lower-level Zed `settings_ui` pattern documented in
//! `docs/internal/gpui-api-notes.md` §0.5 (Reference B): outer row split
//! into a fixed-width sidebar (`w_64`) and a flex-1 content pane. The
//! gpui-component `setting` module (Reference A) is the higher-level
//! alternative we may migrate to in a later milestone.

pub mod panel;
pub mod sections;

/// Open the settings panel as a dedicated window (P10b T4 — discharges T13 / D-001).
/// Previously a tracing stub; now opens the real `SettingsPanel` GPUI window.
pub fn open_settings_window(cx: &mut gpui::App) {
    use crate::settings::store::SettingsStore;
    use gpui::{AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
    use gpui_component::Root;

    let store = SettingsStore::with_path(
        crate::platform::config_dir()
            .expect("config dir")
            .join("settings.toml"),
    );
    let bounds = Bounds::centered(None, size(px(720.), px(560.)), cx);
    let _ = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(dat0_i18n::t("settings.window.title").into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        move |window, cx| {
            let view = cx.new(|cx| panel::SettingsPanel::new(store, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        },
    );
}
