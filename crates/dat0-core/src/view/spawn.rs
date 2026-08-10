//! Driving a [`ViewChange`] through the engine and back to the grid.
//!
//! [`ViewModel`](super::ViewModel)'s mutators are pure: they hand back a
//! [`ViewChange`] and leave the round-trip to the caller. This is that
//! round-trip, and it exists as one function because two of its rules are easy
//! to get wrong independently at four call sites.
//!
//! # Superseding is internal, and scoped to the View lane
//!
//! The contract used to be "the caller interrupts before issuing a newer
//! change". None of the call sites did, so a rapid sequence of filter or sort
//! changes left every stale round-trip running to completion. Superseding now
//! lives here and fires unconditionally — safe *because* it is scoped to
//! [`QueryLane::View`], so a console run or a grid prefetch sharing the
//! connection is left alone.
//!
//! The lane is claimed **synchronously, before the returned future is
//! polled**. Two changes issued back to back must supersede in issue order;
//! claiming inside the future would let them race for the slot. That is why
//! this is a plain `fn` returning a future rather than an `async fn`.
//!
//! # A supersede is not an error the user asked for
//!
//! A superseded round-trip comes back as [`EngineError::Interrupted`]. Showing
//! a banner for it would raise one on every filter tweak, so that one variant
//! is logged and dropped. Every other failure still banners: a view change
//! that silently fails leaves the grid on the stale view with no indication
//! the filter was never applied.
//!
//! # Display-only changes never touch the engine
//!
//! A projection op (rename, reorder, delete-column) recompiles to identical
//! SQL, so it neither supersedes nor claims the lane — a column rename must
//! not abort an in-flight filter that is still wanted.

use std::sync::Arc;

use dat0_engine::{DuckDBEngine, EngineError, QueryEngine, QueryLane};

use super::ViewChange;
use crate::grid::GridDataSource;

/// Start a view change. The lane claim happens before this returns.
///
/// The returned future resolves to the [`GridDataSource`] the grid must rebind
/// to, or `None` when there is nothing to rebind: a display-only change, a
/// change superseded by a newer one, or a failure (which has already been
/// bannered).
///
/// Spawning is the caller's: `tokio::spawn` from a background owner, or the
/// renderer's own `spawn` from a component that wants to write the result into
/// a signal without crossing a thread.
pub fn start_view_change(
    engine: Arc<DuckDBEngine>,
    base_table: String,
    change: ViewChange,
) -> impl Future<Output = Option<Arc<GridDataSource>>> + Send + 'static {
    // Claimed here, not in the future: see the module docs.
    let token = if change.is_display_only() {
        None
    } else {
        engine.interrupt_lane(QueryLane::View);
        Some(engine.begin_query(QueryLane::View))
    };

    async move {
        let outcome = run_view_change(Arc::clone(&engine), base_table, change).await;

        // Retire the token on BOTH paths, and before any banner: leaving it in
        // the slot would let the next supersede interrupt a query that has
        // already returned.
        if let Some(token) = token {
            engine.end_query(token);
        }

        match outcome {
            Ok(ds) => ds,
            Err(e) => {
                let engine_err = e.downcast_ref::<EngineError>();
                if matches!(engine_err, Some(EngineError::Interrupted)) {
                    tracing::debug!("view change superseded; interrupt suppressed");
                    return None;
                }
                tracing::error!(error = %e, "view change failed");
                crate::error_ux::push(match engine_err {
                    Some(engine_err) => crate::error_ux::banner_for(engine_err),
                    None => crate::error_ux::Banner::error(
                        dat0_i18n::t("view.change.failed"),
                        format!("{e:#}"),
                    ),
                });
                None
            }
        }
    }
}

/// The round-trip itself: bind the view, build the data source, drop the view
/// the change replaced.
async fn run_view_change(
    engine: Arc<DuckDBEngine>,
    base_table: String,
    change: ViewChange,
) -> anyhow::Result<Option<Arc<GridDataSource>>> {
    // Display-only fast path: `new_active_view: Some` with `sql: None`. The
    // engine already has this view bound to the current SQL, so there is no
    // create, no rebind and no drop — `previous_active_view` is `None` here by
    // construction. Emitted when a projection op is applied, or when an
    // undo/redo removes a redundant op and the stack recompiles identically.
    if change.is_display_only() {
        return Ok(None);
    }

    let table_name = match (&change.new_active_view, &change.sql) {
        (Some(view), Some(sql)) => {
            engine.create_or_replace_view(view, sql).await?;
            view.clone()
        }
        (None, None) => base_table.clone(),
        // `(Some, None)` was short-circuited above. The only shape left is
        // `(None, Some)`: a base rebind carrying SQL, which is a real
        // invariant violation rather than a case to paper over.
        _ => {
            anyhow::bail!("ViewChange invariant violated: sql present without a new_active_view");
        }
    };

    let ds = Arc::new(GridDataSource::new(Arc::clone(&engine), table_name).await?);

    // Best-effort: the new view is already bound, so failing to drop the old
    // one costs a little memory and nothing else.
    if let Some(prev) = change.previous_active_view {
        if let Err(e) = engine.drop_view(&prev).await {
            tracing::debug!(view = %prev, error = %e, "drop_view best-effort: ignored");
        }
    }

    Ok(Some(ds))
}

#[cfg(test)]
mod tests {
    //! Exercises the round-trip against a real `DuckDBEngine`. The critical
    //! case is the display-only `(Some, None)` shape produced by undo/redo of a
    //! projection or a redundant op: it must return without bailing and without
    //! dropping the still-active view.

    use super::*;
    use crate::view::ViewModel;
    use dat0_engine::{MemoryBudget, RegisterOpts, Transformation};
    use tempfile::TempDir;

    /// An engine with a 100-row CSV (`a` INTEGER, `b` TEXT) registered, plus the
    /// already-quoted base-table name.
    async fn engine_with_table(tmp: &TempDir) -> (Arc<DuckDBEngine>, String) {
        let csv = tmp.path().join("t.csv");
        let mut s = String::from("a,b\n");
        for i in 0..100_i64 {
            s.push_str(&format!("{i},x{i}\n"));
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
    async fn a_display_only_change_keeps_the_active_view_bound() {
        let tmp = TempDir::new().unwrap();
        let (engine, base) = engine_with_table(&tmp).await;
        let mut vm = ViewModel::new("tab1".into(), base.clone());

        let create = vm.apply(filter_a_gte(50));
        assert!(!create.is_display_only());
        let view_name = create.new_active_view.clone().expect("filter names a view");
        run_view_change(engine.clone(), base.clone(), create)
            .await
            .expect("real data-view change must succeed");

        engine
            .execute_paged(&format!("SELECT * FROM \"{view_name}\""), 0, 10)
            .await
            .expect("view must exist after create");

        let display_only = vm.apply(Transformation::Rename {
            column: "a".into(),
            to: "A".into(),
        });
        assert!(
            display_only.is_display_only(),
            "rename recompiles to identical SQL -> display-only, got {display_only:?}"
        );
        assert_eq!(
            display_only.new_active_view.as_deref(),
            Some(view_name.as_str()),
            "display-only change keeps the SAME view bound"
        );
        assert_eq!(display_only.previous_active_view, None);

        let rebind = run_view_change(engine.clone(), base.clone(), display_only)
            .await
            .expect("display-only change must NOT bail");
        assert!(
            rebind.is_none(),
            "a display-only change gives the grid nothing to rebind to"
        );

        engine
            .execute_paged(&format!("SELECT * FROM \"{view_name}\""), 0, 10)
            .await
            .expect("display-only change must NOT drop the still-active view");
    }

    #[tokio::test]
    async fn undoing_a_projection_keeps_the_active_view_bound() {
        // Undo of a projection op yields the same `(Some, None)` shape from a
        // different direction — the path reached from the undo action.
        let tmp = TempDir::new().unwrap();
        let (engine, base) = engine_with_table(&tmp).await;
        let mut vm = ViewModel::new("tab1".into(), base.clone());

        let create = vm.apply(filter_a_gte(50));
        let view_name = create.new_active_view.clone().unwrap();
        run_view_change(engine.clone(), base.clone(), create)
            .await
            .unwrap();

        vm.apply(Transformation::Rename {
            column: "a".into(),
            to: "A".into(),
        });
        let undo = vm.undo().expect("undo yields a ViewChange");
        assert!(undo.is_display_only(), "undo of rename is display-only");
        assert_eq!(undo.previous_active_view, None);

        run_view_change(engine.clone(), base.clone(), undo)
            .await
            .expect("display-only undo must NOT bail");

        engine
            .execute_paged(&format!("SELECT * FROM \"{view_name}\""), 0, 10)
            .await
            .expect("display-only undo must NOT drop the still-active view");
    }

    /// A display-only undo re-PROJECTS the Inspector (cards re-arrange to the
    /// new column projection) but never re-PROFILES it: the profiled SQL is
    /// unchanged, so no Inspector refresh hook is needed at that seam.
    ///
    /// Projection ops are no-ops in `compile_view_sql`, which is the premise
    /// this pins. If one ever starts emitting SQL, the undo's `ViewChange`
    /// stops being display-only and this fails — flagging that the hook is now
    /// required.
    #[tokio::test]
    async fn a_display_only_undo_leaves_the_profiled_sql_identical() {
        use dat0_engine::compile_view_sql;
        let tmp = TempDir::new().unwrap();
        let (engine, base) = engine_with_table(&tmp).await;
        let mut vm = ViewModel::new("tab1".into(), base.clone());

        let create = vm.apply(filter_a_gte(50));
        let profiled_sql = create.sql.clone().expect("filter is a real data change");
        run_view_change(engine.clone(), base.clone(), create)
            .await
            .unwrap();

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

        let after_undo = compile_view_sql(&base, &[filter_a_gte(50)]).unwrap();
        assert_eq!(
            after_undo, profiled_sql,
            "display-only undo leaves the profiled SQL unchanged"
        );
    }

    #[tokio::test]
    async fn sql_without_a_view_name_is_an_invariant_violation() {
        // The only shape that remains a genuine violation: `(None, Some)`.
        let tmp = TempDir::new().unwrap();
        let (engine, base) = engine_with_table(&tmp).await;
        let bad = ViewChange {
            new_active_view: None,
            previous_active_view: None,
            sql: Some("SELECT 1".into()),
        };
        // `expect_err` would need `Debug` on the Ok side, and a data source is
        // a live engine handle rather than something to print.
        let Err(err) = run_view_change(engine, base, bad).await else {
            panic!("(None, Some) must bail as an invariant violation");
        };
        assert!(
            err.to_string().contains("invariant violated"),
            "unexpected error: {err}"
        );
    }
}
