//! Integration tests for session.json schema migration (T8 — P4a).
//!
//! Covers:
//!   - v1 fixture  → v2 in-memory state (default new fields)
//!   - v2 fixture  → loads as-is (round-trip)
//!   - forward-incompat (schema_version > current) → UnsupportedVersion error
//!   - unknown v1 fields survive migration via `Tab::extra`
//!   - malformed JSON returns Json error
//!   - missing file returns Io NotFound
//!   - v2 serialise → deserialise round-trip

use std::path::PathBuf;

use tempfile::TempDir;

use dat0_app::session::{SESSION_SCHEMA_VERSION, SessionState, migrate};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A v1 session.json: no schema_version, no transform_stack, no undo_cursor.
const V1_FIXTURE_JSON: &str = r#"{
  "tabs": [
    { "table_name": "orders", "source_path": "/tmp/orders.csv" },
    { "table_name": "customers", "source_path": null }
  ],
  "active_tab": 1
}"#;

/// A v2 session.json with tagged Transformation + Scalar wire format (PD-014).
///
/// Wire format:
///   - Transformation uses `tag = "kind"` (e.g. `"kind": "filter"`)
///   - FilterValue uses `tag = "kind"` (e.g. `"kind": "range"`)
///   - Scalar uses adjacent tags `"type"` + `"value"` (e.g. `"type": "float"`)
const V2_FIXTURE_JSON: &str = r#"{
  "schema_version": 2,
  "tabs": [
    {
      "table_name": "orders",
      "source_path": "/tmp/orders.csv",
      "transform_stack": [
        {
          "kind": "filter",
          "column": "price",
          "op": "between",
          "value": {
            "kind": "range",
            "lo":  { "type": "float", "value": 10.0 },
            "hi":  { "type": "float", "value": 99.99 },
            "inclusive": true
          }
        },
        {
          "kind": "sort",
          "keys": [{ "column": "city", "direction": "asc" }]
        }
      ],
      "undo_cursor": 2
    }
  ],
  "active_tab": 0
}"#;

/// A session.json written by a hypothetical future dat0 version.
const V999_FIXTURE_JSON: &str = r#"{
  "schema_version": 999,
  "tabs": [],
  "active_tab": null
}"#;

/// A v1 session.json that contains a field unknown to this version of dat0.
const V1_WITH_UNKNOWN_FIELDS: &str = r#"{
  "tabs": [
    { "table_name": "orders", "source_path": null, "future_field": "ignore me" }
  ],
  "active_tab": 0
}"#;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn write_temp(tmp: &TempDir, body: &str) -> PathBuf {
    let p = tmp.path().join("session.json");
    std::fs::write(&p, body).unwrap();
    p
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn v1_fixture_migrates_to_v2() {
    let tmp = TempDir::new().unwrap();
    let p = write_temp(&tmp, V1_FIXTURE_JSON);

    let state: SessionState = migrate::load(&p).unwrap();

    assert_eq!(
        state.schema_version, SESSION_SCHEMA_VERSION,
        "schema_version must be bumped to current after migration"
    );
    assert_eq!(state.tabs.len(), 2);
    assert_eq!(state.tabs[0].table_name, "orders");
    assert_eq!(
        state.tabs[0].source_path,
        Some(std::path::PathBuf::from("/tmp/orders.csv"))
    );
    assert!(
        state.tabs[0].transform_stack.is_empty(),
        "default transform_stack should be empty after v1 migration"
    );
    assert_eq!(state.tabs[0].undo_cursor, 0);
    assert_eq!(state.active_tab, Some(1));
}

#[test]
fn v2_fixture_loads_as_is() {
    let tmp = TempDir::new().unwrap();
    let p = write_temp(&tmp, V2_FIXTURE_JSON);

    let state: SessionState = migrate::load(&p).unwrap();

    assert_eq!(state.schema_version, 2);
    assert_eq!(state.tabs.len(), 1);
    assert_eq!(state.tabs[0].transform_stack.len(), 2);
    assert_eq!(state.tabs[0].undo_cursor, 2);
}

#[test]
fn forward_incompat_returns_unsupported_version() {
    let tmp = TempDir::new().unwrap();
    let p = write_temp(&tmp, V999_FIXTURE_JSON);

    match migrate::load(&p) {
        Err(migrate::SessionLoadError::UnsupportedVersion(n)) => {
            assert_eq!(n, 999, "error must carry the actual version number")
        }
        other => panic!("expected UnsupportedVersion(999), got {:?}", other),
    }
}

#[test]
fn unknown_fields_in_v1_flow_through_to_extra() {
    let tmp = TempDir::new().unwrap();
    let p = write_temp(&tmp, V1_WITH_UNKNOWN_FIELDS);

    let state: SessionState = migrate::load(&p).unwrap();
    let extra = &state.tabs[0].extra;

    assert!(
        extra.get("future_field").is_some(),
        "unknown v1 field must survive migration via Tab::extra"
    );
    assert_eq!(
        extra["future_field"],
        serde_json::json!("ignore me"),
        "unknown field value must be preserved verbatim"
    );
}

#[test]
fn malformed_json_returns_json_error() {
    let tmp = TempDir::new().unwrap();
    let p = write_temp(&tmp, "{ bad json");

    match migrate::load(&p) {
        Err(migrate::SessionLoadError::Json(_)) => {}
        other => panic!("expected Json error, got {:?}", other),
    }
}

#[test]
fn missing_file_returns_io_not_found() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("does_not_exist.json");

    match migrate::load(&p) {
        Err(migrate::SessionLoadError::Io(e)) => {
            assert_eq!(
                e.kind(),
                std::io::ErrorKind::NotFound,
                "missing file must yield NotFound"
            );
        }
        other => panic!("expected Io(NotFound), got {:?}", other),
    }
}

#[test]
fn v2_round_trip_through_serialization() {
    // Parse the v2 fixture → re-serialize → re-parse; all fields must survive.
    let state: SessionState = serde_json::from_str(V2_FIXTURE_JSON).unwrap();
    let json = serde_json::to_string_pretty(&state).unwrap();
    let back: SessionState = serde_json::from_str(&json).unwrap();

    assert_eq!(back.schema_version, 2);
    assert_eq!(back.tabs.len(), 1);
    assert_eq!(back.tabs[0].transform_stack.len(), 2);
    assert_eq!(back.tabs[0].undo_cursor, 2);
    assert_eq!(back.active_tab, Some(0));
}

/// Session::recover eagerly calls persist() after loading, so a v1 file is
/// rewritten as v2 on the first open — no subsequent mutation required.
#[tokio::test]
async fn recover_eagerly_writes_back_v2_on_first_open() {
    use std::fs;
    use uuid::Uuid;

    const BUDGET: u64 = 256 * 1024 * 1024;

    // Build a scratch dir with a UUID name (Session::recover parses it).
    let tmp = TempDir::new().unwrap();
    let window_id = Uuid::now_v7();
    let scratch_dir = tmp.path().join("scratch").join(window_id.to_string());
    fs::create_dir_all(&scratch_dir).unwrap();

    // Write a v1 fixture to disk.
    let session_path = scratch_dir.join("session.json");
    fs::write(&session_path, V1_FIXTURE_JSON).unwrap();

    // Recover — this must eagerly persist v2 before returning.
    let _session = dat0_app::session::Session::recover(scratch_dir.clone(), BUDGET)
        .await
        .expect("Session::recover should succeed on a v1 file");

    // The on-disk file must now be v2 without any further persist() call.
    let after = fs::read_to_string(&session_path).unwrap();
    assert!(
        after.contains("\"schema_version\": 2") || after.contains("\"schema_version\":2"),
        "post-recover file should be v2, got: {}",
        &after[..after.len().min(300)]
    );
}

#[test]
fn v1_migration_write_back_produces_v2_on_disk() {
    // Simulates the write-back path: load a v1 file, then re-serialize as v2.
    // This mirrors what Session::recover + Session::persist does on first save
    // after a v1→v2 migration.
    let tmp = TempDir::new().unwrap();
    let p = write_temp(&tmp, V1_FIXTURE_JSON);

    let state = migrate::load(&p).unwrap();
    // Write the migrated state back to disk (simulate atomic-write path).
    let json = serde_json::to_string_pretty(&state).unwrap();
    std::fs::write(&p, &json).unwrap();

    // Re-read: must parse as v2 (no migration needed).
    let state2 = migrate::load(&p).unwrap();
    assert_eq!(
        state2.schema_version, SESSION_SCHEMA_VERSION,
        "written-back file must have current schema_version"
    );
    assert_eq!(state2.tabs.len(), 2);
    assert_eq!(state2.tabs[0].table_name, "orders");
}
