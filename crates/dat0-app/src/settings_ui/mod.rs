//! Settings panel UI — sidebar list of section labels on the left,
//! active section's content pane on the right.
//!
//! `SettingsPanel` (panel.rs) is the GPUI `Render` entity, opened as a
//! dedicated window by `open_settings_window`. The two integration tests
//! in `tests/settings_ui.rs` exercise the `sections` registry directly so
//! the panel stays testable independent of the GPUI window lifecycle.

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
