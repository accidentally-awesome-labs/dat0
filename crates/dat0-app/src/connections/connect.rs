//! Async orchestration of MotherDuck connect/disconnect/forget. The pure
//! `precheck` is unit-tested; the engine-touching `run_connect` is covered by
//! the env-gated engine integration test (T3) + manual UAT.

use anyhow::Result;
use dat0_engine::{DuckDBEngine, QueryEngine as _, types::AttachOpts};
use std::sync::Arc;

use crate::connections::{ConnectionStatus, MD_ALIAS, token_store::TokenStore};

pub enum Precheck {
    NeedToken,
    Ready(String),
}

/// Decide whether we can connect with what's stored. Pure (no network).
pub fn precheck(store: &dyn TokenStore) -> Result<Precheck> {
    Ok(match store.get()? {
        Some(t) if !t.is_empty() => Precheck::Ready(t),
        _ => Precheck::NeedToken,
    })
}

/// Perform the ATTACH using a token already in the store. Maps engine errors
/// to a `ConnectionStatus::Error(localized)`. Returns the terminal status.
pub async fn run_connect(engine: Arc<DuckDBEngine>, token: String) -> ConnectionStatus {
    let opts = AttachOpts {
        token: Some(token),
        ..Default::default()
    };
    // `MD_ALIAS` is passed for the engine's `attach(dsn, alias, opts)` contract
    // but the md arm ignores it — workspace mode attaches the account's dbs
    // under their real names (no alias). See `build_attach_md_sql`.
    match engine.attach("md:", MD_ALIAS, opts).await {
        Ok(()) => ConnectionStatus::Connected,
        Err(dat0_engine::EngineError::MotherDuckAuth) => {
            ConnectionStatus::Error(dat0_i18n::t("connections.error.auth"))
        }
        Err(dat0_engine::EngineError::ExtensionLoad { .. }) => {
            ConnectionStatus::Error(dat0_i18n::t("connections.error.extension"))
        }
        Err(_) => ConnectionStatus::Error(dat0_i18n::t("connections.error.network")),
    }
}

/// Detach every attached MotherDuck database (best-effort). Workspace mode has
/// no single `md` alias, so the caller passes the real db names (from
/// [`list_databases`]).
pub async fn run_disconnect(engine: Arc<DuckDBEngine>, md_databases: Vec<String>) {
    for db in md_databases {
        let _ = engine.detach(&db).await;
    }
}

/// Shallow catalog enumeration for the panel (design §4.3): the names of the
/// attached **MotherDuck** databases only. CI confirmed (run 27028725998) that
/// `duckdb_databases()` tags MotherDuck attachments with `type = 'motherduck'`
/// (their `path` is the db name or `_share/…`, NOT a `md:` URI). The internal
/// `motherduck_info` database (`md_information_schema`) is deliberately excluded
/// by the exact-match. NO per-table origins (D-012 stays deferred). TRIM-VALVE ①.
pub async fn list_databases(engine: Arc<DuckDBEngine>) -> Vec<String> {
    use dat0_engine::QueryEngine as _;
    use duckdb::arrow::array::Array as _;
    let Ok(result) = engine
        .execute(
            "SELECT database_name FROM duckdb_databases() \
             WHERE lower(type) = 'motherduck' ORDER BY 1;",
        )
        .await
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for batch in &result.batches {
        if let Some(arr) = batch
            .column(0)
            .as_any()
            .downcast_ref::<duckdb::arrow::array::StringArray>()
        {
            for i in 0..arr.len() {
                if arr.is_valid(i) {
                    out.push(arr.value(i).to_string());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connections::token_store::{MemoryTokenStore, TokenStore};

    #[test]
    fn connect_requires_a_stored_token() {
        let store = MemoryTokenStore::default();
        // No token yet → precheck returns NeedToken.
        assert!(matches!(precheck(&store).unwrap(), Precheck::NeedToken));
        store.set("tok").unwrap();
        assert!(matches!(precheck(&store).unwrap(), Precheck::Ready(_)));
    }
}
