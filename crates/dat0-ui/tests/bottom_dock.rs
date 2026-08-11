//! The SQL console as a pane (S4).
//!
//! Ported from `dat0-app/tests/bottom_dock.rs` (B8). Everything that suite
//! defended was about a `gpui-component` bottom `Dock`, and most of it was
//! defence against upstream owning writers dat0 did not:
//!
//! * `sql_console_visible` was a *derived* getter over `Dock::is_dock_open`,
//!   because upstream's own title-bar chevron flipped the dock behind the
//!   shell's back and a cached bool would then send the next ⌘⇧C the wrong
//!   way. The Dioxus console has one source of truth,
//!   `DockLayout::console_open`, and two writers — the pane chevron and the
//!   chord — that both write it. The desync test ports directly and is still
//!   the most valuable test in the file.
//! * The dock was mounted lazily so a user who never opened a console never
//!   saw upstream's 29px collapsed title bar. A closed console here is simply
//!   not rendered, so the claim ports as "no console nodes at all".
//! * `repeated_toggles_never_remount_the_dock` guarded `set_bottom_dock`'s
//!   per-call subscription leak. There is no such call; the guarantee that
//!   survives is the observable one — however often it is toggled, there is
//!   exactly one console pane, never two responding to the same chevron.
//! * `console_controls_stay_tab_reachable_inside_the_dock` guarded
//!   `TabPanel`'s double-registered `FocusId`. `sql_console/mod.rs` records
//!   that the trap is gone with the widget library: "the pane header is a
//!   button and the editor is a div, and each is a tab stop exactly once".
//!   Exactly once is the part that is still assertable here, and it is
//!   asserted.
//!
//! Hermeticity: every test pins `DAT0_CONFIG_DIR` at a fresh temp dir and seeds
//! `first_run_done`, because the shell reads both — without it the first-run
//! tour opens over the shell on any machine with no `settings.toml`, and the
//! suite would pass or fail according to whose home directory it ran in.
//! `#[serial]` because the env var is process-global.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use serial_test::serial;
use tempfile::TempDir;

use dat0_core::actions::builtin::register_all;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::{AppEvent, AppEventRx, AppEvents};
use dat0_core::session::dock_layout::DockLayout;
use dat0_core::settings::store::SettingsStore;
use dat0_ui::components::shell::Shell;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::dom::NodeKey;
use support::{Harness, Key, Modifiers};

// ── host ─────────────────────────────────────────────────────────────────────

/// The real `Shell`, its four window contexts, and a pump.
///
/// **Why the pump.** A chord resolves through `keys::Cascade` to an action id,
/// which the shell hands to `ActionRegistry::dispatch`; every descriptor posts
/// `AppEvent::RunAction` on the bus, and the window's own `use_future` drains
/// it into `router::route`. The harness never polls a task — `settle` only
/// re-renders — so a future-based drain would silently never run and every
/// chord test would pass by measuring nothing. `drive-pump` does exactly what
/// `App`'s drain does, on a click the test controls.
#[derive(Clone, PartialEq, Props, Default)]
struct HostProps {
    layout: DockLayout,
}

#[component]
fn Host(props: HostProps) -> Element {
    let mut ws = Workspace::provide();
    Theme::provide(None);
    use_context_provider(|| {
        let reg = ActionRegistry::new();
        register_all(&reg).expect("built-in actions register without conflict");
        reg
    });
    let (events, rx) = use_hook(|| {
        let (tx, rx) = AppEvents::channel();
        (tx, Rc::new(RefCell::new(rx)))
    });
    use_context_provider(|| events.clone());

    {
        let seed = props.layout.clone();
        use_hook(move || ws.layout.set(seed));
    }

    let pump_events = events.clone();
    let surface = use_context_provider(|| Signal::new(Option::<dat0_ui::router::Surface>::None));
    rsx! {
        Shell {}
        button {
            "data-a11y-id": "drive-pump",
            onclick: move |_| drain(&rx, ws, &pump_events, surface),
            "pump"
        }
    }
}

/// Perform every queued `RunAction`, exactly as `components::App` does.
fn drain(
    rx: &Rc<RefCell<AppEventRx>>,
    ws: Workspace,
    events: &AppEvents,
    surface: dat0_ui::router::SurfaceSlot,
) {
    let mut queued = Vec::new();
    while let Ok(ev) = rx.borrow_mut().try_recv() {
        queued.push(ev);
    }
    for ev in queued {
        if let AppEvent::RunAction { id, .. } = ev {
            assert!(
                dat0_ui::router::route(ws, events, surface, &id),
                "the router must claim {id} — an unrouted descriptor is the \
                 failure mode the router exists to make impossible"
            );
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
/// onboarding carousel when it is false, and this suite is about the console,
/// not about what is painted over it.
fn with_shell<R>(layout: DockLayout, f: impl FnOnce(&mut Harness) -> R) -> R {
    with_config_dir(|tmp| {
        let store = SettingsStore::with_path(tmp.path().join("settings.toml"));
        dat0_core::settings::set_first_run_done(&store, true).expect("seed first_run_done");

        let mut h = Harness::new(Host, HostProps { layout });
        h.settle();
        f(&mut h)
    })
}

/// Press a chord at the shell root and let the bus settle.
fn chord(h: &mut Harness, key: Key, mods: Modifiers) {
    h.key_global(key, mods);
    h.click("drive-pump");
}

/// ⌘⇧C on macOS, ⌃⇧C elsewhere — `keys::Cascade` picks the platform row, so
/// the test supplies the platform's own modifier.
fn console_toggle(h: &mut Harness) {
    let primary = if cfg!(target_os = "macos") {
        Modifiers::META
    } else {
        Modifiers::CONTROL
    };
    chord(h, Key::Character("c".into()), primary | Modifiers::SHIFT);
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn count_id(h: &Harness, id: &str) -> usize {
    h.dom()
        .walk()
        .into_iter()
        .filter(|k| h.dom().get(*k).attr("data-a11y-id") == Some(id))
        .count()
}

fn by_class(h: &Harness, class: &str) -> Option<NodeKey> {
    h.dom()
        .walk()
        .into_iter()
        .find(|k| h.dom().get(*k).attr("class") == Some(class))
}

/// The centre column's row template, where the console's height lives.
fn centre_track(h: &Harness) -> String {
    let k = by_class(h, "d0-centre").expect("the centre column is always rendered");
    h.attr(k, "style").expect("the centre sizes its tracks")
}

fn open() -> DockLayout {
    DockLayout {
        console_open: true,
        ..DockLayout::default()
    }
}

// ── the pane ─────────────────────────────────────────────────────────────────

#[test]
#[serial]
fn no_console_pane_until_the_console_is_first_opened() {
    with_shell(DockLayout::default(), |h| {
        assert_eq!(
            count_id(h, "pane-console"),
            0,
            "a freshly booted shell must have no console pane at all — B8 \
             mounted the dock lazily for exactly this reason, so the first-run \
             hero is not sitting on a collapsed title bar"
        );
        assert_eq!(count_id(h, "console-splitter"), 0);
        // One track, not a trailing 0px one — see `right_dock.rs`.
        assert_eq!(
            centre_track(h),
            "grid-template-rows: minmax(0, 1fr)",
            "and the grid keeps the whole centre"
        );

        console_toggle(h);

        assert_eq!(count_id(h, "pane-console"), 1, "the first ⌘⇧C mounts it");
    });
}

#[test]
#[serial]
fn the_console_shortcut_opens_and_closes_it() {
    with_shell(DockLayout::default(), |h| {
        console_toggle(h);
        assert_eq!(count_id(h, "pane-console"), 1, "open after one toggle");

        console_toggle(h);
        assert_eq!(count_id(h, "pane-console"), 0, "closed after two");

        console_toggle(h);
        assert_eq!(count_id(h, "pane-console"), 1, "open again after three");
    });
}

/// ⚠ The desync test, and the reason B8 derived `sql_console_visible` rather
/// than caching it.
///
/// Two writers reach the console's open state — the pane chevron and the
/// chord. If they did not share one value, a chevron collapse would leave the
/// chord's copy believing the console was open, and the next ⌘⇧C would toggle
/// that stale value: closing an already-closed console, i.e. reopening it,
/// exactly backwards from what the user asked for.
#[test]
#[serial]
fn the_chevron_and_the_shortcut_share_one_source_of_truth() {
    with_shell(open(), |h| {
        assert_eq!(count_id(h, "pane-console"), 1, "seed: open");

        // The chevron. Under GPUI this writer belonged to upstream and dat0
        // could not see it; here it is the pane's own header.
        h.click("pane-head-console");
        assert_eq!(
            count_id(h, "pane-console"),
            0,
            "the header chevron closes the console"
        );

        // Now the user presses the shortcut. It must OPEN, not close-again.
        console_toggle(h);
        assert_eq!(
            count_id(h, "pane-console"),
            1,
            "after a chevron close, the next chord must OPEN — a second cached \
             copy of the open state would have gone the other way"
        );
    });
}

/// B8's remount guard, restated as the thing a user could see.
///
/// `set_bottom_dock` leaked a subscription per call, so B8 asserted the dock
/// entity's identity never changed. There is no such call here; what would
/// still be a real defect is a second console pane accumulating behind the
/// first, both answering the same chevron.
#[test]
#[serial]
fn repeated_toggles_leave_exactly_one_console_pane() {
    with_shell(DockLayout::default(), |h| {
        console_toggle(h);
        assert_eq!(count_id(h, "pane-console"), 1);

        for _ in 0..4 {
            console_toggle(h);
            console_toggle(h);
        }

        assert_eq!(
            count_id(h, "pane-console"),
            1,
            "eight further toggles leave one pane, not a stack of them"
        );
        assert_eq!(
            count_id(h, "pane-head-console"),
            1,
            "and one header, so a chevron click cannot be ambiguous"
        );
        assert_eq!(count_id(h, "console-splitter"), 1);
    });
}

/// S4's pane header, on the console: the chevron, the `.d0-label` id, the
/// title and the right-aligned meta.
///
/// The id is the frozen name B8 pinned as `SqlConsole::PANEL_NAME`
/// (`"SqlConsolePanel"`), which was the `PanelRegistry` serialization
/// contract. There is no registry now; the pane's id is what the DOM, the
/// tests and `DockLayout` all key off, so it is the name that has to stay put.
#[test]
#[serial]
fn the_console_pane_is_headed_by_its_frozen_id_and_its_run_chord() {
    with_shell(open(), |h| {
        let head = h
            .by_a11y_id("pane-head-console")
            .expect("the console is a pane and a pane has a header");
        let text = h.text_of(head);

        assert!(text.contains('▾'), "the chevron is there: {text:?}");
        assert!(
            text.contains("console"),
            "the `.d0-label` id names the pane: {text:?}"
        );
        assert!(
            text.contains(&dat0_i18n::t("sql.editor")),
            "the title is the console's own: {text:?}"
        );
        assert!(
            text.contains("⌘⏎ run"),
            "and the meta carries the run chord, which is the console's only \
             header affordance: {text:?}"
        );

        // Exactly one, and operable. `sql_console/mod.rs` claims the header
        // "is a button and … a tab stop exactly once"; the count is the
        // assertable half of that, and it is what B8's `count_label(&run) == 1`
        // was really about — a duplicate name made `query_by_role` panic and
        // took whole suites down.
        assert_eq!(count_id(h, "pane-head-console"), 1);
        assert_eq!(
            h.dom().get(head).tag(),
            Some("button"),
            "the header is a real button, not a div with a click handler"
        );
        assert!(h.has_listener(head, "click"));
    });
}

/// B8's `a_closed_console_leaves_no_controls_in_the_tree`.
///
/// Upstream kept a closed bottom dock on screen at `h(px(29.))` so its title
/// bar stayed clickable, which is what made "does the collapsed bar still
/// contribute nodes" a real question. Here a closed console contributes
/// nothing at all, and that is the stronger claim: no phantom controls for a
/// screen reader to find.
#[test]
#[serial]
fn a_closed_console_leaves_no_console_nodes_in_the_tree() {
    with_shell(open(), |h| {
        assert_eq!(count_id(h, "pane-console"), 1, "seed: present while open");

        h.click("pane-head-console");

        for id in ["pane-console", "pane-head-console", "pane-body-console"] {
            assert_eq!(
                count_id(h, id),
                0,
                "a closed console must leave no {id} behind"
            );
        }
    });
}

#[test]
#[serial]
fn the_console_takes_height_from_the_centre_only_while_it_is_open() {
    with_shell(open(), |h| {
        let opened = centre_track(h);
        assert!(
            !opened.contains("1fr) 0px"),
            "an open console has real height: {opened}"
        );

        h.click("pane-head-console");

        assert_eq!(
            centre_track(h),
            "grid-template-rows: minmax(0, 1fr)",
            "and closing it gives every pixel back to the grid"
        );
    });
}

/// The console splitter gets a row of its own, so the console keeps its height.
///
/// Three children — pane stack, splitter, console — into a two-row template
/// meant the splitter took the console's 260px row and the console was
/// auto-placed into an implicit third row, 99px tall. Same defect as
/// `.d0-shell`'s and `.d0-workarea`'s; pinned by track count for the same
/// reason, because "not 0px" cannot see it.
#[test]
#[serial]
fn the_open_console_declares_a_track_for_every_child() {
    with_shell(open(), |h| {
        assert_eq!(
            centre_track(h),
            "grid-template-rows: minmax(0, 1fr) 4px 260px",
            "pane stack, splitter and console are three children and need \
             three rows — with two, the console wraps into an implicit one"
        );
    });
}

/// The console's own resize edge — event-driven, not the per-frame
/// `DockArea::dump()` diff the GPUI shell needed.
#[test]
#[serial]
fn an_open_console_carries_a_splitter_that_starts_a_drag() {
    with_shell(open(), |h| {
        let splitter = h
            .by_a11y_id("console-splitter")
            .expect("an open console has a resize edge");
        assert_eq!(h.attr(splitter, "role"), Some("separator".to_string()));
        assert_eq!(
            h.attr(splitter, "aria-orientation"),
            Some("horizontal".to_string()),
            "a console edge is a horizontal separator"
        );
        assert!(
            h.has_listener(splitter, "mousedown"),
            "and it is wired — a splitter with no gesture is decoration"
        );
    });
}
