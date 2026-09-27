//! A CSV the sniff cannot settle opens in the import wizard (PD-023, step
//! 5.10).
//!
//! `file_drop` hands back `DropOutcome::OpenWizard` for a CSV whose delimiter
//! two sniffs disagree on, or whose first 8 KB is not UTF-8, and the window
//! logged it and let it go: the file never opened, and nothing said why. The
//! wizard (`components::import_wizard`) was built, with its validation, and
//! nothing opened it, in this build or the GPUI one. Now the file opens in
//! the wizard, which asks for the dialect and the columns, and its answer is
//! read in as any file is: a table of the session's own, in a tab.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dioxus::prelude::*;
use parking_lot::Mutex;

use dat0_core::error_ux::Banner;
use dat0_core::import_wizard::SniffSummary;
use dat0_core::session::{Session, Tab};
use dat0_engine::{FileFormat, QueryEngine as _, RegisterOpts, quote_ident};
use dat0_i18n::t;

use crate::components::import_wizard::{WizardModel, describe_csv};
use crate::components::modals::{ModalOutcome, ModalReply};
use crate::state::{Modal, TabView, Workspace};

/// Open `path` in the import wizard, seeded with what the sniff made of it.
///
/// A dialog already up keeps its place, and a banner says the file waits:
/// the wizard needs the user, and the slot holds one dialog.
pub fn offer(ws: Workspace, session: Arc<Mutex<Session>>, path: PathBuf, sniff: SniffSummary) {
    spawn(async move {
        let columns = describe(&path, &sniff.top_delimiter.to_string()).await;
        if ws.modal.peek().is_some() {
            ws.push_banner(Banner::warning_with_body(
                t("wizard.busy"),
                file_name(&path),
            ));
            return;
        }
        let model = WizardModel::from_sniff(&path, &sniff, columns);
        let reply = ModalReply::new(move |outcome: ModalOutcome| {
            if let ModalOutcome::Import(model) = outcome {
                let session = session.clone();
                spawn(async move { import(ws, session, model).await });
            }
        });
        let mut modal = ws.modal;
        modal.set(Some(Modal::ImportWizard {
            model: Signal::new(model),
            reply,
        }));
    });
}

/// The columns DuckDB reads in `path` under `delimiter`, off the window's
/// thread. None when it cannot read it that way: the wizard's dialect step
/// is where that is fixed.
async fn describe(path: &Path, delimiter: &str) -> Vec<(String, String)> {
    let (path, delimiter) = (path.to_path_buf(), delimiter.to_string());
    let shown = path.display().to_string();
    match tokio::task::spawn_blocking(move || describe_csv(&path, &delimiter, "\"", true)).await {
        Ok(Ok(columns)) => columns,
        Ok(Err(e)) => {
            tracing::info!(path = %shown, "the sniff's dialect does not read: {e:#}");
            Vec::new()
        }
        Err(e) => {
            tracing::warn!(path = %shown, "describe task failed: {e}");
            Vec::new()
        }
    }
}

/// Read the file as the wizard describes it, into a table of the session's
/// own, and open it in a tab: the import `file_drop` makes, with the
/// wizard's dialect and types, then its columns and their names.
async fn import(ws: Workspace, session: Arc<Mutex<Session>>, model: WizardModel) {
    let engine = session.lock().engine.clone();
    let path = model.path.clone();
    let opts = RegisterOpts {
        format: Some(FileFormat::Csv),
        delimiter: model.delimiter.chars().next(),
        quote_char: model.quote.chars().next(),
        has_header: Some(model.has_header),
        type_overrides: model
            .columns
            .iter()
            .map(|c| (c.source.clone(), c.ty.clone()))
            .collect(),
        ..Default::default()
    };
    let info = match engine.register_file_as_table(&path, opts).await {
        Ok(info) => info,
        Err(e) => {
            ws.push_banner(Banner::error(
                t("drop.register_failed"),
                format!("{}: {e}", path.display()),
            ));
            return;
        }
    };
    if let Err(e) = shape(&engine, &info.name, &model).await {
        // The table is there, as the file reads; say what was not done.
        ws.push_banner(Banner::warning_with_body(
            t("wizard.shape_failed"),
            format!("{e:#}"),
        ));
    }

    let persisted = session.lock().add_tab(Tab {
        table_name: info.name.clone(),
        source_path: Some(path.clone()),
        transform_stack: Vec::new(),
        undo_cursor: 0,
        extra: Default::default(),
    });
    if let Err(e) = persisted {
        tracing::warn!(?path, "opened, but could not persist the session: {e:#}");
        ws.push_banner(Banner::warning_with_body(
            t("session.persist_failed"),
            format!("{e:#}"),
        ));
    }
    let mut ws = ws;
    ws.tabs.write().push(TabView {
        table: info.name,
        path: Some(path),
        label: None,
    });
    let last = ws.tabs.read().len() - 1;
    ws.active.set(Some(last));
}

/// The wizard's columns on the table read in: the ones left out dropped,
/// then the rest renamed, through placeholder names first so that a swap
/// (`a` → `b`, `b` → `a`) cannot collide with itself.
async fn shape(
    engine: &Arc<dat0_engine::DuckDBEngine>,
    table: &str,
    model: &WizardModel,
) -> anyhow::Result<()> {
    let table = quote_ident(table);
    for c in model.columns.iter().filter(|c| !c.include) {
        engine
            .execute(&format!(
                "ALTER TABLE {table} DROP COLUMN {}",
                quote_ident(&c.source)
            ))
            .await?;
    }
    let renamed: Vec<(&str, &str)> = model
        .included()
        .map(|c| (c.source.as_str(), c.name.trim()))
        .filter(|(source, name)| source != name)
        .collect();
    for (i, (source, _)) in renamed.iter().enumerate() {
        engine
            .execute(&format!(
                "ALTER TABLE {table} RENAME COLUMN {} TO {}",
                quote_ident(source),
                quote_ident(&format!("__dat0_wizard_{i}"))
            ))
            .await?;
    }
    for (i, (_, name)) in renamed.iter().enumerate() {
        engine
            .execute(&format!(
                "ALTER TABLE {table} RENAME COLUMN {} TO {}",
                quote_ident(&format!("__dat0_wizard_{i}")),
                quote_ident(name)
            ))
            .await?;
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
