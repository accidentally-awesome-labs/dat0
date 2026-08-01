//! Per-tab view state: active Transformation stack + undo cursor + active view name.

pub mod column_view;
pub mod command_palette;
pub mod crash_report;
pub mod distinct_values;
pub mod export_dialog;
pub mod filter_popover;
pub mod filter_popover_entity;
pub mod model;
pub mod name_prompt;
pub mod pipeline_bar;
pub mod query_library;
pub mod saved_query_picker;
pub mod sort_header;
pub mod sql_console;
pub mod status_bar;

pub use column_view::fold_columns;
pub use model::{HISTORY_CAP, ViewChange, ViewModel, route_outcome};

// ---------------------------------------------------------------------------
// spawn_view_change — drives engine round-trip + grid rebind (T13)
// ---------------------------------------------------------------------------

use std::sync::Arc;

use dat0_engine::{DuckDBEngine, QueryEngine};

use crate::grid::GridDataSource;

/// Callback type for the `on_rebind` parameter of [`spawn_view_change`].
///
/// Invoked on the GPUI main thread (via `MainThreadDispatcher`) once the
/// new `GridDataSource` is ready. The closure should call
/// `WorkspaceShell::apply_view_change` and trigger a re-render.
pub type RebindFn = Arc<dyn Fn(Arc<GridDataSource>, &mut gpui::App) + Send + Sync>;

/// Spawn an async engine round-trip for a [`ViewChange`] and rebind the grid.
///
/// Sequence:
/// 1. If `change.sql` is `Some`, call `engine.create_or_replace_view` on the
///    tokio thread pool.
/// 2. Build a new [`GridDataSource`] from the resulting view name (or from
///    `base_table` when `new_active_view` is `None`).
/// 3. Post the `on_rebind` closure onto the GPUI main thread via
///    [`crate::window_registry::dispatcher`].
/// 4. Drop the previous view (best-effort) if `previous_active_view` is set.
///
/// **Supersede semantics**: the caller is responsible for calling
/// `engine.interrupt()` before calling this function when a newer
/// `ViewChange` has already arrived. `DuckDBEngine::interrupt()` signals
/// a cooperative cancel on any in-flight query; the previous task either
/// completes normally (if the cancel arrived too late) or returns an error
/// which `spawn_view_change` silently discards — the grid has already been
/// rebound by the newer task, so the stale result is harmless.
///
/// Structured cancellation (D-008) is deferred to P5; this implementation
/// uses the existing `DuckDBEngine::interrupt()` cooperative-cancel surface.
pub fn spawn_view_change(
    engine: Arc<DuckDBEngine>,
    base_table: String,
    change: ViewChange,
    on_rebind: RebindFn,
) {
    tokio::spawn(async move {
        if let Err(e) = run_view_change_inner(engine, base_table, change, on_rebind).await {
            tracing::error!(error = %e, "spawn_view_change failed");
        }
    });
}

async fn run_view_change_inner(
    engine: Arc<DuckDBEngine>,
    base_table: String,
    change: ViewChange,
    on_rebind: RebindFn,
) -> anyhow::Result<()> {
    // Display-only fast path (Option-B projection design): `new_active_view:
    // Some` with `sql: None`. The data view is UNCHANGED — the engine already
    // has this view bound to the current SQL. This shape is emitted when a
    // projection op (Rename/Reorder/DeleteColumn, T6–T8) is applied, or when an
    // undo/redo removes a redundant op and the new stack recompiles to identical
    // SQL. No engine round-trip is needed: no create_or_replace_view, no rebind,
    // no drop_view (`previous_active_view` is None here by construction). The
    // grid header / ColumnView refresh happens at the WorkspaceShell layer in a
    // later task, off the back of the returned change.
    if change.is_display_only() {
        return Ok(());
    }

    // Phase 1: create (or rebind to base when cursor == 0).
    let table_name = match (&change.new_active_view, &change.sql) {
        (Some(view), Some(sql)) => {
            engine.create_or_replace_view(view, sql).await?;
            view.clone()
        }
        (None, None) => base_table.clone(),
        // `(Some, None)` is handled above (display-only). The only shape left
        // here is `(None, Some)`, which is a genuine invariant violation: a base
        // rebind must not carry SQL.
        _ => {
            // ViewChange invariant: a view name and its SQL appear together
            // (except the display-only `(Some, None)` short-circuited above).
            anyhow::bail!("ViewChange invariant violated: sql present without a new_active_view");
        }
    };

    // Phase 2: build the new GridDataSource.
    let ds = Arc::new(GridDataSource::new(engine.clone(), table_name).await?);

    // Phase 3: post the rebind on the main thread.
    // Dispatch only if the main-thread dispatcher is installed (it is in
    // production; may be absent in unit tests that exercise this path headlessly).
    if let Some(dispatcher) = crate::window_registry::dispatcher() {
        let on_rebind_clone = Arc::clone(&on_rebind);
        let _ = dispatcher.dispatch(move |cx: &mut gpui::App| {
            on_rebind_clone(ds, cx);
        });
    } else {
        tracing::warn!("spawn_view_change: no MainThreadDispatcher installed; rebind skipped");
    }

    // Phase 4: drop the previous view (best-effort; ignore errors).
    if let Some(prev) = change.previous_active_view {
        if let Err(e) = engine.drop_view(&prev).await {
            tracing::debug!(view = %prev, error = %e, "drop_view best-effort: ignored");
        }
    }

    Ok(())
}

#[cfg(test)]
mod consumer_tests {
    //! Exercises `run_view_change_inner` (the already-wired undo/redo consumer)
    //! against a real `DuckDBEngine`. The critical case is the display-only
    //! `(Some, None)` shape produced by undo/redo of a projection or redundant
    //! op: the consumer must return Ok WITHOUT bailing and WITHOUT dropping the
    //! still-active view. No `MainThreadDispatcher` is installed in tests, so
    //! Phase 3 logs a warn and skips — Phases 1/2/4 are what we assert here.

    use super::*; // brings ViewModel, ViewChange, run_view_change_inner, RebindFn, engine types
    use dat0_engine::{MemoryBudget, RegisterOpts, Transformation};
    use tempfile::TempDir;

    /// Build an in-memory DuckDB engine with a 100-row CSV (`a` INTEGER, `b` TEXT)
    /// and return the engine plus the already-quoted base-table name.
    async fn engine_with_table(tmp: &TempDir) -> (Arc<DuckDBEngine>, String) {
        let csv = tmp.path().join("t.csv");
        let mut s = String::from("a,b\n");
        for i in 0..100_i64 {
            s.push_str(&format!("{},x{}\n", i, i));
        }
        std::fs::write(&csv, s).unwrap();
        let engine = DuckDBEngine::new(
            tmp.path().join("scratch.duckdb"),
            MemoryBudget {
                bytes: 128 * 1024 * 1024,
            },
        )
        .unwrap();
        engine.init().await.unwrap();
        let info = engine
            .register_file(&csv, RegisterOpts::default())
            .await
            .unwrap();
        let quoted = format!("\"{}\"", info.name.replace('"', "\"\""));
        (Arc::new(engine), quoted)
    }

    /// A no-op rebind closure. It is never invoked in tests (no dispatcher is
    /// installed), but `run_view_change_inner` requires one.
    fn noop_rebind() -> RebindFn {
        Arc::new(|_ds, _cx| {})
    }

    fn filter_a_gte(n: i64) -> Transformation {
        use dat0_engine::{FilterOp, FilterValue, Scalar};
        Transformation::Filter {
            column: "a".into(),
            op: FilterOp::Gte,
            value: FilterValue::Scalar {
                value: Scalar::Int(n),
            },
        }
    }

    #[tokio::test]
    async fn display_only_change_does_not_bail_and_keeps_view_bound() {
        let tmp = TempDir::new().unwrap();
        let (engine, base) = engine_with_table(&tmp).await;
        let mut vm = ViewModel::new("tab1".into(), base.clone());

        // 1) A real data-view change: apply a filter. Route the (Some, Some)
        //    change through the consumer so the engine actually binds the view.
        let create = vm.apply(filter_a_gte(50));
        assert!(!create.is_display_only());
        let view_name = create.new_active_view.clone().expect("filter names a view");
        run_view_change_inner(engine.clone(), base.clone(), create, noop_rebind())
            .await
            .expect("real data-view change must succeed");

        // The view is bound and queryable.
        engine
            .execute_paged(&format!("SELECT * FROM \"{view_name}\""), 0, 10)
            .await
            .expect("view must exist after create");

        // 2) Apply a projection Rename → display-only (Some, None) change.
        let display_only = vm.apply(Transformation::Rename {
            column: "a".into(),
            to: "A".into(),
        });
        assert!(
            display_only.is_display_only(),
            "rename recompiles to identical SQL → display-only, got {display_only:?}"
        );
        assert_eq!(
            display_only.new_active_view.as_deref(),
            Some(view_name.as_str()),
            "display-only change keeps the SAME view bound"
        );
        assert_eq!(display_only.previous_active_view, None);

        // THE REGRESSION GUARD: routing the display-only change through the real
        // consumer must return Ok (no bail) and must NOT drop the active view.
        run_view_change_inner(engine.clone(), base.clone(), display_only, noop_rebind())
            .await
            .expect("display-only change must NOT bail in the consumer");

        // The original view is still bound — nothing was dropped.
        engine
            .execute_paged(&format!("SELECT * FROM \"{view_name}\""), 0, 10)
            .await
            .expect("display-only change must NOT drop the still-active view");
    }

    #[tokio::test]
    async fn display_only_change_from_undo_does_not_bail() {
        // Undo of a projection op also yields the (Some, None) shape — the path
        // reached from dispatch_undo. Prove it routes through the consumer Ok.
        let tmp = TempDir::new().unwrap();
        let (engine, base) = engine_with_table(&tmp).await;
        let mut vm = ViewModel::new("tab1".into(), base.clone());

        let create = vm.apply(filter_a_gte(50));
        let view_name = create.new_active_view.clone().unwrap();
        run_view_change_inner(engine.clone(), base.clone(), create, noop_rebind())
            .await
            .unwrap();

        vm.apply(Transformation::Rename {
            column: "a".into(),
            to: "A".into(),
        });
        let undo = vm.undo().expect("undo yields a ViewChange");
        assert!(undo.is_display_only(), "undo of rename is display-only");
        assert_eq!(undo.previous_active_view, None);

        run_view_change_inner(engine.clone(), base.clone(), undo, noop_rebind())
            .await
            .expect("display-only undo must NOT bail in the consumer");

        engine
            .execute_paged(&format!("SELECT * FROM \"{view_name}\""), 0, 10)
            .await
            .expect("display-only undo must NOT drop the still-active view");
    }

    /// PD-022 follow-up — a display-only undo/redo re-PROJECTS the Inspector
    /// (cards re-arrange to the new column projection) but never re-PROFILES it:
    /// the profiled SQL is unchanged. `dispatch_undo`/`dispatch_redo` `cx.notify()`
    /// to re-render (cheap), and this guard pins that no re-SUMMARIZE is needed.
    ///
    /// The Inspector profiles the bound view's SQL (Current-view mode) or the base
    /// table (Whole-table mode). Projection ops (Rename/Reorder/DeleteColumn) are
    /// no-ops in `compile_view_sql` (engine `render.rs`; see the engine test
    /// `projection_ops_are_sql_noops_flat_parity`), so undoing/redoing one leaves
    /// that SQL — and hence the profile and the table-level lineage — byte-identical.
    /// This guards the premise: if a projection op ever starts emitting SQL, the
    /// undo's `ViewChange` stops being display-only and this assertion fails,
    /// flagging that an Inspector refresh hook is then required at that seam.
    #[tokio::test]
    async fn display_only_undo_keeps_inspector_profile_source_stable() {
        use dat0_engine::compile_view_sql;
        let tmp = TempDir::new().unwrap();
        let (engine, base) = engine_with_table(&tmp).await;
        let mut vm = ViewModel::new("tab1".into(), base.clone());

        // A real data op defines what the Inspector's Current-view mode profiles.
        let create = vm.apply(filter_a_gte(50));
        let profiled_sql = create.sql.clone().expect("filter is a real data change");
        run_view_change_inner(engine.clone(), base.clone(), create, noop_rebind())
            .await
            .unwrap();

        // Apply then undo a projection Rename — display-only in both directions.
        let proj = vm.apply(Transformation::Rename {
            column: "a".into(),
            to: "A".into(),
        });
        assert!(
            proj.is_display_only() && proj.sql.is_none(),
            "applying a projection op is display-only (no profiled-SQL change)"
        );
        let undo = vm.undo().expect("undo yields a ViewChange");
        assert!(
            undo.is_display_only(),
            "undo of a projection op is display-only — the Inspector needs no refresh"
        );

        // The profile source is invariant: the data SQL the Inspector would
        // SUMMARIZE is byte-identical to before the projection op, so there is
        // nothing for an Inspector refresh on this path to change.
        let after_undo = compile_view_sql(&base, &[filter_a_gte(50)]).unwrap();
        assert_eq!(
            after_undo, profiled_sql,
            "display-only undo leaves the profiled SQL unchanged"
        );
    }

    #[tokio::test]
    async fn invalid_sql_without_view_still_bails() {
        // The only shape that remains a genuine invariant violation: (None, Some).
        let tmp = TempDir::new().unwrap();
        let (engine, base) = engine_with_table(&tmp).await;
        let bad = ViewChange {
            new_active_view: None,
            previous_active_view: None,
            sql: Some("SELECT 1".into()),
        };
        let err = run_view_change_inner(engine, base, bad, noop_rebind())
            .await
            .expect_err("(None, Some) must bail as an invariant violation");
        assert!(
            err.to_string().contains("invariant violated"),
            "unexpected error: {err}"
        );
    }
}
