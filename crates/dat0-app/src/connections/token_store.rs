//! Storage for the single MotherDuck token (design D5). Trait-based so the
//! connect flow is unit-testable with an in-memory backend; production uses the
//! OS keychain via `dat0-keychain`.

use anyhow::Result;

/// Fixed keychain service + key. The key is a plain string so a future
/// multi-account feature can use labelled keys without migrating this one.
const SERVICE: &str = "dat0.motherduck";
const KEY: &str = "token";

pub trait TokenStore: Send + Sync {
    fn get(&self) -> Result<Option<String>>;
    fn set(&self, token: &str) -> Result<()>;
    fn forget(&self) -> Result<()>;
}

/// Production store backed by the OS secret store.
pub struct KeychainTokenStore {
    kc: dat0_keychain::Keychain,
}

impl KeychainTokenStore {
    pub fn new() -> Result<Self> {
        Ok(Self {
            kc: dat0_keychain::Keychain::new(SERVICE)?,
        })
    }
}

impl TokenStore for KeychainTokenStore {
    fn get(&self) -> Result<Option<String>> {
        Ok(self
            .kc
            .get(KEY)?
            .map(|b| String::from_utf8_lossy(&b).into_owned()))
    }
    fn set(&self, token: &str) -> Result<()> {
        self.kc.set(KEY, token.as_bytes())
    }
    fn forget(&self) -> Result<()> {
        self.kc.delete(KEY)
    }
}

/// In-memory store for tests.
#[derive(Default)]
pub struct MemoryTokenStore {
    inner: std::sync::Mutex<Option<String>>,
}

impl TokenStore for MemoryTokenStore {
    fn get(&self) -> Result<Option<String>> {
        Ok(self.inner.lock().unwrap().clone())
    }
    fn set(&self, token: &str) -> Result<()> {
        *self.inner.lock().unwrap() = Some(token.to_string());
        Ok(())
    }
    fn forget(&self) -> Result<()> {
        *self.inner.lock().unwrap() = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trips_and_forgets() {
        let s = MemoryTokenStore::default();
        assert_eq!(s.get().unwrap(), None);
        s.set("tok-abc").unwrap();
        assert_eq!(s.get().unwrap().as_deref(), Some("tok-abc"));
        s.forget().unwrap();
        assert_eq!(s.get().unwrap(), None);
    }
}
