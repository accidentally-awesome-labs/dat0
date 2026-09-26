//! Saving a window's work as a workspace (PD-023, step 5.4c).
//!
//! A scratch session lives under the state root and is swept once it has
//! nothing left to recover. Save Workspace moves its database and session file
//! into `<folder>/.dat0/` (`Session::promote_to`), so the window's tables,
//! tabs, views and SQL belong to the folder from then on: kept under its lock,
//! opened again from File → Open Workspace or the recent list. The window goes
//! on as the workspace; nothing reopens.
//!
//! The move renames the file the window's engine has open, and a second
//! engine on it would see an empty database. So everything that holds the
//! engine lets go first: the window's session slot turns `Booting`, which
//! takes the grids off the engine (`Views`) and queues a drop, and the save
//! waits for the tasks still running to finish. A console query still running
//! when the wait runs out leaves the session where it was, and the save says
//! so. Anything the wait missed cannot open the file twice: the engine
//! refuses a file an engine of this process still has, by any name.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dioxus::prelude::*;
use parking_lot::Mutex;

use dat0_core::error_ux::Banner;
use dat0_core::session::Session;
use dat0_core::session::slot::SessionSlot;
use dat0_core::workspace::Home;
use dat0_engine::{EngineStatus, QueryEngine as _};
use dat0_i18n::t;

use crate::state::Workspace;

/// How long a save waits for the window's tasks to let go of its engine. A
/// page of rows or a completion list takes milliseconds; a console query still
/// running is a reason to try again once it ends, not to wait on it.
const RELEASE_WAIT: Duration = Duration::from_secs(5);

/// Save the window `ws`'s session as a workspace in `folder`.
pub async fn save(ws: Workspace, folder: PathBuf) {
    let mut ws = ws;
    let Some(session) = ws.session.peek().ready().cloned() else {
        return;
    };
    // What a read-only window shows is not its to move.
    if *ws.read_only.peek() {
        ws.push_banner(Banner::info(t("view.read_only")));
        return;
    }
    if session.lock().is_workspace() {
        ws.push_banner(Banner::info(t("workspace.save.already")));
        return;
    }
    let root = std::fs::canonicalize(&folder).unwrap_or(folder);
    if Home::dat0_dir_for(&root).exists() {
        ws.push_banner(Banner::warning_with_body(
            t("workspace.save.exists"),
            root.display().to_string(),
        ));
        return;
    }
    let boot = try_consume_context::<crate::launch::Boot>();

    ws.session.set(Arc::new(SessionSlot::Booting));
    let slot = match alone(session, RELEASE_WAIT).await {
        Ok(session) => {
            close_console_results(ws);
            promote(ws, boot, session, &root).await
        }
        Err(session) => {
            ws.push_banner(Banner::warning(t("workspace.save.busy")));
            SessionSlot::Ready(session)
        }
    };
    crate::session_boot::land(ws, slot).await;
}

/// Close the tabs showing a console run's rows: they are a view in this
/// engine alone, and do not move. The query's SQL stays in the console.
fn close_console_results(ws: Workspace) {
    let (mut tabs, mut active) = (ws.tabs, ws.active);
    if !tabs.peek().iter().any(|t| t.table.starts_with("__dat0")) {
        return;
    }
    let showing = active
        .peek()
        .and_then(|i| tabs.peek().get(i).map(|t| t.table.clone()));
    tabs.write().retain(|t| !t.table.starts_with("__dat0"));
    let left = tabs.peek();
    let at = showing
        .and_then(|table| left.iter().position(|t| t.table == table))
        .or_else(|| (!left.is_empty()).then_some(0));
    active.set(at);
}

/// The session, once nothing else holds it or its engine; or back, when
/// something still does after `wait`.
async fn alone(
    session: Arc<Mutex<Session>>,
    wait: Duration,
) -> Result<Session, Arc<Mutex<Session>>> {
    let deadline = Instant::now() + wait;
    loop {
        if Arc::strong_count(&session) == 1 && Arc::strong_count(&session.lock().engine) == 1 {
            // Nothing else can reach either now, so neither count can rise.
            return Arc::try_unwrap(session).map(Mutex::into_inner);
        }
        if Instant::now() >= deadline {
            return Err(session);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Move `session` into `root`, and what the window goes on with: the
/// workspace, or what is on disk when the move failed part-way.
async fn promote(
    ws: Workspace,
    boot: Option<crate::launch::Boot>,
    mut session: Session,
    root: &Path,
) -> SessionSlot {
    let budget = dat0_core::settings::budget::configured();
    let scratch = session.home.root_dir().to_path_buf();
    // Where each table came from, which an engine opened from disk does not
    // know (PD-031): given back if the window has to reopen.
    let origins = session.engine.origins();
    let failure = match session.promote_to(root, budget).await {
        Ok(()) => {
            adopted(ws, boot.as_ref(), &mut session, root);
            ws.push_banner(Banner {
                body: root.display().to_string(),
                ..Banner::info(t("workspace.save.done.title"))
            });
            return SessionSlot::Ready(Arc::new(Mutex::new(session)));
        }
        Err(e) => format!("{e:#}"),
    };
    tracing::warn!(error = %failure, "save workspace failed");
    ws.push_banner(Banner::error(t("workspace.save.failed.title"), failure));
    // Refused before anything moved: the session is as it was.
    if matches!(session.engine.status(), EngineStatus::Ready) {
        return SessionSlot::Ready(Arc::new(Mutex::new(session)));
    }
    // Its engine is closed. Let the file go, then open what is on disk.
    drop(session);
    let dat0 = Home::dat0_dir_for(root);
    // Moved only once it is gone from the scratch directory: a move across
    // volumes copies, and a copy cut short leaves the original where it was.
    let moved = dat0.join("workspace.duckdb").is_file() && !scratch.join("scratch.duckdb").exists();
    let reopened = if moved {
        // The session file moves after the database, and may not have.
        if !dat0.join("session.json").exists()
            && let Err(e) = std::fs::copy(scratch.join("session.json"), dat0.join("session.json"))
        {
            tracing::warn!(error = %e, "save workspace: the session file stayed behind");
        }
        let finished = dat0_core::workspace::promote::finish_incomplete(
            &dat0,
            dat0_core::time::now_epoch_secs(),
        );
        match finished {
            Ok(()) => Session::recover_workspace(root.to_path_buf(), budget)
                .await
                .map(|mut s| {
                    adopted(ws, boot.as_ref(), &mut s, root);
                    s
                }),
            Err(e) => Err(e),
        }
    } else {
        Session::recover(scratch, budget).await
    };
    match reopened {
        Ok(s) => {
            for (name, origin) in origins {
                s.engine.restore_origin(&name, origin);
            }
            SessionSlot::Ready(Arc::new(Mutex::new(s)))
        }
        Err(e) => SessionSlot::Failed(format!("{e:#}")),
    }
}

/// The window's session is now the workspace in `root`: record it in the
/// cross-machine lock on a sync drive, at the top of the recent workspaces,
/// and as this window's, so opening the folder brings this window forward;
/// and name the window for it.
fn adopted(ws: Workspace, boot: Option<&crate::launch::Boot>, session: &mut Session, root: &Path) {
    let networked = dat0_core::workspace::networked::is_networked(
        root,
        &dat0_core::workspace::networked::settings(),
    );
    if networked {
        crate::session_boot::claim_lock(ws, session);
    }
    crate::session_boot::remember(root);
    if let Some(boot) = boot {
        boot.windows.holds(ws.window_id, root.to_path_buf());
    }
    let mut name = ws.name;
    name.set(Workspace::name_for(root));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_session_held_elsewhere_is_given_back() {
        let tmp = tempfile::tempdir().unwrap();
        let session = Arc::new(Mutex::new(
            Session::new(tmp.path(), 128 * 1024 * 1024).await.unwrap(),
        ));
        let engine = Arc::clone(&session.lock().engine);
        let wait = Duration::from_millis(50);
        let Err(back) = alone(Arc::clone(&session), wait).await else {
            panic!("a second holder of the session");
        };
        drop(session);
        let Err(back) = alone(back, wait).await else {
            panic!("a holder of its engine");
        };
        drop(engine);
        let Ok(owned) = alone(back, wait).await else {
            panic!("once it is alone");
        };
        owned.engine.execute("SELECT 1").await.expect("still open");
    }
}
