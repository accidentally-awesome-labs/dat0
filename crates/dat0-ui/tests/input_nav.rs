//! Text-field keyboard behaviour, after `InputState` went away.
//!
//! # What this file is about
//!
//! The GPUI original covered two `gpui_component::input::InputState` surfaces:
//! the SQL editor and the shared name prompt. Everything it asserted about the
//! editor is now split by construction — the editor is CodeMirror, which owns
//! its own keymap inside the webview, so its Escape ladder and its ⌘⏎ live in
//! `sql_console_nav.rs` and `examples/console_probe.rs`, and the question of
//! *which scope* claims Escape is `keys::Cascade`'s, unit-tested beside it.
//!
//! What is left is the half that a plain `<input>` inherited, and it is the
//! half most likely to rot quietly. `InputState` was a widget: single-line by
//! construction, focused by `InputState::new`, masked by a builder flag, and
//! the owner of its own Enter. An `<input>` is markup, so each of those is now
//! an attribute somebody has to keep writing — and every one of them is
//! invisible in a screenshot.
//!
//! # Not duplicated here
//!
//! `views_b.rs` already drives the prompt's *validation* — empty, whitespace,
//! a pasted newline, an untrimmed value, a seeded default. This file assumes
//! that and asserts the keyboard contract around it.

mod support;

use std::cell::RefCell;
use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::settings::store::SettingsStore;
use dat0_ui::components::name_prompt::NamePrompt;
use dat0_ui::components::settings_ui::{Bus, SECTIONS, SettingsPanel, SettingsProps, Store};
use support::{Harness, Key, Modifiers};

thread_local! {
    /// Keystrokes that got past the prompt to the surface hosting it.
    ///
    /// The prompt always renders inside something — the modal host, and the
    /// shell under that — so "the field consumed it" is only meaningful
    /// against a listener above it. Each `#[test]` owns its thread, so each
    /// owns its recorder.
    static BEHIND: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn behind() -> Vec<String> {
    BEHIND.with(|c| c.borrow().clone())
}

/// Type into a field: `oninput` carries the whole new value, not the keystroke.
fn form(value: &str) -> dioxus::html::SerializedFormData {
    dioxus::html::SerializedFormData::new(value.to_string(), Vec::new())
}

#[derive(Clone, PartialEq, Props)]
struct PromptHostProps {
    initial: String,
    secret: bool,
}

/// The prompt under a keydown recorder — the surface a keystroke reaches if
/// the field lets it through.
#[component]
fn PromptHost(props: PromptHostProps) -> Element {
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        div {
            "data-a11y-id": "surface",
            onkeydown: move |e: KeyboardEvent| {
                BEHIND.with(|c| c.borrow_mut().push(e.key().to_string()));
            },

            NamePrompt {
                title: "Save query as…".to_string(),
                initial: props.initial.clone(),
                secret: props.secret,
                on_confirm: move |v: String| log.write().push(format!("confirm[{v}]")),
                on_cancel: move |_| log.write().push("cancel".to_string()),
            }
        }
        div { "data-a11y-id": "captured", "{log.read().join(\"|\")}" }
    }
}

fn prompt(initial: &str) -> Harness {
    BEHIND.with(|c| c.borrow_mut().clear());
    Harness::new(
        PromptHost,
        PromptHostProps {
            initial: initial.to_string(),
            secret: false,
        },
    )
}

fn secret_prompt() -> Harness {
    BEHIND.with(|c| c.borrow_mut().clear());
    Harness::new(
        PromptHost,
        PromptHostProps {
            initial: String::new(),
            secret: true,
        },
    )
}

fn captured(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("captured").expect("the readback is mounted"))
}

// ─────────────────────────────────────────────────────────────────────────────
// The prompt field
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_prompt_field_takes_the_keyboard_the_moment_it_opens() {
    // GPUI got this from `InputState::new`, which focused itself, and
    // `prompt_focused_on_open` asserted it: a prompt that opens unfocused makes
    // a keyboard user Tab into their own dialog before they can type its one
    // required value. `autofocus` is the markup that replaces the constructor.
    let h = prompt("");
    let field = h
        .by_a11y_id("name-prompt-field")
        .expect("the field renders");
    assert_eq!(h.attr(field, "autofocus").as_deref(), Some("true"));

    // And it is the first thing in the panel, so the autofocus and the trap's
    // first stop are the same control rather than two different answers to
    // "where does the keyboard start".
    let ids: Vec<String> = h
        .dom()
        .walk()
        .into_iter()
        .filter(|k| matches!(h.dom().get(*k).tag(), Some("input" | "button")))
        .filter_map(|k| h.attr(k, "data-a11y-id"))
        .collect();
    assert_eq!(
        ids.first().map(String::as_str),
        Some("name-prompt-field"),
        "something precedes the field in the panel: {ids:?}"
    );
}

#[test]
fn a_secret_prompt_never_renders_what_was_typed() {
    // Two of the five flows through this prompt are the MotherDuck token and
    // the AI key. `InputState` masked them with `.masked(true)`; an `<input>`
    // needs `type="password"`, and getting it wrong puts a credential on screen
    // and into any screen recording — with nothing else about the dialog
    // looking different.
    let plain = prompt("");
    assert_eq!(
        plain
            .attr(plain.by_a11y_id("name-prompt-field").unwrap(), "type")
            .as_deref(),
        Some("text"),
    );

    let secret = secret_prompt();
    assert_eq!(
        secret
            .attr(secret.by_a11y_id("name-prompt-field").unwrap(), "type")
            .as_deref(),
        Some("password"),
        "a token prompt is rendering its value in clear text"
    );
}

#[test]
fn enter_in_the_field_confirms_once_and_goes_no_further() {
    // Two guarantees that only mean something together. `InputState` owned its
    // Enter and emitted one `PressEnter`; an `<input>` inside a live keydown
    // tree does not, so the field has to consume it — and the GPUI suite's
    // sibling `escape_with_field_focused_emits_exactly_one_cancel` exists
    // because the cell-editor slice shipped an Enter that fired twice.
    let mut h = prompt("");
    let field = h.by_a11y_id("name-prompt-field").unwrap();
    h.dispatch(field, "input", form("q2 revenue"));
    h.key(field, Key::Enter, Modifiers::empty());

    assert_eq!(captured(&h), "confirm[q2 revenue]");
    assert_eq!(
        behind(),
        Vec::<String>::new(),
        "Enter reached the surface behind the prompt as well as confirming it"
    );
}

#[test]
fn escape_in_the_field_is_left_for_the_dialog_to_act_on() {
    // The exact inverse of the rule above, and the reason it cannot simply be
    // "the field eats its keys". Escape is the modal's — `Dat0Modal` binds it
    // and the host turns it into a Cancel — so a field that swallowed Escape
    // would make a prompt uncancellable from the one control that is focused
    // when it opens. GPUI had the same split: `InputState` bound Enter but
    // Escape resolved further up the dispatch stack.
    let mut h = prompt("draft");
    let field = h.by_a11y_id("name-prompt-field").unwrap();
    h.key(field, Key::Escape, Modifiers::empty());

    assert_eq!(
        behind(),
        vec!["Escape".to_string()],
        "the field consumed Escape, so no dialog above it can cancel"
    );
    assert_eq!(
        captured(&h),
        "",
        "and cancelling is not the field's decision to make"
    );
}

#[test]
fn a_chord_the_field_does_not_claim_still_reaches_the_shell() {
    // ⌘K opens the palette, and it has to keep working while a prompt is up:
    // GPUI let an unconsumed keystroke fall through to the global scope, and a
    // field that stopped everything typed into it would make every global chord
    // dead inside a dialog. A bare character has the same obligation — the
    // field reads it, and lets it go.
    let mut h = prompt("draft");
    let field = h.by_a11y_id("name-prompt-field").unwrap();
    h.key(field, Key::Character("k".into()), Modifiers::META);
    h.key(field, Key::Character("q".into()), Modifiers::empty());

    assert_eq!(behind(), vec!["k".to_string(), "q".to_string()]);
    assert_eq!(
        captured(&h),
        "",
        "neither is a confirm, and the field decided otherwise"
    );
}

#[test]
fn the_prompts_two_buttons_are_native_buttons_a_keyboard_can_operate() {
    // GPUI's Probe 4 Tab-walked the modal to prove "Save"/"Cancel" were stops,
    // because `focus_stop` had to be applied by hand and could be forgotten.
    // Here the element type is the guarantee — and the `disabled` attribute is
    // load-bearing twice over, since the trap's focus ring skips a disabled
    // button, so a disarmed Save must not be a stop the user lands on.
    let h = prompt("");
    for id in ["name-prompt-ok", "name-prompt-cancel"] {
        let k = h.by_a11y_id(id).unwrap_or_else(|| panic!("{id} renders"));
        assert_eq!(
            h.dom().get(k).tag(),
            Some("button"),
            "{id} is not a native button, so Enter and Space do nothing on it"
        );
        assert!(h.attr(k, "aria-label").is_some(), "{id} has no name");
        assert!(h.has_listener(k, "click"), "{id} is wired to nothing");
    }
    assert_eq!(
        h.attr(h.by_a11y_id("name-prompt-ok").unwrap(), "disabled")
            .as_deref(),
        Some("true"),
        "an empty prompt arms a Save the consumer would silently drop",
    );

    let mut typed = prompt("");
    let field = typed.by_a11y_id("name-prompt-field").unwrap();
    typed.dispatch(field, "input", form("q2"));
    assert_eq!(
        typed
            .attr(typed.by_a11y_id("name-prompt-ok").unwrap(), "disabled")
            .as_deref(),
        Some("false"),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Every other field the substitution touched
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn every_settings_field_is_a_real_input_that_announces_itself() {
    // `InputState` carried its own placeholder, its own accessible name and its
    // own change notification. Split across markup, each is a separate thing to
    // forget — and a field with no accessible name is one a screen-reader user
    // cannot identify, while a field with no `input` listener is one that types
    // and then throws the value away.
    //
    // Walked rather than listed, so a field added next week is covered without
    // this test being edited.
    let mut seen = 0;
    for section in SECTIONS {
        let store = Arc::new(SettingsStore::open_in_memory());
        let mut h = Harness::new(
            SettingsPanel,
            SettingsProps {
                store: Store(store),
                events: Bus(None),
            },
        );
        h.click(section.id);

        for k in h.dom().walk() {
            if h.dom().get(k).tag() != Some("input") {
                continue;
            }
            seen += 1;
            let id = h.attr(k, "data-a11y-id").unwrap_or_default();
            assert!(
                h.attr(k, "aria-label").is_some(),
                "the {} section has an unnamed field: {id:?}",
                section.id
            );
            assert!(
                h.has_listener(k, "input") || h.has_listener(k, "change"),
                "{id:?} in the {} section discards what is typed into it",
                section.id
            );
        }
    }

    // A walk that found nothing passes for free. Settings ships the name, the
    // email and the memory budget, so three is the floor; a section that stops
    // rendering its fields is a failure, not a quiet pass.
    assert!(
        seen >= 3,
        "only {seen} settings fields were walked; the test found nothing to check"
    );
}
