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
