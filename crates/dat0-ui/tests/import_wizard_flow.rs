//! A CSV the sniff cannot settle opens in the import wizard (PD-023, step
//! 5.10).
//!
//! `file_drop` said so (`DropOutcome::OpenWizard`) and the window logged it
//! and let it go: the file never opened, and nothing said why. These mount
//! the real shell over a real session. A file whose head is not UTF-8 is the
//! sniff's own trigger, and is opened as a drop would open it; a semicolon
//! file is handed to the wizard directly, as the sniff hands one whose two
//! samples disagree on the delimiter, and read in as the wizard is told.

mod support;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::register_all;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::import_wizard::SniffSummary;
use dat0_i18n::t;
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::Surface;
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

/// A state root and a config dir of the binary's own, the first run behind.
static STATE_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("state");
    let cfg = tmp.path().join("cfg");
    std::fs::create_dir_all(&root).expect("mkdir state");
    std::fs::create_dir_all(&cfg).expect("mkdir cfg");
    // SAFETY: every test in this binary is `#[serial]`, so no other thread
    // races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", &cfg) };
    dat0_core::settings::set_first_run_done(
        &dat0_core::settings::store::SettingsStore::with_path(cfg.join("settings.toml")),
        true,
    )
    .expect("seed first_run_done");
    dat0_core::globals::install_state_root(root.clone());
    std::mem::forget(tmp);
    root
});

/// A scratch directory for the test's files.
fn scratch() -> PathBuf {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().to_path_buf();
    std::mem::forget(dir);
    path
}

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    /// What the window opens, as the command line would.
    open: Vec<PathBuf>,
    /// A file to hand the wizard directly, as an ambiguous sniff would.
    offer: Option<PathBuf>,
}

/// `App`'s wiring, minus what needs a desktop window.
#[component]
fn Host(props: HostProps) -> Element {
    Theme::provide(None);
    let ws = Workspace::provide();
    let surface = use_context_provider(|| Signal::new(Option::<Surface>::None));
    let boot = use_hook(|| {
        let reg = ActionRegistry::new();
        register_all(&reg).expect("built-in actions register");
        Boot::new(reg, Vec::new())
    });
    use_context_provider(|| boot.registry.clone());
    session_boot::use_session(ws, props.open.clone());
    let _events = use_window_bus(boot, ws, surface);
    let ready = ws.session.read().ready().is_some();
    let offer = props.offer.clone();
    rsx! {
        if ready {
            div { "data-a11y-id": "session-ready" }
        }
        button {
            "data-a11y-id": "offer",
            onclick: move |_| {
                let session = ws.session.peek().ready().cloned().expect("session ready");
                let path = offer.clone().expect("a file to offer");
                let sniff = SniffSummary {
                    top_delimiter: ';',
                    top_score: 0.55,
                    next_score: 0.53,
                    encoding_supported: true,
                    any_low_confidence_column: false,
                };
                dat0_ui::import_flow::offer(ws, session, path, sniff);
            },
        }
        Shell {}
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

fn has(h: &Harness, id: &str) -> bool {
    h.by_a11y_id(id).is_some()
}

fn text(h: &Harness, id: &str) -> String {
    h.by_a11y_id(id).map(|k| h.text_of(k)).unwrap_or_default()
}

fn type_into(h: &mut Harness, id: &str, value: &str) {
    let field = h.by_a11y_id(id).unwrap_or_else(|| panic!("no {id}"));
    h.dispatch(
        field,
        "input",
        dioxus::html::SerializedFormData::new(value.to_string(), Vec::new()),
    );
    h.settle();
}

fn uncheck(h: &mut Harness, id: &str) {
    let field = h.by_a11y_id(id).unwrap_or_else(|| panic!("no {id}"));
    h.dispatch(
        field,
        "change",
        dioxus::html::SerializedFormData::new("false".to_string(), Vec::new()),
    );
    h.settle();
}

fn mount(open: Vec<PathBuf>, offer: Option<PathBuf>) -> Harness {
    let _ = STATE_ROOT.as_path();
    let mut h = Harness::new(Host, HostProps { open, offer });
    assert!(
        pump(&mut h, |h| has(h, "session-ready")),
        "the session opens"
    );
    h
}

/// A CSV in Latin-1: `é` is one byte there, and no UTF-8.
fn latin1(dir: &Path) -> PathBuf {
    let path = dir.join("cafes.csv");
    std::fs::write(&path, b"name,town\ncaf\xe9,Paris\n").expect("write");
    path
}

/// A CSV separated by semicolons, with commas in its values.
fn semicolons(dir: &Path) -> PathBuf {
    let path = dir.join("prices.csv");
    std::fs::write(
        &path,
        "id;label;price\n1;apples, red;2.5\n2;pears;3\n3;plums, dark;4.25\n",
    )
    .expect("write");
    path
}

#[test]
#[serial]
fn a_file_the_sniff_cannot_settle_opens_the_wizard_and_says_why() {
    let rt = runtime();
    let _guard = rt.enter();
    let dir = scratch();

    let mut h = mount(vec![latin1(&dir)], None);
    assert!(
        pump(&mut h, |h| has(h, "import-wizard")),
        "the wizard opens where the file used to be dropped without a word"
    );
    assert!(text(&h, "import-wizard").contains("cafes.csv"));
    assert!(
        text(&h, "wizard-issues").contains(&t("wizard.issue.encoding_unsupported")),
        "{:?}",
        text(&h, "wizard-issues")
    );

    h.click("wizard-cancel");
    h.settle();
    assert!(!has(&h, "import-wizard"));
    assert!(!has(&h, "tab-0"), "a cancelled wizard opens nothing");
}

#[test]
#[serial]
fn the_wizard_reads_the_file_as_it_is_told() {
    let rt = runtime();
    let _guard = rt.enter();
    let dir = scratch();
    let file = semicolons(&dir);

    let mut h = mount(Vec::new(), Some(file));
    h.click("offer");
    assert!(
        pump(&mut h, |h| has(h, "wizard-delimiter")),
        "the wizard opens"
    );

    // The columns follow the dialect: split on a character the file does
    // not hold, each line is one column; on semicolons, three.
    type_into(&mut h, "wizard-delimiter", "|");
    h.click("wizard-next");
    assert!(
        pump(&mut h, |h| has(h, "wizard-name-0")
            && !has(h, "wizard-name-1")),
        "one column: {:?}",
        text(&h, "wizard-columns")
    );
    h.click("wizard-back");
    type_into(&mut h, "wizard-delimiter", ";");
    // Read without a header, which the sniff would never choose here: the
    // first line becomes a row, and the columns are named by position.
    uncheck(&mut h, "wizard-header");
    h.click("wizard-next");
    assert!(
        pump(&mut h, |h| has(h, "wizard-name-2")),
        "semicolons read three: {:?}",
        text(&h, "wizard-columns")
    );

    // The second renamed, the third left out.
    type_into(&mut h, "wizard-name-1", "fruit");
    uncheck(&mut h, "wizard-include-2");
    h.click("wizard-next");
    h.click("wizard-import");
    assert!(
        pump(&mut h, |h| text(h, "cell-1-1") == "apples, red"),
        "the file is read as told: {:?} / {:?}",
        text(&h, "tabstrip"),
        text(&h, "banner-host")
    );
    assert!(!has(&h, "import-wizard"));
    assert_eq!(text(&h, "cell-0-0"), "id", "the first line is a row");
    assert!(
        text(&h, "tab-0").contains("prices"),
        "{:?}",
        text(&h, "tab-0")
    );
    let header = text(&h, "grid-head");
    assert!(header.contains("fruit"), "renamed: {header:?}");
    assert!(!header.contains("column2"), "left out: {header:?}");
}
