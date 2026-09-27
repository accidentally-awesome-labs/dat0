//! Exporting a window's work as a `.dat0` package (PD-023, step 5.4d).
//!
//! File → Export as .dat0 Package was disabled. It now asks where to write the
//! package and seals the window's tables into it, with its tabs' views, its
//! saved queries and charts. What the package carries of the session is copied
//! under a brief lock (`package::Portable`); the engine work that follows holds
//! none. A package already at the path is replaced only once the new one is
//! whole.

use std::path::PathBuf;

use dioxus::prelude::*;

use dat0_core::error_ux::Banner;
use dat0_core::package::{self, Portable};
use dat0_i18n::t;

use crate::state::Workspace;

/// File → Export as .dat0 Package…: ask where, then export.
pub fn pick(ws: Workspace) {
    if !crate::launch::has_desktop() {
        tracing::debug!("export package: no window system, nothing to show");
        return;
    }
    let suggested = format!("{}.dat0", ws.name.peek());
    spawn(async move {
        let Some(dest) = crate::files::pick_save_path(&suggested).await else {
            return;
        };
        export(ws, dest).await;
    });
}

/// Seal the window `ws`'s tables, with its views, saved queries and charts,
/// into a package at `dest`.
pub async fn export(ws: Workspace, dest: PathBuf) {
    let Some(session) = ws.session.peek().ready().cloned() else {
        ws.push_banner(Banner::warning(t("package.export.no_workspace.title")));
        return;
    };
    let (engine, portable) = {
        let s = session.lock();
        (s.engine.clone(), Portable::of(&s))
    };
    let to = dest.clone();
    let written = crate::background::run(async move {
        let contents = package::contents_from_engine(engine.as_ref(), portable).await?;
        dat0_format::Writer::write(&contents, engine.as_ref(), &to).await?;
        anyhow::Ok(())
    })
    .await;
    match written {
        Ok(()) => ws.push_banner(Banner {
            body: dest.display().to_string(),
            ..Banner::info(t("package.export.done.title"))
        }),
        Err(e) => ws.push_banner(Banner::error(
            t("package.export.failed.title"),
            format!("{e:#}"),
        )),
    }
}
