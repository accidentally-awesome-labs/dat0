//! Typing an AI key and model (step 5.6a).
//!
//! The AI panel's Save key and Save model wrote their request to an outbox
//! that nothing read, so no key could be typed and AI could never become ready
//! (PD-023). These tests mount the real shell with an in-memory key store and a
//! probe that never reaches the network, open the panel as the menu does, and
//! type a key and a model the way a user does.

mod support;

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
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

const COMMANDS: &[&str] = &[ids::AI_PANEL_OPEN];

/// A probe that answers without the network.
struct Offline;

impl AiProbe for Offline {
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
        _: AiRequest,
        _: Box<dyn FnMut(String)>,
    ) -> AiFuture<Result<String, String>> {
        Box::pin(async { Err("offline".to_string()) })
    }
}

/// The window's AI dependencies, held here to read back what the panel wrote.
#[derive(Clone)]
struct Held {
    keys: Arc<MemoryKeyStore>,
    store: Arc<SettingsStore>,
    /// The keychain would not open, so keys are held in memory.
    volatile: bool,
}

impl PartialEq for Held {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.keys, &other.keys)
    }
}

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    held: Held,
}

/// `App`'s wiring, minus what needs a desktop window, with the AI's
/// dependencies provided the way a test must: never the OS keychain.
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
    let held = props.held.clone();
    use_context_provider(move || Deps {
        ai: AiDeps {
            store: held.store.clone(),
            keys: held.keys.clone(),
            probe: Arc::new(Offline),
        },
        volatile: held.volatile,
    });
    session_boot::use_session(ws, Vec::new());
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

fn type_into(h: &mut Harness, id: &str, value: &str) {
    let field = h.by_a11y_id(id).expect("the field");
    let typed = dioxus::html::SerializedFormData::new(value.to_string(), Vec::new());
    h.dispatch(field, "input", typed);
    h.settle();
}

/// A window with the AI panel open and Anthropic chosen.
fn panel() -> (Harness, Held) {
    panel_with(false)
}

fn panel_with(volatile: bool) -> (Harness, Held) {
    let _ = STATE_ROOT.as_path();
    let held = Held {
        keys: Arc::new(MemoryKeyStore::default()),
        store: Arc::new(SettingsStore::open_in_memory()),
        volatile,
    };
    let mut h = Harness::new(Host, HostProps { held: held.clone() });
    h.settle();
    h.click(&format!("do-{}", ids::AI_PANEL_OPEN));
    assert!(pump(&mut h, |h| has(h, "ai-panel")), "the AI panel opens");
    h.click("ai-provider-cycle");
    h.settle();
    (h, held)
}

#[test]
#[serial]
fn a_key_typed_into_its_prompt_is_kept_and_the_panel_comes_back() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, held) = panel();
    assert_eq!(text(&h, "ai-key-state"), t("ai.key.unset"));

    h.click("ai-key-set");
    assert!(
        pump(&mut h, |h| has(h, "name-prompt")),
        "Save key asks for the key"
    );
    let field = h.by_a11y_id("name-prompt-field").unwrap();
    assert_eq!(
        h.attr(field, "type").as_deref(),
        Some("password"),
        "a key is typed out of sight"
    );
    type_into(&mut h, "name-prompt-field", "  sk-test-123  ");
    h.click("name-prompt-ok");
    assert!(
        pump(&mut h, |h| has(h, "ai-panel") && !has(h, "name-prompt")),
        "the panel comes back"
    );
    assert_eq!(text(&h, "ai-key-state"), t("ai.key.set"));
    assert_eq!(
        held.keys.get(Provider::Anthropic).unwrap().as_deref(),
        Some("sk-test-123"),
        "kept in the key store, trimmed"
    );
    assert!(!h.html().contains("sk-test-123"), "never in the page");
}

#[test]
#[serial]
fn a_model_typed_into_its_prompt_is_saved_in_the_settings() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, held) = panel();

    h.click("ai-model-set");
    assert!(pump(&mut h, |h| has(h, "name-prompt")));
    let field = h.by_a11y_id("name-prompt-field").unwrap();
    assert_eq!(h.attr(field, "type").as_deref(), Some("text"));
    type_into(&mut h, "name-prompt-field", "claude-test");
    h.click("name-prompt-ok");
    assert!(pump(&mut h, |h| has(h, "ai-panel") && !has(h, "name-prompt")));
    assert!(
        h.html().contains("claude-test"),
        "the panel shows the model"
    );
    assert_eq!(
        held.store.load_or_default().unwrap().ai.model,
        "claude-test"
    );
}

#[test]
#[serial]
fn a_prompt_cancelled_changes_nothing_and_the_panel_comes_back() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, held) = panel();

    h.click("ai-key-set");
    assert!(pump(&mut h, |h| has(h, "name-prompt")));
    type_into(&mut h, "name-prompt-field", "sk-not-this");
    h.click("name-prompt-cancel");
    assert!(pump(&mut h, |h| has(h, "ai-panel") && !has(h, "name-prompt")));
    assert_eq!(text(&h, "ai-key-state"), t("ai.key.unset"));
    assert_eq!(held.keys.get(Provider::Anthropic).unwrap(), None);
}

#[test]
#[serial]
fn a_key_that_cannot_reach_the_keychain_says_it_will_not_last() {
    let rt = runtime();
    let _guard = rt.enter();
    let (mut h, held) = panel_with(true);

    h.click("ai-key-set");
    assert!(pump(&mut h, |h| has(h, "name-prompt")));
    type_into(&mut h, "name-prompt-field", "sk-for-now");
    h.click("name-prompt-ok");
    let warned = t("ai.key.volatile");
    assert!(
        pump(&mut h, |h| text(h, "banner-host").contains(&warned)),
        "{:?}",
        text(&h, "banner-host")
    );
    assert_eq!(
        held.keys.get(Provider::Anthropic).unwrap().as_deref(),
        Some("sk-for-now")
    );
}
