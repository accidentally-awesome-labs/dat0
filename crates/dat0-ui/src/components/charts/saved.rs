//! Saved charts (step 5.5c).
//!
//! The pane's Save was wired to `|_| {}`, and a saved chart, the demo's and a
//! package's included, could not be shown again. [`save`] keeps the chart in
//! the session under a name, beside the saved queries; [`reopen`] shows a
//! saved chart over its table's tab, from the inspector's lineage.

use dioxus::prelude::*;

use dat0_core::charts::spec::ChartSpec;
use dat0_core::error_ux::Banner;
use dat0_core::session::charts::{SavedChart, default_chart_name, upsert_chart};
use dat0_engine::{QueryEngine, TableOrigin};
use dat0_i18n::t;

use super::host::ChartHost;
use crate::components::modals::{ModalOutcome, ModalReply};
use crate::state::{Modal, Workspace};

/// Save the chart shown under a name the user gives. The prompt opens on the
/// name the chart's type and axes suggest; the chart is kept in the session
/// beside the saved queries, replacing one of the same name.
pub fn save(ws: Workspace, host: ChartHost) {
    let Some(table) = host.bound_to.peek().clone() else {
        return;
    };
    // A query's rows are a TEMP view, which goes with the window: a chart of
    // them could not be drawn again.
    if table.starts_with("__dat0") {
        ws.push_banner(Banner::info(t("chart.save.transient")));
        return;
    }
    let spec = host.spec.peek().clone();
    let mut modal = ws.modal;
    modal.set(Some(Modal::NamePrompt {
        title: t("chart.save.prompt"),
        initial: default_chart_name(&spec),
        placeholder: None,
        confirm_label: Some(t("prompt.save")),
        secret: false,
        reply: ModalReply::new(move |outcome| {
            if let ModalOutcome::Named(name) = outcome {
                keep(ws, host, name.trim().to_string(), spec.clone());
            }
        }),
    }));
}

/// Keep `spec` in the session as the chart `name`.
fn keep(ws: Workspace, host: ChartHost, name: String, spec: ChartSpec) {
    let Some(session) = ws.session.peek().ready().cloned() else {
        return;
    };
    if name.is_empty() {
        return;
    }
    let mut session = session.lock();
    let mut charts = session.charts().to_vec();
    upsert_chart(
        &mut charts,
        SavedChart {
            id: uuid::Uuid::now_v7(),
            name: name.clone(),
            spec,
            saved_at: now_ms(),
        },
    );
    match session.set_charts(charts) {
        Ok(()) => {
            ws.push_banner(Banner {
                body: name,
                ..Banner::info(t("chart.save.done.title"))
            });
            crate::workspace_save::suggest(ws, &session);
            let mut saved = host.saved;
            saved += 1;
        }
        Err(e) => ws.push_banner(Banner::warning_with_body(
            t("chart.save.failed"),
            format!("{e:#}"),
        )),
    }
}

/// Show the saved chart `name` over its table's tab, opening the tab if it
/// was not open. The table is found among the window's tabs, then the
/// session's tables and views, by the name the chart quotes: what a chart
/// stored, perhaps in a package from someone else, is compared with names the
/// window already has, never put into a query itself.
pub fn reopen(ws: Workspace, host: ChartHost, name: String) {
    let Some(session) = ws.session.peek().ready().cloned() else {
        return;
    };
    let (engine, saved) = {
        let s = session.lock();
        let saved = s.charts().iter().find(|c| c.name == name).cloned();
        (s.engine.clone(), saved)
    };
    let Some(saved) = saved else {
        return;
    };
    spawn(async move {
        // The table the chart names, however it was written: `"t"` as this
        // build saves it, or `"main"."t"` as the GPUI build, and so the demo
        // package, stored it.
        let wanted = super::source_table(&saved.spec.source);
        let quotes = |table: &str| wanted.as_deref() == Some(table);
        let open = ws.tabs.peek().iter().find(|t| quotes(&t.table)).cloned();
        let (table, path) = match open {
            Some(tab) => (Some(tab.table), tab.path),
            None => {
                let tables = engine.get_tables().await.unwrap_or_default();
                let table = tables.into_iter().map(|t| t.name).find(|t| quotes(t));
                let path = table.as_ref().and_then(|t| match engine.origins().get(t) {
                    Some(TableOrigin::File(path)) => Some(path.clone()),
                    _ => None,
                });
                (table, path)
            }
        };
        let Some(table) = table else {
            ws.push_banner(Banner {
                body: super::source_label(&saved.spec.source),
                ..Banner::info(t("chart.reopen.gone"))
            });
            return;
        };
        host.show(ws, table, path, saved.spec);
    });
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
