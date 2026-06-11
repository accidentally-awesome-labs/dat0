//! P7a T0 GATE: DuckDB close→fs-move→reopen must round-trip all data.
//! If this fails, switch promote() to ATTACH+COPY (design §T0 spike #2).
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget {
        bytes: 128 * 1024 * 1024,
    }
}

#[tokio::test]
async fn duckdb_file_survives_close_move_reopen() {
    let tmp = tempfile::TempDir::new().unwrap();
    let src = tmp.path().join("scratch.duckdb");
    let dst_dir = tmp.path().join(".dat0");
    std::fs::create_dir_all(&dst_dir).unwrap();
    let dst = dst_dir.join("workspace.duckdb");

    // 1. Create an engine, materialize a base table, close it.
    //    `close()` flips the status flag; the DuckDB connection itself is only
    //    released when the engine drops (Arc<Mutex<Connection>> reaches zero).
    //    We therefore drop the engine binding after `close()` so the OS file
    //    handle is released before we attempt `fs::rename`.
    {
        let engine = DuckDBEngine::new(src.clone(), budget()).unwrap();
        engine.init().await.unwrap();
        engine
            .execute("CREATE TABLE t AS SELECT * FROM range(1000) AS r(id)")
            .await
            .unwrap();
        engine.close().await.unwrap();
        // engine drops here — connection Arc reaches zero, file handle released.
    }

    // 2. Move the DB file (+ any WAL sibling) to the workspace location.
    let wal = src.with_extension("duckdb.wal");
    std::fs::rename(&src, &dst).unwrap();
    if wal.exists() {
        std::fs::rename(&wal, dst_dir.join("workspace.duckdb.wal")).unwrap();
    }

    // 3. Reopen at the new path; data must be intact.
    let reopened = DuckDBEngine::new(dst.clone(), budget()).unwrap();
    reopened.init().await.unwrap();
    let result = reopened
        .execute("SELECT count(*) AS n FROM t")
        .await
        .unwrap();
    assert_eq!(
        scalar_i64(&result, "n") as u64,
        1000,
        "row count must survive the move"
    );
}

// Local helper — mirrors the Arrow downcast used in engine integration tests.
// DuckDB returns COUNT(*) as Int64Array (confirmed in export_to_path.rs:137).
fn scalar_i64(result: &dat0_engine::QueryResult, col: &str) -> i64 {
    use duckdb::arrow::array::{Array, Int64Array};
    let batch = result.batches.first().expect("one batch");
    let idx = batch.schema().index_of(col).expect("column present");
    let arr = batch.column(idx);
    if let Some(a) = arr.as_any().downcast_ref::<Int64Array>() {
        a.value(0)
    } else {
        panic!("unexpected type for column {col}: {:?}", arr.data_type())
    }
}
