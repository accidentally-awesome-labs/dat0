use dat0_app::settings::store::SettingsStore;
use tempfile::tempdir;

#[test]
fn writes_then_reads_roundtrip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let store = SettingsStore::with_path(path.clone());

    let mut s = store.load_or_default().unwrap();
    s.profile.author_name = "Jane Doe".into();
    s.profile.author_email = "jane@example.org".into();
    store.save(&s).unwrap();

    let reloaded = store.load_or_default().unwrap();
    assert_eq!(reloaded.profile.author_name, "Jane Doe");
    assert_eq!(reloaded.profile.author_email, "jane@example.org");
}

#[test]
fn missing_file_returns_default() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("does-not-exist.toml");
    let store = SettingsStore::with_path(path);
    let s = store.load_or_default().unwrap();
    assert_eq!(s.theme.name, "dark");
}

// ---------------------------------------------------------------------------
// Durability tests (PD-002 closure — settings.toml side)
//
// Full fsync cannot be asserted in-process (the kernel decides when pages
// reach stable storage). What we CAN assert:
//  (a) save → reload round-trips correctly (content contract),
//  (b) overwriting an existing settings.toml yields a complete valid file —
//      not a partial or truncated one (write-then-rename contract), and
//  (c) no settings.toml.tmp is left behind after a successful save (atomic
//      rename contract — mirrors session/mod.rs `persist_is_atomic_via_rename`).
// These serve as regression guards for the durable-write path.
// ---------------------------------------------------------------------------

#[test]
fn save_no_tmp_file_left_after_successful_save() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let store = SettingsStore::with_path(path.clone());

    let mut s = store.load_or_default().unwrap();
    s.profile.author_name = "Durability Test".into();
    store.save(&s).unwrap();

    // The tmp file must not exist after a successful save.
    let tmp_path = dir.path().join("settings.toml.tmp");
    assert!(
        !tmp_path.exists(),
        "settings.toml.tmp should not exist after a successful save"
    );
}

#[test]
fn save_overwrite_yields_complete_valid_toml() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let store = SettingsStore::with_path(path.clone());

    // First write.
    let mut s = store.load_or_default().unwrap();
    s.profile.author_name = "First".into();
    store.save(&s).unwrap();

    // Second write — overwrite with different content.
    let mut s2 = store.load_or_default().unwrap();
    s2.profile.author_name = "Second".into();
    s2.profile.author_email = "second@example.org".into();
    store.save(&s2).unwrap();

    // Reload must yield a fully valid TOML, not a partial/truncated file.
    let reloaded = store
        .load_or_default()
        .expect("should parse as valid TOML after overwrite");
    assert_eq!(reloaded.profile.author_name, "Second");
    assert_eq!(reloaded.profile.author_email, "second@example.org");

    // Tmp file must be gone.
    let tmp_path = dir.path().join("settings.toml.tmp");
    assert!(
        !tmp_path.exists(),
        "settings.toml.tmp should not exist after overwrite"
    );
}

#[test]
fn save_durability_round_trip_all_fields() {
    // Exercises every settings field to ensure the write path serialises
    // + deserialises the complete struct without truncation.
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let store = SettingsStore::with_path(path.clone());

    let mut s = store.load_or_default().unwrap();
    s.profile.author_name = "Alice Exemplar".into();
    s.profile.author_email = "alice@example.com".into();
    s.theme.name = "light".into();
    store.save(&s).unwrap();

    let got = store.load_or_default().unwrap();
    assert_eq!(got.profile.author_name, "Alice Exemplar");
    assert_eq!(got.profile.author_email, "alice@example.com");
    assert_eq!(got.theme.name, "light");
}
