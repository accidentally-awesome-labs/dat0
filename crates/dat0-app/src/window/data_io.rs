//! Data in and out: the export dialog and its event route, `run_export`,
//! save-view-as-table, opening a sample / recent entry / picked file, and
//! the landing path for a file dropped on the window.
//!
//! `route_drop_outcomes` carries a coverage gap recorded in B10: gpui 0.2.2
//! cannot simulate a platform drag, so no window-level drop test exists.
//! `on_drop` itself is covered by `file_drop.rs`'s unit tests.

use super::*;

impl WorkspaceShell {
    /// Mount the File → Export… dialog (P4c T11).
    ///
    /// Follows the `on_funnel_click` popover pattern: build the entity via
    /// `cx.new`, subscribe to its [`ExportEvent`], and STORE the subscription in
    /// `export_dialog_sub` (a dropped `Subscription` deregisters the callback
    /// silently — the P4a T10b trap). No-op (graceful) when no `ViewModel` is
    /// mounted, so Export… off an empty workspace does nothing rather than
    /// presenting a dialog that can't build a SELECT.
    ///
    /// [`ExportEvent`]: crate::view::export_dialog::ExportEvent
    pub fn open_export_dialog(&mut self, cx: &mut Context<Self>) {
        use crate::view::export_dialog::{ExportDialog, ExportEvent};

        if self.view_model.is_none() {
            tracing::debug!("open_export_dialog: no ViewModel (no file registered yet)");
            return;
        }

        let dialog = cx.new(ExportDialog::new);
        // STORE the subscription — callbacks fire silently if the returned
        // Subscription is dropped (P4a T10b post-review lesson; mirrors
        // `on_funnel_click`'s `popover_sub`).
        let sub = cx.subscribe(&dialog, |ws: &mut Self, _dialog, ev: &ExportEvent, cx| {
            ws.route_export_event(ev.clone(), cx);
        });
        self.export_dialog_sub = Some(sub);
        self.export_dialog = Some(dialog);
        // B2: this path has no `Window`, so `render` does the focusing. See
        // `pending_modal_focus`.
        self.pending_modal_focus = true;
        cx.notify();
    }

    /// Route an [`ExportEvent`] from the dialog: `Export` runs the save panel +
    /// COPY (and dismisses); `Cancel` just dismisses.
    ///
    /// [`ExportEvent`]: crate::view::export_dialog::ExportEvent
    pub(super) fn route_export_event(
        &mut self,
        ev: crate::view::export_dialog::ExportEvent,
        cx: &mut Context<Self>,
    ) {
        use crate::view::export_dialog::ExportEvent;
        match ev {
            ExportEvent::Export { scope, format } => {
                self.run_export(scope, format, cx);
            }
            ExportEvent::Cancel => {
                self.export_dialog = None;
                self.export_dialog_sub = None;
                // No `Window` on this path — `render` drains the restore.
                self.pending_modal_restore = true;
                cx.notify();
            }
        }
    }

    /// Open the native save panel, then stream the export via COPY (P4c T11).
    ///
    /// Builds the surrogate-stripped projection SELECT off `scope` + the live
    /// view state (current-view applies rename/reorder/exclude via `column_view`;
    /// full-table is the raw base columns minus the surrogate). The save panel
    /// (`App::prompt_for_new_path`) returns a `oneshot::Receiver`, awaited on the
    /// GPUI foreground executor inside `cx.spawn`; the async engine COPY
    /// (`export_query_to_path`) is awaited directly because the tokio runtime is
    /// entered for the whole `Application::run` closure (window.rs `runtime.enter()`),
    /// mirroring the file-drop async-engine pattern. The result surfaces through
    /// the `error_ux` banner queue (the same surface as the paste-reject banner).
    pub fn run_export(
        &mut self,
        scope: crate::view::export_dialog::ExportScope,
        format: dat0_engine::types::ExportFormat,
        cx: &mut Context<Self>,
    ) {
        use crate::view::export_dialog::build_export;

        let Some(base_table) = self.base_table() else {
            self.export_dialog = None;
            self.export_dialog_sub = None;
            self.pending_modal_restore = true;
            cx.notify();
            return;
        };
        // Active view name, already-quoted (the inner SELECT reads it directly).
        let active_view = self
            .view_model
            .as_ref()
            .and_then(|vm| vm.active_view())
            .map(|v| format!("\"{}\"", v.replace('"', "\"\"")));
        let base_columns = self
            .data_source
            .as_ref()
            .map(|ds| ds.visible_column_names())
            .unwrap_or_default();
        let (inner, cols) = build_export(
            scope,
            &base_table,
            active_view.as_deref(),
            &self.column_view,
            &base_columns,
        );
        let select = dat0_engine::render::render_export_select(&inner, &cols);
        let ext = match format {
            dat0_engine::types::ExportFormat::Csv => "csv",
            dat0_engine::types::ExportFormat::Json => "json",
            dat0_engine::types::ExportFormat::Parquet => "parquet",
        };
        let suggested = format!("export.{ext}");
        let engine = self.engine();

        // GPUI native save panel (`App::prompt_for_new_path` derefs through
        // `Context`). Returns a `oneshot::Receiver<Result<Option<PathBuf>>>`:
        // `Ok(Some(path))` on confirm, `Ok(None)` on cancel.
        let path_rx = cx.prompt_for_new_path(std::path::Path::new(""), Some(&suggested));
        cx.spawn(async move |_this: gpui::WeakEntity<Self>, _async_cx| {
            // `export_query_to_path` is a `QueryEngine` trait method.
            use dat0_engine::QueryEngine as _;
            // `await` yields `Result<Result<Option<PathBuf>>, oneshot::Canceled>`;
            // collapse both layers to `Option<PathBuf>` (cancel / closed = None).
            let dest = match path_rx.await {
                Ok(Ok(Some(dest))) => dest,
                _ => return,
            };
            // The engine COPY is async + Send; the tokio runtime is entered for
            // the GPUI loop (window.rs `runtime.enter()`), so awaiting it here on
            // the foreground executor drives the streaming COPY to completion.
            match engine.export_query_to_path(&select, format, &dest).await {
                Ok(()) => {
                    let mut banner =
                        crate::error_ux::Banner::info(dat0_i18n::t("export.done.title"));
                    banner.body = format!("{}", dest.display());
                    crate::error_ux::push(banner);
                }
                Err(e) => {
                    crate::error_ux::push(crate::error_ux::Banner::error(
                        dat0_i18n::t("export.failed.title"),
                        e.to_string(),
                    ));
                }
            }
        })
        .detach();

        // Dismiss the dialog immediately — the save panel + COPY run async.
        self.export_dialog = None;
        self.export_dialog_sub = None;
        // No `Window` on this path either — `render` drains the restore.
        self.pending_modal_restore = true;
        cx.notify();
    }

    /// Open the shared name-prompt overlay to promote the active grid view's
    /// transform stack to a derived table (P5b T11). Guards on an active
    /// `ViewModel` with a non-empty op stack (no-op otherwise — the PipelineBar
    /// pill already only renders in that case, but this is defensive). The
    /// `ViewModel` is re-read on confirm by [`save_view_as_table`], so nothing
    /// is captured here beyond opening the modal with the
    /// [`SaveViewAsTable`](NamePromptIntent::SaveViewAsTable) intent.
    pub(crate) fn open_save_view_as_table(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vm) = self.view_model.as_ref() else {
            return;
        };
        if vm.active().is_empty() {
            return;
        }
        self.open_name_prompt_with(
            "Save view as table…",
            "",
            NamePromptIntent::SaveViewAsTable,
            window,
            cx,
        );
    }

    /// Promote the active grid view's transform stack to a derived table (P5b
    /// T11), invoked from the [`SaveViewAsTable`](NamePromptIntent::SaveViewAsTable)
    /// Confirm arm of [`on_name_prompt_event`](Self::on_name_prompt_event).
    ///
    /// Compiles the active op stack against the base table via
    /// [`compile_view_sql`](dat0_engine::compile_view_sql) for the CTAS SQL, and
    /// records the parent + ops as `DerivedOrigin::Transform` — the
    /// lineage-meaningful path (the engine now honors the passed origin, see the
    /// T11 engine fix). On success the autocomplete snapshot is refreshed so the
    /// new table appears in completions; on failure the error is logged.
    ///
    /// Send discipline (matches the T2/T8/T10 bridge): only `Send + 'static`
    /// values cross into `tokio::spawn` — the engine `Arc`, the owned
    /// `name`/`base`/`sql` strings + `ops` vec, and the `Weak` shell handle. The
    /// GPUI entity is touched ONLY inside the dispatcher closure after
    /// `.upgrade()`.
    pub(crate) fn save_view_as_table(&mut self, name: String, cx: &mut Context<Self>) {
        if crate::grid::edit_ops::mutation_blocked(self.read_only) {
            return;
        }
        let Some(vm) = self.view_model.as_ref() else {
            return;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let base = vm.base_table().to_string();
        let ops = vm.active().to_vec();
        if ops.is_empty() {
            return;
        }
        let sql = match dat0_engine::compile_view_sql(&base, &ops) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "save_view_as_table: compile failed");
                crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                    dat0_i18n::t("save_as_table.failed.title"),
                    format!("{e}"),
                ));
                return;
            }
        };
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let origin = dat0_engine::DerivedOrigin::Transform { parent: base, ops };
            let outcome = engine.create_table(&name, &sql, origin).await;
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app: &mut gpui::App| {
                    if let Some(ws) = ws_weak.upgrade() {
                        ws.update(app, |ws, cx| match &outcome {
                            Ok(_) => {
                                ws.refresh_completion_snapshot(cx);
                                ws.refresh_catalog(cx);
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "save_view_as_table failed");
                                crate::error_ux::push(crate::error_ux::Banner::warning_with_body(
                                    dat0_i18n::t("save_as_table.failed.title"),
                                    format!("{e}"),
                                ));
                            }
                        });
                    }
                });
            } else {
                tracing::warn!(
                    "save_view_as_table: no MainThreadDispatcher installed; result dropped"
                );
            }
        });
    }

    /// Materialize and open a sample dataset (P11a T3).
    ///
    /// For bundled variants (`BundledCsv` / `BundledSqlite`): extracts bytes
    /// to `$state_root/samples/<dest>` (idempotent) then feeds the path to the
    /// `handle_drop` → data-source pipeline.  For `Remote` (NYC taxi): reuses
    /// `fetch_remote` + `fetch_failed_banner`.  Mirrors `drop_listener`'s
    /// `cx.spawn` + view-refresh pattern.  Wired by T4 hero buttons.
    pub(crate) fn open_sample_kind(
        &mut self,
        kind: crate::sample_data::SampleKind,
        cx: &mut Context<Self>,
    ) {
        use crate::sample_data::SampleKind;
        let Some(state_root) = crate::window_registry::state_root() else {
            crate::error_ux::push(crate::error_ux::Banner::error(
                "Cannot open sample",
                "App state directory not initialised",
            ));
            return;
        };
        let session = Arc::clone(&self.session);
        match kind {
            SampleKind::BundledCsv {
                bytes,
                dest_filename,
            }
            | SampleKind::BundledSqlite {
                bytes,
                dest_filename,
            } => {
                let path = match crate::sample_data::ensure_bundled_extracted(
                    state_root,
                    bytes,
                    dest_filename,
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        crate::error_ux::push(crate::error_ux::Banner::error(
                            "Sample extract failed",
                            e.to_string(),
                        ));
                        return;
                    }
                };
                cx.spawn(async move |weak_shell, async_cx| {
                    let outcomes = handle_drop(vec![path], session).await;
                    Self::route_drop_outcomes(outcomes, weak_shell, async_cx).await;
                })
                .detach();
            }
            SampleKind::Remote {
                url,
                sha256,
                dest_filename,
                ..
            } => {
                let state_root = state_root.to_owned();
                cx.spawn(
                    async move |weak_shell, async_cx| match crate::sample_data::fetch_remote(
                        url,
                        sha256,
                        &state_root,
                        dest_filename,
                    )
                    .await
                    {
                        Ok(path) => {
                            let outcomes = handle_drop(vec![path], session).await;
                            Self::route_drop_outcomes(outcomes, weak_shell, async_cx).await;
                        }
                        Err(ref e) => {
                            crate::error_ux::push(crate::sample_data::fetch_failed_banner(url, e));
                        }
                    },
                )
                .detach();
            }
        }
    }

    /// Open a recent workspace or package entry (P11a T3).
    ///
    /// - `Workspace` entries use `open_workspace_at` (opens / focuses the
    ///   workspace window).
    /// - `Package` entries use `open_package_at` (read-only Inspect window).
    ///
    /// Wired by T4 hero recents list.  `Context<Self>` derefs to `App` so the
    /// free-function calls below compile without an explicit cast.
    pub(crate) fn open_recent_entry(
        &mut self,
        entry: crate::recents::RecentEntry,
        cx: &mut Context<Self>,
    ) {
        use crate::recents::RecentEntry;
        let path = entry.path().to_owned();
        match entry {
            RecentEntry::Workspace { .. } => open_workspace_at(cx, path),
            RecentEntry::Package { .. } => open_package_at(cx, path),
        }
    }

    /// Show the native file picker and open the chosen file (P11a T3).
    ///
    /// Equivalent to dropping a file onto the shell: `prompt_for_paths`
    /// → `handle_drop` → data-source refresh.  Mirrors `drop_listener`.
    ///
    /// Wired by T4 hero "Open file…" button.
    pub(crate) fn open_file_picker(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        let session = Arc::clone(&self.session);
        cx.spawn(async move |weak_shell, async_cx| {
            let path = match rx.await {
                Ok(Ok(Some(mut v))) if !v.is_empty() => v.remove(0),
                _ => return,
            };
            let outcomes = handle_drop(vec![path], session).await;
            Self::route_drop_outcomes(outcomes, weak_shell, async_cx).await;
        })
        .detach();
    }

    /// Shared post-[`handle_drop`] outcome routing used by the three hero open
    /// helpers (P11a T3).
    ///
    /// Mirrors the inner `cx.spawn` body of `drop_listener` exactly:
    /// partitions outcomes into wizard requests and registered tables, opens
    /// any import wizard dialogs, then promotes the last registered table into
    /// the active data source with a full view refresh (`set_data_source` +
    /// `refresh_catalog` + `cx.notify()`).
    pub(super) async fn route_drop_outcomes(
        outcomes: Vec<DropOutcome>,
        weak_shell: gpui::WeakEntity<WorkspaceShell>,
        async_cx: &mut gpui::AsyncApp,
    ) {
        let mut wizard_requests: Vec<(std::path::PathBuf, crate::import_wizard::SniffSummary)> =
            Vec::new();
        let mut last_registered: Option<String> = None;
        for o in outcomes {
            match o {
                DropOutcome::Registered { table_name, .. } => {
                    last_registered = Some(table_name);
                }
                DropOutcome::OpenWizard { path, sniff } => {
                    wizard_requests.push((path, sniff));
                }
                _ => {}
            }
        }
        for (path, sniff) in wizard_requests {
            let _ = async_cx.update(|app_cx| {
                crate::import_wizard::open(app_cx, &path, sniff);
            });
        }
        if let Some(table_name) = last_registered {
            let engine = async_cx
                .update(|app_cx| {
                    weak_shell
                        .update(app_cx, |view, _cx| view.session.lock().engine.clone())
                        .ok()
                })
                .ok()
                .flatten();
            if let Some(engine) = engine {
                match GridDataSource::new(engine, table_name.clone()).await {
                    Ok(ds) => {
                        let _ = async_cx.update(|app_cx| {
                            let _ = weak_shell.update(app_cx, |view, cx| {
                                let quoted = format!("\"{}\"", table_name.replace('"', "\"\""));
                                view.view_model = Some(ViewModel::new(table_name.clone(), quoted));
                                view.set_data_source(Arc::new(ds));
                                view.refresh_catalog(cx);
                                cx.notify();
                            });
                        });
                    }
                    Err(e) => {
                        tracing::warn!("hero open: GridDataSource::new failed: {e}");
                    }
                }
            }
        }
    }
}
