use dat0_core::settings::store::SettingsStore;
use dat0_core::settings::{set_log_level, set_memory_budget_mb};

#[test]
fn memory_budget_defaults_to_1024_and_round_trips() {
    let store = SettingsStore::open_in_memory();
    assert_eq!(store.load_or_default().unwrap().memory_budget_mb, 1024);
    set_memory_budget_mb(&store, 4096).unwrap();
    assert_eq!(store.load_or_default().unwrap().memory_budget_mb, 4096);
}

#[test]
fn budget_helper_converts_mb_to_bytes() {
    let store = SettingsStore::open_in_memory();
    set_memory_budget_mb(&store, 2048).unwrap();
    assert_eq!(
        dat0_core::settings::budget::memory_budget_bytes(&store),
        2048u64 * 1024 * 1024
    );
}

#[test]
fn log_level_defaults_and_round_trips() {
    let store = SettingsStore::open_in_memory();
    assert_eq!(
        store.load_or_default().unwrap().log_level,
        "info,dat0=debug"
    );
    set_log_level(&store, "warn").unwrap();
    assert_eq!(store.load_or_default().unwrap().log_level, "warn");
}
