use dat0_app::settings::store::SettingsStore;
use dat0_app::settings_ui::sections;
use dat0_app::settings_ui::sections::profile::ProfileSection;
use dat0_app::settings_ui::sections::theme::{THEME_IDS, ThemeSection};

#[test]
fn registry_has_profile_and_theme() {
    let all = sections::all_sections();
    let ids: Vec<_> = all.iter().map(|s| s.id()).collect();
    assert!(ids.contains(&"profile"), "profile section missing");
    assert!(ids.contains(&"theme"), "theme section missing");
}

#[test]
fn each_section_has_resolvable_name_key() {
    for section in sections::all_sections() {
        let resolved = dat0_i18n::t(section.name_key());
        assert_ne!(
            resolved,
            section.name_key(),
            "section name key {} did not resolve via i18n",
            section.name_key()
        );
    }
}

// --- P3b T11 (D-001 closure) -------------------------------------------------
//
// The plan-verbatim snippet calls `SettingsStore::open_in_memory()` +
// `.set(key, val)` + `.get_string(key)`. The P1 store is TOML-backed and
// exposes `with_path` / `load_or_default` / `save`; T11 added the KV
// facade so the plan contract holds. `set` returns `anyhow::Result<()>`
// (consistent with the existing `save`), so the tests `.unwrap()` it —
// semantically equivalent to the plan-verbatim `store.set(..)` call.

#[test]
fn profile_name_persists_via_settings_store() {
    let store = SettingsStore::open_in_memory();
    store.set("author.name", "Salar").unwrap();
    assert_eq!(store.get_string("author.name"), Some("Salar".to_string()));
}

#[test]
fn profile_email_persists_via_settings_store() {
    let store = SettingsStore::open_in_memory();
    store.set("author.email", "salar.sayyad@gmail.com").unwrap();
    assert_eq!(
        store.get_string("author.email"),
        Some("salar.sayyad@gmail.com".to_string())
    );
}

#[test]
fn theme_dropdown_persists_via_settings_store() {
    let store = SettingsStore::open_in_memory();
    store.set("theme.id", "high-contrast").unwrap();
    assert_eq!(
        store.get_string("theme.id"),
        Some("high-contrast".to_string())
    );
}

#[test]
fn profile_change_closures_round_trip_via_store() {
    // Exercises the closure shapes T13 will mount as the live
    // `Input::on_change` handlers — making sure `ProfileSection`'s
    // exported helpers actually persist through the KV facade today.
    let store = SettingsStore::open_in_memory();
    ProfileSection::on_name_change(&store, "Salar Sayyad").unwrap();
    ProfileSection::on_email_change(&store, "salar.sayyad@gmail.com").unwrap();
    assert_eq!(
        store.get_string("author.name"),
        Some("Salar Sayyad".to_string())
    );
    assert_eq!(
        store.get_string("author.email"),
        Some("salar.sayyad@gmail.com".to_string())
    );
}

#[test]
fn theme_change_closure_round_trips_each_option() {
    // Exercises the closure shape T13 will mount as the live
    // `Select::on_change` handler — every option surfaced by `THEME_IDS`
    // must round-trip through the SettingsStore KV facade.
    let store = SettingsStore::open_in_memory();
    for id in THEME_IDS {
        ThemeSection::on_theme_change(&store, id).unwrap();
        assert_eq!(store.get_string("theme.id"), Some((*id).to_string()));
    }
}

#[test]
fn settings_store_set_rejects_unknown_key() {
    let store = SettingsStore::open_in_memory();
    let err = store
        .set("nonsense.key", "value")
        .expect_err("unknown key should error");
    assert!(
        err.to_string().contains("unknown key"),
        "unexpected error message: {err}"
    );
}
