//! MotherDuck + attachment connection management (P5c).
pub mod token_store;
pub mod connect;
pub mod routing;

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
}

impl ConnectionManager {
    pub fn md_status(&self) -> &ConnectionStatus {
        self.md.as_ref().unwrap_or(&ConnectionStatus::Disconnected)
    }
    pub fn set_md_status(&mut self, s: ConnectionStatus) {
        self.md = Some(s);
    }
    pub fn md_alias(&self) -> &'static str { MD_ALIAS }
    pub fn sqlite(&self) -> &[Attachment] { &self.sqlite }
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
    /// Aliases currently attached (for the routing classifier, T9).
    pub fn attached_aliases(&self) -> Vec<String> {
        let mut v: Vec<String> = self.sqlite.iter().map(|a| a.alias.clone()).collect();
        if matches!(self.md, Some(ConnectionStatus::Connected)) {
            v.push(MD_ALIAS.to_string());
        }
        v
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
    fn sqlite_attachments_add_and_remove() {
        let mut m = ConnectionManager::default();
        m.add_sqlite("data".into(), "/tmp/x.db".into());
        assert_eq!(m.sqlite().len(), 1);
        m.remove_attachment("data");
        assert!(m.sqlite().is_empty());
    }
}
