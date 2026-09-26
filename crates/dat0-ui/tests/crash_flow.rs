//! The crash report offered at relaunch, and Report a Bug (step 5.7d).
//!
//! The report panel and its relaunch gate were built and nothing called
//! either (PD-023): a crashed run's staged report was never offered, and no
//! command opened a bug report. These tests stage a crash where the panic
//! hook would, and mount the real shell as the next launch's first window.
//!
//! The offer is made once per process, so exactly one test here mounts the
//! real shell; the others drive the flow's functions under a modal host.
//!
//! A report sent here goes to a transport of the test's own (`SENT`), never
//! the network: the settings here say reports are on, and Send reads them
//! when it is pressed (step 5.11a).

mod support;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::register_all;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::telemetry::crash::{self, StagedCrash};
use dat0_core::telemetry::egress;
use dat0_i18n::t;
use dat0_ui::components::modals::{ModalHost, ModalOutcome, ModalReply};
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::Surface;
use dat0_ui::session_boot;
use dat0_ui::state::{Modal, Workspace};
use dat0_ui::theme::Theme;
use support::Harness;

/// The reports this binary has sent, as its own transport counts them.
static SENT: LazyLock<Arc<Captured>> = LazyLock::new(|| Arc::new(Captured(AtomicUsize::new(0))));

struct Captured(AtomicUsize);

impl sentry::Transport for Captured {
    fn send_envelope(&self, _: sentry::Envelope) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn sent() -> usize {
    SENT.0.load(Ordering::SeqCst)
}

/// The state root the crash guard would stage into, with a crash from the
/// last run staged, and the config dir, with crash reports opted in.
static STATE_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    // A client of the test's own, bound on the process's hub before any
    // test reads a hub, so each thread's starts from it and Send never binds
    // the real one.
    let transport = SENT.clone();
    let client = sentry::Client::from(sentry::ClientOptions {
        dsn: Some("https://public@dat0.invalid/1".parse().expect("a dsn")),
        transport: Some(Arc::new(move |_: &sentry::ClientOptions| {
            transport.clone() as Arc<dyn sentry::Transport>
        })),
        ..Default::default()
    });
    sentry::Hub::main().bind_client(Some(Arc::new(client)));
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("state");
    let cfg = tmp.path().join("cfg");
    std::fs::create_dir_all(&root).expect("mkdir state");
    std::fs::create_dir_all(&cfg).expect("mkdir cfg");
    // SAFETY: every test in this binary is `#[serial]`, so no other thread
    // races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", &cfg) };
    let store = dat0_core::settings::store::SettingsStore::with_path(cfg.join("settings.toml"));
    dat0_core::settings::set_first_run_done(&store, true).expect("seed first_run_done");
    let mut settings = store.load_or_default().expect("settings");
    settings.telemetry.crash_submission_enabled = true;
    store.save(&settings).expect("opt in");
    crash::mark_running(&root).expect("the last run's marker");
    crash::write_staged(&root, &staged()).expect("stage the crash");
    dat0_core::globals::install_state_root(root.clone());
    std::mem::forget(tmp);
    root
});

fn staged() -> StagedCrash {
    StagedCrash {
        message: "panicked at the grid: index out of bounds".into(),
        backtrace: "0: dat0::grid::paint".into(),
        version: "0.1.0".into(),
    }
}

/// `App`'s wiring, minus what needs a desktop window.
#[component]
fn ShellHost() -> Element {
    Theme::provide(None);
    let ws = Workspace::provide();
    let surface = use_context_provider(|| Signal::new(Option::<Surface>::None));
    let boot = use_hook(|| {
        let reg = ActionRegistry::new();
        register_all(&reg).expect("built-in actions register");
        Boot::new(reg, Vec::new())
    });
    use_context_provider(|| boot.registry.clone());
    session_boot::use_session(ws, Vec::new());
    let _events = use_window_bus(boot, ws, surface);
    rsx! { Shell {} }
}

#[derive(Clone, PartialEq, Props)]
struct SlotProps {
    /// A dialog up before the flow runs.
    busy: bool,
}

/// A modal host, and buttons that run the flow's functions.
#[component]
fn Slot(props: SlotProps) -> Element {
    Theme::provide(None);
    let mut ws = Workspace::provide();
    let busy = props.busy;
    rsx! {
        button {
            "data-a11y-id": "offer",
            onclick: move |_| {
                if busy {
                    ws.modal.set(Some(Modal::NamePrompt {
                        title: "Save query as…".to_string(),
                        initial: String::new(),
                        placeholder: None,
                        confirm_label: None,
                        secret: false,
                        reply: ModalReply::new(|_: ModalOutcome| {}),
                    }));
                }
                dat0_ui::crash_flow::offer(ws, STATE_ROOT.clone(), staged());
            },
        }
        button {
            "data-a11y-id": "report-bug",
            onclick: move |_| dat0_ui::crash_flow::report_bug(ws),
        }
        ModalHost {}
        div { "data-a11y-id": "banners",
            for (i, b) in ws.banners.read().iter().enumerate() {
                p { key: "{i}", "{b.title}" }
            }
        }
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

/// Settle, then sleep, until `done` or two minutes.
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

fn text(h: &Harness, id: &str) -> String {
    h.by_a11y_id(id).map(|k| h.text_of(k)).unwrap_or_default()
}

fn has(h: &Harness, id: &str) -> bool {
    h.by_a11y_id(id).is_some()
}

fn staged_on_disk(root: &Path) -> bool {
    crash::staged_path(root).exists()
}

/// Turn crash reports on or off in this binary's settings.
fn reports(on: bool) {
    let dir = dat0_core::platform::config_dir().expect("config dir");
    let store = dat0_core::settings::store::SettingsStore::with_path(dir.join("settings.toml"));
    let mut settings = store.load_or_default().expect("settings");
    settings.telemetry.crash_submission_enabled = on;
    store.save(&settings).expect("save settings");
}

/// Crash reports off until dropped, then on again, as the other tests expect.
struct ReportsOff;

impl ReportsOff {
    fn now() -> Self {
        reports(false);
        Self
    }
}

impl Drop for ReportsOff {
    fn drop(&mut self) {
        reports(true);
    }
}

#[test]
#[serial]
fn a_crashed_run_is_offered_its_report_at_the_next_launch() {
    let rt = runtime();
    let _guard = rt.enter();
    let root = STATE_ROOT.clone();
    // Staged here as well as at the root's making: a report sent by a test
    // before this one clears it, as either exit does.
    crash::mark_running(&root).expect("the last run's marker");
    crash::write_staged(&root, &staged()).expect("stage the crash");
    assert!(staged_on_disk(&root), "seed: a crash is staged");

    let mut h = Harness::new(ShellHost, ());
    assert!(
        pump(&mut h, |h| has(h, "report")),
        "the first window offers the report"
    );
    assert!(
        text(&h, "modal").contains(&t("crash.dialog.title")),
        "{:?}",
        text(&h, "modal")
    );
    assert!(
        text(&h, "report").contains(&t("crash.dialog.body")),
        "{:?}",
        text(&h, "report")
    );

    h.click("report-dismiss");
    h.settle();
    assert!(!has(&h, "report"));
    assert!(
        !staged_on_disk(&root),
        "a report declined is deleted, never sent later"
    );
}

#[test]
#[serial]
fn a_dialog_already_up_keeps_its_place_and_the_report_stays_staged() {
    let rt = runtime();
    let _guard = rt.enter();
    let root = STATE_ROOT.clone();
    crash::write_staged(&root, &staged()).expect("stage again");

    let mut h = Harness::new(Slot, SlotProps { busy: true });
    h.settle();
    h.click("offer");
    h.settle();
    assert!(!has(&h, "report"));
    assert!(text(&h, "modal").contains("Save query as"));
    assert!(staged_on_disk(&root), "offered at a later launch instead");
}

#[test]
#[serial]
fn report_a_bug_opens_the_report_with_nothing_staged() {
    let rt = runtime();
    let _guard = rt.enter();
    let _ = STATE_ROOT.as_path();

    let mut h = Harness::new(Slot, SlotProps { busy: false });
    h.settle();
    h.click("report-bug");
    h.settle();
    assert!(has(&h, "report"));
    assert!(
        text(&h, "modal").contains(&t("report.dialog.title")),
        "{:?}",
        text(&h, "modal")
    );
    assert!(text(&h, "report").contains(&t("report.dialog.body")));
}

#[test]
#[serial]
fn a_bug_report_sent_says_so_and_counts_what_left() {
    let rt = runtime();
    let _guard = rt.enter();
    let _ = STATE_ROOT.as_path();
    assert!(
        dat0_core::telemetry::submission_allowed(),
        "seed: reports on"
    );
    let (reports_before, bytes_before) = (sent(), egress::total_sent());

    let mut h = Harness::new(Slot, SlotProps { busy: false });
    h.settle();
    h.click("report-bug");
    h.settle();
    h.click("report-send");
    h.settle();

    assert!(!has(&h, "report"));
    assert_eq!(sent(), reports_before + 1, "the report went");
    assert!(
        egress::total_sent() > bytes_before,
        "and what left this machine is counted"
    );
    assert!(
        text(&h, "banners").contains(&t("report.sent")),
        "{:?}",
        text(&h, "banners")
    );
}

#[test]
#[serial]
fn report_a_bug_while_reports_are_off_says_so_and_sends_nothing() {
    let rt = runtime();
    let _guard = rt.enter();
    let _ = STATE_ROOT.as_path();
    let _off = ReportsOff::now();

    let mut h = Harness::new(Slot, SlotProps { busy: false });
    h.settle();
    h.click("report-bug");
    h.settle();
    assert_eq!(text(&h, "report-body"), t("report.dialog.off"));
    assert!(
        !has(&h, "report-send"),
        "no Send for a report that cannot go"
    );
}

#[test]
#[serial]
fn a_report_sent_after_reports_are_turned_off_does_not_go_and_says_so() {
    // Settings is a window of its own: reports can be turned off while a
    // crash report is up. Send reads them when it is pressed.
    let rt = runtime();
    let _guard = rt.enter();
    let root = STATE_ROOT.clone();
    crash::write_staged(&root, &staged()).expect("stage again");
    let (reports_before, bytes_before) = (sent(), egress::total_sent());

    let mut h = Harness::new(Slot, SlotProps { busy: false });
    h.settle();
    h.click("offer");
    h.settle();
    assert!(has(&h, "report-send"));
    let _off = ReportsOff::now();
    h.click("report-send");
    h.settle();

    assert!(!has(&h, "report"));
    assert_eq!(sent(), reports_before, "nothing went");
    assert_eq!(egress::total_sent(), bytes_before, "nothing is counted");
    assert!(
        text(&h, "banners").contains(&t("report.not_sent")),
        "{:?}",
        text(&h, "banners")
    );
    assert!(!staged_on_disk(&root), "and it is not offered again");
}
