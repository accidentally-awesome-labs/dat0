//! The BYOK AI panel, and the runtime behind it.
//!
//! Ported from `dat0-app`'s `ai/panel.rs` (the render) and `window/ai.rs` (the
//! handler). The chrome is re-cut; the rules are not:
//!
//! * **The API key is write-only.** It is never echoed into a field, never
//!   held in [`AiDraft`], never written to `settings.toml`, never logged. The
//!   panel shows "key set" / "no key" and nothing more. The value goes
//!   straight to the [`KeyStore`] and, for a request, is moved into the task
//!   and dropped the moment the response lands.
//! * **Two monotonic supersede counters**, exactly as they were.
//!   [`AiState::test_load_id`] guards the Test-connection result and is bumped
//!   by *every* configuration change, so a probe that was in flight when you
//!   switched provider cannot report "✓ Connected" about the old one.
//!   [`AiState::stream_load_id`] guards NL→SQL and Explain — one counter for
//!   both, as upstream, because they share the preview strip and only one may
//!   own it.
//! * **R17: no row data leaves.** Both streams send `sample_rows: None` and a
//!   schema built by `build_schema_context`, which drops the surrogate column.
//!
//! ## Why the completion path is a public method
//!
//! `apply_test_result`, `push_stream_delta` and `finish_stream` all take the
//! `load_id` the work started with and drop the result when it no longer
//! matches. The spawned task calls exactly those methods — there is no second
//! copy of the comparison inside the async block — which is what keeps the
//! rule testable without a provider and stops the two paths drifting.

use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::ai::key_store::KeyStore;
use dat0_core::ai::settings::AiSettings;
use dat0_core::ai::transport::TestOutcome;
use dat0_core::ai::{AiRequest, Provider};
use dat0_core::settings::store::SettingsStore;
use dat0_engine::TableInfo;

use crate::a11y::AccessRole;
use crate::components::import_progress::human_bytes;

/// Format the transient Test-connection result line. Ported verbatim — the
/// glyphs are what the GPUI suite asserted on.
pub fn test_result_message(ok: bool, msg: &str) -> String {
    if ok {
        format!("✓ {msg}")
    } else {
        format!("✗ {msg}")
    }
}

/// The panel's draft state.
///
/// A mirror of the persisted `AiSettings` plus a key-*presence* flag. There is
/// deliberately no key field: a struct that cannot hold the secret cannot leak
/// it through a debug print.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct AiDraft {
    pub provider: Option<Provider>,
    /// Whether the keychain has a key for `provider`. Never the key itself.
    pub key_set: bool,
    pub model: String,
    pub enabled: bool,
    pub advanced_override: bool,
    pub include_sample_rows: bool,
    /// Transient Test-connection line; cleared by the next config action.
    pub test_result: Option<String>,
}

/// Which value the entry prompt is collecting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiEntry {
    Key,
    Model,
}

/// One panel intent. Ported one-for-one from `AiPanelEvent`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AiPanelEvent {
    /// Cycle to this provider.
    SelectProvider(Provider),
    /// Empty string opens the entry prompt; a non-empty value is written to
    /// the keychain. The sentinel is upstream's and is kept so the prompt's
    /// Confirm re-dispatches the same event with the real value.
    SetKey(String),
    /// Same sentinel convention as [`AiPanelEvent::SetKey`].
    SetModel(String),
    ToggleEnabled,
    ToggleAdvancedOverride,
    ToggleIncludeSampleRows,
    TestConnection,
    ForgetKey,
}

/// Which stream owns the preview strip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamKind {
    NlToSql,
    Explain,
}

impl StreamKind {
    pub fn id(self) -> &'static str {
        match self {
            StreamKind::NlToSql => "nl2sql",
            StreamKind::Explain => "explain",
        }
    }

    fn label_key(self) -> &'static str {
        match self {
            StreamKind::NlToSql => "ai.stream.nl2sql",
            StreamKind::Explain => "ai.stream.explain",
        }
    }
}

/// Where a stream is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StreamPhase {
    #[default]
    Idle,
    Streaming,
    Done,
    Failed,
}

impl StreamPhase {
    pub fn id(self) -> &'static str {
        match self {
            StreamPhase::Idle => "idle",
            StreamPhase::Streaming => "streaming",
            StreamPhase::Done => "done",
            StreamPhase::Failed => "failed",
        }
    }
}

/// The live stream, as the console renders it.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct StreamView {
    pub kind: Option<StreamKind>,
    /// What was asked. For Explain this is the SQL buffer.
    pub prompt: String,
    /// Everything received so far.
    pub text: String,
    pub phase: StreamPhase,
    pub error: Option<String>,
}

/// A future a probe hands back.
///
/// Deliberately **not** `Send`. Dioxus tasks run on the window's own local
/// pool, and the delta sink writes `Signal`s, which are single-threaded by
/// construction — requiring `Send` here would force the sink to cross a
/// channel to reach the state it is trying to update.
pub type AiFuture<T> = std::pin::Pin<Box<dyn std::future::Future<Output = T>>>;

/// The network seam.
///
/// A trait rather than a direct call to `ai::transport` so the supersede rule
/// and the streaming states can be driven without a provider, an API key or a
/// socket. Production is [`LiveProbe`].
pub trait AiProbe: 'static {
    fn test_connection(
        &self,
        provider: Provider,
        key: String,
        cfg: AiSettings,
    ) -> AiFuture<TestOutcome>;

    /// Returns the full text, or the error message.
    fn stream(
        &self,
        provider: Provider,
        key: String,
        cfg: AiSettings,
        req: AiRequest,
        on_delta: Box<dyn FnMut(String)>,
    ) -> AiFuture<Result<String, String>>;
}

/// The real thing: `dat0_core::ai::transport`, which carries the SSRF and
/// schema-only guarantees.
pub struct LiveProbe;

impl AiProbe for LiveProbe {
    fn test_connection(
        &self,
        provider: Provider,
        key: String,
        cfg: AiSettings,
    ) -> AiFuture<TestOutcome> {
        Box::pin(async move {
            let outcome = dat0_core::ai::transport::test_connection(provider, &key, &cfg).await;
            // Drop the key as early as possible; it is no longer needed.
            drop(key);
            outcome
        })
    }

    fn stream(
        &self,
        provider: Provider,
        key: String,
        cfg: AiSettings,
        req: AiRequest,
        mut on_delta: Box<dyn FnMut(String)>,
    ) -> AiFuture<Result<String, String>> {
        Box::pin(async move {
            let result = dat0_core::ai::transport::send_stream(provider, &key, &cfg, &req, |d| {
                on_delta(d.to_string())
            })
            .await;
            drop(key);
            result.map_err(|e| e.to_string())
        })
    }
}

/// The panel's reactive state. `Copy`, so a closure captures it for free.
#[derive(Clone, Copy, PartialEq)]
pub struct AiState {
    pub draft: Signal<AiDraft>,
    pub stream: Signal<StreamView>,
    /// Guards the Test-connection result.
    pub test_load_id: Signal<u64>,
    /// Guards NL→SQL and Explain deltas. One counter for both.
    pub stream_load_id: Signal<u64>,
    /// Set when the panel wants a value typed. The host opens its prompt and
    /// re-dispatches `SetKey`/`SetModel` with the answer. An outbox rather than
    /// a callback because the modal slot belongs to the shell, not here.
    pub entry: Signal<Option<AiEntry>>,
}

/// The things the panel cannot recreate: persistence, the keychain, the wire.
#[derive(Clone)]
pub struct AiDeps {
    pub store: Arc<SettingsStore>,
    pub keys: Arc<dyn KeyStore>,
    pub probe: Arc<dyn AiProbe>,
}

impl PartialEq for AiDeps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.store, &other.store)
            && Arc::ptr_eq(&self.keys, &other.keys)
            && Arc::ptr_eq(&self.probe, &other.probe)
    }
}

/// State plus dependencies: everything a handler needs.
#[derive(Clone, PartialEq)]
pub struct AiController {
    pub state: AiState,
    pub deps: AiDeps,
}

impl AiController {
    /// Mount a controller in the current scope, hydrating once from the
    /// persisted settings and a keychain key-presence probe.
    pub fn use_new(deps: AiDeps) -> Self {
        let this = Self {
            state: AiState {
                draft: use_signal(AiDraft::default),
                stream: use_signal(StreamView::default),
                test_load_id: use_signal(|| 0_u64),
                stream_load_id: use_signal(|| 0_u64),
                entry: use_signal(|| None),
            },
            deps,
        };
        let hydrate = this.clone();
        use_hook(move || hydrate.hydrate());
        this
    }

    /// Build a controller from raw parts. For a host that owns its own signals.
    pub fn new(state: AiState, deps: AiDeps) -> Self {
        Self { state, deps }
    }

    /// Load the draft from settings + key presence. Never reads the key value.
    pub fn hydrate(&self) {
        let settings = self.settings();
        let provider = settings.provider.as_deref().and_then(Provider::from_id);
        let key_set = provider.is_some_and(|p| self.key_present(p));
        let mut draft = self.state.draft;
        draft.set(AiDraft {
            provider,
            key_set,
            model: settings.model,
            enabled: settings.enabled,
            advanced_override: settings.advanced_override,
            include_sample_rows: settings.include_sample_rows,
            test_result: None,
        });
    }

    fn key_present(&self, p: Provider) -> bool {
        self.deps.keys.get(p).ok().flatten().is_some()
    }

    /// The persisted AI settings, or defaults when the store cannot be read.
    pub fn settings(&self) -> AiSettings {
        self.deps
            .store
            .load_or_default()
            .map(|s| s.ai)
            .unwrap_or_default()
    }

    /// Mutate the persisted `AiSettings` through the atomic load-mutate-save
    /// path. The key is not a field here, so it can never reach settings.toml.
    fn update_settings(&self, f: impl FnOnce(&mut AiSettings)) {
        let mut settings = match self.deps.store.load_or_default() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?e, "ai: settings load failed; change not persisted");
                return;
            }
        };
        f(&mut settings.ai);
        if let Err(e) = self.deps.store.save(&settings) {
            tracing::warn!(?e, "ai: settings save failed; change not persisted");
        }
    }

    /// Whether AI features are usable. Gates the NL→SQL chip and both streams.
    pub fn ready(&self) -> bool {
        let d = self.state.draft.read();
        d.enabled && d.key_set && !d.model.is_empty()
    }

    /// Bump the Test-connection guard. Every configuration change does this:
    /// the result of a probe issued against the old configuration is not an
    /// answer about the new one.
    fn invalidate_test(&self) {
        let mut id = self.state.test_load_id;
        let next = id().wrapping_add(1);
        id.set(next);
    }

    /// Show the first-use privacy notice exactly once, then persist the ack.
    fn maybe_show_privacy_banner(&self) {
        if !dat0_core::ai::settings::should_show_privacy_banner(self.settings().privacy_ack) {
            return;
        }
        dat0_core::error_ux::banner::push(dat0_core::error_ux::banner::Banner {
            body: dat0_i18n::t("ai.privacy.body"),
            ..dat0_core::error_ux::banner::Banner::info(dat0_i18n::t("ai.privacy.title"))
        });
        self.update_settings(|s| s.privacy_ack = true);
    }

    /// Perform one panel intent.
    pub fn handle(&self, ev: AiPanelEvent) {
        let mut draft = self.state.draft;
        // Any action dismisses a prior Test-connection message.
        draft.write().test_result = None;

        match ev {
            AiPanelEvent::SelectProvider(p) => {
                self.invalidate_test();
                draft.write().provider = Some(p);
                let id = p.id().to_string();
                self.update_settings(|s| s.provider = Some(id));
                // Key presence is per provider, so re-probe rather than carry
                // the old answer over.
                let present = self.key_present(p);
                draft.write().key_set = present;
            }
            AiPanelEvent::SetKey(value) => {
                self.invalidate_test();
                if value.is_empty() {
                    self.state.entry.clone().set(Some(AiEntry::Key));
                    return;
                }
                let Some(provider) = draft.read().provider else {
                    return; // nothing to key
                };
                match self.deps.keys.set(provider, &value) {
                    // Reflect "key set" without retaining the value.
                    Ok(()) => draft.write().key_set = true,
                    Err(e) => {
                        // KeychainKeyStore errors never embed the secret.
                        draft.write().test_result =
                            Some(test_result_message(false, &e.to_string()));
                    }
                }
            }
            AiPanelEvent::SetModel(value) => {
                self.invalidate_test();
                if value.is_empty() {
                    self.state.entry.clone().set(Some(AiEntry::Model));
                    return;
                }
                draft.write().model = value.clone();
                self.update_settings(|s| s.model = value);
            }
            AiPanelEvent::ToggleEnabled => {
                self.invalidate_test();
                let v = !draft.read().enabled;
                draft.write().enabled = v;
                self.update_settings(|s| s.enabled = v);
                if v {
                    self.maybe_show_privacy_banner();
                }
            }
            AiPanelEvent::ToggleAdvancedOverride => {
                self.invalidate_test();
                let v = !draft.read().advanced_override;
                draft.write().advanced_override = v;
                self.update_settings(|s| s.advanced_override = v);
            }
            AiPanelEvent::ToggleIncludeSampleRows => {
                self.invalidate_test();
                let v = !draft.read().include_sample_rows;
                draft.write().include_sample_rows = v;
                self.update_settings(|s| s.include_sample_rows = v);
            }
            AiPanelEvent::ForgetKey => {
                self.invalidate_test();
                if let Some(provider) = draft.read().provider {
                    let _ = self.deps.keys.forget(provider);
                }
                draft.write().key_set = false;
            }
            AiPanelEvent::TestConnection => {
                self.maybe_show_privacy_banner();
                self.spawn_test();
            }
        }
    }

    /// Clear the entry outbox once the host has opened its prompt.
    pub fn take_entry(&self) -> Option<AiEntry> {
        let mut entry = self.state.entry;
        let e = *entry.read();
        if e.is_some() {
            entry.set(None);
        }
        e
    }

    /// Start a Test-connection probe.
    ///
    /// The key and settings are resolved *here*, before the spawn, so the task
    /// captures only owned values and the key is dropped when it ends.
    pub fn spawn_test(&self) {
        let mut draft = self.state.draft;
        let Some(provider) = draft.read().provider else {
            draft.write().test_result = Some(test_result_message(
                false,
                &dat0_i18n::t("ai.test.no_provider"),
            ));
            return;
        };
        let Some(key) = self.deps.keys.get(provider).ok().flatten() else {
            draft.write().test_result =
                Some(test_result_message(false, &dat0_i18n::t("ai.test.no_key")));
            return;
        };
        let cfg = self.settings();

        // Bump before spawning, so a config change arriving mid-flight
        // invalidates this result.
        self.invalidate_test();
        let load_id = (self.state.test_load_id)();
        let this = self.clone();
        let probe = Arc::clone(&self.deps.probe);
        spawn(async move {
            let outcome = probe.test_connection(provider, key, cfg).await;
            this.apply_test_result(load_id, outcome.ok, &outcome.message);
        });
    }

    /// Record a Test-connection result, unless a newer request superseded it.
    ///
    /// The comparison lives here and nowhere else; the spawned task calls this
    /// rather than re-implementing the check inside the async block.
    pub fn apply_test_result(&self, load_id: u64, ok: bool, message: &str) {
        if (self.state.test_load_id)() != load_id {
            tracing::debug!(
                load_id,
                current = (self.state.test_load_id)(),
                "ai: stale test result discarded"
            );
            return;
        }
        self.state.draft.clone().write().test_result = Some(test_result_message(ok, message));
    }

    /// Begin a stream, superseding any that is running. Returns the `load_id`
    /// this stream must quote to be believed.
    pub fn begin_stream(&self, kind: StreamKind, prompt: String) -> u64 {
        let mut id = self.state.stream_load_id;
        let next = id().wrapping_add(1);
        id.set(next);
        self.state.stream.clone().set(StreamView {
            kind: Some(kind),
            prompt,
            text: String::new(),
            phase: StreamPhase::Streaming,
            error: None,
        });
        next
    }

    /// Append one delta, unless a newer stream superseded this one.
    pub fn push_stream_delta(&self, load_id: u64, text: &str) {
        if (self.state.stream_load_id)() != load_id {
            return;
        }
        self.state.stream.clone().write().text.push_str(text);
    }

    /// Close a stream. `err` is `None` on success.
    pub fn finish_stream(&self, load_id: u64, err: Option<String>) {
        if (self.state.stream_load_id)() != load_id {
            return;
        }
        let mut stream = self.state.stream;
        let mut v = stream.write();
        v.phase = if err.is_some() {
            StreamPhase::Failed
        } else {
            StreamPhase::Done
        };
        v.error = err;
    }

    /// Start an NL→SQL or Explain stream over the given catalog.
    ///
    /// R17: `sample_rows` is always `None` and the schema is names and types
    /// only — the surrogate column is dropped by `build_schema_context`.
    /// Returns the `load_id`, or `None` when the preconditions are not met.
    pub fn spawn_stream(
        &self,
        kind: StreamKind,
        prompt: String,
        tables: &[TableInfo],
    ) -> Option<u64> {
        let provider = self.state.draft.read().provider?;
        let key = self.deps.keys.get(provider).ok().flatten()?;
        let cfg = self.settings();
        if cfg.model.is_empty() {
            return None;
        }
        if prompt.trim().is_empty() {
            return None;
        }

        let (schema, note) = dat0_core::ai::schema_ctx::build_schema_context(
            tables,
            dat0_core::ai::schema_ctx::SchemaCaps::default(),
        );
        // The truncation note rides on the prompt, not the schema field: the
        // schema is a structured payload and a sentence is not part of it.
        let mut user_prompt = prompt.clone();
        if let Some(note) = note {
            user_prompt.push_str("\n\n(");
            user_prompt.push_str(&note);
            user_prompt.push(')');
        }
        let system = match kind {
            StreamKind::NlToSql => dat0_core::ai::prompt::nl_to_sql_system(),
            StreamKind::Explain => dat0_core::ai::prompt::explain_system(),
        };
        let req = AiRequest {
            model: cfg.model.clone(),
            system: Some(system.to_string()),
            schema,
            prompt: user_prompt,
            sample_rows: None, // R17: neither stream ever sends row data
            max_tokens: 1024,
        };

        let load_id = self.begin_stream(kind, prompt);
        let this = self.clone();
        let delta_sink = self.clone();
        let probe = Arc::clone(&self.deps.probe);
        spawn(async move {
            let result = probe
                .stream(
                    provider,
                    key,
                    cfg,
                    req,
                    Box::new(move |d: String| delta_sink.push_stream_delta(load_id, &d)),
                )
                .await;
            this.finish_stream(load_id, result.err());
        });
        Some(load_id)
    }
}

/// The provider button's label: the chosen provider, or the unset placeholder.
fn provider_label(provider: Option<Provider>) -> String {
    match provider {
        Some(p) => format!(
            "{}: {}",
            dat0_i18n::t("ai.provider"),
            dat0_i18n::t(&format!("ai.provider.{}", p.id()))
        ),
        None => dat0_i18n::t("ai.provider.unset"),
    }
}

/// Clicking the provider button advances to the next one, wrapping.
fn next_provider(current: Option<Provider>) -> Provider {
    match current {
        None | Some(Provider::Custom) => Provider::Anthropic,
        Some(Provider::Anthropic) => Provider::OpenAI,
        Some(Provider::OpenAI) => Provider::OpenRouter,
        Some(Provider::OpenRouter) => Provider::Custom,
    }
}

/// The egress line. Always shown, including when it reads zero: "no bytes left
/// this machine" is a claim, and a hidden counter cannot make it.
///
/// A `+` marks a channel dat0 cannot meter (the MotherDuck extension owns its
/// own socket), so the figure is a floor and says so.
pub fn egress_line() -> String {
    let total = dat0_core::telemetry::egress::total_sent();
    let suffix = if dat0_core::telemetry::egress::has_unmetered_channel() {
        "+"
    } else {
        ""
    };
    format!(
        "{} {}{suffix}",
        dat0_i18n::t("ai.egress"),
        human_bytes(total)
    )
}

#[derive(Clone, PartialEq, Props)]
pub struct AiPanelProps {
    pub controller: AiController,
}

/// The panel.
#[component]
pub fn AiPanel(props: AiPanelProps) -> Element {
    let ctl = props.controller;
    let draft = ctl.state.draft.read().clone();
    let stream = ctl.state.stream.read().clone();

    let model_display = if draft.model.is_empty() {
        match draft.provider {
            Some(p) => format!("{}: {}", dat0_i18n::t("ai.model"), p.model_hint()),
            None => dat0_i18n::t("ai.model"),
        }
    } else {
        format!("{}: {}", dat0_i18n::t("ai.model"), draft.model)
    };

    rsx! {
        div {
            class: "d0-ai",
            "data-a11y-id": "ai-panel",
            role: AccessRole::Navigation.aria(),
            "aria-label": dat0_i18n::t("ai.title"),

            Row {
                id: "ai-toggle-enabled",
                label: if draft.enabled { dat0_i18n::t("ai.enabled.on") } else { dat0_i18n::t("ai.enabled.off") },
                controller: ctl.clone(),
                event: AiPanelEvent::ToggleEnabled,
            }

            Row {
                id: "ai-provider-cycle",
                label: provider_label(draft.provider),
                controller: ctl.clone(),
                event: AiPanelEvent::SelectProvider(next_provider(draft.provider)),
            }

            div { class: "d0-ai-row",
                span {
                    class: "d0-mono",
                    "data-a11y-id": "ai-key-state",
                    if draft.key_set { {dat0_i18n::t("ai.key.set")} } else { {dat0_i18n::t("ai.key.unset")} }
                }
                Row {
                    id: "ai-key-set",
                    label: dat0_i18n::t("ai.key.set_button"),
                    controller: ctl.clone(),
                    // The empty sentinel: the host opens its prompt and
                    // re-dispatches with the typed value.
                    event: AiPanelEvent::SetKey(String::new()),
                }
                // Kept mounted and merely `aria-disabled` when there is no key.
                // Upstream unmounted it and had to hand focus to a sibling
                // afterwards, because the button a keyboard user had just
                // activated stopped existing under them. `aria-disabled`
                // (rather than `disabled`) keeps it focusable, so focus stays
                // where the user put it.
                Row {
                    id: "ai-key-forget",
                    label: dat0_i18n::t("ai.key.forget"),
                    controller: ctl.clone(),
                    event: AiPanelEvent::ForgetKey,
                    enabled: draft.key_set,
                }
            }

            div { class: "d0-ai-row",
                span { class: "d0-mono", "data-a11y-id": "ai-model-state", "{model_display}" }
                Row {
                    id: "ai-model-set",
                    label: dat0_i18n::t("ai.model.set_button"),
                    controller: ctl.clone(),
                    event: AiPanelEvent::SetModel(String::new()),
                }
            }

            Row {
                id: "ai-toggle-advanced",
                label: if draft.advanced_override { dat0_i18n::t("ai.advanced.on") } else { dat0_i18n::t("ai.advanced.off") },
                controller: ctl.clone(),
                event: AiPanelEvent::ToggleAdvancedOverride,
            }

            Row {
                id: "ai-toggle-sample-rows",
                label: if draft.include_sample_rows { dat0_i18n::t("ai.sample_rows.on") } else { dat0_i18n::t("ai.sample_rows.off") },
                controller: ctl.clone(),
                event: AiPanelEvent::ToggleIncludeSampleRows,
            }

            Row {
                id: "ai-test-connection",
                label: dat0_i18n::t("ai.test"),
                controller: ctl.clone(),
                event: AiPanelEvent::TestConnection,
            }

            if let Some(msg) = draft.test_result.clone() {
                div {
                    class: "d0-mono d0-ai-result",
                    "data-a11y-id": "ai-test-result",
                    role: AccessRole::Label.aria(),
                    "aria-label": "{msg}",
                    "aria-live": "polite",
                    "{msg}"
                }
            }

            // The privacy claim, measured. Shown whether or not AI is on, so
            // turning it on has a visible before and after.
            div {
                class: "d0-mono d0-ai-egress is-ok",
                "data-a11y-id": "ai-egress",
                role: AccessRole::Label.aria(),
                "aria-label": egress_line(),
                "{egress_line()}"
            }

            if stream.phase != StreamPhase::Idle {
                div {
                    class: "d0-ai-stream",
                    "data-a11y-id": "ai-stream",
                    "data-phase": stream.phase.id(),
                    "data-kind": stream.kind.map(StreamKind::id).unwrap_or(""),
                    div { class: "d0-label",
                        if let Some(k) = stream.kind {
                            "{dat0_i18n::t(k.label_key())}"
                        }
                    }
                    pre { class: "d0-mono", "data-a11y-id": "ai-stream-text", "{stream.text}" }
                    if let Some(err) = stream.error.clone() {
                        div {
                            class: "d0-mono is-error",
                            "data-a11y-id": "ai-stream-error",
                            role: AccessRole::Alert.aria(),
                            "aria-label": "{err}",
                            "{err}"
                        }
                    }
                }
            }
        }
    }
}

/// One panel button.
///
/// `<button>` gives Enter/Space activation and the focus ring for free — the
/// whole of what GPUI's `focus_stop` had to hand-roll.
#[component]
fn Row(
    id: &'static str,
    label: String,
    controller: AiController,
    event: AiPanelEvent,
    #[props(default = true)] enabled: bool,
) -> Element {
    rsx! {
        button {
            class: "d0-btn",
            "data-a11y-id": "{id}",
            role: AccessRole::Button.aria(),
            "aria-label": "{label}",
            "aria-disabled": if enabled { "false" } else { "true" },
            onclick: move |_| {
                if enabled {
                    controller.handle(event.clone());
                }
            },
            "{label}"
        }
    }
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
    fn the_provider_cycle_wraps_and_starts_from_unset() {
        assert_eq!(next_provider(None), Provider::Anthropic);
        assert_eq!(next_provider(Some(Provider::Anthropic)), Provider::OpenAI);
        assert_eq!(next_provider(Some(Provider::OpenAI)), Provider::OpenRouter);
        assert_eq!(next_provider(Some(Provider::OpenRouter)), Provider::Custom);
        assert_eq!(next_provider(Some(Provider::Custom)), Provider::Anthropic);
    }

    #[test]
    fn the_provider_label_falls_back_rather_than_echoing_a_key() {
        assert_eq!(provider_label(None), dat0_i18n::t("ai.provider.unset"));
        assert!(provider_label(Some(Provider::OpenRouter)).contains("OpenRouter"));
    }
}
