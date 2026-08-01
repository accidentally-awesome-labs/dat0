//! Command-palette keyboard coverage (UI redesign B4).
//!
//! Drives keys through the REAL keymap with `simulate_keystrokes` — the latter
//! bypasses the keymap, which is exactly the mechanism under test, and a green
//! test over a dead production key path is how carve-out #7's Escape ladder
//! shipped broken past five reviews. `enter_emits_run_for_the_active_row` is the
//! single documented exception; its doc comment carries the reason and says what
//! covers the keymap half instead.
//!
//! The arrow tests exist in PAIRS, one per focus stop, because the palette's two
//! stops resolve `down` completely differently: with the query field focused,
//! upstream's `MoveDown` (key context "Input", deeper) is chosen first and falls
//! through to ours only because a single-line `Input` registers no handler for
//! it; with the results list focused, "Input" is absent from the context stack
//! and `PaletteDown` is matched directly. The B4 T0 gate measured both. An
//! earlier design handled only the first case and would have shipped arrows that
//! die on the second stop with every test still green.
//!
//! Harness helpers are copied per-binary from `tests/modal_b2_nav.rs` (the
//! per-binary-copy convention; see `tests/support/mod.rs` for the rationale).
#![allow(dead_code, unused_imports)]

mod support;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AppContext as _, Entity, FocusHandle, TestAppContext, VisualTestContext};
use gpui_component::Root;
use parking_lot::Mutex;
use serial_test::serial;

use dat0_app::session::Session;
use dat0_app::window::WorkspaceShell;

use dat0_app::actions::registry::{ActionDescriptor, ActionGroup, ActionId, ActionRegistry};
use dat0_app::view::command_palette::{CommandPalette, CommandPaletteEvent};

use support::A11ySnapshot;

/// Register the bindings production installs in `run_app`. The harness calls
/// only `gpui_component::init`, so without this the palette's keys are unbound
/// and every keystroke test would pass vacuously (T0 gate G3 measured the chord
/// as unavailable in a bare test app).
fn init_components(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);
    cx.update(dat0_app::overlay::register_modal_keys);
    cx.update(dat0_app::command_palette::register_command_palette_keys);
}

/// Install the two process-wide singletons production sets up in `spawn_window`
/// and `main`: the focused-workspace weak handle (how `command_palette::open`
/// finds this shell from a bare `&mut App`) and the `ActionRegistry` (what the
/// palette lists). Both are `OnceCell`-backed, so this is idempotent across the
/// serial tests in this binary.
fn install_shell_globals(shell: &Entity<WorkspaceShell>) {
    dat0_app::window_registry::install_focused_workspace(shell.downgrade().into());
    let reg = ActionRegistry::new();
    dat0_app::actions::builtin::register_all(&reg).expect("register_all");
    dat0_app::window_registry::install_action_registry(reg);
}

/// Three descriptors with known titles and ids, so ordering assertions do not
/// depend on the real registry's contents. `visible_items` sorts an empty query
/// alphabetically: Alpha, Beta, Gamma.
fn probe_registry_with_three() -> ActionRegistry {
    let reg = ActionRegistry::new();
    for (id, title) in [
        ("probe.alpha", "Alpha"),
        ("probe.beta", "Beta"),
        ("probe.gamma", "Gamma"),
    ] {
        reg.register(ActionDescriptor {
            id: ActionId::from(id),
            title: title.to_string(),
            group: ActionGroup::Navigation,
            keybinding: None,
            dispatch: Arc::new(|_app| {}),
        })
        .expect("unique id");
    }
    reg
}

/// Mount a standalone `CommandPalette` under a `Root`, the way
/// `tests/a11y_content.rs::open_sql_console_window` mounts a bare console:
/// production builds it with the same `ActionRegistry` + `&mut Window`.
fn open_palette(
    cx: &mut TestAppContext,
    reg: ActionRegistry,
) -> (Entity<CommandPalette>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<CommandPalette>>>> = Rc::new(RefCell::new(None));
    let slot2 = slot.clone();
    let (_root, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_window();
        let p = cx.new(|cx| CommandPalette::new(reg, window, cx));
        *slot2.borrow_mut() = Some(p.clone());
        Root::new(p, window, cx)
    });
    let p = slot.borrow().clone().expect("palette captured");
    (p, vcx)
}

/// Focus the query field — the stop the production open path focuses.
fn focus_query_field(palette: &Entity<CommandPalette>, vcx: &mut VisualTestContext) {
    let fh: FocusHandle = palette.read_with(vcx, |p, cx| p.input_focus_handle(cx));
    vcx.update(|window, _cx| window.focus(&fh));
    vcx.run_until_parked();
}

#[gpui::test]
fn arrows_move_the_active_row_and_clamp_at_both_ends(cx: &mut TestAppContext) {
    init_components(cx);
    let (palette, vcx) = open_palette(cx, probe_registry_with_three());
    focus_query_field(&palette, vcx);

    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 0);
    assert_eq!(palette.read_with(vcx, |p, _| p.item_count_for_test()), 3);

    // Up at the top is a no-op — list surfaces CLAMP, only radio groups wrap.
    vcx.simulate_keystrokes("up");
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 0);

    vcx.simulate_keystrokes("down down");
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 2);

    // …and clamps at the bottom too.
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 2);
}

/// The second stop. With focus here the "Input" key context is absent, so any
/// mechanism keyed on upstream's `MoveDown` is dead — this is the test the
/// original `capture_action` design would have failed.
#[gpui::test]
fn arrows_work_with_focus_on_the_results_list_too(cx: &mut TestAppContext) {
    init_components(cx);
    let (palette, vcx) = open_palette(cx, probe_registry_with_three());
    let lf = palette.read_with(vcx, |p, _| p.list_focus_handle_for_test());
    vcx.update(|window, _cx| window.focus(&lf));
    vcx.run_until_parked();

    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 1);
}

/// ⚠ The ONE place this binary uses `dispatch_action` instead of a keystroke,
/// and it needs its reason on the record.
///
/// `InputState::enter` calls `cx.propagate()` on a single-line field
/// (`input/state.rs:1166`), so the Enter keystroke is deliberately NOT consumed:
/// it continues past the action into text insertion and drops a `"\n"` into the
/// buffer. `NamePrompt` gets away with that because its modal unmounts before
/// the next frame; a palette mounted STANDALONE (as here, with no shell to
/// dismiss it) re-renders and gpui's text system panics with "text argument
/// should not contain newlines". Same trap the cell-editor slice hit.
///
/// The keymap half is not left unproven: `Enter` under key context "Input" is
/// upstream's own binding, `arrows_*` above prove real keystrokes reach this
/// palette, and T3's shell-mounted suite drives Enter with a REAL ⏎ through the
/// production path, where the shell dismisses the modal and the stray newline
/// dies with it.
#[gpui::test]
fn enter_emits_run_for_the_active_row(cx: &mut TestAppContext) {
    init_components(cx);
    let (palette, vcx) = open_palette(cx, probe_registry_with_three());
    focus_query_field(&palette, vcx);

    let seen: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let seen2 = seen.clone();
    let _sub = vcx.update(|_window, cx| {
        cx.subscribe(&palette, move |_p, ev: &CommandPaletteEvent, _cx| {
            if let CommandPaletteEvent::Run(id) = ev {
                seen2.borrow_mut().push(id.as_str().to_string());
            }
        })
    });
    vcx.run_until_parked();

    // Arrow by keystroke (real keymap), then Enter by action (no stray newline).
    vcx.simulate_keystrokes("down");
    vcx.run_until_parked();
    vcx.update(|window, cx| {
        window.dispatch_action(
            Box::new(gpui_component::input::Enter { secondary: false }),
            cx,
        )
    });
    vcx.run_until_parked();
    assert_eq!(seen.borrow().len(), 1, "exactly one Run per Enter");
    assert_eq!(seen.borrow()[0], "probe.beta");
}

#[gpui::test]
fn typing_narrows_the_list_and_resets_the_active_row(cx: &mut TestAppContext) {
    init_components(cx);
    let (palette, vcx) = open_palette(cx, probe_registry_with_three());
    focus_query_field(&palette, vcx);

    vcx.simulate_keystrokes("down down");
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.active_for_test()), 2);

    palette.update_in(vcx, |p, window, cx| {
        p.seed_query_for_test("gam", window, cx)
    });
    vcx.run_until_parked();
    assert_eq!(palette.read_with(vcx, |p, _| p.item_count_for_test()), 1);
    assert_eq!(
        palette.read_with(vcx, |p, _| p.active_for_test()),
        0,
        "a stale active index would run the wrong command"
    );
}

/// The rows really are rendered content, not just model state. Only the visible
/// window reaches the capture tree (T0 gate G1), which is fine for three rows.
#[gpui::test]
fn rows_render_their_titles_as_a11y_content(cx: &mut TestAppContext) {
    init_components(cx);
    let (_palette, vcx) = open_palette(cx, probe_registry_with_three());
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_any("Alpha"),
        "row title missing from the tree"
    );
    assert!(snap.has_label_any("Gamma"));
    assert!(
        !snap.has_label_any("Delta"),
        "negative control: a title that was never registered must be absent"
    );
}

// ---------------------------------------------------------------------------
// Shell-mounted suite (T3): the production shape — real ⌘⇧P, real modal host,
// real Tab trap, real focus restore.
// ---------------------------------------------------------------------------

const BUDGET: u64 = 128 * 1024 * 1024;

/// Point `config_dir()` at `dir` for the rest of this (serial) test.
fn set_config_dir(dir: &Path) {
    // SAFETY: tests are `#[serial]`, so no other thread races this process-global
    // write; each test sets it before doing anything that reads `config_dir()`.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", dir) };
}

/// Build a real, EMPTY in-memory session inside a dedicated multi-thread tokio
/// runtime (`Session::new` is async + uses `spawn_blocking`).
fn build_empty_session(state_root: &Path) -> Arc<Mutex<Session>> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let sess = rt
        .block_on(Session::new(state_root, BUDGET))
        .expect("Session::new");
    Arc::new(Mutex::new(sess))
}

/// Open a real ACTIVATED window whose root is a `gpui_component::Root` wrapping
/// a fresh `WorkspaceShell` (mirrors production `open_window_view`).
fn open_shell_window(
    cx: &mut TestAppContext,
    session: Arc<Mutex<Session>>,
) -> (Entity<WorkspaceShell>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<WorkspaceShell>>>> = Rc::new(RefCell::new(None));
    let slot2 = slot.clone();
    let (_root, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_window();
        let shell = cx.new(|c| WorkspaceShell::new(session, c));
        *slot2.borrow_mut() = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = slot.borrow().clone().expect("shell captured");
    (shell, vcx)
}

/// Focus the shell the way a real keyboard user does: click a neutral spot in
/// the enriched band's top row, then PROVE the click landed on the shell's own
/// focus handle and not a wired hero button (copied from `modal_b2_nav.rs`).
///
/// LOAD-BEARING for any Tab-driven test: with NOTHING focused the dispatch path
/// is the window root alone, so not even `Root`'s own "tab" binding matches and
/// Tab is completely inert.
fn focus_shell_neutrally(cx: &mut VisualTestContext) {
    let tt_bounds = cx
        .debug_bounds("hero-take-tour")
        .expect("hero-take-tour must be painted");
    let focus_pt = gpui::point(tt_bounds.origin.x - gpui::px(60.), tt_bounds.center().y);
    cx.simulate_click(focus_pt, gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(
        cx.update(|window, app| window.focused(app).is_some()),
        "clicking into the workspace must focus something (precondition for Tab)"
    );
}

/// Press the real chord. `command_palette::open` resolves the focused workspace
/// through `window_registry`, so the shell must be installed there first.
fn press_palette_chord(vcx: &mut VisualTestContext) {
    #[cfg(target_os = "macos")]
    vcx.simulate_keystrokes("cmd-shift-p");
    #[cfg(not(target_os = "macos"))]
    vcx.simulate_keystrokes("ctrl-shift-p");
    vcx.run_until_parked();
}

#[gpui::test]
#[serial]
fn cmd_shift_p_opens_the_palette_from_the_shell(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().expect("tempdir");
    set_config_dir(tmp.path());
    init_components(cx);
    let session = build_empty_session(tmp.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    install_shell_globals(&shell);
    focus_shell_neutrally(vcx);

    assert_eq!(
        shell.read_with(vcx, |s, cx| s.open_modal_count_for_test(cx)),
        0
    );
    press_palette_chord(vcx);
    assert_eq!(
        shell.read_with(vcx, |s, cx| s.open_modal_count_for_test(cx)),
        1,
        "the palette is the mounted modal"
    );
    assert!(shell.read_with(vcx, |s, _| s.command_palette_for_test().is_some()));

    // The card renders, and it is the palette's.
    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.query_by_role(dat0_app::a11y::AccessRole::Dialog, "Command Palette"));
}

#[gpui::test]
#[serial]
fn escape_dismisses_the_palette_and_restores_focus(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().expect("tempdir");
    set_config_dir(tmp.path());
    init_components(cx);
    let session = build_empty_session(tmp.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    install_shell_globals(&shell);
    focus_shell_neutrally(vcx);

    let before = vcx.update(|window, app| window.focused(app));
    press_palette_chord(vcx);
    assert_eq!(
        shell.read_with(vcx, |s, cx| s.open_modal_count_for_test(cx)),
        1
    );

    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert_eq!(
        shell.read_with(vcx, |s, cx| s.open_modal_count_for_test(cx)),
        0,
        "Escape must dismiss"
    );
    assert_eq!(
        vcx.update(|window, app| window.focused(app)),
        before,
        "focus must return to where it was before the palette opened"
    );
}

/// The production Enter path, driven by a REAL keystroke — the coverage the
/// standalone `enter_emits_run_for_the_active_row` deliberately trades away.
/// Here the shell dismisses on Run, so the stray newline `InputState::enter`
/// leaves in the buffer dies with the unmounted modal instead of panicking the
/// next frame.
#[gpui::test]
#[serial]
fn enter_runs_a_command_through_the_real_keymap(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().expect("tempdir");
    set_config_dir(tmp.path());
    init_components(cx);
    let session = build_empty_session(tmp.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    install_shell_globals(&shell);
    focus_shell_neutrally(vcx);

    press_palette_chord(vcx);
    assert_eq!(
        shell.read_with(vcx, |s, cx| s.open_modal_count_for_test(cx)),
        1
    );

    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert_eq!(
        shell.read_with(vcx, |s, cx| s.open_modal_count_for_test(cx)),
        0,
        "Enter must run the active row and dismiss — if this hangs at 1, the \
         stray newline is about to panic the text system on the next frame"
    );
}

/// Tab must cycle the palette's three stops and never escape into the obscured
/// shell behind it (B1's WCAG 2.4.3 trap, inherited for free by being in
/// `mounted_modals`).
#[gpui::test]
#[serial]
fn tab_stays_inside_the_palette(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().expect("tempdir");
    set_config_dir(tmp.path());
    init_components(cx);
    let session = build_empty_session(tmp.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    install_shell_globals(&shell);
    focus_shell_neutrally(vcx);

    press_palette_chord(vcx);
    let palette = shell
        .read_with(vcx, |s, _| s.command_palette_for_test())
        .expect("palette mounted");
    let stops: Vec<gpui::FocusHandle> = palette.read_with(vcx, |p, cx| {
        use dat0_app::overlay::ModalContent as _;
        p.modal_focus_order(cx)
    });
    assert_eq!(stops.len(), 3, "input, list, close");

    // Four Tabs from the first stop return to it. A Tab that leaked into the
    // shell would land on a handle that is not in this list.
    for _ in 0..4 {
        vcx.simulate_keystrokes("tab");
        vcx.run_until_parked();
        let focused = vcx.update(|window, app| window.focused(app));
        assert!(
            focused.is_some_and(|f| stops.contains(&f)),
            "Tab escaped the palette"
        );
    }
}

// ---------------------------------------------------------------------------
// Router suite (T4): the seven Window-blocked ids that shipped as breadcrumbs.
// ---------------------------------------------------------------------------

/// A tokio runtime kept alive for the whole test.
///
/// LOAD-BEARING for anything that routes `console.toggle`: `toggle_sql_console`
/// calls `refresh_completion_snapshot`, which does a bare `tokio::spawn` and
/// panics with "there is no reactor running" outside a runtime context. Same
/// trap the AI-config nav slice hit.
struct AsyncHarness {
    #[allow(dead_code)]
    rt: tokio::runtime::Runtime,
}

fn enter_async_harness(cx: &mut TestAppContext) -> AsyncHarness {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    cx.executor().allow_parking();
    AsyncHarness { rt }
}

/// The gate that keeps `WINDOW_ROUTED` honest. A listed id with no arm would be
/// shown in the palette and silently do nothing — the exact defect this slice
/// exists to avoid.
#[gpui::test]
#[serial]
fn every_window_routed_id_is_actually_handled(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.rt.enter();
    let tmp = tempfile::tempdir().expect("tempdir");
    set_config_dir(tmp.path());
    init_components(cx);
    let session = build_empty_session(tmp.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    install_shell_globals(&shell);

    for id in dat0_app::command_palette::WINDOW_ROUTED {
        let handled = shell.update_in(vcx, |ws, window, cx| {
            ws.run_palette_action_for_test(&ActionId::from(*id), window, cx)
        });
        vcx.run_until_parked();
        assert!(
            handled,
            "{id} is listed as window-routed but the router ignores it"
        );
        // Some arms open a modal; dismiss it so the next iteration starts clean
        // and the single-modal invariant is never violated across the loop.
        shell.update_in(vcx, |ws, window, cx| {
            ws.dismiss_all_modals_for_test(window, cx)
        });
        vcx.run_until_parked();
    }
}

#[gpui::test]
#[serial]
fn an_unrouted_id_falls_through_to_the_registry(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().expect("tempdir");
    set_config_dir(tmp.path());
    init_components(cx);
    let session = build_empty_session(tmp.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    install_shell_globals(&shell);

    let handled = shell.update_in(vcx, |ws, window, cx| {
        ws.run_palette_action_for_test(&ActionId::from("settings.open"), window, cx)
    });
    assert!(
        !handled,
        "a live App-path action must NOT be claimed by the router"
    );
}

/// The payoff test: before B4 this id logged "handled view-scoped (needs
/// Window); no-op from App path" and nothing happened.
#[gpui::test]
#[serial]
fn running_console_toggle_from_the_palette_mounts_the_console(cx: &mut TestAppContext) {
    let harness = enter_async_harness(cx);
    let _guard = harness.rt.enter();
    let tmp = tempfile::tempdir().expect("tempdir");
    set_config_dir(tmp.path());
    init_components(cx);
    let session = build_empty_session(tmp.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    install_shell_globals(&shell);

    assert!(
        !shell.read_with(vcx, |s, _| s.sql_console_is_mounted_for_test()),
        "precondition: no console yet"
    );
    shell.update_in(vcx, |ws, window, cx| {
        ws.run_palette_action_for_test(&ActionId::from("console.toggle"), window, cx);
    });
    vcx.run_until_parked();
    assert!(
        shell.read_with(vcx, |s, _| s.sql_console_is_mounted_for_test()),
        "the breadcrumb dispatch would have left this unmounted — that is the \
         whole point of B4's router"
    );
}

/// Both lists name real registry ids. A typo'd or stale entry would otherwise
/// sit there doing nothing, which is what the lists exist to prevent.
#[test]
fn listed_ids_are_all_really_registered() {
    let reg = ActionRegistry::new();
    dat0_app::actions::builtin::register_all(&reg).expect("register_all");
    for id in dat0_app::command_palette::HIDDEN
        .iter()
        .chain(dat0_app::command_palette::WINDOW_ROUTED.iter())
    {
        assert!(
            reg.contains(id),
            "{id} is listed but not registered — stale id"
        );
    }
}

/// ⌘⇧P is a GLOBAL binding, so it fires even while another modal owns the
/// screen. Mounting the palette on top would make two modals — the second one
/// untrapped in release, and a `debug_assert!` panic in debug.
#[gpui::test]
#[serial]
fn the_chord_is_inert_while_another_modal_is_open(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().expect("tempdir");
    set_config_dir(tmp.path());
    init_components(cx);
    let session = build_empty_session(tmp.path());
    let (shell, vcx) = open_shell_window(cx, session);
    vcx.run_until_parked();
    install_shell_globals(&shell);
    focus_shell_neutrally(vcx);

    // Open a NamePrompt first (the shell's own test opener).
    shell.update_in(vcx, |ws, window, cx| {
        ws.open_name_prompt_for_test(window, cx);
    });
    vcx.run_until_parked();
    assert_eq!(
        shell.read_with(vcx, |s, cx| s.open_modal_count_for_test(cx)),
        1
    );

    press_palette_chord(vcx);
    assert_eq!(
        shell.read_with(vcx, |s, cx| s.open_modal_count_for_test(cx)),
        1,
        "the palette must not stack on top of an open modal"
    );
    assert!(
        shell.read_with(vcx, |s, _| s.command_palette_for_test().is_none()),
        "the palette specifically must be the one that did not mount"
    );
}
