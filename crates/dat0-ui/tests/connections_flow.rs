//! MotherDuck and attached databases (step 5.6c).
//!
//! The connections panel was finished and nothing opened it, and the async
//! half of its intents had no host (PD-023). These tests mount the real shell
//! over a real session. A database of the session's own stands in for the
//! MotherDuck account, and the token store is in memory, so nothing reaches
//! the network or the OS keychain. They connect with a token typed into its
//! prompt, open a table from CONNECTIONS, disconnect, connect again when a
//! session lands, and detach a SQLite file.

mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::register_all;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::connections::ConnectionStatus;
use dat0_core::connections::token_store::{MemoryTokenStore, TokenStore};
use dat0_core::events::Opening;
use dat0_core::session::{PersistedAttachment, PersistedAttachmentKind, Session};
use dat0_engine::{DuckDBEngine, QueryEngine as _};
use dat0_i18n::t;
use dat0_ui::components::settings_ui::CONNECTIONS_OPEN;
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::connections_flow::{Connected, Connector, Deps};
use dat0_ui::launch::Boot;
use dat0_ui::router::{Surface, route};
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

const BUDGET: u64 = 256 * 1024 * 1024;

/// Process-global state root, config dir and recents store, leaked so a
/// session never reads a deleted directory mid-test.
static STATE_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    dat0_engine::extension_bootstrap::__test_install_sqlite_scanner().expect("sqlite_scanner");
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
    dat0_core::globals::install_recents(std::sync::Arc::new(std::sync::Mutex::new(
        dat0_core::recents::Recents::with_path(tmp.path().join("recents.json")),
    )));
    std::mem::forget(tmp);
    root
});

/// MotherDuck, stood in for: a token it accepts attaches a database of the
/// session's own named `cloud`, holding `trips`.
struct StandIn;

const GOOD: &str = "md-good-token";

impl Connector for StandIn {
    fn connect(&self, engine: Arc<DuckDBEngine>, token: String) -> Connected {
        Box::pin(async move {
            if token != GOOD {
                return (
                    ConnectionStatus::Error(t("connections.error.auth")),
                    Vec::new(),
                );
            }
            engine
                .execute("ATTACH IF NOT EXISTS ':memory:' AS cloud")
                .await
                .expect("attach the stand-in");
            engine
                .execute("CREATE TABLE IF NOT EXISTS cloud.main.trips AS SELECT 7 AS miles")
                .await
                .expect("its table");
            (ConnectionStatus::Connected, vec!["cloud".to_string()])
        })
    }
}

#[derive(Clone, Props)]
struct HostProps {
    opening: Opening,
    tokens: Arc<MemoryTokenStore>,
    boot: Boot,
}

/// Props never change after mount.
impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        self.opening == other.opening
    }
}

/// `App`'s wiring, minus what needs a desktop window, with the connections'
/// dependencies provided the way a test must.
#[component]
fn Host(props: HostProps) -> Element {
    Theme::provide(None);
    let ws = Workspace::provide_for(&props.opening);
    let surface = use_context_provider(|| Signal::new(Option::<Surface>::None));
    let boot = use_context_provider(|| props.boot.clone());
    use_context_provider(|| boot.registry.clone());
    let tokens = props.tokens.clone();
    use_context_provider(move || Deps {
        tokens,
        connector: Arc::new(StandIn),
    });
    session_boot::use_session_on(ws, props.opening.clone());
    let events = use_window_bus(boot, ws, surface);
    // The session's attachments, read on demand: the session is not a signal.
    let mut recorded = use_signal(String::new);
    rsx! {
        button {
            "data-a11y-id": "open-connections",
            onclick: move |_| assert!(route(ws, &events, surface, CONNECTIONS_OPEN), "not routed"),
        }
        button {
            "data-a11y-id": "read-attachments",
            onclick: move |_| {
                let names = ws.session.peek().ready().map(|s| {
                    s.lock()
                        .attachments()
                        .iter()
                        .map(|a| a.alias.clone())
                        .collect::<Vec<_>>()
                        .join(",")
                });
                recorded.set(names.unwrap_or_default());
            },
        }
        div { "data-a11y-id": "attachments", "{recorded}" }
        Shell {}
    }
}

fn mount(opening: Opening, tokens: Arc<MemoryTokenStore>) -> Harness {
    let _ = STATE_ROOT.as_path();
    let reg = ActionRegistry::new();
    register_all(&reg).expect("built-in actions register");
    let boot = Boot::new(reg, Vec::new());
    Harness::new(
        Host,
        HostProps {
            opening,
            tokens,
            boot,
        },
    )
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

fn type_into(h: &mut Harness, id: &str, value: &str) {
    let field = h.by_a11y_id(id).expect("the field");
    let typed = dioxus::html::SerializedFormData::new(value.to_string(), Vec::new());
    h.dispatch(field, "input", typed);
    h.settle();
}

/// The CONNECTIONS rows, in order.
fn connections(h: &Harness) -> Vec<String> {
    (0..20)
        .map(|i| text(h, &format!("row-connections-{i}")))
        .take_while(|v| !v.is_empty())
        .collect()
}

fn recorded(h: &mut Harness) -> String {
    h.click("read-attachments");
    h.settle();
    text(h, "attachments")
}

/// The status bar's engine, in `state`.
fn engine(state: &str) -> String {
    format!("{} · {}", t("status.engine"), t(state))
}

fn status(h: &Harness) -> String {
    text(h, "connections-md-status")
}

/// A window over a CSV, with its grid painted.
fn window(dir: &str, tokens: Arc<MemoryTokenStore>) -> Harness {
    let dir = STATE_ROOT.join(dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let csv = dir.join("local.csv");
    std::fs::write(&csv, "id\n1\n").expect("write csv");
    let mut h = mount(Opening::files(vec![csv]), tokens);
    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "1"),
        "no grid painted"
    );
    h
}

#[test]
#[serial]
fn a_token_typed_connects_and_its_databases_open_from_the_sidebar() {
    let rt = runtime();
    let _guard = rt.enter();
    let tokens = Arc::new(MemoryTokenStore::default());
    let mut h = window("connects", tokens.clone());

    h.click("open-connections");
    assert!(pump(&mut h, |h| has(h, "connections")), "the panel opens");
    assert_eq!(status(&h), t("connections.md.status.disconnected"));

    h.click("connections-md-connect");
    assert!(
        pump(&mut h, |h| has(h, "name-prompt")),
        "it asks for a token"
    );
    let field = h.by_a11y_id("name-prompt-field").unwrap();
    assert_eq!(h.attr(field, "type").as_deref(), Some("password"));
    type_into(&mut h, "name-prompt-field", GOOD);
    h.click("name-prompt-ok");
    assert!(
        pump(&mut h, |h| status(h)
            == t("connections.md.status.connected")),
        "connected: {:?}",
        status(&h)
    );
    assert_eq!(text(&h, "connections-db-0"), "cloud");
    assert_eq!(
        tokens.get().unwrap().as_deref(),
        Some(GOOD),
        "the token is kept"
    );
    assert!(!h.html().contains(GOOD), "never in the page");
    assert_eq!(recorded(&mut h), "md", "the session records MotherDuck");

    assert!(
        pump(&mut h, |h| connections(h) == ["cloud 1", "trips"]),
        "{:?}",
        connections(&h)
    );
    h.click("row-connections-1");
    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "7"),
        "its table in a tab: {:?}",
        text(&h, "cell-0-0")
    );
}

#[test]
#[serial]
fn a_rejected_token_says_so_and_a_test_reports_it() {
    let rt = runtime();
    let _guard = rt.enter();
    let tokens = Arc::new(MemoryTokenStore::default());
    tokens.set("md-stale-token").unwrap();
    let mut h = window("rejected", tokens);

    h.click("open-connections");
    assert!(pump(&mut h, |h| has(h, "connections")));
    h.click("connections-md-test");
    assert!(
        pump(&mut h, |h| text(h, "connections-md-test-result")
            .contains(&t("connections.error.auth"))),
        "{:?}",
        text(&h, "connections")
    );
    // A failed probe leaves the panel in error, which offers Retry.
    h.click("connections-md-retry");
    assert!(
        pump(&mut h, |h| text(h, "connections-md-error")
            .contains(&t("connections.error.auth"))),
        "{:?}",
        text(&h, "connections")
    );
    assert!(connections(&h).is_empty());
    assert_eq!(recorded(&mut h), "", "nothing recorded");
}

#[test]
#[serial]
fn disconnecting_forgets_motherduck_in_the_session() {
    let rt = runtime();
    let _guard = rt.enter();
    let tokens = Arc::new(MemoryTokenStore::default());
    tokens.set(GOOD).unwrap();
    let mut h = window("disconnects", tokens.clone());

    h.click("open-connections");
    assert!(pump(&mut h, |h| has(h, "connections")));
    h.click("connections-md-connect");
    assert!(pump(&mut h, |h| connections(h) == ["cloud 1", "trips"]));
    // The title bar's pill and the status bar's engine say so (step 5.8c).
    assert!(
        pump(&mut h, |h| text(h, "source-pill").contains("live")
            && text(h, "status-engine")
                == engine("status.engine.motherduck")),
        "{:?} / {:?}",
        text(&h, "source-pill"),
        text(&h, "status-engine")
    );

    h.click("connections-md-disconnect");
    assert!(
        pump(&mut h, |h| connections(h).is_empty()),
        "{:?}",
        connections(&h)
    );
    assert_eq!(status(&h), t("connections.md.status.disconnected"));
    assert!(
        pump(&mut h, |h| text(h, "source-pill").contains("local")
            && text(h, "status-engine") == engine("status.engine.native")),
        "{:?} / {:?}",
        text(&h, "source-pill"),
        text(&h, "status-engine")
    );
    assert_eq!(recorded(&mut h), "");
    assert_eq!(
        tokens.get().unwrap().as_deref(),
        Some(GOOD),
        "the token stays"
    );

    // Connected again with the token kept, then Forget: token and all.
    h.click("connections-md-connect");
    assert!(pump(&mut h, |h| connections(h) == ["cloud 1", "trips"]));
    assert!(!has(&h, "name-prompt"), "the kept token is used");
    h.click("connections-md-forget");
    assert!(pump(&mut h, |h| connections(h).is_empty()));
    assert_eq!(tokens.get().unwrap(), None, "Forget drops it");
    assert_eq!(recorded(&mut h), "");
}

#[test]
#[serial]
fn a_session_opened_again_connects_again_while_a_token_is_kept() {
    let rt = runtime();
    let _guard = rt.enter();
    let dir = rt.block_on(async {
        let mut sess = Session::new(&STATE_ROOT.join("sessions"), BUDGET)
            .await
            .expect("session");
        sess.set_attachments(vec![PersistedAttachment {
            alias: "md".into(),
            kind: PersistedAttachmentKind::Md,
        }])
        .expect("record md");
        let dir = sess.home.root_dir().to_path_buf();
        sess.engine.close().await.expect("close");
        dir
    });
    let tokens = Arc::new(MemoryTokenStore::default());
    tokens.set(GOOD).unwrap();

    let mut h = mount(Opening::Recover { dir }, tokens);
    assert!(
        pump(&mut h, |h| connections(h) == ["cloud 1", "trips"]),
        "connected at landing, with no panel opened: {:?}",
        connections(&h)
    );
}

#[test]
#[serial]
fn a_sqlite_file_detaches_and_its_tabs_close_with_it() {
    let rt = runtime();
    let _guard = rt.enter();
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/small/simple.sqlite");
    let dir = STATE_ROOT.join("detach");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let file = dir.join("shop.sqlite");
    std::fs::copy(fixture, &file).expect("copy the fixture");
    let file = std::fs::canonicalize(file).expect("canonical");
    let mut h = mount(
        Opening::files(vec![file.clone()]),
        Arc::new(MemoryTokenStore::default()),
    );
    assert!(
        pump(&mut h, |h| text(h, "cell-0-1") == "a"),
        "the file's first table in a tab"
    );
    assert_eq!(connections(&h), ["shop 1", "items"]);

    h.click("open-connections");
    assert!(pump(&mut h, |h| has(h, "connections-file-shop")));
    assert!(text(&h, "connections-file-shop").contains(&file.display().to_string()));
    h.click("connections-detach-shop");
    assert!(
        pump(&mut h, |h| connections(h).is_empty()),
        "{:?}",
        connections(&h)
    );
    assert!(
        !h.html().contains("shop_items"),
        "its table's tab closed with it"
    );
    assert_eq!(recorded(&mut h), "");
    assert!(!has(&h, "connections-file-shop"));
    let _ = Path::new("");
}
