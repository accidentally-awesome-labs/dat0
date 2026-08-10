//! Streaming execution via `spawn_blocking` worker pushing batches to a
//! bounded `tokio::sync::mpsc` channel. Per spec §2.1.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use duckdb::arrow::record_batch::RecordBatch;
use futures::Stream;
use tokio::sync::mpsc;

use crate::Result;
use crate::error::EngineError;

/// Spawn a blocking worker that pulls batches from DuckDB and pushes them
/// onto a bounded channel; return a stream that polls the channel.
pub(crate) fn spawn_streaming(
    conn: Arc<Mutex<duckdb::Connection>>,
    sql: String,
) -> Result<crate::types::ArrowRecordBatchStream> {
    // capacity 1: producer waits when consumer hasn't pulled the previous batch.
    let (tx, rx) = mpsc::channel::<Result<RecordBatch>>(1);

    // Worker: holds the connection mutex while iterating. Other engine
    // operations (including execute()) will queue behind it. This is by design;
    // DuckDB connections are single-threaded for execution anyway.
    tokio::task::spawn_blocking(move || {
        let conn = match conn.lock() {
            Ok(g) => g,
            Err(_) => {
                let _ = tx.blocking_send(Err(EngineError::EnginePoisoned));
                return;
            }
        };
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.blocking_send(Err(crate::execute::translate_duckdb_err(e)));
                return;
            }
        };
        let arrow_iter = match stmt.query_arrow([]) {
            Ok(it) => it,
            Err(e) => {
                let _ = tx.blocking_send(Err(crate::execute::translate_duckdb_err(e)));
                return;
            }
        };
        // D-030: `arrow_iter` yields a bare `RecordBatch`, so a mid-stream
        // DuckDB error terminates this loop exactly as EOF does and the
        // consumer sees a short-but-successful stream. Unfixable at duckdb-rs
        // 1.4.4 (`arrow_batch.rs:27-33`) and undetectable here: unlike
        // `execute::paged::run_paged` this path has no row count to reconcile
        // against. Prepare/bind errors above DO reach the consumer.
        for batch in arrow_iter {
            // blocking_send blocks until consumer pulls; channel cap=1.
            // If consumer dropped, send fails — exit cleanly.
            if tx.blocking_send(Ok(batch)).is_err() {
                tracing::debug!("streaming consumer dropped; producer shutting down");
                return;
            }
        }
    });

    Ok(Box::pin(ChannelStream { rx }))
}

struct ChannelStream {
    rx: mpsc::Receiver<Result<RecordBatch>>,
}

impl Stream for ChannelStream {
    type Item = Result<RecordBatch>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}
