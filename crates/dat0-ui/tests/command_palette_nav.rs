//! The command palette in its production shape: opened by the real chord, over
//! the real shell, against the registry the app actually ships.
//!
//! `tests/command_palette.rs` mounts the palette on its own and proves what the
//! surface owns — ranking reaching the DOM, the ring, Enter, Escape, the click
//! paths, the windowing. Everything here is a guarantee that only exists once
//! the palette is part of a window:
//!
//! - the chord opens it, through `Cascade` and the one keymap table;
//! - and does **not** open it while a dialog owns the screen, because ⌘⇧P is a
//!   GLOBAL row and would otherwise stack a second overlay on a modal — the
//!   `debug_assert!` panic in debug and an untrapped overlay in release;
//! - an arrow resolves from the results list as well as from the query field,
//!   the pair of focus stops the GPUI suite kept two tests for;
//! - Tab does not leak into the surface the palette obscures;
//! - and an action a row runs reaches the shell's router, which is what
//!   replaced the seven `WINDOW_ROUTED` breadcrumbs.
//!
//! Ported from `dat0-app/tests/command_palette_nav.rs`. Two of its scaffolding
//! problems are gone rather than solved: there is no `dispatch_action` escape
//! hatch for Enter (a plain `<input>` does not insert a newline behind an
//! action, so `tests/command_palette.rs` drives Enter with a real keystroke),
//! and there are no process-wide singletons to install — the registry and the
//! bus are context, so the host provides them.

mod support;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::ids;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_ui::components::command_palette::{PaletteKey, palette_key};
use dat0_ui::components::shell::Shell;
use dat0_ui::keys::Cascade;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::{Harness, Key, Modifiers};

/// Point `config_dir()` at a scratch directory for the body of `f`, then put
/// the environment back.
///
/// Copied from `tests/onboarding.rs`. `DAT0_CONFIG_DIR` is process-global, so
/// every test that touches it is `#[serial]`; a Shell mount that read the
/// developer's own `settings.toml` would decide from an unwritten file whether
/// a dialog is in front of the palette.
fn with_config_dir<R>(f: impl FnOnce(&tempfile::TempDir) -> R) -> R {
    let tmp = tempfile::TempDir::new().unwrap();
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

/// Put the first run behind us. Every test here wants a shell with nothing in
/// front of it — except `the_chord_is_inert_while_a_modal_is_open`, which
/// wants the opposite and is the one test that does not call this.
fn seed_first_run_done(dir: &tempfile::TempDir) {
    let store =
        dat0_core::settings::store::SettingsStore::with_path(dir.path().join("settings.toml"));
    dat0_core::settings::set_first_run_done(&store, true).expect("seed first_run_done");
}

/// The shell over the REAL built-in registry — the palette must list the
/// commands the app ships, not three fixtures.
#[component]
fn Host() -> Element {
    let ws = Workspace::provide();
    Theme::provide(None);
    let registry = use_hook(|| {
        let reg = ActionRegistry::new();
        dat0_core::actions::builtin::register_all(&reg).expect("the built-ins register");
        reg
    });
    use_context_provider(|| registry);
    let (events, rx) = AppEvents::channel();
    use_hook(|| std::rc::Rc::new(rx));
    use_context_provider(|| events.clone());

    // The router probe. Production reaches `route` from the bus drain in
    // `components::App`; the harness does not poll futures, so the call is made
    // from a click instead — the same function, the same workspace.
    let mut routed = use_signal(|| Option::<bool>::None);
    let (console_events, unknown_events) = (events.clone(), events.clone());
    // The shell writes its command handler here; `route` falls through to it
    // for the ids whose state the shell owns.
    let surface = use_context_provider(|| Signal::new(Option::<dat0_ui::router::Surface>::None));

    rsx! {
        Shell {}
        button {
            "data-a11y-id": "route-console",
            onclick: move |_| {
                routed.set(Some(dat0_ui::router::route(ws, &console_events, surface, ids::CONSOLE_TOGGLE)));
            },
        }
        button {
            "data-a11y-id": "route-nothing",
            onclick: move |_| {
                routed
                    .set(Some(dat0_ui::router::route(ws, &unknown_events, surface, "nothing.claims.this")));
            },
        }
        div { "data-a11y-id": "routed", "{routed:?}" }
    }
}

/// Mount and let the first frame's own state writes land — the auto-opened
/// onboarding carousel is one, and a shell that had not settled would still be
/// carrying the cascade it built before the dialog existed.
fn shell() -> Harness {
    let mut h = Harness::new(Host, ());
    h.settle();
    h
}

/// Press the real chord, at the shell root where the cascade lives.
fn press_palette_chord(h: &mut Harness) {
    let mods = support::primary() | Modifiers::SHIFT;
    // Shift really does deliver an uppercase character; the keymap lowercases.
    h.key_at("window", Key::Character("P".into()), mods);
}

fn palette_is_open(h: &Harness) -> bool {
    h.by_a11y_id("palette").is_some()
}

/// The indices of the rows currently in the DOM. The list is windowed, so this
/// is a window over the registry, not the whole of it.
fn row_ids(h: &Harness) -> Vec<usize> {
    let mut out: Vec<usize> = h
        .dom()
        .walk()
        .into_iter()
        .filter_map(|k| {
            h.dom()
                .get(k)
                .attr("data-a11y-id")?
                .strip_prefix("palette-row-")
                .and_then(|n| n.parse().ok())
        })
        .collect();
    out.sort_unstable();
    out
}

fn selected(h: &Harness) -> Option<usize> {
    row_ids(h).into_iter().find(|i| {
        let k = h.by_a11y_id(&format!("palette-row-{i}")).unwrap();
        h.attr(k, "aria-selected").as_deref() == Some("true")
    })
}

/// The cascade the shell builds while the palette is up.
fn palette_cascade() -> Cascade {
    Cascade {
        palette_open: true,
        ..Cascade::default()
    }
}

// ── the chord ───────────────────────────────────────────────────────────────

/// A settled shell with the first run behind it — nothing in front of the
/// palette, which is what every test but the modal one wants.
fn returning_shell(dir: &tempfile::TempDir) -> Harness {
    seed_first_run_done(dir);
    shell()
}

#[test]
#[serial]
fn the_chord_opens_the_palette_from_the_shell() {
    with_config_dir(|dir| {
        let mut h = returning_shell(dir);
        assert!(!palette_is_open(&h), "precondition: nothing is up");

        press_palette_chord(&mut h);

        assert!(palette_is_open(&h), "⌘⇧P must mount the palette");
        assert!(
            h.query_by_role("dialog", &dat0_i18n::t("palette.title")),
            "and the thing it mounted is the palette's own dialog"
        );
        assert!(
            !row_ids(&h).is_empty(),
            "an open palette over the real registry lists commands"
        );
    });
}

/// ⌘⇧P is a GLOBAL row, so it fires while a dialog owns the screen too.
/// Mounting the palette on top would make two overlays, the second one
/// untrapped.
#[test]
#[serial]
fn the_chord_is_inert_while_a_modal_is_open() {
    // The one test here that does NOT seed `first_run_done`: an unseeded
    // scratch dir is a first run, and a first-run window opens the onboarding
    // carousel by itself.
    with_config_dir(|_dir| {
        let mut h = shell();

        // If the tour ever stops opening itself, the user-visible route to the
        // same dialog is the hero's own button. Either way, what this test
        // needs is a real dialog in front of the shell.
        if h.by_a11y_id("modal").is_none() {
            h.click("hero-take-tour");
        }
        assert!(
            h.by_a11y_id("modal").is_some(),
            "precondition: a dialog owns the screen"
        );

        press_palette_chord(&mut h);

        assert!(
            !palette_is_open(&h),
            "the palette must not stack on top of a modal"
        );
        assert!(
            h.by_a11y_id("modal").is_some(),
            "and the modal that owns the screen keeps it"
        );
    });
}

/// The GPUI original also asserted that focus returned to where it had been.
/// That half is a browser fact — `modals::RELEASE_JS` is how a real window does
/// it — and the headless harness owns its own focus cursor, so what is provable
/// here is the residue that matters as much: the shell is still listening. A
/// dismissed palette that left the keyboard somewhere unreachable would make
/// the second chord do nothing.
#[test]
#[serial]
fn escape_dismisses_the_palette_and_the_shell_keeps_the_keyboard() {
    with_config_dir(|dir| {
        let mut h = returning_shell(dir);
        press_palette_chord(&mut h);
        assert!(palette_is_open(&h));

        h.key_at("palette", Key::Escape, Modifiers::empty());
        assert!(!palette_is_open(&h), "Escape must dismiss");

        press_palette_chord(&mut h);
        assert!(
            palette_is_open(&h),
            "the shell root still resolves the chord after a dismissal"
        );
    });
}

// ── the two focus stops ─────────────────────────────────────────────────────

/// The GPUI palette needed a test per focus stop because the two resolved Down
/// through different mechanisms: with the query field focused upstream's
/// `MoveDown` was chosen first and only fell through to dat0's because a
/// single-line `Input` registered no handler for it, while with the results
/// list focused the "Input" key context was absent entirely. Here one handler
/// on the panel serves both stops — so the guarantee to keep is that a
/// keystroke raised anywhere inside the palette reaches it.
#[test]
#[serial]
fn an_arrow_raised_on_the_results_list_moves_the_ring_too() {
    with_config_dir(|dir| {
        let mut h = returning_shell(dir);
        press_palette_chord(&mut h);
        assert_eq!(selected(&h), Some(0), "the ring starts on the first row");

        let list = h.by_a11y_id("palette-list").expect("the results list");
        h.key(list, Key::ArrowDown, Modifiers::empty());
        assert_eq!(selected(&h), Some(1), "an arrow at the list stop moves it");

        let row = h.by_a11y_id("palette-row-1").expect("the selected row");
        h.key(row, Key::ArrowDown, Modifiers::empty());
        assert_eq!(
            selected(&h),
            Some(2),
            "…and so does one raised on a row, where a click leaves focus"
        );
    });
}

/// WCAG 2.4.3. The GPUI palette inherited this for free by living in
/// `mounted_modals`; the Dioxus palette is deliberately not in the modal slot,
/// so it owns the trap itself.
///
/// Its stops are one — the query field — because the rows are `tabindex="-1"`
/// and are reached with the arrows. Consuming Tab therefore *is* the cycle:
/// there is nowhere else inside to send focus, and the shell behind must not
/// be it.
#[test]
#[serial]
fn tab_never_leaves_the_open_palette() {
    assert_eq!(
        palette_key(palette_cascade(), &Key::Tab, Modifiers::empty()),
        PaletteKey::Trap
    );
    assert_eq!(
        palette_key(palette_cascade(), &Key::Tab, Modifiers::SHIFT),
        PaletteKey::Trap,
        "backwards out of the first stop is the escape a forward trap misses"
    );
    // The negative control: a chord the palette does not own still belongs to
    // the shell, or Undo would be dead whenever the palette happened to be up.
    assert_eq!(
        palette_key(
            palette_cascade(),
            &Key::Character("z".into()),
            Modifiers::META
        ),
        PaletteKey::Fallthrough
    );

    // And the structure the trap assumes: nothing inside the panel is a Tab
    // stop of its own, so holding focus where it is holds it inside.
    with_config_dir(|dir| {
        let mut h = returning_shell(dir);
        press_palette_chord(&mut h);

        let panel = h.by_a11y_id("palette").expect("the panel");
        assert_eq!(h.attr(panel, "tabindex").as_deref(), Some("-1"));
        for i in row_ids(&h) {
            let row = h.by_a11y_id(&format!("palette-row-{i}")).unwrap();
            assert_eq!(
                h.attr(row, "tabindex").as_deref(),
                Some("-1"),
                "row {i} is a Tab stop, so the single-stop trap would strand \
                 focus"
            );
        }

        h.key(panel, Key::Tab, Modifiers::empty());
        assert!(
            palette_is_open(&h),
            "Tab is consumed, not treated as a dismissal"
        );
        assert_eq!(selected(&h), Some(0), "and it does not move the ring");
    });
}

// ── the router ──────────────────────────────────────────────────────────────
//
// The GPUI suite gated a list called `WINDOW_ROUTED`: seven ids whose registry
// closure was a breadcrumb, because `DispatchFn`'s `Fn(&mut App)` could not
// reach a `Window`, and which the shell had to special-case. The list is gone —
// every descriptor posts the same `AppEvent::RunAction` and `router::route`
// performs it — so what survives is the defect the list existed to prevent: a
// palette row that runs nothing.

#[test]
#[serial]
fn running_console_toggle_from_the_palette_mounts_the_console() {
    // The payoff test. Before the router this id logged "handled view-scoped
    // (needs Window); no-op from App path" and nothing happened.
    with_config_dir(|dir| {
        let mut h = returning_shell(dir);
        assert!(
            h.by_a11y_id("pane-console").is_none(),
            "precondition: no console yet"
        );

        h.click("route-console");

        assert_eq!(
            h.text_of(h.by_a11y_id("routed").unwrap()),
            "Some(true)",
            "the router must claim an id the palette can show"
        );
        assert!(
            h.by_a11y_id("pane-console").is_some(),
            "and claiming it must actually mount the console"
        );
    });
}

#[test]
#[serial]
fn an_id_no_arm_claims_is_reported_rather_than_silently_swallowed() {
    // `route` returning false is what makes `components::App` log; a router
    // that returned true for everything would turn a missing handler into
    // silence, which is exactly how the breadcrumb ids shipped.
    with_config_dir(|dir| {
        let mut h = returning_shell(dir);
        h.click("route-nothing");
        assert_eq!(h.text_of(h.by_a11y_id("routed").unwrap()), "Some(false)");
    });
}
