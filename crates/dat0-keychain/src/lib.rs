//! dat0 cross-platform keychain primitive.
//!
//! Stores small secrets (DB credentials, API keys) in OS-native secret stores:
//! macOS Keychain, Linux Secret Service (libsecret/gnome-keyring/KWallet).
//! Windows is not yet implemented and returns an error at runtime.

use anyhow::Result;

/// Handle to an OS-native secret store, scoped by a service name.
pub struct Keychain {
    service: String,
}

impl Keychain {
    /// Create a new keychain handle bound to `service`. The service string
    /// becomes part of the lookup attributes used by the underlying OS store.
    pub fn new(service: impl Into<String>) -> Result<Self> {
        Ok(Self {
            service: service.into(),
        })
    }

    /// Store `value` under `key`. Overwrites any existing value at `key`.
    pub fn set(&self, key: &str, value: &[u8]) -> Result<()> {
        platform::set(&self.service, key, value)
    }

    /// Retrieve the value stored under `key`, or `None` if absent.
    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        platform::get(&self.service, key)
    }

    /// Delete the value stored under `key`. Idempotent: deleting a missing
    /// key is not an error.
    pub fn delete(&self, key: &str) -> Result<()> {
        platform::delete(&self.service, key)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use anyhow::Result;
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    // errSecItemNotFound — returned when a query matches nothing.
    // See: SecBase.h, Apple Security framework headers.
    const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

    pub fn set(service: &str, key: &str, value: &[u8]) -> Result<()> {
        set_generic_password(service, key, value)?;
        Ok(())
    }

    pub fn get(service: &str, key: &str) -> Result<Option<Vec<u8>>> {
        match get_generic_password(service, key) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn delete(service: &str, key: &str) -> Result<()> {
        match delete_generic_password(service, key) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use anyhow::{Context, Result};

    pub fn set(service: &str, key: &str, value: &[u8]) -> Result<()> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(async {
            let ss = secret_service::SecretService::connect(
                secret_service::EncryptionType::Dh,
            )
            .await?;
            let collection = ss.get_default_collection().await?;
            collection
                .create_item(
                    &format!("dat0/{service}/{key}"),
                    std::collections::HashMap::from([("service", service), ("key", key)]),
                    value,
                    true,
                    "application/octet-stream",
                )
                .await
                .context("create_item")?;
            anyhow::Ok(())
        })
    }

    pub fn get(service: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(async {
            let ss = secret_service::SecretService::connect(
                secret_service::EncryptionType::Dh,
            )
            .await?;
            let attrs = std::collections::HashMap::from([("service", service), ("key", key)]);
            let items = ss.search_items(attrs).await?;
            match items.unlocked.first() {
                Some(item) => Ok(Some(item.get_secret().await?)),
                None => Ok(None),
            }
        })
    }

    pub fn delete(service: &str, key: &str) -> Result<()> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(async {
            let ss = secret_service::SecretService::connect(
                secret_service::EncryptionType::Dh,
            )
            .await?;
            let attrs = std::collections::HashMap::from([("service", service), ("key", key)]);
            for item in ss.search_items(attrs).await?.unlocked {
                item.delete().await?;
            }
            anyhow::Ok(())
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use anyhow::{anyhow, Result};

    pub fn set(_: &str, _: &str, _: &[u8]) -> Result<()> {
        Err(anyhow!("unsupported platform"))
    }

    pub fn get(_: &str, _: &str) -> Result<Option<Vec<u8>>> {
        Err(anyhow!("unsupported platform"))
    }

    pub fn delete(_: &str, _: &str) -> Result<()> {
        Err(anyhow!("unsupported platform"))
    }
}
