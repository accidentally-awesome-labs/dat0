//! The shared single-line name prompt.
//!
//! Ported from `view/name_prompt.rs`. Five flows reuse it — save query, save
//! as table, save view as table, save chart, and the two secret prompts
//! (MotherDuck token, AI key/model) — so every rule here is load-bearing for
//! all of them.
//!
//! # The rejection rules, and where they come from
//!
//! GPUI's prompt validated nothing: it emitted `Confirm(value)` and each
//! consumer silently returned on a bad value (`save_named_query`,
//! `save_named_chart` and `save_view_as_table` all open with
//! `if name.trim().is_empty() { return; }`). Pressing Save on an empty field
//! therefore closed the modal and did nothing, with no explanation.
//!
//! The rules are the same rules; they are enforced where the user can see
//! them. Confirm is disabled and Enter is inert rather than the modal closing
//! on a value that would be dropped downstream.
//!
//! The control-character rule is new only in the sense that GPUI got it for
//! free: `InputState::new(..)` built a **single-line** field, which structurally
//! could not hold a newline. A plain `<input>` accepts a pasted one, so what
//! was a property of the widget has to become a check.
//!
//! The value is emitted **untrimmed**. `save_named_query` trims it itself, and
//! the token prompt must not have its payload rewritten by a UI component.

use dioxus::prelude::*;

use crate::a11y::AccessRole;

/// Why a value was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    /// Nothing but whitespace — every consumer drops this.
    Empty,
    /// A control character, almost always a newline from a paste.
    Control,
}

impl NameError {
    pub fn message(self) -> String {
        match self {
            NameError::Empty => dat0_i18n::t("prompt.error.empty"),
            NameError::Control => dat0_i18n::t("prompt.error.control"),
        }
    }
}

/// Check a prompt value against the rules every consumer relies on.
pub fn validate(value: &str) -> Result<(), NameError> {
    if value.trim().is_empty() {
        return Err(NameError::Empty);
    }
    if value.chars().any(char::is_control) {
        return Err(NameError::Control);
    }
    Ok(())
}

#[derive(Clone, Props, PartialEq)]
pub struct NamePromptProps {
    /// What is being named. Rendered as the field's label — the dialog frame's
    /// own header is the modal host's.
    pub title: String,
    /// Seed text. `default_value` in GPUI; empty for the save flows.
    #[props(default)]
    pub initial: String,
    /// Field placeholder. Defaults to the GPUI prompt's `"name"`.
    #[props(default)]
    pub placeholder: Option<String>,
    /// Confirm button text. Defaults to `Save`, which is what GPUI hard-coded.
    #[props(default)]
    pub confirm_label: Option<String>,
    /// Mask the field. Set for the MotherDuck token and the AI key, which are
    /// the two flows whose value must not survive in a screen recording.
    #[props(default = false)]
    pub secret: bool,
    /// The value, exactly as typed. Consumers trim if they mean to.
    pub on_confirm: EventHandler<String>,
    pub on_cancel: EventHandler<()>,
}

#[component]
pub fn NamePrompt(props: NamePromptProps) -> Element {
    let mut value = use_signal(|| props.initial.clone());
    // Nothing is wrong until something has been typed: a save flow opens with
    // an empty field by design, and greeting it with "enter a name" is noise.
    let mut touched = use_signal(|| false);

    let error = validate(&value()).err();
    let showing = if touched() { error } else { None };
    let placeholder = props
        .placeholder
        .clone()
        .unwrap_or_else(|| dat0_i18n::t("prompt.placeholder"));
    let confirm_label = props
        .confirm_label
        .clone()
        .unwrap_or_else(|| dat0_i18n::t("prompt.save"));

    let on_confirm = props.on_confirm;
    let on_cancel = props.on_cancel;
    let mut confirm = move || {
        let v = value();
        if validate(&v).is_err() {
            // Enter on a bad value is inert rather than a silent dismiss —
            // the GPUI prompt closed and the consumer dropped it.
            touched.set(true);
            return;
        }
        on_confirm.call(v);
    };

    rsx! {
        div { class: "d0-prompt", "data-a11y-id": "name-prompt",

            label {
                class: "d0-label",
                r#for: "name-prompt-field",
                "data-a11y-id": "name-prompt-title",
                "{props.title}"
            }

            input {
                id: "name-prompt-field",
                class: "d0-field",
                "data-a11y-id": "name-prompt-field",
                r#type: if props.secret { "password" } else { "text" },
                role: "textbox",
                "aria-label": props.title.clone(),
                "aria-invalid": if showing.is_some() { "true" } else { "false" },
                placeholder,
                value: "{value}",
                // The GPUI prompt focused the field in `new` so a keyboard
                // user could type immediately; this is that.
                autofocus: true,
                oninput: move |e| {
                    touched.set(true);
                    value.set(e.value());
                },
                onkeydown: move |e| {
                    if e.key() == Key::Enter {
                        // Enter in the single-line field submits, mirroring the
                        // confirm button — GPUI subscribed to `PressEnter` for
                        // exactly this, and it must not also submit a form.
                        e.prevent_default();
                        e.stop_propagation();
                        confirm();
                    }
                },
            }

            if let Some(err) = showing {
                div {
                    class: "d0-form-error d0-mono",
                    "data-a11y-id": "name-prompt-error",
                    role: AccessRole::Alert.aria(),
                    "aria-label": err.message(),
                    "{err.message()}"
                }
            }

            div { class: "d0-form-actions",
                button {
                    class: "d0-btn is-primary",
                    "data-a11y-id": "name-prompt-ok",
                    role: AccessRole::Button.aria(),
                    "aria-label": confirm_label.clone(),
                    disabled: error.is_some(),
                    onclick: move |_| confirm(),
                    "{confirm_label}"
                }
                button {
                    class: "d0-btn is-ghost",
                    "data-a11y-id": "name-prompt-cancel",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("common.cancel"),
                    onclick: move |_| on_cancel.call(()),
                    "{dat0_i18n::t(\"common.cancel\")}"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_is_the_same_as_empty() {
        // `save_named_query` trims before its emptiness check, so a field of
        // spaces was already a no-op; it just did not say so.
        assert_eq!(validate(""), Err(NameError::Empty));
        assert_eq!(validate("   \t "), Err(NameError::Empty));
        assert!(validate("q2").is_ok());
    }

    #[test]
    fn a_pasted_newline_is_refused() {
        // The GPUI field was single-line and could not hold one; an `<input>`
        // can, so the constraint has to be checked instead of assumed.
        assert_eq!(validate("a\nb"), Err(NameError::Control));
        assert_eq!(validate("a\tb"), Err(NameError::Control));
    }

    #[test]
    fn surrounding_space_does_not_reject_a_real_name() {
        // Emitted untrimmed: the consumer trims. Rejecting here would break a
        // token pasted with a trailing space, which is not ours to rewrite.
        assert!(validate("  q2  ").is_ok());
    }
}
