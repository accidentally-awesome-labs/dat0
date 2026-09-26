//! MotherDuck and attached databases (PD-023, step 5.6c).
//!
//! The connections panel was finished and nothing opened it: Settings'
//! MotherDuck → Manage posted `connections.open`, and nothing handled it. The
//! async half of the panel's intents, which its pure `route` names and leaves
//! to its host, had no host: the token prompt, the connect, the test, the
//! detach. [`ConnectionsHost`] opens the panel and does what its outcomes ask.
//! A MotherDuck connection is recorded in the session and made again when the
//! session lands, while a token is stored; its databases are listed under
//! CONNECTIONS beside the attached SQLite files, whose tables open the same
//! way. Detaching a SQLite file closes its tables' tabs with it.
//!
//! The connect goes through [`Connector`], so a test can stand a database in
//! for MotherDuck and never reach the network, and the token store is
//! provided the same way, so a test never touches the OS keychain.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use dioxus::prelude::*;
use parking_lot::Mutex;

use dat0_core::connections::connect::{list_databases, run_connect, test_result_message};
use dat0_core::connections::token_store::{KeychainTokenStore, MemoryTokenStore, TokenStore};
use dat0_core::connections::{ConnectionStatus, MD_ALIAS, sqlite};
use dat0_core::error_ux::Banner;
use dat0_core::events::{AppEvent, AppEvents};
use dat0_core::session::{PersistedAttachment, PersistedAttachmentKind, Session};
use dat0_engine::{DuckDBEngine, QueryEngine as _, quote_ident};
use dat0_i18n::t;

use crate::components::connections::{Connections, ConnectionsEvent, Outcome, ProbeId, route};
use crate::components::modals::{ModalOutcome, ModalReply};
use crate::state::{Attached, Modal, Workspace};

/// A connect's answer: the status it ended in, and the databases it brought.
pub type Connected = Pin<Box<dyn Future<Output = (ConnectionStatus, Vec<String>)>>>;

/// The network seam: MotherDuck's ATTACH, or a test's stand-in.
pub trait Connector: 'static {
    fn connect(&self, engine: Arc<DuckDBEngine>, token: String) -> Connected;
}

/// MotherDuck itself.
pub struct LiveConnector;

impl Connector for LiveConnector {
    fn connect(&self, engine: Arc<DuckDBEngine>, token: String) -> Connected {
        Box::pin(async move {
            let status = run_connect(engine.clone(), token).await;
            let databases = match status {
                ConnectionStatus::Connected => list_databases(engine).await,
                _ => Vec::new(),
            };
            (status, databases)
        })
    }
}

/// What connecting needs.
#[derive(Clone)]
pub struct Deps {
    pub tokens: Arc<dyn TokenStore>,
    pub connector: Arc<dyn Connector>,
}

/// The window's connections.
#[derive(Clone, Copy)]
pub struct ConnectionsHost {
    /// The panel's state.
    pub state: Signal<Connections>,
    /// MotherDuck's databases and their tables, for CONNECTIONS.
    pub databases: Signal<Vec<Attached>>,
    deps: CopyValue<Deps>,
}

impl ConnectionsHost {
    /// This window's connections. A hook: call it once, from the shell's body.
    pub fn use_new(ws: Workspace) -> Self {
        let deps = use_hook(|| {
            CopyValue::new(try_consume_context::<Deps>().unwrap_or_else(|| {
                // A keychain that will not open leaves MotherDuck to tokens
                // held in memory, as the AI panel's keys are.
                let tokens: Arc<dyn TokenStore> = match KeychainTokenStore::new() {
                    Ok(k) => Arc::new(k),
                    Err(e) => {
                        tracing::warn!("keychain unavailable: {e:#}");
                        Arc::new(MemoryTokenStore::default())
                    }
                };
                Deps {
                    tokens,
                    connector: Arc::new(LiveConnector),
                }
            }))
        });
        let host = Self {
            state: use_signal(Connections::default),
            databases: use_signal(Vec::new),
            deps,
        };
        // A session that lands with MotherDuck recorded connects again, while
        // a token is stored: the attachment lasts only as long as its engine.
        use_effect(move || {
            let Some(session) = ws.session.read().ready().cloned() else {
                return;
            };
            let recorded = session
                .lock()
                .attachments()
                .iter()
                .any(|a| matches!(a.kind, PersistedAttachmentKind::Md));
            if recorded {
                host.act(ws, ConnectionsEvent::ConnectMd, false);
            }
        });
        // The title bar's pill says `live`, and the status bar's engine
        // `motherduck`, while MotherDuck is connected (PD-023, step 5.8c).
        // `Workspace::live` was never written, so the pill said `local`.
        let state = host.state;
        use_effect(move || {
            let connected = matches!(state.read().md_status(), ConnectionStatus::Connected);
            let mut live = ws.live;
            if *live.peek() != connected {
                live.set(connected);
            }
        });
        host
    }

    fn deps(&self) -> Deps {
        self.deps.read().clone()
    }

    /// Open the panel, listing the files attached now.
    pub fn open(&self, ws: Workspace) {
        let mut state = self.state;
        {
            let mut s = state.write();
            let files: Vec<(String, String)> = ws
                .attached
                .peek()
                .iter()
                .map(|a| (a.alias.clone(), a.path.display().to_string()))
                .collect();
            let listed: Vec<String> = s.sqlite().iter().map(|a| a.alias.clone()).collect();
            for alias in listed {
                s.remove_attachment(&alias);
            }
            for (alias, path) in files {
                s.add_sqlite(alias, path);
            }
        }
        let host = *self;
        let mut modal = ws.modal;
        modal.set(Some(Modal::Connections {
            state,
            reply: ModalReply::new(move |outcome| {
                if let ModalOutcome::Connections(ev) = outcome {
                    host.act(ws, ev, true);
                }
            }),
        }));
    }

    /// Do one panel intent. `asked`: the user pressed it, so a missing token
    /// is asked for; a reconnect at landing asks nothing.
    fn act(&self, ws: Workspace, ev: ConnectionsEvent, asked: bool) {
        let deps = self.deps();
        let outcome = {
            let mut state = self.state;
            let mut s = state.write();
            route(&mut s, ev, deps.tokens.as_ref())
        };
        match outcome {
            Outcome::Done => {}
            Outcome::NeedToken if asked => self.ask_token(ws),
            Outcome::NeedToken => {}
            Outcome::Connect { token, probe } => self.connect(ws, token, probe, false),
            Outcome::Test { token, probe } => self.connect(ws, token, probe, true),
            Outcome::Disconnected => self.disconnected(ws),
            Outcome::Detach { alias } => detach(ws, alias),
            Outcome::Browse => browse(ws),
        }
    }

    /// Ask for the token, out of sight; keep it, and connect.
    fn ask_token(&self, ws: Workspace) {
        let host = *self;
        let mut modal = ws.modal;
        modal.set(Some(Modal::NamePrompt {
            title: t("connections.md.token_prompt"),
            initial: String::new(),
            placeholder: None,
            confirm_label: Some(t("connections.md.connect")),
            secret: true,
            reply: ModalReply::new(move |outcome| {
                // Never logged: the token passes through here.
                let token = match outcome {
                    ModalOutcome::Named(token) => token.trim().to_string(),
                    _ => String::new(),
                };
                let kept = !token.is_empty() && {
                    match host.deps().tokens.set(&token) {
                        Ok(()) => true,
                        Err(e) => {
                            let mut state = host.state;
                            state.write().fail(format!("{e:#}"));
                            false
                        }
                    }
                };
                // Back to the panel, once the prompt has cleared the slot.
                spawn(async move {
                    host.open(ws);
                    if kept {
                        host.act(ws, ConnectionsEvent::ConnectMd, true);
                    }
                });
            }),
        }));
    }

    /// ATTACH MotherDuck with `token`, and show what came of it.
    fn connect(&self, ws: Workspace, token: String, probe: ProbeId, testing: bool) {
        let Some(session) = ws.session.peek().ready().cloned() else {
            return;
        };
        let engine = session.lock().engine.clone();
        let (host, connector) = (*self, self.deps().connector);
        spawn(async move {
            let (status, databases) = connector.connect(engine.clone(), token).await;
            let connected = matches!(status, ConnectionStatus::Connected);
            let mut state = host.state;
            let applied = if testing {
                let message = test_result_message(&status);
                state
                    .write()
                    .finish_probe(probe, status, databases.clone(), message)
            } else {
                state
                    .write()
                    .finish_connect(probe, status, databases.clone())
            };
            if applied && connected {
                record_md(&session);
                host.list(engine, databases).await;
            }
        });
    }

    /// List MotherDuck's databases under CONNECTIONS, with their tables.
    async fn list(&self, engine: Arc<DuckDBEngine>, databases: Vec<String>) {
        let mut shown = Vec::new();
        for db in databases {
            let tables = engine.attached_tables(&db).await.unwrap_or_default();
            shown.push(Attached {
                alias: db,
                path: PathBuf::new(),
                tables,
            });
        }
        let mut listed = self.databases;
        listed.set(shown);
    }

    /// MotherDuck is off: it is no longer recorded, and its databases leave
    /// CONNECTIONS. A soft disconnect, as the panel's `Outcome::Disconnected`
    /// says why: no DETACH.
    fn disconnected(&self, ws: Workspace) {
        if let Some(session) = ws.session.peek().ready().cloned() {
            let mut s = session.lock();
            let kept: Vec<PersistedAttachment> = s
                .attachments()
                .iter()
                .filter(|a| !matches!(a.kind, PersistedAttachmentKind::Md))
                .cloned()
                .collect();
            if let Err(e) = s.set_attachments(kept) {
                tracing::warn!(error = %format!("{e:#}"), "could not forget MotherDuck");
            }
        }
        let mut listed = self.databases;
        listed.set(Vec::new());
    }
}

/// Ask this window to open the panel: the menu bar's Connections…, which is
/// handled where the panel's state lives, as Settings' Manage is.
pub fn request(events: &AppEvents) {
    events.send(AppEvent::RunAction {
        id: crate::components::settings_ui::CONNECTIONS_OPEN,
        window: None,
    });
}

/// Record MotherDuck in the session, once.
fn record_md(session: &Arc<Mutex<Session>>) {
    let mut s = session.lock();
    if s.attachments()
        .iter()
        .any(|a| matches!(a.kind, PersistedAttachmentKind::Md))
    {
        return;
    }
    let mut all = s.attachments().to_vec();
    all.push(PersistedAttachment {
        alias: MD_ALIAS.to_string(),
        kind: PersistedAttachmentKind::Md,
    });
    if let Err(e) = s.set_attachments(all) {
        tracing::warn!(error = %format!("{e:#}"), "could not record MotherDuck");
    }
}

/// Detach the SQLite file attached as `alias`: its tables' tabs close, the
/// views they read go, and the session no longer records it.
fn detach(ws: Workspace, alias: String) {
    let Some(session) = ws.session.peek().ready().cloned() else {
        return;
    };
    let mut ws = ws;
    let tables = ws
        .attached
        .peek()
        .iter()
        .find(|a| a.alias == alias)
        .map(|a| a.tables.clone())
        .unwrap_or_default();
    let views: Vec<String> = tables
        .iter()
        .map(|t| sqlite::view_name(&alias, t))
        .collect();
    close_tabs(ws, &views);
    ws.attached.write().retain(|a| a.alias != alias);
    {
        let mut s = session.lock();
        let kept: Vec<PersistedAttachment> = s
            .attachments()
            .iter()
            .filter(|a| a.alias != alias)
            .cloned()
            .collect();
        if let Err(e) = s.set_attachments(kept) {
            tracing::warn!(error = %format!("{e:#}"), "could not forget the file");
        }
    }
    let engine = session.lock().engine.clone();
    spawn(async move {
        for view in &views {
            let _ = engine
                .execute(&format!("DROP VIEW IF EXISTS {}", quote_ident(view)))
                .await;
        }
        if let Err(e) = engine.detach(&alias).await {
            ws.push_banner(Banner::warning_with_body(
                t("connections.detach.failed"),
                format!("{e:#}"),
            ));
        }
    });
}

/// Close the tabs showing any of `tables`, keeping the active one where it
/// can stay.
fn close_tabs(ws: Workspace, tables: &[String]) {
    let (mut tabs, mut active) = (ws.tabs, ws.active);
    let current = (*active.peek()).and_then(|i| tabs.peek().get(i).map(|t| t.table.clone()));
    tabs.write().retain(|t| !tables.contains(&t.table));
    let left = tabs.peek();
    let at = current
        .and_then(|table| left.iter().position(|t| t.table == table))
        .or_else(|| (!left.is_empty()).then_some(0));
    drop(left);
    active.set(at);
}

/// Attach a SQLite file the user picks.
fn browse(ws: Workspace) {
    if !crate::launch::has_desktop() {
        return;
    }
    spawn(async move {
        let Some(path) = crate::files::pick_data_file().await else {
            return;
        };
        let Some(session) = ws.session.peek().ready().cloned() else {
            return;
        };
        crate::sqlite_open::open(ws, session, path).await;
    });
}
