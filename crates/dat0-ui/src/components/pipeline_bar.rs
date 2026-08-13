//! The pipeline bar: the active transform stack, as chips above the grid.
//!
//! Two shapes, same data. Collapsed it is a horizontal chip strip —
//! `base › Filter price › Sort qty↑` — and expanded it is a vertical timeline
//! with a remove button per step. The chevron switches between them.
//!
//! # The cursor is the undo scrubber
//!
//! `cursor` is how many of `stack`'s ops are active. Clicking chip `i` calls
//! `on_jump(i + 1)`, which routes to [`ViewModel::jump_to`] — "keep the first
//! `i + 1` ops, as one undo step". Chips at or past the cursor are the ops that
//! `jump_to` dropped: they stay visible and clickable so the gesture is
//! reversible by pointing at where you came from, but they are dimmed because
//! they are not what the grid is showing.
//!
//! Rendering them at all is the whole point of the widget. A bar that deleted
//! the chips it scrubbed past would be a list, not a scrubber, and the only way
//! back would be ⌘Z.
//!
//! [`ViewModel::jump_to`]: dat0_core::view::model::ViewModel::jump_to

use dioxus::prelude::*;

use dat0_core::view::pipeline_bar::describe_transform;
use dat0_engine::transform::Transformation;

use crate::a11y::{AccessRole, format_swatch};

#[derive(Clone, PartialEq, Props)]
pub struct PipelineBarProps {
    /// The tab's transform stack, oldest first.
    pub stack: Vec<Transformation>,
    /// How many of `stack`'s ops are active. Everything from here on is dimmed.
    pub cursor: usize,
    /// The base table's source file, when it came from one. Gives the base chip
    /// its S8 format swatch, so the strip says which *file* it is filtering.
    #[props(default)]
    pub source: Option<String>,
    /// Keep the first `k` ops. `0` returns to the base table.
    pub on_jump: EventHandler<usize>,
    /// Drop `stack[i]`.
    pub on_remove: EventHandler<usize>,
    /// Promote the active stack to a derived table.
    pub on_save_as_table: EventHandler<()>,
}

#[component]
pub fn PipelineBar(props: PipelineBarProps) -> Element {
    // No bar until a transform exists: an empty strip reading `base` on every
    // freshly-opened table is chrome that explains nothing.
    if props.stack.is_empty() {
        return rsx! {};
    }

    let mut expanded = use_signal(|| false);

    let on_jump = props.on_jump;
    let on_remove = props.on_remove;
    let on_save = props.on_save_as_table;
    let cursor = props.cursor;

    let base_label = props
        .source
        .clone()
        .unwrap_or_else(|| dat0_i18n::t("pipeline.base"));
    let swatch = props
        .source
        .as_deref()
        .map(|s| format_swatch(std::path::Path::new(s)));

    rsx! {
        div {
            class: "d0-pipeline",
            "data-a11y-id": "pipeline-bar",
            "aria-label": dat0_i18n::t("pipeline.label"),

            span { class: "d0-label", {dat0_i18n::t("pipeline.label")} }

            if expanded() {
                div { class: "d0-pipeline-steps",
                    button {
                        class: "d0-chip",
                        "data-a11y-id": "pipeline-base",
                        role: AccessRole::Button.aria(),
                        "aria-label": dat0_i18n::t("pipeline.base"),
                        onclick: move |_| on_jump.call(0),
                        if let Some(sw) = swatch {
                            span { class: "d0-sw {sw}" }
                        }
                        "{base_label}"
                    }
                    for (i, t) in props.stack.iter().enumerate() {
                        {step_row(i, t, cursor, on_jump, on_remove)}
                    }
                }
            } else {
                div { class: "d0-pipeline-strip",
                    button {
                        class: "d0-chip",
                        "data-a11y-id": "pipeline-base",
                        role: AccessRole::Button.aria(),
                        "aria-label": dat0_i18n::t("pipeline.base"),
                        onclick: move |_| on_jump.call(0),
                        if let Some(sw) = swatch {
                            span { class: "d0-sw {sw}" }
                        }
                        "{base_label}"
                    }
                    for (i, t) in props.stack.iter().enumerate() {
                        {chip(i, t, cursor, on_jump)}
                    }
                }
            }

            div { class: "d0-pipeline-tail",
                button {
                    class: "d0-chip",
                    "data-a11y-id": "pipeline-save-table",
                    role: AccessRole::Button.aria(),
                    "aria-label": dat0_i18n::t("view.save_as_table"),
                    onclick: move |_| on_save.call(()),
                    {dat0_i18n::t("view.save_as_table")}
                }
                button {
                    class: if expanded() { "d0-chevron" } else { "d0-chevron is-collapsed" },
                    "data-a11y-id": "pipeline-toggle",
                    role: AccessRole::Button.aria(),
                    "aria-expanded": if expanded() { "true" } else { "false" },
                    "aria-label": if expanded() { dat0_i18n::t("common.collapse") } else { dat0_i18n::t("common.expand") },
                    onclick: move |_| {
                        let now = expanded();
                        expanded.set(!now);
                    },
                    "▾"
                }
            }
        }
    }
}

/// One chip in the collapsed strip, preceded by its `then` separator.
fn chip(i: usize, t: &Transformation, cursor: usize, on_jump: EventHandler<usize>) -> Element {
    let label = describe_transform(t);
    let past = i >= cursor;
    rsx! {
        span { key: "{i}", class: "d0-pipeline-step",
            span {
                class: "d0-pipeline-sep",
                role: AccessRole::Label.aria(),
                "aria-label": dat0_i18n::t("pipeline.step_separator"),
                "›"
            }
            button {
                class: if past { "d0-chip is-past" } else { "d0-chip" },
                "data-a11y-id": "pipeline-chip-{i}",
                role: AccessRole::Button.aria(),
                "aria-label": "{label}",
                // Past chips stay live: clicking one is how you get back to it.
                onclick: move |_| on_jump.call(i + 1),
                "{label}"
            }
        }
    }
}

/// One row in the expanded timeline: the step, plus its remove button.
fn step_row(
    i: usize,
    t: &Transformation,
    cursor: usize,
    on_jump: EventHandler<usize>,
    on_remove: EventHandler<usize>,
) -> Element {
    let label = describe_transform(t);
    let past = i >= cursor;
    rsx! {
        div { key: "{i}", class: "d0-pipeline-row",
            span {
                class: "d0-pipeline-sep",
                role: AccessRole::Label.aria(),
                "aria-label": dat0_i18n::t("pipeline.step_separator"),
                "›"
            }
            button {
                class: if past { "d0-chip is-past" } else { "d0-chip" },
                "data-a11y-id": "pipeline-chip-{i}",
                role: AccessRole::Button.aria(),
                "aria-label": "{label}",
                onclick: move |_| on_jump.call(i + 1),
                "{label}"
            }
            button {
                class: "d0-pipeline-remove",
                "data-a11y-id": "pipeline-remove-{i}",
                role: AccessRole::Button.aria(),
                "aria-label": dat0_i18n::t("pipeline.remove_step"),
                onclick: move |_| on_remove.call(i),
                "✕"
            }
        }
    }
}
