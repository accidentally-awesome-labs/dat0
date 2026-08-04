//! B6 T0 gate — does a `TabPanel`'s chrome change what the single-frame a11y
//! capture sees?
//!
//! B5 measured that `DockItem::Panel` (the center mount, zero chrome) does NOT
//! double-render, and retired the master plan's top risk for that path only.
//! B6 mounts a real dock, and a dock's item is built out of `TabPanel`s, which
//! wrap the panel view in `overflow_y_scroll` and then in
//! `.cached(StyleRefinement::default().absolute().size_full())`
//! (`tab_panel.rs:851-861`), and mark the container `.tab_group()`.
//!
//! A cached element is the one construct that could plausibly break a capture
//! that runs during a single forced frame — and note the likely failure here is
//! nodes going MISSING, not duplicating. The generation-counter fallback
//! designed at `a11y/mod.rs:24` fixes duplicates and would NOT fix omissions,
//! so this has to be measured before any production code depends on the answer.
//!
//! The probe panel is synthetic on purpose: the only difference between the two
//! measurements is upstream chrome. The center and right probes carry DIFFERENT
//! labels so a count can never be misattributed between them.
//!
//! Hermeticity: `#[serial]` because the capture collector is process-global.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    App, AppContext as _, Axis, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, Window, div, px,
};
use gpui_component::dock::{DockArea, DockItem, Panel, PanelEvent};
use serial_test::serial;

use dat0_app::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
use dat0_app::theme::tokens::Dat0Theme as _;
use gpui_component::ActiveTheme as _;
use support::A11ySnapshot;

const CENTER_LABEL: &str = "probe-center";
const RIGHT_LABEL: &str = "probe-right";
/// B8: the bottom dock's probe. A third distinct label so a count can never be
/// misattributed between the three placements.
const BOTTOM_LABEL: &str = "probe-bottom";

/// A panel that emits exactly ONE capture node, so a count of 1 / 2 / 0 reads
/// directly as intact / duplicated / swallowed.
struct ProbePanel {
    fh: FocusHandle,
    label: &'static str,
}

impl EventEmitter<PanelEvent> for ProbePanel {}

impl Focusable for ProbePanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.fh.clone()
    }
}

impl Panel for ProbePanel {
    fn panel_name(&self) -> &'static str {
        "ProbePanel"
    }
}

impl Render for ProbePanel {
    /// Carries a real `focus_stop` as well as its capture node. The stop is
    /// what makes the Tab characterization below mean anything: `focus_stop`
    /// registers `fh → id` with the focus oracle, so `focused_label()` can name
    /// which probe Tab actually landed on. A gpui-component `Button` would not
    /// do — it has its own tab stop but never calls `record_focus_id`, so the
    /// oracle would report `None` whether or not it was focused.
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ring = cx.theme().d0().focus_ring;
        // `.a11y`, NOT `.a11y_label`: the focus oracle resolves a name in two
        // stages — focused handle → its static id, then a captured node whose
        // `click_id` matches that id → its text. A content-only `.a11y_label`
        // node records `click_id: None`, so focus would be set correctly and
        // still be unnameable. Exactly ONE of the two helpers per element —
        // both `push()` a node (A5), so using both would double the count the
        // tests above assert on.
        div()
            .a11y(self.label, AccessRole::Button, self.label.to_string())
            .focus_stop(self.label, &self.fh, 0, ring, |_ev, _window, _app| {})
            .child(self.label)
    }
}

/// `add_window_view`'s closure returns the VIEW VALUE, not an `Entity`, so
/// hosting a child entity needs a small owner struct (B5 lesson).
struct DockHost {
    dock: Entity<DockArea>,
    /// Kept so the Tab walk can focus a known starting point. B1 measured that
    /// with NOTHING focused the dispatch path is the window root alone and Tab
    /// is completely inert, so a walk that does not stage focus first measures
    /// nothing at all.
    center: Entity<ProbePanel>,
}

impl Render for DockHost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.dock.clone())
    }
}

/// Baseline / control: the B5 mount. Center only, `DockItem::Panel`, no chrome.
#[gpui::test]
#[serial]
fn bare_center_panel_emits_exactly_one_node(cx: &mut TestAppContext) {
    let (center, right) = mount_and_count(cx, false);
    assert_eq!(
        center, 1,
        "baseline: a bare DockItem::Panel center must emit the probe's single \
         node exactly once — this is B5's measured result, re-proven here as \
         the control for the chrome case"
    );
    assert_eq!(right, 0, "no right dock was mounted in the baseline case");
}

/// The B6 mount: a right dock, whose split item is built out of `TabPanel`s.
#[gpui::test]
#[serial]
fn right_dock_tab_panel_chrome_emits_exactly_one_node(cx: &mut TestAppContext) {
    let (center, right) = mount_and_count(cx, true);
    assert_eq!(
        center, 1,
        "adding a right dock must not disturb the center's own capture"
    );
    assert_eq!(
        right, 1,
        "TabPanel chrome (overflow_y_scroll + .cached(..) + .tab_group()) must \
         not change what a single-frame capture sees. 2 => the dock \
         double-renders and the generation counter at a11y/mod.rs:24 is now \
         needed; 0 => the .cached() wrapper swallowed the frame and that \
         counter would NOT help — see the B6 design doc section 8"
    );
}

/// Does a focus stop stay Tab-reachable from inside a dock's `TabPanel`?
///
/// `TabPanel::render` marks its container `.tab_group()` (`tab_panel.rs:1192`),
/// and B1 measured that gpui tab groups REORDER traversal rather than containing
/// it. That makes "can Tab still get into a docked panel at all" a real B6
/// question, and this answers it with the chrome in place.
///
/// The expected sequence is characterization, not preference — these are probe
/// panels, not the real Inspector and Charts. Its job is to be loud if the
/// answer ever changes.
#[gpui::test]
#[serial]
fn a_focus_stop_inside_dock_chrome_stays_tab_reachable(cx: &mut TestAppContext) {
    let observed = mount_and_walk_tabs(cx, 3);
    assert!(
        observed.iter().any(|f| f.as_deref() == Some(RIGHT_LABEL)),
        "Tab must be able to reach a focus stop inside the right dock's \
         TabPanel chrome; walked {observed:?}"
    );
}

/// B8 T0 (a): the bottom dock's `TabPanel` chrome, measured the same way B6
/// measured the right dock's.
///
/// The bottom placement is NOT a re-run of the right-dock case: `Dock::render`
/// branches on placement (`dock.rs:372-386`), and only the bottom branch keeps
/// rendering while closed. Measuring it separately is what makes the closed
/// case below interpretable.
#[gpui::test]
#[serial]
fn bottom_dock_tab_panel_chrome_emits_exactly_one_node(cx: &mut TestAppContext) {
    let (center, bottom) = mount_and_count_bottom(cx);
    assert_eq!(
        center, 1,
        "adding a bottom dock must not disturb the center's own capture"
    );
    assert_eq!(
        bottom, 1,
        "bottom-dock TabPanel chrome must not change what a single-frame \
         capture sees. 2 => the dock double-renders; 0 => the .cached() \
         wrapper swallowed the frame"
    );
}

/// B8 T0 (a), the part that has no analogue in B6/B7: a CLOSED bottom dock is
/// still rendered.
///
/// `Dock::render` returns an empty div for a closed left/right dock but
/// deliberately keeps a closed BOTTOM dock at `h(px(29.))`, so the user can
/// click its title bar to reopen (`dock.rs:372-380`, and the matching
/// reopen-on-tab-click at `tab_panel.rs:740-752`).
///
/// What that means for the a11y tree is the question: does the collapsed bar
/// keep contributing the panel's node? This pins the answer either way — the
/// number is characterization, and its job is to be loud if upstream changes.
#[gpui::test]
#[serial]
fn a_closed_bottom_dock_is_still_rendered(cx: &mut TestAppContext) {
    let (_center, dock, vcx) = mount(cx, false, true);

    let open_count = A11ySnapshot::capture(vcx).count_label(BOTTOM_LABEL);
    assert_eq!(open_count, 1, "sanity: the probe is captured while open");

    vcx.update(|window, cx| {
        dock.update(cx, |dock, cx| {
            dock.toggle_dock(gpui_component::dock::DockPlacement::Bottom, window, cx);
        });
    });
    vcx.run_until_parked();

    let closed_open_flag = vcx.cx.update(|app| {
        dock.read(app)
            .is_dock_open(gpui_component::dock::DockPlacement::Bottom, app)
    });
    assert!(!closed_open_flag, "toggle_dock must have closed it");

    let closed_count = A11ySnapshot::capture(vcx).count_label(BOTTOM_LABEL);
    println!("B8 T0(a): collapsed bottom dock contributes {closed_count} node(s)");
    assert!(
        closed_count <= 1,
        "a collapsed bottom dock must not DUPLICATE its panel's node; got \
         {closed_count}"
    );
}

/// B8 T0 (d): `is_dock_open` is observable IMMEDIATELY after `toggle_dock`.
///
/// This is the load-bearing assumption behind making `sql_console_visible` a
/// derived getter instead of a cached bool. `Dock::set_open` assigns
/// `self.open` synchronously and defers only `set_collapsed`
/// (`dock.rs:259-266`) — if that were not so, the getter would report the OLD
/// value inside the very call that toggled, and `toggle_sql_console`'s
/// refresh-on-show would fire on the wrong edge.
///
/// It also pins the direction: two toggles return to the starting state. A
/// cached bool desynced by upstream's own chevron is what would make the next
/// toggle move backwards, which is the bug this design removes.
#[gpui::test]
#[serial]
fn toggling_the_bottom_dock_is_observable_synchronously(cx: &mut TestAppContext) {
    let (_center, dock, vcx) = mount(cx, false, true);

    let observed = vcx.update(|window, cx| {
        let mut seen = vec![];
        dock.update(cx, |d, cx| {
            seen.push(d.is_dock_open(gpui_component::dock::DockPlacement::Bottom, cx));
            d.toggle_dock(gpui_component::dock::DockPlacement::Bottom, window, cx);
            // Read INSIDE the same update, with no frame in between.
            seen.push(d.is_dock_open(gpui_component::dock::DockPlacement::Bottom, cx));
            d.toggle_dock(gpui_component::dock::DockPlacement::Bottom, window, cx);
            seen.push(d.is_dock_open(gpui_component::dock::DockPlacement::Bottom, cx));
        });
        seen
    });

    assert_eq!(
        observed,
        vec![true, false, true],
        "is_dock_open must track toggle_dock with no settle frame, and two \
         toggles must return to the starting state"
    );
}

/// B8 T0 (c): can Tab reach a focus stop inside the BOTTOM dock's chrome?
///
/// ⚠ B7's rule applies and is why this walks from the center probe: a Tab-walk
/// probe is meaningless unless the reference point outside the tab group is
/// itself a registered tab stop. Two consecutive B7 probes "passed" while
/// measuring nothing, and the more convincing one was
/// `tab_node_for_focus_id → None → next(None)` restarting from the beginning.
/// `ProbePanel` carries a real `focus_stop`, so the center is a genuine
/// reference point.
#[gpui::test]
#[serial]
fn a_focus_stop_inside_the_bottom_dock_stays_tab_reachable(cx: &mut TestAppContext) {
    let (center, _dock, vcx) = mount(cx, false, true);

    vcx.update(|window, cx| {
        let fh = center.read(cx).fh.clone();
        window.focus(&fh);
    });
    vcx.run_until_parked();

    let mut observed = vec![
        A11ySnapshot::capture(vcx)
            .focused_label()
            .map(str::to_string),
    ];
    for _ in 0..3 {
        support::press_tab(vcx);
        observed.push(
            A11ySnapshot::capture(vcx)
                .focused_label()
                .map(str::to_string),
        );
    }

    println!("B8 T0(c): bottom-dock tab walk = {observed:?}");
    assert!(
        observed.iter().any(|f| f.as_deref() == Some(BOTTOM_LABEL)),
        "Tab must be able to reach a focus stop inside the bottom dock's \
         TabPanel chrome; walked {observed:?}"
    );
}

/// Mount a `DockArea` with a probe panel in the center, optionally also one in
/// a right dock, and return how many times each probe's label reached the
/// capture.
fn mount_and_count(cx: &mut TestAppContext, with_right_dock: bool) -> (usize, usize) {
    let (_center, _dock, vcx) = mount(cx, with_right_dock, false);
    let snap = A11ySnapshot::capture(vcx);
    (
        snap.count_label(CENTER_LABEL),
        snap.count_label(RIGHT_LABEL),
    )
}

/// B8: mount the bottom-dock case and return (center, bottom) label counts.
fn mount_and_count_bottom(cx: &mut TestAppContext) -> (usize, usize) {
    let (_center, _dock, vcx) = mount(cx, false, true);
    let snap = A11ySnapshot::capture(vcx);
    (
        snap.count_label(CENTER_LABEL),
        snap.count_label(BOTTOM_LABEL),
    )
}

/// Mount the right-dock case, stage focus on the center probe, then record the
/// focused label after each Tab press.
fn mount_and_walk_tabs(cx: &mut TestAppContext, presses: usize) -> Vec<Option<String>> {
    let (center, _dock, vcx) = mount(cx, true, false);

    // Stage focus. Without this the dispatch path is the window root alone and
    // Tab is inert — B1's measured result, and the reason a naive walk reports
    // an all-`None` sequence that looks like a finding but is an artifact.
    vcx.update(|window, cx| {
        let fh = center.read(cx).fh.clone();
        window.focus(&fh);
    });
    vcx.run_until_parked();

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

/// Open a window whose root is a `gpui_component::Root` wrapping the dock host,
/// and hand back the center probe entity plus the visual context.
///
/// The `Root` wrapper is not decoration: B1 measured that `Root` is what binds
/// `tab` / `shift-tab` as ACTIONS under key context `"Root"`
/// (`crates/ui/src/root.rs:21-22`). A bare host has NO Tab binding anywhere in
/// its tree, so a Tab walk against one measures the absence of `Root`, not the
/// behaviour of dock chrome. Production always has a `Root`.
fn mount(
    cx: &mut TestAppContext,
    with_right_dock: bool,
    with_bottom_dock: bool,
) -> (
    Entity<ProbePanel>,
    Entity<DockArea>,
    &mut gpui::VisualTestContext,
) {
    cx.update(gpui_component::init);

    let slot: Rc<RefCell<Option<(Entity<ProbePanel>, Entity<DockArea>)>>> =
        Rc::new(RefCell::new(None));
    let slot_in = slot.clone();

    let (_root, vcx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|cx| build_host(with_right_dock, with_bottom_dock, window, cx));
        let h = host.read(cx);
        *slot_in.borrow_mut() = Some((h.center.clone(), h.dock.clone()));
        gpui_component::Root::new(host, window, cx)
    });
    vcx.run_until_parked();

    let (center, dock) = slot.borrow_mut().take().expect("center probe entity");
    (center, dock, vcx)
}

fn build_host(
    with_right_dock: bool,
    with_bottom_dock: bool,
    window: &mut Window,
    cx: &mut Context<DockHost>,
) -> DockHost {
    let dock = cx.new(|cx| {
        let mut dock = DockArea::new("spike-dock", Some(1), window, cx);
        // Same posture as production: resize + collapse only, never drag.
        dock.set_locked(true, window, cx);
        dock
    });
    let weak_dock = dock.downgrade();

    let center = cx.new(|cx| ProbePanel {
        fh: cx.focus_handle(),
        label: CENTER_LABEL,
    });
    let center_item = DockItem::panel(std::sync::Arc::new(center.clone()));

    dock.update(cx, |dock, cx| {
        dock.set_center(center_item, window, cx);
    });

    if with_right_dock {
        let right = cx.new(|cx| ProbePanel {
            fh: cx.focus_handle(),
            label: RIGHT_LABEL,
        });
        // `DockItem::tab`, not `DockItem::panel`: `StackPanel::insert_panel`
        // hard-asserts that a split's children are TabPanel/StackPanel
        // (`stack_panel.rs:106-112`), which is exactly why B6 cannot dodge the
        // 30px title bar.
        let tab = DockItem::tab(right, &weak_dock, window, cx).size(px(288.));
        let split = DockItem::split(Axis::Horizontal, vec![tab], &weak_dock, window, cx);
        dock.update(cx, |dock, cx| {
            dock.set_right_dock(split, Some(px(288.)), true, window, cx);
        });
    }

    if with_bottom_dock {
        let bottom = cx.new(|cx| ProbePanel {
            fh: cx.focus_handle(),
            label: BOTTOM_LABEL,
        });
        // B8 passes the `DockItem::tab` STRAIGHT to `set_bottom_dock`, with no
        // enclosing split — the bottom dock holds exactly one panel, and a
        // single-panel `tab` is the only shape immune to B7's `set_active_ix`
        // re-entrancy panic. This probe therefore mounts the production shape.
        let tab = DockItem::tab(bottom, &weak_dock, window, cx);
        dock.update(cx, |dock, cx| {
            dock.set_bottom_dock(tab, Some(px(320.)), true, window, cx);
        });
    }

    DockHost { dock, center }
}
