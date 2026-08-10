//! The right column (S5) — inspector and charts as two independent panes.
//!
//! Ported from `dat0-app/tests/right_dock.rs` (B6). That suite proved the
//! inspector and charts bools reached a real `DockArea`, by asserting through
//! the a11y capture rather than by re-reading the bool it had just written.
//! The rule survives; the thing it walks does not.
//!
//! Under GPUI the right dock was a `DockItem::split` that **always reserved
//! space**: `sync_right_dock` opened and closed it, but the split itself was
//! mounted for the life of the window, and each panel wore `TabPanel` chrome
//! dat0 was actively fighting. S5 replaces it with a plain stack of two
//! [`Pane`](dat0_ui::components::pane::Pane)s that collapse independently, and
//! adds a guarantee GPUI never had: **when both are closed the column is zero
//! pixels wide and the grid takes the space.** That is what most of this file
//! is about.
//!
//! Deliberately dropped: `showing_charts_renders_its_title_and_export_buttons`
//! asserted `chart.export.png` / `chart.export.svg` in the charts title bar.
//! Chart export has no implementation anywhere in `dat0-ui` — the descriptors
//! are still registered in `dat0_core::actions::view_actions`, but `router.rs`
//! claims neither and the charts toolbar carries type, axes and Save only — so
//! there is nothing to assert against. The title half of that test is kept
//! below; the export half is reported as a Phase-5 gap rather than weakened
//! into something that passes.
//!
//! Hermeticity: every test pins `DAT0_CONFIG_DIR` at a fresh temp dir and seeds
//! `first_run_done`, because the shell reads both — without it the first-run
//! tour opens over the shell on any machine with no `settings.toml`, and the
//! suite would pass or fail according to whose home directory it ran in.
//! `#[serial]` because the env var is process-global.

mod support;

use std::rc::Rc;

use dioxus::prelude::*;
use serial_test::serial;
use tempfile::TempDir;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_core::session::dock_layout::DockLayout;
use dat0_core::settings::store::SettingsStore;
use dat0_ui::components::shell::Shell;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;
use support::dom::NodeKey;

// ── host ─────────────────────────────────────────────────────────────────────

/// The real `Shell` under the four contexts a window provides, seeded with a
/// layout and given two drivers.
///
/// The drivers exist for the same reason B6's `chart_bind_for_test` and
/// `seed_lineage_target_for_test` shims did: with both panes closed the column
/// is not rendered at all, so there is no header to click and no way in from
/// the DOM. Everything a test can reach through the UI, it reaches through the
/// UI — the drivers are only ever used to *open* from nothing.
#[derive(Clone, PartialEq, Props, Default)]
struct HostProps {
    layout: DockLayout,
}

#[component]
fn Host(props: HostProps) -> Element {
    let mut ws = Workspace::provide();
    Theme::provide(None);
    use_context_provider(ActionRegistry::new);
    let (events, _rx) = use_hook(|| {
        let (tx, rx) = AppEvents::channel();
        (tx, Rc::new(std::cell::RefCell::new(rx)))
    });
    use_context_provider(|| events.clone());

    // Before the first child render, so `Shell` never sees the default layout.
    {
        let seed = props.layout.clone();
        use_hook(move || ws.layout.set(seed));
    }

    rsx! {
        Shell {}
        button {
            "data-a11y-id": "drive-inspector",
            onclick: move |_| {
                let v = ws.layout.read().inspector_visible;
                ws.layout.write().inspector_visible = !v;
            },
            "inspector"
        }
        button {
            "data-a11y-id": "drive-charts",
            onclick: move |_| {
                let v = ws.layout.read().charts_visible;
                ws.layout.write().charts_visible = !v;
            },
            "charts"
        }
    }
}

/// Run `f` with `DAT0_CONFIG_DIR` pointed at a fresh temp dir.
///
/// Copied verbatim from `tests/onboarding.rs` — one shape for every suite that
/// mounts the shell, rather than a per-file variant that drifts.
fn with_config_dir<R>(f: impl FnOnce(&TempDir) -> R) -> R {
    let tmp = TempDir::new().unwrap();
    let previous = std::env::var_os("DAT0_CONFIG_DIR");
    // SAFETY: `#[serial]` keeps every env-touching test off the same clock, and
    // nothing else in this binary reads the variable concurrently.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", tmp.path()) };
    let out = f(&tmp);
    unsafe {
        match previous {
            Some(v) => std::env::set_var("DAT0_CONFIG_DIR", v),
            None => std::env::remove_var("DAT0_CONFIG_DIR"),
        }
    }
    out
}

/// Mount the shell over `layout`, on a machine that has already seen the tour.
///
/// Seeding `first_run_done` is not decoration: the shell auto-opens the
/// onboarding carousel when it is false, and this suite is about the right
/// column, not about what is painted over it.
fn with_shell<R>(layout: DockLayout, f: impl FnOnce(&mut Harness) -> R) -> R {
    with_config_dir(|tmp| {
        let store = SettingsStore::with_path(tmp.path().join("settings.toml"));
        dat0_core::settings::set_first_run_done(&store, true).expect("seed first_run_done");

        let mut h = Harness::new(Host, HostProps { layout });
        h.settle();
        f(&mut h)
    })
}

fn closed() -> DockLayout {
    DockLayout::default()
}

fn with_inspector() -> DockLayout {
    DockLayout {
        inspector_visible: true,
        ..DockLayout::default()
    }
}

fn with_charts() -> DockLayout {
    DockLayout {
        charts_visible: true,
        ..DockLayout::default()
    }
}

fn with_both() -> DockLayout {
    DockLayout {
        inspector_visible: true,
        charts_visible: true,
        ..DockLayout::default()
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// How many nodes carry this `data-a11y-id`.
///
/// `by_a11y_id` answers "is there one"; B6's assertions were about *exactly*
/// one, because two meant the panel had been mounted twice and both copies
/// responded.
fn count_id(h: &Harness, id: &str) -> usize {
    h.dom()
        .walk()
        .into_iter()
        .filter(|k| h.dom().get(*k).attr("data-a11y-id") == Some(id))
        .count()
}

/// The first node with this CSS class. The shell's three grid containers carry
/// their track sizes in an inline `style` and no id, and the track size is the
/// only place "zero width" is observable in a harness with no layout engine.
fn by_class(h: &Harness, class: &str) -> Option<NodeKey> {
    h.dom()
        .walk()
        .into_iter()
        .find(|k| h.dom().get(*k).attr("class") == Some(class))
}

/// The right column's width, read off the work area's grid template.
fn right_track(h: &Harness) -> String {
    let k = by_class(h, "d0-workarea").expect("the work area is always rendered");
    h.attr(k, "style").expect("the work area sizes its tracks")
}

fn expanded(h: &Harness, id: &str) -> Option<String> {
    h.attr(
        h.by_a11y_id(id)
            .unwrap_or_else(|| panic!("no element with data-a11y-id={id:?}")),
        "aria-expanded",
    )
}

fn inspector_title() -> String {
    dat0_i18n::t("inspector.title")
}

fn charts_title() -> String {
    dat0_i18n::t("chart.panel.title")
}

// ── the column ───────────────────────────────────────────────────────────────

/// The new guarantee, and the one GPUI could not make: nothing in the column
/// means no column.
#[test]
#[serial]
fn the_right_column_is_zero_wide_when_both_panes_are_closed() {
    with_shell(closed(), |h| {
        assert_eq!(
            count_id(h, "right-column"),
            0,
            "a fresh workspace shows neither pane, so the column must not be \
             in the tree at all"
        );
        assert_eq!(
            count_id(h, "pane-inspector"),
            0,
            "no Inspector pane while it is hidden"
        );
        assert_eq!(
            count_id(h, "pane-charts"),
            0,
            "no Charts pane while it is hidden"
        );
        assert_eq!(
            count_id(h, "right-splitter"),
            0,
            "and no splitter for a column that is not there"
        );

        assert!(
            right_track(h).contains("minmax(0, 1fr) 0px"),
            "the column must be ZERO pixels, not merely empty — the GPUI split \
             reserved its width whether or not a panel was showing: {}",
            right_track(h)
        );
    });
}

#[test]
#[serial]
fn showing_the_inspector_opens_the_column_and_titles_its_pane() {
    with_shell(with_inspector(), |h| {
        assert_eq!(
            count_id(h, "right-column"),
            1,
            "showing the Inspector must open the right column"
        );
        // EXACTLY one. Two would mean the pane is mounted twice, which is the
        // failure B6 counted labels to catch.
        assert_eq!(
            count_id(h, "pane-inspector"),
            1,
            "the Inspector pane appears exactly once"
        );
        assert!(
            h.text_of(h.by_a11y_id("pane-head-inspector").unwrap())
                .contains(&inspector_title()),
            "its header carries the Inspector's title"
        );

        // S5 keeps the sibling MOUNTED and collapsed rather than unmounting
        // it: its header is the only affordance that reopens it, and a pane
        // that vanished would take its own reopen control with it. Collapsed,
        // not absent, is therefore the guarantee — the GPUI split had no such
        // state, because its `TabPanel` was either in the tree or gone.
        assert_eq!(
            count_id(h, "pane-charts"),
            1,
            "Charts is present as a collapsed header when only the Inspector \
             was shown"
        );
        assert_eq!(
            expanded(h, "pane-head-charts"),
            Some("false".to_string()),
            "and it announces itself collapsed"
        );

        assert!(
            !right_track(h).contains("1fr) 0px"),
            "an open pane gives the column real width: {}",
            right_track(h)
        );
    });
}

#[test]
#[serial]
fn showing_charts_opens_the_column_and_titles_its_pane() {
    with_shell(with_charts(), |h| {
        assert_eq!(count_id(h, "right-column"), 1);
        assert_eq!(
            count_id(h, "pane-charts"),
            1,
            "the Charts pane appears exactly once"
        );
        assert!(
            h.text_of(h.by_a11y_id("pane-head-charts").unwrap())
                .contains(&charts_title()),
            "its header carries the Charts title"
        );
        assert_eq!(
            expanded(h, "pane-head-inspector"),
            Some("false".to_string()),
            "the Inspector is collapsed, not open, when only Charts was shown"
        );
    });
}

/// The bidirectional proof. A shell that only ever opened the column would
/// pass every test above.
#[test]
#[serial]
fn closing_both_panes_collapses_the_column_to_zero_width() {
    with_shell(with_both(), |h| {
        assert_eq!(count_id(h, "right-column"), 1, "seed: both panes showing");

        // Through the UI, not through the drivers: each pane header is the
        // user's own close affordance.
        h.click("pane-head-inspector");
        assert_eq!(
            count_id(h, "right-column"),
            1,
            "closing one pane must NOT take the column with it — Charts is \
             still showing"
        );
        assert_eq!(
            count_id(h, "pane-charts"),
            1,
            "and the surviving pane is still mounted"
        );

        h.click("pane-head-charts");
        assert_eq!(
            count_id(h, "right-column"),
            0,
            "closing the last pane collapses the column"
        );
        assert!(
            right_track(h).contains("minmax(0, 1fr) 0px"),
            "and hands its width back to the grid: {}",
            right_track(h)
        );
    });
}

/// S5's "two independently collapsible panes", stated as the thing that
/// distinguishes it from the GPUI split: collapsing one pane must leave the
/// other's chrome, body and open state untouched.
#[test]
#[serial]
fn each_pane_collapses_independently_of_its_sibling() {
    with_shell(with_both(), |h| {
        assert_eq!(expanded(h, "pane-head-inspector"), Some("true".to_string()));
        assert_eq!(expanded(h, "pane-head-charts"), Some("true".to_string()));

        h.click("pane-head-inspector");

        assert_eq!(
            expanded(h, "pane-head-inspector"),
            Some("false".to_string()),
            "the collapsed pane keeps its header and says so"
        );
        assert_eq!(
            h.attr(h.by_a11y_id("pane-inspector").unwrap(), "class"),
            Some("d0-pane is-collapsed".to_string()),
            "and wears the collapsed treatment"
        );
        assert_eq!(
            expanded(h, "pane-head-charts"),
            Some("true".to_string()),
            "its sibling is untouched — this is a stack, not a split that \
             resizes both halves"
        );
    });
}

/// Reopening from a fully collapsed column, which is only reachable from
/// outside the column: the shell's own toggles.
#[test]
#[serial]
fn a_collapsed_column_comes_back_when_either_pane_is_shown() {
    with_shell(closed(), |h| {
        assert_eq!(count_id(h, "right-column"), 0, "seed: collapsed");

        h.click("drive-charts");
        assert_eq!(
            count_id(h, "right-column"),
            1,
            "showing Charts brings the column back"
        );
        assert_eq!(count_id(h, "pane-charts"), 1);

        h.click("drive-charts");
        assert_eq!(
            count_id(h, "right-column"),
            0,
            "and hiding it again collapses the column, not just the pane"
        );
    });
}

/// B6's `inspector_body_content_reaches_the_capture_through_the_dock`.
///
/// That test existed because `TabPanel` wrapped a panel's body in
/// `.cached(..)`, and asserting on the title bar alone would keep passing even
/// if the cache swallowed every body node. There is no cache here, so the
/// claim is the direct one: a pane renders its BODY, not just its chrome, and
/// the body's own content reaches the tree.
#[test]
#[serial]
fn the_inspector_body_reaches_the_tree_through_the_pane() {
    with_shell(with_inspector(), |h| {
        let body = h
            .by_a11y_id("pane-body-inspector")
            .expect("the pane renders a body, not only a header");
        assert_eq!(
            count_id(h, "inspector"),
            1,
            "the Inspector's own root is inside it exactly once"
        );
        assert!(
            h.text_of(body).contains(&dat0_i18n::t("inspector.empty")),
            "and the body's real content — not the header — is what reached \
             the tree: {:?}",
            h.text_of(body)
        );
    });
}

/// The column is resizable, and the drag is an event rather than a poll.
///
/// GPUI discovered a dock resize by serializing `DockArea::dump()` every frame
/// and diffing it, because `Dock` emitted nothing. Here the splitter is a real
/// node with a real `mousedown`, which is the whole of the mechanism.
#[test]
#[serial]
fn an_open_column_carries_a_splitter_that_starts_a_drag() {
    with_shell(with_both(), |h| {
        let splitter = h
            .by_a11y_id("right-splitter")
            .expect("an open column has a resize edge");
        assert_eq!(
            h.attr(splitter, "role"),
            Some("separator".to_string()),
            "the splitter announces itself as one"
        );
        assert_eq!(
            h.attr(splitter, "aria-orientation"),
            Some("vertical".to_string()),
            "a column edge is a vertical separator"
        );
        assert!(
            h.has_listener(splitter, "mousedown"),
            "and it is wired — a splitter with no gesture is decoration"
        );
    });
}

/// B9's `an_absurd_persisted_size_is_clamped_not_obeyed`, on the shell that
/// now restores sizes.
///
/// A layout saved on a 4K display and reopened on a laptop. Mounting the
/// number verbatim gives the centre zero width, puts the splitter off screen
/// and leaves the user no in-app way back. The file keeps what they chose
/// (`dat0-core/tests/dock_layout_persist.rs`); the mount clamps it to four
/// fifths of the axis — 1152px of the harness window's default 1440.
#[test]
#[serial]
fn a_width_restored_from_a_bigger_display_is_clamped_not_obeyed() {
    with_shell(
        DockLayout {
            inspector_visible: true,
            right_size: Some(30_000),
            ..DockLayout::default()
        },
        |h| {
            let track = right_track(h);
            assert!(
                track.contains("1152px"),
                "the column mounts clamped to 80% of the window, not at the \
                 persisted 30 000: {track}"
            );
            assert!(
                !track.contains("30000px"),
                "and never at the raw persisted value: {track}"
            );
        },
    );
}
