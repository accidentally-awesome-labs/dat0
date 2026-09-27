//! Opening SQLite files (PD-023, step 5.4e).
//!
//! A SQLite file dropped, opened from File → Open, passed on the command line
//! or chosen as the Chinook sample was refused as a file type dat0 did not
//! know. It is now attached to the window's session, read-only
//! (`connections::sqlite`): its first table opens in a tab, and all of its
//! tables are listed under CONNECTIONS, where a click opens another. The
//! session records the file, and a session that lands again attaches it again
//! before its tabs come back ([`reattach`]).

use std::path::PathBuf;
use std::sync::Arc;

use dioxus::prelude::*;
use parking_lot::Mutex;

use dat0_core::connections::sqlite;
use dat0_core::error_ux::Banner;
use dat0_core::session::{PersistedAttachment, PersistedAttachmentKind, Session};
use dat0_engine::QueryEngine as _;
use dat0_i18n::t;

use crate::state::{Attached, TabView, Workspace};

/// Attach the SQLite file at `path` to the window's session and open its
/// first table. A file the session has already keeps its alias.
pub async fn open(ws: Workspace, session: Arc<Mutex<Session>>, path: PathBuf) {
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let (engine, alias, known) = {
        let s = session.lock();
        let taken: Vec<String> = s.attachments().iter().map(|a| a.alias.clone()).collect();
        match sqlite::recorded(s.attachments(), &path) {
            Some(alias) => (s.engine.clone(), alias.to_string(), true),
            None => (s.engine.clone(), sqlite::alias_for(&path, &taken), false),
        }
    };
    let tables = match sqlite::attach(engine.as_ref(), &path, &alias).await {
        Ok(tables) => tables,
        Err(e) => {
            ws.push_banner(Banner::error(
                t("sqlite.attach.failed.title"),
                format!("{e:#}"),
            ));
            return;
        }
    };
    if !known {
        let mut s = session.lock();
        let mut all = s.attachments().to_vec();
        all.push(PersistedAttachment {
            alias: alias.clone(),
            kind: PersistedAttachmentKind::Sqlite {
                path: path.display().to_string(),
            },
        });
        if let Err(e) = s.set_attachments(all) {
            tracing::warn!(error = %format!("{e:#}"), "could not record the attached file");
        }
    }
    show(ws, &alias, &path, tables.clone());
    match tables.first() {
        Some(first) => open_table(ws, session, alias, first.clone()).await,
        None => ws.push_banner(Banner::warning_with_body(
            t("sqlite.empty"),
            path.display().to_string(),
        )),
    }
}

/// Open `table` of the database attached as `alias` in a tab, or bring its
/// tab forward.
pub async fn open_table(ws: Workspace, session: Arc<Mutex<Session>>, alias: String, table: String) {
    let mut ws = ws;
    let engine = session.lock().engine.clone();
    match sqlite::open_table(engine.as_ref(), &alias, &table).await {
        Ok(name) => {
            let at = ws.tabs.peek().iter().position(|t| t.table == name);
            let at = at.unwrap_or_else(|| {
                // No file: an attached table is not re-read from one, and
                // Live Refresh must not try.
                ws.tabs.write().push(TabView {
                    table: name,
                    path: None,
                    label: None,
                });
                ws.tabs.peek().len() - 1
            });
            ws.active.set(Some(at));
        }
        Err(e) => ws.push_banner(Banner::error(
            t("sqlite.attach.failed.title"),
            format!("{e:#}"),
        )),
    }
}

/// Attach the session's SQLite files again, and list them under
/// CONNECTIONS. An attachment lasts as long as the engine's connection, so a
/// session that lands (opened again, or moved by Save Workspace onto an engine
/// of its own) has none until this runs. A file that is gone is reported and
/// left out; its tabs cannot be read until it is back.
pub async fn reattach(ws: Workspace, session: &Arc<Mutex<Session>>) {
    let mut ws = ws;
    let (engine, attachments) = {
        let s = session.lock();
        (s.engine.clone(), s.attachments().to_vec())
    };
    let failed = sqlite::reattach(engine.as_ref(), &attachments).await;
    for (path, e) in &failed {
        ws.push_banner(Banner::warning_with_body(
            t("sqlite.reattach.failed.title"),
            format!("{}: {e:#}", path.display()),
        ));
    }
    let mut shown = Vec::new();
    for a in &attachments {
        let PersistedAttachmentKind::Sqlite { path } = &a.kind else {
            continue;
        };
        let path = PathBuf::from(path);
        if failed.iter().any(|(p, _)| *p == path) {
            continue;
        }
        let tables = engine.attached_tables(&a.alias).await.unwrap_or_default();
        shown.push(Attached {
            alias: a.alias.clone(),
            path,
            tables,
        });
    }
    ws.attached.set(shown);
}

/// List `alias` under CONNECTIONS, once.
fn show(ws: Workspace, alias: &str, path: &std::path::Path, tables: Vec<String>) {
    let mut attached = ws.attached;
    let mut list = attached.write();
    list.retain(|a| a.alias != alias);
    list.push(Attached {
        alias: alias.to_string(),
        path: path.to_path_buf(),
        tables,
    });
}
