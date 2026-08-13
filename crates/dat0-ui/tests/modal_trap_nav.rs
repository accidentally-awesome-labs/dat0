//! The modal focus trap, from inside the dialog.
//!
//! # What `tests/modals.rs` already proves, and what is left
//!
//! The slot's own contract — one dialog at a time, Escape closes it, the keys
//! it claims never reach the shell, the scrim's dismissability, a panel
//! stepping without a remount — is `tests/modals.rs`'s, and every one of those
//! tests drives the dialog element itself. This file is the other half: the
//! trap as a *keyboard user* meets it, from a control **inside** the panel.
//!
//! That distinction is the whole reason the GPUI original existed.
//! `gpui_component` bound `escape` only under the key context `"Input"`
//! (`input/state.rs:120`), so a dialog was cancellable from its text field and
//! nowhere else: once Tab reached OK or Cancel, Escape was dead. The fix was a
//! `Dat0Modal`-scoped binding on an ancestor, which only a keystroke dispatched
//! at a stop — not at the dialog — can distinguish from the broken version.
//! Here that ancestor is the dialog's own `onkeydown` and the mechanism is DOM
//! bubbling, but the failure mode is identical and just as invisible.
//!
//! # The stop list, and why it is computed rather than listed
//!
//! [`FOCUSABLE_SELECTOR`] is what [`CYCLE_JS`] rings through, and `overlay.rs`
//! could enumerate its stops because every GPUI modal declared a
//! `Vec<FocusHandle>`. Here the panels belong to other modules and several grow
//! controls with their data, so the DOM is the list — which means the list is
//! only right if the markup is. [`stops`] below applies the selector's rules to
//! the mirror, so a panel that renders a `div` where a `button` belongs, or
//! leaves a disabled control in the ring, fails here.
//!
//! # What needs a browser
//!
//! Three of the original's guarantees are the browser's own focus model and
//! cannot be settled without one: Tab wrapping at the last stop, Tab pulling
//! focus back in when it has escaped, and a dismissal handing the keyboard back
//! to whatever opened the dialog. Those are `examples/modal_trap_probe.rs`,
//! which runs `CAPTURE_JS` / `CYCLE_JS` / `RELEASE_JS` against a real document.
//! What is asserted here is everything upstream of them — that the ring is
//! offered the right stops, and that Tab and Escape are taken by the trap
//! rather than by the shell.

mod support;

use std::cell::RefCell;

use dioxus::prelude::*;

use dat0_ui::components::modals::{
    DIALOG_ID, ModalHost, ModalOutcome, ModalReply, scrim_dismissable,
};
use dat0_ui::state::{Modal, Workspace};
use support::dom::NodeKey;
use support::{Harness, Key, Modifiers};

thread_local! {
    /// The modal to seed the slot with, handed to the host through a
    /// thread-local because a `Modal` is not a prop-comparable value.
    static INITIAL: RefCell<Option<Modal>> = const { RefCell::new(None) };
    /// What the opener was told.
    static REPLIES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Keystrokes that got past the dialog to the shell behind it.
    static ESCAPED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn replies() -> Vec<String> {
    REPLIES.with(|c| c.borrow().clone())
}

fn escaped() -> Vec<String> {
    ESCAPED.with(|c| c.borrow().clone())
}

fn reply() -> ModalReply {
    ModalReply::new(|o: ModalOutcome| {
        REPLIES.with(|c| {
            c.borrow_mut().push(match o {
                ModalOutcome::Cancelled => "cancelled".to_string(),
                ModalOutcome::Named(v) => format!("named[{v}]"),
                other => format!("{other:?}"),
            })
        })
    })
}

/// The prompt every test drives. `initial` decides whether Confirm is armed,
/// which decides whether it is a stop.
fn name_prompt(initial: &str) -> Modal {
    Modal::NamePrompt {
        title: "Save query as…".to_string(),
        initial: initial.to_string(),
        placeholder: None,
        confirm_label: None,
        secret: false,
        reply: reply(),
    }
}

/// The shell, near enough: a keydown recorder, a readable copy of the slot, a
/// background stop, and something recognisable to survive behind the dialog.
#[component]
fn Host() -> Element {
    let ws = Workspace::provide();
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

            div { "data-a11y-id": "open", "{open}" }

            // A stop behind the dialog, so "the ring is scoped to the dialog"
            // is a claim with something to be wrong about.
            button {
                "data-a11y-id": "background",
                role: "button",
                "aria-label": "background",
                tabindex: "0",
                "bg"
            }

            // The console's Run chip, near enough. The GPUI test asserted on
            // this exact node — `sql-run` paints "Run" while idle — because the
            // console had no test-visibility shim and its presence is what
            // proves it did not close along with the modal.
            button {
                "data-a11y-id": "sql-run",
                role: "button",
                "aria-label": dat0_i18n::t("sql.run"),
                tabindex: "0",
                {dat0_i18n::t("sql.run")}
            }

            ModalHost {}
        }
    }
}

fn mount(modal: Modal) -> Harness {
    INITIAL.with(|c| *c.borrow_mut() = Some(modal));
    REPLIES.with(|c| c.borrow_mut().clear());
    ESCAPED.with(|c| c.borrow_mut().clear());
    Harness::new(Host, ())
}

fn is_open(h: &Harness) -> bool {
    h.text_of(h.by_a11y_id("open").expect("the probe is mounted")) == "true"
}

/// Is `key` inside the subtree rooted at `root`?
fn within(h: &Harness, key: NodeKey, root: NodeKey) -> bool {
    let mut at = Some(key);
    while let Some(k) = at {
        if k == root {
            return true;
        }
        at = h.dom().get(k).parent;
    }
    false
}

/// The dialog's focus ring, in document order.
///
/// The rules are [`FOCUSABLE_SELECTOR`]'s, restated over the mirror: a native
/// focusable that is not disabled, a link with a destination, or anything that
/// opted in with a `tabindex` — and in every case not `tabindex="-1"`, which is
/// how a roving group (the export dialog's radios, the saved-query rows)
/// declares that it is one stop and not many.
fn stops(h: &Harness) -> Vec<String> {
    let dialog = h.by_a11y_id(DIALOG_ID).expect("a dialog is open");
    h.dom()
        .walk()
        .into_iter()
        .filter(|k| *k != dialog && within(h, *k, dialog))
        .filter(|k| {
            let n = h.dom().get(*k);
            if n.attr("tabindex") == Some("-1") {
                return false;
            }
            let enabled = n.attr("disabled") != Some("true");
            match n.tag() {
                Some("button" | "input" | "select" | "textarea") => enabled,
                Some("a") => n.attr("href").is_some(),
                _ => n.attr("tabindex").is_some(),
            }
        })
        .map(|k| {
            h.attr(k, "data-a11y-id")
                .unwrap_or_else(|| "<unnamed stop>".to_string())
        })
        .collect()
}

// ── The ring ─────────────────────────────────────────────────────────────────

#[test]
fn the_prompts_focus_order_is_the_field_then_confirm_then_cancel() {
    // The trap's only source of truth, so a render reorder must break this
    // test — the GPUI original said the same thing about `focus_order()`, which
    // was a hand-written `Vec<FocusHandle>` the render had to keep in step
    // with. Reading the ring off the DOM removes the second list, and puts the
    // obligation on the markup instead.
    let h = mount(name_prompt("q2"));
    assert_eq!(
        stops(&h),
        vec!["name-prompt-field", "name-prompt-ok", "name-prompt-cancel"],
    );
}

#[test]
fn a_disarmed_confirm_is_not_one_of_the_stops() {
    // `button:not([disabled])`, and not a detail: the prompt opens empty for
    // every save flow, so this is the state a keyboard user actually meets. A
    // ring that included the dead Save would spend a Tab on a control that
    // cannot be pressed, and Shift-Tab off the field would land there.
    let h = mount(name_prompt(""));
    assert_eq!(
        h.attr(h.by_a11y_id("name-prompt-ok").unwrap(), "disabled")
            .as_deref(),
        Some("true"),
        "precondition: an empty prompt disarms Save",
    );
    assert_eq!(stops(&h), vec!["name-prompt-field", "name-prompt-cancel"]);
}

#[test]
fn the_ring_holds_the_dialogs_own_stops_and_nothing_behind_it() {
    // `CYCLE_JS` queries within the dialog, and `CAPTURE_JS` inerts everything
    // beside the scrim. Both halves say the same thing, and this is the half a
    // headless tree can see: a control in the shell is not one of the dialog's
    // stops, so the ring cannot walk out into the screen the dialog is
    // covering. GPUI's `modal_host` could not make that claim — its `occlude`
    // blocked the mouse only.
    let h = mount(name_prompt("q2"));
    assert!(
        h.by_a11y_id("background").is_some(),
        "the shell stop exists"
    );
    assert!(
        !stops(&h).contains(&"background".to_string()),
        "a shell control is in the dialog's ring: {:?}",
        stops(&h)
    );
}

#[test]
fn a_dialog_that_cannot_be_dismissed_by_a_click_still_offers_a_way_out() {
    // The prompt holds typed text, so its scrim is inert and it has no ✕ —
    // which means the keyboard is the *only* exit, and the ring had better
    // contain one. This is the pairing that makes the two halves of the design
    // safe: `scrim_dismissable(false)` is only acceptable because Escape works
    // from every stop, which the tests below prove.
    let modal = name_prompt("q2");
    assert!(!scrim_dismissable(&modal), "precondition: a typed value");
    let h = mount(modal);
    assert!(
        h.by_a11y_id("modal-close").is_none(),
        "no ✕ on a dialog whose scrim is inert",
    );
    assert!(stops(&h).contains(&"name-prompt-cancel".to_string()));
}

// ── Escape, from a stop that is not the field ────────────────────────────────

#[test]
fn escape_from_the_cancel_button_dismisses_the_dialog() {
    // The defect the GPUI B1 slice fixed, in one line: upstream bound `escape`
    // under `"Input"`, so this exact keystroke — Escape with focus on Cancel —
    // did nothing at all. Dispatching at the button rather than at the dialog
    // is the whole test; at the dialog it passed before the fix too.
    let mut h = mount(name_prompt("q2"));
    h.key_at("name-prompt-cancel", Key::Escape, Modifiers::empty());

    assert_eq!(replies(), vec!["cancelled".to_string()]);
    assert!(!is_open(&h), "Escape from Cancel left the dialog up");
}

#[test]
fn escape_from_the_confirm_button_dismisses_it_too() {
    // The same rule, at the other end of the ring: "from any stop" is the
    // guarantee, and one worked example of it is a coincidence away from being
    // the only stop that works.
    let mut h = mount(name_prompt("q2"));
    h.key_at("name-prompt-ok", Key::Escape, Modifiers::empty());

    assert_eq!(replies(), vec!["cancelled".to_string()]);
    assert!(!is_open(&h));
}

#[test]
fn one_escape_produces_exactly_one_cancel() {
    // The control the GPUI suite kept across the fix. Adding a second `escape`
    // binding is how the cell-editor slice shipped an Enter that fired twice,
    // and a Cancel that fires twice tells the opener to abandon its flow, then
    // tells it again — which for the token prompt means two writes to the
    // keychain.
    let mut h = mount(name_prompt("q2"));
    h.key_at("name-prompt-field", Key::Escape, Modifiers::empty());

    assert_eq!(
        replies().iter().filter(|r| *r == "cancelled").count(),
        1,
        "got {:?}",
        replies()
    );
}

#[test]
fn escape_from_a_stop_never_reaches_the_surface_behind_the_dialog() {
    // One Escape must not walk two rungs of the ladder: the modal closes, the
    // console behind it stays. Bubbling makes this the live hazard — the
    // keystroke starts at a button *inside* the dialog and travels outward, so
    // the dialog's handler is the only thing between it and the shell.
    let mut h = mount(name_prompt("q2"));
    h.key_at("name-prompt-cancel", Key::Escape, Modifiers::empty());

    assert!(!is_open(&h), "the modal closed");
    assert_eq!(
        escaped(),
        Vec::<String>::new(),
        "the shell also saw Escape, so whatever is under the modal acted on it"
    );
    assert!(
        h.query_by_role("button", &dat0_i18n::t("sql.run")),
        "the console behind it closed too"
    );
}

// ── Tab, from a stop that is not the dialog ──────────────────────────────────

#[test]
fn tab_from_a_stop_inside_the_dialog_is_taken_by_the_trap() {
    // `tests/modals.rs` presses Tab at the dialog element; a real keyboard user
    // presses it at whatever they are focused on, which is a control several
    // levels down. If the trap were wired to the panel instead of to an
    // ancestor of it, that version would pass and this one would let Tab
    // through to the shell — where the browser's own sequential focus would
    // walk straight out of the dialog and into the obscured screen.
    let mut h = mount(name_prompt("q2"));
    h.key_at("name-prompt-cancel", Key::Tab, Modifiers::empty());
    h.key_at("name-prompt-field", Key::Tab, Modifiers::SHIFT);

    assert_eq!(
        escaped(),
        Vec::<String>::new(),
        "Tab bubbled past the dialog; focus would leave it"
    );
    assert!(is_open(&h), "Tab is movement, not dismissal");
}

#[test]
fn a_chord_the_dialog_does_not_bind_still_reaches_the_shell_from_inside_it() {
    // The trap contains focus, not the keyboard. ⌘K is global, and a dialog
    // that swallowed everything typed into it would make the palette
    // unreachable from a prompt — which GPUI never did, because an unconsumed
    // keystroke fell through to the global scope.
    let mut h = mount(name_prompt("q2"));
    h.key_at(
        "name-prompt-cancel",
        Key::Character("k".into()),
        Modifiers::META,
    );

    assert_eq!(escaped(), vec!["k".to_string()]);
    assert!(is_open(&h));
}
