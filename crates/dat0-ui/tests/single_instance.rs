//! Single instance: a second `dat0` launch hands its paths to the running one
//! over the UDS and exits, rather than becoming a second process.
//!
//! Three layers, because the failure modes are different:
//!
//! 1. the lock reports contention and the socket carries the message;
//! 2. the running instance turns that message into the `AppEvent::OpenWindow`
//!    its root component drains — this is the seam that replaces the GPUI
//!    build's `MainThreadDispatcher`, whose whole job was hopping a closure
//!    onto the UI thread. Dioxus has a channel instead, and the assertion is
//!    the same one: exactly one open-window request arrives, carrying the
//!    forwarded paths;
//! 3. the REAL binary, launched against a state dir this test already owns,
//!    forwards and exits 0 without opening a window.
//!
//! Layer 3 mutates `DAT0_CONFIG_DIR` / `XDG_DATA_HOME` in this process so it
//! can resolve the same state dir the child will (`platform::data_dir()` is
//! asymmetric across platforms — the seam reaches it on macOS and not on
//! Linux — and guessing would silently mis-target). That makes it `#[serial]`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dat0_core::app_lock::{AppLock, OpenWindowMessage};
use dat0_core::events::{AppEvent, AppEvents};
use futures::StreamExt;
use serial_test::serial;

/// How long a listener gets to bind, and a forwarded message to arrive.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(150);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn a_second_launch_finds_the_lock_taken_and_its_message_lands() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lock = AppLock::try_acquire(tmp.path()).unwrap().expect("first");
    let received = Arc::new(AtomicUsize::new(0));

    let handle = {
        let received = Arc::clone(&received);
        tokio::spawn(async move {
            let _ = lock
                .serve(move |_msg| {
                    received.fetch_add(1, Ordering::SeqCst);
                })
                .await;
        })
    };
    tokio::time::sleep(SETTLE).await;

    // A second launch attempting to acquire must report contention.
    let second = AppLock::try_acquire(tmp.path()).unwrap();
    assert!(second.is_none(), "second acquire must report contention");

    // ...and the message it forwards instead must arrive.
    AppLock::forward_open_window(tmp.path(), OpenWindowMessage { paths: vec![] }).unwrap();
    tokio::time::sleep(SETTLE).await;
    assert_eq!(received.load(Ordering::SeqCst), 1);

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn a_forwarded_launch_becomes_exactly_one_open_window_event() {
    let tmp = tempfile::TempDir::new().unwrap();
    let lock = AppLock::try_acquire(tmp.path()).unwrap().expect("first");

    // The production handler, verbatim: `launch::run_app` gives `serve` a
    // closure that does nothing but post the paths onto the event bus.
    let (events, mut rx) = AppEvents::channel();
    let handle = tokio::spawn(async move {
        let _ = lock
            .serve(move |msg: OpenWindowMessage| {
                events.send(AppEvent::OpenWindow { paths: msg.paths });
            })
            .await;
    });
    tokio::time::sleep(SETTLE).await;

    let wanted = std::path::PathBuf::from("/tmp/forwarded.csv");
    AppLock::forward_open_window(
        tmp.path(),
        OpenWindowMessage {
            paths: vec![wanted.clone()],
        },
    )
    .unwrap();

    let ev = tokio::time::timeout(std::time::Duration::from_secs(5), rx.next())
        .await
        .expect("an open-window event must arrive")
        .expect("the bus must still be open");
    match ev {
        AppEvent::OpenWindow { paths } => assert_eq!(
            paths,
            vec![wanted],
            "the forwarded paths must survive the round trip"
        ),
        other => panic!("expected OpenWindow, got {other:?}"),
    }

    // Exactly one: a single forward must not fan out into two windows.
    tokio::time::sleep(SETTLE).await;
    assert!(
        rx.try_recv().is_err(),
        "one forwarded launch must post exactly one open-window event"
    );

    handle.abort();
}

/// The real binary, against a state dir this test already holds the lock on.
///
/// It must forward and return, not open a window: on a headless CI box a
/// second window is not merely wrong, it hangs. The child is therefore given a
/// deadline and killed if it misses it, so a regression fails rather than
/// blocking the suite.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn the_real_binary_forwards_to_a_running_instance_and_exits_zero() {
    let scratch = tempfile::TempDir::new().unwrap();
    let previous = std::env::var_os("DAT0_CONFIG_DIR");
    let previous_xdg = std::env::var_os("XDG_DATA_HOME");
    // SAFETY: this test is `#[serial]`, so no sibling in this binary races the
    // writes. Resolving the child's state dir any other way would mean
    // duplicating `platform::data_dir`'s per-OS asymmetry here.
    unsafe {
        std::env::set_var("DAT0_CONFIG_DIR", scratch.path());
        std::env::set_var("XDG_DATA_HOME", scratch.path());
    }
    let state_dir = dat0_core::platform::data_dir().expect("data dir");

    let lock = AppLock::try_acquire(&state_dir)
        .unwrap()
        .expect("this test owns the lock");
    let received = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let serve = {
        let received = Arc::clone(&received);
        tokio::spawn(async move {
            let _ = lock
                .serve(move |msg: OpenWindowMessage| {
                    received.lock().push(msg.paths);
                })
                .await;
        })
    };
    tokio::time::sleep(SETTLE).await;

    let csv = scratch.path().join("second-launch.csv");
    std::fs::write(&csv, "a\n1\n").unwrap();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_dat0"))
        .arg(&csv)
        .env("DAT0_CONFIG_DIR", scratch.path())
        .env("XDG_DATA_HOME", scratch.path())
        .spawn()
        .expect("spawn a second dat0");

    let status = tokio::task::spawn_blocking(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            if let Some(status) = child.try_wait().expect("poll child") {
                return Some(status);
            }
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    })
    .await
    .unwrap();

    unsafe {
        match previous {
            Some(p) => std::env::set_var("DAT0_CONFIG_DIR", p),
            None => std::env::remove_var("DAT0_CONFIG_DIR"),
        }
        match previous_xdg {
            Some(p) => std::env::set_var("XDG_DATA_HOME", p),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
    }

    let status = status.expect("a contended launch must exit, not open a window");
    assert!(
        status.success(),
        "a contended launch must exit 0, got {status:?}"
    );

    let forwarded = received.lock().clone();
    assert_eq!(
        forwarded,
        vec![vec![csv]],
        "the running instance must receive exactly the second launch's paths"
    );

    serve.abort();
}
