//! AI (P9c): the BYOK panel, its settings and privacy banner, and the three
//! async surfaces — test-connection, NL to SQL, and Explain.
//!
//! `hydrate_ai_panel` probes the OS keychain and settings, so B9's restore
//! path deliberately routes through `on_left_panel_shown` in `dock` rather
//! than seeding panel visibility directly: a visible but unhydrated panel
//! was the bug that slice's whole-branch review caught.

use super::*;

/// Which AI field the entry modal is collecting (P9c-1 T9).
#[derive(Debug, Clone, Copy)]
enum AiEntryKind {
    Key,
    Model,
}

impl WorkspaceShell {
    /// Open the on-disk settings store (`config_dir/settings.toml`). Returns
    /// `None` (logging) when the config dir is unavailable — callers skip the
    /// persist rather than crash. The API KEY is never routed through this store
    /// (it lives only in the keychain).
    pub(super) fn settings_store() -> Option<crate::settings::store::SettingsStore> {
        match crate::platform::config_dir() {
            Ok(dir) => Some(crate::settings::store::SettingsStore::with_path(
                dir.join("settings.toml"),
            )),
            Err(e) => {
                tracing::warn!(
                    ?e,
                    "settings_store: config_dir unavailable; settings not persisted"
                );
                None
            }
        }
    }

    /// Toggle the left-dock AI panel. On open, hydrate the draft state from the
    /// persisted `AiSettings` and probe the keychain for whether a key is set for
    /// the selected provider (the key value itself is never read into state).
    pub(crate) fn toggle_ai_panel(&mut self, cx: &mut gpui::Context<Self>) {
        // B7: AI is one of three mutually-exclusive left panels now. The
        // hydrate-on-open this used to do lives in `activate_left_panel`. The
        // method name is kept so its callers (the AI menu action) are untouched.
        self.activate_left_panel(LeftPanel::Ai, cx);
    }

    /// Load the AI-panel draft from persisted settings + keychain key-presence.
    /// Never reads the key value — only whether a key exists for the provider.
    pub(super) fn hydrate_ai_panel(&mut self) {
        let settings = Self::settings_store()
            .and_then(|s| s.load_or_default().ok())
            .map(|s| s.ai)
            .unwrap_or_default();
        let provider = settings
            .provider
            .as_deref()
            .and_then(crate::ai::Provider::from_id);
        let key_set = provider
            .and_then(|p| {
                use crate::ai::key_store::KeyStore as _;
                crate::ai::key_store::KeychainKeyStore::new()
                    .ok()
                    .and_then(|ks| ks.get(p).ok())
                    .flatten()
            })
            .is_some();
        self.ai_panel = crate::ai::panel::AiPanel {
            provider,
            key_set,
            model: settings.model,
            enabled: settings.enabled,
            advanced_override: settings.advanced_override,
            include_sample_rows: settings.include_sample_rows,
            test_result: None,
        };
    }

    /// Mutate the persisted `AiSettings` in place via the atomic settings-write
    /// path (load → mutate → save). The API KEY is never a field here, so it can
    /// never reach settings.toml. Logs + skips on any store error.
    fn update_ai_settings(&self, f: impl FnOnce(&mut crate::ai::AiSettings)) {
        let Some(store) = Self::settings_store() else {
            return;
        };
        let mut settings = match store.load_or_default() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?e, "update_ai_settings: load failed; change not persisted");
                return;
            }
        };
        f(&mut settings.ai);
        if let Err(e) = store.save(&settings) {
            tracing::warn!(?e, "update_ai_settings: save failed; change not persisted");
        }
    }

    /// Show the first-use AI privacy notice exactly once, then persist the ack so it
    /// never reappears (D5 / R17 transparency). Idempotent: gated on the persisted
    /// `privacy_ack`. Banner is text-only (no action buttons — D-021).
    fn maybe_show_ai_privacy_banner(&self) {
        let ack = Self::settings_store()
            .and_then(|s| s.load_or_default().ok())
            .map(|s| s.ai.privacy_ack)
            .unwrap_or(false);
        if crate::ai::settings::should_show_privacy_banner(ack) {
            crate::error_ux::banner::push(crate::error_ux::banner::Banner {
                title: dat0_i18n::t("ai.privacy.title"),
                body: dat0_i18n::t("ai.privacy.body"),
                link: None,
                primary: None,
                secondary: None,
                kind: crate::error_ux::banner::BannerKind::Info,
                dismissible: true,
            });
            self.update_ai_settings(|s| s.privacy_ack = true);
        }
    }

    /// Handle one AI-panel button event. Mirrors [`Self::handle_connections_event`]:
    /// config changes persist to settings.toml (NEVER the key), the key writes to
    /// the keychain, and Test-connection runs `ai::transport::test_connection`
    /// async (off the GPUI main thread), recording a transient result.
    pub(crate) fn handle_ai_panel_event(
        &mut self,
        ev: crate::ai::panel::AiPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::ai::panel::AiPanelEvent;

        // Any config action dismisses a prior Test-connection message.
        self.ai_panel.test_result = None;

        match ev {
            AiPanelEvent::SelectProvider(p) => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                self.ai_panel.provider = Some(p);
                let id = p.id().to_string();
                self.update_ai_settings(|s| s.provider = Some(id));
                // Re-probe the keychain for whether THIS provider has a key set.
                use crate::ai::key_store::KeyStore as _;
                self.ai_panel.key_set = crate::ai::key_store::KeychainKeyStore::new()
                    .ok()
                    .and_then(|ks| ks.get(p).ok())
                    .flatten()
                    .is_some();
                cx.notify();
            }
            // Empty string = open the entry prompt; a non-empty value (re-dispatched
            // from the prompt's Confirm) writes the key to the keychain.
            AiPanelEvent::SetKey(value) => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                if value.is_empty() {
                    self.open_ai_entry_prompt(AiEntryKind::Key, window, cx);
                } else {
                    let Some(provider) = self.ai_panel.provider else {
                        return; // No provider selected → nothing to key.
                    };
                    use crate::ai::key_store::KeyStore as _;
                    match crate::ai::key_store::KeychainKeyStore::new()
                        .and_then(|ks| ks.set(provider, &value))
                    {
                        Ok(()) => {
                            // Reflect "key set" WITHOUT retaining the key value.
                            self.ai_panel.key_set = true;
                        }
                        Err(e) => {
                            // The message must not contain the key (it doesn't —
                            // KeychainKeyStore errors never embed the secret).
                            self.ai_panel.test_result =
                                Some(crate::ai::panel::test_result_message(false, &e.to_string()));
                        }
                    }
                    cx.notify();
                }
            }
            AiPanelEvent::SetModel(value) => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                if value.is_empty() {
                    self.open_ai_entry_prompt(AiEntryKind::Model, window, cx);
                } else {
                    self.ai_panel.model = value.clone();
                    self.update_ai_settings(|s| s.model = value);
                    cx.notify();
                }
            }
            AiPanelEvent::ToggleEnabled => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                self.ai_panel.enabled = !self.ai_panel.enabled;
                let v = self.ai_panel.enabled;
                self.update_ai_settings(|s| s.enabled = v);
                // Show privacy notice on first enable (idempotent: gated by persisted ack).
                if v {
                    self.maybe_show_ai_privacy_banner();
                }
                cx.notify();
            }
            AiPanelEvent::ToggleAdvancedOverride => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                self.ai_panel.advanced_override = !self.ai_panel.advanced_override;
                let v = self.ai_panel.advanced_override;
                self.update_ai_settings(|s| s.advanced_override = v);
                cx.notify();
            }
            AiPanelEvent::ToggleIncludeSampleRows => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                self.ai_panel.include_sample_rows = !self.ai_panel.include_sample_rows;
                let v = self.ai_panel.include_sample_rows;
                self.update_ai_settings(|s| s.include_sample_rows = v);
                cx.notify();
            }
            AiPanelEvent::ForgetKey => {
                // Config changed → any in-flight test result is stale.
                self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
                if let Some(provider) = self.ai_panel.provider {
                    use crate::ai::key_store::KeyStore as _;
                    if let Ok(ks) = crate::ai::key_store::KeychainKeyStore::new() {
                        let _ = ks.forget(provider);
                    }
                }
                self.ai_panel.key_set = false;
                // This button REMOVES ITSELF: `ai-key-forget` is only rendered while
                // `key_set` is true (ai/panel.rs), so the element tracking its focus
                // handle stops painting on the very next frame. Without this, focus is
                // left on a handle no element tracks — a keyboard user lands nowhere and
                // has to Tab from the top again. (The mouse path is affected too, since
                // `focus_stop` chains `track_focus`.) Hand focus to the sibling that
                // survives the removal: "Set key…".
                let set_key = self.hero_focus_handle("ai-key-set", cx);
                set_key.focus(window);
                cx.notify();
            }
            AiPanelEvent::TestConnection => {
                self.maybe_show_ai_privacy_banner();
                self.spawn_ai_test(cx);
            }
        }
        // Keep the NL→SQL chip gate in sync after any AI config mutation.
        self.push_ai_ready_to_console(cx);
    }

    /// Whether AI features are ready to use (enabled + key set + model configured).
    /// Gates the NL→SQL chip and the spawn preamble.
    pub(super) fn ai_ready(&self) -> bool {
        self.ai_panel.enabled && self.ai_panel.key_set && !self.ai_panel.model.is_empty()
    }

    /// Push the current `ai_ready()` state into the SQL console (if built).
    /// Called after any AI config mutation to keep the chip gated correctly.
    fn push_ai_ready_to_console(&mut self, cx: &mut Context<Self>) {
        let ready = self.ai_ready();
        if let Some(console) = &self.sql_console {
            console.update(cx, |c, _cx| c.ai_ready = ready);
        }
    }

    /// Spawn the async AI Test-connection probe. Reads the key from the keychain
    /// (never logged, never held in state), loads the persisted `AiSettings`, and
    /// runs `ai::transport::test_connection` (which carries the SSRF + schema-only
    /// guarantees) off the GPUI main thread. The transient pass/fail is written
    /// back on the main thread via the registry dispatcher — mirrors
    /// [`Self::spawn_md_test`].
    fn spawn_ai_test(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(provider) = self.ai_panel.provider else {
            self.ai_panel.test_result = Some(crate::ai::panel::test_result_message(
                false,
                &dat0_i18n::t("ai.test.no_provider"),
            ));
            cx.notify();
            return;
        };
        // Resolve the key + settings on the main thread BEFORE spawning so the
        // task captures only owned `Send` values. The key is moved straight into
        // the task and dropped when it ends; it is never logged.
        use crate::ai::key_store::KeyStore as _;
        let key = match crate::ai::key_store::KeychainKeyStore::new()
            .ok()
            .and_then(|ks| ks.get(provider).ok())
            .flatten()
        {
            Some(k) => k,
            None => {
                self.ai_panel.test_result = Some(crate::ai::panel::test_result_message(
                    false,
                    &dat0_i18n::t("ai.test.no_key"),
                ));
                cx.notify();
                return;
            }
        };
        let cfg = Self::settings_store()
            .and_then(|s| s.load_or_default().ok())
            .map(|s| s.ai)
            .unwrap_or_default();
        // Supersede guard (mirrors `chart_load_id`): bump before spawning so that
        // any config change that arrives while the request is in flight invalidates
        // this result.
        self.ai_test_load_id = self.ai_test_load_id.wrapping_add(1);
        let load_id = self.ai_test_load_id;
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            let outcome = crate::ai::transport::test_connection(provider, &key, &cfg).await;
            // Drop the key as early as possible (it is no longer needed).
            drop(key);
            let message = crate::ai::panel::test_result_message(outcome.ok, &outcome.message);
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak.upgrade() {
                        ws.update(app, |ws, cx| {
                            // Supersede: a config change arrived while we were in
                            // flight → the result is stale; drop it.
                            if ws.ai_test_load_id != load_id {
                                tracing::debug!(
                                    "spawn_ai_test: stale result discarded \
                                     (load_id={load_id}, current={})",
                                    ws.ai_test_load_id
                                );
                                return;
                            }
                            ws.ai_panel.test_result = Some(message);
                            cx.notify();
                        });
                    }
                });
            } else {
                tracing::warn!("spawn_ai_test: no MainThreadDispatcher installed; result dropped");
            }
        });
        cx.notify();
    }

    /// Spawn an NL→SQL streaming request. Mirrors [`spawn_ai_test`]'s preamble
    /// exactly, then streams deltas into the console's NL preview strip via
    /// per-delta main-thread dispatches guarded by `ai_stream_load_id`.
    ///
    /// R17 safety:
    /// - `sample_rows: None` — NL→SQL never sends row data.
    /// - Schema built from `catalog_tables` via `build_schema_context` (names +
    ///   types only; surrogate `__dat0_rowid` dropped by `SchemaCaps::default()`).
    /// - Guard: `ai_stream_load_id` supersede check inside every dispatched closure.
    pub(super) fn spawn_ai_nl2sql(&mut self, prompt: String, cx: &mut gpui::Context<Self>) {
        use crate::ai::key_store::KeyStore as _;
        let Some(provider) = self.ai_panel.provider else {
            return;
        };
        let key = match crate::ai::key_store::KeychainKeyStore::new()
            .ok()
            .and_then(|ks| ks.get(provider).ok())
            .flatten()
        {
            Some(k) => k,
            None => return, // ai_ready gate prevents this; belt-and-suspenders
        };
        let cfg = Self::settings_store()
            .and_then(|s| s.load_or_default().ok())
            .map(|s| s.ai)
            .unwrap_or_default();
        if cfg.model.is_empty() {
            return;
        }
        // Build schema-only context from the cached catalog (R17: names+types only).
        let (schema, note) = crate::ai::schema_ctx::build_schema_context(
            &self.catalog_tables,
            crate::ai::schema_ctx::SchemaCaps::default(),
        );
        let mut user_prompt = prompt.clone();
        if let Some(note) = note {
            user_prompt.push_str("\n\n(");
            user_prompt.push_str(&note);
            user_prompt.push(')');
        }
        let req = crate::ai::request::AiRequest {
            model: cfg.model.clone(),
            system: Some(crate::ai::prompt::nl_to_sql_system().to_string()),
            schema,
            prompt: user_prompt,
            sample_rows: None, // R17: NL→SQL never sends row data
            max_tokens: 1024,
        };

        self.ai_stream_load_id = self.ai_stream_load_id.wrapping_add(1);
        let load_id = self.ai_stream_load_id;
        if let Some(console) = &self.sql_console {
            console.update(cx, |c, cx| c.begin_nl_preview(prompt.clone(), cx));
        }
        let ws_weak = cx.entity().downgrade();
        let ws_weak_finish = ws_weak.clone();

        tokio::spawn(async move {
            let result = crate::ai::transport::send_stream(provider, &key, &cfg, &req, |delta| {
                let text = delta.to_string();
                let ws_weak_delta = ws_weak.clone();
                if let Some(d) = crate::window_registry::dispatcher() {
                    let _ = d.dispatch(move |app: &mut gpui::App| {
                        if let Some(ws) = ws_weak_delta.upgrade() {
                            ws.update(app, |ws, cx| {
                                if ws.ai_stream_load_id != load_id {
                                    return; // stale → drop
                                }
                                if let Some(console) = &ws.sql_console {
                                    console.update(cx, |c, cx| c.push_nl_delta(&text, cx));
                                }
                            });
                        }
                    });
                }
            })
            .await;
            drop(key);
            let err = result.err().map(|e| e.to_string());
            if let Some(d) = crate::window_registry::dispatcher() {
                let _ = d.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak_finish.upgrade() {
                        ws.update(app, |ws, cx| {
                            if ws.ai_stream_load_id != load_id {
                                return;
                            }
                            if let Some(console) = &ws.sql_console {
                                console.update(cx, |c, cx| c.finish_nl_preview(err, cx));
                            }
                        });
                    }
                });
            }
        });
        cx.notify();
    }

    /// Stream a plain-language explanation of the active-tab SQL buffer into the
    /// Explain side panel (P9c-2 T7). Mirrors `spawn_ai_nl2sql` exactly, with:
    /// - prompt = the whole active buffer SQL (read on main thread before spawn);
    /// - system: `explain_system()`;
    /// - `begin_explain`/`push_explain_delta`/`finish_explain` instead of NL variants;
    /// - `max_tokens: 1024`; `sample_rows: None` (R17 invariant).
    ///
    /// Reuses the single `ai_stream_load_id` counter (no second counter added).
    ///
    /// R17 safety:
    /// - `sample_rows: None` — Explain never sends row data.
    /// - Schema built from `catalog_tables` via `build_schema_context`.
    /// - Guard: `ai_stream_load_id` supersede check inside every dispatched closure.
    pub(super) fn spawn_ai_explain(&mut self, cx: &mut gpui::Context<Self>) {
        use crate::ai::key_store::KeyStore as _;
        let Some(provider) = self.ai_panel.provider else {
            return;
        };
        let key = match crate::ai::key_store::KeychainKeyStore::new()
            .ok()
            .and_then(|ks| ks.get(provider).ok())
            .flatten()
        {
            Some(k) => k,
            None => return, // ai_ready gate prevents this; belt-and-suspenders
        };
        let cfg = Self::settings_store()
            .and_then(|s| s.load_or_default().ok())
            .map(|s| s.ai)
            .unwrap_or_default();
        if cfg.model.is_empty() {
            return;
        }

        // Read the active SQL on the main thread BEFORE spawning (no Send across
        // the tokio boundary; the read needs &App which we have here).
        let sql = match &self.sql_console {
            Some(c) => c.read(cx).active_sql_and_cursor(cx).0,
            None => return,
        };
        if sql.trim().is_empty() {
            return;
        }

        // Build schema-only context from the cached catalog (R17: names+types only).
        let (schema, note) = crate::ai::schema_ctx::build_schema_context(
            &self.catalog_tables,
            crate::ai::schema_ctx::SchemaCaps::default(),
        );
        // The Explain prompt IS the SQL; schema truncation note appended to the
        // prompt text (not the schema field), per R17 design.
        let mut explain_prompt = sql.clone();
        if let Some(note) = note {
            explain_prompt.push_str("\n\n(");
            explain_prompt.push_str(&note);
            explain_prompt.push(')');
        }
        let req = crate::ai::request::AiRequest {
            model: cfg.model.clone(),
            system: Some(crate::ai::prompt::explain_system().to_string()),
            schema,
            prompt: explain_prompt,
            sample_rows: None, // R17: Explain never sends row data
            max_tokens: 1024,
        };

        self.ai_stream_load_id = self.ai_stream_load_id.wrapping_add(1);
        let load_id = self.ai_stream_load_id;
        if let Some(console) = &self.sql_console {
            console.update(cx, |c, cx| c.begin_explain(sql, cx));
        }
        let ws_weak = cx.entity().downgrade();
        let ws_weak_finish = ws_weak.clone();

        tokio::spawn(async move {
            let result = crate::ai::transport::send_stream(provider, &key, &cfg, &req, |delta| {
                let text = delta.to_string();
                let ws_weak_delta = ws_weak.clone();
                if let Some(d) = crate::window_registry::dispatcher() {
                    let _ = d.dispatch(move |app: &mut gpui::App| {
                        if let Some(ws) = ws_weak_delta.upgrade() {
                            ws.update(app, |ws, cx| {
                                if ws.ai_stream_load_id != load_id {
                                    return; // stale → drop
                                }
                                if let Some(console) = &ws.sql_console {
                                    console.update(cx, |c, cx| {
                                        c.push_explain_delta(&text, cx);
                                    });
                                }
                            });
                        }
                    });
                }
            })
            .await;
            drop(key);
            let err = result.err().map(|e| e.to_string());
            if let Some(d) = crate::window_registry::dispatcher() {
                let _ = d.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak_finish.upgrade() {
                        ws.update(app, |ws, cx| {
                            if ws.ai_stream_load_id != load_id {
                                return;
                            }
                            if let Some(console) = &ws.sql_console {
                                console.update(cx, |c, cx| c.finish_explain(err, cx));
                            }
                        });
                    }
                });
            }
        });
        cx.notify();
    }

    /// Open the AI key/model entry modal (reuses
    /// [`NamePrompt`](crate::view::name_prompt::NamePrompt)). On Confirm the entered
    /// value is re-dispatched as the corresponding non-empty `SetKey`/`SetModel`
    /// event (which performs the keychain write / settings save). For a key entry
    /// the value never touches panel state until it is written to the keychain, and
    /// is never echoed back into a field.
    fn open_ai_entry_prompt(
        &mut self,
        kind: AiEntryKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::view::name_prompt::{NamePrompt, NamePromptEvent};
        let label = match kind {
            AiEntryKind::Key => dat0_i18n::t("ai.key.prompt"),
            AiEntryKind::Model => dat0_i18n::t("ai.model.prompt"),
        };
        // B1: remember where focus was BEFORE `NamePrompt::new` moves it to the
        // field, so dismissing the modal can hand focus back.
        self.modal_restore_focus = window.focused(cx);
        let prompt = cx.new(|cx| NamePrompt::new(label, "", window, cx));
        let sub = cx.subscribe_in(
            &prompt,
            window,
            move |ws: &mut Self, _prompt, ev: &NamePromptEvent, window, cx| match ev {
                NamePromptEvent::Confirm(value) => {
                    let value = value.clone();
                    // Close the prompt first.
                    ws.ai_entry_prompt = None;
                    ws.ai_entry_prompt_sub = None;
                    // Restore BEFORE dispatching: a handler that opens another
                    // modal must capture the restored focus, not the field's.
                    ws.restore_modal_focus(window);
                    if value.is_empty() {
                        cx.notify();
                        return;
                    }
                    let ev = match kind {
                        AiEntryKind::Key => crate::ai::panel::AiPanelEvent::SetKey(value),
                        AiEntryKind::Model => crate::ai::panel::AiPanelEvent::SetModel(value),
                    };
                    ws.handle_ai_panel_event(ev, window, cx);
                }
                NamePromptEvent::Cancel => {
                    ws.ai_entry_prompt = None;
                    ws.ai_entry_prompt_sub = None;
                    ws.restore_modal_focus(window);
                    cx.notify();
                }
            },
        );
        self.ai_entry_prompt_sub = Some(sub);
        self.ai_entry_prompt = Some(prompt);
        debug_assert!(
            self.open_modal_count(cx) <= 1,
            "two modals mounted at once ({}) — B1 assumes a single modal; see \
             docs/plans/2026-07-28-dat0-ui-redesign-b1-modal-host-design.md §2.7",
            self.open_modal_count(cx)
        );
        cx.notify();
    }
}
