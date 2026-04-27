//! T14 — macOS native menu bar integration test.
//!
//! Confirms that the i18n keys exposed by `menu_macos::menu_i18n_keys()`
//! all resolve to actual translated strings (not just the key itself, which
//! is what `dat0_i18n::t()` returns when a key is missing). On non-macOS
//! platforms the menu module compiles but exposes the same key list so the
//! invariant holds cross-platform.

#[cfg(target_os = "macos")]
#[test]
fn menu_keys_resolve_to_non_key_strings() {
    let keys = dat0_app::menu_macos::menu_i18n_keys();
    assert!(keys.len() >= 5, "expected at least 5 top-level menu keys");
    for key in keys {
        let resolved = dat0_i18n::t(key);
        assert_ne!(resolved, *key, "key {key} did not resolve");
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn menu_module_is_noop() {
    let _ = dat0_app::menu_macos::menu_i18n_keys();
}
