//! Async orchestration of MotherDuck connect/disconnect/forget. The pure
//! `precheck` is unit-tested; the engine-touching `run_connect` is covered by
//! the env-gated engine integration test (T3) + manual UAT.

use std::sync::Arc;
use anyhow::Result;
use dat0_engine::{DuckDBEngine, QueryEngine as _, types::AttachOpts};

use crate::connections::{token_store::TokenStore, MD_ALIAS, ConnectionStatus};

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
    let opts = AttachOpts { token: Some(token), ..Default::default() };
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

pub async fn run_disconnect(engine: Arc<DuckDBEngine>) {
    let _ = engine.detach(MD_ALIAS).await; // best-effort
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
