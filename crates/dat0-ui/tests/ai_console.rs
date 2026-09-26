//! NL→SQL and Explain in the console (step 5.6b).
//!
//! The console drew a strip for an AI answer and the controller could stream
//! one, and nothing started either: the console had no button for them and
//! the shell handed it an empty stream (PD-023). These tests mount the real
//! shell over a real session with a ready AI whose answers are scripted, so
//! nothing reaches the network, and use the console as a user does: ask for a
//! statement, take it and run it, have one explained, and stop an answer.

mod support;

use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::ai::key_store::{KeyStore, MemoryKeyStore};
use dat0_core::ai::provider::Provider;
use dat0_core::ai::transport::TestOutcome;
use dat0_core::ai::{AiRequest, AiSettings};
use dat0_core::settings::store::SettingsStore;
use dat0_i18n::t;
use dat0_ui::ai_flow::Deps;
use dat0_ui::components::ai::{AiDeps, AiFuture, AiProbe};
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::{Surface, route};
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

/// Process-global state root and config dir, leaked so a session never reads a
/// deleted directory mid-test. The shape `shell_grid_binding.rs` uses.
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

const COMMANDS: &[&str] = &[ids::SQL_RUN, ids::CONSOLE_TOGGLE];

/// What the scripted AI answers: each delta in turn, then the end, or never
/// an end at all.
#[derive(Clone)]
struct Script {
    deltas: Vec<&'static str>,
    ends: bool,
    /// What it was asked, and with which schema: the requests it saw.
    asked: Arc<Mutex<Vec<AiRequest>>>,
}

impl AiProbe for Script {
    fn test_connection(&self, _: Provider, _: String, _: AiSettings) -> AiFuture<TestOutcome> {
        Box::pin(async {
            TestOutcome {
                ok: true,
                message: String::new(),
            }
        })
    }

    fn stream(
        &self,
        _: Provider,
        _: String,
        _: AiSettings,
        req: AiRequest,
        mut on_delta: Box<dyn FnMut(String)>,
    ) -> AiFuture<Result<String, String>> {
        self.asked.lock().unwrap().push(req);
        let (deltas, ends) = (self.deltas.clone(), self.ends);
        Box::pin(async move {
            for d in &deltas {
                on_delta(d.to_string());
            }
            if !ends {
                std::future::pending::<()>().await;
            }
            Ok(deltas.concat())
        })
    }
}

#[derive(Clone, Props)]
struct HostProps {
    cli_paths: Vec<PathBuf>,
    deps: Deps,
}

impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        self.cli_paths == other.cli_paths
    }
}

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
    let deps = props.deps.clone();
    use_context_provider(move || deps);
    session_boot::use_session(ws, props.cli_paths.clone());
    let events = use_window_bus(boot, ws, surface);
    rsx! {
        for id in COMMANDS.iter().copied() {
            button {
                key: "{id}",
                "data-a11y-id": "do-{id}",
                onclick: {
                    let events = events.clone();
                    move |_| assert!(route(ws, &events, surface, id), "{id} was not routed")
                },
            }
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

fn text(h: &Harness, id: &str) -> String {
    h.by_a11y_id(id).map(|k| h.text_of(k)).unwrap_or_default()
}

fn has(h: &Harness, id: &str) -> bool {
    h.by_a11y_id(id).is_some()
}

fn disabled(h: &Harness, id: &str) -> bool {
    h.by_a11y_id(id)
        .and_then(|k| h.attr(k, "disabled"))
        .as_deref()
        == Some("true")
}

fn type_into(h: &mut Harness, id: &str, value: &str) {
    let field = h.by_a11y_id(id).expect("the field");
    let typed = dioxus::html::SerializedFormData::new(value.to_string(), Vec::new());
    h.dispatch(field, "input", typed);
    h.settle();
}

/// AI dependencies, ready or not: a provider, a key and a model, and AI on.
fn deps(script: &Script, ready: bool) -> Deps {
    let store = SettingsStore::open_in_memory();
    let mut settings = store.load_or_default().unwrap();
    settings.ai.enabled = ready;
    settings.ai.provider = Some(Provider::Anthropic.id().to_string());
    settings.ai.model = "claude-test".into();
    store.save(&settings).unwrap();
    let keys = MemoryKeyStore::default();
    keys.set(Provider::Anthropic, "sk-test").unwrap();
    Deps {
        ai: AiDeps {
            store: Arc::new(store),
            keys: Arc::new(keys),
            probe: Arc::new(script.clone()),
        },
        volatile: false,
    }
}

fn script(deltas: &[&'static str], ends: bool) -> Script {
    Script {
        deltas: deltas.to_vec(),
        ends,
        asked: Arc::new(Mutex::new(Vec::new())),
    }
}

/// A window over a CSV of sales, with AI as `deps` has it, and its console
/// open.
fn window(dir: &str, deps: Deps) -> Harness {
    let dir = STATE_ROOT.join(dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let csv = dir.join("sales.csv");
    std::fs::write(&csv, "region,amt\nEU,3\nUS,5\n").expect("write csv");
    let mut h = Harness::new(
        Host,
        HostProps {
            cli_paths: vec![csv],
            deps,
        },
    );
    assert!(
        pump(&mut h, |h| !text(h, "cell-0-0").is_empty()),
        "no grid painted"
    );
    h.click(&format!("do-{}", ids::CONSOLE_TOGGLE));
    assert!(
        pump(&mut h, |h| has(h, "console-toolbar")),
        "the console opens"
    );
    h
}

#[test]
#[serial]
fn a_statement_asked_for_is_streamed_taken_into_a_tab_and_runs() {
    let rt = runtime();
    let _guard = rt.enter();
    let script = script(&["Here:\n```sql\nSELECT 4", "2 AS answer\n```"], true);
    let mut h = window("asked", deps(&script, true));
    assert!(!disabled(&h, "console-nl2sql"), "AI is ready");
    assert!(
        text(&h, "sidebar").contains("ai anthropic"),
        "the status names the provider: {:?}",
        text(&h, "sidebar")
    );

    h.click("console-nl2sql");
    assert!(pump(&mut h, |h| has(h, "name-prompt")), "it asks what for");
    type_into(&mut h, "name-prompt-field", "the answer");
    h.click("name-prompt-ok");
    assert!(
        pump(&mut h, |h| has(h, "console-stream-insert")),
        "the answer arrives: {:?}",
        text(&h, "console-stream")
    );
    assert!(text(&h, "console-stream-text").contains("SELECT 42 AS answer"));

    // Asked with the window's tables, by name and type, and no rows.
    let asked = script.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].prompt.contains("the answer"));
    assert!(
        asked[0].schema.tables.iter().any(|t| t.name == "sales"),
        "the schema names the window's tables"
    );
    assert!(asked[0].sample_rows.is_none(), "never a row");

    h.click("console-stream-insert");
    h.settle();
    assert!(!has(&h, "console-stream"), "the strip closes");
    h.click(&format!("do-{}", ids::SQL_RUN));
    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "42"),
        "the statement taken into its own tab runs: {:?}",
        text(&h, "cell-0-0")
    );
}

#[test]
#[serial]
fn a_statement_is_explained_and_the_explanation_closes() {
    let rt = runtime();
    let _guard = rt.enter();
    let script = script(&["It returns ", "the answer."], true);
    let mut h = window("explained", deps(&script, true));
    assert!(
        disabled(&h, "console-explain"),
        "nothing to explain in an empty tab"
    );

    // A statement to explain: the one NL→SQL brought, taken into a tab.
    h.click("console-nl2sql");
    assert!(pump(&mut h, |h| has(h, "name-prompt")));
    type_into(&mut h, "name-prompt-field", "anything");
    h.click("name-prompt-ok");
    assert!(pump(&mut h, |h| has(h, "console-stream-insert")));
    h.click("console-stream-insert");
    assert!(pump(&mut h, |h| !disabled(h, "console-explain")));

    h.click("console-explain");
    assert!(
        pump(&mut h, |h| has(h, "console-stream-close")),
        "{:?}",
        text(&h, "console-stream")
    );
    assert_eq!(text(&h, "console-stream-text"), "It returns the answer.");
    let asked = script.asked.lock().unwrap().clone();
    assert!(
        asked[1].prompt.contains("It returns the answer."),
        "Explain sends the tab's statement: {:?}",
        asked[1].prompt
    );
    h.click("console-stream-close");
    h.settle();
    assert!(!has(&h, "console-stream"));
}

#[test]
#[serial]
fn an_answer_can_be_stopped_and_kept_or_thrown_away() {
    let rt = runtime();
    let _guard = rt.enter();
    let script = script(&["SELECT 1"], false);
    let mut h = window("stopped", deps(&script, true));

    h.click("console-nl2sql");
    assert!(pump(&mut h, |h| has(h, "name-prompt")));
    type_into(&mut h, "name-prompt-field", "one");
    h.click("name-prompt-ok");
    assert!(
        pump(&mut h, |h| has(h, "console-stream-stop")
            && text(h, "console-stream-text").contains("SELECT 1")),
        "still arriving"
    );
    assert!(disabled(&h, "console-nl2sql"), "one answer at a time");

    h.click("console-stream-stop");
    assert!(
        pump(&mut h, |h| has(h, "console-stream-discard")),
        "what came is kept, to take or throw away"
    );
    assert_eq!(text(&h, "console-stream-text"), "SELECT 1");
    h.click("console-stream-discard");
    h.settle();
    assert!(!has(&h, "console-stream"));
    assert!(!disabled(&h, "console-nl2sql"));
}

#[test]
#[serial]
fn with_ai_off_neither_is_offered() {
    let rt = runtime();
    let _guard = rt.enter();
    let script = script(&[], true);
    let h = window("off", deps(&script, false));
    assert!(disabled(&h, "console-nl2sql"));
    assert!(disabled(&h, "console-explain"));
    assert!(text(&h, "sidebar").contains("ai none"));
    assert!(script.asked.lock().unwrap().is_empty());
    let _ = t("sql.nl2sql.chip");
}
