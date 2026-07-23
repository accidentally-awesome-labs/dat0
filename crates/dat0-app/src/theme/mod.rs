//! dat0 theme façade (UI-redesign A1). The single color source of truth is
//! `gpui_component::Theme` — [`Theme::install`] / [`Theme::switch`] apply a
//! full-coverage builtin `ThemeConfig` via `apply_config` and refresh every
//! window. This global only carries the active `{id, mode}` for persistence
//! (`theme.id` in the SettingsStore), the 3-way Settings picker, and
//! `cx.observe_global::<Theme>` fan-out (unchanged subscriber contract).
//!
//! NEVER use `gpui_component::Theme::change` for the 3-way switch: it
//! re-applies from the stored light/dark slots and clobbers high-contrast
//! (master plan §4, verified at rev 0f0ab35).

pub mod contrast;
pub mod tokens;

use std::rc::Rc;
use std::sync::LazyLock;

use gpui_component::{ThemeConfig, ThemeMode};

#[derive(Debug, Clone)]
pub struct Theme {
    /// Logical id: `"dark" | "light" | "high-contrast"`. Matches the value
    /// persisted at `theme.id` in the SettingsStore.
    pub id: String,
    /// The gpui-component mode this id maps to (high-contrast is a `Dark`
    /// config; `apply_config` sets the component-side mode itself).
    pub mode: ThemeMode,
}

impl gpui::Global for Theme {}

fn parse(name: &str, json: &str) -> ThemeConfig {
    // Builtins are compiled in; a parse failure is a programmer error and
    // the coverage gate in tests/theme.rs keeps them well-formed. Loud
    // failure over silent fallback (same policy as the old
    // `load_builtin_or_default` inner expect).
    serde_json::from_str(json).unwrap_or_else(|e| panic!("built-in theme '{name}' must parse: {e}"))
}

static DARK: LazyLock<ThemeConfig> =
    LazyLock::new(|| parse("dark", include_str!("builtins/dark.json")));
static LIGHT: LazyLock<ThemeConfig> =
    LazyLock::new(|| parse("light", include_str!("builtins/light.json")));
static HIGH_CONTRAST: LazyLock<ThemeConfig> =
    LazyLock::new(|| parse("high-contrast", include_str!("builtins/high-contrast.json")));

/// The parsed builtin `ThemeConfig` for a dat0 theme id, or `None` for
/// unknown ids (callers that want fallback semantics use
/// [`Theme::switch`], which maps unknown → `"dark"`).
pub fn builtin_config(id: &str) -> Option<&'static ThemeConfig> {
    match id {
        "dark" => Some(&DARK),
        "light" => Some(&LIGHT),
        "high-contrast" => Some(&HIGH_CONTRAST),
        _ => None,
    }
}

impl Theme {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// App-boot install: read the persisted `theme.id` (missing/unknown →
    /// `"dark"`), set the façade global, and restyle gpui-component.
    /// Called once from `run_app` before any window opens.
    pub fn install(cx: &mut gpui::App, settings: &crate::settings::store::SettingsStore) {
        let id = settings
            .get_string("theme.id")
            .unwrap_or_else(|| "dark".into());
        Self::activate(cx, &id);
    }

    /// Install the default (`"dark"`) theme — the no-config-dir fallback
    /// path in `run_app` and pure-test convenience.
    pub fn install_default(cx: &mut gpui::App) {
        Self::activate(cx, "dark");
    }

    /// Switch to `new_id` and fan out: sets the façade global (observers
    /// registered with `cx.observe_global::<Theme>` re-render next tick)
    /// and re-applies the matching config to the gpui-component global so
    /// widgets actually restyle. Unknown ids fall back to `"dark"`.
    pub fn switch(cx: &mut gpui::App, new_id: &str) {
        Self::activate(cx, new_id);
    }

    fn activate(cx: &mut gpui::App, requested: &str) {
        let (id, cfg) = match builtin_config(requested) {
            Some(cfg) => (requested, cfg),
            None => {
                tracing::warn!(requested, "unknown theme id; falling back to 'dark'");
                ("dark", builtin_config("dark").expect("'dark' is a builtin"))
            }
        };
        cx.set_global(Self {
            id: id.to_string(),
            mode: cfg.mode,
        });
        // Forward to the gpui-component global so widgets restyle. No-op in
        // pure-test contexts that never ran `gpui_component::init` — the
        // façade global still installs so observer-based tests keep working
        // (A0 spike pattern).
        if cx.has_global::<gpui_component::Theme>() {
            gpui_component::Theme::global_mut(cx).apply_config(&Rc::new(cfg.clone()));
            cx.refresh_windows();
        }
    }
}
