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

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, IntoElement, ParentElement, SharedString, Styled, WeakEntity,
    Window, div,
};
use gpui_component::input::{Input, InputState};
use gpui_component::spinner::Spinner;
use gpui_component::table::{Table, TableState};

use crate::grid::{GridDataSource, GridTableDelegate};
use crate::query::{ResultTarget, SqlTabMeta};
use crate::window::WorkspaceShell;

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
    /// The result is showing in the console-owned results pane (P5a T9). The
    /// bound `GridDataSource` lives in `SqlConsole::pane_source`; `render`
    /// lazily promotes it to a `TableState` and mounts the pane grid.
    Pane,
}

/// The SQL Console GPUI entity. Owns the open tabs + the result-region state.
/// Execution (run/cancel) is driven by `WorkspaceShell` via [`SqlConsoleEvent`].
pub struct SqlConsole {
    pub tabs: Vec<ConsoleTab>,
    pub active: usize,
    pub region: ResultRegion,
    pub running: bool,
    /// When the active run started, for the live elapsed-seconds counter (T7).
    /// `Some` while `running`, `None` otherwise. Read in `render`.
    pub started_at: Option<std::time::Instant>,
    /// The `GridDataSource` bound by a `Run { target: Pane }` result (P5a T9).
    /// Stored by [`set_pane_source`](Self::set_pane_source); `render` lazily
    /// promotes it into `pane_table_state` (the `TableState::new` call needs a
    /// `&mut Window`, only available inside `render` — mirrors the main grid's
    /// lazy-promotion discipline in `WorkspaceShell::render`).
    pub(crate) pane_source: Option<Arc<GridDataSource>>,
    /// The console-owned results grid built from `pane_source` (P5a T9).
    /// `None` until the first render after a Pane result lands, then rebuilt
    /// whenever `pane_source` swaps to a different `Arc`.
    pub(crate) pane_table_state: Option<Entity<TableState<GridTableDelegate>>>,
    /// Weak handle to the owning `WorkspaceShell`, passed in by `set_pane_source`
    /// so the pane delegate's header/scroll closures can dispatch into the shell
    /// (P5a T9). `new_invalid()` until a Pane result binds.
    pub(crate) pane_ws: WeakEntity<WorkspaceShell>,
    /// Shared per-window schema cache for autocomplete (P5b T2). Cloned into
    /// every tab's [`SchemaCompletionProvider`]; refreshed off the engine by
    /// `WorkspaceShell::refresh_completion_snapshot`, so one update reaches all
    /// tabs (the `RefCell` is shared by `Rc`).
    pub(crate) snapshot: crate::query::completion::SharedSnapshot,
    /// Wall time (ms) of the most recently completed run, set by
    /// [`set_last_elapsed`](Self::set_last_elapsed) from `finish_sql_run`
    /// (P5b T4). Drives the timing chip (T9). `None` until the first run.
    pub(crate) last_elapsed_ms: Option<u64>,
    /// Transient query-history overlay (P5b T5). `Some(entries)` while the
    /// history panel is open; `None` when closed. Populated by
    /// [`show_history`](Self::show_history) (fed from the session by
    /// `WorkspaceShell` on a `ShowHistory` event) and rendered as an overlay
    /// inside [`render`](Self::render) — which owns the `&mut Window` a row
    /// click needs to load its SQL into a new tab.
    pub(crate) history_overlay: Option<Vec<crate::session::queries::HistoryEntry>>,
    /// SQL queued by a [`SqlConsoleEvent::LoadSql`] to load into a new tab on
    /// the next render (P5b T5/T8). `load_into_new_tab` needs a `&mut Window`,
    /// which only `render` has; the event handler on `WorkspaceShell` (reached
    /// from a windowless subscription) sets this via [`queue_load`](Self::queue_load),
    /// and `render` drains it. `None` when nothing is pending.
    pub(crate) pending_load: Option<String>,
}

/// Install the autocomplete provider on a freshly-built tab editor (P5b T2).
/// Shared by all three tab-build paths ([`SqlConsole::new`]'s persisted loop +
/// empty fallback, and [`SqlConsole::new_tab`]) so every editor gets a provider
/// backed by the same per-window snapshot.
fn attach_completion_provider(
    input: &Entity<InputState>,
    snapshot: &crate::query::completion::SharedSnapshot,
    cx: &mut Context<SqlConsole>,
) {
    let snap = snapshot.clone();
    input.update(cx, |s, _cx| {
        s.lsp.completion_provider = Some(std::rc::Rc::new(
            crate::query::completion::SchemaCompletionProvider { snapshot: snap },
        ));
    });
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
    /// Open the query-history panel. `WorkspaceShell` fetches the entries from
    /// the session and pushes them back into the console via
    /// [`SqlConsole::show_history`] (the console owns the `Window`-having render).
    ShowHistory,
    /// Load `sql` into a new tab (from history or a saved query).
    LoadSql(String),
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
        snapshot: crate::query::completion::SharedSnapshot,
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
                attach_completion_provider(&input, &snapshot, cx);
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
            attach_completion_provider(&input, &snapshot, cx);
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
            started_at: None,
            pane_source: None,
            pane_table_state: None,
            pane_ws: WeakEntity::new_invalid(),
            snapshot,
            last_elapsed_ms: None,
            history_overlay: None,
            pending_load: None,
        }
    }

    /// The active tab's full SQL + the editor cursor byte offset. Cursor-only
    /// (T0 spike: no public selection accessor exists at this gpui-component
    /// rev — `selected_range`/`selected_text()` are `pub(super)`; only
    /// `cursor() -> usize` is public).
    ///
    /// Takes `&gpui::App` (not `&Context<Self>`) for the same reason as
    /// [`snapshot`](Self::snapshot): `WorkspaceShell::spawn_sql_run` calls it
    /// with its own `Context<WorkspaceShell>` after a `console.read(cx)`, where
    /// `Entity::read` on the tab's `InputState` only needs `&App`.
    pub fn active_sql_and_cursor(&self, cx: &gpui::App) -> (String, usize) {
        let st = self.tabs[self.active].input.read(cx);
        (st.value().to_string(), st.cursor())
    }

    /// Replace the result-region state and request a re-render (T6 run path).
    pub fn set_region(&mut self, region: ResultRegion, cx: &mut Context<Self>) {
        self.region = region;
        cx.notify();
    }

    /// Toggle the running spinner / Run↔Cancel button label (T6 run path).
    ///
    /// Stamps `started_at` when the run begins so `render` can show the live
    /// elapsed-seconds counter, and clears it when the run ends (T7).
    pub fn set_running(&mut self, running: bool, cx: &mut Context<Self>) {
        self.running = running;
        self.started_at = if running {
            Some(std::time::Instant::now())
        } else {
            None
        };
        cx.notify();
    }

    /// Record the most recent run's wall time (P5b T4/T9). Drives the timing chip.
    pub fn set_last_elapsed(&mut self, ms: u64, cx: &mut Context<Self>) {
        self.last_elapsed_ms = Some(ms);
        cx.notify();
    }

    /// Bind a result-producing run's `GridDataSource` to the console-owned
    /// results pane (P5a T9, `Run { target: Pane }`). Stores the `Arc` + the
    /// owning shell's weak handle, drops any stale `TableState` (it wrapped the
    /// previous source), and kicks a background prefetch of the first page so
    /// the pane paints real values rather than em-dash placeholders on the next
    /// frame.
    ///
    /// `TableState::new` is NOT called here: it needs a `&mut Window`, which
    /// this method (reached from `WorkspaceShell::finish_sql_run`, a
    /// dispatcher callback with only `&mut App`) does not have. The actual
    /// `TableState` promotion happens lazily in [`render`](Self::render), which
    /// owns a `&mut Window` — exactly the discipline the main grid uses in
    /// `WorkspaceShell::render`.
    pub fn set_pane_source(
        &mut self,
        ds: Arc<GridDataSource>,
        ws_weak: WeakEntity<WorkspaceShell>,
        cx: &mut Context<Self>,
    ) {
        // Drop the stale state — it wrapped the previous run's delegate/source
        // and would paint stale rows. `render` rebuilds from the new `Arc`.
        self.pane_table_state = None;
        self.pane_ws = ws_weak;

        // Prefetch the first page of the new source so the synchronous
        // `render_td` finds cached rows on the next frame. The fetch runs OFF
        // the GPUI main thread; the re-render notify is posted back via the
        // canonical `MainThreadDispatcher` (same discipline as
        // `WorkspaceShell::prefetch_visible_rows`). Scroll-paging is handled by
        // the delegate's `visible_rows_changed` hook via the shell.
        if !ds.is_empty() {
            let ds_for_task = Arc::clone(&ds);
            let this = cx.entity().downgrade();
            tokio::spawn(async move {
                if let Err(e) = ds_for_task.page_for(0).await {
                    tracing::warn!(error = %e, "set_pane_source: first-page prefetch failed");
                    return;
                }
                if let Some(dispatcher) = crate::window_registry::dispatcher() {
                    let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                        if let Some(h) = this.upgrade() {
                            h.update(app_cx, |_c, cx| cx.notify());
                        }
                    });
                }
            });
        }

        self.pane_source = Some(ds);
        cx.notify();
    }

    /// Append a fresh empty tab and focus it (P5a T10). Eagerly builds the new
    /// editor's `InputState` (needs `&mut Window` — same construction as
    /// [`SqlConsole::new`]) and emits [`SqlConsoleEvent::Persist`] so the new
    /// tab set reaches `session.json` immediately.
    pub fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.tabs.len() + 1;
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("sql")
                .line_number(true)
                .placeholder(dat0_i18n::t("sql.placeholder"))
        });
        let snapshot = self.snapshot.clone();
        attach_completion_provider(&input, &snapshot, cx);
        self.tabs.push(ConsoleTab {
            meta: SqlTabMeta::new(format!("Query {n}")),
            input,
        });
        self.active = self.tabs.len() - 1;
        cx.emit(SqlConsoleEvent::Persist);
        cx.notify();
    }

    /// Open a new tab pre-filled with `sql` (P5b T5 history / saved-query load).
    ///
    /// Mirrors [`new_tab`](Self::new_tab)'s construction exactly — same eager
    /// `InputState` code-editor build (needs `&mut Window`) and the SAME
    /// [`attach_completion_provider`] helper (T2), so the loaded tab gets
    /// autocomplete just like a freshly-added one — then seeds the buffer with
    /// `sql` via `set_value` (also `&mut Window`) and focuses the new tab.
    /// Emits [`SqlConsoleEvent::Persist`] so the new tab set reaches
    /// `session.json` immediately.
    pub fn load_into_new_tab(&mut self, sql: String, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.tabs.len() + 1;
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("sql")
                .line_number(true)
                .placeholder(dat0_i18n::t("sql.placeholder"))
        });
        let snapshot = self.snapshot.clone();
        attach_completion_provider(&input, &snapshot, cx);
        input.update(cx, |s, cx| s.set_value(sql, window, cx));
        self.tabs.push(ConsoleTab {
            meta: SqlTabMeta::new(format!("Query {n}")),
            input,
        });
        self.active = self.tabs.len() - 1;
        cx.emit(SqlConsoleEvent::Persist);
        cx.notify();
    }

    /// Open the query-history overlay with `entries` (P5b T5). Called by
    /// `WorkspaceShell::on_sql_console_event` after a `ShowHistory` event, with
    /// the entries pulled from `session.query_history()`. `render` then mounts
    /// the list; picking a row loads it into a new tab and closes the overlay.
    pub fn show_history(
        &mut self,
        entries: Vec<crate::session::queries::HistoryEntry>,
        cx: &mut Context<Self>,
    ) {
        self.history_overlay = Some(entries);
        cx.notify();
    }

    /// Queue `sql` to load into a new tab on the next render (P5b T5/T8). Used
    /// by the windowless `LoadSql` event path: `load_into_new_tab` needs a
    /// `&mut Window`, which only [`render`](Self::render) owns, so the SQL is
    /// stashed here and `render` drains it. Also closes any open history overlay.
    pub fn queue_load(&mut self, sql: String, cx: &mut Context<Self>) {
        self.history_overlay = None;
        self.pending_load = Some(sql);
        cx.notify();
    }

    /// Remove the tab at `ix`, keeping at least one open (P5a T10). Clamps the
    /// active index if the closed tab was at/after it, and emits
    /// [`SqlConsoleEvent::Persist`] so the trimmed tab set reaches `session.json`.
    ///
    /// The closed tab's `last_result_view` TEMP VIEW is not explicitly dropped:
    /// it is connection-scoped, so it is reclaimed on connection teardown
    /// (P5a leaves the GC to app/connection close — no engine DROP wiring here).
    pub fn close_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.tabs.len() == 1 {
            return; // keep at least one — never an empty console
        }
        self.tabs.remove(ix);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        cx.emit(SqlConsoleEvent::Persist);
        cx.notify();
    }

    /// Snapshot all tabs to the persistable shape (for `Session::set_sql_tabs`).
    ///
    /// Takes `&App` (not `&Context<Self>`) so `WorkspaceShell` can call it with
    /// its own `Context<WorkspaceShell>` (which derefs to `App`) after a
    /// `console.read(cx)` — `Entity::read` for each tab's `InputState` only
    /// needs `&App`.
    // Used by T6/T10 (persist on run / tab mutation). LIVE as of T6 (called by
    // `WorkspaceShell::persist_sql_console` after every run).
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Drain any SQL queued by a windowless `LoadSql` event (P5b T5/T8):
        // `load_into_new_tab` needs the `&mut Window` we now hold. Done before
        // reading `active` so the freshly-loaded tab becomes the active one.
        if let Some(sql) = self.pending_load.take() {
            self.load_into_new_tab(sql, window, cx);
        }
        let active = self.active;

        // ── Lazy-promote the Pane result source → TableState (P5a T9) ───────
        // Mirrors `WorkspaceShell::render`: `set_pane_source` stored the bound
        // `Arc<GridDataSource>` (no `Window` in that dispatcher callback); here
        // — inside `render`, where a `&mut Window` exists — we build the
        // delegate + `TableState`. Rebuilt only when the stored state wraps a
        // different source (a new Pane run). The delegate's columns derive from
        // the visible schema (empty `column_view` = identity), the read-only
        // fallback the unit-test path uses.
        if let Some(ds) = self.pane_source.as_ref() {
            let needs_rebuild = match self.pane_table_state.as_ref() {
                None => true,
                Some(state) => !state.read(cx).delegate().source_ptr_eq(ds),
            };
            if needs_rebuild {
                let delegate = GridTableDelegate::new(Arc::clone(ds), self.pane_ws.clone(), &[]);
                self.pane_table_state = Some(cx.new(|cx| TableState::new(delegate, window, cx)));
            }
        }

        // ── Tab strip ──────────────────────────────────────────────────────
        // Each tab is a clickable label (→ set active + Persist) with a small
        // "✕" close glyph (→ `close_tab`, which keeps ≥1 tab). A trailing "+"
        // appends a fresh tab via `new_tab`. No `.tooltip()` helper exists at
        // this gpui-component rev (T9), so the glyphs are the affordance; the
        // `sql.new_tab` / `sql.close_tab` i18n strings (T5) back a later tooltip
        // polish task.
        let tab_count = self.tabs.len();
        let tab_strip = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .children(self.tabs.iter().enumerate().map(|(i, t)| {
                let title: SharedString = t.meta.title.clone().into();
                let mut tab = div()
                    .id(("sql-tab", i))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .child(
                        div()
                            .id(("sql-tab-label", i))
                            .cursor_pointer()
                            .child(title)
                            .on_click(cx.listener(move |this, _ev, _window, cx| {
                                this.active = i;
                                cx.emit(SqlConsoleEvent::Persist);
                                cx.notify();
                            })),
                    );
                // Show the close glyph only when more than one tab is open —
                // `close_tab` is a no-op on the last tab, so hiding it avoids a
                // dead control.
                if tab_count > 1 {
                    tab = tab.child(
                        div()
                            .id(("sql-tab-close", i))
                            .cursor_pointer()
                            .child(SharedString::from("✕"))
                            .on_click(cx.listener(move |this, _ev, _window, cx| {
                                this.close_tab(i, cx);
                            })),
                    );
                }
                if i == active {
                    tab = tab.border_b_1();
                }
                tab
            }))
            .child(
                div()
                    .id("sql-tab-add")
                    .px_2()
                    .py_1()
                    .cursor_pointer()
                    .child(SharedString::from("+"))
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        this.new_tab(window, cx);
                    })),
            );

        // ── Run / Cancel split-button (primary Run + ▾ "run in pane") ───────
        // The primary segment runs Cancel while in flight, else Run→MainGrid.
        // The caret segment (idle only) runs Run→Pane, routing the result into
        // the console-owned results pane (P5a T9, Tier 1). A full dropdown menu
        // would be cosmetic; the caret directly emits the Pane target.
        let run_label = if self.running {
            dat0_i18n::t("sql.cancel")
        } else {
            dat0_i18n::t("sql.run")
        };
        let primary_btn = div()
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
        // The caret is only meaningful when idle (running shows Cancel only).
        let run_caret: gpui::AnyElement = if self.running {
            div().into_any_element()
        } else {
            // No `.tooltip()` helper exists at this gpui-component rev, so the
            // caret itself is the affordance for "Run in results pane"
            // (`dat0_i18n::t("sql.run_in_pane")`, from T5). A later polish task
            // can add a hover tooltip / full PopupMenu.
            div()
                .id("sql-run-pane")
                .px_2()
                .py_1()
                .cursor_pointer()
                .border_l_1()
                .child(SharedString::from("▾"))
                .on_click(cx.listener(move |_this, _ev, _window, cx| {
                    cx.emit(SqlConsoleEvent::Run {
                        target: ResultTarget::Pane,
                    });
                }))
                .into_any_element()
        };
        let run_btn = div()
            .flex()
            .flex_row()
            .items_center()
            .child(primary_btn)
            .child(run_caret);

        // ── Progress indicator (spinner + live elapsed seconds) ─────────────
        // Shown only while a run is in flight. The `Spinner` self-animates via
        // its own gpui `Animation`, so it spins without our help; the elapsed
        // counter needs a ~per-second repaint, scheduled below.
        let progress: gpui::AnyElement = if self.running {
            let elapsed = self.started_at.map(|t| t.elapsed().as_secs()).unwrap_or(0);
            let label = format!("{} {}s", dat0_i18n::t("sql.running"), elapsed);
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .px_2()
                .child(Spinner::new())
                .child(SharedString::from(label))
                .into_any_element()
        } else {
            div().into_any_element()
        };

        // Drive a ~1s repaint while running so the elapsed counter advances.
        // This schedules ONE delayed `notify` per render frame while running;
        // each notify re-renders, which (still running) schedules the next.
        // When `running` flips false, the next render does not schedule → the
        // loop self-terminates. (Redundant in-flight timers from extra renders
        // only cause harmless extra notifies — acceptable for P5a.)
        if self.running {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let _ = this.update(cx, |_this, cx| cx.notify());
            })
            .detach();
        }

        // ── Editor for the active tab ───────────────────────────────────────
        let editor = Input::new(&self.tabs[active].input).h_full();

        // ── Result region (inline status / error strip; or pane grid) ───────
        let region: gpui::AnyElement = match &self.region {
            ResultRegion::Empty | ResultRegion::BoundToGrid => div().into_any_element(),
            ResultRegion::Pane => {
                // P5a T9 (Tier 2): render the console-owned results grid. The
                // `TableState` was promoted above; while it is still being built
                // (first frame after bind, or a zero-row result) show a brief
                // placeholder mirroring the main grid's `(Some(_), None)` arm.
                match self.pane_table_state.as_ref() {
                    Some(state) => div()
                        .flex_1()
                        .min_h(gpui::px(120.0))
                        .child(Table::new(state).stripe(true).bordered(true))
                        .into_any_element(),
                    None => div()
                        .px_2()
                        .py_1()
                        .child(SharedString::from(dat0_i18n::t("sql.running")))
                        .into_any_element(),
                }
            }
            ResultRegion::Status(s) => div()
                .px_2()
                .py_1()
                .child(SharedString::from(s.clone()))
                .into_any_element(),
            ResultRegion::Error(e) => {
                let title = self.tabs[self.active].meta.title.clone();
                let msg = format!("{title}: {e}");
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .px_2()
                    .py_1()
                    .child(SharedString::from(msg))
                    .child(
                        div()
                            .id("sql-err-dismiss")
                            .cursor_pointer()
                            .px_1()
                            .child(SharedString::from("✕"))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.region = ResultRegion::Empty;
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            }
            ResultRegion::Cancelled => div()
                .px_2()
                .py_1()
                .child(SharedString::from(dat0_i18n::t("sql.cancelled")))
                .into_any_element(),
        };

        // ── Query-history overlay (P5b T5) ──────────────────────────────────
        // Mounted INSIDE the console's own render so a row click reaches a live
        // `&mut Window` (needed by `load_into_new_tab`). The pick closure
        // captures this entity (`cx.entity()`); the raw-`div` row `on_click`
        // forwards its `window`/`cx`, and `Entity::update` re-enters this entity
        // to load the SQL into a new tab AND close the overlay. A trailing close
        // affordance (✕) also clears it.
        let history_overlay: Option<gpui::AnyElement> =
            self.history_overlay.as_ref().map(|entries| {
                let this = cx.entity();
                let on_pick = move |sql: String, window: &mut Window, app: &mut gpui::App| {
                    this.update(app, |c, cx| {
                        c.history_overlay = None;
                        c.load_into_new_tab(sql, window, cx);
                    });
                };
                let close = cx.entity();
                div()
                    .absolute()
                    .top_8()
                    .right_2()
                    .w(gpui::px(420.))
                    .max_h(gpui::px(320.))
                    .overflow_hidden()
                    .border_1()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .justify_between()
                            .items_center()
                            .px_2()
                            .py_1()
                            .border_b_1()
                            .child(SharedString::from(dat0_i18n::t("sql.history")))
                            .child(
                                div()
                                    .id("sql-history-close")
                                    .cursor_pointer()
                                    .px_1()
                                    .child(SharedString::from("✕"))
                                    .on_click(move |_ev, _window, cx| {
                                        close.update(cx, |c, cx| {
                                            c.history_overlay = None;
                                            cx.notify();
                                        });
                                    }),
                            ),
                    )
                    .child(crate::view::query_library::render_history_list(
                        entries, on_pick,
                    ))
                    .into_any_element()
            });

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
                    .items_center()
                    .px_2()
                    .py_1()
                    .child(tab_strip)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(progress)
                            // ── Query-history clock (P5b T5) ──────────────────
                            // Emits `ShowHistory`; `WorkspaceShell` fetches the
                            // session's history and pushes it back via
                            // `show_history`, which opens the overlay below.
                            .child(
                                div()
                                    .id("sql-history")
                                    .px_2()
                                    .py_1()
                                    .cursor_pointer()
                                    .child(SharedString::from("🕘"))
                                    .on_click(cx.listener(|_this, _ev, _window, cx| {
                                        cx.emit(SqlConsoleEvent::ShowHistory);
                                    })),
                            )
                            .child(run_btn),
                    ),
            )
            .child(div().flex_1().child(editor))
            .child(region)
            .children(history_overlay)
    }
}
