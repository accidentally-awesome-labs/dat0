use dat0_app::workspace::source_watcher::SourceWatcher;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn fires_on_change_debounced() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sales.csv");
    std::fs::write(&path, "a\n1\n").unwrap();

    let hits: Arc<Mutex<Vec<std::path::PathBuf>>> = Arc::new(Mutex::new(vec![]));
    let h2 = hits.clone();
    let watcher = SourceWatcher::start(path.clone(), Duration::from_millis(200), move |p| {
        h2.lock().unwrap().push(p);
    })
    .unwrap();

    // Three rapid writes within the debounce window → coalesced to ~1 fire.
    for i in 0..3 {
        std::fs::write(&path, format!("a\n{i}\n")).unwrap();
        std::thread::sleep(Duration::from_millis(30));
    }
    std::thread::sleep(Duration::from_millis(800));

    let after_first = {
        let observed = hits.lock().unwrap();
        assert!(
            !observed.is_empty(),
            "watcher should have fired at least once"
        );
        assert!(
            observed.len() <= 2,
            "debounce should coalesce rapid writes, got {}",
            observed.len()
        );
        assert_eq!(observed.last().unwrap(), &path);
        observed.len()
    };

    // A second distinct burst after the quiet window must re-arm the outer loop
    // and fire again (this is the behavior the nested loop structure exists for).
    for i in 3..6 {
        std::fs::write(&path, format!("a\n{i}\n")).unwrap();
        std::thread::sleep(Duration::from_millis(30));
    }
    std::thread::sleep(Duration::from_millis(800));

    {
        let observed = hits.lock().unwrap();
        assert!(
            observed.len() > after_first,
            "second burst should fire again (loop re-arms), still {}",
            observed.len()
        );
        assert!(
            observed.len() <= after_first + 2,
            "second burst should also coalesce, got {}",
            observed.len()
        );
        assert_eq!(observed.last().unwrap(), &path);
    }
    drop(watcher);
}
