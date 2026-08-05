//! SQL console (P5a, docked in B8): mounting and toggling the console,
//! running and cancelling a query, the completion snapshot, the query
//! library, and save-console-as-table.
//!
//! `SqlRunOutcome` and its two classification helpers live here because they
//! exist to turn an `EngineError` into something the console can display.

use super::*;

/// Reduce a `ViewModel::base_table()` name to its bare (unquoted, unqualified)
/// form for catalog matching (P7c). `base_table()` is quoted and may be
/// schema-qualified (`"main"."orders"`); the catalog keys on the bare name
/// (`orders`). Mirrors the reduction in [`WorkspaceShell::inspector_projection`].
pub(super) fn bare_table_name(base: &str) -> String {
    base.rsplit('.')
        .next()
        .unwrap_or(base)
        .trim_matches('"')
        .to_string()
}

/// The terminal state of one SQL console run, computed OFF the GPUI main thread
/// inside `spawn_sql_run` and applied on the main thread by `finish_sql_run`.
pub(crate) enum SqlRunOutcome {
    /// A result-producing statement bound to a fresh `GridDataSource`.
    Bound(std::sync::Arc<crate::grid::GridDataSource>),
    /// A DDL/DML statement completed; carries the status line.
    Status(String),
    /// The run failed; carries the DuckDB error message.
    Error(String),
    /// The run was interrupted (cooperative cancel).
    Cancelled,
}

/// Map a `dat0_engine::EngineError` onto a run outcome. The dedicated
/// `EngineError::Interrupted` variant (engine `execute/mod.rs` surfaces it when
/// `Engine::interrupt()` fires) maps to `Cancelled`; everything else is an
/// inline error.
fn classify_run_err(e: dat0_engine::EngineError) -> SqlRunOutcome {
    if matches!(e, dat0_engine::EngineError::Interrupted) {
        SqlRunOutcome::Cancelled
    } else {
        SqlRunOutcome::Error(e.to_string())
    }
}

/// Build the status line for a completed EXEC statement. DuckDB does not
/// uniformly expose an affected-row count through `QueryResult` here, so a
/// generic localized "OK" is used for P5a.
fn format_exec_status(_r: &dat0_engine::QueryResult) -> String {
    dat0_i18n::t("sql.ok")
}

/// Wall-clock millis since the Unix epoch (app runtime; not a workflow script).
pub(super) fn now_unix_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl WorkspaceShell {
    /// Toggle the SQL Console bottom panel (P5a T5).
    ///
    /// On the first toggle, lazily constructs the [`SqlConsole`] from the
    /// session's persisted SQL tabs (which needs the `&mut Window` for the
    /// per-tab code editors) and subscribes to its [`SqlConsoleEvent`]. The
    /// subscription is STORED in `sql_console_sub` — a dropped `Subscription`
    /// deregisters the callback silently (the P4a T10b trap). Subsequent
    /// toggles just open and close the dock without tearing the console down,
    /// preserving the editor buffers.
    ///
    /// B8: the console lives in the `DockArea`'s BOTTOM dock rather than a
    /// fixed strip above the grid, and the dock's own open flag is the single
    /// source of truth for visibility — see
    /// [`sql_console_visible`](Self::sql_console_visible).
    ///
    /// [`SqlConsole`]: crate::view::sql_console::SqlConsole
    /// [`SqlConsoleEvent`]: crate::view::sql_console::SqlConsoleEvent
    /// B8/B9: build the SQL console, subscribe to it, register the close-flush
    /// hook and mount the bottom dock. Idempotent — returns immediately if the
    /// console already exists.
    ///
    /// Extracted from [`toggle_sql_console`](Self::toggle_sql_console) at B9,
    /// because the restore path needs the same mount from a second call site and
    /// copying it would leave two mounts to keep in step.
    ///
    /// `dock` is passed in rather than re-derived: one caller is
    /// `ensure_dock_area` itself, mid-build, which must not re-enter. `height`
    /// is a parameter for the same reason — a restored console mounts at its
    /// persisted height, not the constant.
    pub(super) fn mount_sql_console(
        &mut self,
        dock: &gpui::Entity<gpui_component::dock::DockArea>,
        height: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sql_console.is_some() {
            return;
        }
        let (persisted, active) = {
            let s = self.session.lock();
            (s.sql_tabs().to_vec(), s.active_sql_tab())
        };
        // Ensure the per-window autocomplete snapshot exists, then clone it
        // into the console so every tab's provider shares one `RefCell`
        // (P5b T2). The refresh below populates `tables` off the engine.
        let snapshot = self
            .sql_snapshot
            .get_or_insert_with(crate::query::completion::new_shared_snapshot)
            .clone();
        let console = cx.new(|cx| {
            crate::view::sql_console::SqlConsole::new(
                &persisted,
                active,
                snapshot.clone(),
                window,
                cx,
            )
        });
        // `subscribe_in` (not `subscribe`) so the event callback receives a
        // live `&mut Window` — the Save-query path (`SaveQuery`) builds a
        // `NamePrompt` whose single-line `InputState` needs one eagerly
        // (P5b T8). The window is valid because the subscription fires inside
        // a window update.
        let sub = cx.subscribe_in(
            &console,
            window,
            |ws: &mut Self, console, ev: &crate::view::sql_console::SqlConsoleEvent, window, cx| {
                ws.on_sql_console_event(console.clone(), ev.clone(), window, cx);
            },
        );
        self.sql_console_sub = Some(sub);
        // Hydrate ai_ready on the freshly-built console.
        let ready = self.ai_ready();
        console.update(cx, |c, _cx| c.ai_ready = ready);
        self.sql_console = Some(console.clone());

        // B8: mount the bottom dock, open.
        //
        // ⚠ `set_bottom_dock` is called EXACTLY ONCE, here, and must stay
        // that way. It runs `subscribe_item`, which pushes onto the
        // `DockArea`'s `_subscriptions` and recurses over the item tree
        // (`dock/mod.rs:955-963`); nothing ever removes them. Every later
        // open and close goes through `toggle_dock` below, which
        // re-subscribes nothing. Same constraint as `set_left_dock` and
        // `set_right_dock` — see `ensure_dock_area`.
        //
        // Mounted LAZILY rather than beside the left and right docks
        // because upstream keeps a CLOSED bottom dock on screen at
        // `h(px(29.))` so its title bar can be clicked to reopen
        // (`dock.rs:372-380`). Building it here means a user who never
        // opens the console never sees that bar — the first-run hero is
        // untouched.
        //
        // A bare `DockItem::tab`, with no enclosing split: the bottom dock
        // holds exactly one panel, and that is also the only shape immune
        // to B7's `set_active_ix` re-entrancy panic (see
        // `ensure_dock_area`, which this is called from).
        let weak_dock = dock.downgrade();
        let item = gpui_component::dock::DockItem::tab(console.clone(), &weak_dock, window, cx);
        dock.update(cx, |dock, cx| {
            dock.set_bottom_dock(item, Some(gpui::px(height)), true, window, cx);
        });

        // Persist the console one last time on window close (P5a T10). This
        // is a best-effort backstop ON TOP OF the guaranteed per-mutation
        // persists (Run / tab add / close / active-switch each emit
        // `Persist` → `set_sql_tabs` → disk), so disk is already current;
        // the close hook flushes any edit-buffer text typed since the last
        // mutation. Registered once, the first time the console is built
        // (we hold the only `&mut Window` here). `should_close` returns
        // `true` so the default close proceeds.
        if !self.sql_console_close_hooked {
            self.sql_console_close_hooked = true;
            let ws_weak = cx.entity().downgrade();
            window.on_window_should_close(cx, move |_window, app| {
                if let Some(ws) = ws_weak.upgrade() {
                    ws.update(app, |ws, cx| ws.persist_sql_console(cx));
                }
                true
            });
        }
    }

    pub(crate) fn toggle_sql_console(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dock = self.ensure_dock_area(window, cx);
        if self.sql_console.is_none() {
            self.mount_sql_console(&dock, SQL_CONSOLE_DOCK_HEIGHT, window, cx);
        } else {
            dock.update(cx, |dock, cx| {
                dock.toggle_dock(gpui_component::dock::DockPlacement::Bottom, window, cx);
            });
        }
        // Refresh the autocomplete schema whenever the console is (re)shown so
        // tables created/dropped while it was hidden are reflected (P5b T2).
        //
        // Reading the derived getter immediately after the toggle is sound:
        // `Dock::set_open` assigns `self.open` synchronously and defers only
        // `set_collapsed` (`dock.rs:259-266`), measured at B8's T0.
        if self.sql_console_visible(cx) {
            self.refresh_completion_snapshot(cx);
        }
        // Keep the Catalog dock fresh if it's open (P6a T7).
        if self.catalog_panel_visible {
            self.refresh_catalog(cx);
        }
        // v11: persist the console's open state and height. Runs after both
        // branches so it observes the toggle, and after the `sql_console_visible`
        // read above for the same reason that read is sound — `Dock::set_open`
        // assigns `open` synchronously.
        self.persist_dock_layout(cx);
        cx.notify();
    }

    /// Rebuild the autocomplete schema snapshot off the live engine (P5b T2).
    /// Runs `get_tables()` OFF the GPUI main thread, then posts the result back
    /// via the canonical `MainThreadDispatcher` and writes the shared `RefCell`
    /// ON the main thread. Called on console-open and after every run (covers
    /// CREATE/DROP/Save-as-Table).
    ///
    /// Send discipline: `SharedSnapshot` is `Rc<RefCell<..>>` — neither `Send`
    /// nor allowed in the dispatcher's `Send + 'static` closure. So the snapshot
    /// is NEVER captured across the thread boundary. Instead a weak
    /// `WorkspaceShell` handle (Send) crosses into the task; the dispatcher
    /// closure upgrades it on the main thread and reaches `self.sql_snapshot`
    /// there, mirroring the `finish_sql_run` / `prefetch_rows_for` bridge.
    pub(crate) fn refresh_completion_snapshot(&mut self, cx: &mut Context<Self>) {
        if self.sql_snapshot.is_none() {
            return; // console never opened; nothing to refresh
        }
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let tables = match engine.get_tables().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(error = %e, "refresh_completion_snapshot: get_tables failed");
                    return;
                }
            };
            // `tables` is `Vec<TableInfo>` (Send). Build the `TableEntry`s and
            // write the shared `RefCell` on the main thread, where the `Rc` is
            // reachable via the upgraded shell handle.
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    let Some(ws) = ws_weak.upgrade() else { return };
                    ws.update(app_cx, |ws, _cx| {
                        let Some(snapshot) = ws.sql_snapshot.as_ref() else {
                            return;
                        };
                        let entries = tables
                            .iter()
                            .map(|t| crate::query::completion::TableEntry {
                                name: t.name.clone().into(),
                                columns: t.columns.iter().map(|c| c.name.clone().into()).collect(),
                            })
                            .collect();
                        snapshot.borrow_mut().tables = entries;
                    });
                });
            } else {
                tracing::warn!(
                    "refresh_completion_snapshot: no MainThreadDispatcher installed; snapshot stale"
                );
            }
        });
    }

    /// Route a [`SqlConsoleEvent`] from the console.
    ///
    /// T5 stubbed `Run`/`Cancel`; T6 implements `Run` (statement resolve →
    /// VIEW/EXEC → bind grid). `Cancel` lands in T7.
    ///
    /// [`SqlConsoleEvent`]: crate::view::sql_console::SqlConsoleEvent
    pub(crate) fn on_sql_console_event(
        &mut self,
        console: Entity<crate::view::sql_console::SqlConsole>,
        ev: crate::view::sql_console::SqlConsoleEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::view::sql_console::SqlConsoleEvent::*;
        match ev {
            Persist => self.persist_sql_console(cx),
            Run { target } => self.spawn_sql_run(console, target, cx),
            Cancel => self.cancel_sql_run(cx),
            // P5b T5: fetch the session's persisted history (newest last in the
            // store; the list view reverses to newest-first) and hand it to the
            // console, which renders the overlay where a row click owns a live
            // `Window` to load into a new tab.
            ShowHistory => {
                let entries = self.session.lock().query_history().to_vec();
                console.update(cx, |c, cx| c.show_history(entries, cx));
            }
            // P5b T8: capture the active tab's SQL NOW and open the Save-query
            // name-prompt overlay (export-dialog idiom). Confirm → save; Cancel →
            // dismiss (both in `on_name_prompt_event`).
            SaveQuery => {
                let sql = console.read(cx).active_sql_and_cursor(cx).0;
                self.open_name_prompt(sql, window, cx);
            }
            // P5b T8: mount the window-level saved-query picker overlay. Picking
            // a row queues its SQL into a new tab (via the console's `queue_load`,
            // drained by `SqlConsole::render` with a real `Window`); deleting a
            // row removes it from the session and refreshes the overlay.
            ShowSaved => {
                self.show_saved_picker(window, cx);
            }
            // P5b T10: open the shared name modal with the SaveConsoleAsTable
            // intent. Confirm re-reads the statement-under-cursor and CTAS-
            // promotes it via `create_table(.., DerivedOrigin::Sql)`; the
            // SaveQuery path's captured-SQL snapshot is not needed here.
            SaveAsTable => {
                self.open_name_prompt_with(
                    "Save as table…",
                    "",
                    NamePromptIntent::SaveConsoleAsTable,
                    window,
                    cx,
                );
            }
            OpenNl2SqlPrompt => {
                self.open_name_prompt_with(
                    dat0_i18n::t("sql.nl2sql.prompt_title"),
                    "",
                    NamePromptIntent::Nl2SqlPrompt,
                    window,
                    cx,
                );
            }
            StopAiStream => {
                // Supersede: drop the in-flight stream; partial text stays Insert-able.
                // Also finish the Explain panel if that was the active stream (T7).
                self.ai_stream_load_id = self.ai_stream_load_id.wrapping_add(1);
                if let Some(console) = &self.sql_console {
                    console.update(cx, |c, cx| {
                        c.finish_nl_preview(None, cx);
                        c.finish_explain(None, cx);
                    });
                }
            }
            Explain => {
                self.spawn_ai_explain(cx);
            }
            CloseExplain => {
                self.ai_stream_load_id = self.ai_stream_load_id.wrapping_add(1);
                if let Some(console) = &self.sql_console {
                    console.update(cx, |c, cx| c.clear_explain(cx));
                }
            }
        }
    }

    /// Fire the active run's cancel drop-guard (P5a T7). `QueryCancel::cancel()`
    /// invokes the engine's connection-wide `interrupt()`, so the in-flight
    /// `spawn_sql_run` task's engine call resolves to `EngineError::Interrupted`,
    /// which `classify_run_err` maps to `SqlRunOutcome::Cancelled`; `finish_sql_run`
    /// then renders the muted "Cancelled" region and clears `running`.
    ///
    /// Safe when there is no active run (`active_query_cancel` is `None`). Safe
    /// under double-cancel: `QueryCancel::cancel()` is idempotent (disarms after
    /// firing), and `finish_sql_run`'s later `take()+disarm()` on the
    /// already-disarmed guard is a no-op.
    pub(crate) fn cancel_sql_run(&mut self, _cx: &mut Context<Self>) {
        if let Some(g) = self.active_query_cancel.as_mut() {
            g.cancel(); // fires engine.interrupt(); the in-flight task resolves to Cancelled
        }
    }

    /// Snapshot the console's tabs into the session and persist (P5a T5).
    /// Now LIVE — called from `finish_sql_run` after every run (T6).
    ///
    /// Persistence cadence (P5a T10): every console mutation that emits
    /// `SqlConsoleEvent::Persist` routes here — Run, tab add (`new_tab`), tab
    /// close (`close_tab`), and active-tab switch — plus a window-close backstop
    /// registered in `toggle_sql_console`. Editor-buffer text typed between
    /// mutations is captured by the next mutation or the close backstop. Blur is
    /// intentionally NOT wired: `InputState` owns its focus handle internally
    /// (no clean seam to subscribe its blur at this gpui-component rev), and the
    /// guaranteed per-mutation + close triggers already keep disk current.
    pub(crate) fn persist_sql_console(&mut self, cx: &mut Context<Self>) {
        if let Some(console) = &self.sql_console {
            let app: &gpui::App = cx;
            let (tabs, active) = console.read(app).snapshot(app);
            let _ = self.session.lock().set_sql_tabs(tabs, active);
        }
    }

    /// Execute the SQL statement under the cursor OFF the GPUI main thread and
    /// bind the result to the grid (P5a T6). Structurally mirrors
    /// [`crate::view::spawn_view_change`] / `run_view_change_inner`: the engine
    /// round-trip + `GridDataSource::new` run inside a `tokio::spawn`, then the
    /// main-thread apply is posted back via the [`MainThreadDispatcher`]
    /// (`crate::window_registry::dispatcher`). NEVER `cx.update` from the task.
    ///
    /// **Cursor-only** (T0 spike): there is no public selection accessor on
    /// `InputState` at this gpui-component rev, so the run statement is resolved
    /// via [`crate::query::statement::statement_at`] from the editor cursor.
    ///
    /// [`MainThreadDispatcher`]: crate::main_bridge::MainThreadDispatcher
    pub(crate) fn spawn_sql_run(
        &mut self,
        console: gpui::Entity<crate::view::sql_console::SqlConsole>,
        target: crate::query::ResultTarget,
        cx: &mut Context<Self>,
    ) {
        use crate::query::statement::{ResultKind, classify, statement_at};
        use crate::view::sql_console::ResultRegion;

        // Resolve the statement under the cursor (cursor-only; no selection).
        let (sql, cursor) = console.read(cx).active_sql_and_cursor(cx);
        let span = statement_at(&sql, cursor);
        let stmt = sql[span.start..span.end].trim().to_string();
        if stmt.is_empty() {
            return;
        }
        let kind = classify(&stmt);

        // Read-only guard (P8 T8): in Inspect mode only result-producing
        // statements (SELECT / WITH / PRAGMA / DESCRIBE / SUMMARIZE / …) are
        // allowed; DDL/DML (ResultKind::Exec) is silently blocked here because
        // the Parquet-backed VIEWs would reject it at the engine level anyway.
        if crate::grid::edit_ops::mutation_blocked(self.read_only) && kind == ResultKind::Exec {
            return;
        }

        let engine = self.engine();
        let win_disc = self.window_disc();
        let tab_ix = console.read(cx).active;
        let view_name = crate::query::result_view_name(&win_disc, tab_ix);

        // Flip the console into the running state immediately.
        console.update(cx, |c, cx| {
            c.set_running(true, cx);
            c.set_region(ResultRegion::Empty, cx);
        });
        self.active_query_cancel = Some(crate::query::QueryCancel::new(&engine));

        let ws_weak = cx.entity().downgrade();
        let console_weak = console.downgrade();
        let engine_for_task = std::sync::Arc::clone(&engine);

        tokio::spawn(async move {
            // `create_or_replace_view` / `execute` are `QueryEngine` trait methods.
            use dat0_engine::QueryEngine as _;
            let outcome: SqlRunOutcome = match kind {
                ResultKind::Result => match engine_for_task
                    .create_or_replace_view(&view_name, &stmt)
                    .await
                {
                    Ok(()) => match crate::grid::GridDataSource::new(
                        std::sync::Arc::clone(&engine_for_task),
                        view_name.clone(),
                    )
                    .await
                    {
                        Ok(ds) => SqlRunOutcome::Bound(std::sync::Arc::new(ds)),
                        Err(e) => SqlRunOutcome::Error(e.to_string()),
                    },
                    Err(e) => classify_run_err(e),
                },
                ResultKind::Exec => match engine_for_task.execute(&stmt).await {
                    Ok(r) => SqlRunOutcome::Status(format_exec_status(&r)),
                    Err(e) => classify_run_err(e),
                },
            };
            // Post the apply onto the GPUI main thread. Matches the dispatcher
            // discipline of `run_view_change_inner` / `prefetch_visible_rows`.
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    if let (Some(ws), Some(console)) = (ws_weak.upgrade(), console_weak.upgrade()) {
                        ws.update(app_cx, |ws, cx| {
                            ws.finish_sql_run(&console, target, outcome, cx);
                        });
                    }
                });
            } else {
                tracing::warn!("spawn_sql_run: no MainThreadDispatcher installed; result dropped");
            }
        });
    }

    /// Apply a completed SQL run on the GPUI main thread (P5a T6). Disarms the
    /// cancel guard, clears the running flag, then routes the outcome: a bound
    /// result rebinds the grid; status/error/cancelled render the inline strip.
    fn finish_sql_run(
        &mut self,
        console: &gpui::Entity<crate::view::sql_console::SqlConsole>,
        target: crate::query::ResultTarget,
        outcome: SqlRunOutcome,
        cx: &mut Context<Self>,
    ) {
        use crate::view::sql_console::ResultRegion;

        // Normal completion: disarm so the dropped guard does NOT interrupt.
        if let Some(mut g) = self.active_query_cancel.take() {
            g.disarm();
        }

        // Compute elapsed from the console's run-start stamp BEFORE set_running(false)
        // clears it (set_running(false) sets started_at = None). Capture the FULL
        // editor buffer (not just the statement under cursor) — history shows what
        // the user ran/typed, and load re-opens the whole buffer.
        let elapsed_ms = console
            .read(cx)
            .started_at
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let (sql_text, _) = console.read(cx).active_sql_and_cursor(cx);
        let ok = !matches!(outcome, SqlRunOutcome::Error(_) | SqlRunOutcome::Cancelled);
        // P5c T9: routing tag for the chip, using the live set of attached
        // MotherDuck database names (workspace mode attaches them under real
        // names) so a query is tagged `md`/`mixed` only when it references one
        // that is actually attached.
        let routing = crate::connections::routing::classify_routing(
            &sql_text,
            self.connections.md_databases(),
        );
        console.update(cx, |c, cx| c.set_last_elapsed(elapsed_ms, routing, cx));
        {
            let entry = crate::session::queries::HistoryEntry {
                sql: sql_text,
                ran_at: now_unix_millis(),
                ok,
                elapsed_ms,
            };
            let mut sess = self.session.lock();
            let mut hist = sess.query_history().to_vec();
            crate::session::queries::push_history(&mut hist, entry);
            let _ = sess.set_query_history(hist);
        }

        console.update(cx, |c, cx| c.set_running(false, cx));

        match outcome {
            SqlRunOutcome::Bound(ds) => match target {
                crate::query::ResultTarget::MainGrid => {
                    self.apply_view_change(ds, cx);
                    console.update(cx, |c, cx| c.set_region(ResultRegion::BoundToGrid, cx));
                }
                crate::query::ResultTarget::Pane => {
                    // T9 (Tier 2): route into the console-owned results grid
                    // instead of the main DataGrid. `set_pane_source` stores the
                    // `Arc` + this shell's weak handle (for the pane delegate's
                    // header/scroll closures) and kicks a first-page prefetch;
                    // the console's `render` lazily promotes it to a `TableState`
                    // (it owns the `&mut Window` this callback lacks). The main
                    // grid / table tab is left untouched.
                    let ws_weak = cx.entity().downgrade();
                    console.update(cx, |c, cx| {
                        c.set_pane_source(ds, ws_weak, cx);
                        c.set_region(ResultRegion::Pane, cx);
                    });
                }
            },
            SqlRunOutcome::Status(s) => {
                console.update(cx, |c, cx| c.set_region(ResultRegion::Status(s), cx))
            }
            SqlRunOutcome::Error(e) => {
                console.update(cx, |c, cx| c.set_region(ResultRegion::Error(e), cx))
            }
            SqlRunOutcome::Cancelled => {
                console.update(cx, |c, cx| c.set_region(ResultRegion::Cancelled, cx))
            }
        }
        self.persist_sql_console(cx);
        // Pick up tables created/dropped by this run (CREATE/DROP/Save-as-Table)
        // so autocomplete reflects the new schema on the next keystroke (P5b T2).
        self.refresh_completion_snapshot(cx);
        // Mirror into the Catalog dock so created/dropped tables appear (P6a T7).
        self.refresh_catalog(cx);
    }

    /// Persist the current tab's SQL as a named saved query (P5b T6). Upserts by
    /// name (case-insensitive). No-op on empty name/sql. Called from
    /// [`on_name_prompt_event`](Self::on_name_prompt_event) on a Save confirm (T8).
    pub(crate) fn save_named_query(&mut self, name: String, sql: String, _cx: &mut Context<Self>) {
        if name.trim().is_empty() || sql.trim().is_empty() {
            return;
        }
        let q = crate::session::queries::SavedQuery {
            id: uuid::Uuid::now_v7(),
            name: name.trim().to_string(),
            sql,
            saved_at: now_unix_millis(),
        };
        let mut sess = self.session.lock();
        let mut list = sess.saved_queries().to_vec();
        crate::session::queries::upsert_saved(&mut list, q);
        let _ = sess.set_saved_queries(list);
        drop(sess);
        self.maybe_prompt_save_workspace();
    }

    /// Promote the statement under the cursor to a derived table (P5b T10).
    /// Called from [`on_name_prompt_event`](Self::on_name_prompt_event) on a
    /// confirm of the [`SaveConsoleAsTable`](NamePromptIntent::SaveConsoleAsTable)
    /// intent. Resolves the statement-under-cursor itself (it does NOT use the
    /// SaveQuery captured-SQL), wraps it in a CTAS-style `SELECT * FROM (…)`, and
    /// runs `create_table(.., DerivedOrigin::Sql)` off-thread. On success the
    /// console shows a status line and the autocomplete snapshot is refreshed so
    /// the new table appears in completions; on failure (bad SQL, name
    /// collision) the DuckDB error renders inline in the console's Error region
    /// (no modal — sidesteps PD-021).
    ///
    /// Send discipline (matches the T2/T6/T8 bridge): only `Send + 'static`
    /// values cross into the `tokio::spawn` — the engine `Arc`, the owned
    /// `name`/`stmt`/`select` strings, and the `Weak` shell/console handles. The
    /// GPUI entities are touched ONLY inside the dispatcher closure on the main
    /// thread after `.upgrade()`.
    pub(crate) fn save_console_as_table(&mut self, name: String, _cx: &mut Context<Self>) {
        let Some(console) = self.sql_console.clone() else {
            return;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        let (sql, cursor) = {
            let app: &gpui::App = _cx;
            console.read(app).active_sql_and_cursor(app)
        };
        let span = crate::query::statement::statement_at(&sql, cursor);
        let stmt = sql[span.start..span.end].trim().to_string();
        if stmt.is_empty() {
            return;
        }
        let select = format!("SELECT * FROM ({stmt})");
        let engine = self.engine();
        let ws_weak = _cx.entity().downgrade();
        let console_weak = console.downgrade();
        tokio::spawn(async move {
            use dat0_engine::QueryEngine as _;
            let origin = dat0_engine::DerivedOrigin::Sql(stmt);
            let outcome = engine.create_table(&name, &select, origin).await;
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app: &mut gpui::App| {
                    if let (Some(ws), Some(console)) = (ws_weak.upgrade(), console_weak.upgrade()) {
                        ws.update(app, |ws, cx| match &outcome {
                            Ok(_) => {
                                console.update(cx, |c, cx| {
                                    c.set_region(
                                        crate::view::sql_console::ResultRegion::Status(format!(
                                            "Saved table {name}"
                                        )),
                                        cx,
                                    )
                                });
                                ws.refresh_completion_snapshot(cx);
                                ws.refresh_catalog(cx);
                            }
                            Err(e) => console.update(cx, |c, cx| {
                                c.set_region(
                                    crate::view::sql_console::ResultRegion::Error(e.to_string()),
                                    cx,
                                )
                            }),
                        });
                    }
                });
            } else {
                tracing::warn!(
                    "save_console_as_table: no MainThreadDispatcher installed; result dropped"
                );
            }
        });
    }

    /// Delete a saved query by id (P5b T6). Called from the saved-query picker's
    /// per-row ✕ (T8).
    pub(crate) fn delete_named_query(&mut self, id: uuid::Uuid, _cx: &mut Context<Self>) {
        let mut sess = self.session.lock();
        let mut list = sess.saved_queries().to_vec();
        crate::session::queries::delete_saved(&mut list, id);
        let _ = sess.set_saved_queries(list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_table_name_strips_quotes_and_schema() {
        // ViewModel::base_table() is quoted + may be schema-qualified.
        assert_eq!(bare_table_name("\"main\".\"orders\""), "orders");
        // Bare quoted name (no schema qualifier).
        assert_eq!(bare_table_name("\"orders\""), "orders");
        // Already bare/unquoted — identity.
        assert_eq!(bare_table_name("orders"), "orders");
        // Embedded dots only appear as the schema separator in this layer, so
        // the last segment is the table; quotes are trimmed from both ends.
        assert_eq!(bare_table_name("\"my_db\".\"main\".\"sales\""), "sales");
    }
}
