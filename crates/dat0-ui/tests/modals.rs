//! The single modal slot.
//!
//! What is proven here is what the *slot* owns, not what any one dialog draws:
//! that a second open replaces the first instead of stacking on it, that the
//! keyboard cannot get out, that a dialog which must not be dismissed by a
//! stray click is not, and that a panel can step through its own states
//! without the slot tearing it down.
//!
//! That last one is the regression this whole design exists to prevent.
//! `WindowExt::open_dialog` stacked, so the GPUI onboarding carousel advanced
//! by `close_dialog` + `open_dialog` per panel — an unmount per step, which
//! meant no dialog could hold state across one.
//!
//! The trap's *containment* half is `inert` on the background, applied by
//! `CAPTURE_JS`, and a headless harness has no browser to run it in. What the
//! harness can prove — and what a broken trap would break first — is that the
//! keys the modal claims never reach the shell behind it, and that the keys it
//! does not claim still do. A recorder is mounted above `ModalHost` for
//! exactly that.

mod support;

use std::cell::RefCell;
use std::path::PathBuf;

use dioxus::prelude::*;

use dat0_ui::components::modals::{
    CAPTURE_JS, CLOSE_ID, CYCLE_JS, DIALOG_ID, FOCUSABLE_SELECTOR, ModalHost, ModalOutcome,
    ModalReply, SCRIM_ID, TrapAction, scrim_dismissable, title, trap_action,
};
use dat0_ui::components::workspace_in_use::InUse;
use dat0_ui::keys::Cascade;
use dat0_ui::state::{Modal, Workspace};
use support::{Harness, Key, Modifiers};

thread_local! {
    /// What the slot holds at mount.
    static INITIAL: RefCell<Option<Modal>> = const { RefCell::new(None) };
    /// What clicking `swap` puts in the slot — a second open, with the first
    /// still up.
    static SWAP_TO: RefCell<Option<Modal>> = const { RefCell::new(None) };
    /// Keystrokes that reached the shell above the dialog. Anything the trap
    /// should have consumed and did not lands here.
    static ESCAPED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Outcomes the opener was told about.
    static REPLIES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Mount the host with `modal` already in the slot.
fn mount(modal: Modal) -> Harness {
    INITIAL.with(|c| *c.borrow_mut() = Some(modal));
    SWAP_TO.with(|c| *c.borrow_mut() = None);
    ESCAPED.with(|c| c.borrow_mut().clear());
    REPLIES.with(|c| c.borrow_mut().clear());
    Harness::new(Host, ())
}

/// A reply that records what the opener was told.
fn reply() -> ModalReply {
    ModalReply::new(|o: ModalOutcome| {
        REPLIES.with(|r| r.borrow_mut().push(format!("{o:?}")));
    })
}

fn escaped() -> Vec<String> {
    ESCAPED.with(|c| c.borrow().clone())
}

fn replies() -> Vec<String> {
    REPLIES.with(|c| c.borrow().clone())
}

/// The shell, near enough: a keydown recorder, a readable copy of the slot's
/// own state, one background focus stop, and the host.
#[component]
fn Host() -> Element {
    let ws = Workspace::provide();
    // Once, on mount. Writing every render would re-dirty the tree forever and
    // the harness would report a hang rather than a failure.
    use_hook(move || {
        let mut ws = ws;
        ws.modal.set(INITIAL.with(|c| c.borrow_mut().take()));
    });

    let open = ws.modal.read().is_some();

    rsx! {
        div {
            "data-a11y-id": "shell",
            onkeydown: move |e: KeyboardEvent| {
                ESCAPED.with(|c| c.borrow_mut().push(e.key().to_string()));
            },

            // The slot's own state, so "it closed" is an assertion about the
            // signal and not merely about a missing element.
            div { "data-a11y-id": "open", "{open}" }

            button {
                "data-a11y-id": "swap",
                onclick: move |_| {
                    let mut ws = ws;
                    ws.modal.set(SWAP_TO.with(|c| c.borrow().clone()));
                },
                "swap"
            }

            // Something behind the dialog for focus to escape to, if it could.
            button { "data-a11y-id": "background", tabindex: "0", "aria-label": "background", "bg" }

            ModalHost {}
        }
    }
}

fn about() -> Modal {
    Modal::About {
        newer: None,
        // The check is a blocking `ureq` call on `spawn_blocking`, which panics
        // outright with no Tokio runtime under it.
        check_latest: false,
    }
}

fn refresh() -> Modal {
    Modal::LiveRefresh {
        dropped_edits: 3,
        dropped_deletes: 1,
        reply: reply(),
    }
}

fn in_use() -> Modal {
    Modal::WorkspaceInUse {
        kind: InUse::SameMachine,
        reply: reply(),
    }
}

fn dialogs(h: &Harness) -> usize {
    h.by_role("dialog").len()
}

fn is_open(h: &Harness) -> bool {
    h.text_of(h.by_a11y_id("open").expect("the probe is mounted")) == "true"
}

// ── One slot ─────────────────────────────────────────────────────────────────

#[test]
fn a_second_open_replaces_the_first_instead_of_stacking_on_it() {
    let mut h = mount(about());
    assert_eq!(dialogs(&h), 1);
    assert!(h.by_a11y_id("about").is_some());

    SWAP_TO.with(|c| *c.borrow_mut() = Some(refresh()));
    h.click("swap");

    // The GPUI failure this replaces: `open_dialog` pushed, so both panels
    // would be mounted, the second offset 16px over the first.
    assert_eq!(dialogs(&h), 1, "two dialogs are mounted at once");
    assert!(
        h.by_a11y_id("about").is_none(),
        "the displaced panel survived"
    );
    assert!(h.by_a11y_id("live-refresh").is_some());
}

#[test]
fn the_dialog_is_labelled_by_the_variants_title() {
    let h = mount(refresh());
    let dialog = h.by_a11y_id(DIALOG_ID).expect("a dialog is mounted");
    assert_eq!(
        h.attr(dialog, "aria-label").as_deref(),
        Some(&*title(&refresh()))
    );
    assert_eq!(h.attr(dialog, "aria-modal").as_deref(), Some("true"));
}

// ── Escape ───────────────────────────────────────────────────────────────────

#[test]
fn escape_closes_the_slot_and_tells_the_opener_it_was_cancelled() {
    let mut h = mount(refresh());
    h.key_at(DIALOG_ID, Key::Escape, Modifiers::empty());

    assert!(!is_open(&h), "the slot is still full");
    assert_eq!(dialogs(&h), 0);
    // Cancelled, not Confirmed: Escape is always the safe branch, which for
    // this dialog means the file is not re-imported and the edits survive.
    assert_eq!(replies(), vec!["Cancelled".to_string()]);
}

#[test]
fn escape_closes_a_dialog_whose_scrim_is_inert() {
    // `Dialog::keyboard` defaulted to `true` upstream, so Escape cancelled even
    // a `confirm()` whose overlay and ✕ were both disabled. An un-escapable
    // modal is a trapped user.
    let mut h = mount(in_use());
    assert!(!scrim_dismissable(&in_use()));

    h.key_at(DIALOG_ID, Key::Escape, Modifiers::empty());
    assert!(!is_open(&h));
}

// ── Containment ──────────────────────────────────────────────────────────────

#[test]
fn the_keys_the_modal_owns_never_reach_the_shell() {
    let mut h = mount(refresh());

    h.key_at(DIALOG_ID, Key::Tab, Modifiers::empty());
    h.key_at(DIALOG_ID, Key::Tab, Modifiers::SHIFT);
    assert_eq!(
        escaped(),
        Vec::<String>::new(),
        "Tab bubbled past the dialog; the shell would move focus out of it"
    );
    // Tab is movement, not dismissal.
    assert!(is_open(&h));
}

#[test]
fn a_key_the_modal_does_not_bind_still_reaches_the_shell() {
    // ⌘K is global. A modal that swallowed everything would make the command
    // palette unreachable from a dialog, which GPUI never did: an unconsumed
    // keystroke fell through to the global scope.
    let mut h = mount(refresh());
    h.key_at(DIALOG_ID, Key::Character("k".into()), Modifiers::META);

    assert_eq!(escaped(), vec!["k".to_string()]);
    assert!(is_open(&h));
}

#[test]
fn escape_is_consumed_by_the_modal_rather_than_by_the_shell_below_it() {
    let mut h = mount(refresh());
    h.key_at(DIALOG_ID, Key::Escape, Modifiers::empty());
    assert_eq!(
        escaped(),
        Vec::<String>::new(),
        "the shell also saw Escape; whichever surface is under the modal would \
         act on it too"
    );
}

// ── The scrim ────────────────────────────────────────────────────────────────

#[test]
fn a_scrim_click_closes_a_dismissable_dialog() {
    let mut h = mount(about());
    assert!(scrim_dismissable(&about()));

    h.click(SCRIM_ID);
    assert!(!is_open(&h));
    assert_eq!(dialogs(&h), 0);
}

#[test]
fn a_scrim_click_does_not_close_a_dialog_that_gates_a_decision() {
    // Upstream: `confirm()` sets `overlay_closable(false)`. Both exits from
    // this one have consequences — proceeding may corrupt a workspace opened
    // on another machine — so a stray click must not pick one.
    let mut h = mount(in_use());

    h.click(SCRIM_ID);
    assert!(is_open(&h), "a stray click dismissed a decision gate");
    assert_eq!(dialogs(&h), 1);
    assert!(
        replies().is_empty(),
        "the opener was told something happened"
    );
}

#[test]
fn a_click_inside_the_panel_is_not_a_click_outside_it() {
    // The scrim's handler sees clicks that bubble up from the panel, so
    // without the panel stopping them, using a dismissable dialog would close
    // it on the first button press.
    let mut h = mount(about());
    h.click(DIALOG_ID);
    assert!(is_open(&h));
}

#[test]
fn the_close_affordance_appears_exactly_when_the_scrim_dismisses() {
    // Upstream coupled them: `Dialog::confirm()` disables `overlay_closable`
    // and `close_button` together. A ✕ beside an inert scrim would offer the
    // very dismissal the inert scrim exists to refuse.
    let h = mount(about());
    assert!(h.by_a11y_id(CLOSE_ID).is_some());

    let h = mount(in_use());
    assert!(h.by_a11y_id(CLOSE_ID).is_none());

    // …and it works.
    let mut h = mount(about());
    h.click(CLOSE_ID);
    assert!(!is_open(&h));
}

#[test]
fn a_panels_own_button_closes_the_slot() {
    // The other half of the close wiring: the ✕ and the scrim are the host's,
    // but every panel also has its own exit, and the host is what turns that
    // into an empty slot. `About`'s OK is the smallest one.
    let mut h = mount(about());
    h.click("about-ok");
    assert!(!is_open(&h));
    assert_eq!(dialogs(&h), 0);
}

// ── Stepping ─────────────────────────────────────────────────────────────────

#[test]
fn the_onboarding_carousel_steps_without_the_slot_closing() {
    // The reason the slot is a slot. Under `open_dialog` this step was a
    // `close_dialog` followed by a fresh `open_dialog`, so the carousel could
    // not have held its own index — it had to be threaded through the caller.
    let mut h = mount(Modal::Onboarding);
    let headline = h.by_a11y_id("tour-headline").expect("the tour is mounted");
    let first = h.text_of(headline);

    h.click("tour-next");

    assert!(is_open(&h), "stepping closed the slot");
    assert_eq!(dialogs(&h), 1, "stepping mounted a second dialog");
    let headline = h.by_a11y_id("tour-headline").expect("the tour is still up");
    assert_ne!(h.text_of(headline), first, "the panel did not advance");
}

#[test]
fn re_setting_the_slot_to_the_same_variant_keeps_the_panel_mounted() {
    // A panel's own state lives in its `use_signal`s, which survive a diff and
    // not a remount. The export dialog depends on this: the caller re-sets the
    // slot with the directory its picker returned, and the file name already
    // typed into the dialog has to still be there.
    let mut h = mount(Modal::Onboarding);
    h.click("tour-next");
    let stepped = h.text_of(h.by_a11y_id("tour-headline").unwrap());

    SWAP_TO.with(|c| *c.borrow_mut() = Some(Modal::Onboarding));
    h.click("swap");

    assert_eq!(
        h.text_of(h.by_a11y_id("tour-headline").unwrap()),
        stepped,
        "the panel was remounted and lost its step"
    );
}

// ── Pure decisions ───────────────────────────────────────────────────────────
//
// These live here rather than in a `#[cfg(test)] mod tests` because the crate's
// unit-test target does not currently build, and a test that cannot run is
// worse than no test: it looks like coverage.

fn modal_cascade() -> Cascade {
    Cascade {
        modal_open: true,
        palette_open: true,
        sql_console_focused: true,
    }
}

#[test]
fn a_modal_outranks_the_palette_and_the_console_for_escape() {
    // The precedence claim, made against the same table the shell reads: the
    // modal scope is the first one `Cascade` tries, so no surface underneath
    // the dialog can win the Escape ladder.
    assert_eq!(
        trap_action(modal_cascade(), &Key::Escape, Modifiers::empty()),
        TrapAction::Close
    );
}

#[test]
fn tab_and_shift_tab_are_the_two_cycle_directions() {
    assert_eq!(
        trap_action(modal_cascade(), &Key::Tab, Modifiers::empty()),
        TrapAction::Cycle(1)
    );
    assert_eq!(
        trap_action(modal_cascade(), &Key::Tab, Modifiers::SHIFT),
        TrapAction::Cycle(-1)
    );
}

#[test]
fn an_unbound_chord_falls_through_to_the_shell() {
    assert_eq!(
        trap_action(
            modal_cascade(),
            &Key::Character("k".into()),
            Modifiers::META
        ),
        TrapAction::Fallthrough
    );
}

#[test]
fn the_trap_scripts_and_the_ids_they_select_on_agree() {
    // Both scripts reach into the DOM by attribute value, so a rename of
    // either id is invisible to the compiler and would silently leave the
    // background reachable, or the cycle inert.
    assert!(CAPTURE_JS.contains(SCRIM_ID), "{CAPTURE_JS}");
    assert!(CYCLE_JS.contains(DIALOG_ID), "{CYCLE_JS}");
    // The cycle is templated, not concatenated; both placeholders must survive
    // into the constant or the substitution silently does nothing.
    assert!(CYCLE_JS.contains("SELECTOR") && CYCLE_JS.contains("DELTA"));
    // The dialog itself is `tabindex="-1"`, so the selector must exclude it or
    // the first Tab would land on the container.
    assert!(FOCUSABLE_SELECTOR.contains(r#"[tabindex]:not([tabindex="-1"])"#));
}

#[test]
fn every_variant_that_produces_a_decision_reports_a_cancel() {
    // A dismissed modal that says nothing leaves its opener waiting: the
    // export flow never learns the user backed out, the workspace-claim gate
    // never resolves. Checked over the variants cheap enough to build here.
    for modal in [
        refresh(),
        in_use(),
        Modal::Export {
            destination: None,
            reply: reply(),
        },
        Modal::NamePrompt {
            title: "n".into(),
            initial: String::new(),
            placeholder: None,
            confirm_label: None,
            secret: false,
            reply: reply(),
        },
        Modal::Recovery {
            scratch_root: PathBuf::from("/nonexistent"),
            recent_roots: Vec::new(),
            reply: reply(),
        },
    ] {
        let mut h = mount(modal);
        h.key_at(DIALOG_ID, Key::Escape, Modifiers::empty());
        assert_eq!(replies(), vec!["Cancelled".to_string()]);
    }
}
