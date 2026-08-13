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
        let id = settings
            .and_then(|s| s.get_string("theme.id"))
            .unwrap_or_else(|| DEFAULT_ID.to_string());
        Self(use_context_provider(|| {
            Signal::new(builtin_or_default(&id))
        }))
    }

    /// The context-provided theme.
    pub fn use_current() -> Self {
        Self(use_context())
    }

    /// Switch themes. One signal write; the `<style>` element re-renders.
    pub fn set(&mut self, id: &str) {
        self.0.set(builtin_or_default(id));
    }

    pub fn tokens(&self) -> ThemeTokens {
        (self.0)()
    }
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
