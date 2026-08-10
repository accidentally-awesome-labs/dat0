//! The connections surface: the MotherDuck connect flow and the SQLite
//! attachment list.
//!
//! Ported from `connections/panel.rs` (the markup and the per-status button
//! set) and the non-async half of `window/connections.rs`
//! (`handle_connections_event`, which decided everything before a `tokio::spawn`).
//!
//! # Three things worth knowing before changing this
//!
//! **The panel is still a pure function of state.** Every button emits a
//! [`ConnectionsEvent`]; nothing here reaches the keychain or the engine. That
//! was true in GPUI for a reason — the panel is rendered inside the shell,
//! which owns the async flows — and it is what lets the whole surface be
//! driven from a test with no engine and no OS secret store.
//!
//! **[`route`] is the ported handler.** It performs exactly the state
//! transitions `handle_connections_event` performed, takes the token store as
//! a trait object so a test can use `MemoryTokenStore`, and returns an
//! [`Outcome`] naming the async work it deliberately did not do.
//!
//! **Test results are superseded, not merged.** A "Test connection" probe is
//! slow (a network ATTACH) and the user can press Disconnect, Forget or
//! Connect while it is in flight. Without a guard the stale probe's message
//! and status land on top of the newer state — "Connected as md" under a
//! panel that says Disconnected. Every state-changing action bumps a
//! monotonic [`ProbeId`]; a result that does not carry the current id is
//! dropped. This is the `ai_test_load_id` guard from `window/ai.rs:303-327`,
//! same semantics, applied to the connection probe that never had one.

use dioxus::prelude::*;

use dat0_core::connections::connect::{Precheck, precheck};
use dat0_core::connections::token_store::TokenStore;
use dat0_core::connections::{Attachment, AttachmentKind, ConnectionStatus};

use crate::a11y::{AccessRole, format_swatch};

/// One attached SQLite file, as the panel needs it.
///
/// `dat0_core::connections::Attachment` is neither `PartialEq` nor comparable,
/// so it cannot cross a Dioxus prop boundary; this is the same two fields the
/// panel ever read off it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqliteAttachment {
    pub alias: String,
    pub path: String,
}

impl SqliteAttachment {
    /// Project a core attachment. `None` for the MotherDuck entry, which the
    /// panel tracks separately.
    pub fn from_core(att: &Attachment) -> Option<Self> {
        match &att.kind {
            AttachmentKind::Sqlite { path } => Some(Self {
                alias: att.alias.clone(),
                path: path.clone(),
            }),
            AttachmentKind::MotherDuck => None,
        }
    }
}

/// A ticket for one in-flight probe. Opaque so nobody invents one: the only
/// way to get a valid ticket is [`Connections::begin_probe`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeId(u64);

/// The panel's state.
///
/// Mirrors `dat0_core::connections::ConnectionManager` rule for rule — the
/// database list is dropped whenever the status leaves `Connected`, and the
/// test message is cleared by any connection action — plus the supersede
/// counter, which `ConnectionManager` never had.
#[derive(Debug, Clone, PartialEq)]
pub struct Connections {
    md: ConnectionStatus,
    /// Shallow catalog enumeration: database names only.
    md_databases: Vec<String>,
    /// Transient result of the last Test-connection probe. A localised
    /// message — NEVER the token.
    md_test_result: Option<String>,
    sqlite: Vec<SqliteAttachment>,
    /// Monotonic probe ticket. Bumped by every state-changing action, so a
    /// result minted before the change no longer matches.
    probe: u64,
}

impl Default for Connections {
    fn default() -> Self {
        Self {
            md: ConnectionStatus::Disconnected,
            md_databases: Vec::new(),
            md_test_result: None,
            sqlite: Vec::new(),
            probe: 0,
        }
    }
}

impl Connections {
    pub fn md_status(&self) -> &ConnectionStatus {
        &self.md
    }

    /// Set the status, dropping the cached database list whenever the status
    /// leaves `Connected` — a disconnect must not leave stale databases on
    /// screen.
    pub fn set_md_status(&mut self, s: ConnectionStatus) {
        if !matches!(s, ConnectionStatus::Connected) {
            self.md_databases.clear();
        }
        self.md = s;
    }

    pub fn md_databases(&self) -> &[String] {
        &self.md_databases
    }

    pub fn md_test_result(&self) -> Option<&str> {
        self.md_test_result.as_deref()
    }

    pub fn sqlite(&self) -> &[SqliteAttachment] {
        &self.sqlite
    }

    pub fn add_sqlite(&mut self, alias: impl Into<String>, path: impl Into<String>) {
        self.sqlite.push(SqliteAttachment {
            alias: alias.into(),
            path: path.into(),
        });
    }

    pub fn remove_attachment(&mut self, alias: &str) {
        self.sqlite.retain(|a| a.alias != alias);
    }

    /// Record a status the caller could not even attempt — a keychain that
    /// would not open, say. Invalidates any in-flight probe, because the panel
    /// has just moved on.
    pub fn fail(&mut self, message: impl Into<String>) {
        self.invalidate();
        self.set_md_status(ConnectionStatus::Error(message.into()));
    }

    /// Drop any pending test message and invalidate in-flight probes.
    ///
    /// Called at the head of every action, which is what makes an older result
    /// unable to overwrite a newer state.
    fn invalidate(&mut self) -> ProbeId {
        self.md_test_result = None;
        self.probe = self.probe.wrapping_add(1);
        ProbeId(self.probe)
    }

    /// Start a probe: invalidate whatever came before and mint the ticket the
    /// result must carry back.
    pub fn begin_probe(&mut self) -> ProbeId {
        self.invalidate()
    }

    /// Whether `id` is still the live probe.
    pub fn is_current(&self, id: ProbeId) -> bool {
        id.0 == self.probe
    }

    /// Apply a connect result. Returns `false` when the result was superseded
    /// and therefore dropped.
    pub fn finish_connect(
        &mut self,
        id: ProbeId,
        status: ConnectionStatus,
        databases: Vec<String>,
    ) -> bool {
        if !self.is_current(id) {
            tracing::debug!(
                "connections: stale connect result discarded (probe={}, current={})",
                id.0,
                self.probe
            );
            return false;
        }
        let connected = matches!(status, ConnectionStatus::Connected);
        // `set_md_status` clears the list when not Connected, so the databases
        // go in AFTER it or they are wiped by their own success.
        self.set_md_status(status);
        if connected {
            self.md_databases = databases;
        }
        true
    }

    /// Apply a Test-connection result. Same supersede rule as
    /// [`finish_connect`](Self::finish_connect), plus the transient message —
    /// the status pill alone cannot say "still OK" when already Connected.
    pub fn finish_probe(
        &mut self,
        id: ProbeId,
        status: ConnectionStatus,
        databases: Vec<String>,
        message: String,
    ) -> bool {
        if !self.finish_connect(id, status, databases) {
            return false;
        }
        // After the status: `set_md_status` never touches the message, but the
        // ordering is what the GPUI probe documented and reading it the other
        // way round invites a "clean-up" that clears it.
        self.md_test_result = Some(message);
        true
    }
}

/// Intent emitted by a panel button. The names are the GPUI enum's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectionsEvent {
    /// Connect, or Retry from an error state.
    ConnectMd,
    DisconnectMd,
    /// Forget the stored token, and disconnect.
    ForgetMd,
    /// Probe MotherDuck with the stored token and report a transient pass/fail.
    TestMd,
    /// Attach a SQLite file. The file picker belongs to the caller.
    AttachSqlite,
    Detach(String),
}

/// The async work [`route`] deliberately did not do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing further.
    Done,
    /// No token is stored: open the token prompt.
    NeedToken,
    /// ATTACH with this token, then report through
    /// [`Connections::finish_connect`].
    Connect { token: String, probe: ProbeId },
    /// Probe with this token, then report through
    /// [`Connections::finish_probe`].
    Test { token: String, probe: ProbeId },
    /// MotherDuck is off: drop the persisted `md` attachment.
    ///
    /// A SOFT disconnect — deliberately no `DETACH`. In workspace mode DETACH
    /// persists to the account's saved MotherDuck workspace, so a local
    /// disconnect would move the user's cloud database to "Detached
    /// Databases". The in-session attachment lingers harmlessly.
    Disconnected,
    /// DETACH this alias through the engine and drop its persisted entry.
    Detach { alias: String },
    /// Run the SQLite file picker.
    Browse,
}

/// Route one panel intent: the whole of `handle_connections_event` except the
/// spawns.
///
/// Every arm begins by invalidating: any connection action dismisses a prior
/// Test-connection message *and* makes an in-flight probe stale.
pub fn route(state: &mut Connections, ev: ConnectionsEvent, store: &dyn TokenStore) -> Outcome {
    let probe = state.invalidate();

    match ev {
        ConnectionsEvent::ConnectMd | ConnectionsEvent::TestMd => {
            let testing = matches!(ev, ConnectionsEvent::TestMd);
            match precheck(store) {
                Ok(Precheck::NeedToken) => Outcome::NeedToken,
                Ok(Precheck::Ready(token)) => {
                    state.set_md_status(ConnectionStatus::Connecting);
                    if testing {
                        Outcome::Test { token, probe }
                    } else {
                        Outcome::Connect { token, probe }
                    }
                }
                Err(e) => {
                    state.set_md_status(ConnectionStatus::Error(e.to_string()));
                    Outcome::Done
                }
            }
        }
        ConnectionsEvent::DisconnectMd => {
            state.set_md_status(ConnectionStatus::Disconnected);
            Outcome::Disconnected
        }
        ConnectionsEvent::ForgetMd => {
            // Best-effort, exactly as GPUI: a keychain that refuses to delete
            // must not leave the panel stuck Connected.
            let _ = store.forget();
            state.set_md_status(ConnectionStatus::Disconnected);
            Outcome::Disconnected
        }
        ConnectionsEvent::AttachSqlite => Outcome::Browse,
        ConnectionsEvent::Detach(alias) => {
            state.remove_attachment(&alias);
            Outcome::Detach { alias }
        }
    }
}

/// Localised status label for the MotherDuck pill.
pub fn status_label(s: &ConnectionStatus) -> String {
    match s {
        ConnectionStatus::Disconnected => dat0_i18n::t("connections.md.status.disconnected"),
        ConnectionStatus::Connecting => dat0_i18n::t("connections.md.status.connecting"),
        ConnectionStatus::Connected => dat0_i18n::t("connections.md.status.connected"),
        ConnectionStatus::Error(_) => dat0_i18n::t("connections.md.status.error"),
    }
}

/// The status dot's classes.
///
/// Connected pulses green; connecting pulses amber; an error is red and
/// deliberately still, because a pulse reads as "working". Disconnected is the
/// bare dot.
pub fn status_dot_class(s: &ConnectionStatus) -> &'static str {
    match s {
        ConnectionStatus::Connected => "d0-dot is-live",
        ConnectionStatus::Connecting => "d0-dot is-busy",
        ConnectionStatus::Error(_) => "d0-dot is-error",
        ConnectionStatus::Disconnected => "d0-dot",
    }
}

#[derive(Clone, Props, PartialEq)]
pub struct ConnectionsProps {
    /// The panel's state. Shared rather than owned so the caller's async
    /// results can land in the same place the panel reads.
    pub state: Signal<Connections>,
    /// Every button. The caller runs [`route`] and the async work.
    pub on_event: EventHandler<ConnectionsEvent>,
}

#[component]
pub fn ConnectionsPanel(props: ConnectionsProps) -> Element {
    let state = props.state;
    let s = state.read();
    let status = s.md_status().clone();
    let databases: Vec<String> = s.md_databases().to_vec();
    let test_result = s.md_test_result().map(str::to_string);
    let files: Vec<SqliteAttachment> = s.sqlite().to_vec();
    drop(s);

    let on_event = props.on_event;

    rsx! {
        div { class: "d0-conn", "data-a11y-id": "connections",

            // ── MotherDuck ──────────────────────────────────────────────
            section { class: "d0-conn-section", "data-a11y-id": "connections-md",
                div { class: "d0-label", "{dat0_i18n::t(\"connections.md.heading\")}" }

                div { class: "d0-conn-status",
                    span { class: status_dot_class(&status), "data-a11y-id": "connections-md-dot" }
                    span {
                        class: "d0-mono",
                        "data-a11y-id": "connections-md-status",
                        role: AccessRole::Label.aria(),
                        "aria-label": status_label(&status),
                        "{status_label(&status)}"
                    }
                }

                match &status {
                    // Disconnected: connect, or probe the stored token first.
                    ConnectionStatus::Disconnected => rsx! {
                        div { class: "d0-form-actions",
                            ActionButton {
                                id: "connections-md-connect",
                                label: dat0_i18n::t("connections.md.connect"),
                                primary: true,
                                event: ConnectionsEvent::ConnectMd,
                                on_event,
                            }
                            ActionButton {
                                id: "connections-md-test",
                                label: dat0_i18n::t("connections.md.test"),
                                event: ConnectionsEvent::TestMd,
                                on_event,
                            }
                        }
                    },
                    // No buttons while connecting: the only honest actions are
                    // ones that would race the in-flight ATTACH.
                    ConnectionStatus::Connecting => rsx! {},
                    ConnectionStatus::Connected => rsx! {
                        div { class: "d0-form-actions",
                            ActionButton {
                                id: "connections-md-disconnect",
                                label: dat0_i18n::t("connections.md.disconnect"),
                                event: ConnectionsEvent::DisconnectMd,
                                on_event,
                            }
                            ActionButton {
                                id: "connections-md-forget",
                                label: dat0_i18n::t("connections.md.forget"),
                                event: ConnectionsEvent::ForgetMd,
                                on_event,
                            }
                            ActionButton {
                                id: "connections-md-test",
                                label: dat0_i18n::t("connections.md.test"),
                                event: ConnectionsEvent::TestMd,
                                on_event,
                            }
                        }
                    },
                    ConnectionStatus::Error(msg) => rsx! {
                        div {
                            class: "d0-form-error d0-mono",
                            "data-a11y-id": "connections-md-error",
                            role: AccessRole::Alert.aria(),
                            // The localised message carried by the status; it
                            // is the only place the failure reason is shown.
                            "aria-label": msg.clone(),
                            "{msg}"
                        }
                        div { class: "d0-form-actions",
                            ActionButton {
                                id: "connections-md-retry",
                                label: dat0_i18n::t("connections.md.retry"),
                                primary: true,
                                event: ConnectionsEvent::ConnectMd,
                                on_event,
                            }
                        }
                    },
                }

                // Shallow catalog enumeration: names only, and only while
                // Connected — `set_md_status` has already emptied this
                // otherwise, so no second guard is needed.
                if !databases.is_empty() {
                    ul { class: "d0-conn-dbs", "data-a11y-id": "connections-md-databases",
                        for (i , name) in databases.iter().enumerate() {
                            li {
                                key: "{name}",
                                class: "d0-mono",
                                "data-a11y-id": "connections-db-{i}",
                                role: AccessRole::Label.aria(),
                                "aria-label": name.clone(),
                                "{name}"
                            }
                        }
                    }
                }

                if let Some(msg) = test_result {
                    div {
                        class: "d0-mono d0-conn-test",
                        "data-a11y-id": "connections-md-test-result",
                        role: AccessRole::Label.aria(),
                        "aria-label": msg.clone(),
                        "{msg}"
                    }
                }
            }

            // ── Attached files ──────────────────────────────────────────
            section { class: "d0-conn-section", "data-a11y-id": "connections-files",
                div { class: "d0-label", "{dat0_i18n::t(\"connections.files.heading\")}" }

                for att in files.iter() {
                    div {
                        key: "{att.alias}",
                        class: "d0-row",
                        "data-a11y-id": "connections-file-{att.alias}",
                        span { class: "d0-row-name d0-mono",
                            span { class: "d0-swatch {format_swatch(std::path::Path::new(&att.path))}" }
                            "{att.alias} · {att.path}"
                        }
                        ActionButton {
                            id: format!("connections-detach-{}", att.alias),
                            label: dat0_i18n::t("connections.files.detach"),
                            event: ConnectionsEvent::Detach(att.alias.clone()),
                            on_event,
                        }
                    }
                }

                div { class: "d0-form-actions",
                    ActionButton {
                        id: "connections-attach-sqlite",
                        label: dat0_i18n::t("connections.files.attach"),
                        event: ConnectionsEvent::AttachSqlite,
                        on_event,
                    }
                }
            }
        }
    }
}

/// A panel button that emits one intent. Every action in this surface goes
/// through it, so a new button cannot forget its accessible name or its id.
#[component]
fn ActionButton(
    id: String,
    label: String,
    event: ConnectionsEvent,
    on_event: EventHandler<ConnectionsEvent>,
    #[props(default = false)] primary: bool,
) -> Element {
    rsx! {
        button {
            class: if primary { "d0-btn is-primary" } else { "d0-btn is-ghost" },
            "data-a11y-id": "{id}",
            role: AccessRole::Button.aria(),
            "aria-label": label.clone(),
            onclick: move |_| on_event.call(event.clone()),
            "{label}"
        }
    }
}
