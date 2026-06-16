//! Global AI settings (settings.toml). The API KEY is NOT here — it lives only
//! in the keychain (`ai::key_store`).

use serde::{Deserialize, Serialize};

// `Debug` is safe to derive here: `AiSettings` holds no secret. The API key
// lives only in the keychain (`ai::key_store`); `custom_base_url` is a non-secret
// endpoint already stored in plaintext settings.toml.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AiSettings {
    /// Master enable. Off by default; separate from telemetry.
    pub enabled: bool,
    /// Provider id (`ai::Provider::id`). `None` = unset (D5).
    pub provider: Option<String>,
    /// User-entered model name (no default — placeholder hint only).
    pub model: String,
    /// Custom provider base URL (SSRF-validated at use).
    pub custom_base_url: String,
    /// Allow http + private IPs for the Custom provider (local models).
    pub advanced_override: bool,
    /// Include sample rows in the outbound payload (default off — R17).
    pub include_sample_rows: bool,
    /// Set once the first-use privacy banner has been shown.
    pub privacy_ack: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn defaults_are_off_and_unset() {
        let a = AiSettings::default();
        assert!(!a.enabled);
        assert!(a.provider.is_none()); // D5: no default provider
        assert!(a.model.is_empty());
        assert!(!a.advanced_override);
        assert!(!a.include_sample_rows);
        assert!(!a.privacy_ack);
    }

    #[test]
    fn settings_toml_never_contains_a_key() {
        // The struct has no key field, so serialised TOML can never carry a key.
        let mut s = Settings::default();
        s.ai.enabled = true;
        s.ai.provider = Some("openrouter".into());
        s.ai.model = "vendor/model:free".into();
        let toml = toml::to_string_pretty(&s).unwrap();

        // Guard by FIELD NAME, not value prefix (a value scan passes vacuously):
        // assert the serialised [ai] table exposes no secret-bearing field, so a
        // future accidental `key`/`token`/`secret`/`password` field fails here. R17.
        let parsed: toml::Value = toml::from_str(&toml).unwrap();
        let ai_table = parsed
            .get("ai")
            .and_then(|v| v.as_table())
            .expect("settings.toml has an [ai] table");
        for field in ai_table.keys() {
            let lower = field.to_lowercase();
            assert!(
                !(lower.contains("key")
                    || lower.contains("secret")
                    || lower.contains("token")
                    || lower.contains("password")),
                "AiSettings serialised a secret-bearing field: {field}"
            );
        }
        assert!(!toml.contains("sk-")); // belt-and-braces: no literal key value either

        // round-trips
        let back: Settings = toml::from_str(&toml).unwrap();
        assert_eq!(back.ai, s.ai);
    }
}
