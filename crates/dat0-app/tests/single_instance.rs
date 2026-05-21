//! Single-instance enforcement: second `dat0` launch forwards via UDS,
//! running instance grows its window count.

use dat0_app::app_lock::{AppLock, OpenWindowMessage};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_launch_forwards_and_exits() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lock_a = AppLock::try_acquire(tmp.path()).unwrap().expect("first");
    let received = Arc::new(AtomicUsize::new(0));
    let received_clone = Arc::clone(&received);
    let handle = tokio::spawn(async move {
        let _ = lock_a
            .serve(move |_msg| {
                received_clone.fetch_add(1, Ordering::SeqCst);
            })
            .await;
    });

    // Let the listener bind.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Simulate a second launch by attempting to acquire — must fail.
    let second = AppLock::try_acquire(tmp.path()).unwrap();
    assert!(second.is_none(), "second acquire must report contention");

    // Forward the message that the second launcher would have sent.
    AppLock::forward_open_window(tmp.path(), OpenWindowMessage { paths: vec![] }).unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(received.load(Ordering::SeqCst), 1);

    handle.abort();
}
