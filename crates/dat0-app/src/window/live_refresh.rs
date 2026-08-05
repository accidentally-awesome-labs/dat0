//! Live data (P7c): watching the source file behind an imported table,
//! re-importing it, and replaying the view's projection onto the new data.
//!
//! `partition_replay_on_drift` decides which projection ops survive a schema
//! change; its three unit tests are the only pure part of this surface and
//! sit at the bottom of this file.

use super::*;

/// Schema-drift pre-validate for live re-import replay (P7c D3).
///
/// The `replayable` ops are column-keyed. Only `Filter` and `Sort` reference
/// columns in the *executed* SQL (the projection ops `Reorder`/`Rename`/
/// `DeleteColumn` are display-only — they never reach `compile_view_sql`, so they
/// can't cause an engine error). If any executed-SQL op references a column that
/// the re-imported file no longer has, the replayed `SELECT` would fail at engine
/// execute time. `compile_view_sql` is a pure string renderer with no schema
/// knowledge, so it cannot detect this — we check column references against the
/// fresh column set here instead.
///
/// Returns `(ops, drifted)`: on drift, `ops` is empty (land on the bare base) and
/// `drifted` is `true` (caller raises the schema-drift banner). All-or-nothing —
/// a partial replay that silently dropped one filter would be more surprising
/// than landing cleanly on the bare base.
fn partition_replay_on_drift(
    replayable: Vec<dat0_engine::transform::Transformation>,
    columns: &[String],
) -> (Vec<dat0_engine::transform::Transformation>, bool) {
    use dat0_engine::transform::Transformation;
    let has = |c: &str| columns.iter().any(|known| known == c);
    let drifted = replayable.iter().any(|op| match op {
        Transformation::Filter { column, .. } => !has(column),
        Transformation::Sort { keys } => keys.iter().any(|k| !has(&k.column)),
        // Display-only projection ops never reach the executed SQL, so a stale
        // column reference in them is harmless (the grid `ColumnView` fold simply
        // ignores an unknown source). Don't treat them as drift.
        Transformation::Reorder { .. }
        | Transformation::Rename { .. }
        | Transformation::DeleteColumn { .. } => false,
        // Edit/RowDelete are never in `replayable` (split_replayable drops them).
        Transformation::Edit { .. } | Transformation::RowDelete { .. } => false,
    });
    if drifted {
        (Vec::new(), true)
    } else {
        (replayable, false)
    }
}

/// `live.refresh` action handler (P7c). Resolves the focused workspace and runs
/// its refresh flow.
///
/// Resolves the focused shell (same precedent as `dispatch_undo` /
/// `promote_focused_into`) and calls [`WorkspaceShell::run_refresh`], which runs
/// the real split → confirm → re-CTAS → replay flow (P7c T6).
pub fn dispatch_live_refresh(app: &mut gpui::App) {
    let Some(weak) = crate::window_registry::focused_workspace_weak() else {
        tracing::debug!("live.refresh: no focused workspace");
        return;
    };
    let Some(any_entity) = weak.upgrade() else {
        return;
    };
    let Ok(shell) = any_entity.downcast::<WorkspaceShell>() else {
        return;
    };
    shell.update(app, |shell, cx| shell.run_refresh(cx));
}

impl WorkspaceShell {
    /// A mutation touched `table`'s data/schema. Invalidate the inspector's cached
    /// profile for it (epoch bump) and, if that table is the live inspector target,
    /// re-profile it now so the open dock updates. (Hybrid write path, P6a T12.)
    pub(crate) fn on_table_mutated_structural(
        &mut self,
        table: &str,
        cx: &mut gpui::Context<Self>,
    ) {
        self.inspector.bump_epoch(table);
        if self.inspector.target_table.as_deref() == Some(table) {
            self.load_inspector_profile(cx); // re-SUMMARIZE the now-invalidated table
        }
        cx.notify();
    }

    /// The on-disk source path of the currently-mounted table, if it was
    /// imported from a file (P7c). Matches the active `view_model`'s base table
    /// against `catalog_tables` on the bare (unquoted) name — the catalog/lineage
    /// keying used elsewhere (see [`Self::inspector_projection`]). Returns `None`
    /// when no table is mounted, the table has no `File` origin, or the catalog
    /// has not yet been refreshed for it.
    pub(crate) fn active_source_path(&self) -> Option<std::path::PathBuf> {
        let base = self.view_model.as_ref()?.base_table();
        let bare = bare_table_name(base);
        self.catalog_tables.iter().find_map(|t| match &t.origin {
            dat0_engine::TableOrigin::File(p) if t.name == bare => Some(p.clone()),
            _ => None,
        })
    }

    /// (Re)create the source watcher for the active table (P7c). Drops any
    /// previous watch first (so switching tables retargets, not stacks). No-op
    /// (clears the watch) when the active table has no `File` source or the
    /// source file no longer exists on disk.
    pub(crate) fn retarget_source_watch(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.active_source_path() else {
            self.source_watcher = None;
            return;
        };
        if !path.exists() {
            self.source_watcher = None; // file gone → nothing to watch
            return;
        }
        let ws = cx.entity().downgrade();
        let watcher = crate::workspace::source_watcher::SourceWatcher::start(
            path.clone(),
            std::time::Duration::from_millis(500),
            move |changed| {
                // Runs on the `dat0-source-debounce` bg thread — must NOT touch
                // GPUI directly. Hop to the main thread via the dispatcher, then
                // upgrade the weak shell handle and raise the banner.
                if let Some(d) = crate::window_registry::dispatcher() {
                    let ws = ws.clone();
                    let _ = d.dispatch(move |app| {
                        if let Some(h) = ws.upgrade() {
                            h.update(app, |shell, cx| shell.on_source_changed(changed, cx));
                        }
                    });
                }
            },
        );
        match watcher {
            Ok(w) => self.source_watcher = Some(w),
            Err(e) => tracing::warn!(error = %e, "failed to start source watcher"),
        }
    }

    /// Raise a one-click Refresh banner for an externally-changed source file
    /// (P7c). De-dups: if a refresh banner for this file's title is already
    /// present, do nothing (a burst of saves coalesces to a single banner —
    /// the watcher already debounces, this guards re-raise across drains).
    pub(crate) fn on_source_changed(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let file = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let title = dat0_i18n::t("livedata.changed.title").replace("{file}", &file);
        let already = self.banners.iter().any(|b| b.title == title);
        if already {
            return;
        }
        self.banners.push(
            crate::error_ux::banner::Banner::warning(title).with_primary(
                dat0_i18n::t("livedata.changed.refresh"),
                crate::actions::builtin::ids::LIVE_REFRESH,
            ),
        );
        cx.notify();
    }

    /// Re-import the active table's source file and replay its structural
    /// transforms (P7c D3). If the active stack carries rowid-keyed edits
    /// (`Edit`/`RowDelete`), confirm-discard first via a blocking Dialog — those
    /// ops can't survive a re-CTAS (the `__dat0_rowid` surrogate regenerates).
    /// A pure (filter/sort/projection)-only stack proceeds without a prompt.
    ///
    /// On success the refresh banner for this file is cleared and the watch is
    /// re-targeted (an atomic save may have replaced the inode).
    pub(crate) fn run_refresh(&mut self, cx: &mut Context<Self>) {
        use dat0_engine::transform::split_replayable;
        // Resolve the active table's source up front so a no-source tab (e.g. a
        // SQL-derived table) is a clean no-op rather than half-running.
        if self.active_source_path().is_none() {
            return;
        }
        let Some(vm) = self.view_model.as_ref() else {
            return;
        };
        let split = split_replayable(vm.stack());
        if split.has_dropped() {
            let body = dat0_i18n::t("livedata.refresh.confirm.body")
                .replace("{edits}", &split.dropped_edits.to_string())
                .replace("{deletes}", &split.dropped_deletes.to_string());
            let ws = cx.entity().downgrade();
            crate::live_refresh_dialog::confirm_discard(cx, body, move |app| {
                if let Some(h) = ws.upgrade() {
                    h.update(app, |shell, cx| shell.perform_reimport(cx));
                }
            });
        } else {
            self.perform_reimport(cx);
        }
    }

    /// Off-thread re-CTAS of the active table's source file, then on-main replay
    /// of the structural stack (P7c). The re-import is idempotent — DuckDB
    /// `CREATE OR REPLACE TABLE` under the same derived name (see
    /// [`dat0_engine::QueryEngine::register_file_as_table`]) — so it overwrites
    /// the base in place and re-injects `__dat0_rowid`. Mirrors the file-drop
    /// import path's spawn discipline (`tokio::spawn` + `window_registry`
    /// dispatcher + upgraded weak shell handle).
    pub(crate) fn perform_reimport(&mut self, cx: &mut Context<Self>) {
        use dat0_engine::transform::split_replayable;
        let Some(path) = self.active_source_path() else {
            return;
        };
        let Some(vm) = self.view_model.as_ref() else {
            return;
        };
        let replayable = split_replayable(vm.stack()).replayable;
        let engine = self.engine();
        let ws = cx.entity().downgrade();
        let file = path.file_name().map(|s| s.to_string_lossy().to_string());

        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let opts = dat0_engine::RegisterOpts::default();
            let result = engine.register_file_as_table(&path, opts).await;
            if let Some(d) = crate::window_registry::dispatcher() {
                let _ = d.dispatch(move |app| {
                    let Some(h) = ws.upgrade() else {
                        return;
                    };
                    h.update(app, |shell, cx| match result {
                        Ok(info) => {
                            let columns: Vec<String> =
                                info.columns.iter().map(|c| c.name.clone()).collect();
                            shell.apply_refresh_replay(replayable, columns, file, cx);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "live re-import failed");
                            shell.banners.push(crate::error_ux::Banner::error(
                                dat0_i18n::t("livedata.reimport.failed.title"),
                                format!("{e}"),
                            ));
                            cx.notify();
                        }
                    });
                });
            } else {
                tracing::warn!(
                    "perform_reimport: no MainThreadDispatcher installed; re-import result dropped"
                );
            }
        });
    }

    /// On the main thread: replay the structural ops onto the freshly re-imported
    /// base via the force-rebind primitive ([`ViewModel::reset_to_replayed`]),
    /// drive the engine round-trip, and clear the refresh banner (P7c).
    ///
    /// **Schema-drift guard (D3):** the replayable ops are column-keyed. If a
    /// `Filter`/`Sort` op references a column that the re-imported file no longer
    /// has (a rename or drop upstream), the replayed `SELECT` would fail at engine
    /// execute time — `spawn_view_change` only logs that failure, leaving the grid
    /// silently un-rebound. So we pre-validate every column reference against the
    /// fresh column set BEFORE `reset_to_replayed`; on any miss we land on the
    /// bare base (no transforms) plus a schema-drift warning banner. (Note:
    /// `compile_view_sql` is a pure string renderer and does NOT know the schema,
    /// so it can't be the guard — the column-set check is the real validation.)
    pub(crate) fn apply_refresh_replay(
        &mut self,
        replayable: Vec<dat0_engine::transform::Transformation>,
        columns: Vec<String>,
        file: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let (replayable, drifted) = partition_replay_on_drift(replayable, &columns);
        if drifted {
            tracing::warn!("live replay schema drift; landing on bare base");
            self.banners
                .push(crate::error_ux::Banner::warning(dat0_i18n::t(
                    "livedata.replay.schema_drift",
                )));
        }

        let Some(vm) = self.view_model.as_mut() else {
            return;
        };
        let change = vm.reset_to_replayed(replayable);
        let base = vm.base_table().to_string();
        let engine = self.engine();
        let ws = cx.entity().downgrade();
        crate::view::spawn_view_change(
            engine,
            base,
            change,
            std::sync::Arc::new(move |new_ds, app_cx| {
                if let Some(h) = ws.upgrade() {
                    h.update(app_cx, |shell, cx| shell.apply_view_change(new_ds, cx));
                }
            }),
        );

        // Drop the refresh banner(s) this file raised — the click is resolved.
        if let Some(file) = file {
            let title = dat0_i18n::t("livedata.changed.title").replace("{file}", &file);
            self.banners.retain(|b| b.title != title);
        }
        // An atomic save (write-temp + rename) replaces the watched inode, so the
        // old watch is now dead — re-target it at the live path.
        self.retarget_source_watch(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    //! Pure decision tests for the live re-import flow (P7c T6). The clickable
    //! Dialog (`confirm_discard`) and the engine round-trip are UAT — these cover
    //! the two pure gates: (1) whether `split_replayable().has_dropped()` drives
    //! the confirm prompt, and (2) the schema-drift column-existence guard.
    use super::*;
    use dat0_engine::transform::{
        CellEdit, RowKey, Scalar, SortDirection, SortKey, Transformation, split_replayable,
    };

    fn one_cell_edit() -> Transformation {
        Transformation::Edit {
            cells: vec![CellEdit {
                row: RowKey::Surrogate { id: 7 },
                column: "amount".into(),
                value: Scalar::Int(42),
            }],
        }
    }

    fn sort_on(col: &str) -> Transformation {
        Transformation::Sort {
            keys: vec![SortKey {
                column: col.into(),
                direction: SortDirection::Asc,
            }],
        }
    }

    /// The confirm dialog must fire iff the stack carries rowid-keyed ops
    /// (`Edit`/`RowDelete`) — those can't survive a re-CTAS. A column-keyed-only
    /// stack (e.g. a lone Sort) refreshes silently.
    #[test]
    fn refresh_needs_confirm_only_when_rowid_ops_present() {
        // One real cell edit + a sort → has dropped (Edit), confirm required.
        let mixed = vec![one_cell_edit(), sort_on("amount")];
        let split = split_replayable(&mixed);
        assert!(
            split.has_dropped(),
            "an Edit in the stack must require confirm"
        );
        assert_eq!(split.dropped_edits, 1);
        assert_eq!(split.dropped_deletes, 0);
        // The Sort survives into the replayable set.
        assert_eq!(split.replayable, vec![sort_on("amount")]);

        // Sort-only stack → nothing dropped, no confirm.
        let pure = vec![sort_on("amount")];
        let split = split_replayable(&pure);
        assert!(
            !split.has_dropped(),
            "a column-keyed-only stack refreshes without a prompt"
        );
    }

    /// The schema-drift guard: a Filter/Sort referencing a column the re-imported
    /// file no longer has → drop to bare base (empty ops) + drift flag. Present
    /// columns replay unchanged.
    #[test]
    fn schema_drift_lands_on_bare_base_when_column_missing() {
        let columns = vec!["id".to_string(), "amount".to_string()];

        // Sort on a present column → kept, no drift.
        let (ops, drifted) = partition_replay_on_drift(vec![sort_on("amount")], &columns);
        assert!(!drifted);
        assert_eq!(ops, vec![sort_on("amount")]);

        // Filter on a now-missing column → drift, land on bare base.
        let filter_missing = Transformation::Filter {
            column: "removed_col".into(),
            op: dat0_engine::transform::FilterOp::IsNotEmpty,
            value: dat0_engine::transform::FilterValue::None,
        };
        let (ops, drifted) = partition_replay_on_drift(vec![filter_missing], &columns);
        assert!(drifted, "a filter on a dropped column is schema drift");
        assert!(ops.is_empty(), "drift lands on the bare base (no ops)");

        // Sort key on a missing column → drift too.
        let (ops, drifted) = partition_replay_on_drift(vec![sort_on("gone")], &columns);
        assert!(drifted);
        assert!(ops.is_empty());
    }

    /// Display-only projection ops never reach the executed SQL, so a stale
    /// column reference in them is NOT drift (the grid fold ignores unknowns).
    #[test]
    fn projection_ops_referencing_missing_columns_are_not_drift() {
        let columns = vec!["id".to_string()];
        let rename_stale = Transformation::Rename {
            column: "old_name".into(), // not in `columns`
            to: "shiny".into(),
        };
        let (ops, drifted) = partition_replay_on_drift(vec![rename_stale.clone()], &columns);
        assert!(!drifted, "display-only ops can't cause an engine error");
        assert_eq!(ops, vec![rename_stale]);
    }
}
