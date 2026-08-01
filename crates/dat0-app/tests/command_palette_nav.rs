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
use std::rc::Rc;
use std::sync::Arc;

use gpui::{AppContext as _, Entity, FocusHandle, TestAppContext, VisualTestContext};
use gpui_component::Root;

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
