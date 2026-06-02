//! SQL Console view (P5a §2.2): a collapsible bottom panel with a tab strip,
//! a gpui-component code editor per tab, a Run/Cancel button, and a result
//! region (inline error/status strip; optional results pane). Emits
//! [`SqlConsoleEvent`] to `WorkspaceShell`, which owns the run/cancel execution
//! (wired in P5a T6/T7).
//!
//! # Construction
//!
//! Unlike the filter popover (which defers `InputState::new` to first render),
//! the console is constructed with a real `&mut Window` — `WorkspaceShell`
//! lazily builds it inside `toggle_sql_console`, which has a `Window`. So each
//! tab's `InputState` code-editor is created eagerly in [`SqlConsole::new`].
//!
//! # Highlighting
//!
//! Each tab's editor is built via `InputState::new(window, cx).code_editor("sql")`.
//! The "sql" grammar was registered at boot (P5a T4 — `query::highlight`), so
//! the runtime `LanguageRegistry` drives tree-sitter-sequel highlighting.

use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, IntoElement, ParentElement, SharedString, Styled, Window, div,
};
use gpui_component::input::{Input, InputState};

use crate::query::{ResultTarget, SqlTabMeta};

/// One open console tab: persistable metadata + the live editor buffer.
pub struct ConsoleTab {
    pub meta: SqlTabMeta,
    pub input: Entity<InputState>,
}

/// What the result region currently shows.
#[derive(Debug, Clone)]
pub enum ResultRegion {
    Empty,
    /// "42 rows changed" / "OK" — a DML/DDL status line.
    Status(String),
    /// A DuckDB error message.
    Error(String),
    Cancelled,
    /// The result is showing in the main DataGrid (no inline region needed).
    BoundToGrid,
}

/// The SQL Console GPUI entity. Owns the open tabs + the result-region state.
/// Execution (run/cancel) is driven by `WorkspaceShell` via [`SqlConsoleEvent`].
pub struct SqlConsole {
    pub tabs: Vec<ConsoleTab>,
    pub active: usize,
    pub region: ResultRegion,
    pub running: bool,
}

/// Events the console emits up to `WorkspaceShell`.
#[derive(Debug, Clone)]
pub enum SqlConsoleEvent {
    /// User pressed Run; `target` is where the result should render.
    Run { target: ResultTarget },
    /// User pressed Cancel (while a run is in flight).
    Cancel,
    /// Tab set / active index changed; persist to the session.
    Persist,
}

impl EventEmitter<SqlConsoleEvent> for SqlConsole {}

impl SqlConsole {
    /// Build from persisted SQL tabs (or a single empty "Query 1" if none).
    ///
    /// Requires `&mut Window` because each tab's code-editor `InputState`
    /// is constructed eagerly (the caller — `toggle_sql_console` — has one).
    pub fn new(
        persisted: &[crate::session::SqlTabState],
        active: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut tabs: Vec<ConsoleTab> = persisted
            .iter()
            .map(|p| {
                let sql = p.sql.clone();
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .code_editor("sql")
                        .line_number(true)
                        .placeholder(dat0_i18n::t("sql.placeholder"))
                });
                input.update(cx, |s, cx| s.set_value(sql, window, cx));
                ConsoleTab {
                    meta: SqlTabMeta {
                        id: p.id,
                        title: p.title.clone(),
                        result_target: ResultTarget::MainGrid,
                        last_result_view: None,
                    },
                    input,
                }
            })
            .collect();
        if tabs.is_empty() {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .code_editor("sql")
                    .line_number(true)
                    .placeholder(dat0_i18n::t("sql.placeholder"))
            });
            tabs.push(ConsoleTab {
                meta: SqlTabMeta::new("Query 1"),
                input,
            });
        }
        let active = active.unwrap_or(0).min(tabs.len() - 1);
        Self {
            tabs,
            active,
            region: ResultRegion::Empty,
            running: false,
        }
    }

    /// The currently-active tab.
    // Used by T6 (Run reads the active tab's editor buffer).
    #[allow(dead_code)]
    pub fn active_tab(&self) -> &ConsoleTab {
        &self.tabs[self.active]
    }

    /// Snapshot all tabs to the persistable shape (for `Session::set_sql_tabs`).
    ///
    /// Takes `&App` (not `&Context<Self>`) so `WorkspaceShell` can call it with
    /// its own `Context<WorkspaceShell>` (which derefs to `App`) after a
    /// `console.read(cx)` — `Entity::read` for each tab's `InputState` only
    /// needs `&App`.
    // Used by T6/T10 (persist on run / tab mutation).
    #[allow(dead_code)]
    pub fn snapshot(&self, cx: &gpui::App) -> (Vec<crate::session::SqlTabState>, Option<usize>) {
        let tabs = self
            .tabs
            .iter()
            .map(|t| crate::session::SqlTabState {
                id: t.meta.id,
                title: t.meta.title.clone(),
                sql: t.input.read(cx).value().to_string(),
            })
            .collect();
        (tabs, Some(self.active))
    }
}

impl Render for SqlConsole {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active;

        // ── Tab strip ──────────────────────────────────────────────────────
        let tab_strip = div()
            .flex()
            .flex_row()
            .gap_1()
            .children(self.tabs.iter().enumerate().map(|(i, t)| {
                let title: SharedString = t.meta.title.clone().into();
                let mut tab = div()
                    .id(("sql-tab", i))
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .child(title)
                    .on_click(cx.listener(move |this, _ev, _window, cx| {
                        this.active = i;
                        cx.emit(SqlConsoleEvent::Persist);
                        cx.notify();
                    }));
                if i == active {
                    tab = tab.border_b_1();
                }
                tab
            }));

        // ── Run / Cancel button ────────────────────────────────────────────
        let run_label = if self.running {
            dat0_i18n::t("sql.cancel")
        } else {
            dat0_i18n::t("sql.run")
        };
        let run_btn = div()
            .id("sql-run")
            .px_3()
            .py_1()
            .cursor_pointer()
            .child(SharedString::from(run_label))
            .on_click(cx.listener(|this, _ev, _window, cx| {
                if this.running {
                    cx.emit(SqlConsoleEvent::Cancel);
                } else {
                    cx.emit(SqlConsoleEvent::Run {
                        target: ResultTarget::MainGrid,
                    });
                }
            }));

        // ── Editor for the active tab ───────────────────────────────────────
        let editor = Input::new(&self.tabs[active].input).h_full();

        // ── Result region (inline status / error strip) ─────────────────────
        let region: gpui::AnyElement = match &self.region {
            ResultRegion::Empty | ResultRegion::BoundToGrid => div().into_any_element(),
            ResultRegion::Status(s) => div()
                .px_2()
                .py_1()
                .child(SharedString::from(s.clone()))
                .into_any_element(),
            ResultRegion::Error(e) => div()
                .px_2()
                .py_1()
                .child(SharedString::from(e.clone()))
                .into_any_element(),
            ResultRegion::Cancelled => div()
                .px_2()
                .py_1()
                .child(SharedString::from(dat0_i18n::t("sql.cancelled")))
                .into_any_element(),
        };

        // ── Assemble ─────────────────────────────────────────────────────────
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .px_2()
                    .py_1()
                    .child(tab_strip)
                    .child(run_btn),
            )
            .child(div().flex_1().child(editor))
            .child(region)
    }
}
