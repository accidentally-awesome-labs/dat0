//! The saved-query picker.
//!
//! Ported from `view/saved_query_picker.rs`. The LISTBOX pattern: ONE tab stop
//! on the container plus an active index that arrows move, never a focus stop
//! per row — a hundred saved queries must not cost a hundred Tabs.
//!
//! The picker never mutates. It emits a pick or a delete and the caller owns
//! both (`WorkspaceShell::delete_named_query` in the GPUI build), so the list
//! it renders is always whatever the caller last handed it.
//!
//! Arrows CLAMP here. Only radio groups wrap — see `export_dialog::cycle_ix`.

use dioxus::prelude::*;
use uuid::Uuid;

use dat0_core::session::queries::SavedQuery;

use crate::a11y::AccessRole;

#[derive(Clone, Props, PartialEq)]
pub struct SavedQueriesProps {
    /// The saved queries, in display order. Re-read by the caller on every
    /// change, so a delete shrinks this on the next render.
    pub queries: Vec<SavedQuery>,
    /// Load this query. The caller closes the picker.
    pub on_pick: EventHandler<SavedQuery>,
    /// Delete by id. The list stays open — deleting three in a row should not
    /// cost three round trips through the command that opened it.
    pub on_delete: EventHandler<Uuid>,
}

#[component]
pub fn SavedQueriesPicker(props: SavedQueriesProps) -> Element {
    let mut active = use_signal(|| 0usize);
    let rows = props.queries.clone();
    let len = rows.len();
    // A delete can leave the index past the end; clamp before rendering so the
    // ring lands on a row that exists.
    let active_ix = active().min(len.saturating_sub(1));

    let on_pick = props.on_pick;
    let on_delete = props.on_delete;
    let rows_for_keys = rows.clone();

    rsx! {
        div { class: "d0-picker", "data-a11y-id": "saved-queries",

            div {
                class: "d0-picker-list",
                "data-a11y-id": "sql-saved-list",
                role: "listbox",
                "aria-label": dat0_i18n::t("sql.load_query"),
                "aria-activedescendant": if len > 0 { format!("saved-row-{active_ix}") } else { String::new() },
                tabindex: "0",
                onkeydown: move |e| {
                    match e.key() {
                        Key::ArrowDown => {
                            e.prevent_default();
                            active.set((active_ix + 1).min(len.saturating_sub(1)));
                        }
                        Key::ArrowUp => {
                            e.prevent_default();
                            active.set(active_ix.saturating_sub(1));
                        }
                        Key::Delete | Key::Backspace => {
                            // An empty list has no active row, so this is a
                            // no-op rather than a panic.
                            if let Some(q) = rows_for_keys.get(active_ix) {
                                e.prevent_default();
                                on_delete.call(q.id);
                            }
                        }
                        // Enter/Space load the active row — the other half of
                        // the listbox keyboard contract.
                        k if k == Key::Enter || k == Key::Character(" ".into()) => {
                            if let Some(q) = rows_for_keys.get(active_ix) {
                                e.prevent_default();
                                on_pick.call(q.clone());
                            }
                        }
                        _ => {}
                    }
                },

                if rows.is_empty() {
                    div { class: "d0-row is-empty", "data-a11y-id": "saved-empty",
                        "{dat0_i18n::t(\"sql.saved.empty\")}"
                    }
                } else {
                    for (i , q) in rows.iter().cloned().enumerate() {
                        div {
                            key: "{q.id}",
                            id: "saved-row-{i}",
                            class: if i == active_ix { "d0-row is-active" } else { "d0-row" },
                            "data-a11y-id": "saved-row-{i}",
                            role: "option",
                            "aria-selected": if i == active_ix { "true" } else { "false" },
                            "aria-label": q.name.clone(),
                            onclick: {
                                let q = q.clone();
                                move |_| {
                                    active.set(i);
                                    on_pick.call(q.clone());
                                }
                            },
                            span { class: "d0-row-name d0-mono", "{q.name}" }
                            button {
                                class: "d0-btn is-ghost d0-picker-del",
                                "data-a11y-id": "saved-del-{i}",
                                role: AccessRole::Button.aria(),
                                "aria-label": dat0_i18n::t("sql.saved.delete"),
                                onclick: {
                                    let id = q.id;
                                    move |e: MouseEvent| {
                                        // Without this the row's own click also
                                        // fires and the query is loaded on its
                                        // way out. GPUI's picker had the same
                                        // shape and the same latent bug; a DOM
                                        // event bubbles every time.
                                        e.stop_propagation();
                                        on_delete.call(id);
                                    }
                                },
                                "✕"
                            }
                        }
                    }
                }
            }
        }
    }
}
