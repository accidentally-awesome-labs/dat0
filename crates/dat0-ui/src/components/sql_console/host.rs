//! What a console intent does.
//!
//! The console renders and reports ([`ConsoleIntent`]); this module performs.
//! It owns the run — the statement under the caret, the engine round trip, the
//! cancel guard — and the query library around it: history, saved queries and
//! Save as Table. The shell holds the state, one [`ConsoleHost`], and calls in
//! here from the console's `on_intent` and from the router, so a run reached
//! from Cmd-Enter, the Run chip, the palette or the menu bar is one
//! implementation.
//!
//! A run that returns rows lands in the main grid, as a tab named for the
//! query tab it came from. The rows are a temporary view, one per query tab
//! ([`result_view_name`]), so running again replaces them in place. The run
//! builds the grid's data source itself — counting the rows is where a slow
//! query spends its time — so Cancel reaches it, and the grid takes the
//! finished source instead of counting a second time.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dioxus::prelude::*;

use dat0_core::error_ux::Banner;
use dat0_core::grid::data_source::GridDataSource;
use dat0_core::query::completion::{SharedSnapshot, TableEntry};
use dat0_core::query::statement::{ResultKind, classify};
use dat0_core::query::{QueryCancel, result_view_name};
use dat0_core::session::queries::{self, HistoryEntry, SavedQuery};
use dat0_engine::{
    DerivedOrigin, DuckDBEngine, EngineError, QueryEngine as _, QueryLane, quote_ident,
};

use super::ConsoleIntent;
use super::sql_text::{interrupted, message, statement, view_body};
use super::tabs::Tabs;
use crate::components::grid::views::Views;
use crate::components::modals::{ModalOutcome, ModalReply};
use crate::components::query_library::first_line;
use crate::state::{Modal, TabView, Workspace};

/// Where each query tab's caret was last reported: tab id to (line, column),
/// 1-based, as CodeMirror counts them.
pub type Carets = Signal<HashMap<String, (usize, usize)>>;

/// The run in flight.
pub struct Run {
    started: std::time::Instant,
    cancel: QueryCancel,
    /// Set by Cancel, and checked between the run's two engine steps: DuckDB's
    /// interrupt only reaches a query that is executing, so a Cancel landing
    /// between creating the view and counting it would otherwise be lost.
    stop: Arc<AtomicBool>,
}

/// The console's state, as the shell holds it.
#[derive(Clone)]
pub struct ConsoleHost {
    pub ws: Workspace,
    pub tabs: Signal<Tabs>,
    /// The failed-run strip.
    pub error: Signal<Option<String>>,
    run: Signal<Option<Run>>,
    /// How long the last run took, ms, for the status bar's query chip.
    last_ms: Signal<Option<u64>>,
    pub carets: Carets,
    /// The grid's views, which a run's rows are handed to.
    views: Views,
    /// What the editor completes against.
    pub schema: SharedSnapshot,
}

impl ConsoleHost {
    /// What the status bar's query chip says: running while a run is out,
    /// then how long the last one took.
    pub fn chip(&self) -> crate::components::status_bar::QueryChip {
        use crate::components::status_bar::QueryChip;
        if self.run.read().is_some() {
            QueryChip::Running
        } else {
            (self.last_ms)().map_or(QueryChip::Idle, |ms| QueryChip::Done { ms })
        }
    }

    /// This window's console state. A hook: call it once, from the shell's
    /// body.
    ///
    /// The completion schema follows the window's tables — refreshed when the
    /// session lands, when a tab opens or closes, and after every run.
    pub fn use_new(ws: Workspace, views: Views) -> Self {
        let host = Self {
            ws,
            tabs: use_signal(Tabs::new),
            error: use_signal(|| None),
            run: use_signal(|| None),
            last_ms: use_signal(|| None),
            carets: use_signal(HashMap::new),
            views,
            schema: use_hook(dat0_core::query::completion::new_shared_snapshot),
        };
        let refresh = host.clone();
        use_effect(move || {
            let _ = ws.tabs.read().len();
            if ws.session.read().ready().is_some() {
                refresh_schema(&refresh);
            }
        });
        host
    }

    /// Whether a statement is in flight.
    pub fn running(&self) -> bool {
        self.run.read().is_some()
    }
}

/// Perform one console intent.
pub fn perform(host: &ConsoleHost, intent: ConsoleIntent) {
    let mut tabs = host.tabs;
    let mut error = host.error;
    match intent {
        ConsoleIntent::NewTab => {
            tabs.write().open();
        }
        ConsoleIntent::CloseTab => {
            // Refused on the last tab: a console with no tab has nowhere to
            // type, and the widget would have to invent one back.
            tabs.write().close_active();
        }
        ConsoleIntent::DocChanged { tab, doc } => tabs.write().set_doc(&tab, doc),
        // There is no results pane yet, so every run lands in the main grid.
        ConsoleIntent::Run { tab, sql, .. } => run(host, &tab, sql),
        ConsoleIntent::Cancel { .. } => cancel(host),
        ConsoleIntent::ShowHistory => show_history(host),
        ConsoleIntent::LoadQuery => show_saved(host),
        ConsoleIntent::SaveQuery { sql, .. } => prompt_save_query(host, sql),
        ConsoleIntent::SaveAsTable { tab, .. } => prompt_save_as_table(host, tab),
        ConsoleIntent::DismissError => error.set(None),
        other => tracing::debug!(?other, "console intent not routed yet"),
    }
}

/// The active query tab's id and document.
pub fn active(host: &ConsoleHost) -> (String, String) {
    let tabs = host.tabs.peek();
    let tab = tabs.active_tab();
    (tab.id.clone(), tab.doc.clone())
}

/// Say why nothing ran, where the user is looking for the answer.
fn refuse(host: &ConsoleHost, key: &str) {
    let (mut error, mut ws) = (host.error, host.ws);
    error.set(Some(dat0_i18n::t(key)));
    // A run started from the palette or the menu bar with the console shut
    // must still say why nothing happened.
    ws.layout.write().console_open = true;
}

/// Run the statement under `tab`'s caret.
pub fn run(host: &ConsoleHost, tab: &str, doc: String) {
    if host.run.peek().is_some() {
        // One at a time. The Run chip is Cancel while a run is in flight, and
        // a second Cmd-Enter must not orphan the first run's cancel guard.
        return;
    }
    let caret = host.carets.peek().get(tab).copied();
    let Some(stmt) = statement(&doc, caret) else {
        return refuse(host, "sql.error.empty");
    };
    let Some(engine) = engine(&host.ws) else {
        return refuse(host, "sql.error.no_session");
    };
    let kind = classify(&stmt);
    if kind == ResultKind::Exec && *host.ws.read_only.peek() {
        return refuse(host, "sql.error.read_only");
    }

    let (index, title) = {
        let tabs = host.tabs.peek();
        match tabs.all().iter().position(|t| t.id == tab) {
            Some(i) => (i, tabs.all()[i].title.clone()),
            None => (tabs.active(), tabs.active_tab().title.clone()),
        }
    };
    let view = result_view_name(&window_key(&host.ws), index);
    let body = match kind {
        ResultKind::Result => view_body(&stmt),
        ResultKind::Exec => None,
    };
    // What DuckDB quotes back in an error, ahead of the user's own text.
    let wrapper = format!(
        "{} AS {}",
        quote_ident(&view),
        body.as_ref().map_or("", |(_, before)| *before)
    );
    let token = engine.begin_query(QueryLane::Console);
    let stop = Arc::new(AtomicBool::new(false));
    let (mut run, mut error) = (host.run, host.error);
    run.set(Some(Run {
        started: std::time::Instant::now(),
        cancel: QueryCancel::new(&engine, token),
        stop: Arc::clone(&stop),
    }));
    error.set(None);

    let host = host.clone();
    spawn(async move {
        let outcome = match &body {
            Some((body, _)) => rows(&engine, &view, body, &stop).await.map(Ran::Rows),
            None => engine
                .execute(&stmt)
                .await
                .map(|r| Ran::Done {
                    hidden: r.batches.iter().map(|b| b.num_rows()).sum(),
                })
                .map_err(anyhow::Error::from),
        };
        finish(&host, doc, &stmt, outcome, view, title, &wrapper);
    });
}

/// Interrupt the run in flight, if there is one. Its task still finishes,
/// through [`finish`], which reports it cancelled.
pub fn cancel(host: &ConsoleHost) {
    let mut run = host.run;
    if let Some(r) = run.write().as_mut() {
        r.stop.store(true, Ordering::Relaxed);
        r.cancel.cancel();
    }
}

/// What a finished run produced.
enum Ran {
    /// Rows, for the grid.
    Rows(Arc<GridDataSource>),
    /// Nothing for the grid. `hidden` counts rows the statement returned
    /// anyway, from a PRAGMA or an EXPLAIN (see [`view_body`]).
    Done { hidden: usize },
}

/// A result-producing statement: bind it to this query tab's view and count
/// it, which is the part that runs the query.
async fn rows(
    engine: &Arc<DuckDBEngine>,
    view: &str,
    body: &str,
    stop: &AtomicBool,
) -> anyhow::Result<Arc<GridDataSource>> {
    engine.create_or_replace_view(view, body).await?;
    if stop.load(Ordering::Relaxed) {
        return Err(EngineError::Interrupted.into());
    }
    let source = GridDataSource::new(Arc::clone(engine), view.to_string()).await?;
    Ok(Arc::new(source))
}

fn finish(
    host: &ConsoleHost,
    doc: String,
    stmt: &str,
    outcome: anyhow::Result<Ran>,
    view: String,
    title: String,
    wrapper: &str,
) {
    let (mut run, mut error, mut ws) = (host.run, host.error, host.ws);
    let elapsed_ms = match run.write().take() {
        Some(mut r) => {
            r.cancel.disarm();
            u64::try_from(r.started.elapsed().as_millis()).unwrap_or(u64::MAX)
        }
        None => 0,
    };
    let mut last_ms = host.last_ms;
    last_ms.set(Some(elapsed_ms));
    // The whole buffer, as GPUI recorded it: loading from history reopens
    // what was in front of you, not the one statement that ran.
    record_history(host, doc, outcome.is_ok(), elapsed_ms);

    match outcome {
        Ok(Ran::Rows(source)) => show_rows(host, view, title, source),
        Ok(Ran::Done { hidden }) => {
            let mut done = Banner::info(dat0_i18n::t("sql.exec.done"));
            done.body = first_line(stmt, 80);
            if hidden > 0 {
                done.body.push('\n');
                done.body.push_str(&dat0_i18n::t("sql.exec.rows_hidden"));
            }
            ws.push_banner(done);
        }
        Err(e) => {
            error.set(Some(if interrupted(&e) {
                dat0_i18n::t("sql.cancelled")
            } else {
                message(&e, wrapper)
            }));
            // A run started from the palette or the menu bar may have the
            // console shut, and the strip is where its failure is shown.
            ws.layout.write().console_open = true;
        }
    }
    refresh_schema(host);
}

/// Show a run's rows: the query tab's result tab, made active.
fn show_rows(host: &ConsoleHost, view: String, title: String, source: Arc<GridDataSource>) {
    let mut ws = host.ws;
    host.views.replaced(view.clone(), source);
    let existing = ws.tabs.peek().iter().position(|t| t.table == view);
    let at = match existing {
        Some(i) => {
            ws.tabs.write()[i].label = Some(title);
            i
        }
        None => {
            ws.tabs.write().push(TabView {
                table: view,
                path: None,
                label: Some(title),
            });
            ws.tabs.peek().len() - 1
        }
    };
    ws.active.set(Some(at));
}

fn record_history(host: &ConsoleHost, sql: String, ok: bool, elapsed_ms: u64) {
    let Some(slot) = host.ws.session.peek().ready().cloned() else {
        return;
    };
    let mut session = slot.lock();
    let mut history = session.query_history().to_vec();
    queries::push_history(
        &mut history,
        HistoryEntry {
            sql,
            ran_at: now_ms(),
            ok,
            elapsed_ms,
        },
    );
    if let Err(e) = session.set_query_history(history) {
        tracing::warn!("query history not saved: {e:#}");
    }
}

fn show_history(host: &ConsoleHost) {
    let entries = match host.ws.session.peek().ready() {
        Some(slot) => slot.lock().query_history().to_vec(),
        None => Vec::new(),
    };
    let tabs = host.tabs;
    let mut ws = host.ws;
    ws.modal.set(Some(Modal::QueryLibrary {
        entries,
        reply: ModalReply::new(move |outcome| {
            if let ModalOutcome::SqlPicked(sql) = outcome {
                // A new tab: a picked statement never overwrites what is in
                // front of you.
                let mut tabs = tabs;
                tabs.write().open_with(sql);
            }
        }),
    }));
}

fn show_saved(host: &ConsoleHost) {
    let queries = match host.ws.session.peek().ready() {
        Some(slot) => slot.lock().saved_queries().to_vec(),
        None => Vec::new(),
    };
    let reply = {
        let host = host.clone();
        ModalReply::new(move |outcome| match outcome {
            ModalOutcome::QueryPicked(q) => {
                let mut tabs = host.tabs;
                tabs.write().open_with(q.sql);
            }
            ModalOutcome::QueryDeleted(id) => {
                if let Some(slot) = host.ws.session.peek().ready() {
                    let mut session = slot.lock();
                    let mut list = session.saved_queries().to_vec();
                    queries::delete_saved(&mut list, id);
                    if let Err(e) = session.set_saved_queries(list) {
                        tracing::warn!("saved queries not written: {e:#}");
                    }
                }
                // The picker stays up after a delete: show it the new list.
                show_saved(&host);
            }
            _ => {}
        })
    };
    let mut ws = host.ws;
    ws.modal.set(Some(Modal::SavedQueries { queries, reply }));
}

fn prompt_save_query(host: &ConsoleHost, sql: String) {
    if sql.trim().is_empty() {
        return refuse(host, "sql.error.empty");
    }
    let asker = host.clone();
    let mut ws = host.ws;
    ws.modal.set(Some(name_prompt(
        "sql.save_query",
        ModalReply::new(move |outcome| {
            let ModalOutcome::Named(name) = outcome else {
                return;
            };
            let Some(slot) = asker.ws.session.peek().ready().cloned() else {
                return refuse(&asker, "sql.error.no_session");
            };
            let name = name.trim().to_string();
            let mut session = slot.lock();
            let mut list = session.saved_queries().to_vec();
            queries::upsert_saved(
                &mut list,
                SavedQuery {
                    id: uuid::Uuid::now_v7(),
                    name: name.clone(),
                    sql: sql.clone(),
                    saved_at: now_ms(),
                },
            );
            match session.set_saved_queries(list) {
                Ok(()) => {
                    let mut saved = Banner::info(dat0_i18n::t("sql.query_saved"));
                    saved.body = name;
                    ws.push_banner(saved);
                    crate::workspace_save::suggest(ws, &session);
                }
                Err(e) => ws.push_banner(Banner::warning_with_body(
                    dat0_i18n::t("sql.query_not_saved"),
                    format!("{e:#}"),
                )),
            }
        }),
    )));
}

/// Ask for a name, then keep the statement under the caret as a table.
///
/// The statement is resolved when the name is confirmed, not when the prompt
/// opens, so what is saved is what the caret is on after the prompt closes.
fn prompt_save_as_table(host: &ConsoleHost, tab: String) {
    let reply = {
        let host = host.clone();
        ModalReply::new(move |outcome| {
            if let ModalOutcome::Named(name) = outcome {
                save_as_table(&host, &tab, name.trim().to_string());
            }
        })
    };
    let mut ws = host.ws;
    ws.modal.set(Some(name_prompt("sql.save_as_table", reply)));
}

fn save_as_table(host: &ConsoleHost, tab: &str, name: String) {
    let doc = host
        .tabs
        .peek()
        .all()
        .iter()
        .find(|t| t.id == tab)
        .map(|t| t.doc.clone())
        .unwrap_or_default();
    let caret = host.carets.peek().get(tab).copied();
    let Some(stmt) = statement(&doc, caret) else {
        return refuse(host, "sql.error.empty");
    };
    let Some(engine) = engine(&host.ws) else {
        return refuse(host, "sql.error.no_session");
    };
    let host = host.clone();
    spawn(async move {
        let select = format!("SELECT * FROM ({stmt})");
        let made = engine
            .create_table(&name, &select, DerivedOrigin::Sql(stmt))
            .await;
        let (mut error, mut ws) = (host.error, host.ws);
        match made {
            Ok(info) => {
                // Nothing lists the engine's tables yet, so the new table
                // opens as a tab: otherwise it would exist and be nowhere.
                ws.tabs.write().push(TabView {
                    table: info.name.clone(),
                    path: None,
                    label: None,
                });
                let last = ws.tabs.peek().len() - 1;
                ws.active.set(Some(last));
                let mut saved = Banner::info(dat0_i18n::t("sql.table_saved"));
                saved.body = info.name;
                ws.push_banner(saved);
                refresh_schema(&host);
            }
            Err(e) => {
                error.set(Some(e.to_string()));
                ws.layout.write().console_open = true;
            }
        }
    });
}

fn name_prompt(title_key: &str, reply: ModalReply) -> Modal {
    Modal::NamePrompt {
        title: dat0_i18n::t(title_key),
        initial: String::new(),
        placeholder: None,
        confirm_label: Some(dat0_i18n::t("prompt.save")),
        secret: false,
        reply,
    }
}

/// Fill the completion schema from the engine's tables.
///
/// The editor reads it when a query tab initialises, so a table created by a
/// run is offered from the next tab switch.
pub fn refresh_schema(host: &ConsoleHost) {
    let Some(engine) = engine(&host.ws) else {
        return;
    };
    let schema = host.schema.clone();
    spawn(async move {
        match engine.get_tables().await {
            Ok(tables) => {
                schema.lock().tables = tables
                    .into_iter()
                    .map(|t| TableEntry {
                        name: t.name,
                        columns: t.columns.into_iter().map(|c| c.name).collect(),
                    })
                    .collect();
            }
            Err(e) => tracing::warn!("completion schema not refreshed: {e}"),
        }
    });
}

fn engine(ws: &Workspace) -> Option<Arc<DuckDBEngine>> {
    ws.session
        .peek()
        .ready()
        .map(|slot| Arc::clone(&slot.lock().engine))
}

/// A short, stable name for this window, for its result views. Each window
/// has an engine of its own, so this only has to be a valid identifier.
fn window_key(ws: &Workspace) -> String {
    let id = ws.window_id.simple().to_string();
    id[id.len() - 8..].to_string()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
