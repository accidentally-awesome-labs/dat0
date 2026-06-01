//! Per-tab view state: active Transformation stack + undo cursor + active view name.

pub mod column_view;
pub mod distinct_values;
pub mod filter_popover;
pub mod filter_popover_entity;
pub mod model;
pub mod sort_header;

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
    // Phase 1: create (or rebind to base when cursor == 0).
    let table_name = match (&change.new_active_view, &change.sql) {
        (Some(view), Some(sql)) => {
            engine.create_or_replace_view(view, sql).await?;
            view.clone()
        }
        (None, None) => base_table.clone(),
        _ => {
            // ViewChange invariant: new_active_view ↔ sql appear together.
            anyhow::bail!(
                "ViewChange invariant violated: new_active_view and sql must both be Some or both None"
            );
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
