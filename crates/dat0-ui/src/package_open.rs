//! Opening a `.dat0` package, read-only (PD-023, step 5.4d).
//!
//! A package is a sealed analysis: its tables as Parquet, the recipe that made
//! them, and the tabs, views, saved queries and charts they were shown with.
//! Open Package, the sidebar's Packages rows, the hero's recent list and a
//! dropped package open one in a window of its own (`Opening::Inspect`),
//! read-only: nothing the window does changes the package, and the window
//! refuses edits. They used to open it as a data file, which the drop path
//! refused as a file type it does not know.
//!
//! The window reads the package's tables from its Parquet, extracted under the
//! state root (`inspect/<window id>/`), each a view in a database of its own,
//! and brings back its tabs with their views and its saved queries. Nothing in
//! that directory is the user's: the next launch removes it once the window is
//! gone (`recovery_scan::sweep_inspect`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context as _;
use dioxus::prelude::*;

use dat0_core::error_ux::Banner;
use dat0_core::events::{AppEvent, AppEvents, Opening};
use dat0_core::session::Session;
use dat0_i18n::t;

use crate::state::Workspace;

/// Whether `path` names a package, by its extension.
pub fn is_package(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dat0"))
}

/// Open the package at `package` in a window of its own, on behalf of the
/// window `ws`.
pub fn open(ws: Workspace, events: &AppEvents, package: PathBuf) {
    let package = std::fs::canonicalize(&package).unwrap_or(package);
    if !package.is_file() {
        ws.push_banner(Banner::warning_with_body(
            t("package.open.failed.title"),
            package.display().to_string(),
        ));
        return;
    }
    // The new window belongs to no workbench window, so it is asked for on
    // the process bus, and still opens if this one closes first.
    let bus = crate::launch::process_bus().unwrap_or_else(|| events.clone());
    bus.send(AppEvent::OpenWindow(Opening::Inspect { package }));
}

/// [`open`], from where no bus is to hand: a package dropped on the window,
/// or passed on the command line.
pub fn open_here(ws: Workspace, package: PathBuf) {
    match try_consume_context::<AppEvents>() {
        Some(events) => open(ws, &events, package),
        None => tracing::warn!(package = %package.display(), "open package: no event bus"),
    }
}

/// File → Open Package…: pick a package, then open it.
pub fn pick(ws: Workspace, events: &AppEvents) {
    if !crate::launch::has_desktop() {
        tracing::debug!("open package: no window system, nothing to show");
        return;
    }
    let events = events.clone();
    spawn(async move {
        if let Some(package) = crate::files::pick_package().await {
            open(ws, &events, package);
        }
    });
}

/// The session an Inspect window shows: the package's tables, read from its
/// Parquet, and its tabs, saved queries and charts.
pub async fn build(ws: Workspace, package: &Path, budget: u64) -> anyhow::Result<Session> {
    let state_root = dat0_core::globals::state_root().context("state root not installed")?;
    let dir = state_root.join("inspect").join(ws.window_id.to_string());
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    // Opening checks every entry against its checksum, and the tables are
    // read from Parquet extracted beside them: off the window's thread.
    let (path, extract_to) = (package.to_path_buf(), dir.clone());
    let (parsed, engine) = crate::background::run(async move {
        let parsed =
            dat0_format::Reader::open(&path).with_context(|| format!("open {}", path.display()))?;
        let (engine, _) =
            dat0_core::package::inspect::open_readonly(&parsed, &extract_to, budget).await?;
        anyhow::Ok((parsed, engine))
    })
    .await?;
    let (tabs, saved, charts) = dat0_core::package::session_parts(&parsed);
    remember(package);
    Ok(Session::from_parts(
        dir,
        Arc::new(engine),
        tabs,
        saved,
        charts,
    ))
}

/// Put the package at the top of the recent list, where the sidebar's
/// Packages section and the hero read it.
fn remember(package: &Path) {
    let Some(recents) = dat0_core::globals::recents() else {
        return;
    };
    let Ok(mut recents) = recents.lock() else {
        return;
    };
    let entry = dat0_core::recents::RecentEntry::Package {
        path: package.to_path_buf(),
    };
    if let Err(e) = recents.push(entry) {
        tracing::warn!(error = %format!("{e:#}"), "could not record the recent package");
    }
}
