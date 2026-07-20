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
    Context, Entity, EventEmitter, Focusable as _, IntoElement, ParentElement, SharedString,
    Styled, WeakEntity, Window, div,
};
use gpui_component::input::{Input, InputState};
use gpui_component::spinner::Spinner;
use gpui_component::table::{Table, TableState};

use crate::a11y::{A11yExt as _, AccessRole, FocusStopExt as _};
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

/// Live NL→SQL preview-strip state. `None` on `SqlConsole` = strip hidden.
#[derive(Debug, Clone)]
pub struct NlPreview {
    /// The NL prompt, echoed above the streamed SQL for context.
    pub prompt: String,
    /// Generated SQL, accumulated per SSE delta.
    pub sql: String,
    /// True while the stream is in flight (Stop shown); false → Insert/Discard.
    pub streaming: bool,
    /// Inline error (rate limit / refusal / HTTP body), shown red. Never blocks Insert.
    pub error: Option<String>,
}

impl NlPreview {
    pub fn new(prompt: String) -> Self {
        Self {
            prompt,
            sql: String::new(),
            streaming: true,
            error: None,
        }
    }
    pub fn push(&mut self, text: &str) {
        self.sql.push_str(text);
    }
    pub fn finish(&mut self, error: Option<String>) {
        self.streaming = false;
        self.error = error;
    }
}

/// Live Explain side-panel state. `None` on `SqlConsole` = panel hidden.
#[derive(Debug, Clone)]
pub struct ExplainView {
    /// The SQL that was explained, kept for context display.
    pub sql: String,
    /// Streamed plain-language explanation, accumulated per SSE delta.
    pub prose: String,
    /// True while the stream is in flight (Stop shown); false → Close.
    pub streaming: bool,
    /// Inline error (rate limit / refusal / HTTP body), shown red.
    pub error: Option<String>,
}

impl ExplainView {
    pub fn new(sql: String) -> Self {
        Self {
            sql,
            prose: String::new(),
            streaming: true,
            error: None,
        }
    }
    pub fn push(&mut self, text: &str) {
        self.prose.push_str(text);
    }
    pub fn finish(&mut self, error: Option<String>) {
        self.streaming = false;
        self.error = error;
    }
}

/// `pending_focus` sentinel meaning "the active tab's editor". Not a real
/// `toolbar_fh` id — `render` resolves it to the editor's `FocusHandle`.
const EDITOR_FOCUS: &str = "__editor__";

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
    /// Routing tag for the last run (P5c T9). Drives the chip suffix.
    pub(crate) last_routing: Option<crate::connections::routing::Routing>,
    /// Transient query-history overlay (P5b T5). `Some(entries)` while the
    /// history panel is open; `None` when closed. Populated by
    /// [`show_history`](Self::show_history) (fed from the session by
    /// `WorkspaceShell` on a `ShowHistory` event) and rendered as an overlay
    /// inside [`render`](Self::render) — which owns the `&mut Window` a row
    /// click needs to load its SQL into a new tab.
    pub(crate) history_overlay: Option<Vec<crate::session::queries::HistoryEntry>>,
    /// SQL queued to load into a new tab on the next render (P5b T8). The
    /// saved-query picker is a WINDOW-level overlay (no live `&mut Window` in its
    /// pick closure), but `load_into_new_tab` needs a `&mut Window`, which only
    /// [`render`](Self::render) holds; the picker's pick stashes the SQL here via
    /// [`queue_load`](Self::queue_load) and `render` drains it. `None` when
    /// nothing is pending.
    pub(crate) pending_load: Option<String>,
    /// Focus target queued for the next render (transient-bars nav, carve-out #7).
    /// Focus is a `&mut Window` op, but the setters that decide it (`begin_*`/
    /// `finish_*`/`show_history` + the Escape ladder) run in `Context<Self>` with
    /// no window in scope. Stash the target's `&'static str` id (a button id
    /// resolved via `toolbar_fh`, or [`EDITOR_FOCUS`] for the active editor) and
    /// let [`render`](Self::render) drain it — mirrors `pending_load`.
    pub(crate) pending_focus: Option<&'static str>,
    /// Active row index for the query-history overlay listbox (carve-out #7).
    /// Reset to 0 in [`show_history`](Self::show_history); clamped to the entry
    /// count at render. Mirrors `WorkspaceShell.recents_active`.
    // Written here; the read (history-overlay listbox nav) lands in a later
    // carve-out #7 slice — reserved field, mirrors `toast.rs`'s future-call-site
    // pattern.
    #[allow(dead_code)]
    pub(crate) history_active: usize,
    /// Live NL→SQL preview strip state (P9c-2 T6). `None` while hidden.
    pub(crate) nl_preview: Option<NlPreview>,
    /// Live Explain side-panel state (P9c-2 T7). `None` while hidden.
    pub(crate) explain: Option<ExplainView>,
    /// Whether AI is ready (enabled + key set + model non-empty). Set by the
    /// shell on AI config changes and on console creation. Gates the NL→SQL chip
    /// and the Explain button.
    pub(crate) ai_ready: bool,
    /// Stable focus handle for the NL→SQL chip (AI-config-nav slice). Minted once
    /// here so the chip is a stable Tab stop across re-renders.
    pub(crate) nl2sql_focus: gpui::FocusHandle,
    /// Stable focus handle for the Explain button (AI-config-nav slice). Minted
    /// once here so the button is a stable Tab stop across re-renders.
    pub(crate) explain_focus: gpui::FocusHandle,
    /// Toolbar-button focus handles (SQL-Console-nav slice), keyed by the
    /// control's `&'static str` id. Get-or-insert via [`Self::toolbar_fh`] so each
    /// button is a stable Tab stop across re-renders. Kept separate from the
    /// `nl2sql_focus`/`explain_focus` named fields, which predate this map.
    pub(crate) toolbar_focus: std::collections::HashMap<&'static str, gpui::FocusHandle>,
    /// Stable focus handle for the tab-strip tablist container (SQL-Console-nav
    /// slice). ONE stop for the whole strip; ←/→ switch the active tab.
    pub(crate) tabstrip_focus: gpui::FocusHandle,
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
    /// Open the Save-query name prompt. `WorkspaceShell` captures the active
    /// tab's SQL and mounts a [`NamePrompt`](crate::view::name_prompt::NamePrompt)
    /// overlay; confirming saves it to the session (P5b T8).
    SaveQuery,
    /// Open the saved-query picker. `WorkspaceShell` mounts a window-level
    /// overlay listing the session's saved queries; picking one queues it into a
    /// new tab and deleting removes it (P5b T8).
    ShowSaved,
    /// Promote the statement under the cursor to a derived table (P5b T10).
    /// `WorkspaceShell` opens a NamePrompt overlay; confirming runs a CTAS via
    /// `engine.create_table(.., DerivedOrigin::Sql)` and refreshes the
    /// autocomplete snapshot.
    SaveAsTable,
    /// Chip clicked — ask the shell to open the NL-prompt modal.
    OpenNl2SqlPrompt,
    /// Stop the in-flight AI stream (NL→SQL or Explain) — supersede guard.
    StopAiStream,
    /// Explain the whole active-tab buffer in a side panel (P9c-2 T7).
    Explain,
    /// Close the Explain side panel.
    CloseExplain,
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
            last_routing: None,
            history_overlay: None,
            pending_load: None,
            pending_focus: None,
            history_active: 0,
            nl_preview: None,
            explain: None,
            ai_ready: false,
            nl2sql_focus: cx.focus_handle(),
            explain_focus: cx.focus_handle(),
            toolbar_focus: std::collections::HashMap::new(),
            tabstrip_focus: cx.focus_handle(),
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

    /// Record the most recent run's wall time + routing (P5b T4 / P5c T9).
    /// Drives the timing chip.
    pub fn set_last_elapsed(
        &mut self,
        ms: u64,
        routing: crate::connections::routing::Routing,
        cx: &mut Context<Self>,
    ) {
        self.last_elapsed_ms = Some(ms);
        self.last_routing = Some(routing);
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

    /// Queue `sql` to load into a new tab on the next render (P5b T8). Used by
    /// the window-level saved-query picker, whose pick closure has only a
    /// `&mut App` (no live `&mut Window`); `load_into_new_tab` needs a
    /// `&mut Window`, which only [`render`](Self::render) owns, so the SQL is
    /// stashed here and `render` drains it. Also closes any open history overlay.
    pub fn queue_load(&mut self, sql: String, cx: &mut Context<Self>) {
        self.history_overlay = None;
        self.pending_load = Some(sql);
        cx.notify();
    }

    pub(crate) fn begin_nl_preview(&mut self, prompt: String, cx: &mut Context<Self>) {
        self.nl_preview = Some(NlPreview::new(prompt));
        self.pending_focus = Some("nl2sql-stop"); // streaming → focus Stop
        cx.notify();
    }
    pub(crate) fn push_nl_delta(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.nl_preview {
            p.push(text);
            cx.notify();
        }
    }
    pub(crate) fn finish_nl_preview(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.nl_preview {
            p.finish(error);
            self.pending_focus = Some("nl2sql-insert"); // re-home across Stop→Insert swap
            cx.notify();
        }
    }

    pub(crate) fn begin_explain(&mut self, sql: String, cx: &mut Context<Self>) {
        self.explain = Some(ExplainView::new(sql));
        cx.notify();
    }
    pub(crate) fn push_explain_delta(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(e) = &mut self.explain {
            e.push(text);
            cx.notify();
        }
    }
    pub(crate) fn finish_explain(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        if let Some(e) = &mut self.explain {
            e.finish(error);
            cx.notify();
        }
    }
    pub(crate) fn clear_explain(&mut self, cx: &mut Context<Self>) {
        self.explain = None;
        cx.notify();
    }

    /// True while any AI stream (NL→SQL or Explain) is actively streaming.
    /// Used to gate the chip + Explain button so they are disabled mid-flight.
    pub(crate) fn ai_busy(&self) -> bool {
        self.nl_preview.as_ref().is_some_and(|p| p.streaming)
            || self.explain.as_ref().is_some_and(|e| e.streaming)
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

    /// Get-or-insert the stable toolbar focus handle for `id` (SQL-Console-nav
    /// slice). Mirrors `WorkspaceShell::hero_focus_handle`; returns a CLONE so the
    /// caller can chain it into `focus_stop` without holding a borrow on the map.
    fn toolbar_fh(&mut self, id: &'static str, cx: &mut Context<Self>) -> gpui::FocusHandle {
        self.toolbar_focus
            .entry(id)
            .or_insert_with(|| cx.focus_handle())
            .clone()
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
        // Drain any SQL queued by the window-level saved-query picker (P5b T8):
        // its pick stashed the SQL via `queue_load` (no `&mut Window` there);
        // `load_into_new_tab` needs the `&mut Window` we now hold. Done before
        // reading `active` so the freshly-loaded tab becomes the active one.
        if let Some(sql) = self.pending_load.take() {
            self.load_into_new_tab(sql, window, cx);
        }
        // Drain any focus target queued by a transient-bar setter/handler
        // (carve-out #7). Done after the `pending_load` load so a freshly-opened
        // tab is already active when `EDITOR_FOCUS` resolves.
        if let Some(id) = self.pending_focus.take() {
            let fh = if id == EDITOR_FOCUS {
                self.tabs[self.active].input.read(cx).focus_handle(cx)
            } else {
                self.toolbar_fh(id, cx)
            };
            window.focus(&fh);
        }
        let active = self.active;
        let run_fh = self.toolbar_fh("sql-run", cx);
        let run_pane_fh = self.toolbar_fh("sql-run-pane", cx);
        let new_tab_fh = self.toolbar_fh("sql-tab-add", cx);
        let history_fh = self.toolbar_fh("sql-history", cx);
        let save_fh = self.toolbar_fh("sql-save", cx);
        let saved_fh = self.toolbar_fh("sql-saved", cx);
        let save_as_table_fh = self.toolbar_fh("sql-save-as-table", cx);
        let nl_stop_fh = self.toolbar_fh("nl2sql-stop", cx);
        let nl_insert_fh = self.toolbar_fh("nl2sql-insert", cx);
        let nl_discard_fh = self.toolbar_fh("nl2sql-discard", cx);
        let _explain_stop_fh = self.toolbar_fh("explain-stop", cx);
        let _explain_close_fh = self.toolbar_fh("explain-close", cx);
        let _err_dismiss_fh = self.toolbar_fh("sql-err-dismiss", cx);
        let _history_close_fh = self.toolbar_fh("sql-history-close", cx);
        let _history_list_fh = self.toolbar_fh("sql-history-list", cx);
        let tabstrip_fh = self.tabstrip_focus.clone();
        let tabstrip_name: String = self.tabs[self.active].meta.title.clone();

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
            .id("sql-tabstrip")
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
                    .focus_stop(
                        "sql-tab-add",
                        &new_tab_fh,
                        0,
                        cx.listener(|this, _ev: &gpui::KeyDownEvent, window, cx| {
                            this.new_tab(window, cx);
                        }),
                    )
                    .a11y(
                        "sql-tab-add",
                        AccessRole::Button,
                        dat0_i18n::t("sql.new_tab"),
                    )
                    .on_click(cx.listener(move |this, _ev, window, cx| {
                        this.new_tab(window, cx);
                    })),
            )
            // Enter/Space are a no-op: in the auto-activate model the tab under
            // the cursor is already the live tab.
            .focus_stop(
                "sql-tabstrip",
                &tabstrip_fh,
                0,
                cx.listener(|_this, _ev: &gpui::KeyDownEvent, _window, _cx| {}),
            )
            .a11y("sql-tabstrip", AccessRole::Button, tabstrip_name)
            // Second on_key_down for ←/→/Delete. gpui PUSHES key_down listeners, so
            // this coexists with focus_stop's Enter/Space listener (recents-nav R1).
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                let m = &ev.keystroke.modifiers;
                if m.shift || m.platform || m.control || m.alt {
                    return;
                }
                match ev.keystroke.key.as_str() {
                    "left" => {
                        if this.active > 0 {
                            this.active -= 1;
                            cx.emit(SqlConsoleEvent::Persist);
                            cx.notify();
                        }
                    }
                    "right" => {
                        if this.active + 1 < this.tabs.len() {
                            this.active += 1;
                            cx.emit(SqlConsoleEvent::Persist);
                            cx.notify();
                        }
                    }
                    "delete" | "backspace" => {
                        let a = this.active;
                        this.close_tab(a, cx); // no-op on the last tab; clamps active
                    }
                    _ => {}
                }
            }));

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
        let run_key = cx.listener(|this, _ev: &gpui::KeyDownEvent, _window, cx| {
            if this.running {
                cx.emit(SqlConsoleEvent::Cancel);
            } else {
                cx.emit(SqlConsoleEvent::Run {
                    target: ResultTarget::MainGrid,
                });
            }
        });
        let primary_btn = div()
            .id("sql-run")
            .px_3()
            .py_1()
            .cursor_pointer()
            .child(SharedString::from(run_label.clone()))
            .focus_stop("sql-run", &run_fh, 0, run_key)
            .a11y("sql-run", AccessRole::Button, run_label)
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
                .focus_stop(
                    "sql-run-pane",
                    &run_pane_fh,
                    0,
                    cx.listener(|_this, _ev: &gpui::KeyDownEvent, _window, cx| {
                        cx.emit(SqlConsoleEvent::Run {
                            target: ResultTarget::Pane,
                        });
                    }),
                )
                .a11y(
                    "sql-run-pane",
                    AccessRole::Button,
                    dat0_i18n::t("sql.run_in_pane"),
                )
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
                    None => {
                        // UAT Gap 2 (test-only): the pane's "running…" placeholder
                        // shown between bind and first-page prefetch. Content-only
                        // `Label` node; compiles out (identity no-op) in release.
                        let running = dat0_i18n::t("sql.running");
                        div()
                            .px_2()
                            .py_1()
                            .a11y_label(crate::a11y::AccessRole::Label, running.clone())
                            .child(SharedString::from(running))
                            .into_any_element()
                    }
                }
            }
            ResultRegion::Status(s) => div()
                .px_2()
                .py_1()
                // UAT Gap 2 (test-only): DML/DDL status line ("N rows changed"/"OK")
                // as a content-only `Label` node. No-op in release.
                .a11y_label(crate::a11y::AccessRole::Label, s.clone())
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
                    // UAT Gap 2 (test-only): the DuckDB error strip as a content-only
                    // `Alert` node (not `Label` — an error is an alert). No-op in release.
                    .a11y_label(crate::a11y::AccessRole::Alert, msg.clone())
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
            ResultRegion::Cancelled => {
                // UAT Gap 2 (test-only): the "Cancelled" strip as a content-only
                // `Label` node. No-op in release.
                let cancelled = dat0_i18n::t("sql.cancelled");
                div()
                    .px_2()
                    .py_1()
                    .a11y_label(crate::a11y::AccessRole::Label, cancelled.clone())
                    .child(SharedString::from(cancelled))
                    .into_any_element()
            }
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
                            .child({
                                // ── Timing chip (P5b T9) ──────────────────────
                                // Shows "⏱ N ms · local" when idle and a run has
                                // completed. Hidden while running (the progress
                                // spinner takes that slot) and before the first
                                // run. The "· local" suffix reserves the P5c
                                // local-vs-md slot.
                                let timing_chip: gpui::AnyElement =
                                    match (self.running, self.last_elapsed_ms) {
                                        (false, Some(ms)) => {
                                            let key = self
                                                .last_routing
                                                .map(|r| r.i18n_key())
                                                .unwrap_or("sql.local");
                                            let chip_text =
                                                format!("⏱ {ms} ms · {}", dat0_i18n::t(key));
                                            div()
                                                .px_2()
                                                .py_1()
                                                // UAT Gap 2 (test-only): the timing chip as a
                                                // content-only `Label` node so the harness can
                                                // assert it rendered. Compiles out in release.
                                                .a11y_label(
                                                    crate::a11y::AccessRole::Label,
                                                    chip_text.clone(),
                                                )
                                                .child(SharedString::from(chip_text))
                                                .into_any_element()
                                        }
                                        _ => div().into_any_element(),
                                    };
                                timing_chip
                            })
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
                                    .focus_stop(
                                        "sql-history",
                                        &history_fh,
                                        0,
                                        cx.listener(
                                            |_this, _ev: &gpui::KeyDownEvent, _window, cx| {
                                                cx.emit(SqlConsoleEvent::ShowHistory);
                                            },
                                        ),
                                    )
                                    .a11y(
                                        "sql-history",
                                        AccessRole::Button,
                                        dat0_i18n::t("sql.history"),
                                    )
                                    .on_click(cx.listener(|_this, _ev, _window, cx| {
                                        cx.emit(SqlConsoleEvent::ShowHistory);
                                    })),
                            )
                            // ── Save-query button (P5b T8) ────────────────────
                            // Emits `SaveQuery`; `WorkspaceShell` captures the
                            // active tab's SQL and opens a NamePrompt overlay.
                            .child(
                                div()
                                    .id("sql-save")
                                    .px_2()
                                    .py_1()
                                    .cursor_pointer()
                                    .child(SharedString::from("💾"))
                                    .focus_stop(
                                        "sql-save",
                                        &save_fh,
                                        0,
                                        cx.listener(
                                            |_this, _ev: &gpui::KeyDownEvent, _window, cx| {
                                                cx.emit(SqlConsoleEvent::SaveQuery);
                                            },
                                        ),
                                    )
                                    .a11y(
                                        "sql-save",
                                        AccessRole::Button,
                                        dat0_i18n::t("sql.save_query"),
                                    )
                                    .on_click(cx.listener(|_this, _ev, _window, cx| {
                                        cx.emit(SqlConsoleEvent::SaveQuery);
                                    })),
                            )
                            // ── Saved-query picker button (P5b T8) ────────────
                            // Emits `ShowSaved`; `WorkspaceShell` mounts the
                            // window-level saved-query picker overlay.
                            .child(
                                div()
                                    .id("sql-saved")
                                    .px_2()
                                    .py_1()
                                    .cursor_pointer()
                                    .child(SharedString::from("📑"))
                                    .focus_stop(
                                        "sql-saved",
                                        &saved_fh,
                                        0,
                                        cx.listener(
                                            |_this, _ev: &gpui::KeyDownEvent, _window, cx| {
                                                cx.emit(SqlConsoleEvent::ShowSaved);
                                            },
                                        ),
                                    )
                                    .a11y(
                                        "sql-saved",
                                        AccessRole::Button,
                                        dat0_i18n::t("sql.load_query"),
                                    )
                                    .on_click(cx.listener(|_this, _ev, _window, cx| {
                                        cx.emit(SqlConsoleEvent::ShowSaved);
                                    })),
                            )
                            // ── Save-as-Table button (P5b T10) ────────────────
                            // Emits `SaveAsTable`; `WorkspaceShell` opens a
                            // NamePrompt and CTAS-promotes the statement under the
                            // cursor to a derived table (`DerivedOrigin::Sql`).
                            .child(
                                div()
                                    .id("sql-save-as-table")
                                    .px_2()
                                    .py_1()
                                    .cursor_pointer()
                                    .child(SharedString::from("⤓ Table"))
                                    .focus_stop(
                                        "sql-save-as-table",
                                        &save_as_table_fh,
                                        0,
                                        cx.listener(
                                            |_this, _ev: &gpui::KeyDownEvent, _window, cx| {
                                                cx.emit(SqlConsoleEvent::SaveAsTable);
                                            },
                                        ),
                                    )
                                    .a11y(
                                        "sql-save-as-table",
                                        AccessRole::Button,
                                        dat0_i18n::t("sql.save_as_table"),
                                    )
                                    .on_click(cx.listener(|_this, _ev, _window, cx| {
                                        cx.emit(SqlConsoleEvent::SaveAsTable);
                                    })),
                            )
                            // ── NL→SQL chip (P9c-2 T6) ───────────────────────
                            // Disabled while any AI stream is in flight (T7
                            // follow-up: gate chip + Explain button consistently).
                            .child({
                                let busy = self.ai_busy();
                                let enabled = self.ai_ready && !busy;
                                let chip = div()
                                    .id("nl2sql-chip")
                                    .px_2()
                                    .py_1()
                                    .border_1()
                                    .when(enabled, |d| d.cursor_pointer())
                                    .child(SharedString::from(dat0_i18n::t("sql.nl2sql.chip")));
                                if enabled {
                                    let key = cx.listener(
                                        |_console, _ev: &gpui::KeyDownEvent, _window, cx| {
                                            cx.emit(SqlConsoleEvent::OpenNl2SqlPrompt);
                                        },
                                    );
                                    chip.focus_stop("nl2sql-chip", &self.nl2sql_focus, 0, key)
                                        .a11y(
                                            "nl2sql-chip",
                                            AccessRole::Button,
                                            dat0_i18n::t("sql.nl2sql.chip"),
                                        )
                                        .on_click(cx.listener(|_console, _ev, _window, cx| {
                                            cx.emit(SqlConsoleEvent::OpenNl2SqlPrompt);
                                        }))
                                } else {
                                    chip
                                }
                            })
                            // ── Explain button (P9c-2 T7) ────────────────────
                            // Disabled while AI not ready or a stream is in
                            // flight (streaming gate consistent with chip above).
                            .child({
                                let busy = self.ai_busy();
                                let enabled = self.ai_ready && !busy;
                                let btn = div()
                                    .id("sql-explain")
                                    .px_2()
                                    .py_1()
                                    .border_1()
                                    .when(enabled, |d| d.cursor_pointer())
                                    .child(SharedString::from(dat0_i18n::t("sql.explain.button")));
                                if enabled {
                                    let key = cx.listener(
                                        |_console, _ev: &gpui::KeyDownEvent, _window, cx| {
                                            cx.emit(SqlConsoleEvent::Explain);
                                        },
                                    );
                                    btn.focus_stop("sql-explain", &self.explain_focus, 0, key)
                                        .a11y(
                                            "sql-explain",
                                            AccessRole::Button,
                                            dat0_i18n::t("sql.explain.button"),
                                        )
                                        .on_click(cx.listener(|_console, _ev, _window, cx| {
                                            cx.emit(SqlConsoleEvent::Explain);
                                        }))
                                } else {
                                    btn
                                }
                            })
                            .child(run_btn),
                    ),
            )
            .child(div().flex_1().child(editor))
            .child(region)
            // ── NL→SQL preview strip (P9c-2 T6) ─────────────────────────────
            .children(self.nl_preview.as_ref().map(|p| {
                let mut strip = div().flex().flex_col().gap_1().p_2().border_t_1();
                strip = strip.child(div().child(SharedString::from(format!(
                    "{}: {}",
                    dat0_i18n::t("sql.nl2sql.prompt"),
                    p.prompt
                ))));
                strip = strip.child(div().child(SharedString::from(p.sql.clone())));
                if let Some(err) = &p.error {
                    strip = strip.child(div().child(SharedString::from(format!("✗ {err}"))));
                }
                if p.streaming {
                    let key = cx.listener(|_c, _ev: &gpui::KeyDownEvent, _w, cx| {
                        cx.emit(SqlConsoleEvent::StopAiStream);
                    });
                    strip = strip.child(
                        div()
                            .id("nl2sql-stop")
                            .px_2()
                            .py_1()
                            .border_1()
                            .cursor_pointer()
                            .child(SharedString::from(dat0_i18n::t("sql.ai.stop")))
                            .focus_stop("nl2sql-stop", &nl_stop_fh, 0, key)
                            .a11y(
                                "nl2sql-stop",
                                AccessRole::Button,
                                dat0_i18n::t("sql.ai.stop"),
                            )
                            .on_click(cx.listener(|_c, _ev, _w, cx| {
                                cx.emit(SqlConsoleEvent::StopAiStream);
                            })),
                    );
                } else {
                    let insert_key = cx.listener(|c, _ev: &gpui::KeyDownEvent, window, cx| {
                        if let Some(p) = c.nl_preview.take() {
                            c.load_into_new_tab(p.sql, window, cx);
                        }
                        c.pending_focus = Some(EDITOR_FOCUS);
                        cx.notify();
                    });
                    let discard_key = cx.listener(|c, _ev: &gpui::KeyDownEvent, _w, cx| {
                        c.nl_preview = None;
                        c.pending_focus = Some(EDITOR_FOCUS);
                        cx.notify();
                    });
                    strip = strip.child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .child(
                                div()
                                    .id("nl2sql-insert")
                                    .px_2()
                                    .py_1()
                                    .border_1()
                                    .cursor_pointer()
                                    .child(SharedString::from(dat0_i18n::t("sql.nl2sql.insert")))
                                    .focus_stop("nl2sql-insert", &nl_insert_fh, 0, insert_key)
                                    .a11y(
                                        "nl2sql-insert",
                                        AccessRole::Button,
                                        dat0_i18n::t("sql.nl2sql.insert"),
                                    )
                                    .on_click(cx.listener(|c, _ev, window, cx| {
                                        if let Some(p) = c.nl_preview.take() {
                                            c.load_into_new_tab(p.sql, window, cx);
                                        }
                                        c.pending_focus = Some(EDITOR_FOCUS);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                div()
                                    .id("nl2sql-discard")
                                    .px_2()
                                    .py_1()
                                    .border_1()
                                    .cursor_pointer()
                                    .child(SharedString::from(dat0_i18n::t("sql.nl2sql.discard")))
                                    .focus_stop("nl2sql-discard", &nl_discard_fh, 0, discard_key)
                                    .a11y(
                                        "nl2sql-discard",
                                        AccessRole::Button,
                                        dat0_i18n::t("sql.nl2sql.discard"),
                                    )
                                    .on_click(cx.listener(|c, _ev, _w, cx| {
                                        c.nl_preview = None;
                                        c.pending_focus = Some(EDITOR_FOCUS);
                                        cx.notify();
                                    })),
                            ),
                    );
                }
                strip
            }))
            // ── Explain side panel (P9c-2 T7) ────────────────────────────────
            .children(self.explain.as_ref().map(|e| {
                let mut panel = div().flex().flex_col().gap_1().p_2().border_t_1();
                panel =
                    panel.child(div().child(SharedString::from(dat0_i18n::t("sql.explain.title"))));
                panel = panel.child(div().child(SharedString::from(e.prose.clone())));
                if let Some(err) = &e.error {
                    panel = panel.child(div().child(SharedString::from(format!("✗ {err}"))));
                }
                if e.streaming {
                    panel = panel.child(
                        div()
                            .id("explain-stop")
                            .px_2()
                            .py_1()
                            .border_1()
                            .cursor_pointer()
                            .child(SharedString::from(dat0_i18n::t("sql.ai.stop")))
                            .on_click(cx.listener(|_console, _ev, _window, cx| {
                                cx.emit(SqlConsoleEvent::StopAiStream);
                            })),
                    );
                } else {
                    panel = panel.child(
                        div()
                            .id("explain-close")
                            .px_2()
                            .py_1()
                            .border_1()
                            .cursor_pointer()
                            .child(SharedString::from(dat0_i18n::t("sql.explain.close")))
                            .on_click(cx.listener(|_console, _ev, _window, cx| {
                                cx.emit(SqlConsoleEvent::CloseExplain);
                            })),
                    );
                }
                panel
            }))
            .children(history_overlay)
            // Consolidated Escape ladder (transient-bars nav, carve-out #7).
            // First matching rung wins; gpui bubbles the action to this one
            // ancestor handler. Rung 5 preserves the carve-out #6 editor
            // trap-exit (Escape leaves the code editor onto Run).
            .on_action(
                cx.listener(|this, _ev: &gpui_component::input::Escape, window, cx| {
                    // 1. History overlay open → close, return to editor.
                    if this.history_overlay.is_some() {
                        this.history_overlay = None;
                        this.pending_focus = Some(EDITOR_FOCUS);
                        cx.notify();
                        return;
                    }
                    // 2. NL→SQL strip → stop if streaming, else discard.
                    if let Some(streaming) = this.nl_preview.as_ref().map(|p| p.streaming) {
                        if streaming {
                            cx.emit(SqlConsoleEvent::StopAiStream);
                        } else {
                            this.nl_preview = None;
                            this.pending_focus = Some(EDITOR_FOCUS);
                            cx.notify();
                        }
                        return;
                    }
                    // 3. Explain panel → stop if streaming, else close.
                    if let Some(streaming) = this.explain.as_ref().map(|e| e.streaming) {
                        if streaming {
                            cx.emit(SqlConsoleEvent::StopAiStream);
                        } else {
                            this.pending_focus = Some(EDITOR_FOCUS);
                            cx.emit(SqlConsoleEvent::CloseExplain);
                        }
                        return;
                    }
                    // 4. Error strip → dismiss, keep current focus.
                    if matches!(this.region, ResultRegion::Error(_)) {
                        this.region = ResultRegion::Empty;
                        cx.notify();
                        return;
                    }
                    // 5. Editor focused → leave onto Run (carve-out #6 trap-exit).
                    if this.tabs[this.active]
                        .input
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)
                    {
                        let run_fh = this.toolbar_fh("sql-run", cx);
                        window.focus(&run_fh);
                        cx.notify();
                    }
                }),
            )
    }
}

#[cfg(feature = "a11y-capture")]
impl SqlConsole {
    /// The active tab index — lets a test assert an arrow switched tabs.
    pub fn active_tab_for_test(&self) -> usize {
        self.active
    }

    /// The open-tab count — lets a test assert Delete closed a tab.
    pub fn tab_count_for_test(&self) -> usize {
        self.tabs.len()
    }

    /// Whether the tab-strip tablist container currently holds focus — the
    /// title-agnostic reach oracle for the tab strip (its accessible name is the
    /// active tab's title, which is dynamic, so the test detects reach by focus).
    pub fn tabstrip_focused_for_test(&self, window: &Window) -> bool {
        self.tabstrip_focus.is_focused(window)
    }

    /// The open-tab titles in strip order — lets a test assert that Delete closed
    /// the ACTIVE tab specifically (by identity), not merely that the count dropped.
    pub fn tab_titles_for_test(&self) -> Vec<String> {
        self.tabs.iter().map(|t| t.meta.title.clone()).collect()
    }

    /// The active tab's editor `FocusHandle` — lets a test focus the editor
    /// directly (it is a native tab-stop but not part of the `focus_stop` kit).
    pub fn editor_focus_handle_for_test(&self, cx: &gpui::App) -> gpui::FocusHandle {
        self.tabs[self.active].input.read(cx).focus_handle(cx)
    }

    /// Whether the active editor holds focus (the trap-exit oracle).
    pub fn editor_focused_for_test(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        self.tabs[self.active]
            .input
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    }

    /// Inject a streaming NL→SQL preview (bypasses the real SSE flow).
    pub fn begin_nl_preview_for_test(&mut self, prompt: String, cx: &mut Context<Self>) {
        self.begin_nl_preview(prompt, cx);
    }
    /// Append a generated-SQL delta to the injected preview.
    pub fn push_nl_delta_for_test(&mut self, text: &str, cx: &mut Context<Self>) {
        self.push_nl_delta(text, cx);
    }
    /// Finish the injected preview (flips streaming → Insert/Discard).
    pub fn finish_nl_preview_for_test(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.finish_nl_preview(error, cx);
    }
    /// Whether the NL→SQL strip is currently open.
    pub fn nl_preview_open_for_test(&self) -> bool {
        self.nl_preview.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::{ExplainView, NlPreview};

    #[test]
    fn explain_view_accumulates_prose() {
        let mut e = ExplainView::new("SELECT 1".into());
        assert!(e.streaming && e.prose.is_empty());
        e.push("This query ");
        e.push("returns 1.");
        assert_eq!(e.prose, "This query returns 1.");
        e.finish(None);
        assert!(!e.streaming);
    }

    #[test]
    fn nl_preview_accumulates_then_finishes() {
        let mut p = NlPreview::new("top users".into());
        assert!(p.streaming && p.sql.is_empty());
        p.push("SEL");
        p.push("ECT 1");
        assert_eq!(p.sql, "SELECT 1");
        p.finish(None);
        assert!(!p.streaming && p.error.is_none());
    }

    #[test]
    fn nl_preview_finish_with_error_keeps_partial() {
        let mut p = NlPreview::new("q".into());
        p.push("SELECT");
        p.finish(Some("429 rate limited".into()));
        assert!(!p.streaming);
        assert_eq!(p.sql, "SELECT"); // partial retained for Insert/Discard
        assert_eq!(p.error.as_deref(), Some("429 rate limited"));
    }
}
