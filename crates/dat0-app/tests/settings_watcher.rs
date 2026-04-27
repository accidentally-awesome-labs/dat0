use dat0_app::settings::{store::SettingsStore, watcher::SettingsWatcher, Settings};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn watcher_fires_on_change() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let store = SettingsStore::with_path(path.clone());
    store.save(&Settings::default()).unwrap();

    let received: Arc<Mutex<Vec<Settings>>> = Arc::new(Mutex::new(vec![]));
    let recv_clone = received.clone();
    let watcher = SettingsWatcher::start(path.clone(), move |new_settings| {
        recv_clone.lock().unwrap().push(new_settings);
    }).unwrap();

    // Mutate
    let mut s = Settings::default();
    s.profile.author_name = "Updated".into();
    store.save(&s).unwrap();

    // Allow watcher debounce (notify defaults around 100ms)
    std::thread::sleep(Duration::from_millis(500));

    let observed = received.lock().unwrap();
    assert!(!observed.is_empty(), "watcher should have fired");
    assert_eq!(observed.last().unwrap().profile.author_name, "Updated");

    drop(watcher);
}
