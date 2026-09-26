//! A window's tabs, their views and its SQL, kept in its session.
//!
//! The session file is what a window leaves behind: what the recovery panel
//! lists and restores, and what a package or a workspace will carry. The
//! Dioxus shell recorded a file's tab when it opened and nothing after that —
//! not a sort, not an edit, not a line of SQL — so a crashed window could come
//! back as its bare tables at best, and its SQL not at all (PD-023).
//!
//! [`use_session_sync`] reads a landed session into the window once, then
//! records every change back to it.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use dioxus::prelude::*;
use parking_lot::Mutex;

use dat0_core::events::Opening;
use dat0_core::session::{Session, SqlTabState, Tab};
use dat0_engine::{QueryEngine as _, TableOrigin, Transformation};

use crate::components::grid::views::Views;
use crate::components::sql_console::tabs::Tabs;
use crate::state::{TabView, Workspace};

/// How long a change waits before it is written. Short: a crash inside it
/// loses the change, and a gesture is one change, not a stream of them. A run
/// of keystrokes in the console is the stream it coalesces.
const RECORD_DEBOUNCE: Duration = Duration::from_millis(250);

/// What the window records. Compared before writing, so a render that
/// changed nothing writes nothing.
#[derive(Clone, PartialEq)]
struct Recorded {
    /// Each grid tab: its table, its file, its view's steps.
    tabs: Vec<(String, Option<PathBuf>, Vec<Transformation>)>,
    active: Option<usize>,
    /// Each query tab: its id, title and SQL.
    sql: Vec<(String, String, String)>,
    sql_active: usize,
}

/// Restore the landed session into this window, then keep it up to date. A
/// hook: call it once, from the shell's body, after `views` and the console.
pub fn use_session_sync(ws: Workspace, views: Views, console: Signal<Tabs>) {
    // Only a session opened again — recovered, or a workspace — has anything
    // to bring back. A new one's tabs are the files this window opens into
    // it, which arrive as tabs of their own; restoring them too would show
    // each twice.
    let reopened = use_hook(|| {
        matches!(
            try_consume_context::<Opening>(),
            Some(Opening::Recover { .. } | Opening::Workspace { .. } | Opening::Inspect { .. })
        )
    });
    // Recording waits until the session has been read in: the first record
    // would otherwise write this window's empty tab list over the tabs it was
    // opened to show.
    let mut restored = use_signal(move || !reopened);
    let started = use_hook(|| Rc::new(Cell::new(false)));
    use_effect(move || {
        let Some(session) = ws.session.read().ready().cloned() else {
            return;
        };
        if !reopened || started.replace(true) {
            return;
        }
        spawn(async move {
            restore(ws, views, console, &session).await;
            restored.set(true);
        });
    });

    // What the last render saw, so one that changed nothing does not restart
    // the wait, and what the session holds, so a write carries every change
    // since the last one — not just the change that ended the wait.
    let generation = use_hook(|| Rc::new(Cell::new(0u64)));
    let seen = use_hook(|| Rc::new(RefCell::new(None::<Recorded>)));
    let written = use_hook(|| Rc::new(RefCell::new(None::<Recorded>)));
    use_effect(move || {
        if !*restored.read() {
            return;
        }
        let Some(session) = ws.session.read().ready().cloned() else {
            return;
        };
        let now = snapshot(ws, views, console);
        if seen.replace(Some(now.clone())).as_ref() == Some(&now) {
            return;
        }
        let epoch = generation.get() + 1;
        generation.set(epoch);
        let (generation, written) = (generation.clone(), written.clone());
        spawn(async move {
            tokio::time::sleep(RECORD_DEBOUNCE).await;
            if generation.get() == epoch {
                write(&session, written.borrow().as_ref(), &now);
                crate::workspace_save::suggest(ws, &session.lock());
                *written.borrow_mut() = Some(now);
            }
        });
    });
}

/// What the window holds now. Reads every signal it records, so the effect
/// calling it runs again when any of them changes.
fn snapshot(ws: Workspace, views: Views, console: Signal<Tabs>) -> Recorded {
    let tabs = ws.tabs.read();
    let active = *ws.active.read();
    // A console run's rows sit in a temporary view that does not outlive the
    // window, so its tab is not kept either.
    let kept: Vec<(usize, &TabView)> = tabs
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.table.starts_with("__dat0"))
        .collect();
    let console = console.read();
    Recorded {
        tabs: kept
            .iter()
            .map(|(_, t)| (t.table.clone(), t.path.clone(), views.stack(&t.table).0))
            .collect(),
        active: active.and_then(|a| kept.iter().position(|(i, _)| *i == a)),
        sql: console
            .all()
            .iter()
            .map(|t| (t.id.clone(), t.title.clone(), t.doc.clone()))
            .collect(),
        sql_active: console.active(),
    }
}

/// Write what changed since `before`, the state last written; all of it when
/// nothing has been.
fn write(session: &Arc<Mutex<Session>>, before: Option<&Recorded>, now: &Recorded) {
    let mut s = session.lock();
    if before.is_none_or(|b| (&b.tabs, b.active) != (&now.tabs, now.active)) {
        let tabs = now
            .tabs
            .iter()
            .cloned()
            .map(|(table, path, stack)| {
                // What a newer dat0 stored on the tab, kept.
                let extra = s
                    .tabs()
                    .iter()
                    .find(|t| t.table_name == table)
                    .map(|t| t.extra.clone())
                    .unwrap_or_default();
                Tab {
                    table_name: table,
                    source_path: path,
                    undo_cursor: stack.len(),
                    transform_stack: stack,
                    extra,
                }
            })
            .collect();
        if let Err(e) = s.set_tabs(tabs, now.active) {
            tracing::warn!(error = %format!("{e:#}"), "recording the tabs failed");
        }
    }
    if before.is_none_or(|b| (&b.sql, b.sql_active) != (&now.sql, now.sql_active)) {
        let sql = now
            .sql
            .iter()
            .cloned()
            .map(|(id, title, doc)| SqlTabState {
                id: id
                    .strip_prefix("console-")
                    .and_then(|u| uuid::Uuid::parse_str(u).ok())
                    .unwrap_or_else(uuid::Uuid::now_v7),
                title,
                sql: doc,
            })
            .collect();
        if let Err(e) = s.set_sql_tabs(sql, Some(now.sql_active)) {
            tracing::warn!(error = %format!("{e:#}"), "recording the console failed");
        }
    }
}

/// Bring what `session` holds into the window: its tabs ahead of any opened
/// while it booted, each tab's view, and the console's query tabs.
async fn restore(
    ws: Workspace,
    views: Views,
    console: Signal<Tabs>,
    session: &Arc<Mutex<Session>>,
) {
    let (tabs, active, sql, sql_active, engine) = {
        let s = session.lock();
        (
            s.tabs().to_vec(),
            s.active_tab_index(),
            s.sql_tabs().to_vec(),
            s.active_sql_tab(),
            Arc::clone(&s.engine),
        )
    };

    if !sql.is_empty() {
        let mut console = console;
        console.set(Tabs::adopt(
            sql.into_iter()
                .map(|q| crate::components::sql_console::Tab {
                    id: format!("console-{}", q.id),
                    title: q.title,
                    doc: q.sql,
                })
                .collect(),
            sql_active.unwrap_or(0),
        ));
    }
    if tabs.is_empty() {
        return;
    }

    // A tab whose table the database no longer holds cannot be shown.
    let held: std::collections::HashSet<String> = match engine.get_tables().await {
        Ok(t) => t.into_iter().map(|t| t.name).collect(),
        Err(e) => {
            tracing::warn!(error = %e, "restore: could not list the tables");
            return;
        }
    };
    let mut shown = Vec::new();
    let mut now_active = None;
    for (i, tab) in tabs.into_iter().enumerate() {
        if !held.contains(&tab.table_name) {
            tracing::warn!(table = %tab.table_name, "restore: the table is gone");
            continue;
        }
        if active == Some(i) {
            now_active = Some(shown.len());
        }
        // Where it came from lives in memory only; without it, reading the
        // file again would import beside the table rather than into it.
        if let Some(path) = &tab.source_path {
            engine.restore_origin(&tab.table_name, TableOrigin::File(path.clone()));
        }
        let steps = tab.undo_cursor.min(tab.transform_stack.len());
        if steps > 0 {
            views.replay(
                tab.table_name.clone(),
                tab.transform_stack[..steps].to_vec(),
            );
        }
        shown.push(TabView {
            table: tab.table_name,
            path: tab.source_path,
            label: None,
        });
    }
    if shown.is_empty() {
        return;
    }
    let (mut tabs_sig, mut active_sig) = (ws.tabs, ws.active);
    let opened_meanwhile = tabs_sig.peek().len();
    let at = now_active.unwrap_or(0);
    tabs_sig.with_mut(|t| {
        t.splice(0..0, shown);
    });
    let was = *active_sig.peek();
    match was {
        // A file dropped while the session opened stays the tab showing.
        Some(a) if opened_meanwhile > 0 => {
            let restored = tabs_sig.peek().len() - opened_meanwhile;
            active_sig.set(Some(a + restored));
        }
        _ => active_sig.set(Some(at)),
    }
}
