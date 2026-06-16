//! Per-provider AI key storage (copy of connections/token_store.rs). Keys live
//! ONLY here — never in settings.toml, logs, or telemetry.

use anyhow::Result;
use std::collections::HashMap;

use crate::ai::provider::Provider;

const SERVICE: &str = "dat0.ai";

pub trait KeyStore: Send + Sync {
    fn get(&self, provider: Provider) -> Result<Option<String>>;
    fn set(&self, provider: Provider, key: &str) -> Result<()>;
    fn forget(&self, provider: Provider) -> Result<()>;
}

/// Production store backed by the OS secret store.
pub struct KeychainKeyStore {
    kc: dat0_keychain::Keychain,
}

impl KeychainKeyStore {
    pub fn new() -> Result<Self> {
        Ok(Self {
            kc: dat0_keychain::Keychain::new(SERVICE)?,
        })
    }
}

impl KeyStore for KeychainKeyStore {
    fn get(&self, provider: Provider) -> Result<Option<String>> {
        Ok(self
            .kc
            .get(provider.id())?
            .map(|b| String::from_utf8_lossy(&b).into_owned()))
    }
    fn set(&self, provider: Provider, key: &str) -> Result<()> {
        self.kc.set(provider.id(), key.as_bytes())
    }
    fn forget(&self, provider: Provider) -> Result<()> {
        self.kc.delete(provider.id())
    }
}

/// In-memory store for tests.
#[derive(Default)]
pub struct MemoryKeyStore {
    inner: std::sync::Mutex<HashMap<&'static str, String>>,
}

impl KeyStore for MemoryKeyStore {
    fn get(&self, provider: Provider) -> Result<Option<String>> {
        Ok(self.inner.lock().unwrap().get(provider.id()).cloned())
    }
    fn set(&self, provider: Provider, key: &str) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .insert(provider.id(), key.to_string());
        Ok(())
    }
    fn forget(&self, provider: Provider) -> Result<()> {
        self.inner.lock().unwrap().remove(provider.id());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::provider::Provider;

    #[test]
    fn memory_store_round_trips_per_provider() {
        let s = MemoryKeyStore::default();
        assert!(s.get(Provider::OpenRouter).unwrap().is_none());
        s.set(Provider::OpenRouter, "sk-or-123").unwrap();
        s.set(Provider::Anthropic, "sk-ant-456").unwrap();
        assert_eq!(
            s.get(Provider::OpenRouter).unwrap().as_deref(),
            Some("sk-or-123")
        );
        assert_eq!(
            s.get(Provider::Anthropic).unwrap().as_deref(),
            Some("sk-ant-456")
        );
        s.forget(Provider::OpenRouter).unwrap();
        assert!(s.get(Provider::OpenRouter).unwrap().is_none());
        assert!(s.get(Provider::Anthropic).unwrap().is_some());
    }
}
