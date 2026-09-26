//! Live Refresh: read a tab's file again, and keep its view.
//!
//! A tab opened from a file holds a copy of it. When the file changes on disk
//! a banner says so, and Refresh re-imports it under the same table and replays
//! the view's steps that can survive onto the new rows: sorts, filters, column
//! moves, hides and renames. Edits and deleted rows are keyed by row ids the
//! re-import makes anew, so they cannot; when the view has any, the dialog says
//! how many would go and asks first. Before this, the dialog opened with zeros
//! and its answer was thrown away, and nothing watched the file (PD-023).

use std::path::PathBuf;
use std::time::Duration;

use dioxus::prelude::*;

use dat0_core::error_ux::Banner;
use dat0_core::events::{AppEvent, AppEvents};
use dat0_core::workspace::source_watcher::SourceWatcher;
use dat0_engine::transform::{Transformation, split_replayable};
use dat0_engine::{QueryEngine, RegisterOpts};
use dat0_i18n::t;

use super::views::{Views, engine};
use crate::components::modals::{ModalOutcome, ModalReply};
use crate::state::{Modal, Workspace};

/// Refresh the active tab from its file.
pub fn refresh(ws: Workspace, views: Views) {
    let Some(tab) = ws.active_tab() else {
        return;
    };
    let Some(path) = tab.path.clone() else {
        // A table made by SQL, or a query's rows: there is no file to read.
        ws.push_banner(Banner::warning(t("livedata.no_file")));
        return;
    };
    let (stack, cursor) = views.stack(&tab.table);
    let split = split_replayable(&stack[..cursor.min(stack.len())]);
    if !split.has_dropped() {
        return reimport(ws, views, tab.table, path, split.replayable);
    }
    let (table, keep) = (tab.table, split.replayable);
    let mut modal = ws.modal;
    modal.set(Some(Modal::LiveRefresh {
        dropped_edits: split.dropped_edits,
        dropped_deletes: split.dropped_deletes,
        reply: ModalReply::new(move |outcome| {
            if matches!(outcome, ModalOutcome::Confirmed) {
                reimport(ws, views, table.clone(), path.clone(), keep.clone());
            }
        }),
    }));
}

/// Read `path` into `table` again, then rebuild the view from `keep`.
fn reimport(ws: Workspace, views: Views, table: String, path: PathBuf, keep: Vec<Transformation>) {
    let Some(engine) = engine(&ws) else {
        return;
    };
    spawn(async move {
        match engine
            .register_file_as_table(&path, RegisterOpts::default())
            .await
        {
            Ok(info) if info.name == table => {
                let columns: Vec<String> = info.columns.iter().map(|c| c.name.clone()).collect();
                let (ops, drifted) = replayable_on(keep, &columns);
                if drifted {
                    ws.push_banner(Banner::warning(t("livedata.replay.schema_drift")));
                }
                views.replay(table, ops);
                clear_changed_banner(ws, &path);
            }
            // The file's table has another name now: another table took this
            // one's since it was opened. Nothing to replay onto.
            Ok(info) => ws.push_banner(Banner::warning_with_body(
                t("livedata.reimport.failed.title"),
                info.name,
            )),
            Err(e) => ws.push_banner(Banner::warning_with_body(
                t("livedata.reimport.failed.title"),
                e.to_string(),
            )),
        }
    });
}

/// `ops`, when every filter and sort among them names a column the file still
/// has; otherwise none, and `true`. All or nothing: a replay that quietly
/// dropped one filter would show rows the user had filtered out.
fn replayable_on(ops: Vec<Transformation>, columns: &[String]) -> (Vec<Transformation>, bool) {
    let has = |c: &str| columns.iter().any(|known| known == c);
    let drifted = ops.iter().any(|op| match op {
        Transformation::Filter { column, .. } => !has(column),
        Transformation::Sort { keys } => keys.iter().any(|k| !has(&k.column)),
        // Display-only: an unknown column in them is ignored when the header
        // folds them, and never reaches the SQL.
        _ => false,
    });
    if drifted {
        (Vec::new(), true)
    } else {
        (ops, false)
    }
}

fn changed_title(path: &std::path::Path) -> String {
    let file = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    t("livedata.changed.title").replace("{file}", &file)
}

fn clear_changed_banner(ws: Workspace, path: &std::path::Path) {
    let title = changed_title(path);
    let mut banners = ws.banners;
    banners.write().retain(|b| b.title != title);
}

/// Watch the active tab's file while it is active, and say when it changes.
/// A hook: call it once, from the shell's body.
pub fn use_source_watch(ws: Workspace, events: AppEvents) {
    let mut watching = use_signal(|| Option::<(PathBuf, SourceWatcher)>::None);
    use_effect(move || {
        let path = ws.active_tab().and_then(|t| t.path);
        if watching.peek().as_ref().map(|(p, _)| p) == path.as_ref() {
            return;
        }
        let Some(path) = path.filter(|p| p.exists()) else {
            watching.set(None);
            return;
        };
        let events = events.clone();
        let title = changed_title(&path);
        let started = SourceWatcher::start(path.clone(), Duration::from_millis(500), move |_| {
            let banner = Banner::warning(title.clone()).with_primary(
                t("livedata.changed.refresh"),
                dat0_core::actions::builtin::ids::LIVE_REFRESH,
            );
            events.send(AppEvent::Banner(banner));
        });
        match started {
            Ok(w) => watching.set(Some((path, w))),
            Err(e) => {
                tracing::warn!(error = %e, "could not watch the tab's file");
                watching.set(None);
            }
        }
    });
}
