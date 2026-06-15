//! Reusable single-line name-prompt modal (P5b). Emits the entered name on
//! confirm, or Cancelled. Used by Save Query + Save as Table.
use gpui::prelude::*;
use gpui::{Context, Entity, EventEmitter, ParentElement, SharedString, Styled, Window, div};
use gpui_component::input::{Input, InputState};

#[derive(Debug, Clone)]
pub enum NamePromptEvent {
    Confirm(String),
    Cancel,
}

pub struct NamePrompt {
    title: SharedString,
    input: Entity<InputState>,
}

impl NamePrompt {
    pub fn new(
        title: impl Into<SharedString>,
        initial: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // `default_value` seeds the field eagerly at build time (no later
        // `set_value` + `&mut Window` juggling). Empty seed → behaves exactly as
        // before for the Save-query / Save-as-table flows.
        let initial = initial.into();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("name")
                .default_value(initial)
        });
        Self {
            title: title.into(),
            input,
        }
    }

    fn value(&self, cx: &gpui::App) -> String {
        self.input.read(cx).value().to_string()
    }
}

impl EventEmitter<NamePromptEvent> for NamePrompt {}

impl Render for NamePrompt {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .child(self.title.clone())
            .child(Input::new(&self.input))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .id("name-prompt-ok")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .child(SharedString::from("Save"))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                let v = this.value(cx);
                                cx.emit(NamePromptEvent::Confirm(v));
                            })),
                    )
                    .child(
                        div()
                            .id("name-prompt-cancel")
                            .px_3()
                            .py_1()
                            .cursor_pointer()
                            .child(SharedString::from("Cancel"))
                            .on_click(cx.listener(|_this, _ev, _window, cx| {
                                cx.emit(NamePromptEvent::Cancel);
                            })),
                    ),
            )
    }
}
