//! What the chrome states about the process (PD-023, step 5.8b): the bytes
//! dat0 has sent off this machine, and how many windows are open.
//!
//! The status bar's egress figure was a field nothing wrote, so it read
//! `egress 0 B` whatever had been sent, and the sidebar's footer said
//! `1 window` whatever was open. These mount the real shell, on a real `Boot`,
//! and change what it states from outside.
//!
//! The egress counter is process-wide: this binary is the one that records
//! into it, and its tests are `#[serial]`.

mod support;

use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::register_all;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::telemetry::egress;
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::Surface;
use dat0_ui::state::{Status, Workspace};
use dat0_ui::theme::Theme;
use support::Harness;

/// A config dir with the first run behind it, so no dialog opens over the
/// shell.
static CONFIG: LazyLock<()> = LazyLock::new(|| {
    let tmp = tempfile::tempdir().expect("tempdir");
    // SAFETY: every test in this binary is `#[serial]`, so no other thread
    // races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", tmp.path()) };
    let store =
        dat0_core::settings::store::SettingsStore::with_path(tmp.path().join("settings.toml"));
    dat0_core::settings::set_first_run_done(&store, true).expect("seed first_run_done");
    std::mem::forget(tmp);
});

thread_local! {
    /// The process's boot, shared by every window a test opens.
    static BOOT: Boot = {
        let reg = ActionRegistry::new();
        register_all(&reg).expect("built-in actions register");
        Boot::new(reg, Vec::new())
    };
}

/// `App`'s wiring, minus what needs a desktop window: the boot in context,
/// the window listed on it, and the shell.
#[component]
fn Window() -> Element {
    Theme::provide(None);
    let ws = Workspace::provide();
    let surface = use_context_provider(|| Signal::new(Option::<Surface>::None));
    let boot = use_context_provider(|| BOOT.with(Boot::clone));
    use_context_provider(|| boot.registry.clone());
    let _events = use_window_bus(boot, ws, surface);
    rsx! { Shell {} }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

/// Settle every window, then sleep, until `done` or two minutes.
fn pump(windows: &mut [&mut Harness], done: impl Fn(&[&mut Harness]) -> bool) -> bool {
    for _ in 0..4800 {
        for h in windows.iter_mut() {
            h.settle();
        }
        if done(windows) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn text(h: &Harness, id: &str) -> String {
    h.by_a11y_id(id).map(|k| h.text_of(k)).unwrap_or_default()
}

/// The figure the counter stands at now, as the chrome writes it.
fn measured() -> String {
    dat0_ui::chrome::egress_line(&Status {
        egress: egress::total_sent(),
        egress_floor: egress::has_unmetered_channel(),
        ..Status::default()
    })
}

#[test]
#[serial]
fn the_status_bar_and_the_footer_say_what_dat0_has_sent() {
    let rt = runtime();
    let _guard = rt.enter();
    LazyLock::force(&CONFIG);
    let mut h = Harness::new(Window, ());
    let states = |h: &Harness, line: &str| {
        text(h, "statusbar").contains(line) && text(h, "sidebar").contains(line)
    };
    assert!(
        pump(&mut [&mut h], |w| states(w[0], &measured())),
        "{:?}",
        text(&h, "statusbar")
    );

    // An AI request, say: bytes dat0 put on the wire.
    let before = egress::total_sent();
    egress::record_sent(3 * 1024);
    let sent = measured();
    assert_ne!(sent, "egress 0 B");
    assert!(
        pump(&mut [&mut h], |w| states(w[0], &sent)),
        "{sent:?}: {:?}",
        text(&h, "statusbar")
    );
    if before == 0 {
        assert!(text(&h, "statusbar").contains("egress 3.0 KB"));
    }

    // MotherDuck's own connection, which dat0 cannot meter: the figure is a
    // floor from now on, and says so.
    egress::note_unmetered_channel();
    let floor = measured();
    assert!(floor.ends_with('+'), "{floor:?}");
    assert!(
        pump(&mut [&mut h], |w| states(w[0], &floor)),
        "{floor:?}: {:?}",
        text(&h, "statusbar")
    );
}

#[test]
#[serial]
fn the_footer_counts_the_windows_open() {
    let rt = runtime();
    let _guard = rt.enter();
    LazyLock::force(&CONFIG);
    let footer = |h: &Harness| text(h, "sidebar");

    let mut first = Harness::new(Window, ());
    assert!(
        pump(&mut [&mut first], |w| footer(w[0])
            .contains("session · 1 window ·")),
        "{:?}",
        footer(&first)
    );

    let mut second = Harness::new(Window, ());
    assert!(
        pump(&mut [&mut first, &mut second], |w| {
            w.iter()
                .all(|h| footer(h).contains("session · 2 windows ·"))
        }),
        "{:?} / {:?}",
        footer(&first),
        footer(&second)
    );

    drop(second);
    assert!(
        pump(&mut [&mut first], |w| footer(w[0])
            .contains("session · 1 window ·")),
        "a window closed: {:?}",
        footer(&first)
    );
}
