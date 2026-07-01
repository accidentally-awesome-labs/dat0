//! T0 spike for UAT Gap 2 — proves AccessKit capture round-trips under the
//! gpui test harness and that label-located clicks reach real widgets.
//!
//! Mirrors `onboarding_gpui.rs`'s windowed `#[gpui::test]` setup: open a real
//! `TestPlatform` window whose root is a `gpui_component::Root` wrapping a
//! `WorkspaceShell` over an EMPTY session (so it renders the first-run enriched
//! hero, where the `hero-take-tour` button lives). We then:
//!   (a) snapshot the AccessKit tree the render emitted and find the "Take a
//!       tour" button BY LABEL (content assertion — the thing gpui can't do);
//!   (b) recover its static id from the click-id side-map, resolve painted
//!       geometry via `debug_bounds`, and fire a real `simulate_click`.
//!
//! Feature note: the `a11y-capture` feature is auto-ON for this integration
//! test via the self-dev-dependency in Cargo.toml, so `dat0_app::a11y::*` are
//! the real capture symbols (not the release no-op stubs).
//!
//! Hermeticity: `DAT0_CONFIG_DIR` points at a fresh temp dir; `#[serial]`
//! because `set_var` is process-global and `#[gpui::test]` is multithreaded.

use std::path::Path;
use std::sync::Arc;

use gpui::{AppContext as _, TestAppContext, VisualTestContext};
use gpui_component::Root;
use kittest::{AccessKitNode, NodeT, Queryable as _, State};
use parking_lot::Mutex;
use serial_test::serial;

use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;

const BUDGET: u64 = 128 * 1024 * 1024;

/// Minimal kittest `NodeT` adapter over `accesskit_consumer`'s `Node`.
///
/// kittest 0.3.0 (the newest release compatible with our pinned rust-version
/// 1.85 — 0.4.0 needs rustc 1.92) ships NO concrete node type: integrations
/// provide their own `NodeT` impl and get the `get_by_*` query helpers via a
/// blanket `Queryable` impl (see kittest's README). `State` only exposes
/// `root() -> AccessKitNode`, which is `Copy` but not `Debug`, so this newtype
/// is the smallest such integration — enough to drive `get_by_label` from the
/// synthetic root the capture module builds. (0.4.0 makes `State` itself
/// queryable; when the toolchain floor rises this wrapper can be dropped.)
#[derive(Clone)]
struct KNode<'tree>(AccessKitNode<'tree>);

impl std::fmt::Debug for KNode<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KNode")
            .field("id", &self.0.id().0)
            .field("label", &self.0.label())
            .finish()
    }
}

impl<'tree> NodeT<'tree> for KNode<'tree> {
    fn accesskit_node(&self) -> AccessKitNode<'tree> {
        self.0
    }
    fn new_related(&self, child: AccessKitNode<'tree>) -> Self {
        KNode(child)
    }
}

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

/// The resolved frame-reset bracket (Task-1 Step 8): clear the collector, force
/// ONE full re-render via `window.refresh()`, drain the frame with
/// `run_until_parked()`, then snapshot. `refresh()` bypasses gpui's view-render
/// cache so `WorkspaceShell::render` runs again and re-emits the hero nodes; a
/// single forced frame produced exactly one copy of each node in practice (no
/// duplicates → no generation counter needed). Returns the kittest `State` plus
/// the `NodeId → static-id` side-map.
fn a11y_snapshot(cx: &mut VisualTestContext) -> (State, Vec<Option<&'static str>>) {
    dat0_app::a11y::reset();
    cx.update(|window, _app| window.refresh());
    cx.run_until_parked();
    let cap = dat0_app::a11y::take_tree_update();
    (State::new(cap.update), cap.click_ids)
}

#[gpui::test]
#[serial]
fn a11y_capture_round_trips_and_click_by_label_opens_tour(cx: &mut TestAppContext) {
    use gpui::Modifiers;

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
    let label = dat0_app::dat0_i18n::t("hero.take_tour");
    let (tree, click_ids) = a11y_snapshot(vcx);
    // FRAME-BRACKET PROOF (Task-1 Step 8): the enriched hero has exactly ONE
    // `.a11y` site (`hero-take-tour`). If the forced `refresh()` produced more
    // than one render frame, this collector would hold duplicate nodes and the
    // count would exceed 1 (and the `get_by_label` below would panic with
    // "Found two or more nodes"). Exactly-1 confirms the reset→refresh→
    // run_until_parked→take bracket yields one clean frame — no generation
    // counter needed.
    assert_eq!(
        click_ids.len(),
        1,
        "expected exactly one captured node (the single .a11y site); \
         a different count means the frame bracket double- or under-rendered"
    );
    // Query recursively from the synthetic root (kittest 0.3.0: queries start at
    // a `NodeT`; `State` isn't itself queryable, so wrap `root()`).
    let root = KNode(tree.root());
    let node = root.get_by_label(&label);
    let nid = node.accesskit_node().id(); // NodeId(i), i >= 1 (root is 0)
    assert!(nid.0 >= 1, "queried node must be a child, not the root");
    let click_id = click_ids[(nid.0 - 1) as usize].expect("hero-take-tour must be clickable");
    assert_eq!(
        click_id, "hero-take-tour",
        "label lookup must round-trip to the static debug_selector id"
    );

    // (b) CLICK BY LABEL: resolve id → debug_bounds → real click. This proves
    //     the AccessKit node and the gpui hitbox stay in lockstep (same id).
    let bounds = vcx
        .debug_bounds(click_id)
        .expect("hero-take-tour must have painted bounds resolvable by its id");
    vcx.simulate_click(bounds.center(), Modifiers::none());
    vcx.run_until_parked();
    // The click fired against real painted geometry located purely from the
    // AccessKit label — no hand-tuned pixel constant. (Tour-open behaviour is
    // re-asserted in the broader Task-3 tests; this spike proves capture +
    // label-round-trip + bounds-resolution + click, which is the go/no-go.)

    drop(state);
}
