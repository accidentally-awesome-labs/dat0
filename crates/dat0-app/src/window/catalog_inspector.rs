//! Catalog tree and Inspector (P6a / P6b): refreshing the tree, keyboard
//! navigation over it, opening a table tab, and the Inspector's column
//! profile, extras, and lineage chain.
//!
//! These two surfaces share a module because the catalog selects what the
//! Inspector profiles — `set_inspector_target` is the seam, and splitting
//! them would put a two-call handoff across a module boundary.

use super::*;

impl WorkspaceShell {
    /// Recompute `column_view` from the base columns (the visible source columns
    /// of the active view) + the active transform stack (P4c T5). Called after
    /// every stack change and after a data-source (re)bind so the view never
    /// goes stale.
    ///
    /// With no projection ops in the stack the fold is the identity over the
    /// visible columns, so `source_for_screen_col(&column_view, i)` returns the
    /// same column `ds.column_name(i)` does — existing behaviour is unchanged.
    pub(crate) fn refresh_column_view(&mut self) {
        let base: Vec<String> = self
            .data_source
            .as_ref()
            .map(|ds| ds.visible_column_names())
            .unwrap_or_default();
        let ops: &[dat0_engine::Transformation] = self
            .view_model
            .as_ref()
            .map(|vm| vm.active())
            .unwrap_or(&[]);
        self.column_view = crate::view::fold_columns(&base, ops);
    }

    /// The active grid tab's column projection, for the Inspector to mirror —
    /// but only when the Inspector is actually targeting that tab's table, so
    /// inspecting table X while the grid shows Y never mis-projects. `None`
    /// otherwise (no view/data source, or a cross-table target) → the Inspector
    /// falls back to its raw, unprojected card list. Identity is the bare
    /// (unquoted) table name, consistent with the app's catalog/lineage keying.
    pub(crate) fn inspector_projection(
        &self,
    ) -> Option<crate::inspector::projection::ProjectionContext> {
        let target = self.inspector.target_table.as_deref()?;
        let vm = self.view_model.as_ref()?;
        let ds = self.data_source.as_ref()?;
        // `base_table()` is quoted `"schema"."table"`; reduce to the bare name.
        let active = vm
            .base_table()
            .rsplit('.')
            .next()
            .unwrap_or("")
            .trim_matches('"');
        if active != target {
            return None;
        }
        Some(crate::inspector::projection::ProjectionContext {
            visible: self.column_view.clone(),
            base_sources: ds.visible_column_names(),
        })
    }

    /// Resolve a header (screen) column index to its bare SOURCE column name via
    /// the active `ColumnView` (P4c T5). Returns `None` if no column maps to
    /// `col_ix`.
    ///
    /// Screen-col→source is resolved through the folded `column_view` rather
    /// than positionally over the Arrow schema, so after a display-only reorder
    /// or delete a screen index still addresses the right source column. With no
    /// projection ops the view is identity, so this is equivalent to the
    /// previous `ds.column_name(col_ix)`.
    pub(crate) fn column_name(&self, col_ix: usize) -> Option<String> {
        crate::view::column_view::source_for_screen_col(&self.column_view, col_ix)
            .map(str::to_string)
    }

    /// Open `name` into the main grid (P6a T7). The main window is single-view —
    /// one `view_model` at a time — so "open" mirrors the file-import load path
    /// (window.rs `last_registered` branch): build a `GridDataSource` off-thread,
    /// then on the main thread install a fresh `ViewModel` + data source.
    ///
    /// Bridge: a raw `tokio::spawn` + the `window_registry::dispatcher()` →
    /// upgraded weak handle, matching `refresh_completion_snapshot` (the import
    /// branch's `async_cx.update` bridge is only reachable from inside the render
    /// `cx.spawn`; this is an on-click handler, so the dispatcher bridge is the
    /// canonical off-thread→main-thread write here).
    pub(crate) fn open_table_tab(
        &mut self,
        name: String,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            match crate::grid::GridDataSource::new(engine, name.clone()).await {
                Ok(ds) => {
                    if let Some(dispatcher) = crate::window_registry::dispatcher() {
                        let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                            let Some(ws) = ws_weak.upgrade() else {
                                return;
                            };
                            ws.update(app_cx, |ws, cx| {
                                // base_table passed to ViewModel must already be quoted.
                                let quoted = format!("\"{}\"", name.replace('"', "\"\""));
                                ws.view_model =
                                    Some(crate::view::model::ViewModel::new(name.clone(), quoted));
                                ws.set_data_source(std::sync::Arc::new(ds));
                                // P6a T9: point the Inspector at the freshly-opened
                                // table and (lazily) load its profile.
                                ws.set_inspector_target(name.clone(), cx);
                                // P7c: re-target the live-data watch onto the
                                // newly-active table's source file. The catalog
                                // is already populated for an already-imported
                                // table being re-opened, so this resolves now;
                                // `refresh_catalog` retargets again as a backstop.
                                ws.retarget_source_watch(cx);
                                cx.notify();
                            });
                        });
                    } else {
                        tracing::warn!(
                            "open_table_tab: no MainThreadDispatcher installed; table not opened"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "open_table_tab: GridDataSource::new failed")
                }
            }
        });
    }

    /// Rebuild [`Self::catalog_tree`] from the engine's table list (P6a T7).
    /// Mirrors `refresh_completion_snapshot`: enumerate `get_tables()` off-thread,
    /// then write the freshly-built tree on the main thread via the dispatcher +
    /// upgraded weak handle. Called on every catalog-mutation point (toggle /
    /// import / create / drop / save-as-table).
    pub(crate) fn refresh_catalog(&mut self, cx: &mut gpui::Context<Self>) {
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::{DerivedOrigin, QueryEngine as _, TableOrigin};
            let tables = match engine.get_tables().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(error = %e, "refresh_catalog: get_tables failed");
                    return;
                }
            };
            // Resolve each Sql-origin table's lineage parents off-thread so
            // `recompute_lineage` (on the main thread) stays synchronous (P6b).
            let mut sql_parents = std::collections::HashMap::new();
            for t in &tables {
                if let TableOrigin::Derived(DerivedOrigin::Sql(sql)) = &t.origin {
                    if !sql.is_empty() {
                        match engine.referenced_tables(sql).await {
                            Ok(parents) => {
                                sql_parents.insert(t.name.clone(), parents);
                            }
                            Err(e) => tracing::warn!(error = %e, table = %t.name,
                                "refresh_catalog: referenced_tables failed; lineage edge skipped"),
                        }
                    }
                }
            }
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    let Some(ws) = ws_weak.upgrade() else {
                        return;
                    };
                    ws.update(app_cx, |ws, cx| {
                        ws.catalog_tree = crate::catalog::CatalogTree::build(&tables);
                        ws.catalog_tables = tables;
                        ws.sql_parents = sql_parents;
                        ws.recompute_lineage();
                        // P7c: now that `catalog_tables` is current, (re)target the
                        // live-data watch onto the active table's source file. This
                        // is the authoritative retarget point after a fresh import
                        // (the file-drop mount has stale catalog at mount time).
                        ws.retarget_source_watch(cx);
                        cx.notify();
                    });
                });
            } else {
                tracing::warn!("refresh_catalog: no MainThreadDispatcher installed; catalog stale");
            }
        });
    }

    /// Apply one keyboard-nav key to the Catalog panel: flatten the current
    /// tree, clamp the active index (SINGLE clamp site — ring, arrows and
    /// Enter all use this same index), then act on the pure `tree_nav`
    /// transition. Both the container's `focus_stop` activate (enter/space)
    /// and the chained arrow handler route here (single source of truth).
    pub(crate) fn catalog_nav_key(
        &mut self,
        key: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let rows = crate::catalog::nav::visible_rows(&self.catalog_tree, &self.catalog_collapsed);
        if rows.is_empty() {
            return;
        }
        let active = self.catalog_active.min(rows.len() - 1);
        self.catalog_active = active;
        match crate::catalog::nav::tree_nav(&rows, active, key) {
            crate::catalog::nav::NavAction::Move(i) => {
                self.catalog_active = i;
                cx.notify();
            }
            crate::catalog::nav::NavAction::Toggle(alias) => {
                self.toggle_catalog_parent(alias, cx);
            }
            crate::catalog::nav::NavAction::Open(name) => {
                self.open_table_tab(name, window, cx);
            }
            crate::catalog::nav::NavAction::None => {}
        }
    }

    /// Flip an attach parent's expand/collapse state. Single source of truth —
    /// the parent row's mouse `on_click` AND the keyboard Toggle arm both call
    /// this (mouse and keyboard cannot drift). Clamps the active index against
    /// the post-toggle row count so a collapse can never dangle the ring.
    pub(crate) fn toggle_catalog_parent(&mut self, alias: String, cx: &mut gpui::Context<Self>) {
        if !self.catalog_collapsed.remove(&alias) {
            self.catalog_collapsed.insert(alias);
        }
        let rows = crate::catalog::nav::visible_rows(&self.catalog_tree, &self.catalog_collapsed);
        self.catalog_active = self.catalog_active.min(rows.len().saturating_sub(1));
        self.persist_dock_ui();
        cx.notify();
    }

    /// Point the Inspector at `name` and load its profile (P6a T9). If the
    /// (table,epoch) profile is already cached the load is skipped (warm hit);
    /// otherwise [`Self::load_inspector_profile`] fetches it off-thread.
    ///
    /// Takes no `Window` — none of the inspector methods need one, which lets
    /// `open_table_tab`'s dispatcher closure (which has no `window`) call this.
    pub(crate) fn set_inspector_target(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        self.inspector.set_target(name);
        self.recompute_lineage();
        if self.inspector.cached().is_some() {
            // Warm hit — nothing to load; just repaint the dock.
            cx.notify();
            return;
        }
        self.load_inspector_profile(cx);
        cx.notify();
    }

    /// Rebuild the Inspector's lineage chain for the current target from the
    /// cached `catalog_tables` + `sql_parents` (P6b). Called on every catalog
    /// refresh, on `set_inspector_target`, and on rebind (PD-022). Takes no `cx`;
    /// the caller is responsible for `cx.notify()` afterward.
    pub(crate) fn recompute_lineage(&mut self) {
        if let Some(target) = self.inspector.target_table.clone() {
            // P9a-2: saved charts join the lineage as descendants of their source
            // table. Each chart's `spec.source` is a quoted (possibly qualified)
            // identifier; reduce it to the bare catalog key the lineage matches on.
            let chart_nodes: Vec<crate::inspector::lineage::ChartNode> = {
                let sess = self.session.lock();
                sess.charts()
                    .iter()
                    .map(|c| crate::inspector::lineage::ChartNode {
                        name: c.name.clone(),
                        source_table: bare_table_name(&c.spec.source),
                    })
                    .collect()
            };
            let graph = crate::inspector::lineage::LineageGraph::build(
                &self.catalog_tables,
                &self.sql_parents,
                &chart_nodes,
            );
            self.inspector.set_lineage(graph.closure(&target));
        }
    }

    /// Load the profile for the current inspector target off-thread, then write
    /// it back on the main thread under the supersede guard (P6a T9). Mirrors
    /// [`Self::open_table_tab`] / [`Self::refresh_catalog`]: `tokio::spawn` +
    /// `window_registry::dispatcher()` + upgraded weak handle.
    ///
    /// In [`ProfileTargetMode::CurrentView`] the active view's `SELECT` is
    /// compiled off the live `view_model` and profiled via `profile_query`;
    /// otherwise the stored table is profiled via `profile_table`.
    pub(crate) fn load_inspector_profile(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(target) = self.inspector.target_table.clone() else {
            return;
        };
        let mode = self.inspector.mode;
        let load_id = self.inspector.begin_load();
        let engine = self.engine();
        // For CurrentView mode, compile the active view's SELECT off the view_model.
        let view_sql: Option<String> =
            if matches!(mode, crate::inspector::ProfileTargetMode::CurrentView) {
                self.view_model
                    .as_ref()
                    .and_then(|vm| dat0_engine::compile_view_sql(vm.base_table(), vm.active()).ok())
            } else {
                None
            };
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let result = match view_sql {
                Some(sql) => engine.profile_query(&sql).await,
                None => engine.profile_table(&target, None).await,
            };
            let profile = match result {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "inspector profile load failed");
                    return;
                }
            };
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    let Some(ws) = ws_weak.upgrade() else {
                        return;
                    };
                    ws.update(app_cx, |ws, cx| {
                        // Supersede guard: only the latest load writes its result.
                        if ws.inspector.is_current(load_id) {
                            ws.inspector.put(profile.clone());
                            cx.notify();
                            // Fetch inline-chart extras for the columns that
                            // qualify, reusing this load's `load_id` to guard
                            // against stale bars when tables switch fast (T10).
                            ws.load_column_extras(load_id, profile, cx);
                        }
                    });
                });
            } else {
                tracing::warn!(
                    "load_inspector_profile: no MainThreadDispatcher installed; profile dropped"
                );
            }
        });
    }

    /// Fetch inline-chart extras for a freshly-loaded profile (P6a T10), reusing
    /// the profile load's `load_id` as the supersede guard so switching tables
    /// fast never lands stale bars. Eager-on-load (vs lazy-on-expand): acceptable
    /// because the *qualifying* set is small — only low-cardinality columns get a
    /// `column_topn` fetch and only numeric high-cardinality columns get a
    /// sampled histogram; everything else issues no query. This naturally bounds
    /// the query count for typical tables.
    ///
    /// Extras are only fetched in `WholeTable` mode: they query the base table by
    /// its bare name, so they match the profiled data only when the profile is of
    /// the whole table (in `CurrentView` the profile is of a filtered SELECT).
    fn load_column_extras(
        &mut self,
        load_id: u64,
        profile: dat0_engine::TableProfile,
        cx: &mut gpui::Context<Self>,
    ) {
        if !matches!(
            self.inspector.mode,
            crate::inspector::ProfileTargetMode::WholeTable
        ) {
            return;
        }
        let Some(table) = self.inspector.target_table.clone() else {
            return;
        };
        let engine = self.engine();

        for col in profile.columns {
            // Low-cardinality → top-N horizontal bars.
            if col.approx_distinct > 0 && col.approx_distinct <= 24 {
                let engine = engine.clone();
                let table = table.clone();
                let col_name = col.name.clone();
                let ws_weak = cx.entity().downgrade();
                tokio::spawn(async move {
                    use dat0_engine::QueryEngine as _;
                    let data = match engine.column_topn(&table, &col_name, 8).await {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!(error = %e, col = %col_name, "column_topn failed");
                            return;
                        }
                    };
                    Self::dispatch_extra(ws_weak, load_id, move |ws, cx| {
                        ws.inspector.put_topn(&col_name, data);
                        cx.notify();
                    });
                });
                continue;
            }

            // Numeric high-cardinality → sampled histogram. Cast to DOUBLE so the
            // Arrow column is always Float64 regardless of the source numeric type.
            if let Some(numeric) = col.numeric.clone() {
                if col.approx_distinct > 24 {
                    let engine = engine.clone();
                    let col_name = col.name.clone();
                    let col_q = dat0_engine::quote_ident(&col_name);
                    let tbl_q = dat0_engine::quote_ident(&table);
                    let sql = format!(
                        "SELECT CAST({c} AS DOUBLE) AS v FROM {t} \
                         WHERE {c} IS NOT NULL USING SAMPLE 2048 ROWS",
                        c = col_q,
                        t = tbl_q
                    );
                    let ws_weak = cx.entity().downgrade();
                    tokio::spawn(async move {
                        use dat0_engine::QueryEngine as _;
                        use duckdb::arrow::array::{Array, Float64Array};
                        let result = match engine.execute(&sql).await {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::warn!(error = %e, col = %col_name, "histogram sample failed");
                                return;
                            }
                        };
                        let mut values: Vec<f64> = Vec::new();
                        for batch in &result.batches {
                            if let Some(a) = batch.column(0).as_any().downcast_ref::<Float64Array>()
                            {
                                for row in 0..a.len() {
                                    if a.is_valid(row) {
                                        values.push(a.value(row));
                                    }
                                }
                            }
                        }
                        if values.is_empty() {
                            return;
                        }
                        let bins =
                            crate::charts::histogram_bins(numeric.min, numeric.max, &values, 16);
                        Self::dispatch_extra(ws_weak, load_id, move |ws, cx| {
                            ws.inspector.put_histogram(&col_name, bins);
                            cx.notify();
                        });
                    });
                }
            }
        }
    }

    /// Hop an extras-write back to the main thread via the registry dispatcher
    /// under the supersede guard (T10). `f` runs only if `load_id` is still the
    /// current inspector load.
    fn dispatch_extra(
        ws_weak: gpui::WeakEntity<Self>,
        load_id: u64,
        f: impl FnOnce(&mut Self, &mut gpui::Context<Self>) + Send + 'static,
    ) {
        let Some(dispatcher) = crate::window_registry::dispatcher() else {
            tracing::warn!("load_column_extras: no MainThreadDispatcher; extra dropped");
            return;
        };
        let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
            let Some(ws) = ws_weak.upgrade() else {
                return;
            };
            ws.update(app_cx, |ws, cx| {
                if ws.inspector.is_current(load_id) {
                    f(ws, cx);
                }
            });
        });
    }

    /// Flip the inspector between Whole-table and Current-view profiling and
    /// re-profile (P6a T9). The cache is keyed by (table,epoch) — *not* by mode —
    /// so a toggle always `begin_load`s and re-fetches; the latest mode's profile
    /// wins (switching back re-fetches). Takes no `Window` (none needed).
    pub(crate) fn toggle_inspector_mode(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        use crate::inspector::ProfileTargetMode::*;
        self.inspector.mode = match self.inspector.mode {
            WholeTable => CurrentView,
            CurrentView => WholeTable,
        };
        // Drop stale extras so a WholeTable column's bars don't survive onto a
        // CurrentView card with the same name (T10).
        self.inspector.clear_extras();
        self.load_inspector_profile(cx);
        cx.notify();
    }
}
