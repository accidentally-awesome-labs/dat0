//! AI panel/dock (P9c-1). Event enum + render fn; handled in window.rs.
//!
//! Mirrors `connections/panel.rs`: the panel is a *free function*
//! (`render_ai_panel`) — not a GPUI `Render`/`EventEmitter` entity — because
//! every button needs to reach `WorkspaceShell` (to persist `AiSettings`, write
//! the key to the keychain, and spawn the async Test-connection probe). Rendering
//! it inside `WorkspaceShell::render` lets each `on_click` use
//! `cx.listener(|ws, …| ws.handle_ai_panel_event(…))` so there is no event
//! plumbing to keep alive.
//!
//! SECURITY: the API key is WRITE-ONLY through this panel. It is never echoed
//! back into the input (the panel shows a "key set" indicator instead), never
//! stored in `AiSettings`/settings.toml, and never logged. The handler writes it
//! straight to the keychain via `ai::key_store::KeychainKeyStore`.

use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
use crate::ai::Provider;
use crate::window::WorkspaceShell;
use gpui::prelude::*;
use gpui::{Context, SharedString, div};

/// Intent emitted by a panel button, dispatched to
/// [`WorkspaceShell::handle_ai_panel_event`]. A plain enum (not a GPUI
/// `EventEmitter`) — the panel is a free function bound to the shell's context,
/// so each button calls the handler directly via `cx.listener`.
#[derive(Debug, Clone)]
pub enum AiPanelEvent {
    /// Cycle the active provider (Anthropic → OpenAI → OpenRouter → Custom → …).
    SelectProvider(Provider),
    /// Set the API key (carried from the entry prompt's Confirm). Written ONLY
    /// to the keychain; never stored in panel state beyond writing it.
    SetKey(String),
    /// Set the model name (carried from the entry prompt's Confirm).
    SetModel(String),
    /// Toggle the master enable flag.
    ToggleEnabled,
    /// Toggle the Custom-provider advanced override (allow http + private IPs).
    ToggleAdvancedOverride,
    /// Toggle whether sample rows are included in the outbound payload.
    ToggleIncludeSampleRows,
    /// Probe the provider with the stored key and record a transient pass/fail.
    TestConnection,
    /// Delete the stored key from the keychain (clears the "key set" indicator).
    ForgetKey,
}

/// Live AI-panel draft state held on the `WorkspaceShell`. Mirrors the
/// `ChartPanel` state-struct idiom: the render is a pure function of this. Loaded
/// from `AiSettings` + a keychain key-presence probe when the panel opens; the
/// API KEY itself is NEVER held here (only a "key is set" boolean).
#[derive(Debug, Clone, Default)]
pub struct AiPanel {
    /// Currently-selected provider (`None` until the user picks one — D5).
    pub provider: Option<Provider>,
    /// Whether a key is currently stored in the keychain for `provider`. Drives
    /// the write-only "key set" indicator; the key value is never held here.
    pub key_set: bool,
    /// Draft model name (mirrors `AiSettings.model`).
    pub model: String,
    /// Draft master-enable flag (mirrors `AiSettings.enabled`).
    pub enabled: bool,
    /// Draft Custom-provider advanced override (mirrors `AiSettings.advanced_override`).
    pub advanced_override: bool,
    /// Draft include-sample-rows flag (mirrors `AiSettings.include_sample_rows`).
    pub include_sample_rows: bool,
    /// Transient Test-connection result, rendered via [`test_result_message`].
    /// Cleared on the next config action.
    pub test_result: Option<String>,
}

/// Format the transient Test-connection result line shown under the button.
pub fn test_result_message(ok: bool, msg: &str) -> String {
    if ok {
        format!("✓ {msg}")
    } else {
        format!("✗ {msg}")
    }
}

/// Localized provider button label: the provider id (or an "unset" placeholder).
fn provider_label(provider: Option<Provider>) -> SharedString {
    match provider {
        Some(p) => SharedString::from(format!(
            "{}: {}",
            dat0_i18n::t("ai.provider"),
            dat0_i18n::t(&format!("ai.provider.{}", p.id()))
        )),
        None => SharedString::from(dat0_i18n::t("ai.provider.unset")),
    }
}

/// Render the AI panel from the current draft state. Called from
/// `WorkspaceShell::render`. A pure function of `panel` — mirrors
/// `render_connections`.
pub fn render_ai_panel(
    panel: &AiPanel,
    handles: &crate::empty_state::HeroHandles,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::AnyElement {
    // ── Enable toggle ──────────────────────────────────────────────────────
    let enabled_label = if panel.enabled {
        dat0_i18n::t("ai.enabled.on")
    } else {
        dat0_i18n::t("ai.enabled.off")
    };
    let enabled_row = action_button(
        "ai-toggle-enabled",
        enabled_label,
        AiPanelEvent::ToggleEnabled,
        handles.get("ai-toggle-enabled"),
        cx,
    );

    // ── Provider cycle ─────────────────────────────────────────────────────
    // Click advances to the next provider; SelectProvider carries the target so
    // the handler is a pure dispatch (no state read inside the listener).
    let next_provider = match panel.provider {
        None | Some(Provider::Custom) => Provider::Anthropic,
        Some(Provider::Anthropic) => Provider::OpenAI,
        Some(Provider::OpenAI) => Provider::OpenRouter,
        Some(Provider::OpenRouter) => Provider::Custom,
    };
    let provider_row = action_button(
        "ai-provider-cycle",
        provider_label(panel.provider),
        AiPanelEvent::SelectProvider(next_provider),
        handles.get("ai-provider-cycle"),
        cx,
    );

    // ── API key (write-only) ───────────────────────────────────────────────
    // Never echoes the key: shows a "set" / "not set" indicator. "Set key…"
    // opens an entry prompt (handler); "Forget" deletes from the keychain.
    let key_state = if panel.key_set {
        dat0_i18n::t("ai.key.set")
    } else {
        dat0_i18n::t("ai.key.unset")
    };
    let mut key_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(div().child(SharedString::from(key_state)))
        .child(action_button(
            "ai-key-set",
            dat0_i18n::t("ai.key.set_button"),
            // Empty string sentinel: the handler opens the entry prompt and
            // re-dispatches SetKey with the real value on Confirm.
            AiPanelEvent::SetKey(String::new()),
            handles.get("ai-key-set"),
            cx,
        ));
    if panel.key_set {
        key_row = key_row.child(action_button(
            "ai-key-forget",
            dat0_i18n::t("ai.key.forget"),
            AiPanelEvent::ForgetKey,
            handles.get("ai-key-forget"),
            cx,
        ));
    }

    // ── Model ──────────────────────────────────────────────────────────────
    // Shows the current model (or the provider's placeholder hint when empty).
    let model_display = if panel.model.is_empty() {
        match panel.provider {
            Some(p) => {
                SharedString::from(format!("{}: {}", dat0_i18n::t("ai.model"), p.model_hint()))
            }
            None => SharedString::from(dat0_i18n::t("ai.model")),
        }
    } else {
        SharedString::from(format!("{}: {}", dat0_i18n::t("ai.model"), panel.model))
    };
    let model_row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(div().child(model_display))
        .child(action_button(
            "ai-model-set",
            dat0_i18n::t("ai.model.set_button"),
            // Empty sentinel: the handler opens the entry prompt and re-dispatches.
            AiPanelEvent::SetModel(String::new()),
            handles.get("ai-model-set"),
            cx,
        ));

    // ── Advanced override + include-sample-rows toggles ────────────────────
    let advanced_label = if panel.advanced_override {
        dat0_i18n::t("ai.advanced.on")
    } else {
        dat0_i18n::t("ai.advanced.off")
    };
    let advanced_row = action_button(
        "ai-toggle-advanced",
        advanced_label,
        AiPanelEvent::ToggleAdvancedOverride,
        handles.get("ai-toggle-advanced"),
        cx,
    );

    let sample_label = if panel.include_sample_rows {
        dat0_i18n::t("ai.sample_rows.on")
    } else {
        dat0_i18n::t("ai.sample_rows.off")
    };
    let sample_row = action_button(
        "ai-toggle-sample-rows",
        sample_label,
        AiPanelEvent::ToggleIncludeSampleRows,
        handles.get("ai-toggle-sample-rows"),
        cx,
    );

    // ── Test connection + result line ──────────────────────────────────────
    let test_button = action_button(
        "ai-test-connection",
        dat0_i18n::t("ai.test"),
        AiPanelEvent::TestConnection,
        handles.get("ai-test-connection"),
        cx,
    );

    let mut panel_div = div()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .child(div().child(SharedString::from(dat0_i18n::t("ai.title"))))
        .child(enabled_row)
        .child(provider_row)
        .child(key_row)
        .child(model_row)
        .child(advanced_row)
        .child(sample_row)
        .child(test_button);

    // Transient Test-connection result; only appended when present so the parent
    // gap_2 leaves no phantom gap when there is no message (mirrors connections).
    if let Some(msg) = &panel.test_result {
        panel_div = panel_div.child(div().child(SharedString::from(msg.clone())));
    }

    panel_div.into_any_element()
}

/// A clickable, keyboard-operable panel button that dispatches `ev` to the shell
/// handler. `focus_stop` makes it a real Tab stop with Enter/Space activation +
/// focus ring (ships in release); the `.a11y` twin (same `id`) is the oracle's
/// label source and a release no-op. The Enter/Space handler calls the SAME
/// `handle_ai_panel_event` the `on_click` does, so keyboard and mouse cannot drift.
fn action_button(
    id: &'static str,
    label: impl Into<SharedString>,
    ev: AiPanelEvent,
    fh: &gpui::FocusHandle,
    cx: &mut Context<WorkspaceShell>,
) -> gpui::Stateful<gpui::Div> {
    let label: SharedString = label.into();
    let ev_key = ev.clone();
    let click = cx.listener(move |ws, _ev, window, cx| {
        ws.handle_ai_panel_event(ev.clone(), window, cx);
    });
    let key = cx.listener(move |ws, _ev: &gpui::KeyDownEvent, window, cx| {
        ws.handle_ai_panel_event(ev_key.clone(), window, cx);
    });
    div()
        .id(id)
        .px_2()
        .py_1()
        .border_1()
        .cursor_pointer()
        .focus_stop(id, fh, 0, key)
        .a11y(id, AccessRole::Button, label.to_string())
        .child(label)
        .on_click(click)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_message_formats() {
        assert_eq!(test_result_message(true, "Connected"), "✓ Connected");
        assert_eq!(
            test_result_message(false, "401 unauthorized"),
            "✗ 401 unauthorized"
        );
    }

    #[test]
    fn event_variants_exist() {
        let _ = [
            AiPanelEvent::SelectProvider(crate::ai::Provider::OpenRouter),
            AiPanelEvent::SetKey("k".into()),
            AiPanelEvent::SetModel("m".into()),
            AiPanelEvent::ToggleEnabled,
            AiPanelEvent::ToggleAdvancedOverride,
            AiPanelEvent::ToggleIncludeSampleRows,
            AiPanelEvent::TestConnection,
            AiPanelEvent::ForgetKey,
        ];
    }
}
