//! The BYOK AI panel as a navigable, operable surface.
//!
//! Ported from `dat0-app`'s `tests/ai_nav.rs`, whose subject was a dock in the
//! left panel. The dock is gone (S1); the panel now lives in the shell's single
//! modal slot, reached from the sidebar's CONNECTIONS section and from
//! Settings' "AI" pane. What that test actually *proved* survives the move:
//!
//! * every control the panel offers is a real, reachable, operable button,
//!   painted in a fixed order, carrying the field ids the rest of the app
//!   names;
//! * activating one changes the draft **and** reaches `settings.toml`;
//! * none of them exists while the panel is closed;
//! * the Forget button is live only when there is a key to forget.
//!
//! Two guarantees ride along because they are what the panel is *for*, and
//! because a headless harness can prove them where a windowed one could not:
//!
//! * **Both monotonic supersede counters.** `test_load_id` is bumped by every
//!   configuration change, so a probe issued against the old configuration can
//!   never report about the new one. `stream_load_id` is one counter shared by
//!   NL→SQL and Explain, because both own the same preview strip.
//! * **R17: no row data leaves.** Neither stream sends sample rows, the schema
//!   is names and types only, and the engine's surrogate column is dropped —
//!   even with `include_sample_rows` switched on, which is the flag most likely
//!   to be read as permission.
//!
//! What is *not* here, and why: `console_ai_triggers_reachable`,
//! `enter_on_explain_emits_explain` and
//! `console_ai_triggers_not_tab_stops_when_not_ready` walked the GPUI SQL
//! console's `nl2sql-chip` and `sql-explain` buttons. That console was rebuilt
//! (Phase 4) and has no such controls; the guarantee that survived the rebuild
//! is that the two stream kinds exist and share one guard, which
//! [`the_two_stream_kinds_share_one_guard`] proves directly instead of through
//! a button that no longer exists.
//!
//! Tab *order* is likewise not asserted as a focus walk: every control here is
//! a native `<button>`, so the browser's tab order is document order, and the
//! harness has no browser. Document order is asserted instead — it is the
//! thing the port could actually get wrong.

// `AiDeps` declares `probe`/`keys` as `Arc<dyn ..>`, and these scripted
// fixtures hold `RefCell`, so the Arc is not Sync. The harness is
// single-threaded and the type is the production API's, not a choice made
// here - satisfying the lint would mean changing `AiDeps`.
#![allow(clippy::arc_with_non_send_sync)]

mod support;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;

use dioxus::prelude::*;
use futures::channel::oneshot;

use dat0_core::ai::key_store::{KeyStore, MemoryKeyStore};
use dat0_core::ai::settings::AiSettings;
use dat0_core::ai::transport::TestOutcome;
use dat0_core::ai::{AiRequest, Provider};
use dat0_core::settings::Settings;
use dat0_core::settings::store::SettingsStore;
use dat0_engine::transform::ROWID_COL;
use dat0_engine::types::{ColumnInfo, DerivedOrigin, TableInfo, TableOrigin};

use dat0_ui::components::ai::{
    AiController, AiDeps, AiFuture, AiPanel, AiProbe, StreamKind, StreamPhase,
};
use dat0_ui::components::modals::ModalHost;
use dat0_ui::state::{Modal, Workspace};

use support::Harness;

/// Every control the panel paints, in paint order, with the i18n key of its
/// accessible name in the panel's default (nothing switched on) state.
///
/// This list is the field-id contract the GPUI suite asserted by Tab-walking
/// labels. Anything that renames an id or reorders the panel fails here.
const CONTROLS: [(&str, &str); 8] = [
    ("ai-toggle-enabled", "ai.enabled.off"),
    ("ai-provider-cycle", "ai.provider"),
    ("ai-key-set", "ai.key.set_button"),
    ("ai-key-forget", "ai.key.forget"),
    ("ai-model-set", "ai.model.set_button"),
    ("ai-toggle-advanced", "ai.advanced.off"),
    ("ai-toggle-sample-rows", "ai.sample_rows.off"),
    ("ai-test-connection", "ai.test"),
];

// ─────────────────────────────────────────────────────────────────────────────
// harness
// ─────────────────────────────────────────────────────────────────────────────

/// A probe the test drives by hand, and which records the request it was given.
///
/// The recording is what makes R17 testable: the guarantee is about the bytes
/// handed to the transport, and only the seam that would send them can see it.
#[derive(Default)]
struct ScriptedProbe {
    tests: RefCell<VecDeque<oneshot::Receiver<(bool, String)>>>,
    #[allow(clippy::type_complexity)]
    streams: RefCell<VecDeque<oneshot::Receiver<(Vec<String>, Option<String>)>>>,
    /// Every request the panel asked to be sent, in order.
    sent: RefCell<Vec<AiRequest>>,
}

impl ScriptedProbe {
    fn queue_test(&self) -> oneshot::Sender<(bool, String)> {
        let (tx, rx) = oneshot::channel();
        self.tests.borrow_mut().push_back(rx);
        tx
    }

    #[allow(clippy::type_complexity)]
    fn queue_stream(&self) -> oneshot::Sender<(Vec<String>, Option<String>)> {
        let (tx, rx) = oneshot::channel();
        self.streams.borrow_mut().push_back(rx);
        tx
    }
}

impl AiProbe for ScriptedProbe {
    fn test_connection(
        &self,
        _provider: Provider,
        _key: String,
        _cfg: AiSettings,
    ) -> AiFuture<TestOutcome> {
        let rx = self.tests.borrow_mut().pop_front();
        Box::pin(async move {
            match rx {
                Some(rx) => match rx.await {
                    Ok((ok, message)) => TestOutcome { ok, message },
                    Err(_) => TestOutcome {
                        ok: false,
                        message: "dropped".into(),
                    },
                },
                None => TestOutcome {
                    ok: false,
                    message: "unscripted".into(),
                },
            }
        })
    }

    fn stream(
        &self,
        _provider: Provider,
        _key: String,
        _cfg: AiSettings,
        req: AiRequest,
        mut on_delta: Box<dyn FnMut(String)>,
    ) -> AiFuture<Result<String, String>> {
        self.sent.borrow_mut().push(req);
        let rx = self.streams.borrow_mut().pop_front();
        Box::pin(async move {
            let Some(rx) = rx else {
                return Err("unscripted".into());
            };
            match rx.await {
                Ok((deltas, err)) => {
                    let mut full = String::new();
                    for d in deltas {
                        full.push_str(&d);
                        on_delta(d);
                    }
                    match err {
                        Some(e) => Err(e),
                        None => Ok(full),
                    }
                }
                Err(_) => Err("dropped".into()),
            }
        })
    }
}

/// A catalog with a surrogate column in it, which R17 says must not travel.
fn catalog() -> Vec<TableInfo> {
    vec![TableInfo {
        name: "sales".into(),
        schema: "main".into(),
        columns: vec![
            ColumnInfo {
                name: ROWID_COL.into(),
                data_type: "BIGINT".into(),
                nullable: false,
            },
            ColumnInfo {
                name: "region".into(),
                data_type: "VARCHAR".into(),
                nullable: true,
            },
            ColumnInfo {
                name: "amount".into(),
                data_type: "DOUBLE".into(),
                nullable: true,
            },
        ],
        row_count_estimate: Some(3),
        origin: TableOrigin::Derived(DerivedOrigin::Sql(String::new())),
    }]
}

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    deps: AiDeps,
}

/// The panel plus a readback of the state a DOM query cannot see, and two
/// buttons that start the streams the console used to start.
#[component]
fn Host(props: HostProps) -> Element {
    let ctl = AiController::use_new(props.deps.clone());
    let nl = ctl.clone();
    let explain = ctl.clone();
    let test_id = (ctl.state.test_load_id)();
    let stream_id = (ctl.state.stream_load_id)();
    let phase = ctl.state.stream.read().phase;
    let ready = ctl.ready();
    rsx! {
        AiPanel { controller: ctl.clone() }
        button {
            "data-a11y-id": "t-nl2sql",
            onclick: move |_| {
                nl.spawn_stream(StreamKind::NlToSql, "count the rows".into(), &catalog());
            },
            "nl2sql"
        }
        button {
            "data-a11y-id": "t-explain",
            onclick: move |_| {
                explain.spawn_stream(StreamKind::Explain, "SELECT 1".into(), &catalog());
            },
            "explain"
        }
        div { "data-a11y-id": "t-test-id", "{test_id}" }
        div { "data-a11y-id": "t-stream-id", "{stream_id}" }
        div { "data-a11y-id": "t-phase", "{phase.id()}" }
        div { "data-a11y-id": "t-ready", "{ready}" }
    }
}

/// Settings a configured panel hydrates from: OpenRouter, a stored key, a
/// model. `privacy_ack` is pre-set so enabling AI does not push a banner into
/// the middle of an assertion.
fn configured() -> Settings {
    let mut s = Settings::default();
    s.ai.provider = Some("openrouter".into());
    s.ai.model = "vendor/model".into();
    s.ai.privacy_ack = true;
    s
}

fn mount(settings: Settings, key: bool) -> (Harness, Arc<ScriptedProbe>, Arc<SettingsStore>) {
    let store = Arc::new(SettingsStore::open_in_memory());
    store.save(&settings).unwrap();
    let keys = Arc::new(MemoryKeyStore::default());
    if key {
        keys.set(Provider::OpenRouter, "sk-or-seeded").unwrap();
    }
    let probe = Arc::new(ScriptedProbe::default());
    let deps = AiDeps {
        store: Arc::clone(&store),
        keys,
        probe: Arc::clone(&probe) as Arc<dyn AiProbe>,
    };
    (Harness::new(Host, HostProps { deps }), probe, store)
}

/// The default: configured, with a key.
fn panel() -> (Harness, Arc<ScriptedProbe>, Arc<SettingsStore>) {
    mount(configured(), true)
}

fn read(h: &Harness, id: &str) -> String {
    h.text_of(h.by_a11y_id(id).unwrap_or_else(|| panic!("no {id}")))
}

fn test_id(h: &Harness) -> u64 {
    read(h, "t-test-id").parse().unwrap()
}

/// Position of each id in document order. Panics if one is missing, which is
/// the same failure a Tab walk that never reached it used to report.
fn positions(h: &Harness, ids: &[&str]) -> Vec<usize> {
    let walk = h.dom().walk();
    ids.iter()
        .map(|id| {
            walk.iter()
                .position(|k| h.dom().get(*k).attr("data-a11y-id") == Some(*id))
                .unwrap_or_else(|| panic!("the panel never painted {id}"))
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// reachability and operability
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn every_control_is_a_real_button_painted_in_a_fixed_order() {
    let (h, _probe, _store) = panel();

    for (id, _) in CONTROLS {
        let key = h
            .by_a11y_id(id)
            .unwrap_or_else(|| panic!("the panel never painted {id}"));
        assert_eq!(
            h.dom().get(key).tag(),
            Some("button"),
            "{id} must be a native <button>: that is what makes it a tab stop \
             and Enter/Space-operable without hand-rolling either"
        );
        assert_eq!(h.attr(key, "role").as_deref(), Some("button"), "{id}");
        assert!(h.has_listener(key, "click"), "{id} has no click handler");
    }

    let ids: Vec<&str> = CONTROLS.iter().map(|(id, _)| *id).collect();
    let at = positions(&h, &ids);
    assert!(
        at.windows(2).all(|w| w[0] < w[1]),
        "the controls must paint in the declared order; got {at:?} for {ids:?}"
    );
}

#[test]
fn every_control_carries_the_accessible_name_it_is_named_by() {
    // The GPUI suite reached these by their rendered label; the port keeps the
    // labels so a screen reader — and the palette's search — still find them.
    let (h, _probe, _store) = panel();
    for (id, key) in CONTROLS {
        let node = h.by_a11y_id(id).unwrap();
        let label = h.attr(node, "aria-label").unwrap_or_default();
        if key == "ai.provider" {
            // Interpolated: "Provider: OpenRouter".
            assert!(
                label.starts_with(&dat0_i18n::t("ai.provider")) && label.contains("OpenRouter"),
                "{id} label was {label:?}"
            );
        } else {
            assert_eq!(label, dat0_i18n::t(key), "{id}");
        }
        assert_ne!(label, key, "{id} echoed its i18n key instead of resolving");
    }
}

#[test]
fn the_enabled_toggle_flips_the_draft_and_reaches_settings_toml() {
    // The GPUI T0 gate's probe 2, minus the keyboard: a native button needs no
    // proof that Enter activates it, but it does need proof that activating it
    // changes both the thing on screen and the thing on disk.
    let (mut h, _probe, store) = panel();
    assert!(!store.load_or_default().unwrap().ai.enabled);
    assert!(h.has_label(&dat0_i18n::t("ai.enabled.off")));

    h.click("ai-toggle-enabled");
    assert!(
        store.load_or_default().unwrap().ai.enabled,
        "the click must reach settings.toml, not just the draft"
    );
    assert!(h.has_label(&dat0_i18n::t("ai.enabled.on")));

    h.click("ai-toggle-enabled");
    assert!(!store.load_or_default().unwrap().ai.enabled);
    assert!(h.has_label(&dat0_i18n::t("ai.enabled.off")));
}

#[test]
fn the_forget_button_is_live_only_while_a_key_is_stored() {
    // GPUI unmounted this button when there was nothing to forget, which meant
    // the control a keyboard user had just activated stopped existing under
    // them. It stays mounted and `aria-disabled` instead — but a disabled
    // control must still be inert, or the affordance is a lie.
    let (mut h, _probe, _store) = panel();
    let forget = h.by_a11y_id("ai-key-forget").unwrap();
    assert_eq!(h.attr(forget, "aria-disabled").as_deref(), Some("false"));
    assert_eq!(read(&h, "ai-key-state"), dat0_i18n::t("ai.key.set"));

    h.click("ai-key-forget");
    assert_eq!(read(&h, "ai-key-state"), dat0_i18n::t("ai.key.unset"));
    let forget = h
        .by_a11y_id("ai-key-forget")
        .expect("the button must survive its own click");
    assert_eq!(h.attr(forget, "aria-disabled").as_deref(), Some("true"));

    // Clicking it again does nothing at all — no state change, no panic.
    let before = test_id(&h);
    h.click("ai-key-forget");
    assert_eq!(read(&h, "ai-key-state"), dat0_i18n::t("ai.key.unset"));
    assert_eq!(
        test_id(&h),
        before,
        "an inert control must not even invalidate a probe"
    );
}

#[test]
fn a_panel_with_no_key_offers_forget_as_inert_rather_than_absent() {
    let (h, _probe, _store) = mount(configured(), false);
    assert_eq!(read(&h, "ai-key-state"), dat0_i18n::t("ai.key.unset"));
    let forget = h.by_a11y_id("ai-key-forget").expect("still painted");
    assert_eq!(h.attr(forget, "aria-disabled").as_deref(), Some("true"));
    assert_eq!(read(&h, "t-ready"), "false");
}

#[test]
fn the_egress_claim_is_always_on_screen_and_is_its_own_accessible_name() {
    // "no bytes left this machine" is a claim, and a hidden counter cannot make
    // it. It is announced as well as painted, because the whole point is that a
    // user can check it.
    let (h, _probe, _store) = panel();
    let node = h
        .by_a11y_id("ai-egress")
        .expect("the egress line is missing");
    let text = h.text_of(node);
    assert!(
        text.starts_with(&dat0_i18n::t("ai.egress")),
        "egress line read {text:?}"
    );
    assert_eq!(
        h.attr(node, "aria-label").as_deref(),
        Some(text.as_str()),
        "the figure must be announced, not merely painted"
    );
    assert_eq!(h.attr(node, "role").as_deref(), Some("note"));
}

// ─────────────────────────────────────────────────────────────────────────────
// the panel's home: the modal slot
// ─────────────────────────────────────────────────────────────────────────────

thread_local! {
    /// Deps for the modal host's controller. A thread-local because
    /// `ModalHost` takes no props and the controller has to be built inside a
    /// component scope.
    static MODAL_DEPS: RefCell<Option<AiDeps>> = const { RefCell::new(None) };
}

/// The shell, near enough: the slot, the host, and a way to empty it.
#[component]
fn SlotHost() -> Element {
    let ws = Workspace::provide();
    let deps = MODAL_DEPS
        .with(|d| d.borrow().clone())
        .expect("deps seeded");
    let ctl = AiController::use_new(deps);
    let seed = ctl.clone();
    use_hook(move || {
        let mut ws = ws;
        ws.modal.set(Some(Modal::Ai { controller: seed }));
    });

    rsx! {
        div { "data-a11y-id": "shell",
            button {
                "data-a11y-id": "close-slot",
                onclick: move |_| {
                    let mut ws = ws;
                    ws.modal.set(None);
                },
                "close"
            }
            ModalHost {}
        }
    }
}

fn slot() -> Harness {
    let store = Arc::new(SettingsStore::open_in_memory());
    store.save(&configured()).unwrap();
    let keys = Arc::new(MemoryKeyStore::default());
    keys.set(Provider::OpenRouter, "sk-or-seeded").unwrap();
    MODAL_DEPS.with(|d| {
        *d.borrow_mut() = Some(AiDeps {
            store,
            keys,
            probe: Arc::new(ScriptedProbe::default()),
        })
    });
    Harness::new(SlotHost, ())
}

#[test]
fn the_panel_is_reached_through_the_single_modal_slot() {
    // S1 deleted the left dock the panel used to live in. It is a modal now,
    // opened from the sidebar's CONNECTIONS section and from Settings' AI pane.
    let h = slot();
    assert!(
        h.by_a11y_id("ai-panel").is_some(),
        "the AI panel is not in the slot"
    );
    assert!(h.by_a11y_id("modal").is_some(), "no dialog around it");
    assert!(
        h.text_of(h.by_a11y_id("modal").unwrap())
            .contains(&dat0_i18n::t("ai.title")),
        "the dialog must be titled as the AI panel"
    );
    for (id, _) in CONTROLS {
        assert!(
            h.by_a11y_id(id).is_some(),
            "{id} did not survive the move into the slot"
        );
    }
}

#[test]
fn no_ai_control_exists_while_the_panel_is_closed() {
    // The GPUI original walked Tab 40 hops with the dock shut and asserted no
    // AI label was ever focused. Here the stronger statement holds: with the
    // slot empty the controls are not in the tree at all, so there is nothing
    // for Tab, a screen reader or a click to reach.
    let mut h = slot();
    h.click("close-slot");
    assert!(h.by_a11y_id("ai-panel").is_none());
    for (id, key) in CONTROLS {
        assert!(h.by_a11y_id(id).is_none(), "{id} outlived the panel");
        assert!(
            !h.has_label(&dat0_i18n::t(key)),
            "{id}'s accessible name outlived the panel"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// supersede counter 1: the Test-connection probe
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn every_configuration_change_invalidates_an_in_flight_probe() {
    // One bump per action, all six of them. A probe answers about the
    // configuration it was issued against; anything that changes that
    // configuration makes the answer a statement about the past.
    let actions = [
        "ai-toggle-enabled",
        "ai-provider-cycle",
        "ai-key-set",
        "ai-key-forget",
        "ai-model-set",
        "ai-toggle-advanced",
        "ai-toggle-sample-rows",
    ];
    for action in actions {
        let (mut h, _probe, _store) = panel();
        let before = test_id(&h);
        h.click(action);
        assert_ne!(
            test_id(&h),
            before,
            "{action} left an in-flight probe believable"
        );
    }
}

#[test]
fn a_probe_answering_after_a_configuration_change_is_discarded() {
    let (mut h, probe, _store) = panel();
    let gate = probe.queue_test();

    h.click("ai-test-connection");
    assert!(h.by_a11y_id("ai-test-result").is_none(), "still in flight");

    h.click("ai-toggle-sample-rows");

    gate.send((true, "Connected".into())).unwrap();
    h.settle();
    assert!(
        h.by_a11y_id("ai-test-result").is_none(),
        "a result about the previous configuration was shown as if current"
    );

    // …and the panel is not wedged: the next probe reports normally.
    let gate = probe.queue_test();
    h.click("ai-test-connection");
    gate.send((true, "Connected".into())).unwrap();
    h.settle();
    assert_eq!(read(&h, "ai-test-result"), "✓ Connected");
}

// ─────────────────────────────────────────────────────────────────────────────
// supersede counter 2: the stream
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_two_stream_kinds_share_one_guard() {
    // NL→SQL and Explain own the same preview strip, so only one may be
    // running: starting Explain must supersede an NL→SQL that is still out,
    // and the abandoned one must not be able to paint into the strip Explain
    // now owns. This is the guarantee the console's two AI triggers used to
    // stand for.
    let (mut h, probe, _store) = panel();
    let nl = probe.queue_stream();
    let explain = probe.queue_stream();

    h.click("t-nl2sql");
    let first: u64 = read(&h, "t-stream-id").parse().unwrap();
    h.click("t-explain");
    let second: u64 = read(&h, "t-stream-id").parse().unwrap();
    assert_ne!(first, second, "Explain must supersede a running NL→SQL");

    nl.send((vec!["STALE".into()], Some("stale failure".into())))
        .unwrap();
    h.settle();
    assert_eq!(
        read(&h, "t-phase"),
        StreamPhase::Streaming.id(),
        "the abandoned NL→SQL closed the strip Explain was using"
    );
    assert_eq!(read(&h, "ai-stream-text"), "");
    assert!(h.by_a11y_id("ai-stream-error").is_none());

    explain.send((vec!["a projection".into()], None)).unwrap();
    h.settle();
    assert_eq!(read(&h, "t-phase"), StreamPhase::Done.id());
    assert_eq!(read(&h, "ai-stream-text"), "a projection");
    assert_eq!(
        h.attr(h.by_a11y_id("ai-stream").unwrap(), "data-kind")
            .as_deref(),
        Some(StreamKind::Explain.id()),
        "the strip must name the stream that owns it"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// R17
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn no_stream_ever_sends_row_data() {
    // R17. Both kinds, because one of them getting it right is not the claim.
    let (mut h, probe, _store) = panel();
    let _nl = probe.queue_stream();
    h.click("t-nl2sql");
    let _ex = probe.queue_stream();
    h.click("t-explain");

    let sent = probe.sent.borrow();
    assert_eq!(
        sent.len(),
        2,
        "both streams must have reached the transport"
    );
    for req in sent.iter() {
        assert!(
            req.sample_rows.is_none(),
            "a request carried sample rows: {:?}",
            req.sample_rows
        );
    }
}

#[test]
fn switching_include_sample_rows_on_does_not_send_any() {
    // The setting exists (it is persisted and it is on the panel), and it is
    // the single most plausible route to a leak: a flag named
    // `include_sample_rows` reads as permission. It is not — the request
    // builder has no branch on it.
    let mut settings = configured();
    settings.ai.include_sample_rows = true;
    let (mut h, probe, store) = mount(settings, true);
    assert!(
        store.load_or_default().unwrap().ai.include_sample_rows,
        "precondition: the flag really is on"
    );
    assert!(h.has_label(&dat0_i18n::t("ai.sample_rows.on")));

    let _gate = probe.queue_stream();
    h.click("t-nl2sql");

    let sent = probe.sent.borrow();
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].sample_rows.is_none(),
        "include_sample_rows opened a door it must not"
    );
}

#[test]
fn the_schema_that_travels_is_names_and_types_without_the_surrogate() {
    let (mut h, probe, _store) = panel();
    let _gate = probe.queue_stream();
    h.click("t-nl2sql");

    let sent = probe.sent.borrow();
    let schema = &sent[0].schema;
    assert_eq!(schema.tables.len(), 1);
    let table = &schema.tables[0];
    assert_eq!(table.name, "sales");
    let cols: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        cols,
        ["region", "amount"],
        "the engine's surrogate row-id must never reach a model"
    );
    assert!(
        !cols.contains(&ROWID_COL),
        "{ROWID_COL} travelled in the schema"
    );
    let types: Vec<&str> = table.columns.iter().map(|c| c.ty.as_str()).collect();
    assert_eq!(types, ["VARCHAR", "DOUBLE"], "types travel, values do not");

    // And the rendered block a model actually reads holds no value either.
    let rendered = schema.render();
    assert!(
        rendered.contains("sales(region VARCHAR, amount DOUBLE)"),
        "{rendered}"
    );
    assert!(!rendered.contains(ROWID_COL), "{rendered}");
}
