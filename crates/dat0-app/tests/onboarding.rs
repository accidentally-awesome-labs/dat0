use dat0_app::sample_data::{CHINOOK_SQLITE, IRIS_CSV, ensure_bundled_extracted};
use tempfile::tempdir;

#[test]
fn bundled_samples_extract_to_state_root() {
    let dir = tempdir().unwrap();
    let iris = ensure_bundled_extracted(dir.path(), IRIS_CSV, "iris.csv").unwrap();
    let chinook = ensure_bundled_extracted(dir.path(), CHINOOK_SQLITE, "chinook.sqlite").unwrap();
    assert!(iris.exists() && chinook.exists());
    assert_eq!(std::fs::read(&iris).unwrap(), IRIS_CSV);
}
