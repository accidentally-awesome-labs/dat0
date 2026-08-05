//! Connections (P9b): MotherDuck connect / test / disconnect, the token
//! prompt, and detaching an attachment.
//!
//! The connect and test spawns reach the OS keychain through the token
//! store. No test drives that path: activating it for real would delete or
//! overwrite a developer's actual stored credential.

use super::*;

impl WorkspaceShell {
    /// Single routing point for the Connections panel's buttons
    /// ([`ConnectionsEvent`]). Runs the async MotherDuck connect/disconnect/forget
    /// flows (T8) and updates the [`ConnectionManager`] + persisted attachment set.
    ///
    /// The engine-touching connect/disconnect paths can only be compile-verified
    /// here (no MotherDuck token in this environment); CI/UAT exercise them later.
    ///
    /// [`ConnectionsEvent`]: crate::connections::panel::ConnectionsEvent
    /// [`ConnectionManager`]: crate::connections::ConnectionManager
    pub(crate) fn handle_connections_event(
        &mut self,
        ev: crate::connections::panel::ConnectionsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::connections::ConnectionStatus;
        use crate::connections::connect::{Precheck, precheck};
        use crate::connections::panel::ConnectionsEvent;
        use crate::connections::token_store::KeychainTokenStore;

        // Any connection action dismisses a prior Test-connection message.
        self.connections.clear_md_test_result();

        match ev {
            // Connect (or Retry from an error state).
            ConnectionsEvent::ConnectMd => {
                let store = match KeychainTokenStore::new() {
                    Ok(s) => s,
                    Err(e) => {
                        self.connections
                            .set_md_status(ConnectionStatus::Error(e.to_string()));
                        cx.notify();
                        return;
                    }
                };
                match precheck(&store) {
                    Ok(Precheck::NeedToken) => self.open_md_token_prompt(window, cx),
                    Ok(Precheck::Ready(token)) => {
                        self.connections.set_md_status(ConnectionStatus::Connecting);
                        cx.notify();
                        self.spawn_md_connect(token, cx);
                    }
                    Err(e) => {
                        self.connections
                            .set_md_status(ConnectionStatus::Error(e.to_string()));
                        cx.notify();
                    }
                }
            }
            // Test connection: same precheck as Connect, but spawns the probe
            // that records a transient pass/fail message.
            ConnectionsEvent::TestMd => {
                let store = match KeychainTokenStore::new() {
                    Ok(s) => s,
                    Err(e) => {
                        self.connections
                            .set_md_status(ConnectionStatus::Error(e.to_string()));
                        cx.notify();
                        return;
                    }
                };
                match precheck(&store) {
                    Ok(Precheck::NeedToken) => self.open_md_token_prompt(window, cx),
                    Ok(Precheck::Ready(token)) => {
                        self.connections.set_md_status(ConnectionStatus::Connecting);
                        cx.notify();
                        self.spawn_md_test(token, cx);
                    }
                    Err(e) => {
                        self.connections
                            .set_md_status(ConnectionStatus::Error(e.to_string()));
                        cx.notify();
                    }
                }
            }
            ConnectionsEvent::DisconnectMd => self.disconnect_md(cx),
            ConnectionsEvent::ForgetMd => {
                // Best-effort token forget, then disconnect.
                if let Ok(store) = KeychainTokenStore::new() {
                    use crate::connections::token_store::TokenStore as _;
                    let _ = store.forget();
                }
                self.disconnect_md(cx);
            }
            // TRIM-VALVE ②: the native file picker is not yet wired into this
            // codebase (files are loaded only via drag-and-drop). The
            // ConnectionManager `add_sqlite`/`remove_attachment` + the async
            // `engine().attach`/`detach` plumbing exist (Detach below uses them),
            // so wiring a picker here is the only remaining piece.
            // TODO P5c: wire native file picker (cx.prompt_for_paths) → attach the
            // chosen sqlite file via engine().attach("sqlite:<path>", alias, …),
            // then self.connections.add_sqlite(alias, path) + persist.
            ConnectionsEvent::AttachSqlite => {}
            ConnectionsEvent::Detach(alias) => self.detach_attachment(alias, cx),
        }
    }

    /// Disconnect MotherDuck: a SOFT disconnect — flip the manager to
    /// Disconnected and drop the persisted md attachment, but DO NOT `DETACH`.
    /// In workspace mode `DETACH` persists to the account's saved MotherDuck
    /// workspace (the db moves to "Detached Databases", needing manual
    /// re-attach), so a local disconnect must not mutate the user's cloud
    /// workspace. The in-session attachment lingers harmlessly until the window
    /// closes; dat0 simply stops surfacing it, and a later Connect is idempotent
    /// (the engine arm skips a redundant ATTACH). Shared by Disconnect + Forget.
    fn disconnect_md(&mut self, cx: &mut Context<Self>) {
        use crate::connections::ConnectionStatus;
        self.connections
            .set_md_status(ConnectionStatus::Disconnected);
        // Drop the persisted md attachment so a session recover does not re-attach.
        let mut sess = self.session.lock();
        let atts: Vec<crate::session::PersistedAttachment> = sess
            .attachments()
            .iter()
            .filter(|a| !matches!(a.kind, crate::session::PersistedAttachmentKind::Md))
            .cloned()
            .collect();
        let _ = sess.set_attachments(atts);
        drop(sess);
        cx.notify();
    }

    /// Detach a sqlite attachment by alias: spawn the async detach, remove it from
    /// the manager, and drop its persisted entry (P5c T11).
    fn detach_attachment(&mut self, alias: String, cx: &mut Context<Self>) {
        let engine = self.engine();
        let alias_for_engine = alias.clone();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let _ = engine.detach(&alias_for_engine).await;
        });
        self.connections.remove_attachment(&alias);
        let mut sess = self.session.lock();
        let atts: Vec<crate::session::PersistedAttachment> = sess
            .attachments()
            .iter()
            .filter(|a| a.alias != alias)
            .cloned()
            .collect();
        let _ = sess.set_attachments(atts);
        drop(sess);
        cx.notify();
    }

    /// Spawn the async MotherDuck connect (mirrors [`save_view_as_table`]'s
    /// engine bridge, P5c T11). Only `Send + 'static` values cross into
    /// `tokio::spawn` — the engine `Arc`, the owned `token` string, and the
    /// `Weak` shell handle. The GPUI entity is touched ONLY inside the dispatcher
    /// closure after `.upgrade()`. On a Connected result the md attachment is
    /// persisted so a session recover re-attaches it.
    ///
    /// The token is never logged: it is moved straight into `run_connect` (which
    /// itself never logs it) and dropped when the task ends.
    ///
    /// [`save_view_as_table`]: Self::save_view_as_table
    fn spawn_md_connect(&mut self, token: String, cx: &mut Context<Self>) {
        let engine = self.engine();
        let engine_for_list = engine.clone();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            let status = crate::connections::connect::run_connect(engine, token).await;
            let connected = matches!(status, crate::connections::ConnectionStatus::Connected);
            // On success, enumerate database names for the panel (design §4.3).
            let dbs = if connected {
                crate::connections::connect::list_databases(engine_for_list).await
            } else {
                Vec::new()
            };
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak.upgrade() {
                        ws.update(app, |ws, cx| {
                            // `set_md_status` clears md_databases when not
                            // Connected, so set the list AFTER it on success.
                            ws.connections.set_md_status(status.clone());
                            if connected {
                                ws.connections.set_md_databases(dbs.clone());
                                // Persist the md attachment (idempotent).
                                let mut sess = ws.session.lock();
                                let mut atts = sess.attachments().to_vec();
                                if !atts.iter().any(|a| {
                                    matches!(a.kind, crate::session::PersistedAttachmentKind::Md)
                                }) {
                                    atts.push(crate::session::PersistedAttachment {
                                        alias: crate::connections::MD_ALIAS.to_string(),
                                        kind: crate::session::PersistedAttachmentKind::Md,
                                    });
                                    let _ = sess.set_attachments(atts);
                                }
                                drop(sess);
                                // Populate the catalog Cloud group immediately (md dbs just attached).
                                ws.refresh_catalog(cx);
                            }
                            cx.notify();
                        });
                    }
                });
            } else {
                tracing::warn!(
                    "spawn_md_connect: no MainThreadDispatcher installed; result dropped"
                );
            }
        });
    }

    /// Spawn the async MotherDuck "Test connection" probe. Identical engine
    /// bridge to [`spawn_md_connect`] (idempotent workspace-mode ATTACH with the
    /// stored token), but additionally records a transient pass/fail message via
    /// `set_md_test_result` so the panel can confirm the probe ran — the status
    /// pill alone cannot signal "still OK" when already Connected. The token is
    /// moved straight into `run_connect` and never logged.
    ///
    /// [`spawn_md_connect`]: Self::spawn_md_connect
    fn spawn_md_test(&mut self, token: String, cx: &mut Context<Self>) {
        let engine = self.engine();
        let engine_for_list = engine.clone();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            let status = crate::connections::connect::run_connect(engine, token).await;
            let connected = matches!(status, crate::connections::ConnectionStatus::Connected);
            let message = crate::connections::connect::test_result_message(&status);
            let dbs = if connected {
                crate::connections::connect::list_databases(engine_for_list).await
            } else {
                Vec::new()
            };
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak.upgrade() {
                        ws.update(app, |ws, cx| {
                            ws.connections.set_md_status(status.clone());
                            if connected {
                                ws.connections.set_md_databases(dbs.clone());
                                // Persist the md attachment (idempotent) so a
                                // recover re-attaches it — matches spawn_md_connect.
                                let mut sess = ws.session.lock();
                                let mut atts = sess.attachments().to_vec();
                                if !atts.iter().any(|a| {
                                    matches!(a.kind, crate::session::PersistedAttachmentKind::Md)
                                }) {
                                    atts.push(crate::session::PersistedAttachment {
                                        alias: crate::connections::MD_ALIAS.to_string(),
                                        kind: crate::session::PersistedAttachmentKind::Md,
                                    });
                                    let _ = sess.set_attachments(atts);
                                }
                                drop(sess);
                                // Populate the catalog Cloud group immediately (md dbs just attached).
                                ws.refresh_catalog(cx);
                            }
                            // Set the message AFTER status (set_md_status never
                            // touches md_test_result).
                            ws.connections.set_md_test_result(message);
                            cx.notify();
                        });
                    }
                });
            } else {
                tracing::warn!("spawn_md_test: no MainThreadDispatcher installed; result dropped");
            }
        });
    }

    /// On workspace load, if this session had MotherDuck attached, background-
    /// reconnect it (design §5). Non-md workspaces never touch the network: the
    /// early return guards on the persisted attachment set. The token comes from
    /// the keychain (never session.json); if it is gone, we leave the panel
    /// Disconnected so the user can reconnect manually.
    pub(crate) fn reconnect_persisted_md(&mut self, cx: &mut Context<Self>) {
        use crate::connections::ConnectionStatus;
        use crate::connections::connect::{Precheck, precheck};
        use crate::connections::token_store::KeychainTokenStore;
        let has_md = self
            .session
            .lock()
            .attachments()
            .iter()
            .any(|a| matches!(a.kind, crate::session::PersistedAttachmentKind::Md));
        if !has_md {
            return;
        }
        let Ok(store) = KeychainTokenStore::new() else {
            return;
        };
        if let Ok(Precheck::Ready(token)) = precheck(&store) {
            self.connections.set_md_status(ConnectionStatus::Connecting);
            cx.notify();
            self.spawn_md_connect(token, cx);
        }
        // NeedToken / errors: leave Disconnected (panel shows Connect).
    }

    /// Open the MotherDuck token-entry modal (reuses
    /// [`NamePrompt`](crate::view::name_prompt::NamePrompt), P5c T11). On Confirm
    /// the entered token is stored in the keychain, the prompt closes, the manager
    /// flips to Connecting, and the async connect spawns. On Cancel the prompt is
    /// just dismissed.
    ///
    /// Needs `&mut Window` because `NamePrompt::new` builds a single-line
    /// `InputState` eagerly. The subscription is stored in `md_token_prompt_sub`
    /// (a dropped `Subscription` deregisters the callback silently — the P4a T10b
    /// trap).
    fn open_md_token_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::view::name_prompt::{NamePrompt, NamePromptEvent};
        // B1: remember where focus was BEFORE `NamePrompt::new` moves it to the
        // field, so dismissing the modal can hand focus back.
        self.modal_restore_focus = window.focused(cx);
        let prompt = cx
            .new(|cx| NamePrompt::new(dat0_i18n::t("connections.md.token_prompt"), "", window, cx));
        let sub = cx.subscribe_in(
            &prompt,
            window,
            |ws: &mut Self, _prompt, ev: &NamePromptEvent, window, cx| match ev {
                NamePromptEvent::Confirm(token) => {
                    use crate::connections::ConnectionStatus;
                    use crate::connections::token_store::{KeychainTokenStore, TokenStore as _};
                    let token = token.clone();
                    // Close the prompt first.
                    ws.md_token_prompt = None;
                    ws.md_token_prompt_sub = None;
                    ws.restore_modal_focus(window);
                    // Store the token; on failure surface an error and stop.
                    match KeychainTokenStore::new().and_then(|s| s.set(&token)) {
                        Ok(()) => {
                            ws.connections.set_md_status(ConnectionStatus::Connecting);
                            cx.notify();
                            ws.spawn_md_connect(token, cx);
                        }
                        Err(e) => {
                            ws.connections
                                .set_md_status(ConnectionStatus::Error(e.to_string()));
                            cx.notify();
                        }
                    }
                }
                NamePromptEvent::Cancel => {
                    ws.md_token_prompt = None;
                    ws.md_token_prompt_sub = None;
                    ws.restore_modal_focus(window);
                    cx.notify();
                }
            },
        );
        self.md_token_prompt_sub = Some(sub);
        self.md_token_prompt = Some(prompt);
        debug_assert!(
            self.open_modal_count(cx) <= 1,
            "two modals mounted at once ({}) — B1 assumes a single modal; see \
             docs/plans/2026-07-28-dat0-ui-redesign-b1-modal-host-design.md §2.7",
            self.open_modal_count(cx)
        );
        cx.notify();
    }
}
