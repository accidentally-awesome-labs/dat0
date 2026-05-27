//! `MainThreadDispatcher`: channel-mechanics + drop-invariants tests.
//!
//! Main-thread-invocation semantics use `gpui::TestAppContext::single()`
//! (the safe test seam verified at T0 — see `docs/internal/gpui-api-notes.md`
//! §0.A.9) to supply a real `&mut gpui::App` to the synchronous test
//! drain. Production wiring through `cx.spawn` + `MainLoop::consume`
//! is exercised by `tests/single_instance.rs`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use dat0_app::main_bridge::MainThreadDispatcher;

#[test]
fn dispatch_succeeds_while_receiver_alive() {
    let (dispatcher, _main_loop) = MainThreadDispatcher::new();
    let r = dispatcher.dispatch(|_app| {});
    assert!(r.is_ok());
}

#[test]
fn drains_pending_messages_with_test_runner() {
    // Push 5 closures, each incrementing a shared counter. Drop the
    // dispatcher to close the channel cleanly, then synchronously drain
    // the queue against a real `&mut App` obtained via
    // `TestAppContext::single()`.
    let cx = gpui::TestAppContext::single();
    let (dispatcher, mut main_loop) = MainThreadDispatcher::new();
    let counter = Arc::new(AtomicUsize::new(0));
    for _ in 0..5 {
        let c = counter.clone();
        dispatcher
            .dispatch(move |_app| {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .unwrap();
    }
    drop(dispatcher);
    cx.update(|app| main_loop.drain_for_test(app));
    assert_eq!(counter.load(Ordering::SeqCst), 5);
}

#[test]
fn drop_sender_closes_receiver_loop() {
    // Last sender drop should close the channel; a drain pass after that
    // returns immediately even with no real GPUI context available.
    let cx = gpui::TestAppContext::single();
    let (dispatcher, mut main_loop) = MainThreadDispatcher::new();
    drop(dispatcher);
    cx.update(|app| main_loop.drain_for_test(app));
}

#[test]
fn dispatch_after_receiver_drop_errors() {
    let (dispatcher, main_loop) = MainThreadDispatcher::new();
    drop(main_loop);
    // Brief pause to let any cross-task drop side-effects settle (the
    // channel is sync so this is belt-and-braces, not load-bearing).
    std::thread::sleep(Duration::from_millis(10));
    let result = dispatcher.dispatch(|_| {});
    assert!(result.is_err());
}
