//! dat0 internationalization helpers.
//!
//! Loads the English string table at compile time via `include_str!` and
//! exposes a single `t(key)` lookup. Missing keys return the key itself so
//! gaps surface immediately during development rather than silently
//! rendering an empty string.

use once_cell::sync::Lazy;
use std::collections::HashMap;

static STRINGS: Lazy<HashMap<String, String>> = Lazy::new(|| {
    let raw = include_str!("strings/en.json");
    serde_json::from_str(raw).expect("english string table parses")
});

/// Translate a key to its locale-appropriate string. Returns the key itself
/// if missing — surfaces the gap immediately during development.
pub fn t(key: &str) -> String {
    STRINGS.get(key).cloned().unwrap_or_else(|| key.to_string())
}
