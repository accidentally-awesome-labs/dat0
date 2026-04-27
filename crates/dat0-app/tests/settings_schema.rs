use dat0_app::settings::Settings;

#[test]
fn defaults_are_sensible() {
    let s = Settings::default();
    assert_eq!(s.theme.name, "dark");
    assert_eq!(s.profile.author_name, "");
    assert!(!s.telemetry.crash_submission_enabled);
}

#[test]
fn round_trip_toml() {
    let original = Settings::default();
    let serialized = toml::to_string(&original).unwrap();
    let deserialized: Settings = toml::from_str(&serialized).unwrap();
    assert_eq!(original, deserialized);
}
