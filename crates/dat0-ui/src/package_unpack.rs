//! Unpacking a `.dat0` package into a workspace (PD-023, step 5.4d).
//!
//! File → Unpack Package was disabled. It now asks for a package and a folder,
//! writes the package's tables, tabs, saved queries and charts into the
//! folder's `.dat0/` as a workspace (`package::contents_to_workspace`), and
//! opens the workspace in a window of its own, editable. A folder that is a
//! workspace already is refused before the package is read, and an unpack that
//! fails leaves the folder as it was.

use std::path::PathBuf;

use anyhow::Context as _;
use dioxus::prelude::*;

use dat0_core::error_ux::Banner;
use dat0_core::events::AppEvents;
use dat0_core::workspace::Home;
use dat0_i18n::t;

use crate::state::Workspace;

/// File → Unpack Package…: pick a package, then the folder to unpack it into.
pub fn pick(ws: Workspace, events: &AppEvents) {
    if !crate::launch::has_desktop() {
        tracing::debug!("unpack package: no window system, nothing to show");
        return;
    }
    let events = events.clone();
    spawn(async move {
        let Some(package) = crate::files::pick_package().await else {
            return;
        };
        let Some(folder) = crate::files::pick_folder_to_unpack().await else {
            return;
        };
        unpack(ws, &events, package, folder).await;
    });
}

/// Unpack `package` into `folder` as a workspace, then open it.
pub async fn unpack(ws: Workspace, events: &AppEvents, package: PathBuf, folder: PathBuf) {
    let root = std::fs::canonicalize(&folder).unwrap_or(folder);
    let dat0 = Home::dat0_dir_for(&root);
    if dat0.exists() {
        ws.push_banner(Banner::warning_with_body(
            t("package.unpack.exists"),
            root.display().to_string(),
        ));
        return;
    }
    let budget = dat0_core::settings::budget::configured();
    let into = root.clone();
    let unpacked = crate::background::run(async move {
        let parsed = dat0_format::Reader::open(&package)
            .with_context(|| format!("open {}", package.display()))?;
        dat0_core::package::contents_to_workspace(&parsed, &into, budget).await
    })
    .await;
    match unpacked {
        Ok(()) => {
            ws.push_banner(Banner {
                body: root.display().to_string(),
                ..Banner::info(t("package.unpack.done.title"))
            });
            crate::workspace_open::open(ws, events, root);
        }
        Err(e) => ws.push_banner(Banner::error(
            t("package.unpack.failed.title"),
            format!("{e:#}"),
        )),
    }
}
