use dat0_core::settings::Settings;

#[test]
fn defaults_are_sensible() {
    let s = Settings::default();
    assert_eq!(s.theme.name, "light");
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

#[test]
fn update_auto_check_defaults_true() {
    let s = Settings::default();
    assert!(
        s.update_auto_check,
        "update_auto_check should default to true"
    );
}

#[test]
fn update_auto_check_round_trips() {
    // default is true
    let mut original = Settings::default();
    assert!(original.update_auto_check);

    // serialize and round-trip back
    let serialized = toml::to_string(&original).unwrap();
    let deserialized: Settings = toml::from_str(&serialized).unwrap();
    assert!(deserialized.update_auto_check);

    // opt-out round-trip
    original.update_auto_check = false;
    let serialized2 = toml::to_string(&original).unwrap();
    let deserialized2: Settings = toml::from_str(&serialized2).unwrap();
    assert!(!deserialized2.update_auto_check);
}

/// S9: the settings default and the theme default are the same theme.
///
/// They are declared in two modules that deliberately do not depend on each
/// other — `settings` must not import `theme` — so nothing but this test stops
/// them drifting. They were out of step for the whole redesign: `DEFAULT_ID`
/// went to `"light"` and `Theme::default()` stayed `"dark"`, so a window with
/// no settings file booted light and a window with an untouched settings file
/// booted dark.
#[test]
fn the_default_theme_is_the_default_builtin() {
    assert_eq!(
        Settings::default().theme.name,
        dat0_core::theme::DEFAULT_ID,
        "the settings default and the theme default must name the same builtin"
    );
}
