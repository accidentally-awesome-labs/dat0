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

/// A v2 session.json (filter + sort only — no Edit/RowDelete). Owned `String`
/// so callers (e.g. [`inject_unknown_kind`]) can mutate the parsed JSON.
fn sample_v2_session_json() -> String {
    V2_FIXTURE_JSON.to_string()
}

/// Take a v2 sample JSON and splice an unrecognized transform `kind` into the
/// first tab's `transform_stack` — simulating a session a newer dat0 binary
/// wrote with a variant this build doesn't know. The `schema_version` is also
/// bumped to the current version (5) so the doc looks like a real current file
/// (the unknown-kind guard, not the version arm, is what must reject it).
fn inject_unknown_kind(sample: String) -> String {
    let mut doc: serde_json::Value = serde_json::from_str(&sample).unwrap();
    doc["schema_version"] = serde_json::json!(5);
    let stack = doc["tabs"][0]["transform_stack"].as_array_mut().unwrap();
    stack.push(serde_json::json!({
        "kind": "frobnicate",
        "intensity": 11,
        "rows": [{ "kind": "surrogate", "id": 0 }]
    }));
    serde_json::to_string(&doc).unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn v1_fixture_migrates_to_v5() {
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
fn v2_fixture_migrates_to_v5_preserving_content() {
    // v2 → v5 is an identity migration (T13): the version stamp bumps to the
    // current schema, but the transform stack + cursor pass through unchanged.
    let tmp = TempDir::new().unwrap();
    let p = write_temp(&tmp, V2_FIXTURE_JSON);

    let state: SessionState = migrate::load(&p).unwrap();

    assert_eq!(state.schema_version, SESSION_SCHEMA_VERSION);
    assert_eq!(state.schema_version, 5);
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
/// rewritten as the current schema (v5) on the first open — no subsequent
/// mutation required.
#[tokio::test]
async fn recover_eagerly_writes_back_current_version_on_first_open() {
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

    // Recover — this must eagerly persist the current schema before returning.
    let _session = dat0_app::session::Session::recover(scratch_dir.clone(), BUDGET)
        .await
        .expect("Session::recover should succeed on a v1 file");

    // The on-disk file must now be the current schema (v5) without any further
    // persist() call.
    let after = fs::read_to_string(&session_path).unwrap();
    assert!(
        after.contains("\"schema_version\": 5") || after.contains("\"schema_version\":5"),
        "post-recover file should be current schema (v5), got: {}",
        &after[..after.len().min(300)]
    );
}

#[test]
fn v1_migration_write_back_produces_current_version_on_disk() {
    // Simulates the write-back path: load a v1 file, then re-serialize at the
    // current schema version. This mirrors what Session::recover +
    // Session::persist does on first save after a v1→current migration.
    let tmp = TempDir::new().unwrap();
    let p = write_temp(&tmp, V1_FIXTURE_JSON);

    let state = migrate::load(&p).unwrap();
    // Write the migrated state back to disk (simulate atomic-write path).
    let json = serde_json::to_string_pretty(&state).unwrap();
    std::fs::write(&p, &json).unwrap();

    // Re-read: must parse at the current version (no migration needed).
    let state2 = migrate::load(&p).unwrap();
    assert_eq!(
        state2.schema_version, SESSION_SCHEMA_VERSION,
        "written-back file must have current schema_version"
    );
    assert_eq!(state2.tabs.len(), 2);
    assert_eq!(state2.tabs[0].table_name, "orders");
}

// ---------------------------------------------------------------------------
// T13 — v2 → v5 identity migration + unknown-`kind` forward-incompat guard
// ---------------------------------------------------------------------------

#[test]
fn v2_session_migrates_to_v5_identity() {
    // A v2 session.json (no Edit/RowDelete — filter + sort only) loads and is
    // stamped v5 with NO field reshape: the stack + cursor pass through unchanged.
    let v2 = sample_v2_session_json();
    let migrated = migrate::load_str(&v2).unwrap();

    assert_eq!(migrated.schema_version, 5);
    // Identity: the active stack contents and cursor are preserved verbatim.
    assert_eq!(migrated.tabs.len(), 1);
    assert_eq!(migrated.tabs[0].transform_stack.len(), 2);
    assert_eq!(migrated.tabs[0].undo_cursor, 2);
    assert_eq!(migrated.active_tab, Some(0));

    // Confirm v2 content round-trips byte-identically except for the bumped
    // version: re-serialize the migrated state, force schema_version back to 2,
    // and it must equal the v2 state parsed directly.
    let v2_state: SessionState = serde_json::from_str(&v2).unwrap();
    let v2_canonical = serde_json::to_value(&v2_state).unwrap();
    let mut migrated_canonical = serde_json::to_value(&migrated).unwrap();
    migrated_canonical["schema_version"] = serde_json::json!(2);
    assert_eq!(
        migrated_canonical, v2_canonical,
        "v2 → v3 must change ONLY the version (identity migration)"
    );
}

#[test]
fn unknown_transform_kind_triggers_forward_incompat_banner_not_panic() {
    // A v3-shaped session carrying a transform `kind` this build doesn't know
    // (a newer binary wrote it) must surface the forward-incompat banner path,
    // NOT a generic Json parse error and NOT a panic.
    let v3_future = inject_unknown_kind(sample_v2_session_json());
    let err = migrate::load_str(&v3_future);
    // Assert the SPECIFIC offending kind is surfaced — this locks the guarantee
    // that the scan returns the unknown TOP-LEVEL transform kind (`frobnicate`),
    // NOT a nested `kind` (the injected transform also nests a `surrogate`
    // RowKey). A buggy recursive scan would return `surrogate` and fail here.
    assert!(
        matches!(&err, Err(migrate::SessionLoadError::ForwardIncompatTransform(k)) if k == "frobnicate"),
        "unknown transform kind must map to ForwardIncompatTransform(\"frobnicate\"), got {err:?}"
    );
    assert!(err.unwrap_err().is_forward_incompat());
}

// ---------------------------------------------------------------------------
// T4 — session v4 (projection kinds allowlist + migrate_v3_to_v4)
// ---------------------------------------------------------------------------

#[test]
fn v3_with_projection_transform_loads_as_v5() {
    // A v3 file that ALSO carries a P4c projection transform must load (the
    // kinds are now known) and stamp v5 (current).
    let json = r#"{
      "schema_version": 3,
      "tabs": [{
        "table_name": "orders",
        "source_path": null,
        "transform_stack": [
          {"kind":"rename","column":"a","to":"A"},
          {"kind":"reorder","columns":["b","a"]},
          {"kind":"delete_column","columns":["c"]}
        ],
        "undo_cursor": 3
      }],
      "active_tab": 0
    }"#;
    let state = migrate::load_str(json).unwrap();
    assert_eq!(state.schema_version, SESSION_SCHEMA_VERSION);
    assert_eq!(state.tabs[0].transform_stack.len(), 3);
}

#[test]
fn v3_redo_tail_is_dropped_on_v4_upgrade() {
    // v3 persisted full stack + undo_cursor; v4 keeps only the active slice.
    let json = r#"{
      "schema_version": 3,
      "tabs": [{
        "table_name": "orders",
        "source_path": null,
        "transform_stack": [
          {"kind":"filter","column":"a","op":"eq","value":{"kind":"scalar","value":{"type":"int","value":1}}},
          {"kind":"filter","column":"a","op":"eq","value":{"kind":"scalar","value":{"type":"int","value":2}}}
        ],
        "undo_cursor": 1
      }],
      "active_tab": 0
    }"#;
    let state = migrate::load_str(json).unwrap();
    assert_eq!(state.schema_version, SESSION_SCHEMA_VERSION);
    assert_eq!(
        state.tabs[0].transform_stack.len(),
        1,
        "v4 upgrade truncates to the active slice (undo_cursor)"
    );
    assert_eq!(state.tabs[0].undo_cursor, 1);
}

// ---------------------------------------------------------------------------
// T3 — session v5 (additive SQL console tabs `sql_tabs` + `active_sql_tab`)
// ---------------------------------------------------------------------------

#[test]
fn v4_migration_to_v5_defaults_sql_tabs() {
    let v4_json = r#"{
      "schema_version": 4,
      "tabs": [
        { "table_name": "orders", "source_path": null, "transform_stack": [], "undo_cursor": 0 }
      ],
      "active_tab": 0
    }"#;
    let state = migrate::load_str(v4_json).unwrap();
    assert_eq!(state.schema_version, SESSION_SCHEMA_VERSION);
    assert_eq!(state.tabs.len(), 1);
    assert_eq!(state.active_tab, Some(0));
    assert!(state.sql_tabs.is_empty(), "v4->v5: sql_tabs defaults empty");
    assert_eq!(
        state.active_sql_tab, None,
        "v4->v5: active_sql_tab defaults None"
    );
}

#[test]
fn v5_round_trips_sql_tabs() {
    let v5_json = r#"{
      "schema_version": 5,
      "tabs": [],
      "active_tab": null,
      "sql_tabs": [
        { "id": "018f9a00-0000-7000-8000-000000000000", "title": "Query 1", "sql": "SELECT 1" }
      ],
      "active_sql_tab": 0
    }"#;
    let state = migrate::load_str(v5_json).unwrap();
    assert_eq!(state.schema_version, 5);
    assert_eq!(state.sql_tabs.len(), 1);
    assert_eq!(state.sql_tabs[0].title, "Query 1");
    assert_eq!(state.sql_tabs[0].sql, "SELECT 1");
    assert_eq!(state.active_sql_tab, Some(0));
}
