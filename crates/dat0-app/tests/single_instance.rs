//! Single-instance enforcement: second `dat0` launch forwards via UDS,
//! running instance grows its window count.
//!
//! P3b T1 (closes PD-010): the UDS handler now dispatches a visual-spawn
//! closure through the `MainThreadDispatcher` instead of merely logging the
//! message. The end-to-end "open a second visual window" assertion requires
//! a running GPUI event loop bound to the platform main thread, which is
//! not safe to start inside a `#[tokio::test]` harness. The
//! `second_launch_forwards_and_dispatches_to_main_thread` test below
//! exercises the same plumbing the production code uses (UDS receive →
//! dispatcher post → main-thread drain) by substituting a
//! `TestAppContext`-supplied `&mut App` for the foreground executor's
//! event-loop-driven `cx.update`. The `#[ignore]`d
//! `second_launch_spawns_visual_window` test below documents the manual
//! UAT path for verifying the visual window actually appears.

use dat0_app::app_lock::{AppLock, OpenWindowMessage};
use dat0_app::main_bridge::MainThreadDispatcher;
use dat0_app::window_registry;
use serial_test::serial;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
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

/// End-to-end PD-010-closure plumbing test (no GPUI event loop required).
///
/// Mirrors the production sequence: tokio task receives a forwarded UDS
/// message → dispatches a closure through `MainThreadDispatcher` →
/// main-thread drain invokes the closure against a real `&mut gpui::App`.
/// Asserts the closure ran exactly once, which is the same assertion the
/// `#[ignore]`d visual-window test makes about window count (one extra
/// window after the second-launch forward).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn second_launch_forwards_and_dispatches_to_main_thread() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lock_a = AppLock::try_acquire(tmp.path()).unwrap().expect("first");

    // Build the dispatcher pair in this test (production captures it in
    // `main.rs`). Install it into the process-wide slot so the UDS handler
    // can find it via `window_registry::dispatcher()`.
    let (dispatcher, mut main_loop) = MainThreadDispatcher::new();
    window_registry::install_dispatcher(dispatcher);

    let spawn_count = Arc::new(AtomicUsize::new(0));
    let spawn_count_for_handler = Arc::clone(&spawn_count);

    let serve_handle = tokio::spawn(async move {
        let _ = lock_a
            .serve(move |_msg: OpenWindowMessage| {
                // The production UDS handler dispatches a `spawn_window`
                // closure here. We dispatch a count-incrementing closure as
                // a side-channel proxy: the production assertion is
                // "WindowRegistry::len() == 2 after drain"; this assertion
                // is "the closure ran once after drain".
                let Some(d) = window_registry::dispatcher() else {
                    return;
                };
                let c = Arc::clone(&spawn_count_for_handler);
                let _ = d.dispatch(move |_app| {
                    c.fetch_add(1, Ordering::SeqCst);
                });
            })
            .await;
    });

    // Allow listener bind.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    AppLock::forward_open_window(tmp.path(), OpenWindowMessage { paths: vec![] }).unwrap();

    // Allow the UDS handler to receive + dispatch.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    // Drive the main-thread drain against a real `&mut gpui::App` supplied
    // by `TestAppContext::single()` — the safe seam established at T0
    // (see `docs/internal/gpui-api-notes.md` §0.A.9).
    let cx = gpui::TestAppContext::single();
    cx.update(|app| main_loop.drain_for_test(app));

    assert_eq!(
        spawn_count.load(Ordering::SeqCst),
        1,
        "UDS-forwarded message must dispatch exactly one main-thread closure"
    );

    serve_handle.abort();
}

/// Manual UAT only: a true headless event-loop test would need
/// `Application::run` on the main thread plus an X11/Wayland/Cocoa
/// connection, neither of which is reachable from `cargo test`. Run the
/// app twice with `cargo run -p dat0-app` from two separate shells; the
/// second invocation must spawn a visible window in the first instance,
/// then exit. Window-count is observable via `WindowRegistry::len()` at
/// shutdown logs.
#[test]
#[ignore = "requires a running GPUI event loop on the platform main thread; see manual UAT in test docstring"]
fn second_launch_spawns_visual_window() {
    unimplemented!(
        "manual UAT: launch the app twice from separate shells; the second invocation \
         must visibly open a second window in the running instance"
    );
}
