//! Export…: write what the grid shows, or its whole table, to a file.
//!
//! The dialog was reachable and its answer was thrown away (PD-023): Browse
//! did nothing, and Export closed the dialog and wrote nothing. Its choices
//! now go where the GPUI build sent them — `build_export` for what to read,
//! `render_export_select` for which columns under which names (the view's
//! projection, without the row id), and DuckDB's `COPY … TO` to stream it to
//! disk.

use std::path::PathBuf;

use dioxus::prelude::*;

use dat0_core::error_ux::Banner;
use dat0_core::view::export_dialog::build_export;
use dat0_engine::{QueryEngine, quote_ident, render_export_select};
use dat0_i18n::t;

use super::views::{Shown, Views, engine};
use crate::components::export_dialog::ExportRequest;
use crate::components::modals::{ModalOutcome, ModalReply};
use crate::state::{Modal, Workspace};

/// Open the dialog for what the grid shows. It starts pointed at the folder
/// the tab's file came from, when it came from one.
pub fn open(ws: Workspace, views: Views, shown: Shown) {
    // No tab, nothing to write: the dialog opened, and Export closed it
    // having written nothing, saying nothing (step 5.11c).
    if ws.active_tab().is_none() {
        nothing_to_export(ws);
        return;
    }
    let beside = ws
        .active_tab()
        .and_then(|t| t.path)
        .and_then(|p| p.parent().map(PathBuf::from));
    dialog(ws, views, shown, beside);
}

fn dialog(ws: Workspace, views: Views, shown: Shown, destination: Option<PathBuf>) {
    let mut modal = ws.modal;
    modal.set(Some(Modal::Export {
        destination,
        reply: ModalReply::new(move |outcome| match outcome {
            // The dialog stays up, keeping what was typed into it, while the
            // picker runs; its answer comes back as a new destination.
            ModalOutcome::BrowseDestination => {
                if !crate::launch::has_desktop() {
                    return;
                }
                spawn(async move {
                    if let Some(dir) = crate::files::pick_folder().await {
                        dialog(ws, views, shown, Some(dir));
                    }
                });
            }
            ModalOutcome::Export(req) => write(ws, views, shown, req),
            _ => {}
        }),
    }));
}

/// Say there is nothing to export: the grid shows no table.
fn nothing_to_export(ws: Workspace) {
    ws.push_banner(Banner::warning_with_body(
        t("export.nothing"),
        t("export.nothing.body"),
    ));
}

/// Write `req`'s scope of the grid's table to `req.path`.
fn write(ws: Workspace, views: Views, shown: Shown, req: ExportRequest) {
    let Some((table, Ok(src))) = shown.peek().clone().flatten() else {
        nothing_to_export(ws);
        return;
    };
    let Some(engine) = engine(&ws) else {
        return;
    };
    let names = src.visible_column_names();
    let columns = views.columns(&table, &names);
    // The view the grid reads, when it reads one rather than the table.
    let view = views
        .bound
        .peek()
        .get(&table)
        .map(|(reads, _)| reads.clone())
        .filter(|reads| *reads != table)
        .map(|reads| quote_ident(&reads));
    let (rows, cols) = build_export(
        req.scope,
        &quote_ident(&table),
        view.as_deref(),
        &columns,
        &names,
    );
    let sql = render_export_select(&rows, &cols);
    spawn(async move {
        match engine
            .export_query_to_path(&sql, req.format, &req.path)
            .await
        {
            Ok(()) => {
                let mut done = Banner::info(t("export.done.title"));
                done.body = req.path.display().to_string();
                ws.push_banner(done);
            }
            Err(e) => ws.push_banner(Banner::warning_with_body(
                t("export.failed.title"),
                e.to_string(),
            )),
        }
    });
}
