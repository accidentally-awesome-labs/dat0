//! What became of the left dock (S1).
//!
//! The GPUI build put Catalog, Connections and AI in one `DockItem::tabs`
//! selected by an activity rail, and its central invariant was that **at most
//! one** of them could be visible — two would make upstream paint a second
//! selector beside the rail. The redesign deletes the rail and the mode switch:
//! one always-present 238px sidebar shows all three sections at once, each
//! individually collapsible, and ⌘B hides the whole column.
//!
//! So the old invariant is not weakened here, it is **inverted**: the tests that
//! guarded "never two at once" are replaced by one that requires all three, and
//! the tests that drove the mode switch (`activate_left_panel`, the three a11y
//! shims, the rail cursor) are gone with the surface they drove. Everything
//! else the suite proved — the dock exists, it names itself exactly once, its
//! body's content really renders inside it, collapsing does not orphan the
//! keyboard, and a hidden dock is not a Tab stop — is asserted below against
//! the sidebar.
//!
//! ⌘B is driven end to end: keymap chord → `Cascade` → `ActionRegistry`
//! descriptor → `AppEvent::RunAction` → `router::route`. Dispatching the router
//! directly would let a dead key path ship green, which is how a broken Escape
//! ladder once got past five reviews.
//!
//! Hermeticity: `DAT0_CONFIG_DIR` points at a fresh temp dir with
//! `first_run_done` seeded, because the shell reads it to decide whether to
//! auto-open the tour; `#[serial]`, because that variable is process-global.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use serial_test::serial;
use tempfile::TempDir;

use support::{Harness, Key};

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::{AppEvent, AppEventRx, AppEvents};
use dat0_ui::components::shell::Shell;
use dat0_ui::state::{SECTIONS, TabView, Workspace};
use dat0_ui::theme::Theme;

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    tabs: Vec<TabView>,
}

/// The real `Shell`, with the four contexts `components::App` provides.
///
/// The `pump` button stands in for `App`'s event-bus drain, which is an async
/// task the headless harness has no runtime for. Everything else — the chord
/// table, the registry descriptor, the router — is production code.
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

    let seed = props.tabs.clone();
    use_hook(move || {
        if !seed.is_empty() {
            ws.active.set(Some(0));
            ws.tabs.set(seed);
        }
    });

    let pump_events = events.clone();
    let surface = use_context_provider(|| Signal::new(Option::<dat0_ui::router::Surface>::None));
    rsx! {
        Shell {}
        button {
            "data-a11y-id": "pump",
            onclick: move |_| pump(&rx, ws, &pump_events, surface),
        }
    }
}

fn pump(
    rx: &Rc<RefCell<AppEventRx>>,
    ws: Workspace,
    events: &AppEvents,
    surface: dat0_ui::router::SurfaceSlot,
) {
    while let Ok(ev) = rx.borrow_mut().try_recv() {
        if let AppEvent::RunAction { id, .. } = ev {
            assert!(
                dat0_ui::router::route(ws, events, surface, id),
                "no handler claimed {id}"
            );
        }
    }
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

fn boot() -> Harness {
    boot_with(Vec::new())
}

fn boot_with_tabs() -> Harness {
    boot_with(vec![TabView {
        table: "t_1".into(),
        path: Some(std::path::PathBuf::from("/data/sales.csv")),
    }])
}

fn boot_with(tabs: Vec<TabView>) -> Harness {
    let mut h = Harness::new(Host, HostProps { tabs });
    h.settle();
    h
}

/// Press the sidebar chord on the shell root and let the bus reach the router.
fn toggle_sidebar(h: &mut Harness) {
    h.key_at("window", Key::Character("b".into()), support::primary());
    h.click("pump");
}

fn heading(section: &str) -> String {
    dat0_i18n::t(&format!("catalog.group.{section}"))
}

fn is_tab_stop(h: &Harness, id: &str) -> bool {
    h.tab_order()
        .iter()
        .any(|k| h.attr(*k, "data-a11y-id").as_deref() == Some(id))
}

#[test]
#[serial]
fn a_fresh_workspace_shows_the_sidebar() {
    // The GPUI twin asserted the opposite — a fresh workspace showed NO left
    // panel, because the rail had no default selection. A workbench whose
    // catalog is hidden until you find a chord looks broken, so `DockLayout`
    // defaults `sidebar_open` on.
    hermetic(|| {
        let h = boot();
        assert!(h.by_a11y_id("sidebar").is_some());
        assert_eq!(
            h.count_label(&dat0_i18n::t("catalog.title")),
            1,
            "the sidebar names itself exactly once — a duplicate mount would \
             make every label query ambiguous"
        );
    });
}

#[test]
#[serial]
fn all_three_sections_are_visible_at_once() {
    // The inversion of the B7 invariant. The old dock could show one of
    // Catalog / Connections / AI; the sidebar must show FILES, CONNECTIONS and
    // PACKAGES together, because picking a table, then a connection, then a
    // package was the most-repeated interaction the mode switch made expensive.
    hermetic(|| {
        let h = boot();
        for section in SECTIONS {
            assert!(
                h.by_a11y_id(&format!("section-{section}")).is_some(),
                "{section} must be present"
            );
            assert_eq!(
                h.count_label(&heading(section)),
                1,
                "{section} must be named exactly once"
            );
        }
    });
}

#[test]
#[serial]
fn cmd_b_hides_the_sidebar_and_brings_it_back() {
    hermetic(|| {
        let mut h = boot();
        assert!(h.by_a11y_id("sidebar").is_some());

        toggle_sidebar(&mut h);
        assert!(
            h.by_a11y_id("sidebar").is_none(),
            "⌘B must remove the column, not merely narrow it — a closed dock \
             that still reserves pixels is what S5 exists to stop"
        );

        toggle_sidebar(&mut h);
        assert!(h.by_a11y_id("sidebar").is_some(), "⌘B is a toggle");
    });
}

#[test]
fn the_sidebar_toggle_is_a_registered_action() {
    // The seam between the chord and the router: `Cascade` resolves ⌘B to an
    // id, and the registry must actually carry it. An id with no descriptor
    // dispatches to nothing, silently.
    let reg = ActionRegistry::new();
    register_all(&reg).expect("register");
    assert!(reg.contains(ids::SIDEBAR_TOGGLE));

    let cascade = dat0_ui::keys::Cascade::default();
    assert_eq!(
        cascade.resolve(&Key::Character("b".into()), support::primary()),
        Some(ids::SIDEBAR_TOGGLE),
        "the sidebar chord must resolve through the real keymap table"
    );
}

#[test]
#[serial]
fn a_hidden_sidebar_is_not_a_tab_stop() {
    // The GPUI rule for a hidden panel, kept: a surface nobody can see must not
    // be somewhere Tab can land, or the keyboard user's focus vanishes into it.
    hermetic(|| {
        let mut h = boot();
        assert!(
            is_tab_stop(&h, "catalog-tree"),
            "the catalog tree is a Tab stop while the sidebar is showing"
        );

        toggle_sidebar(&mut h);
        assert!(h.by_a11y_id("catalog-tree").is_none());
        assert!(
            !is_tab_stop(&h, "catalog-tree"),
            "a hidden sidebar must not be Tab-reachable"
        );
    });
}

#[test]
#[serial]
fn the_sidebar_body_renders_its_own_content() {
    // The GPUI twin's delegation probe: the dock could paint a correct title
    // bar over an empty body and every title assertion would still pass. Same
    // guard — the section headings and a real row have to be INSIDE the
    // sidebar's subtree, not merely somewhere in the window.
    hermetic(|| {
        let h = boot_with_tabs();
        let body = h.text_of(h.by_a11y_id("sidebar").expect("sidebar"));
        for section in SECTIONS {
            assert!(
                body.contains(&heading(section)),
                "missing {section}: {body}"
            );
        }
        assert!(
            body.contains("sales.csv"),
            "the open tab's file must appear as a FILES row: {body}"
        );
    });
}

#[test]
#[serial]
fn each_section_collapses_independently() {
    // The whole point of replacing the mode switch: collapsing FILES must not
    // touch CONNECTIONS. Under the old dock this was unrepresentable — opening
    // one panel closed the others by construction.
    hermetic(|| {
        let mut h = boot_with_tabs();
        assert!(h.by_a11y_id("row-files-0").is_some());

        h.click("section-toggle-files");
        assert!(
            h.by_a11y_id("row-files-0").is_none(),
            "the collapsed section drops its rows"
        );
        assert_eq!(expanded(&h, "files").as_deref(), Some("false"));
        for section in ["connections", "packages"] {
            assert!(
                h.by_a11y_id(&format!("empty-{section}")).is_some(),
                "{section} is untouched by collapsing FILES"
            );
            assert_eq!(expanded(&h, section).as_deref(), Some("true"));
        }
    });
}

fn expanded(h: &Harness, section: &str) -> Option<String> {
    let toggle = h
        .by_a11y_id(&format!("section-toggle-{section}"))
        .unwrap_or_else(|| panic!("no toggle for {section}"));
    h.attr(toggle, "aria-expanded")
}

#[test]
#[serial]
fn collapsing_a_section_leaves_the_keyboard_somewhere_live() {
    // Design §10 R7, carried over: collapsing must not orphan focus. The rail
    // survived because it was never unmounted; the tree container survives for
    // the same reason — only the rows inside it come and go.
    hermetic(|| {
        let mut h = boot_with_tabs();
        h.click("section-toggle-files");
        assert!(
            h.by_a11y_id("catalog-tree").is_some(),
            "the tree container is never unmounted by a section collapse"
        );
        assert!(
            is_tab_stop(&h, "catalog-tree"),
            "and it is still the Tab stop the user activated from"
        );
    });
}

#[test]
#[serial]
fn the_sidebar_footer_states_the_session_and_its_egress() {
    // S1's three footer lines. `egress 0 B` is always shown rather than hidden
    // at zero: "no data left this machine" is the claim dat0 makes, and a claim
    // you only see when it is broken is not a claim.
    hermetic(|| {
        let h = boot();
        let text = h.text_of(h.by_a11y_id("sidebar").expect("sidebar"));
        assert!(text.contains("session · 1 window"), "got {text}");
        assert!(text.contains("egress 0 B"), "got {text}");
    });
}
