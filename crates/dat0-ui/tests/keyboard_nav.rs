//! Keyboard reachability: can every control be got at without a mouse?
//!
//! # What replaced `focus_stop`
//!
//! The GPUI suite this ports was built around one mechanism. `FocusStopExt::
//! focus_stop(id, role, label)` chained `.track_focus().tab_stop(true)
//! .tab_index(0)` onto a `div`, gpui-component's `Root` bound `"tab"` /
//! `"shift-tab"` to `Window::focus_next` / `focus_prev`, and each stop needed a
//! hand-written `on_key_down` twin so Enter and Space would activate what a
//! click activated. Three of that suite's seven tests spend their doc comments
//! on the mechanism's failure modes: Tab is inert until something is already
//! focused, `Button` calls `prevent_default()` on mouse-down so a button can
//! never be click-focused, and focus does not survive a section switch.
//!
//! None of that exists here. A WebView gives tab order, the focus ring and
//! Enter/Space activation to *native elements* — so a control is
//! keyboard-operable exactly when it is a `button`, an `input`, or carries an
//! explicit `tabindex`. That is the guarantee these tests keep, because it is
//! the one a restyle silently breaks: a `div` with an `onclick` looks identical
//! and is unreachable.
//!
//! # What the harness can and cannot see
//!
//! [`Harness::tab_order`] is the nodes carrying `tabindex="0"`, in document
//! order, and `press_tab` walks them. Implicit focusability — a bare `<button>`
//! with no `tabindex` — is a browser rule with no mirror in a `WriteMutations`
//! tree, so where a surface relies on it the assertion is on the element type
//! rather than on a simulated Tab. Both are checked below; neither stands in
//! for the other.

mod support;

use std::sync::Arc;

use dioxus::prelude::*;
use tempfile::TempDir;

use dat0_core::grid::data_source::GridDataSource;
use dat0_core::grid::selection::SelectionModel;
use dat0_core::recents::RecentEntry;
use dat0_core::sample_data::{SampleKind, entries};
use dat0_core::settings::store::SettingsStore;
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};

use dat0_ui::components::empty_state::{EmptyState, sample_static_id};
use dat0_ui::components::grid::{COL_W_DEFAULT, Grid};
use dat0_ui::components::settings_ui::{Bus, SettingsPanel, SettingsProps, Store};
use support::{Harness, Key, Modifiers};

/// The `data-a11y-id`s of the tab stops, in order.
///
/// Every reachability assertion in this file is one of these lists, because a
/// tab stop that exists but sits in the wrong place is its own defect: the
/// GPUI original asserted the whole walk for the same reason.
fn stops(h: &Harness) -> Vec<String> {
    h.tab_order()
        .into_iter()
        .map(|k| {
            h.attr(k, "data-a11y-id")
                .unwrap_or_else(|| "<unnamed tab stop>".to_string())
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// The hero — the screen a first launch opens on, and the one the GPUI suite's
// T0/T1 spike walked end to end.
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct HeroHostProps {
    recents: Vec<RecentEntry>,
    first_run_done: bool,
}

#[component]
fn HeroHost(props: HeroHostProps) -> Element {
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        EmptyState {
            recents: props.recents.clone(),
            first_run_done: props.first_run_done,
            on_open_sample: move |_: SampleKind| log.write().push("sample".to_string()),
            on_open_recent: move |_: RecentEntry| log.write().push("recent".to_string()),
            on_open_file: move |_| log.write().push("picker".to_string()),
            on_take_tour: move |_| log.write().push("tour".to_string()),
            on_open_demo: move |_| log.write().push("demo".to_string()),
        }
        div { "data-a11y-id": "log", "{log.read().join(\",\")}" }
    }
}

fn hero(recents: Vec<RecentEntry>, first_run_done: bool) -> Harness {
    Harness::new(
        HeroHost,
        HeroHostProps {
            recents,
            first_run_done,
        },
    )
}

/// The first-run hero's stops, derived from the sample catalog rather than
/// spelled out — the GPUI test pulled the same titles from
/// `sample_data::entries()` so it would track the catalog instead of
/// duplicating it.
fn first_run_stop_ids() -> Vec<String> {
    let mut want = vec!["hero-take-tour".to_string(), "hero-open-demo".to_string()];
    want.extend(
        entries()
            .iter()
            .map(|e| sample_static_id(&e.kind).to_string()),
    );
    want.push("hero-open-file-samples".to_string());
    want
}

#[test]
fn the_first_run_hero_puts_every_control_in_the_tab_order() {
    // The GPUI walk began at the activity rail, which S1 deleted: the three-way
    // left dock became one always-present sidebar, and the rail's single
    // listbox stop went with it. Everything after it is unchanged, and the
    // order is still document order because every stop is `tabindex: 0`.
    let h = hero(Vec::new(), false);
    assert_eq!(stops(&h), first_run_stop_ids());
}

#[test]
fn tab_walks_the_hero_in_that_order_and_wraps_at_the_end() {
    // Reachability is not the same claim as presence. A stop that renders but
    // that Tab never lands on is exactly the defect the GPUI T0 spike was
    // written to catch, and it walked the cycle rather than querying the tree.
    let mut h = hero(Vec::new(), false);
    let want = first_run_stop_ids();

    for id in &want {
        h.press_tab();
        assert_eq!(
            h.focused_id().as_deref(),
            Some(id.as_str()),
            "tab order diverged at {id:?}; the walk so far reached {:?}",
            h.focused_id()
        );
    }

    h.press_tab();
    assert_eq!(
        h.focused_id().as_deref(),
        Some(want[0].as_str()),
        "Tab past the last stop must come back to the first, not fall out of \
         the screen"
    );
    h.press_shift_tab();
    assert_eq!(
        h.focused_id().as_deref(),
        Some(want[want.len() - 1].as_str()),
        "and Shift-Tab off the front wraps to the last"
    );
}

#[test]
fn every_hero_control_activates_from_the_keyboard_without_a_key_handler() {
    // GPUI needed an `on_key_down` twin per stop so Enter would do what a click
    // did — T0's criterion (2) and `hero_enter_activates_open_demo` each proved
    // one of those twins was wired. A native `<button>` fires `click` on Enter
    // and on Space itself, so what has to be true now is that these are
    // buttons: a `div` with the same class list and the same `onclick` would
    // look identical, pass every content test, and be dead to a keyboard.
    let h = hero(Vec::new(), false);
    for id in first_run_stop_ids() {
        let k = h
            .by_a11y_id(&id)
            .unwrap_or_else(|| panic!("{id} is not rendered"));
        assert_eq!(
            h.dom().get(k).tag(),
            Some("button"),
            "{id} is not a native button, so Enter and Space do nothing on it"
        );
        assert!(
            h.has_listener(k, "click"),
            "{id} is a button with nothing wired to it"
        );
    }
}

#[test]
fn the_returning_user_hero_reaches_open_file_right_after_the_recents_list() {
    // Task 1b of the GPUI suite: `hero-open-file-recents` is a different button
    // from `hero-open-file-samples` and only renders in this branch, so its
    // reachability had to be proven separately. The list itself is one stop —
    // six recents as six stops is how a keyboard user ends up pressing Tab a
    // dozen times to reach the button after them.
    let mut h = hero(
        vec![RecentEntry::Workspace {
            path: "/tmp/one.dat0".into(),
        }],
        false,
    );

    assert_eq!(
        stops(&h),
        vec![
            "hero-take-tour",
            "hero-open-demo",
            "recents-list",
            "hero-open-file-recents",
        ],
    );

    for _ in 0..4 {
        h.press_tab();
    }
    assert_eq!(h.focused_id().as_deref(), Some("hero-open-file-recents"));
}

#[test]
fn nothing_on_the_hero_is_reachable_by_mouse_alone() {
    // The general form of the rule, applied to the whole surface rather than to
    // the list of ids a test happens to know about: a node that answers a click
    // must be something a keyboard can get to. The recents rows are the one
    // deliberate exception and they say so with `tabindex="-1"` — they are
    // driven by the arrows of the list that contains them, which is itself a
    // stop.
    for recents in [
        Vec::new(),
        vec![RecentEntry::Workspace {
            path: "/tmp/one.dat0".into(),
        }],
    ] {
        let h = hero(recents, false);
        for k in h.dom().walk() {
            if !h.has_listener(k, "click") {
                continue;
            }
            let node = h.dom().get(k);
            let reachable = matches!(node.tag(), Some("button" | "input" | "a" | "select"))
                || node.attr("tabindex").is_some();
            assert!(
                reachable,
                "a {:?} with a click handler is reachable by mouse only: {:?}",
                node.tag(),
                node.attr("data-a11y-id")
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Settings — the GPUI suite's Task 2, which proved all three `toggle_row` call
// sites were reachable and operable.
// ─────────────────────────────────────────────────────────────────────────────

/// Every DIY toggle, with the section that has to be open for it to render.
const TOGGLES: &[(&str, &str)] = &[
    ("telemetry", "tg-telemetry"),
    ("workspace", "tg-workspace"),
    ("updates", "tg-updates"),
];

#[test]
fn every_settings_toggle_is_a_switch_a_keyboard_can_throw() {
    // GPUI reached these through a Tab round trip because that was the only way
    // to prove the hand-rolled `focus_stop` had been applied — and the suite's
    // module note spends 40 lines explaining why the round trip had to start
    // from a click *inside the same section*, since focus did not survive a
    // section switch and gpui-component `Button` refused click-focus outright.
    //
    // The row is now a `<button role="switch">`, so focus, the ring and
    // Space/Enter are the platform's. Per-instance coverage of all three call
    // sites is still the point: a fourth toggle pasted in as a `div` would be
    // invisible to a keyboard, and `aria-checked` is what a screen reader reads
    // out. Whether a throw reaches `settings.toml` is `settings_import.rs`'s.
    for (section, id) in TOGGLES {
        let store = Arc::new(SettingsStore::open_in_memory());
        let mut h = Harness::new(
            SettingsPanel,
            SettingsProps {
                store: Store(store),
                events: Bus(None),
            },
        );
        h.click(section);

        let k = h
            .by_a11y_id(id)
            .unwrap_or_else(|| panic!("{id} did not render in the {section} section"));
        assert_eq!(
            h.dom().get(k).tag(),
            Some("button"),
            "{id} must be a native button or Space does nothing on it"
        );
        assert_eq!(h.attr(k, "role").as_deref(), Some("switch"));
        assert!(
            h.attr(k, "aria-checked").is_some(),
            "{id} announces no state, so a screen reader cannot tell on from off"
        );
        assert!(h.has_listener(k, "click"), "{id} is wired to nothing");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The grid — the GPUI suite's Task 3.
//
// The grid never navigated through the focus chain: Tab had to reach the grid
// *shell*, and from there the arrows drove `SelectionModel` directly, entirely
// independent of which element held focus. That split survives the port —
// `keys::grid_key` is deliberately not part of the shell's chord cascade,
// because an arrow means "move the cursor" here and something else everywhere
// else.
// ─────────────────────────────────────────────────────────────────────────────

/// Three rows, two columns, through the real engine — enough for two separate
/// downs to each move a row without clamping at the bottom edge, which is what
/// the GPUI fixture (`a,b\n1,2\n3,4\n5,6\n`) was sized for.
async fn grid_fixture() -> (Arc<GridDataSource>, Vec<ProjectionColumn>, TempDir) {
    let tmp = TempDir::new().unwrap();
    let engine = DuckDBEngine::new(
        tmp.path().join("scratch.duckdb"),
        MemoryBudget {
            bytes: 128 * 1024 * 1024,
        },
    )
    .unwrap();
    engine.init().await.unwrap();

    const SQL: &str = "SELECT * FROM (VALUES (1, 2), (3, 4), (5, 6)) v(a, b)";
    engine
        .create_table("basic", SQL, DerivedOrigin::Sql(SQL.into()))
        .await
        .unwrap();

    let engine = Arc::new(engine);
    let ds = GridDataSource::new(Arc::clone(&engine), "basic".to_string())
        .await
        .unwrap();
    // `cell_render` is synchronous and shows the placeholder for a missing
    // page, so page 0 has to be resident before the first paint.
    ds.page_for(0).await.unwrap();

    let columns = ds
        .visible_column_names()
        .into_iter()
        .map(|n| ProjectionColumn {
            source: n.clone(),
            display: n,
        })
        .collect();
    (Arc::new(ds), columns, tmp)
}

#[derive(Clone, Props)]
struct GridHostProps {
    source: Arc<GridDataSource>,
    columns: Vec<ProjectionColumn>,
}

// A data source owns an Arrow LRU and a DuckDB handle; identity is the only
// equality that means anything.
impl PartialEq for GridHostProps {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source) && self.columns == other.columns
    }
}

/// The grid under a surface that records what got past it — the stand-in for
/// "the palette behind us" the grid's `stop_propagation` exists to protect.
#[component]
fn GridHost(props: GridHostProps) -> Element {
    let rows = props.source.row_count as usize;
    let cols = props.columns.len();
    let selection = use_signal(|| SelectionModel::new(rows, cols));
    let widths = use_signal(|| vec![COL_W_DEFAULT; cols]);
    let mut behind = use_signal(Vec::<String>::new);
    use_context_provider(|| selection);

    rsx! {
        div {
            "data-a11y-id": "behind",
            onkeydown: move |e: KeyboardEvent| behind.write().push(e.key().to_string()),

            Grid {
                source: props.source.clone(),
                selection,
                columns: props.columns.clone(),
                widths,
            }
            div { "data-a11y-id": "escaped", "{behind.read().join(\",\")}" }
        }
        div {
            "data-a11y-id": "active",
            "{selection.read().active().row},{selection.read().active().col}"
        }
    }
}

fn grid(source: Arc<GridDataSource>, columns: Vec<ProjectionColumn>) -> Harness {
    Harness::new(GridHost, GridHostProps { source, columns })
}

fn active(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("active").expect("the readback is mounted"))
}

#[tokio::test]
async fn tab_reaches_the_grid_and_then_the_arrows_move_the_active_cell() {
    let (source, columns, _tmp) = grid_fixture().await;
    let mut h = grid(source, columns);

    // One stop for the whole grid, as in GPUI: the cells are content, not
    // focusables, and a thousand-row table of tab stops is not navigation.
    assert_eq!(stops(&h), vec!["grid-viewport"]);
    h.press_tab();
    assert_eq!(h.focused_id().as_deref(), Some("grid-viewport"));

    assert_eq!(active(&h), "0,0", "the cursor starts at the origin");

    h.key_at("grid-viewport", Key::ArrowDown, Modifiers::empty());
    h.key_at("grid-viewport", Key::ArrowDown, Modifiers::empty());
    assert_eq!(active(&h), "2,0", "two downs move two rows");

    h.key_at("grid-viewport", Key::ArrowRight, Modifiers::empty());
    assert_eq!(active(&h), "2,1");

    h.key_at("grid-viewport", Key::ArrowDown, Modifiers::empty());
    assert_eq!(
        active(&h),
        "2,1",
        "the last row is the end of the table, not the start of the next one"
    );
}

#[tokio::test]
async fn an_arrow_the_grid_consumes_does_not_also_reach_the_surface_behind_it() {
    // The grid is a modal surface: while it has the keyboard, an arrow is a
    // cursor move and nothing else. Without `stop_propagation` the same
    // keystroke would also scroll whatever is mounted behind it — which under
    // GPUI could not happen, because the grid's grammar was read from a raw
    // key handler that never produced an action for the ambient tree to see.
    let (source, columns, _tmp) = grid_fixture().await;
    let mut h = grid(source, columns);

    h.key_at("grid-viewport", Key::ArrowDown, Modifiers::empty());
    assert_eq!(
        h.text_of(h.by_a11y_id("escaped").unwrap()),
        "",
        "the arrow bubbled past the grid"
    );

    // A key the grid has no grammar for is not the grid's to swallow: the
    // shell's own chords have to keep working while the grid holds focus.
    h.key_at("grid-viewport", Key::Character("k".into()), Modifiers::META);
    assert_eq!(h.text_of(h.by_a11y_id("escaped").unwrap()), "k");
}
