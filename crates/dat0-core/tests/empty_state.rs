//! Integration coverage for P3b T7 — sample-data catalog + bundled
//! fixture extraction.
//!
//! The empty-state view itself ([`dat0_core::empty_state::EmptyState`])
//! has an internal `#[cfg(test)]` smoke test in `src/empty_state.rs`;
//! these tests live in `tests/` so they exercise the public surface
//! `sample_data` exposes to the empty-state view.

use dat0_core::sample_data::{self, SampleKind};
use tempfile::tempdir;

#[test]
fn sample_data_lists_three_entries() {
    let e = sample_data::entries();
    assert_eq!(e.len(), 3, "expected exactly 3 sample entries");
    let titles: Vec<&str> = e.iter().map(|s| s.title).collect();
    assert!(titles.contains(&"Iris"));
    assert!(titles.contains(&"Chinook"));
    assert!(titles.contains(&"NYC taxi"));
}

#[test]
fn iris_extracts_to_state_root() {
    let dir = tempdir().unwrap();
    let p = sample_data::ensure_bundled_extracted(dir.path(), sample_data::IRIS_CSV, "iris.csv")
        .expect("extract iris");
    assert!(p.exists());
    assert!(
        p.starts_with(dir.path()),
        "extracted path {p:?} is not under state_root {:?}",
        dir.path()
    );
    let bytes = std::fs::read(&p).unwrap();
    assert_eq!(bytes.as_slice(), sample_data::IRIS_CSV);
}

#[test]
fn nyc_taxi_entry_has_remote_url() {
    let e = sample_data::entries();
    let taxi = e
        .iter()
        .find(|s| s.title == "NYC taxi")
        .expect("NYC taxi entry");
    match &taxi.kind {
        SampleKind::Remote {
            url,
            sha256,
            dest_filename,
            ..
        } => {
            assert!(url.starts_with("https://"));
            assert_eq!(*url, sample_data::NYC_TAXI_URL);
            assert_eq!(*sha256, sample_data::NYC_TAXI_SHA256);
            assert!(dest_filename.ends_with(".parquet"));
        }
        other => panic!("expected SampleKind::Remote, got {other:?}"),
    }
}
