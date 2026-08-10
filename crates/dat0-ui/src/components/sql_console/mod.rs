//! The SQL console.
//!
//! A pane containing a tab strip, a toolbar, the CodeMirror editor, the
//! transient preview / error strips, and a run-cancel control. The editor
//! bridge is [`editor`]; the tab lifecycle is [`tabs`].
//!
//! # What disappeared with the widget library
//!
//! `view/sql_console.rs` carried a documented trap: `Panel::focus_handle` must
//! not be a tab stop, or `TabPanel` double-registers the `FocusId` and Tab
//! visits the console twice. There is no `TabPanel` here, so there is no trap —
//! the pane header is a `button` and the editor is a `div`, and each is a tab
//! stop exactly once.
//!
//! The second trap it carried is still here, and worth naming: gpui-component's
//! `Input` bound Tab to indent, so a forward Tab-walk that reached the editor
//! never left it, and every transient bar had to auto-focus itself on appear to
//! be reachable at all (`pending_focus`). CodeMirror's `indentWithTab` does the
//! same thing — a SQL editor wants Tab to indent — so the escape hatch survives
//! the migration: Escape inside the editor lands on the Run control, bound in
//! the bundle's own keymap because that is the layer that swallows the key.
//! `examples/console_probe.rs` proves both halves.
//!
//! What did change is the focus queue. `pending_focus` was a slot set from six
//! places and drained during render; it is now [`focus_target`], a function of
//! the state, and the control it names takes the keyboard from its own
//! `onmounted`. (Not `autofocus`: that is processed once per document, so the
//! Insert button that replaces Stop when a stream finishes would be ignored —
//! measured, not assumed.)
//!
//! # The console renders; it never acts
//!
//! Every control emits a [`ConsoleIntent`] and stops. The host owns the tab
//! list ([`tabs::Tabs`]), the engine, the modal slot and the history — so a
//! run, a save and a history pick each have exactly one implementation, in the
//! shell's router, rather than one here and one wherever else the same command
//! is reachable from.

pub mod editor;
pub mod tabs;

use std::collections::BTreeMap;

use dioxus::prelude::*;

use dat0_core::query::ResultTarget;
use dat0_core::query::completion::SharedSnapshot;

use crate::a11y::AccessRole;
use crate::components::ai::{StreamKind, StreamPhase, StreamView};
use crate::theme::Theme;
use editor::{Editor, EditorCmd, EditorMsg};

/// One console tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    /// Stable id; also the editor instance id and the mount element's id.
    pub id: String,
    pub title: String,
    pub doc: String,
}

/// What the console asks the shell to do. The console never touches the engine
/// itself — a run is one decision made in one place.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsoleIntent {
    /// Run the tab's statement into the main grid or into the console's own
    /// result pane.
    Run {
        tab: String,
        sql: String,
        target: ResultTarget,
    },
    Cancel {
        tab: String,
    },
    DocChanged {
        tab: String,
        doc: String,
    },
    /// Open an empty tab.
    NewTab,
    /// Close the showing tab. A no-op on the last one — see
    /// [`tabs::Tabs::close_active`].
    CloseTab,
    /// Show the query-history library.
    ShowHistory,
    /// Save the showing statement under a name.
    SaveQuery {
        tab: String,
        sql: String,
    },
    /// Open the saved-query picker.
    LoadQuery,
    /// Materialise the showing statement as a table.
    SaveAsTable {
        tab: String,
        sql: String,
    },
    /// Stop the streaming NL→SQL or Explain answer.
    StopStream,
    /// Take the generated SQL into a new tab.
    InsertGenerated {
        sql: String,
    },
    /// Throw the generated SQL away.
    DiscardStream,
    /// Close a finished Explain.
    CloseExplain,
    /// Dismiss the failed-run strip.
    DismissError,
}

#[derive(Clone, Props)]
pub struct SqlConsoleProps {
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// The schema the editor completes against, shared with every tab.
    pub schema: SharedSnapshot,
    /// True while a statement is in flight.
    #[props(default = false)]
    pub running: bool,
    /// The NL→SQL / Explain preview strip. A `kind` of `None`, or an `Idle`
    /// phase, means no strip.
    #[props(default)]
    pub stream: StreamView,
    /// The failed-run strip. `None` means no strip.
    #[props(default)]
    pub error: Option<String>,
    pub on_intent: EventHandler<ConsoleIntent>,
    pub on_select_tab: EventHandler<usize>,
}

// The schema snapshot is shared, mutable, and refreshed from a background task;
// comparing its *contents* to decide whether to re-render would both take the
// lock on every diff and miss the point. Identity is the question: a different
// snapshot is a different window.
impl PartialEq for SqlConsoleProps {
    fn eq(&self, other: &Self) -> bool {
        self.tabs == other.tabs
            && self.active == other.active
            && self.running == other.running
            && self.stream == other.stream
            && self.error == other.error
            && std::sync::Arc::ptr_eq(&self.schema, &other.schema)
    }
}

/// Is the strip showing at all, and is it still arriving?
fn strip_phase(s: &StreamView) -> Option<(StreamKind, bool)> {
    let kind = s.kind?;
    match s.phase {
        StreamPhase::Idle => None,
        StreamPhase::Streaming => Some((kind, true)),
        StreamPhase::Done | StreamPhase::Failed => Some((kind, false)),
    }
}

/// The `data-a11y-id` of each control a transient bar can hand the keyboard to.
pub const STREAM_STOP: &str = "console-stream-stop";
pub const STREAM_INSERT: &str = "console-stream-insert";
pub const STREAM_DISCARD: &str = "console-stream-discard";
pub const STREAM_CLOSE: &str = "console-stream-close";
pub const ERROR_DISMISS: &str = "console-error-dismiss";

/// Which control the console gives the keyboard to, for the bars that are up.
///
/// This is `pending_focus` — the slot `view/sql_console.rs` set from six places
/// and drained during render — written as the decision it always was. Having it
/// as a function rather than a slot is what makes the rule assertable without
/// a window, and makes "and here we forgot to re-home it" impossible to spell.
///
/// A run that fails is the one bar that takes nothing: the caret stays in the
/// statement you are fixing.
pub fn focus_target(stream: &StreamView, error: Option<&str>) -> Option<&'static str> {
    match (strip_phase(stream), error) {
        (Some((_, true)), _) => Some(STREAM_STOP),
        (Some((StreamKind::NlToSql, false)), _) => Some(STREAM_INSERT),
        (Some((StreamKind::Explain, false)), _) => Some(STREAM_CLOSE),
        (None, Some(_)) => None,
        (None, None) => None,
    }
}

#[component]
pub fn SqlConsole(props: SqlConsoleProps) -> Element {
    let theme = Theme::use_current();
    let tabs = props.tabs.clone();
    let active = props.active.min(tabs.len().saturating_sub(1));
    let on_intent = props.on_intent;
    let on_select_tab = props.on_select_tab;
    let running = props.running;
    let stream = props.stream.clone();
    let strip = strip_phase(&stream);
    let error = props.error.clone();
    let focus = focus_target(&stream, error.as_deref());
    let tab_count = tabs.len();

    // One channel per window. Cmd-Enter is bound inside CodeMirror's keymap, so
    // a run arrives here as a `run` message rather than through the shell's
    // chord cascade — the editor owns its keys while it has focus.
    let ed = Editor::use_channel(move |msg| match msg {
        EditorMsg::Run { id, doc } => on_intent.call(ConsoleIntent::Run {
            tab: id,
            sql: doc,
            target: ResultTarget::MainGrid,
        }),
        EditorMsg::Change { id, doc } => on_intent.call(ConsoleIntent::DocChanged { tab: id, doc }),
        // `id` is empty for the bundle's own boot ping, and the tab id when
        // an instance mounts.
        EditorMsg::Ready { id } => tracing::debug!(tab = %id, "editor ready"),
        EditorMsg::Cursor { .. } => {}
    });

    let active_tab = tabs.get(active).cloned();
    let sql = active_tab
        .as_ref()
        .map(|t| t.doc.clone())
        .unwrap_or_default();
    let tab_id = active_tab
        .as_ref()
        .map(|t| t.id.clone())
        .unwrap_or_default();

    // Boot the active tab's editor, and re-init when the tab changes. The
    // schema is read at init: `@codemirror/lang-sql` wants the whole map, not
    // an incremental feed.
    //
    // `use_reactive` on the tab id is load-bearing. An effect only re-runs when
    // a *signal* it read changes, and props are not signals — without it the
    // body would fire once, at boot, and switching tabs would leave the
    // previous document on screen under the new tab's title.
    {
        let tabs = tabs.clone();
        let schema = props.schema.clone();
        let vars = editor::theme_vars(&theme.tokens());
        use_effect(use_reactive!(|tab_id| {
            // Also depends on the channel: the bundle is still loading on the
            // first pass, so this must re-run once it is ready.
            if !ed.is_open() || tab_id.is_empty() {
                return;
            }
            let Some(tab) = tabs.iter().find(|t| t.id == tab_id) else {
                return;
            };
            let snap = schema.lock();
            ed.send(EditorCmd::Init {
                id: tab.id.clone(),
                mount: format!("cm-{}", tab.id),
                doc: tab.doc.clone(),
                schema: snap.schema_map(),
                functions: snap.functions.clone(),
                vars: vars.clone(),
            });
        }));
    }

    // A theme switch re-themes the editor in place rather than remounting it,
    // which would lose the caret and the undo history.
    {
        let tabs = tabs.clone();
        use_effect(move || {
            if !ed.is_open() {
                return;
            }
            let vars: BTreeMap<String, String> = editor::theme_vars(&theme.tokens());
            if let Some(tab) = tabs.get(active) {
                ed.send(EditorCmd::Theme {
                    id: tab.id.clone(),
                    vars,
                });
            }
        });
    }

    // Closing a transient bar hands the keyboard back to the editor.
    //
    // In a document, removing the focused element drops focus to `<body>` and
    // the next keystroke goes nowhere — so this is the same guarantee GPUI
    // spelled as `pending_focus`, minus the queue: [`focus_target`] names the
    // control that takes the keyboard, and this hands it back.
    let transient_open = strip.is_some() || props.error.is_some();
    {
        let id = tab_id.clone();
        let mut was_open = use_signal(|| false);
        use_effect(use_reactive!(|transient_open| {
            if *was_open.peek() && !transient_open && ed.is_open() && !id.is_empty() {
                ed.send(EditorCmd::Focus { id: id.clone() });
            }
            was_open.set(transient_open);
        }));
    }

    // The console's rung of the Escape ladder. `keys::Cascade` has already
    // taken Escape for a modal or the palette by the time it reaches here, so
    // what is left is the console's own transient surfaces, innermost first:
    // the preview strip, then the failed-run strip. With neither open the key
    // is not ours and must keep bubbling.
    let escape = {
        move |e: KeyboardEvent| {
            if e.key() != Key::Escape {
                return;
            }
            match (strip, error.is_some()) {
                (Some((_, true)), _) => {
                    e.stop_propagation();
                    on_intent.call(ConsoleIntent::StopStream);
                }
                (Some((StreamKind::Explain, false)), _) => {
                    e.stop_propagation();
                    on_intent.call(ConsoleIntent::CloseExplain);
                }
                (Some((StreamKind::NlToSql, false)), _) => {
                    e.stop_propagation();
                    on_intent.call(ConsoleIntent::DiscardStream);
                }
                (None, true) => {
                    e.stop_propagation();
                    on_intent.call(ConsoleIntent::DismissError);
                }
                (None, false) => {}
            }
        }
    };

    rsx! {
        div {
            class: "d0-console",
            "data-a11y-id": "sql-console",
            onkeydown: escape,

            div { class: "d0-console-tabs", role: "tablist", "data-a11y-id": "console-tabstrip",
                for (i , tab) in tabs.iter().enumerate() {
                    button {
                        key: "{tab.id}",
                        class: if i == active { "d0-console-tab is-active" } else { "d0-console-tab" },
                        "data-a11y-id": "console-tab-{tab.id}",
                        role: "tab",
                        "aria-selected": if i == active { "true" } else { "false" },
                        "aria-label": "{tab.title}",
                        // Roving tab index: the strip is one stop and the
                        // arrows move within it, so Tab does not have to walk
                        // every open tab to leave the console.
                        tabindex: if i == active { "0" } else { "-1" },
                        onclick: move |_| on_select_tab.call(i),
                        onkeydown: move |e: KeyboardEvent| {
                            match e.key() {
                                Key::ArrowLeft | Key::ArrowRight => {
                                    let delta = if e.key() == Key::ArrowLeft { -1 } else { 1 };
                                    e.stop_propagation();
                                    e.prevent_default();
                                    on_select_tab.call(tabs::step_index(active, delta, tab_count));
                                }
                                Key::Delete | Key::Backspace => {
                                    e.stop_propagation();
                                    e.prevent_default();
                                    on_intent.call(ConsoleIntent::CloseTab);
                                }
                                _ => {}
                            }
                        },
                        "{tab.title}"
                    }
                }
                div { style: "margin-left: auto" }
                if running {
                    button {
                        class: "d0-chip",
                        "data-a11y-id": "console-cancel",
                        role: AccessRole::Button.aria(),
                        "aria-label": dat0_i18n::t("sql.cancel"),
                        tabindex: "0",
                        onclick: {
                            let tab_id = tab_id.clone();
                            move |_| on_intent.call(ConsoleIntent::Cancel { tab: tab_id.clone() })
                        },
                        span { class: "d0-dot is-live" }
                        {dat0_i18n::t("sql.cancel")}
                    }
                } else {
                    button {
                        class: "d0-chip",
                        "data-a11y-id": "console-run",
                        role: AccessRole::Button.aria(),
                        "aria-label": dat0_i18n::t("sql.run"),
                        tabindex: "0",
                        onclick: {
                            let (tab_id, sql) = (tab_id.clone(), sql.clone());
                            move |_| {
                                on_intent
                                    .call(ConsoleIntent::Run {
                                        tab: tab_id.clone(),
                                        sql: sql.clone(),
                                        target: ResultTarget::MainGrid,
                                    })
                            }
                        },
                        span { style: "color: var(--d0-ok)", {dat0_i18n::t("sql.run")} }
                        span { class: "d0-key", "⌘⏎" }
                    }
                }
            }

            // The fixed toolbar. Every one of these is also a command-palette
            // action; both routes end at the same intent, and the buttons exist
            // so the commands are discoverable without knowing they are there.
            div { class: "d0-console-toolbar", "data-a11y-id": "console-toolbar",
                Tool {
                    id: "console-run-pane",
                    label: dat0_i18n::t("sql.run_in_pane"),
                    on_act: {
                        let (tab_id, sql) = (tab_id.clone(), sql.clone());
                        move |_| {
                            on_intent
                                .call(ConsoleIntent::Run {
                                    tab: tab_id.clone(),
                                    sql: sql.clone(),
                                    target: ResultTarget::Pane,
                                })
                        }
                    },
                }
                Tool {
                    id: "console-new-tab",
                    label: dat0_i18n::t("sql.new_tab"),
                    on_act: move |_| on_intent.call(ConsoleIntent::NewTab),
                }
                Tool {
                    id: "console-history",
                    label: dat0_i18n::t("sql.history"),
                    on_act: move |_| on_intent.call(ConsoleIntent::ShowHistory),
                }
                Tool {
                    id: "console-save-query",
                    label: dat0_i18n::t("sql.save_query"),
                    on_act: {
                        let (tab_id, sql) = (tab_id.clone(), sql.clone());
                        move |_| {
                            on_intent
                                .call(ConsoleIntent::SaveQuery {
                                    tab: tab_id.clone(),
                                    sql: sql.clone(),
                                })
                        }
                    },
                }
                Tool {
                    id: "console-load-query",
                    label: dat0_i18n::t("sql.load_query"),
                    on_act: move |_| on_intent.call(ConsoleIntent::LoadQuery),
                }
                Tool {
                    id: "console-save-as-table",
                    label: dat0_i18n::t("sql.save_as_table"),
                    on_act: {
                        let (tab_id, sql) = (tab_id.clone(), sql.clone());
                        move |_| {
                            on_intent
                                .call(ConsoleIntent::SaveAsTable {
                                    tab: tab_id.clone(),
                                    sql: sql.clone(),
                                })
                        }
                    },
                }
            }

            if let Some((kind, streaming)) = strip {
                div {
                    class: "d0-console-strip",
                    "data-a11y-id": "console-stream",
                    "data-kind": kind.id(),
                    "data-phase": stream.phase.id(),
                    role: AccessRole::Label.aria(),
                    pre { class: "d0-mono", "data-a11y-id": "console-stream-text", "{stream.text}" }
                    if let Some(err) = stream.error.clone() {
                        span { class: "d0-error", "data-a11y-id": "console-stream-error", "{err}" }
                    }
                    if streaming {
                        Tool {
                            id: STREAM_STOP,
                            label: dat0_i18n::t("sql.ai.stop"),
                            takes_focus: focus == Some(STREAM_STOP),
                            on_act: move |_| on_intent.call(ConsoleIntent::StopStream),
                        }
                    } else if kind == StreamKind::NlToSql {
                        Tool {
                            id: STREAM_INSERT,
                            label: dat0_i18n::t("sql.nl2sql.insert"),
                            takes_focus: focus == Some(STREAM_INSERT),
                            on_act: {
                                let text = stream.text.clone();
                                move |_| {
                                    on_intent
                                        .call(ConsoleIntent::InsertGenerated {
                                            sql: text.clone(),
                                        })
                                }
                            },
                        }
                        Tool {
                            id: STREAM_DISCARD,
                            label: dat0_i18n::t("sql.nl2sql.discard"),
                            takes_focus: focus == Some(STREAM_DISCARD),
                            on_act: move |_| on_intent.call(ConsoleIntent::DiscardStream),
                        }
                    } else {
                        Tool {
                            id: STREAM_CLOSE,
                            label: dat0_i18n::t("sql.explain.close"),
                            takes_focus: focus == Some(STREAM_CLOSE),
                            on_act: move |_| on_intent.call(ConsoleIntent::CloseExplain),
                        }
                    }
                }
            }

            if let Some(tab) = active_tab {
                editor::EditorMount { id: tab.id.clone() }
            } else {
                div { class: "d0-row is-empty", {dat0_i18n::t("sql.no_tabs")} }
            }

            // Below the editor, and the one bar `focus_target` never names: a
            // failed run must not yank the caret out of the statement you are
            // fixing.
            if let Some(msg) = props.error.clone() {
                div {
                    class: "d0-console-strip is-error",
                    "data-a11y-id": "console-error",
                    role: AccessRole::Alert.aria(),
                    span { class: "d0-mono", "data-a11y-id": "console-error-text", "{msg}" }
                    Tool {
                        id: ERROR_DISMISS,
                        label: dat0_i18n::t("sql.error.dismiss"),
                        on_act: move |_| on_intent.call(ConsoleIntent::DismissError),
                    }
                }
            }
        }
    }
}

/// One console button.
///
/// `<button>` gives Enter/Space activation and the focus ring for free — the
/// whole of what GPUI's `focus_stop` had to hand-roll, and the reason the
/// ported tests activate by click rather than by simulating Enter on a `div`.
///
/// `takes_focus` is the replacement for `pending_focus`. It is **not** the
/// `autofocus` attribute: that is processed once per document, so the first
/// bar to appear would take the keyboard and every later one — including the
/// Insert that replaces Stop when a stream finishes — would be ignored, which
/// is exactly the "focus dropped to nowhere" the original guarded against.
/// `onmounted` fires on every mount.
#[component]
fn Tool(
    id: &'static str,
    label: String,
    #[props(default = false)] takes_focus: bool,
    on_act: EventHandler<()>,
) -> Element {
    rsx! {
        button {
            class: "d0-btn",
            "data-a11y-id": "{id}",
            role: AccessRole::Button.aria(),
            "aria-label": "{label}",
            tabindex: "0",
            onmounted: move |e: Event<MountedData>| {
                if takes_focus {
                    spawn(async move {
                        // The result is dropped deliberately. dioxus-desktop's
                        // `set_focus` resolves its eval to `null`, so the typed
                        // future always reports a deserialization error — while
                        // the element does get focus. `console_probe` asserts
                        // the focus, which is the part that matters; logging
                        // the error would cry wolf on every bar that appears.
                        let _ = e.set_focus(true).await;
                    });
                }
            },
            onclick: move |_| on_act.call(()),
            "{label}"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_intent_names_its_tab() {
        // A run must carry which tab it came from: with several open, routing
        // by "the active one" races the click that changed it.
        let r = ConsoleIntent::Run {
            tab: "console-1".into(),
            sql: "SELECT 1".into(),
            target: ResultTarget::MainGrid,
        };
        match r {
            ConsoleIntent::Run { tab, .. } => assert_eq!(tab, "console-1"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn a_strip_with_no_kind_is_no_strip() {
        assert!(strip_phase(&StreamView::default()).is_none());
    }

    #[test]
    fn an_idle_stream_shows_nothing_even_with_a_kind() {
        let s = StreamView {
            kind: Some(StreamKind::NlToSql),
            phase: StreamPhase::Idle,
            ..StreamView::default()
        };
        assert!(strip_phase(&s).is_none());
    }

    #[test]
    fn a_failed_stream_still_offers_its_buttons() {
        // Failed is finished, not absent: the strip must stay up long enough
        // to show why, and to be dismissed.
        let s = StreamView {
            kind: Some(StreamKind::Explain),
            phase: StreamPhase::Failed,
            error: Some("boom".into()),
            ..StreamView::default()
        };
        assert_eq!(strip_phase(&s), Some((StreamKind::Explain, false)));
    }
}
