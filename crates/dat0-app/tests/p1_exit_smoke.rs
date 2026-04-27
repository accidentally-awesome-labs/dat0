//! P1 exit gate smoke test — verifies all P1 deliverables are wired.

#[test]
fn settings_toml_round_trip() {
    use dat0_app::settings::{Settings, store::SettingsStore};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let store = SettingsStore::with_path(path);
    let mut s = Settings::default();
    s.profile.author_name = "Test".into();
    store.save(&s).unwrap();
    let r = store.load_or_default().unwrap();
    assert_eq!(r.profile.author_name, "Test");
}

#[test]
fn theme_default_loads() {
    use dat0_app::theme::Theme;
    assert!(Theme::load_builtin("dark").is_ok());
    assert!(Theme::load_builtin("light").is_ok());
    assert!(Theme::load_builtin("high-contrast").is_ok());
}

#[test]
fn i18n_helper_works() {
    assert_eq!(dat0_i18n::t("app.name"), "dat0");
}

#[test]
fn keychain_round_trip() {
    let kc = dat0_keychain::Keychain::new("dat0-p1-smoke").unwrap();
    let _ = kc.delete("smoke");
    kc.set("smoke", b"value").unwrap();
    assert_eq!(
        kc.get("smoke").unwrap().as_deref(),
        Some(b"value".as_slice())
    );
    kc.delete("smoke").unwrap();
}

#[test]
fn telemetry_redacts_paths() {
    use dat0_app::telemetry::redaction::redact_event;
    let mut event = sentry::protocol::Event::default();
    event.exception.values.push(sentry::protocol::Exception {
        ty: "Test".into(),
        value: Some("at /Users/jane/secret/foo.rs".into()),
        ..Default::default()
    });
    let r = redact_event(event).unwrap();
    let v = r.exception.values[0].value.as_deref().unwrap();
    assert!(!v.contains("/Users/jane"));
}

#[test]
fn recents_round_trip() {
    use dat0_app::recents::{RecentEntry, Recents};
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("recents.json");
    let mut r = Recents::with_path(p.clone());
    r.push(RecentEntry::Workspace {
        path: "/tmp/w".into(),
    })
    .unwrap();
    drop(r);
    // No `mut` — we never push to the second handle, only read.
    let r2 = Recents::with_path(p);
    assert_eq!(r2.list().len(), 1);
}

#[test]
fn platform_paths_resolve() {
    assert!(dat0_app::platform::config_dir().is_ok());
    assert!(dat0_app::platform::data_dir().is_ok());
    assert!(dat0_app::platform::cache_dir().is_ok());
}
