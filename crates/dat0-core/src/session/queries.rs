//! Persisted SQL query stores (P5b): bounded run history + named saved queries.
//! Pure data + helpers; persistence lives in `SessionState` (session/mod.rs).
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Max retained history entries (per window). Oldest dropped past this.
pub const HISTORY_CAP: usize = 100;

/// One executed query, newest pushed to the back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub sql: String,
    /// Unix epoch millis when the run finished.
    pub ran_at: i64,
    pub ok: bool,
    pub elapsed_ms: u64,
}

/// A user-named saved query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedQuery {
    pub id: Uuid,
    pub name: String,
    pub sql: String,
    pub saved_at: i64,
}

/// Push `e`, evicting the oldest until `len <= HISTORY_CAP`.
pub fn push_history(buf: &mut Vec<HistoryEntry>, e: HistoryEntry) {
    buf.push(e);
    while buf.len() > HISTORY_CAP {
        buf.remove(0);
    }
}

/// Insert `q`, replacing any existing entry with the same (case-insensitive)
/// name; otherwise append. Returns true if an existing entry was replaced.
pub fn upsert_saved(list: &mut Vec<SavedQuery>, q: SavedQuery) -> bool {
    let nl = q.name.to_lowercase();
    if let Some(slot) = list.iter_mut().find(|s| s.name.to_lowercase() == nl) {
        *slot = q;
        true
    } else {
        list.push(q);
        false
    }
}

/// Remove the saved query with `id` if present.
pub fn delete_saved(list: &mut Vec<SavedQuery>, id: Uuid) {
    list.retain(|s| s.id != id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(sql: &str) -> HistoryEntry {
        HistoryEntry {
            sql: sql.into(),
            ran_at: 0,
            ok: true,
            elapsed_ms: 1,
        }
    }

    #[test]
    fn history_caps_at_100_dropping_oldest() {
        let mut buf = Vec::new();
        for i in 0..150 {
            push_history(&mut buf, h(&format!("q{i}")));
        }
        assert_eq!(buf.len(), HISTORY_CAP);
        assert_eq!(buf.first().unwrap().sql, "q50"); // 0..49 evicted
        assert_eq!(buf.last().unwrap().sql, "q149");
    }

    #[test]
    fn upsert_replaces_same_name_case_insensitive() {
        let mut list = Vec::new();
        let replaced = upsert_saved(
            &mut list,
            SavedQuery {
                id: Uuid::now_v7(),
                name: "Daily".into(),
                sql: "a".into(),
                saved_at: 0,
            },
        );
        assert!(!replaced);
        let replaced2 = upsert_saved(
            &mut list,
            SavedQuery {
                id: Uuid::now_v7(),
                name: "daily".into(),
                sql: "b".into(),
                saved_at: 1,
            },
        );
        assert!(replaced2);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].sql, "b");
    }

    #[test]
    fn delete_by_id() {
        let id = Uuid::now_v7();
        let mut list = vec![SavedQuery {
            id,
            name: "x".into(),
            sql: "a".into(),
            saved_at: 0,
        }];
        delete_saved(&mut list, id);
        assert!(list.is_empty());
    }
}
