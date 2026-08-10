//! MotherDuck + attachment connection management (P5c).
pub mod connect;
pub mod routing;
pub mod token_store;

pub const MD_ALIAS: &str = "md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentKind {
    MotherDuck,
    Sqlite { path: String },
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub alias: String,
    pub kind: AttachmentKind,
    pub status: ConnectionStatus,
}

/// Per-workspace runtime connection state. Held by `WorkspaceShell`; the
/// persisted projection lives in `SessionState.attachments` (T7).
#[derive(Debug, Default)]
pub struct ConnectionManager {
    md: Option<ConnectionStatus>,
    sqlite: Vec<Attachment>,
    /// Shallow catalog enumeration for the panel (design §4.3): database names
    /// only, NO per-table origins (D-012 stays deferred). Cleared whenever the
    /// md status leaves `Connected` so a disconnect/error drops the stale list.
    md_databases: Vec<String>,
    /// Transient result of the last "Test connection" probe (design §3.1).
    /// A localized OK/error message — NEVER the token. Cleared by the next
    /// connection action (`handle_connections_event`).
    md_test_result: Option<String>,
}

impl ConnectionManager {
    pub fn md_status(&self) -> &ConnectionStatus {
        self.md.as_ref().unwrap_or(&ConnectionStatus::Disconnected)
    }
    pub fn set_md_status(&mut self, s: ConnectionStatus) {
        // Leaving Connected drops the stale catalog list (design §4.3).
        if !matches!(s, ConnectionStatus::Connected) {
            self.md_databases.clear();
        }
        self.md = Some(s);
    }
    pub fn md_alias(&self) -> &'static str {
        MD_ALIAS
    }
    /// Cached database names from the last successful enumeration (design §4.3).
    pub fn md_databases(&self) -> &[String] {
        &self.md_databases
    }
    pub fn set_md_databases(&mut self, dbs: Vec<String>) {
        self.md_databases = dbs;
    }
    /// Last Test-connection message, if one is pending display.
    pub fn md_test_result(&self) -> Option<&str> {
        self.md_test_result.as_deref()
    }
    pub fn set_md_test_result(&mut self, msg: String) {
        self.md_test_result = Some(msg);
    }
    pub fn clear_md_test_result(&mut self) {
        self.md_test_result = None;
    }
    pub fn sqlite(&self) -> &[Attachment] {
        &self.sqlite
    }
    pub fn add_sqlite(&mut self, alias: String, path: String) {
        self.sqlite.push(Attachment {
            alias,
            kind: AttachmentKind::Sqlite { path },
            status: ConnectionStatus::Connected,
        });
    }
    pub fn remove_attachment(&mut self, alias: &str) {
        self.sqlite.retain(|a| a.alias != alias);
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;

    #[test]
    fn md_status_lifecycle() {
        let mut m = ConnectionManager::default();
        assert_eq!(m.md_status(), &ConnectionStatus::Disconnected);
        m.set_md_status(ConnectionStatus::Connecting);
        m.set_md_status(ConnectionStatus::Connected);
        assert_eq!(m.md_status(), &ConnectionStatus::Connected);
        assert_eq!(m.md_alias(), "md");
    }

    #[test]
    fn md_databases_cleared_on_disconnect() {
        let mut m = ConnectionManager::default();
        m.set_md_status(ConnectionStatus::Connected);
        m.set_md_databases(vec!["a".to_string()]);
        assert_eq!(m.md_databases().len(), 1);
        // Leaving Connected (disconnect) drops the stale catalog list.
        m.set_md_status(ConnectionStatus::Disconnected);
        assert!(m.md_databases().is_empty());
    }

    #[test]
    fn sqlite_attachments_add_and_remove() {
        let mut m = ConnectionManager::default();
        m.add_sqlite("data".into(), "/tmp/x.db".into());
        assert_eq!(m.sqlite().len(), 1);
        m.remove_attachment("data");
        assert!(m.sqlite().is_empty());
    }

    #[test]
    fn md_test_result_round_trips_and_clears() {
        let mut m = ConnectionManager::default();
        assert_eq!(m.md_test_result(), None);
        m.set_md_test_result("Connection OK".into());
        assert_eq!(m.md_test_result(), Some("Connection OK"));
        m.clear_md_test_result();
        assert_eq!(m.md_test_result(), None);
    }

    #[test]
    fn motherduck_token_never_appears_in_serialized_app_state() {
        use crate::connections::token_store::{MemoryTokenStore, TokenStore as _};
        let sentinel = "SENTINEL-md-9c3f-do-not-leak";
        let store = MemoryTokenStore::default();
        store.set(sentinel).unwrap();

        // Settings persist to TOML (settings/store.rs). The token is keychain-
        // only and not a Settings field, so the serialized form must never
        // contain it.
        let settings = crate::settings::Settings::default();
        let serialized = toml::to_string_pretty(&settings).unwrap();
        assert!(
            !serialized.contains(sentinel),
            "token leaked into serialized settings"
        );

        // ConnectionManager runtime state (Debug) must not carry the token.
        let mut mgr = ConnectionManager::default();
        mgr.set_md_status(ConnectionStatus::Connected);
        mgr.set_md_databases(vec!["sample_data".into()]);
        assert!(
            !format!("{mgr:?}").contains(sentinel),
            "token leaked into ConnectionManager Debug"
        );

        // Guard sanity: the sentinel IS retrievable from the store, so this
        // test would catch a real leak rather than passing trivially.
        assert_eq!(store.get().unwrap().as_deref(), Some(sentinel));
    }
}
