//! `SettingsStore`'s KV facade.
//!
//! Ported from `dat0-app/tests/settings_ui.rs`, whose subject was split in two
//! by the migration. The half that walked a `SettingsSection` trait object
//! registry went with the UI (the registry is now a `const [Section; 9]` with
//! its own tests beside it); the half below never touched a toolkit at all —
//! it is three string keys and an error path — so it belongs here, next to the
//! store it is about.
//!
//! Why the facade exists rather than `load` + field + `save` at each call site:
//! the three fields it exposes are the ones text inputs write on every
//! keystroke, and each write has to be an atomic load-mutate-save or two fields
//! edited in the same second lose one of them. Keeping that in one place is
//! what makes the settings window's per-keystroke persistence safe.

use dat0_core::settings::store::SettingsStore;

/// Every key the facade accepts, and a value that is not the default.
const KEYS: [(&str, &str); 3] = [
    ("author.name", "Ada Lovelace"),
    ("author.email", "ada@example.org"),
    ("theme.id", "high-contrast"),
];

#[test]
fn every_supported_key_round_trips() {
    let store = SettingsStore::open_in_memory();
    for (key, value) in KEYS {
        store.set(key, value).unwrap();
        assert_eq!(
            store.get_string(key).as_deref(),
            Some(value),
            "{key} did not survive the round trip"
        );
    }
    // …and one key's write did not clobber another's: the setter is a
    // load-mutate-save over the whole document, so a missing field read would
    // silently reset its neighbours.
    for (key, value) in KEYS {
        assert_eq!(store.get_string(key).as_deref(), Some(value), "{key}");
    }
}

#[test]
fn a_value_written_through_the_facade_is_in_the_document_proper() {
    // The facade is a view over `Settings`, not a side table. A value that
    // reached only the KV layer would read back fine and be invisible to every
    // consumer that loads the struct.
    let store = SettingsStore::open_in_memory();
    store.set("author.name", "Ada Lovelace").unwrap();
    store.set("theme.id", "light").unwrap();

    let s = store.load_or_default().unwrap();
    assert_eq!(s.profile.author_name, "Ada Lovelace");
    assert_eq!(s.theme.name, "light");
}

#[test]
fn every_builtin_theme_id_round_trips() {
    // The theme control cycles this list; an id the store refused would strand
    // the button on whichever one it could write.
    let store = SettingsStore::open_in_memory();
    for id in dat0_core::theme::BUILTIN_IDS {
        store.set("theme.id", id).unwrap();
        assert_eq!(store.get_string("theme.id").as_deref(), Some(id));
    }
}

#[test]
fn an_empty_field_reads_as_absent_rather_than_present_and_blank() {
    // Callers treat `None` as "fall back to the default". A blank string
    // handed back instead would put an empty author on a sealed package and
    // an unnamed theme on the window.
    let store = SettingsStore::open_in_memory();
    // A fresh document has no author at all — theme.id is excluded because
    // `Settings::default()` ships one, which is the point of it.
    assert_eq!(store.get_string("author.name"), None);
    assert_eq!(store.get_string("author.email"), None);

    for key in ["author.name", "author.email"] {
        store.set(key, "something").unwrap();
        assert!(store.get_string(key).is_some(), "{key}");
        store.set(key, "").unwrap();
        assert_eq!(
            store.get_string(key),
            None,
            "{key} came back as an empty string after being cleared"
        );
    }
}

#[test]
fn an_unknown_key_is_refused_on_write_and_absent_on_read() {
    let store = SettingsStore::open_in_memory();
    let err = store
        .set("nonsense.key", "value")
        .expect_err("an unknown key must not be silently dropped");
    assert!(
        err.to_string().contains("unknown key"),
        "unexpected error message: {err}"
    );
    assert_eq!(store.get_string("nonsense.key"), None);
    // The refusal is total: nothing was written.
    assert_eq!(
        store.load_or_default().unwrap(),
        dat0_core::settings::Settings::default()
    );
}
