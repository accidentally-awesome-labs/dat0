//! The tab strip: the ⌘K command launcher, the workspace tabs, and how a
//! keyboard reaches them.
//!
//! Ported from `crates/dat0-app/tests/tab_strip_nav.rs`, which proved two
//! things about the search gutter — it is Tab-reachable, and activating it
//! opens the palette through the same action ⌘⇧P dispatches. Both survive; the
//! surface underneath them did not.
//!
//! # What changed with the toolkit
//!
//! * The GPUI strip was gated on `view_model.is_some()`, so the gutter did not
//!   exist until a table was open. The Dioxus strip is unconditional: the
//!   launcher is the first child of the strip in every window state, which is
//!   what makes ⌘K discoverable on a cold launch (S2).
//! * The responsive rule was a Rust branch (`search_gutter_is_compact`) that
//!   built a different widget below 1080px. It is now one `@media` block, so
//!   "both sides of the threshold paint" is a stylesheet guarantee and is
//!   asserted against the stylesheet — the harness has no layout, and a
//!   windowed probe cannot prove the *absence* of a `display: none`.
//! * The strip is now **one** tab stop by design (`AccessRole::Tab` →
//!   `TabStop::Programmatic`). Under GPUI every tab was its own focus stop;
//!   six open tabs meant six Tab presses before the grid. Arrow keys move
//!   within the strip instead. That is a deliberate change, so it is asserted
//!   here rather than left implicit.

mod support;

use std::path::PathBuf;
use std::rc::Rc;

use dioxus::prelude::*;
use serial_test::serial;
use tempfile::TempDir;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_ui::components::shell::Shell;
use dat0_ui::state::{TabView, Workspace};
use dat0_ui::theme::Theme;
use support::{Harness, dom::NodeKey};

/// The stylesheet the shipped binary serves, read from source.
const APP_CSS: &str = include_str!("../assets/app.css");

// ── mounting ─────────────────────────────────────────────────────────────────

/// Point the settings store at a scratch directory whose first run is already
/// behind us, so the shell renders its steady state: no enriched hero, no
/// first-run tour over the top of the strip.
///
/// `DAT0_CONFIG_DIR` is process-global, hence `#[serial]` on every test that
/// mounts.
fn with_settled_config<R>(f: impl FnOnce() -> R) -> R {
    let tmp = TempDir::new().unwrap();
    let store =
        dat0_core::settings::store::SettingsStore::with_path(tmp.path().join("settings.toml"));
    dat0_core::settings::set_first_run_done(&store, true).unwrap();

    let previous = std::env::var_os("DAT0_CONFIG_DIR");
    // SAFETY: `#[serial]` keeps every env-touching test off the same clock, and
    // no other thread in this binary reads the variable.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", tmp.path()) };
    let out = f();
    unsafe {
        match previous {
            Some(v) => std::env::set_var("DAT0_CONFIG_DIR", v),
            None => std::env::remove_var("DAT0_CONFIG_DIR"),
        }
    }
    out
}

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    tabs: Vec<TabView>,
    active: Option<usize>,
}

/// The real `Shell` under the four contexts a window provides, plus a readback
/// node — the harness sees text, not Rust state.
///
/// `components::App` cannot be mounted here: it registers the webview asset
/// handler, which needs a webview. Everything below it can.
#[component]
fn Host(props: HostProps) -> Element {
    let ws = Workspace::provide();
    Theme::provide(None);
    use_context_provider(ActionRegistry::new);
    // The receiver is held for the component's life: an `AppEvents` whose
    // receiver has been dropped silently swallows everything sent to it, which
    // would make a dispatch assertion pass for the wrong reason.
    let (events, _rx) = use_hook(|| {
        let (tx, rx) = AppEvents::channel();
        (tx, Rc::new(std::cell::RefCell::new(rx)))
    });
    use_context_provider(|| events.clone());

    {
        let seed = props.clone();
        use_hook(move || {
            let mut ws = ws;
            ws.tabs.set(seed.tabs.clone());
            ws.active.set(seed.active);
        });
    }

    let palette_open = *ws.palette.read();

    rsx! {
        Shell {}
        div { "data-a11y-id": "rb-palette", "{palette_open}" }
    }
}

fn mount(tabs: Vec<TabView>, active: Option<usize>) -> Harness {
    let mut h = Harness::new(Host, HostProps { tabs, active });
    h.settle();
    h
}

fn tab(table: &str, path: Option<&str>) -> TabView {
    TabView {
        table: table.to_string(),
        path: path.map(PathBuf::from),
    }
}

/// Three tabs, one of every swatch-bearing kind plus a table-only tab.
fn three_tabs() -> Vec<TabView> {
    vec![
        tab("sales", Some("/tmp/sales.csv")),
        tab("events", Some("/tmp/events.parquet")),
        tab("scratch", None),
    ]
}

// ── subtree helpers ──────────────────────────────────────────────────────────

/// Every live descendant of `root`, in document order, `root` included.
fn subtree(h: &Harness, root: NodeKey) -> Vec<NodeKey> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let node = h.dom().get(out[i]);
        for c in &node.children {
            if !h.dom().get(*c).removed {
                out.push(*c);
            }
        }
        i += 1;
    }
    out
}

fn strip(h: &Harness) -> NodeKey {
    h.by_a11y_id("tabstrip")
        .expect("the shell must render a tab strip in every window state")
}

// ── the launcher ─────────────────────────────────────────────────────────────

#[test]
#[serial]
fn the_command_launcher_is_the_strips_first_child() {
    with_settled_config(|| {
        let h = mount(three_tabs(), Some(0));
        let strip = strip(&h);

        assert_eq!(
            h.attr(strip, "role").as_deref(),
            Some("tablist"),
            "the strip is the tablist a reader announces the tabs under"
        );

        let first = *h
            .dom()
            .get(strip)
            .children
            .first()
            .expect("the strip has children");
        assert_eq!(
            h.attr(first, "data-a11y-id").as_deref(),
            Some("command-slot"),
            "the launcher slot leads the strip so it aligns with the sidebar \
             below it (S2); a tab in front of it would break the alignment the \
             whole gutter exists for"
        );
        assert!(
            h.by_a11y_id("command-launcher").is_some(),
            "the slot must contain the launcher button"
        );
    });
}

#[test]
#[serial]
fn the_launcher_is_present_before_any_table_is_open() {
    with_settled_config(|| {
        // The GPUI strip was gated on `view_model.is_some()`: on a cold launch
        // there was no strip and therefore no visible ⌘K affordance at all.
        let h = mount(Vec::new(), None);
        assert!(h.by_a11y_id("tabstrip").is_some());
        assert!(h.by_a11y_id("command-launcher").is_some());
    });
}

#[test]
#[serial]
fn the_launcher_announces_itself_and_is_reachable_by_tab() {
    with_settled_config(|| {
        let mut h = mount(three_tabs(), Some(0));
        let launcher = h.by_a11y_id("command-launcher").unwrap();

        assert_eq!(h.attr(launcher, "role").as_deref(), Some("button"));
        assert_eq!(
            h.attr(launcher, "aria-label").as_deref(),
            Some(dat0_i18n::t("palette.open").as_str()),
            "a painted button nobody can name is worse than no button"
        );

        // Tab-reachable, bounded: the GPUI test walked up to 24 hops before
        // failing, and the shape of the guarantee is the same — Tab gets there
        // without the user hunting for it.
        let mut hops = 0;
        let reached = loop {
            hops += 1;
            if hops > 24 {
                break false;
            }
            h.press_tab();
            if h.focused_id().as_deref() == Some("command-launcher") {
                break true;
            }
        };
        assert!(
            reached,
            "the launcher was never the focused Tab stop within 24 hops — is \
             it still `tabindex=0`? tab order was {:?}",
            h.tab_order()
                .into_iter()
                .map(|k| h.attr(k, "data-a11y-id"))
                .collect::<Vec<_>>()
        );
    });
}

#[test]
#[serial]
fn activating_the_launcher_opens_the_command_palette() {
    with_settled_config(|| {
        let mut h = mount(three_tabs(), Some(0));
        assert_eq!(
            h.text_of(h.by_a11y_id("rb-palette").unwrap()),
            "false",
            "precondition: the palette is shut"
        );
        assert!(
            h.by_a11y_id("palette").is_none(),
            "precondition: nothing is mounted for it"
        );

        h.click("command-launcher");

        assert_eq!(
            h.text_of(h.by_a11y_id("rb-palette").unwrap()),
            "true",
            "clicking the gutter must open the palette — the chord it \
             advertises and the control must not drift apart"
        );
        assert!(
            h.by_a11y_id("palette").is_some(),
            "the palette must actually mount, not merely flip a flag"
        );
    });
}

// ── the tabs ─────────────────────────────────────────────────────────────────

#[test]
#[serial]
fn each_tab_announces_whether_it_is_the_selected_one() {
    with_settled_config(|| {
        let h = mount(three_tabs(), Some(1));

        for (i, want) in [(0, "false"), (1, "true"), (2, "false")] {
            let key = h
                .by_a11y_id(&format!("tab-{i}"))
                .unwrap_or_else(|| panic!("tab {i} must render"));
            assert_eq!(h.attr(key, "role").as_deref(), Some("tab"));
            assert_eq!(
                h.attr(key, "aria-selected").as_deref(),
                Some(want),
                "tab {i} reported the wrong selection state"
            );
        }
    });
}

#[test]
#[serial]
fn clicking_a_tab_moves_the_selection_to_it() {
    with_settled_config(|| {
        let mut h = mount(three_tabs(), Some(0));
        h.click("tab-2");

        assert_eq!(
            h.attr(h.by_a11y_id("tab-2").unwrap(), "aria-selected")
                .as_deref(),
            Some("true")
        );
        assert_eq!(
            h.attr(h.by_a11y_id("tab-0").unwrap(), "aria-selected")
                .as_deref(),
            Some("false"),
            "exactly one tab is selected at a time"
        );
    });
}

#[test]
#[serial]
fn the_whole_strip_is_one_tab_stop_however_many_tabs_are_open() {
    with_settled_config(|| {
        // Six tabs: the number at which the GPUI build's per-tab focus stops
        // became the reason a keyboard user pressed Tab six times to reach the
        // grid. `AccessRole::Tab` is `TabStop::Programmatic` precisely so that
        // Tab reaches the strip once and arrows move within it.
        let tabs: Vec<TabView> = (0..6).map(|i| tab(&format!("t{i}"), None)).collect();
        let h = mount(tabs, Some(0));

        let strip = strip(&h);
        let stops: Vec<String> = subtree(&h, strip)
            .into_iter()
            .filter(|k| h.attr(*k, "tabindex").as_deref() == Some("0"))
            .map(|k| h.attr(k, "data-a11y-id").unwrap_or_default())
            .collect();
        assert_eq!(
            stops,
            vec!["command-launcher".to_string()],
            "the strip must expose exactly one Tab stop"
        );

        for i in 0..6 {
            let key = h.by_a11y_id(&format!("tab-{i}")).unwrap();
            assert_eq!(
                h.attr(key, "tabindex").as_deref(),
                Some("-1"),
                "tab {i} must be programmatically focusable but skipped by Tab \
                 — a `button` with no tabindex is a Tab stop in a real webview, \
                 which is the behaviour this replaces"
            );
        }
    });
}

#[test]
#[serial]
fn a_tab_backed_by_a_file_carries_its_format_swatch() {
    with_settled_config(|| {
        let h = mount(three_tabs(), Some(0));

        // S8: one swatch definition, used by the sidebar, the chips and here,
        // so a `.parquet` is the same purple everywhere.
        let swatch_class = |id: &str| -> Option<String> {
            let key = h.by_a11y_id(id).unwrap();
            subtree(&h, key)
                .into_iter()
                .filter_map(|k| h.attr(k, "class"))
                .find(|c| c.starts_with("d0-swatch "))
        };

        assert_eq!(
            swatch_class("tab-0").as_deref(),
            Some("d0-swatch sw-csv"),
            "a .csv tab wears the csv swatch"
        );
        assert_eq!(
            swatch_class("tab-1").as_deref(),
            Some("d0-swatch sw-parquet"),
            "a .parquet tab wears the parquet swatch"
        );
        assert_eq!(
            swatch_class("tab-2"),
            None,
            "a tab with no file has no format to identify"
        );
    });
}

#[test]
#[serial]
fn a_tab_is_titled_by_its_file_rather_than_its_table() {
    with_settled_config(|| {
        let h = mount(three_tabs(), Some(0));
        assert_eq!(h.text_of(h.by_a11y_id("tab-0").unwrap()), "sales.csv");
        assert_eq!(h.text_of(h.by_a11y_id("tab-2").unwrap()), "scratch");
    });
}

// ── the responsive rule ──────────────────────────────────────────────────────

/// The body of the first `@media (...)` block whose condition contains `cond`.
fn media_block(cond: &str) -> &'static str {
    let at = APP_CSS
        .match_indices("@media")
        .find(|(i, _)| {
            let head = &APP_CSS[*i..APP_CSS[*i..].find('{').map(|o| i + o).unwrap_or(*i)];
            head.contains(cond)
        })
        .unwrap_or_else(|| panic!("app.css has no @media block matching {cond:?}"))
        .0;
    let open = at + APP_CSS[at..].find('{').expect("the block opens");
    let mut depth = 0usize;
    for (i, c) in APP_CSS[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &APP_CSS[open + 1..open + i];
                }
            }
            _ => {}
        }
    }
    panic!("the @media block matching {cond:?} is never closed");
}

#[test]
fn the_narrow_window_rule_shrinks_the_launcher_but_never_hides_it() {
    // The GPUI test drove a real resize across 1080px in both directions
    // because the compact branch built a different widget. There is no branch
    // now — one `@media` block re-sizes the slot — so the guarantee that
    // survives is that the narrow rule still leaves the launcher on screen.
    let narrow = media_block("max-width: 1080px");

    assert!(
        narrow.contains(".d0-search-slot"),
        "the narrow rule must still re-size the launcher slot"
    );
    assert!(
        narrow.contains("flex-basis: 168px"),
        "below the breakpoint the slot shrinks to the design's 168px"
    );
    // Whatever the block says about the launcher or the strip, it may not take
    // either away: below 1080px they are the only route to a command left.
    for rule in narrow.split('}') {
        let Some((selector, body)) = rule.split_once('{') else {
            continue;
        };
        if selector.contains("search") || selector.contains("tabstrip") {
            assert!(
                !body.contains("display: none"),
                "the narrow rule hides {selector:?}, which leaves a narrow \
                 window with no visible way to open the palette"
            );
        }
    }
    // The sidebar is the one thing the breakpoint does remove, and the
    // launcher's width is what replaces its alignment anchor.
    assert!(
        narrow.contains(".d0-sidebar"),
        "the breakpoint's actual job is hiding the sidebar"
    );
}

#[test]
fn the_launcher_is_as_wide_as_the_sidebar_it_aligns_with() {
    // 238px, from one token: the slot and the sidebar column must be the same
    // width or the alignment the design asks for is quietly 12px out.
    assert!(
        APP_CSS.contains("--d0-sidebar-w: 238px;"),
        "the sidebar width token must stay at the design's 238px"
    );
    assert!(
        APP_CSS.contains("flex: 0 0 var(--d0-sidebar-w)"),
        "the launcher slot must take its width from that token rather than \
         repeating the number"
    );
}
