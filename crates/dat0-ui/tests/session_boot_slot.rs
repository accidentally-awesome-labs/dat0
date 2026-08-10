//! A window that exists before its session does.
//!
//! `Session::new` opens DuckDB, runs its `PRAGMA`s and applies the migrations.
//! Under GPUI that happened under `block_on` on the UI thread, so the window
//! could not appear until it finished — and `boot.rs` carried a SAFETY note
//! admitting the call would become a nested-runtime abort the day gpui
//! dispatched an action from inside a tokio task. EN4 opened the window first,
//! in `SessionSlot::Booting`, and that made three things assertable that had
//! previously been unreachable:
//!
//! 1. the hero renders, with a skeleton where the drop copy goes, and the shell
//!    reports `Booting` rather than faking `Ready`;
//! 2. a file dropped during that window is QUEUED, not swallowed, and the queue
//!    drains **in gesture order** when the session lands;
//! 3. a failed `Session::new` is a terminal, visible state with a retry — never
//!    an automatic loop.
//!
//! # What moved
//!
//! `SessionSlot` is `dat0_core::session::slot` now, and the boot is
//! `dat0_ui::session_boot`: a `use_future` that awaits the session and writes a
//! signal, in place of the `MainThreadDispatcher` round trip. There is no
//! `adopt_session` to call — the test waits for the real thing, which is
//! strictly more than the GPUI suite checked, because it also proves the boot
//! task is actually driven.
//!
//! Two pieces of the EN4 contract had no implementation in the Dioxus build
//! when this suite was ported, and were added with it: `Workspace::pending_open`
//! (drops made while booting were reaching `open_paths`, which logged and
//! returned) and the `SessionSlot::Failed` banner (`session.retry` was routed
//! in `router.rs` and reachable from nothing).
//!
//! Hermeticity: one leaked temp dir per process for `DAT0_CONFIG_DIR` and the
//! state root — both are write-once process globals — and every test is
//! `#[serial]`, because the banner queue is global too. Each window gets its
//! own `scratch/<uuid>` under that root, so the tests do not share a database.

mod support;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::error_ux::banner::drain_pending;
use dat0_core::session::slot::SessionSlot;
use dat0_ui::components::empty_state::EmptyState;
use dat0_ui::session_boot::{self, OpenRoute};
use dat0_ui::state::Workspace;
use support::Harness;

/// Install the process-global paths once, into a directory that outlives the
/// binary.
///
/// `globals::install_state_root` is an `OnceLock` and `DAT0_CONFIG_DIR` is an
/// environment variable, so there is exactly one of each per process however
/// many tests run. Leaked rather than dropped: a session reading a deleted
/// state root mid-test would fail for the wrong reason.
static STATE_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("state");
    std::fs::create_dir_all(&root).expect("mkdir state");
    // SAFETY: every test in this binary is `#[serial]`, so no other thread
    // races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", tmp.path().join("cfg")) };
    dat0_core::globals::install_state_root(root.clone());
    std::mem::forget(tmp);
    root
});

fn state_root() -> &'static Path {
    STATE_ROOT.as_path()
}

/// A two-column CSV. One column would leave the sniffer with no delimiter to
/// agree on and `handle_drop` would route to the import wizard instead of
/// registering.
fn write_csv(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("mkdir");
    let p = dir.join(name);
    std::fs::write(&p, "a,b\n1,2\n3,4\n").expect("write csv");
    p
}

/// A scratch directory the session cannot create, because a FILE already sits
/// where it needs a directory.
///
/// Real failure injection rather than a hand-built `Failed` slot: it exercises
/// `Session::new_with_id`'s own error path and the `{e:#}` chain the banner
/// renders, which a synthesised slot would skip.
fn poison_scratch_for(window_id: uuid::Uuid) {
    let scratch = state_root().join("scratch");
    std::fs::create_dir_all(&scratch).expect("mkdir scratch");
    std::fs::write(scratch.join(window_id.to_string()), b"not a directory").expect("poison");
}

// ── The host ───────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    /// Chosen by the test so a failure can be injected at the exact scratch
    /// path this window will use.
    window_id: uuid::Uuid,
    /// Launch arguments, as `launch::main` supplies them.
    #[props(default)]
    cli_paths: Vec<PathBuf>,
    /// One entry per drop gesture the test can perform, in order. Each renders
    /// a button at `data-a11y-id="drop-{i}"`.
    #[props(default)]
    gestures: Vec<Vec<PathBuf>>,
}

/// The smallest window that still has a session.
///
/// It wires exactly what `Shell` wires — `use_session`, and the hero with
/// `booting` read from the slot — plus a readback node, because the harness
/// has no way to look inside a signal.
#[component]
fn Host(props: HostProps) -> Element {
    let ws = Workspace {
        window_id: props.window_id,
        ..Workspace::provide()
    };
    session_boot::use_session(ws, props.cli_paths.clone());

    let slot = ws.session.read();
    let phase = match &**slot {
        SessionSlot::Booting => "booting".to_string(),
        SessionSlot::Ready(_) => "ready".to_string(),
        SessionSlot::Failed(msg) => format!("failed:{msg}"),
    };
    let booting = slot.is_booting();
    drop(slot);

    let queue: Vec<String> = ws
        .pending_open
        .read()
        .iter()
        .map(|p| {
            p.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let tabs: Vec<String> = ws.tabs.read().iter().map(|t| t.table.clone()).collect();
    let active = (*ws.active.read())
        .map(|i| i.to_string())
        .unwrap_or_else(|| "-".into());
    let engine_ok = ws.status.read().engine_ok;

    rsx! {
        div { "data-a11y-id": "host",
            div {
                "data-a11y-id": "readback",
                "phase={phase} queue=[{queue.join(\",\")}] tabs=[{tabs.join(\",\")}] active={active} engine_ok={engine_ok}"
            }
            for (i, paths) in props.gestures.iter().cloned().enumerate() {
                button {
                    key: "{i}",
                    "data-a11y-id": "drop-{i}",
                    onclick: move |_| {
                        let paths = paths.clone();
                        spawn(async move { session_boot::open_paths(ws, paths).await });
                    },
                    "drop {i}"
                }
            }
            if ws.tabs.read().is_empty() {
                EmptyState {
                    recents: Vec::new(),
                    first_run_done: true,
                    booting,
                    on_open_sample: move |_| {},
                    on_open_recent: move |_| {},
                    on_open_file: move |_| {},
                    on_take_tour: move |_| {},
                    on_open_demo: move |_| {},
                }
            }
        }
    }
}

/// A tokio runtime entered for the whole test.
///
/// Load-bearing: the boot future and `handle_drop` both reach `spawn_blocking`
/// for the DuckDB work, and the harness polls those futures on THIS thread —
/// without an entered runtime they panic with "there is no reactor running".
struct Rt(tokio::runtime::Runtime);

fn runtime() -> Rt {
    Rt(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime"))
}

fn readback(h: &Harness) -> String {
    let key = h
        .by_a11y_id("readback")
        .expect("the host renders a readback");
    h.text_of(key)
}

/// Settle, then sleep, until `done` or two minutes.
///
/// The boot crosses into tokio, so a wake can land after the VirtualDom's queue
/// has already gone quiet; `settle` alone would return before the session
/// exists. `render_immediate` drains newly-woken tasks, so alternating the two
/// is what drives an async component to completion.
fn pump(h: &mut Harness, done: impl Fn(&Harness) -> bool) -> bool {
    // Two minutes, and deliberately nowhere near the expected time (~1s
    // unloaded). These are the only tests in the workspace that boot a real
    // DuckDB session inside a component harness, so under a full parallel
    // `cargo nextest run --workspace` they are competing with ~1700 other
    // tests for the same cores; 10s and then 30s were both measured failing
    // there while passing alone.
    //
    // A deadlock guard belongs far above the worst plausible real time, not
    // near it. Sitting close to the measured worst case is how a suite gets a
    // test everyone knows is "just flaky", which is how a suite stops being
    // read at all.
    for _ in 0..4800 {
        h.settle();
        if done(h) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn await_phase(h: &mut Harness, phase: &str) {
    let needle = format!("phase={phase}");
    assert!(
        pump(h, |h| readback(h).contains(&needle)),
        "the session never reached {phase}; last readback: {}",
        readback(h)
    );
}

// ── 1. The decision, without a window ──────────────────────────────────────

#[test]
fn a_drop_is_queued_while_booting_and_discarded_after_a_failure() {
    assert_eq!(
        session_boot::route(&SessionSlot::Booting),
        OpenRoute::Queue,
        "a gesture the user already made must survive a slow DuckDB"
    );
    assert_eq!(
        session_boot::route(&SessionSlot::Failed("disk full".into())),
        OpenRoute::Discard,
        "holding a gesture for a session that will never exist is not queueing, \
         it is a leak with a delayed surprise"
    );
    // `Ready` is covered end to end by the drain tests below; asserting the
    // discriminant here keeps the match exhaustive if a fourth state appears.
    let ready = SessionSlot::Booting;
    assert_ne!(session_boot::route(&ready), OpenRoute::Register);
}

#[test]
fn the_failure_banner_is_the_only_exit_from_a_failed_session() {
    let banner = session_boot::failure_banner("could not create scratch dir: disk quota exceeded");

    assert_eq!(banner.title, dat0_i18n::t("session.failed"));
    assert!(
        banner.body.contains("disk quota exceeded"),
        "the anyhow chain is the only diagnosis available; it must be shown: {}",
        banner.body
    );
    let primary = banner.primary.as_ref().expect("a retry action");
    assert_eq!(
        primary.action_id,
        dat0_core::actions::builtin::ids::SESSION_RETRY,
        "routed by id, so the banner, the palette and the menu reach one handler"
    );
    assert_eq!(primary.label, dat0_i18n::t("session.retry"));
    assert!(
        !banner.dismissible,
        "dismissing it would strand the window with no engine and no way to ask \
         for one — the boot deliberately never retries on its own"
    );
}

// ── 2. The booting surface ─────────────────────────────────────────────────

#[test]
#[serial]
fn a_booting_window_renders_the_hero_with_a_skeleton_drop_zone() {
    let _rt = runtime();
    let _guard = _rt.0.enter();
    state_root();

    let h = Harness::new(
        Host,
        HostProps {
            window_id: uuid::Uuid::now_v7(),
            cli_paths: Vec::new(),
            gestures: Vec::new(),
        },
    );

    // Before anything is awaited the slot is Booting and the hero is up: the
    // window is not blank while it waits, because the hero IS the sessionless
    // surface — there is no data source without a session.
    assert!(readback(&h).contains("phase=booting"), "{}", readback(&h));
    assert!(
        h.by_a11y_id("empty-state").is_some(),
        "the hero must render"
    );
    assert!(
        h.has_label(&dat0_i18n::t("session.booting")),
        "the drop copy is replaced by the skeleton's caption, so the hero stops \
         promising the 'no waiting' it cannot deliver yet"
    );
    assert!(
        !h.text().contains(&dat0_i18n::t("hero.drop")),
        "the real drop copy belongs to Ready"
    );
}

#[test]
#[serial]
fn the_session_landing_clears_the_skeleton_and_binds_the_engine() {
    let _rt = runtime();
    let _guard = _rt.0.enter();
    state_root();

    let mut h = Harness::new(
        Host,
        HostProps {
            window_id: uuid::Uuid::now_v7(),
            cli_paths: Vec::new(),
            gestures: Vec::new(),
        },
    );
    await_phase(&mut h, "ready");

    assert!(
        readback(&h).contains("engine_ok=true"),
        "the status bar's engine dot follows the slot: {}",
        readback(&h)
    );
    assert!(
        !h.has_label(&dat0_i18n::t("session.booting")),
        "the skeleton must not survive the flip to Ready"
    );
    assert!(
        h.text().contains(&dat0_i18n::t("hero.drop")),
        "the real drop copy is back"
    );
}

// ── 3. The queue — the edge this suite exists for ──────────────────────────

/// THE drop-while-booting test.
///
/// Two files are dropped as two separate gestures while DuckDB is still
/// opening. Both must be queued (not swallowed), in the order they were
/// dropped, and both must be registered when the session lands — with the LAST
/// one active, which is what `open_paths` does for a multi-file drop and
/// therefore the observable that proves the drain ran front-to-back rather
/// than reversed.
#[test]
#[serial]
fn drops_while_booting_queue_in_order_and_drain_when_the_session_lands() {
    let _rt = runtime();
    let _guard = _rt.0.enter();
    let root = state_root();

    let data = root.join("drops-queue");
    let first = write_csv(&data, "alpha.csv");
    let second = write_csv(&data, "omega.csv");

    let mut h = Harness::new(
        Host,
        HostProps {
            window_id: uuid::Uuid::now_v7(),
            cli_paths: Vec::new(),
            gestures: vec![vec![first], vec![second]],
        },
    );
    assert!(readback(&h).contains("phase=booting"), "{}", readback(&h));

    h.click("drop-0");
    h.click("drop-1");

    let seen = readback(&h);
    assert!(
        seen.contains("queue=[alpha,omega]"),
        "a second drop during boot must APPEND, not replace: {seen}"
    );
    assert!(
        seen.contains("tabs=[]"),
        "nothing can be bound before the session exists: {seen}"
    );

    await_phase(&mut h, "ready");
    assert!(
        pump(&mut h, |h| readback(h).contains("tabs=[alpha,omega]")),
        "the queue must drain front to back; last readback: {}",
        readback(&h)
    );

    let seen = readback(&h);
    assert!(
        seen.contains("queue=[]"),
        "the drain must empty the queue: {seen}"
    );
    assert!(
        seen.contains("active=1"),
        "the LAST-dropped file is the active one; a reversed drain would leave \
         alpha active: {seen}"
    );
}

#[test]
#[serial]
fn a_drop_after_the_session_lands_takes_the_direct_path() {
    let _rt = runtime();
    let _guard = _rt.0.enter();
    let root = state_root();

    let later = write_csv(&root.join("drops-direct"), "later.csv");
    let mut h = Harness::new(
        Host,
        HostProps {
            window_id: uuid::Uuid::now_v7(),
            cli_paths: Vec::new(),
            gestures: vec![vec![later]],
        },
    );
    await_phase(&mut h, "ready");

    h.click("drop-0");
    assert!(
        pump(&mut h, |h| readback(h).contains("tabs=[later]")),
        "a Ready window registers straight away; last readback: {}",
        readback(&h)
    );
    assert!(
        readback(&h).contains("queue=[]"),
        "a Ready window must never queue: {}",
        readback(&h)
    );
}

/// The launch arguments take the same path as a drop.
///
/// They used to be a separate channel, which is how the GPUI build could open
/// the CLI files and lose a drop made while it did. One queue, one drain.
#[test]
#[serial]
fn the_launch_arguments_open_through_the_same_queue() {
    let _rt = runtime();
    let _guard = _rt.0.enter();
    let root = state_root();

    let cli = write_csv(&root.join("drops-cli"), "argv.csv");
    let mut h = Harness::new(
        Host,
        HostProps {
            window_id: uuid::Uuid::now_v7(),
            cli_paths: vec![cli],
            gestures: Vec::new(),
        },
    );
    await_phase(&mut h, "ready");

    assert!(
        pump(&mut h, |h| readback(h).contains("tabs=[argv]")),
        "a file named on the command line must open; last readback: {}",
        readback(&h)
    );
}

// ── 4. Failure is terminal and visible ─────────────────────────────────────

#[test]
#[serial]
fn a_failed_session_raises_a_retry_banner_and_does_not_loop() {
    let _rt = runtime();
    let _guard = _rt.0.enter();
    state_root();
    let _ = drain_pending();

    let window_id = uuid::Uuid::now_v7();
    poison_scratch_for(window_id);

    let mut h = Harness::new(
        Host,
        HostProps {
            window_id,
            cli_paths: Vec::new(),
            gestures: Vec::new(),
        },
    );
    assert!(
        pump(&mut h, |h| readback(h).contains("phase=failed:")),
        "the boot must reach a terminal Failed; last readback: {}",
        readback(&h)
    );

    let seen = readback(&h);
    assert!(
        seen.contains("engine_ok=false"),
        "a failed session must not report a live engine: {seen}"
    );
    assert!(
        !h.has_label(&dat0_i18n::t("session.booting")),
        "the skeleton belongs to Booting only — a terminal failure must not \
         pretend something is still in flight"
    );

    let banners = drain_pending();
    let failure = banners
        .iter()
        .find(|b| b.title == dat0_i18n::t("session.failed"))
        .unwrap_or_else(|| panic!("the Failed arm must raise a banner; saw {banners:?}"));
    assert!(
        !failure.body.is_empty(),
        "the anyhow chain is the only diagnosis available; it must be shown"
    );
    assert_eq!(
        failure.primary.as_ref().map(|a| a.action_id.as_str()),
        Some(dat0_core::actions::builtin::ids::SESSION_RETRY)
    );

    // Terminal: nothing re-armed the boot on its own, and no second banner
    // accumulated behind the first.
    for _ in 0..8 {
        h.settle();
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(
        readback(&h).contains("phase=failed:"),
        "a failed session must not retry itself: {}",
        readback(&h)
    );
    assert!(
        drain_pending().is_empty(),
        "a retry loop would show as a growing pile of identical banners"
    );
}

#[test]
#[serial]
fn a_drop_onto_a_failed_window_is_discarded_rather_than_queued() {
    let _rt = runtime();
    let _guard = _rt.0.enter();
    let root = state_root();
    let _ = drain_pending();

    let window_id = uuid::Uuid::now_v7();
    poison_scratch_for(window_id);
    let orphan = write_csv(&root.join("drops-failed"), "orphan.csv");

    let mut h = Harness::new(
        Host,
        HostProps {
            window_id,
            cli_paths: Vec::new(),
            gestures: vec![vec![orphan]],
        },
    );
    assert!(
        pump(&mut h, |h| readback(h).contains("phase=failed:")),
        "last readback: {}",
        readback(&h)
    );

    h.click("drop-0");
    for _ in 0..4 {
        h.settle();
        std::thread::sleep(Duration::from_millis(25));
    }

    let seen = readback(&h);
    assert!(
        seen.contains("queue=[]"),
        "a gesture held for a session that will never exist is a delayed \
         surprise, not a queue: {seen}"
    );
    assert!(
        seen.contains("tabs=[]"),
        "and nothing was registered: {seen}"
    );
    let _ = drain_pending();
}
