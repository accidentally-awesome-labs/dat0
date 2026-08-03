//! B7 T0 HARD GATE — throwaway spike. Answers the design's §12 probes before any
//! production code is written. Deleted at T6 once real tests cover the same
//! ground.
//!
//! B6's `dock_chrome_spike.rs` already settled that dock chrome is transparent to
//! the a11y capture and that A focus stop inside it stays Tab-reachable. B7's
//! question is the one B6 could not ask: its two panels had **zero** focus stops
//! between them, so `.tab_group()` had nothing to reorder. B7 moves NINE live
//! handles (`catalog-tree` plus eight `ai-*`) into one tab group at once.
//!
//! So the probe here carries MULTIPLE stops in a single docked panel and asks
//! whether Tab reaches all of them, in document order — the thing that would
//! actually break `catalog_nav` and `ai_nav`.
//!
//! The harness is adapted from `tests/dock_chrome_spike.rs` (synthetic probes,
//! `Root` wrapper, staged focus); §12's P4 and P5 use the real shell instead.
//!
//! Hermeticity: `#[serial]` because the capture collector is process-global.

mod support;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, TestAppContext, Window, div, px,
};
use gpui_component::dock::{DockArea, DockItem, Panel, PanelEvent};
use serial_test::serial;

use dat0_app::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
use dat0_app::theme::tokens::Dat0Theme as _;
use gpui_component::ActiveTheme as _;
use support::A11ySnapshot;

/// A docked probe panel carrying `stops` independently focusable stops, so a Tab
/// walk can tell "reaches the panel" from "reaches everything in the panel".
///
/// `visible` is a plain field the test flips through the entity, mirroring how
/// production reads a shell bool from `Panel::visible`.
struct MultiStopPanel {
    name: &'static str,
    stop_ids: Vec<&'static str>,
    handles: Vec<FocusHandle>,
    visible: bool,
}

impl MultiStopPanel {
    fn new(name: &'static str, stop_ids: Vec<&'static str>, cx: &mut Context<Self>) -> Self {
        let handles = stop_ids.iter().map(|_| cx.focus_handle()).collect();
        Self {
            name,
            stop_ids,
            handles,
            visible: true,
        }
    }
}

impl EventEmitter<PanelEvent> for MultiStopPanel {}

impl Focusable for MultiStopPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.handles
            .first()
            .cloned()
            .unwrap_or_else(|| cx.focus_handle())
    }
}

impl Panel for MultiStopPanel {
    fn panel_name(&self) -> &'static str {
        "MultiStopPanel"
    }

    /// Emits a capture node so P3 can tell a 30px TITLE ROW (one visible panel)
    /// from a TAB BAR (two or more): the title branch renders only the single
    /// visible panel's title, the tab bar renders every visible panel's.
    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let t = format!("title-{}", self.name);
        div()
            .a11y_label(AccessRole::Label, t.clone())
            .child(SharedString::from(t))
    }

    fn visible(&self, _cx: &App) -> bool {
        self.visible
    }
}

impl Render for MultiStopPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ring = cx.theme().d0().focus_ring;
        let mut root = div();
        for (i, id) in self.stop_ids.iter().enumerate() {
            let fh = self.handles[i].clone();
            // `.a11y`, not `.a11y_label`: the focus oracle is a two-stage join
            // (focused handle → static id → a captured node whose `click_id`
            // matches), so a content-only label sets focus correctly and is
            // unnameable (B6).
            root = root.child(
                div()
                    .a11y(id, AccessRole::Button, id.to_string())
                    .focus_stop(id, &fh, 0, ring, |_ev, _window, _app| {})
                    .child(SharedString::from(*id)),
            );
        }
        root
    }
}

struct DockHost {
    dock: Entity<DockArea>,
    left: Vec<Entity<MultiStopPanel>>,
    center: Entity<MultiStopPanel>,
    /// A focus stop OUTSIDE the DockArea entirely — the shell's own chrome, in
    /// production terms (the activity rail, the status bar, the hero). This is
    /// what makes "can Tab escape the dock's tab group" answerable: the center
    /// probe turned out not to register as a tab stop at all, so it could not
    /// serve as the outside reference.
    host_fh: FocusHandle,
}

impl Render for DockHost {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ring = cx.theme().d0().focus_ring;
        div()
            .size_full()
            .child(
                div()
                    .a11y("host-stop", AccessRole::Button, "host-stop".to_string())
                    .focus_stop("host-stop", &self.host_fh, 0, ring, |_ev, _w, _a| {})
                    .child("host-stop"),
            )
            .child(self.dock.clone())
    }
}

/// Mount a `DockArea` whose LEFT dock is one `DockItem::tabs` holding three
/// probe panels — B7's shape.
///
/// The `Root` wrapper is load-bearing: `Root` is what binds `tab`/`shift-tab` as
/// actions under key context `"Root"`, so a bare host measures the absence of
/// `Root` rather than the behaviour of dock chrome (B1/B6).
fn mount(
    cx: &mut TestAppContext,
) -> (
    Vec<Entity<MultiStopPanel>>,
    Entity<MultiStopPanel>,
    FocusHandle,
    &mut gpui::VisualTestContext,
) {
    cx.update(gpui_component::init);

    type Slot = Rc<
        RefCell<
            Option<(
                Vec<Entity<MultiStopPanel>>,
                Entity<MultiStopPanel>,
                FocusHandle,
            )>,
        >,
    >;
    let slot: Slot = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();

    let (_root, vcx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|cx| build_host(window, cx));
        {
            let h = host.read(cx);
            *slot_in.borrow_mut() = Some((h.left.clone(), h.center.clone(), h.host_fh.clone()));
        }
        gpui_component::Root::new(host, window, cx)
    });
    vcx.run_until_parked();

    let (panels, center, host_fh) = slot.borrow_mut().take().expect("host built");
    (panels, center, host_fh, vcx)
}

fn build_host(window: &mut Window, cx: &mut Context<DockHost>) -> DockHost {
    let dock = cx.new(|cx| {
        let mut dock = DockArea::new("b7-spike-dock", Some(1), window, cx);
        dock.set_locked(true, window, cx);
        dock
    });
    let weak_dock = dock.downgrade();

    // Center: a zero-chrome `DockItem::Panel` with one stop, so the Tab walk has
    // a staging point outside the dock's tab group.
    let center = cx.new(|cx| MultiStopPanel::new("center", vec!["center-stop"], cx));
    dock.update(cx, |dock, cx| {
        dock.set_center(DockItem::panel(Arc::new(center.clone())), window, cx);
    });

    // Left: THREE panels in ONE `DockItem::tabs`, the B7 shape. `a` carries three
    // stops — the multi-stop question B6 never asked.
    let a = cx.new(|cx| MultiStopPanel::new("a", vec!["a-stop-1", "a-stop-2", "a-stop-3"], cx));
    let b = cx.new(|cx| MultiStopPanel::new("b", vec!["b-stop-1"], cx));
    let c = cx.new(|cx| MultiStopPanel::new("c", vec!["c-stop-1"], cx));

    // Only `a` starts visible — the production invariant is at-most-one.
    b.update(cx, |p, _| p.visible = false);
    c.update(cx, |p, _| p.visible = false);

    let left = DockItem::tabs(
        vec![
            Arc::new(a.clone()),
            Arc::new(b.clone()),
            Arc::new(c.clone()),
        ],
        &weak_dock,
        window,
        cx,
    );
    dock.update(cx, |dock, cx| {
        dock.set_left_dock(left, Some(px(384.)), true, window, cx);
    });

    DockHost {
        dock,
        left: vec![a, b, c],
        center,
        host_fh: cx.focus_handle(),
    }
}

/// Stage focus on the center stop, then record the focused label after each Tab.
fn walk_tabs(vcx: &mut gpui::VisualTestContext, presses: usize) -> Vec<Option<String>> {
    let mut observed = vec![
        A11ySnapshot::capture(vcx)
            .focused_label()
            .map(str::to_string),
    ];
    for _ in 0..presses {
        support::press_tab(vcx);
        let snap = A11ySnapshot::capture(vcx);
        observed.push(snap.focused_label().map(str::to_string));
    }
    observed
}

// ---------------------------------------------------------------------------
// P3 — does exactly-one-visible render a TITLE ROW rather than a TAB BAR?
// ---------------------------------------------------------------------------

/// Upstream takes the title branch when `visible_panels.len() == 1 &&
/// panel_style == PanelStyle::default()` (`tab_panel.rs:623-625`). The rail's
/// whole model depends on this: two visible panels would paint a horizontal tab
/// bar directly beside the rail — two selectors for one choice.
#[gpui::test]
#[serial]
fn p3_one_visible_panel_renders_only_its_own_title(cx: &mut TestAppContext) {
    let (_panels, _center, _host_fh, vcx) = mount(cx);
    let snap = A11ySnapshot::capture(vcx);

    assert_eq!(
        snap.count_label("title-a"),
        1,
        "the one visible panel's title must appear exactly once"
    );
    assert_eq!(
        snap.count_label("title-b"),
        0,
        "a HIDDEN panel must contribute no title — a tab bar would list it"
    );
    assert_eq!(snap.count_label("title-c"), 0);
}

/// The negative control for P3: with two visible, upstream must switch to the
/// tab bar and BOTH titles appear. If this does not happen the P3 assertion
/// above proves nothing, because it would pass under either branch.
#[gpui::test]
#[serial]
fn p3_control_two_visible_panels_list_both_titles(cx: &mut TestAppContext) {
    let (panels, _center, _host_fh, vcx) = mount(cx);
    vcx.cx.update(|app| {
        panels[1].update(app, |p, _| p.visible = true);
    });
    vcx.update(|window, _app| window.refresh());
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert_eq!(
        snap.count_label("title-a") + snap.count_label("title-b"),
        2,
        "two visible panels must produce two titles (the tab-bar branch) — if \
         this is 1, the title-row branch is being taken with two visible and \
         the P3 measurement above is vacuous"
    );
}

// ---------------------------------------------------------------------------
// P1/P2 — do MULTIPLE focus stops inside one tab group all stay reachable?
// ---------------------------------------------------------------------------

/// THE B7 GATE. B6 proved a single focus stop inside dock chrome is reachable;
/// B7 moves nine at once into one `.tab_group()`, which B1 measured as REORDERING
/// traversal rather than containing it.
///
/// A failure here means `catalog_nav` (one container stop) or `ai_nav` (eight
/// per-button stops) cannot be made green under `DockItem::tabs`, and Task 4
/// must fall back to `DockItem::split` per design §3.
#[gpui::test]
#[serial]
fn p1_every_stop_in_a_docked_panel_stays_tab_reachable(cx: &mut TestAppContext) {
    let (panels, _center, _host_fh, vcx) = mount(cx);

    // Stage focus INSIDE the dock's first stop. With nothing focused the dispatch
    // path is the window root alone and Tab is completely inert (B1), so a walk
    // that does not stage focus measures nothing at all.
    vcx.update(|window, app| {
        let fh = panels[0].read(app).handles[0].clone();
        window.focus(&fh);
    });
    vcx.run_until_parked();

    let observed = walk_tabs(vcx, 6);
    println!("P1 tab walk = {observed:#?}");

    for want in ["a-stop-1", "a-stop-2", "a-stop-3"] {
        assert!(
            observed.iter().any(|f| f.as_deref() == Some(want)),
            "P1 FAILED: `{want}` is unreachable by Tab inside the dock's tab \
             group; walked {observed:?}"
        );
    }
}

/// Characterization, not preference: records the ORDER Tab visits the three
/// stops. `catalog_nav`/`ai_nav` assert on order, so a reordering here is the
/// early warning that those suites will move.
#[gpui::test]
#[serial]
fn p2_document_order_within_a_docked_panel(cx: &mut TestAppContext) {
    let (panels, _center, _host_fh, vcx) = mount(cx);
    vcx.update(|window, app| {
        let fh = panels[0].read(app).handles[0].clone();
        window.focus(&fh);
    });
    vcx.run_until_parked();

    let observed: Vec<String> = walk_tabs(vcx, 6).into_iter().flatten().collect();
    println!("P2 order = {observed:#?}");

    let positions: Vec<usize> = ["a-stop-1", "a-stop-2", "a-stop-3"]
        .iter()
        .filter_map(|want| observed.iter().position(|f| f == want))
        .collect();
    assert_eq!(
        positions.len(),
        3,
        "all three stops must appear: {observed:?}"
    );
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "P2: stops are visited OUT OF DOCUMENT ORDER ({observed:?}) — catalog_nav \
         and ai_nav assert on order and will need updating"
    );
}

// ---------------------------------------------------------------------------
// P5 — is `.tooltip(..)` usable at this rev?
// ---------------------------------------------------------------------------

/// gpui core exposes the hook (`gpui-0.2.2/src/elements/div.rs:1161`) and
/// gpui-component ships the view (`tooltip.rs:15`, `build` at `:62` returns the
/// `AnyView` the hook wants). Two comments in `view/sql_console.rs` claim no
/// tooltip helper exists at this rev; this decides whether they are wrong.
///
/// ⚠ MEASURED HERE: `Tooltip` is NOT re-exported at gpui-component's crate root
/// (`lib.rs:66` is a bare `pub mod tooltip;`), so the path is
/// `gpui_component::tooltip::Tooltip`. And `.tooltip()` lives on
/// `StatefulInteractiveElement`, so the element needs `.id()` first AND that
/// trait in scope — the same shape as `overflow_y_scroll` (A4).
#[gpui::test]
#[serial]
fn p5_tooltip_builds_and_renders(cx: &mut TestAppContext) {
    let (_panels, _center, _host_fh, vcx) = mount(cx);
    vcx.update(|_window, _app| {
        let _probe = div().id("tooltip-probe").tooltip(|window, app| {
            gpui_component::tooltip::Tooltip::new("probe").build(window, app)
        });
    });
    vcx.run_until_parked();
}

/// ⚠⚠ THE ACTUAL GATE — and the probe that corrected two earlier misreadings.
///
/// Staging focus INSIDE the docked panel first showed Tab cycling
/// `a-1 → a-2 → a-3 → a-1`, never reaching the center probe, which reads exactly
/// like a WCAG 2.1.2 keyboard trap. It is not one. A focus stop rendered inside
/// the CENTER panel (`DockItem::panel`) never registers in the tab-stop order at
/// all — it is captured by the a11y snapshot but is not a tab stop — so there
/// was nothing outside the group to escape TO, and `next()` was simply wrapping
/// to the global first (`tab_stop.rs:130`). The follow-up walk staged on the
/// center then showed `center → a-1`, which looks like traversal but is the
/// `tab_node_for_focus_id → None → next(None)` fallback at `tab_stop.rs:123-125`
/// — an unknown focus id restarting from the beginning.
///
/// Production is unaffected by that center quirk: the grid's tab stop lives on
/// the SHELL's root element, outside the dock, which is why
/// `keyboard_nav::grid_tab_reach_then_arrow_moves_active_cell` has stayed green
/// since B5.
///
/// `host-stop` lives OUTSIDE the `DockArea` — it is the analogue of the activity
/// rail or the hero in production. Staging there and walking answers the only
/// question that matters for B7: once Tab enters the dock's `.tab_group()`, can
/// it get back OUT? A sequence that never returns to `host-stop` is a WCAG 2.1.2
/// keyboard trap and blocks the whole slice.
#[gpui::test]
#[serial]
fn p1d_can_tab_escape_the_dock_group(cx: &mut TestAppContext) {
    let (_panels, _center, host_fh, vcx) = mount(cx);

    let snap = A11ySnapshot::capture(vcx);
    assert_eq!(
        snap.count_label("host-stop"),
        1,
        "host stop must be painted"
    );

    // B1: with nothing focused the dispatch path is the window root alone and
    // Tab is completely inert — measured again above. Focus the host stop.
    vcx.update(|window, _app| window.focus(&host_fh));
    vcx.run_until_parked();

    let observed = walk_tabs(vcx, 8);
    println!("P1d walk from host-stop = {observed:#?}");

    let seq: Vec<&str> = observed.iter().filter_map(|o| o.as_deref()).collect();
    // Entered the group...
    for want in ["a-stop-1", "a-stop-2", "a-stop-3"] {
        assert!(
            seq.contains(&want),
            "Tab must reach `{want}` inside the dock's tab group; walked {seq:?}"
        );
    }
    // ...and got back out. `position` of the FIRST host-stop is 0 (the staging
    // point), so require a SECOND one after the last group stop.
    let last_group = seq
        .iter()
        .rposition(|s| *s == "a-stop-3")
        .expect("a-stop-3 must be visited");
    assert!(
        seq[..last_group].iter().skip(1).any(|s| *s == "host-stop")
            || seq[last_group..].contains(&"host-stop"),
        "GATE FAILED: Tab never returns to a stop OUTSIDE the DockArea — the \
         dock traps the keyboard (WCAG 2.1.2). Walked {seq:?}"
    );
}
