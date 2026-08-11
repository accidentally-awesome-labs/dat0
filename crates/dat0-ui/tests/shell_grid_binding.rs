//! Opening a file must actually put a grid on screen.
//!
//! `tests/session_boot_slot.rs` covers the queue: launch arguments and drops
//! reach `ws.tabs` in the right order, whether or not DuckDB has finished
//! opening. It proves that against a `Host` that renders a readback string and
//! the hero — it never mounts `Shell`. So everything between "a tab exists" and
//! "the user sees their data" was untested, and that is exactly where
//! `dat0 iris.csv` was losing the file: the tab was in `session.json`, the tab
//! strip had it selected, and the work area showed an empty
//! `.d0-grid-loading` div forever.
//!
//! The gap is one `use_resource`. It binds `GridDataSource` for the active tab,
//! and a resource restarts only when a signal READ INSIDE IT changes. The
//! active table was read during `Shell`'s render and captured into the closure,
//! so the future was subscribed to nothing at all on the one poll that mattered
//! — it short-circuited on `table?` before it ever touched `session`.
//!
//! These tests mount the real `Shell` over a real session and assert on the
//! rendered tree, which is the only place that distinguishes "the tab is
//! recorded" from "the grid is showing".

mod support;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_ui::components::shell::Shell;
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

/// Process-global state root and config dir, leaked so a session never reads a
/// deleted directory mid-test. Copied from `session_boot_slot.rs`, which is the
/// suite that established this shape.
static STATE_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("state");
    let cfg = tmp.path().join("cfg");
    std::fs::create_dir_all(&root).expect("mkdir state");
    std::fs::create_dir_all(&cfg).expect("mkdir cfg");
    // SAFETY: every test in this binary is `#[serial]`, so no other thread
    // races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", &cfg) };
    // Without this the shell auto-opens the first-run tour over the grid.
    dat0_core::settings::set_first_run_done(
        &dat0_core::settings::store::SettingsStore::with_path(cfg.join("settings.toml")),
        true,
    )
    .expect("seed first_run_done");
    dat0_core::globals::install_state_root(root.clone());
    std::mem::forget(tmp);
    root
});

fn state_root() -> &'static Path {
    STATE_ROOT.as_path()
}

/// A two-column CSV. One column leaves the sniffer with no delimiter to agree
/// on and `handle_drop` routes to the import wizard instead of registering.
fn write_csv(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).expect("mkdir");
    let p = dir.join(name);
    std::fs::write(&p, "a,b\n1,2\n3,4\n").expect("write csv");
    p
}

#[derive(Clone, PartialEq, Props, Default)]
struct HostProps {
    /// Launch arguments, as `launch::main` supplies them.
    #[props(default)]
    cli_paths: Vec<PathBuf>,
    /// One entry per open gesture the test can perform. Each renders a button
    /// at `data-a11y-id="open-{i}"` — the harness has no other way to reach
    /// `open_paths` on a window that is already up.
    #[props(default)]
    gestures: Vec<Vec<PathBuf>>,
}

/// The real `Shell`, under the contexts `App` provides and the session boot it
/// runs. Everything here is `App`'s wiring minus the asset handler and the menu
/// — neither of which exists without a desktop window.
#[component]
fn Host(props: HostProps) -> Element {
    Theme::provide(None);
    let ws = Workspace::provide();
    use_context_provider(|| {
        let reg = ActionRegistry::new();
        dat0_core::actions::builtin::register_all(&reg).expect("builtins register");
        reg
    });
    use_context_provider(|| AppEvents::channel().0);
    session_boot::use_session(ws, props.cli_paths.clone());

    rsx! {
        for (i, paths) in props.gestures.iter().cloned().enumerate() {
            button {
                key: "{i}",
                "data-a11y-id": "open-{i}",
                onclick: move |_| {
                    let paths = paths.clone();
                    spawn(async move { session_boot::open_paths(ws, paths).await });
                },
                "open {i}"
            }
        }
        Shell {}
    }
}

/// Settle, then sleep, until `done` or two minutes. See
/// `session_boot_slot.rs::pump` for why the budget is nowhere near the
/// expected time.
fn pump(h: &mut Harness, done: impl Fn(&Harness) -> bool) -> bool {
    for _ in 0..4800 {
        h.settle();
        if done(h) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

fn has(h: &Harness, id: &str) -> bool {
    h.by_a11y_id(id).is_some()
}

/// `dat0 <file.csv>` must end with a grid, not a skeleton.
#[test]
#[serial]
fn a_launch_argument_ends_up_as_a_bound_grid() {
    let rt = runtime();
    let _guard = rt.enter();
    let csv = write_csv(&state_root().join("launch-arg"), "iris.csv");

    let mut h = Harness::new(
        Host,
        HostProps {
            cli_paths: vec![csv],
            ..Default::default()
        },
    );

    assert!(
        pump(&mut h, |h| has(h, "grid")),
        "the launch argument never reached a grid. tab present: {}, still on the \
         loading placeholder: {}. A tab the tab strip shows and a work area that \
         renders nothing is the exact shape of the bug this test exists for.",
        has(&h, "tabstrip"),
        has(&h, "grid-loading"),
    );

    assert!(
        !has(&h, "grid-loading"),
        "the loading placeholder must be gone once the source is bound"
    );
    assert!(
        !has(&h, "grid-error"),
        "binding the CSV must not have errored"
    );
}

/// The same path, reached the way a user reaches it: the window is already up
/// and the session is already `Ready` when the file arrives.
///
/// Distinct from the launch-argument case, because the queue drain and a live
/// `open_paths` enter the resource from different states — and the resource is
/// the thing under test.
#[test]
#[serial]
fn a_file_opened_after_boot_also_binds() {
    let rt = runtime();
    let _guard = rt.enter();
    let csv = write_csv(&state_root().join("after-boot"), "later.csv");

    let mut h = Harness::new(
        Host,
        HostProps {
            gestures: vec![vec![csv]],
            ..Default::default()
        },
    );

    // The hero is up and the session has landed; no tab yet.
    assert!(
        pump(&mut h, |h| has(h, "empty-state")),
        "the empty state never rendered"
    );
    assert!(!has(&h, "grid"), "no grid before a file is opened");

    h.click("open-0");

    assert!(
        pump(&mut h, |h| has(h, "grid")),
        "a file opened into a live window never reached a grid; still loading: {}",
        has(&h, "grid-loading")
    );
    assert!(
        pump(&mut h, |h| has(h, "cell-0-0")),
        "and it must paint cells, not just a frame"
    );
}
