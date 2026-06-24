pub mod contrast;
mod zed_schema;

use anyhow::{Context, Result};
pub use zed_schema::*;

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub style: ZedStyle,
}

impl Theme {
    pub fn load_builtin(name: &str) -> Result<Self> {
        let json = match name {
            "dark" => include_str!("builtins/dark.json"),
            "light" => include_str!("builtins/light.json"),
            "high-contrast" => include_str!("builtins/high-contrast.json"),
            other => anyhow::bail!("unknown built-in theme: {other}"),
        };
        let parsed: ZedTheme =
            serde_json::from_str(json).with_context(|| format!("parse builtin theme {name}"))?;
        Ok(Self {
            name: parsed.name,
            style: parsed.style,
        })
    }

    /// Same as [`load_builtin`] but swallows the unknown-id error and
    /// falls back to the `"dark"` built-in. P3b T12 uses this on the
    /// install / switch paths so a corrupt `theme.id` in
    /// `settings.toml` doesn't crash the running app — the failure
    /// shape is "user sees the dark theme" rather than a panic. The
    /// fallible [`load_builtin`] is kept for callers that want to
    /// distinguish "known id" from "unknown id" (tests, future
    /// validation UI).
    pub fn load_builtin_or_default(name: &str) -> Self {
        match Self::load_builtin(name) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    requested = name,
                    "Theme::load_builtin_or_default: unknown id; falling back to 'dark'"
                );
                // Built-in `dark.json` is checked into the repo and parses;
                // an Err here is unreachable in practice. `expect` keeps the
                // failure loud if someone breaks the JSON in a future change.
                Self::load_builtin("dark").expect("built-in 'dark' theme must parse")
            }
        }
    }

    /// Logical id of this theme (matches the value persisted at
    /// `theme.id` in the SettingsStore). The current
    /// [`Theme::load_builtin`] sets `self.name` from the JSON's
    /// `"name"` field, which by convention matches the id for the
    /// three built-ins; tests in
    /// `crates/dat0-app/tests/theme_live_switch.rs` rely on this
    /// equality.
    pub fn id(&self) -> &str {
        &self.name
    }

    /// Background colour hex string, taken from the Zed-schema style
    /// block. Returned as `&str` so the equality assertion in
    /// `theme_live_switch::load_builtin_dark_and_light_differ` can
    /// compare two `&str` slices without needing an Hsla parser.
    pub fn background(&self) -> &str {
        &self.style.background
    }
}

impl gpui::Global for Theme {}

impl Theme {
    /// Install the active theme as a `gpui::Global` at app boot.
    /// Reads the persisted `theme.id` from the [`SettingsStore`] and
    /// falls back to `"dark"` when the key is missing or unknown.
    /// Called once from `run_app` before any window opens — every
    /// view that subscribes via `cx.observe_global::<Theme>` then
    /// sees the same initial palette.
    pub fn install(cx: &mut gpui::App, settings: &crate::settings::store::SettingsStore) {
        let id = settings
            .get_string("theme.id")
            .unwrap_or_else(|| "dark".into());
        let theme = Self::load_builtin_or_default(&id);
        cx.set_global(theme);
    }

    /// Replace the global theme with the built-in identified by
    /// `new_id`. Subscribers registered with `cx.observe_global::<Theme>`
    /// receive a `NotifyGlobalObservers` effect on the next tick and
    /// re-render with the new palette — no app restart required.
    /// Unknown ids fall back to `"dark"` (see
    /// [`Theme::load_builtin_or_default`]). Cross-window propagation
    /// is automatic because the global is app-scoped per
    /// `docs/internal/gpui-api-notes.md` §0.A.4.
    pub fn switch(cx: &mut gpui::App, new_id: &str) {
        let new_theme = Self::load_builtin_or_default(new_id);
        cx.set_global(new_theme);
    }
}
