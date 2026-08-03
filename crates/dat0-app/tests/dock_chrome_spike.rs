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

/// Mount a `DockArea` with a probe panel in the center, optionally also one in
/// a right dock, and return how many times each probe's label reached the
/// capture.
fn mount_and_count(cx: &mut TestAppContext, with_right_dock: bool) -> (usize, usize) {
    let (_center, vcx) = mount(cx, with_right_dock);
    let snap = A11ySnapshot::capture(vcx);
    (
        snap.count_label(CENTER_LABEL),
        snap.count_label(RIGHT_LABEL),
    )
}

/// Mount the right-dock case, stage focus on the center probe, then record the
/// focused label after each Tab press.
fn mount_and_walk_tabs(cx: &mut TestAppContext, presses: usize) -> Vec<Option<String>> {
    let (center, vcx) = mount(cx, true);

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
) -> (Entity<ProbePanel>, &mut gpui::VisualTestContext) {
    cx.update(gpui_component::init);

    let slot: Rc<RefCell<Option<Entity<ProbePanel>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();

    let (_root, vcx) = cx.add_window_view(|window, cx| {
        let host = cx.new(|cx| build_host(with_right_dock, window, cx));
        *slot_in.borrow_mut() = Some(host.read(cx).center.clone());
        gpui_component::Root::new(host, window, cx)
    });
    vcx.run_until_parked();

    let center = slot.borrow_mut().take().expect("center probe entity");
    (center, vcx)
}

fn build_host(with_right_dock: bool, window: &mut Window, cx: &mut Context<DockHost>) -> DockHost {
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

    DockHost { dock, center }
}
