//! Every supported format survives the drop path; `.sqlite` and unknown
//! extensions are refused with a banner.
//!
//! # What moved
//!
//! The drop path is now two halves in two crates, and this suite is the only
//! place they are checked together:
//!
//! 1. [`dat0_ui::files::dropped_paths`] turns a webview drag event into real
//!    filesystem paths. This is what makes the whole design work — the HTML5
//!    payload is a `File`, and dat0 needs a *path* so DuckDB can register a
//!    table without copying gigabytes. It is also why
//!    `Config::with_disable_drag_drop_handler` stays at its default.
//! 2. [`dat0_core::file_drop::handle_drop`] sniffs and registers, unchanged
//!    from the GPUI build.
//!
//! The GPUI suite's last test asserted that the shell mounted a real
//! `gpui_component::table::Table` rather than the placeholder div that
//! preceded it — a claim about a widget type, checked through a static type
//! name because the GPUI render loop was not headless-friendly. That
//! guarantee has no counterpart: there is no widget library, the grid is
//! dat0's own component, and `tests/grid_nav.rs` and
//! `tests/grid_virtualization.rs` assert it renders real cells from a real
//! `GridDataSource`. In its place this suite gains the seam that genuinely had
//! no coverage: the event → path → table round trip.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dat0_core::error_ux::banner::drain_pending;
use dat0_core::file_drop::{DropOutcome, handle_drop};
use dat0_core::session::Session;
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};
use parking_lot::Mutex;
use serial_test::serial;
use tempfile::TempDir;

const BUDGET: u64 = 128 * 1024 * 1024;

fn write_csv(dir: &Path) -> PathBuf {
    let p = dir.join("a.csv");
    std::fs::write(&p, "x,y\n1,a\n2,b\n").expect("write csv");
    p
}

fn write_tsv(dir: &Path) -> PathBuf {
    let p = dir.join("a.tsv");
    std::fs::write(&p, "x\ty\n1\ta\n2\tb\n").expect("write tsv");
    p
}

fn write_jsonl(dir: &Path) -> PathBuf {
    let p = dir.join("a.jsonl");
    std::fs::write(&p, "{\"x\":1}\n{\"x\":2}\n").expect("write jsonl");
    p
}

fn write_ndjson(dir: &Path) -> PathBuf {
    let p = dir.join("a.ndjson");
    std::fs::write(&p, "{\"x\":1}\n{\"x\":2}\n").expect("write ndjson");
    p
}

fn write_json(dir: &Path) -> PathBuf {
    let p = dir.join("a.json");
    std::fs::write(&p, "[{\"x\":1},{\"x\":2}]\n").expect("write json");
    p
}

/// Generate a tiny Parquet file via a side-channel `DuckDBEngine` (CTAS +
/// `COPY … TO 'file.parquet'`). `dat0-fixtures` has no library API for a
/// single small file — see PD-011.
async fn write_parquet(dir: &Path) -> PathBuf {
    let p = dir.join("a.parquet");
    let scratch = dir.join("gen.duckdb");
    let engine = DuckDBEngine::new(scratch, MemoryBudget { bytes: BUDGET }).expect("engine");
    engine.init().await.expect("init");
    let sql = format!(
        "COPY (SELECT 1 AS x, 'a' AS y UNION ALL SELECT 2, 'b') TO '{}' (FORMAT PARQUET)",
        p.display()
    );
    engine.execute(&sql).await.expect("write parquet");
    p
}

async fn session(dir: &Path) -> Arc<Mutex<Session>> {
    let sess = Session::new(dir, BUDGET).await.expect("Session::new");
    Arc::new(Mutex::new(sess))
}

async fn drop_and_assert_registered(path: PathBuf) {
    let _ = drain_pending();
    let tmp = TempDir::new().expect("tempdir");
    let arc = session(tmp.path()).await;
    let out = handle_drop(vec![path], Arc::clone(&arc)).await;
    assert!(
        matches!(out[0], DropOutcome::Registered { .. }),
        "expected Registered: {:?}",
        out[0]
    );
    assert_eq!(arc.lock().tabs().len(), 1);
}

#[tokio::test]
async fn a_dropped_csv_registers() {
    let tmp = TempDir::new().expect("tempdir");
    drop_and_assert_registered(write_csv(tmp.path())).await;
}

#[tokio::test]
async fn a_dropped_tsv_registers() {
    let tmp = TempDir::new().expect("tempdir");
    drop_and_assert_registered(write_tsv(tmp.path())).await;
}

#[tokio::test]
async fn a_dropped_jsonl_registers() {
    let tmp = TempDir::new().expect("tempdir");
    drop_and_assert_registered(write_jsonl(tmp.path())).await;
}

#[tokio::test]
async fn a_dropped_ndjson_registers() {
    let tmp = TempDir::new().expect("tempdir");
    drop_and_assert_registered(write_ndjson(tmp.path())).await;
}

#[tokio::test]
async fn a_dropped_json_registers() {
    let tmp = TempDir::new().expect("tempdir");
    drop_and_assert_registered(write_json(tmp.path())).await;
}

#[tokio::test]
async fn a_dropped_parquet_registers() {
    let tmp = TempDir::new().expect("tempdir");
    let p = write_parquet(tmp.path()).await;
    drop_and_assert_registered(p).await;
}

/// `#[serial]` because `drain_pending` is a process-global queue and a
/// concurrent test that raises a banner would be indistinguishable from this
/// one raising none.
#[tokio::test]
#[serial]
async fn a_dropped_sqlite_file_is_refused_with_a_banner() {
    let _ = drain_pending();
    let tmp = TempDir::new().expect("tempdir");
    let arc = session(tmp.path()).await;
    let sqlite = tmp.path().join("db.sqlite");
    std::fs::write(&sqlite, b"sqlite-stub").expect("write stub");

    let out = handle_drop(vec![sqlite], Arc::clone(&arc)).await;

    assert!(matches!(out[0], DropOutcome::Unsupported { .. }));
    assert!(arc.lock().tabs().is_empty());
    assert!(
        !drain_pending().is_empty(),
        "refusing a file silently is worse than refusing it: expected a Banner"
    );
}

#[tokio::test]
#[serial]
async fn an_unknown_extension_is_refused() {
    let _ = drain_pending();
    let tmp = TempDir::new().expect("tempdir");
    let arc = session(tmp.path()).await;
    let bin = tmp.path().join("data.bin");
    std::fs::write(&bin, b"\x00\x01\x02").expect("write bin");

    let out = handle_drop(vec![bin], Arc::clone(&arc)).await;

    assert!(matches!(out[0], DropOutcome::Unsupported { .. }));
    assert!(arc.lock().tabs().is_empty());
}

// ───────────────────────────────────────────────────────────────────────────
// The event half: `dat0_ui::files::dropped_paths`
// ───────────────────────────────────────────────────────────────────────────

/// A drag event carrying `paths`, in the shape `dioxus-desktop` delivers.
///
/// `SerializedDragData` is the same type the desktop renderer deserializes a
/// real drop into, so this is the event's actual shape rather than a
/// hand-rolled stand-in — and `SerializedFileData::path` is exactly the field
/// the native handler fills with the OS path.
fn drag_event(paths: &[PathBuf]) -> dioxus::events::DragData {
    use dioxus::html::geometry::{ClientPoint, Coordinates, ElementPoint, PagePoint, ScreenPoint};
    use dioxus::html::input_data::MouseButton;
    use dioxus::html::point_interaction::SerializedPointInteraction;
    use dioxus::html::{SerializedDataTransfer, SerializedDragData, SerializedFileData};

    let at = Coordinates::new(
        ScreenPoint::new(0.0, 0.0),
        ClientPoint::new(0.0, 0.0),
        ElementPoint::new(0.0, 0.0),
        PagePoint::new(0.0, 0.0),
    );
    let files = paths
        .iter()
        .map(|p| SerializedFileData {
            path: p.clone(),
            size: std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
            last_modified: 0,
            content_type: None,
            contents: None,
        })
        .collect();

    dioxus::events::DragData::new(SerializedDragData {
        mouse: SerializedPointInteraction::new(
            Some(MouseButton::Primary),
            MouseButton::Primary.into(),
            at,
            dioxus::prelude::Modifiers::empty(),
        ),
        data_transfer: SerializedDataTransfer {
            items: Vec::new(),
            files,
            effect_allowed: "all".to_string(),
            drop_effect: "copy".to_string(),
        },
    })
}

#[test]
fn a_drop_event_yields_the_real_filesystem_paths() {
    let tmp = TempDir::new().expect("tempdir");
    let csv = write_csv(tmp.path());
    let tsv = write_tsv(tmp.path());

    let got = dat0_ui::files::dropped_paths(&drag_event(&[csv.clone(), tsv.clone()]));

    // Paths, in gesture order — not `File` handles, not copies in a temp dir.
    // Registering a 12 GB Parquet file without reading it depends on this.
    assert_eq!(got, vec![csv, tsv]);
}

#[test]
fn a_drop_entry_with_no_file_behind_it_is_discarded() {
    let tmp = TempDir::new().expect("tempdir");
    let real = write_csv(tmp.path());
    let ghost = tmp.path().join("dragged-from-another-app");

    let got = dat0_ui::files::dropped_paths(&drag_event(&[ghost, real.clone()]));

    // A webview drop can carry an entry with no filesystem path — a selection
    // dragged out of another application, say. Passing it on would fail deep
    // in the engine with a message about a table name; dropping it here means
    // the user simply sees nothing happen for that item.
    assert_eq!(got, vec![real]);
}

#[test]
fn an_empty_drop_yields_nothing_rather_than_a_bare_path() {
    assert!(dat0_ui::files::dropped_paths(&drag_event(&[])).is_empty());
}

/// The whole path, end to end: a real drag event over a real file becomes a
/// registered table.
///
/// Neither half proves this alone — `dropped_paths` could return a plausible
/// path the engine cannot open, and `handle_drop` could be perfect while the
/// event never yields a path at all. The shell's `ondrop` is literally these
/// two calls, so this is the shipped flow minus the window.
#[tokio::test]
async fn a_dropped_csv_travels_from_the_event_to_a_registered_table() {
    let tmp = TempDir::new().expect("tempdir");
    let csv = write_csv(tmp.path());

    let paths = dat0_ui::files::dropped_paths(&drag_event(&[csv]));
    assert_eq!(paths.len(), 1, "the event must carry the file");

    let state = TempDir::new().expect("tempdir");
    let arc = session(state.path()).await;
    let out = handle_drop(paths, Arc::clone(&arc)).await;

    match &out[0] {
        DropOutcome::Registered { table_name, .. } => assert_eq!(table_name, "a"),
        other => panic!("expected Registered, got {other:?}"),
    }
    assert_eq!(arc.lock().tabs().len(), 1);
}
