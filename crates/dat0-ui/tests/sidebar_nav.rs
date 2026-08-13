//! The sidebar's own guarantees (S1/S8) — the surface that did not exist under
//! GPUI, and therefore had no coverage to port.
//!
//! The old shell had an activity rail with three mutually exclusive modes; the
//! questions below could not even be asked of it:
//!
//! * are all three sections there at once, on a cold launch with nothing in
//!   any of them?
//! * does an empty section still say what it is and that it is empty?
//! * does a file's format swatch (S8) follow its extension, everywhere?
//! * does ⌘B survive a reopen, or does every launch reset the column?
//!
//! Everything here drives the real `Shell`, because the sidebar's rows come
//! from the shell's own catalog and its open/closed bit from `DockLayout` —
//! mounting the widget alone would assert a fixture instead of the product.

mod support;

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;

use dioxus::prelude::*;
use serial_test::serial;
use tempfile::TempDir;

use support::{Harness, Key};

use dat0_core::actions::builtin::register_all;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::{AppEvent, AppEvents};
use dat0_core::session::dock_layout::DockLayout;
use dat0_ui::components::shell::Shell;
use dat0_ui::state::{SECTION_FILES, SECTIONS, SIDEBAR_WIDTH, TabView, Workspace};
use dat0_ui::theme::Theme;

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    /// The layout a reopened window would restore from the session.
    layout: DockLayout,
    tabs: Vec<TabView>,
}

/// The real `Shell` with `components::App`'s four contexts. `pump` stands in
/// for the async event-bus drain the harness has no runtime for; the chord
/// table, the registry and the router are all production code.
#[component]
fn Host(props: HostProps) -> Element {
    let mut ws = Workspace::provide();
    Theme::provide(None);

    let registry = use_hook(|| {
        let reg = ActionRegistry::new();
        register_all(&reg).expect("built-in actions register without conflict");
        reg
    });
    use_context_provider(|| registry.clone());

    let (events, rx) = use_hook(|| {
        let (tx, rx) = AppEvents::channel();
        (tx, Rc::new(RefCell::new(rx)))
    });
    use_context_provider(|| events.clone());

    let (layout, tabs) = (props.layout.clone(), props.tabs.clone());
    use_hook(move || {
        ws.layout.set(layout);
        if !tabs.is_empty() {
            ws.active.set(Some(0));
            ws.tabs.set(tabs);
        }
    });

    let pump_events = events.clone();
    let restored = ws.layout.read().clone();
    let surface = use_context_provider(|| Signal::new(Option::<dat0_ui::router::Surface>::None));
    rsx! {
        Shell {}
        button {
            "data-a11y-id": "pump",
            onclick: move |_| {
                while let Ok(ev) = rx.borrow_mut().try_recv() {
                    if let AppEvent::RunAction { id, .. } = ev {
                        assert!(
                            dat0_ui::router::route(ws, &pump_events, surface, id),
                            "no handler claimed {id}"
                        );
                    }
                }
            },
        }
        // What a session would write back on close.
        div {
            "data-a11y-id": "layout",
            "{restored.sidebar_open} [{restored.sections_collapsed.iter().cloned().collect::<Vec<_>>().join(\",\")}]"
        }
    }
}

fn mount(layout: DockLayout, tabs: Vec<TabView>) -> Harness {
    let mut h = Harness::new(Host, HostProps { layout, tabs });
    h.settle();
    h
}

fn boot() -> Harness {
    mount(DockLayout::default(), Vec::new())
}

fn tab(path: &str) -> TabView {
    TabView {
        table: path.replace(['/', '.'], "_"),
        path: Some(PathBuf::from(path)),
    }
}

fn toggle_sidebar(h: &mut Harness) {
    h.key_at("window", Key::Character("b".into()), support::primary());
    h.click("pump");
}

/// The layout the window would hand back to the session right now.
fn persisted(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("layout").expect("the layout readback"))
}

/// Is `key` inside the subtree rooted at `ancestor`?
fn within(h: &Harness, ancestor: support::dom::NodeKey, mut key: support::dom::NodeKey) -> bool {
    while let Some(parent) = h.dom().get(key).parent {
        if parent == ancestor {
            return true;
        }
        key = parent;
    }
    false
}

/// Every swatch class **in the sidebar**, in paint order.
///
/// Scoped deliberately: S8 puts the same swatch on tab titles, so an unscoped
/// query would pass on the tab strip's copy while the sidebar drew nothing.
fn swatches(h: &Harness) -> Vec<String> {
    let sidebar = h.by_a11y_id("sidebar").expect("the sidebar");
    h.dom()
        .walk()
        .into_iter()
        .filter(|k| within(h, sidebar, *k))
        .filter_map(|k| h.attr(k, "class"))
        .filter(|c| c.starts_with("d0-swatch "))
        .map(|c| c.trim_start_matches("d0-swatch ").to_string())
        .collect()
}

/// Run `f` against a private config dir with the first-run tour already
/// answered, so the shell renders the plain hero rather than the carousel and
/// nothing depends on the developer's own `settings.toml`.
fn hermetic<R>(f: impl FnOnce() -> R) -> R {
    let tmp = TempDir::new().unwrap();
    let previous = std::env::var_os("DAT0_CONFIG_DIR");
    // SAFETY: `#[serial]` keeps every env-touching test off the same clock, and
    // nothing else in this binary reads the variable concurrently.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", tmp.path()) };
    dat0_core::settings::set_first_run_done(
        &dat0_core::settings::store::SettingsStore::with_path(tmp.path().join("settings.toml")),
        true,
    )
    .unwrap();
    let out = f();
    unsafe {
        match previous {
            Some(v) => std::env::set_var("DAT0_CONFIG_DIR", v),
            None => std::env::remove_var("DAT0_CONFIG_DIR"),
        }
    }
    out
}

/// The width the shell's body grid reserves for the sidebar column.
fn column_style(h: &Harness) -> String {
    let shell = h
        .dom()
        .walk()
        .into_iter()
        .find(|k| h.attr(*k, "class").as_deref() == Some("d0-shell"))
        .expect("the shell grid");
    h.attr(shell, "style").unwrap_or_default()
}

#[test]
#[serial]
fn the_three_sections_stand_together_on_a_cold_launch() {
    // Nothing has been opened, attached or sealed. The shell's shape must still
    // be its final shape: a section that appeared on first use would move every
    // row below it and would never teach anyone that it exists.
    hermetic(|| {
        let h = boot();
        assert!(h.by_a11y_id("sidebar").is_some());
        for section in SECTIONS {
            assert!(
                h.by_a11y_id(&format!("section-{section}")).is_some(),
                "{section} must be present with nothing in it"
            );
        }
    });
}

#[test]
#[serial]
fn an_empty_section_says_so_instead_of_showing_a_bare_heading() {
    hermetic(|| {
        let h = boot();
        for section in SECTIONS {
            let row = h
                .by_a11y_id(&format!("empty-{section}"))
                .unwrap_or_else(|| panic!("{section} must paint its empty-state row"));
            let expected = dat0_i18n::t(&format!("catalog.empty.{section}"));
            assert_ne!(
                expected,
                format!("catalog.empty.{section}"),
                "the key must resolve in en.json"
            );
            assert_eq!(
                h.text_of(row),
                expected,
                "an empty section states its emptiness; a bare heading reads as \
                 a rendering failure"
            );
        }
    });
}

#[test]
#[serial]
fn a_file_row_wears_the_swatch_for_its_extension() {
    // S8: one swatch vocabulary, driven off the extension, so a `.parquet` is
    // the same purple in the sidebar, in a chip and on a tab title.
    hermetic(|| {
        let h = mount(
            DockLayout::default(),
            vec![
                tab("/data/sales.csv"),
                tab("/data/events.parquet"),
                tab("/data/chinook.db"),
                tab("/data/log.json"),
                tab("/data/q2.dat0"),
            ],
        );
        assert_eq!(
            swatches(&h),
            vec!["sw-csv", "sw-parquet", "sw-sqlite", "sw-json", "sw-dat0"],
            "each FILES row carries the swatch its extension maps to, in paint \
             order"
        );
    });
}

#[test]
#[serial]
fn an_unrecognised_extension_still_gets_a_swatch() {
    // The fallback is a class, not the absence of one: a row with no square
    // would sit 7px left of every other row.
    hermetic(|| {
        let h = mount(DockLayout::default(), vec![tab("/data/notes.txt")]);
        assert_eq!(swatches(&h), vec!["sw-other"]);
    });
}

#[test]
#[serial]
fn the_sidebar_opens_at_the_design_width_by_default() {
    // 238px is also the tab strip's search-gutter width; the two are aligned by
    // sharing this number, so a drift here is visible as a misaligned shell.
    hermetic(|| {
        let style = column_style(&boot());
        assert!(
            style.contains(&format!("{SIDEBAR_WIDTH}px")),
            "the body grid must reserve the sidebar's column: {style}"
        );
    });
}

#[test]
#[serial]
fn cmd_b_is_what_a_reopened_window_restores() {
    // The bit the session carries. Toggling must change what a reopen would
    // read, or ⌘B is a per-launch preference the user has to re-apply forever.
    hermetic(|| {
        let mut h = boot();
        assert!(
            persisted(&h).starts_with("true"),
            "a fresh window is open: {}",
            persisted(&h)
        );

        toggle_sidebar(&mut h);
        assert!(
            persisted(&h).starts_with("false"),
            "⌘B must be recorded in DockLayout, not held in the component: {}",
            persisted(&h)
        );

        // What the next launch does with it: same shell, restored layout, no
        // sidebar — and no keystroke needed to get there.
        let mut reopened = mount(
            DockLayout {
                sidebar_open: false,
                ..DockLayout::default()
            },
            Vec::new(),
        );
        assert!(
            reopened.by_a11y_id("sidebar").is_none(),
            "a window reopened with sidebar_open=false must come up collapsed"
        );

        // And the round trip closes: ⌘B in the reopened window brings it back.
        toggle_sidebar(&mut reopened);
        assert!(reopened.by_a11y_id("sidebar").is_some());
    });
}

#[test]
#[serial]
fn a_collapsed_section_is_what_a_reopened_window_restores() {
    hermetic(|| {
        let mut h = boot();
        h.click(&format!("section-toggle-{SECTION_FILES}"));
        assert_eq!(
            persisted(&h),
            format!("true [{SECTION_FILES}]"),
            "section collapse belongs to the persisted layout, not to the widget"
        );

        let reopened = mount(
            DockLayout {
                sections_collapsed: BTreeSet::from([SECTION_FILES.to_string()]),
                ..DockLayout::default()
            },
            vec![tab("/data/sales.csv")],
        );
        assert!(
            reopened.by_a11y_id("row-files-0").is_none(),
            "FILES comes back collapsed"
        );
        assert!(
            reopened.by_a11y_id("empty-connections").is_some(),
            "and only FILES does"
        );
    });
}

#[test]
#[serial]
fn a_restored_width_survives_the_reopen_too() {
    // `sidebar_size` is `None` until the user drags the splitter; a restored
    // window must honour the dragged width rather than snapping back to 238.
    hermetic(|| {
        let style = column_style(&mount(
            DockLayout {
                sidebar_size: Some(300),
                ..DockLayout::default()
            },
            Vec::new(),
        ));
        assert!(style.contains("300px"), "got {style}");
    });
}
