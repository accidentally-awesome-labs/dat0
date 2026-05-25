//! All 6 supported formats render via the drop path. `.sqlite` rejects.
//!
//! P3b T4 adds a single intent-level assertion that the `WorkspaceShell`
//! mounts a real `gpui_component::table::Table` widget (replacing the P3a
//! T10 placeholder div) when a data source is present. The check is done
//! via the static helper `WorkspaceShell::child_widget_type_name` rather
//! than driving a full render loop because the GPUI render loop is hard
//! to bring up headlessly (see `docs/internal/gpui-api-notes.md` §0.A.11
//! for the manual visual-confirmation recipe — the T13 retro revisits
//! deeper visual coverage).

use dat0_app::error_ux::banner::drain_pending;
use dat0_app::file_drop::{DropOutcome, handle_drop};
use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;

const BUDGET: u64 = 128 * 1024 * 1024;

fn write_csv(dir: &Path) -> PathBuf {
    let p = dir.join("a.csv");
    std::fs::write(&p, "x,y\n1,a\n2,b\n").unwrap();
    p
}

fn write_tsv(dir: &Path) -> PathBuf {
    let p = dir.join("a.tsv");
    std::fs::write(&p, "x\ty\n1\ta\n2\tb\n").unwrap();
    p
}

fn write_jsonl(dir: &Path) -> PathBuf {
    let p = dir.join("a.jsonl");
    std::fs::write(&p, "{\"x\":1}\n{\"x\":2}\n").unwrap();
    p
}

fn write_ndjson(dir: &Path) -> PathBuf {
    let p = dir.join("a.ndjson");
    std::fs::write(&p, "{\"x\":1}\n{\"x\":2}\n").unwrap();
    p
}

fn write_json(dir: &Path) -> PathBuf {
    let p = dir.join("a.json");
    std::fs::write(&p, "[{\"x\":1},{\"x\":2}]\n").unwrap();
    p
}

/// Generate a tiny Parquet file via a side-channel `DuckDBEngine` (CTAS +
/// `COPY ... TO 'file.parquet'`). dat0-fixtures has no library API yet — see
/// PD-011 for the proper fixture-generator extraction.
async fn write_parquet(dir: &Path) -> PathBuf {
    let p = dir.join("a.parquet");
    let scratch = dir.join("gen.duckdb");
    let engine = DuckDBEngine::new(scratch, MemoryBudget { bytes: BUDGET }).unwrap();
    engine.init().await.unwrap();
    let sql = format!(
        "COPY (SELECT 1 AS x, 'a' AS y UNION ALL SELECT 2, 'b') TO '{}' (FORMAT PARQUET)",
        p.display()
    );
    engine.execute(&sql).await.expect("write parquet");
    p
}

async fn drop_and_assert_registered(path: PathBuf) {
    let _ = drain_pending();
    let tmp = TempDir::new().unwrap();
    let sess = Session::new(tmp.path(), BUDGET).await.unwrap();
    let arc = Arc::new(Mutex::new(sess));
    let out = handle_drop(vec![path], Arc::clone(&arc)).await;
    assert!(
        matches!(out[0], DropOutcome::Registered { .. }),
        "expected Registered: {:?}",
        out[0]
    );
    assert_eq!(arc.lock().tabs().len(), 1);
}

#[tokio::test]
async fn csv_drop_registers() {
    let tmp = TempDir::new().unwrap();
    drop_and_assert_registered(write_csv(tmp.path())).await;
}

#[tokio::test]
async fn tsv_drop_registers() {
    let tmp = TempDir::new().unwrap();
    drop_and_assert_registered(write_tsv(tmp.path())).await;
}

#[tokio::test]
async fn jsonl_drop_registers() {
    let tmp = TempDir::new().unwrap();
    drop_and_assert_registered(write_jsonl(tmp.path())).await;
}

#[tokio::test]
async fn ndjson_drop_registers() {
    let tmp = TempDir::new().unwrap();
    drop_and_assert_registered(write_ndjson(tmp.path())).await;
}

#[tokio::test]
async fn json_drop_registers() {
    let tmp = TempDir::new().unwrap();
    drop_and_assert_registered(write_json(tmp.path())).await;
}

#[tokio::test]
async fn parquet_drop_registers() {
    let tmp = TempDir::new().unwrap();
    let p = write_parquet(tmp.path()).await;
    drop_and_assert_registered(p).await;
}

#[tokio::test]
async fn sqlite_drop_rejected_with_banner() {
    let _ = drain_pending();
    let tmp = TempDir::new().unwrap();
    let sess = Session::new(tmp.path(), BUDGET).await.unwrap();
    let arc = Arc::new(Mutex::new(sess));
    let sqlite = tmp.path().join("db.sqlite");
    std::fs::write(&sqlite, b"sqlite-stub").unwrap();
    let out = handle_drop(vec![sqlite], Arc::clone(&arc)).await;
    assert!(matches!(out[0], DropOutcome::Unsupported { .. }));
    assert!(arc.lock().tabs().is_empty());
    let banners = drain_pending();
    assert!(!banners.is_empty(), "expected at least one Banner emission");
}

/// P3b T4 — `WorkspaceShell` mounts the real `gpui_component::table::Table`
/// widget over `GridTableDelegate` (not the P3a placeholder div). We assert
/// the static type name rather than driving a render loop because GPUI's
/// render loop is not headless-friendly in this test harness; see the
/// module-level docstring for the rationale.
#[test]
fn workspace_shell_mounts_real_table_widget() {
    let name = WorkspaceShell::child_widget_type_name();
    assert!(
        name.contains("gpui_component"),
        "expected gpui_component path in widget type name, got {name:?}"
    );
    assert!(
        name.contains("::table::Table"),
        "expected ::table::Table in widget type name, got {name:?}"
    );
    assert!(
        name.contains("GridTableDelegate"),
        "expected GridTableDelegate type parameter in widget type name, got {name:?}"
    );
}

#[tokio::test]
async fn unknown_extension_rejected_with_banner() {
    let _ = drain_pending();
    let tmp = TempDir::new().unwrap();
    let sess = Session::new(tmp.path(), BUDGET).await.unwrap();
    let arc = Arc::new(Mutex::new(sess));
    let bin = tmp.path().join("data.bin");
    std::fs::write(&bin, b"\x00\x01\x02").unwrap();
    let out = handle_drop(vec![bin], Arc::clone(&arc)).await;
    assert!(matches!(out[0], DropOutcome::Unsupported { .. }));
    assert!(arc.lock().tabs().is_empty());
}
