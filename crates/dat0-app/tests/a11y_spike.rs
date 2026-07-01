//! T0 spike + finalized-API smoke for UAT Gap 2 — proves AccessKit capture
//! round-trips under the gpui test harness, that label-located clicks reach real
//! widgets, and that the content-only `.a11y_label` path is findable by text.
//!
//! Mirrors `onboarding_gpui.rs`'s windowed `#[gpui::test]` setup: open a real
//! `TestPlatform` window whose root is a `gpui_component::Root` wrapping a
//! `WorkspaceShell` over an EMPTY session (so it renders the first-run enriched
//! hero, where both the `hero-take-tour` button and the tagline live). We then:
//!   (a) snapshot the AccessKit tree the render emitted and find the "Take a
//!       tour" button BY LABEL (content assertion — the thing gpui can't do);
//!   (b) recover its static id from the click-id side-map, resolve painted
//!       geometry via `debug_bounds`, and fire a real `simulate_click`;
//!   (c) find the non-clickable tagline BY VALUE via `.a11y_label` (content-only
//!       proof — no click id, no debug_selector).
//!
//! The `KNode` newtype + snapshot/query/click combinators live in the shared
//! `support` module (hoisted there in Task 2, review Minor M2) so every surface
//! test (Tasks 3-8) reuses ONE copy.
//!
//! Feature note: the `a11y-capture` feature is auto-ON for this integration
//! test via the self-dev-dependency in Cargo.toml, so `dat0_app::a11y::*` are
//! the real capture symbols (not the release no-op stubs).
//!
//! Hermeticity: `DAT0_CONFIG_DIR` points at a fresh temp dir; `#[serial]`
//! because `set_var` is process-global and `#[gpui::test]` is multithreaded.

mod support;

use std::path::Path;
use std::sync::Arc;

use gpui::{AppContext as _, TestAppContext};
use gpui_component::Root;
use parking_lot::Mutex;
use serial_test::serial;

use dat0_app::a11y::AccessRole;
use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;
use support::A11ySnapshot;

const BUDGET: u64 = 128 * 1024 * 1024;

/// Build a real, EMPTY in-memory session on a dedicated tokio runtime (the gpui
/// test executor is not a tokio runtime, and `Session::new` uses
/// `spawn_blocking` internally). Mirrors `onboarding_gpui.rs::build_empty_session`.
fn build_empty_session(state_root: &Path) -> Arc<Mutex<Session>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let sess = rt
        .block_on(Session::new(state_root, BUDGET))
        .expect("Session::new");
    Arc::new(Mutex::new(sess))
}

#[gpui::test]
#[serial]
fn a11y_capture_round_trips_and_click_by_label_opens_tour(cx: &mut TestAppContext) {
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    // SAFETY: `#[serial]` — no other thread races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", cfg.path()) };
    // first_run_done unset (false) → enriched band renders → `hero-take-tour`
    // is painted AND emits its AccessKit node.
    cx.update(gpui_component::init);

    let session = build_empty_session(state.path());
    let (_root, vcx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        let shell = cx.new(|c| WorkspaceShell::new(session, c));
        Root::new(shell, window, cx)
    });
    vcx.run_until_parked();

    // (a) CONTENT: the hero "Take a tour" button is in the emitted tree, located
    //     BY ITS RENDERED LABEL (the exact i18n string the render used).
    let take_tour = dat0_app::dat0_i18n::t("hero.take_tour");
    let snap = A11ySnapshot::capture(vcx);
    // FRAME-BRACKET PROOF (Task-1 Step 8; recount Task 4 — Gap 2 sample-card
    // annotations): the enriched hero has exactly EIGHT capture sites — the
    // tagline (`.a11y_label`) + `hero-take-tour` (`.a11y`) from Task 1, PLUS
    // the 3 sample cards' `.a11y` (Button container) + `.a11y_label` (title
    // text) pairs added in Task 4 (`sample_column` renders because this
    // session is empty ⇒ `recents_empty = true`): 2 + 3*2 = 8. If the forced
    // `refresh()` produced more than one render frame, the collector would
    // hold duplicate nodes and this count would exceed 8 (and the
    // `get_by_label` lookups below would panic with "Found two or more
    // nodes"). Exactly-8 (confirmed deterministic across repeated runs)
    // confirms the reset→refresh→run_until_parked→take bracket still yields
    // one clean frame — no generation counter needed.
    assert_eq!(
        snap.click_ids.len(),
        8,
        "expected exactly eight captured nodes (tagline + hero-take-tour + 3 sample cards \
         × {{.a11y button, .a11y_label title}}); a different count means the frame bracket \
         double- or under-rendered, or the sample-card annotation count changed"
    );

    // Content assertion (finalized API): findable by label, and by role+label.
    assert!(
        snap.has_label(&take_tour),
        "hero-take-tour must be findable by its rendered label"
    );
    assert!(
        snap.query_by_role(AccessRole::Button, &take_tour),
        "hero-take-tour must be findable as a Button with that label"
    );
    // Label lookup must round-trip to the static `debug_selector` id.
    assert_eq!(
        snap.click_id_for_label(&take_tour),
        Some("hero-take-tour"),
        "label lookup must round-trip to the static debug_selector id"
    );

    // (b) CLICK BY LABEL: locate → recover id → debug_bounds → real click. This
    //     proves the AccessKit node and the gpui hitbox stay in lockstep (same
    //     id) — no hand-tuned pixel constant. (Tour-open behaviour is re-asserted
    //     in the broader Task-3+ tests; this spike proves capture + label
    //     round-trip + bounds-resolution + click, the go/no-go.)
    snap.click(vcx, &take_tour);
    vcx.run_until_parked();

    drop(state);
}

#[gpui::test]
#[serial]
fn a11y_label_captures_static_text_for_content_assertion(cx: &mut TestAppContext) {
    // A view that renders a plain label via `.a11y_label` must be findable by
    // value/label even though it is NOT clickable (no click id, no
    // debug_selector). This proves the content-only path; full surface coverage
    // is Tasks 3-8.
    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    // SAFETY: `#[serial]` — no other thread races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", cfg.path()) };
    // first_run_done unset (false) → enriched band renders → the tagline is
    // painted AND emits its content-only AccessKit Label node.
    cx.update(gpui_component::init);

    let session = build_empty_session(state.path());
    let (_root, vcx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        let shell = cx.new(|c| WorkspaceShell::new(session, c));
        Root::new(shell, window, cx)
    });
    vcx.run_until_parked();

    let tagline = dat0_app::dat0_i18n::t("hero.tagline");
    let snap = A11ySnapshot::capture(vcx);

    // CONTENT-ONLY: the tagline is findable by its exact rendered text even
    // though it carries no click id (a `Role::Label` node whose text lives in
    // `value`, matched by the value-vs-label rule).
    assert!(
        snap.has_label(&tagline),
        "tagline must be findable by its rendered text via `.a11y_label`"
    );
    assert!(
        snap.query_by_role(AccessRole::Label, &tagline),
        "tagline must be findable as a Label with that text"
    );
    // Content-only ⇒ NOT clickable: no static id in the side-map.
    assert_eq!(
        snap.click_id_for_label(&tagline),
        None,
        "a `.a11y_label` node is content-only and must have no click id"
    );

    drop(state);
}
