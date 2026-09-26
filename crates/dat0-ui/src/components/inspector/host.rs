//! What feeds the inspector (step 5.5d).
//!
//! The inspector was built with nothing feeding it (PD-023): nothing set its
//! target, so it said "No table selected" whatever was open, and its lineage
//! was never built. [`InspectorHost`] points it at the active tab's table
//! while its pane is open, profiles the table off the window's thread, fetches
//! the small charts its columns carry, and builds its lineage from the
//! session's tables, the tables their SQL reads and the saved charts.
//!
//! Whole table mode profiles the stored table. dat0 lays a sort, a filter or
//! an edit over it as a view, so the profile is kept until the table's rows
//! are read again (a Live Refresh, a console run). Current view mode profiles
//! what the tab's grid reads, again whenever that changes.

use std::collections::HashMap;
use std::sync::Arc;

use dioxus::prelude::*;

use dat0_core::charts::data::PlotTable;
use dat0_core::inspector::ProfileTargetMode;
use dat0_core::inspector::lineage::{ChartNode, LineageGraph, NodeKind};
use dat0_engine::{
    DerivedOrigin, DuckDBEngine, QueryEngine, ROWID_COL, TableOrigin, TableProfile, quote_ident,
};

use super::InspectorState;
use crate::components::charts::host::{ChartHost, reads};
use crate::components::grid::views::Views;
use crate::state::Workspace;

/// A column with at most this many distinct values has its most common values
/// drawn; a numeric column with more, a histogram. The GPUI build's cut.
const TOPN_MAX_DISTINCT: u64 = 24;
/// How many of a column's most common values are drawn.
const TOPN: u64 = 8;
/// Rows sampled for a histogram, and its bins.
const HISTOGRAM_SAMPLE: u32 = 2048;
const HISTOGRAM_BINS: usize = 16;

/// The inspector's state and what feeds it.
#[derive(Clone, Copy)]
pub struct InspectorHost {
    pub state: InspectorState,
    /// Bumped to profile the target again: the mode toggle.
    reload: Signal<u64>,
}

impl InspectorHost {
    /// This window's inspector. A hook: call it once, from the shell's body.
    pub fn use_new(ws: Workspace, views: Views, charts: ChartHost) -> Self {
        let state = InspectorState::use_new();
        let reload = use_signal(|| 0u64);
        let open = use_memo(move || ws.layout.read().inspector_visible);
        let table = use_memo(move || ws.active_tab().map(|t| t.table));
        // Each table's re-read count when it was last profiled.
        let mut seen = use_signal(HashMap::<String, u64>::new);

        // Every signal read inside the future restarts it; the inspector's
        // own model is read without subscribing (`InspectorState::mode`), or
        // the profile it writes would start it again.
        let _profile = use_resource(move || async move {
            if !open() {
                return;
            }
            let Some(table) = table() else {
                return;
            };
            let _ = reload();
            let reread = views.reread.read().get(&table).copied().unwrap_or(0);
            if seen.peek().get(&table).copied().unwrap_or(0) != reread {
                seen.write().insert(table.clone(), reread);
                state.bump_epoch(&table);
            }
            let view = match state.mode() {
                ProfileTargetMode::CurrentView => Some(reads(views, &table)),
                ProfileTargetMode::WholeTable => None,
            };
            if state.target().as_deref() != Some(table.as_str()) {
                state.set_target(table.clone());
            }
            if view.is_none() && state.has_profile() {
                return;
            }
            let Some(engine) = ws.session.read().ready().map(|s| s.lock().engine.clone()) else {
                return;
            };
            let load = state.begin_load();
            let profiled = match &view {
                Some(view) => {
                    let sql = format!("SELECT * FROM {}", quote_ident(view));
                    engine.profile_query(&sql).await
                }
                None => engine.profile_table(&table, None).await,
            };
            match profiled {
                Ok(profile) => {
                    // The small charts read the stored table, so they belong
                    // beside a whole-table profile only.
                    if state.put_profile(load, profile.clone()) && view.is_none() {
                        extras(state, engine, &table, load, profile).await;
                    }
                }
                Err(e) => tracing::warn!(error = %e, %table, "inspector: profile failed"),
            }
        });

        let _lineage = use_resource(move || async move {
            if !open() {
                return;
            }
            let Some(table) = table() else {
                return;
            };
            // A table saved, a chart saved, rows read again: the graph may
            // have changed.
            let _ = (
                ws.tabs.read().len(),
                charts.saved.read(),
                views.reread.read().len(),
            );
            let Some(session) = ws.session.read().ready().cloned() else {
                return;
            };
            let (engine, saved) = {
                let s = session.lock();
                (s.engine.clone(), s.charts().to_vec())
            };
            let tables = match engine.get_tables().await {
                Ok(tables) => tables,
                Err(e) => return tracing::warn!(error = %e, "inspector: tables unread"),
            };
            let mut sql_parents = HashMap::new();
            for t in &tables {
                if let TableOrigin::Derived(DerivedOrigin::Sql(sql)) = &t.origin
                    && !sql.is_empty()
                {
                    match engine.referenced_tables(sql).await {
                        Ok(parents) => {
                            sql_parents.insert(t.name.clone(), parents);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, table = %t.name, "lineage edge skipped")
                        }
                    }
                }
            }
            let nodes: Vec<ChartNode> = saved
                .iter()
                .map(|c| ChartNode {
                    name: c.name.clone(),
                    source_table: unquote(&c.spec.source),
                })
                .collect();
            let graph = LineageGraph::build(&tables, &sql_parents, &nodes);
            state.set_lineage(graph.closure(&table));
        });

        Self { state, reload }
    }

    /// The mode toggle flipped: profile the target again. The profile kept
    /// for the table is the other mode's, so it goes first.
    pub fn reload(&self) {
        if let Some(table) = self.state.target() {
            self.state.bump_epoch(&table);
        }
        let mut reload = self.reload;
        reload += 1;
    }
}

/// Fetch the small charts a freshly profiled table's columns carry: the most
/// common values of a column with few, a histogram of a numeric column with
/// many. Each is written under the profile's load id, so a table switched
/// away from never paints its bars under the next one's cards.
async fn extras(
    state: InspectorState,
    engine: Arc<DuckDBEngine>,
    table: &str,
    load: u64,
    profile: TableProfile,
) {
    for col in profile.columns {
        if col.name == ROWID_COL {
            continue;
        }
        if col.approx_distinct > 0 && col.approx_distinct <= TOPN_MAX_DISTINCT {
            match engine.column_topn(table, &col.name, TOPN).await {
                Ok(top) => {
                    if !state.put_topn(load, &col.name, top) {
                        return;
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, col = %col.name, "inspector: top values failed")
                }
            }
            continue;
        }
        let Some(numeric) = col.numeric else {
            continue;
        };
        let sql = format!(
            "SELECT CAST({c} AS DOUBLE) AS v FROM {t} WHERE {c} IS NOT NULL \
             USING SAMPLE {HISTOGRAM_SAMPLE} ROWS",
            c = quote_ident(&col.name),
            t = quote_ident(table),
        );
        let values = match engine.execute(&sql).await {
            Ok(result) => PlotTable::from_query_result(&result)
                .columns
                .into_iter()
                .next()
                .and_then(|c| c.num)
                .unwrap_or_default(),
            Err(e) => {
                tracing::warn!(error = %e, col = %col.name, "inspector: histogram sample failed");
                continue;
            }
        };
        if values.is_empty() {
            continue;
        }
        let bins =
            dat0_core::charts::histogram_bins(numeric.min, numeric.max, &values, HISTOGRAM_BINS);
        if !state.put_histogram(load, &col.name, bins) {
            return;
        }
    }
}

/// A lineage row was clicked: a saved chart comes back as it was saved, a
/// table comes up in its tab. A file has nothing to open.
pub fn open(ws: Workspace, charts: ChartHost, (kind, name): (NodeKind, String)) {
    match kind {
        NodeKind::Chart => crate::components::charts::saved::reopen(ws, charts, name),
        NodeKind::Table | NodeKind::External => {
            let path = ws.session.peek().ready().and_then(|s| {
                match s.lock().engine.origins().get(&name) {
                    Some(TableOrigin::File(path)) => Some(path.clone()),
                    _ => None,
                }
            });
            ws.show_tab(name, path);
        }
        NodeKind::File => {}
    }
}

/// A quoted identifier as the name it quotes; anything else as it is.
fn unquote(source: &str) -> String {
    source
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .map(|s| s.replace("\"\"", "\""))
        .unwrap_or_else(|| source.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quoted_name_reads_as_the_name_it_quotes() {
        assert_eq!(unquote("\"sales\""), "sales");
        assert_eq!(unquote(&quote_ident("a\"b")), "a\"b");
        assert_eq!(unquote("plain"), "plain");
    }
}
