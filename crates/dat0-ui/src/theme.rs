//! Theme, UI side: the live token signal and the `<style>` element that
//! carries it.
//!
//! The token sets themselves are `dat0_core::theme` — plain CSS values, no
//! toolkit. All this module adds is "which one is active right now", and the
//! answer is a `Signal<ThemeTokens>`.
//!
//! That is the whole of what used to be `Theme::install` / `Theme::switch` /
//! `apply_config` / `refresh_windows`. Switching theme rewrites one `<style>`
//! element's text; nothing re-mounts, no widget library is re-configured, and
//! there is no frame in which half the window has the new palette.

use dioxus::prelude::*;

use dat0_core::settings::store::SettingsStore;
use dat0_core::theme::{DEFAULT_ID, ThemeTokens, builtin_or_default};

/// The active token set, provided at the shell root.
#[derive(Clone, Copy)]
pub struct Theme(pub Signal<ThemeTokens>);

impl Theme {
    /// Read the persisted `theme.id` and provide the matching tokens.
    ///
    /// An unknown or missing id resolves to [`DEFAULT_ID`], which is **light**:
    /// the design's build target is the light rendering. A persisted id still
    /// wins, so anyone who chose dark keeps dark.
    pub fn provide(settings: Option<&SettingsStore>) -> Self {
        // Read once, when the window mounts: re-reading the file on every
        // render would learn nothing the choice below does not already say.
        Self(use_context_provider(|| {
            let id = settings
                .and_then(|s| s.get_string("theme.id"))
                .unwrap_or_else(|| DEFAULT_ID.to_string());
            Signal::new(builtin_or_default(&id))
        }))
    }

    /// [`provide`](Self::provide), from the settings file every launch
    /// reads, so a window opens in the theme last chosen (PD-023, step 5.9).
    /// `App` provided no settings, so a theme chosen was gone at the next
    /// launch.
    pub fn provide_saved() -> Self {
        Self::provide(settings_store().as_ref())
    }

    /// The context-provided theme.
    pub fn use_current() -> Self {
        Self(use_context())
    }

    /// The context-provided theme, from outside a component body.
    ///
    /// [`use_current`](Self::use_current) is a hook. Called from an event
    /// handler or a task it appends a hook slot to the running scope, and the
    /// next out-of-render hook of another type at that index panics — which
    /// the release profile turns into an abort. The action router runs in the
    /// bus task, so it reads the theme this way.
    pub fn current() -> Self {
        Self(consume_context())
    }

    /// Switch themes. One signal write; the `<style>` element re-renders.
    pub fn set(&mut self, id: &str) {
        self.0.set(builtin_or_default(id));
    }

    pub fn tokens(&self) -> ThemeTokens {
        (self.0)()
    }
}

/// Choose a theme for every window, and keep it for the next launch (PD-023,
/// step 5.9).
///
/// This window repaints at once. The others are told over the process bus,
/// as the settings window's own control tells them, and `theme.id` is written
/// where [`Theme::provide_saved`] reads it. The View menu's toggle repainted
/// only the window it was chosen in, and kept nothing.
pub fn choose(id: &str) {
    Theme::current().set(id);
    match settings_store().map(|store| store.set("theme.id", id)) {
        Some(Ok(())) => {}
        Some(Err(e)) => tracing::warn!(error = %e, "the theme chosen could not be kept"),
        None => tracing::debug!("no config directory: the theme chosen is not kept"),
    }
    if let Some(bus) = crate::launch::process_bus() {
        bus.send(dat0_core::events::AppEvent::ThemeChanged { id: id.to_string() });
    }
}

/// The settings file the theme is kept in, where there is a config directory.
fn settings_store() -> Option<SettingsStore> {
    dat0_core::platform::config_dir()
        .ok()
        .map(|dir| SettingsStore::with_path(dir.join("settings.toml")))
}

/// The two `<style>` elements every window carries: the static rules, and the
/// `:root` block for the active theme.
///
/// `app.css` is fetched over the asset protocol rather than inlined, so the
/// webview caches it once per window and the stylesheet stays a real file that
/// an editor can lint. The token block is inlined because it changes.
#[component]
pub fn ThemeStyle() -> Element {
    let theme = Theme::use_current();
    rsx! {
        link { rel: "stylesheet", href: crate::protocol::url("app.css") }
        style { id: "d0-theme", dangerous_inner_html: "{theme.tokens().css_vars()}" }
    }
}
