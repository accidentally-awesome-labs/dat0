//! The dat0 workspace window.
//!
//! B11 is splitting this module into `window/*`; the directory map lands
//! in T16 once every child module exists and can be described accurately.

mod ai;
mod boot;
mod catalog_inspector;
mod charts;
mod connections;
mod data_io;
mod dock;

use dock::SQL_CONSOLE_DOCK_HEIGHT;
mod live_refresh;

pub use live_refresh::dispatch_live_refresh;
mod modals;

use modals::NamePromptIntent;
mod package_ops;
mod render;

pub(crate) use render::bounding_rect;
#[cfg(feature = "a11y-capture")]
mod sql;

use sql::{bare_table_name, now_unix_millis};
mod test_support;

pub(crate) use package_ops::{
    export_package_flow, open_demo_workspace, open_package_at, open_package_flow,
    replay_package_flow, spawn_recovered_scratch, unpack_package_flow,
};
pub use package_ops::{orphan_scan_emit, recovery_scan_emit};
mod workspace_ops;

use workspace_ops::{configured_memory_budget, open_recent_n, open_workspace_at};
pub(crate) use workspace_ops::{
    now_epoch_secs, open_workspace_flow, save_workspace_flow, spawn_workspace_window,
};

pub(crate) use boot::spawn_window;
use boot::{focused_session_arc, open_window_view};
pub use boot::{register_menu_action_handlers, run_app};

use anyhow::Result;
use dat0_i18n::t;
use gpui::{
    App, Application, Bounds, Context, Entity, ExternalPaths, FocusHandle, IntoElement,
    KeyDownEvent, Render, Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, size,
};
use gpui_component::ActiveTheme as _;
use gpui_component::Root;
use gpui_component::h_flex;
use gpui_component::table::{Table, TableState};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;

use crate::app_lock::{AppLock, OpenWindowMessage};
use crate::empty_state::EmptyState;
use crate::file_drop::{DropOutcome, handle_drop};
use crate::grid::{GridDataSource, GridTableDelegate};
use crate::main_bridge::MainLoop;
use crate::recents::Recents;
use crate::session::Session;
use crate::theme::tokens::{Dat0Theme as _, Sp, SpStyled as _};
use crate::view::ViewModel;
use crate::window_registry::{WindowHandle, WindowRegistry};
use crate::workspace::Home;

// ─── Recents helper ──────────────────────────────────────────────────────────

/// B7: which left-dock panel is showing.
///
/// The three shell bools remain the storage; this names the choice they encode
/// so that every transition can go through one place
/// ([`WorkspaceShell::activate_left_panel`]) and the at-most-one-visible
/// invariant can be structural.
/// B9 persists this directly (`session/dock_layout.rs`) rather than mirroring it
/// into a parallel session-side enum, so panel identity has exactly one
/// definition. It is a field-less enum with no gpui dependency, so the session
/// and settings modules can name it freely.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeftPanel {
    Catalog,
    Connections,
    Ai,
}

impl LeftPanel {
    /// `&[T; N]`, not `&'static [T]` — the latter trips
    /// `clippy::redundant_static_lifetimes` under `-D warnings` (A5).
    pub const ALL: &[LeftPanel; 3] = &[LeftPanel::Catalog, LeftPanel::Connections, LeftPanel::Ai];
}

pub struct WorkspaceShell {
    session: Arc<Mutex<Session>>,
    pub(crate) data_source: Option<Arc<GridDataSource>>,
    /// Stateful entity owning the gpui-component Table's scroll handles,
    /// column-resize state, selection, etc. (`gpui-table-api-notes.md` §3).
    /// Rebuilt when `data_source` is swapped (e.g., user drops a second
    /// file). `None` until the first data source lands.
    table_state: Option<Entity<TableState<GridTableDelegate>>>,
    /// B5: the `DockArea` hosting the grid center. Built lazily on the first
    /// render because `DockArea::new` needs a `&mut Window`, which only exists
    /// inside `render` — the same constraint that makes `table_state` a lazy
    /// promotion.
    dock_area: Option<Entity<gpui_component::dock::DockArea>>,
    /// B5: the center panel. Held across frames; rebuilding it per frame would
    /// mint a fresh entity every render and throw away the panel's identity.
    grid_panel: Option<Entity<crate::panels::grid_panel::GridPanel>>,
    /// B6: the right dock's two panels, built lazily alongside the dock.
    inspector_panel: Option<Entity<crate::panels::inspector_panel::InspectorPanel>>,
    charts_panel: Option<Entity<crate::panels::charts_panel::ChartsPanel>>,
    /// B6: the `(inspector, charts)` visibility the right dock was last
    /// reconciled to, so `sync_right_dock` does work only on an actual change.
    right_dock_state: (bool, bool),
    /// B7: the (catalog, connections, ai) visibility triple the left dock was
    /// last reconciled to, so `sync_left_dock` does work only on a real change.
    left_dock_state: (bool, bool, bool),
    /// B9: the layout this window was restored with, consumed once by
    /// `ensure_dock_area` for its dock sizes.
    ///
    /// The visibility bools are seeded directly in the constructor instead,
    /// because `render` reads them before `ensure_dock_area` runs.
    restored_layout: Option<crate::session::dock_layout::DockLayout>,
    /// B7: the activity rail's KEYBOARD cursor — an index into
    /// `view::activity_rail::ITEMS`. Independent of which panel is open: the
    /// cursor still exists when the dock is collapsed.
    rail_cursor: usize,
    catalog_panel: Option<Entity<crate::panels::catalog_panel::CatalogPanel>>,
    connections_panel: Option<Entity<crate::panels::connections_panel::ConnectionsPanel>>,
    ai_dock_panel: Option<Entity<crate::panels::ai_dock_panel::AiDockPanel>>,
    /// Theme observer subscription, kept alive for the lifetime of the
    /// view. Per `docs/internal/gpui-api-notes.md` §0.A.4 the `Theme`
    /// global is app-scoped; switching theme in one window notifies every
    /// observer in every window so the grid re-renders with the new
    /// palette.
    ///
    /// As of P3b T12 (D-002 closure) we subscribe to
    /// `crate::theme::Theme` — dat0's own theme type was promoted to a
    /// `gpui::Global` in `crates/dat0-app/src/theme/mod.rs`, replacing
    /// the T4 placeholder subscription against `gpui_component::Theme`.
    theme_subscription: Option<Subscription>,
    /// Per-tab view model (T13). Owns the active Transformation stack,
    /// undo cursor, and view name. Initialized when a table is first
    /// registered (file drop). `None` until the first table lands.
    ///
    /// T13 note: P4a is single-tab per window; multi-tab (one ViewModel
    /// per tab) is P4b. The field is `Option` so it can be None before
    /// any file is dropped.
    pub(crate) view_model: Option<ViewModel>,
    /// Currently-mounted filter popover (T0 / PD-016 funnel-click wiring).
    /// `Some` while a popover is open for some column; cleared when its
    /// `Outcome` is routed (apply / clear / cancel). Rendered as an overlay
    /// child in `render` when present.
    pub(crate) active_popover:
        Option<Entity<crate::view::filter_popover_entity::FilterPopoverEntity>>,
    /// Subscription to the active popover's `FilterPopoverEvent`. Stored so
    /// the callback stays registered — a dropped `Subscription` deregisters
    /// silently (P4a T10b post-review lesson). Cleared alongside
    /// `active_popover`.
    popover_sub: Option<Subscription>,
    /// Ephemeral grid selection (T4 pure-logic model). `None` until a data
    /// source is mounted; `SelectionModel::new` requires non-empty grid
    /// dimensions, so it is constructed lazily on the first render after a
    /// source lands (see `render`). T11 wires keyboard movers to it; T6 reads
    /// `selection.active()` to locate the cell being edited.
    pub(crate) selection: Option<crate::grid::selection::SelectionModel>,
    /// Currently-mounted inline cell editor (T6). `Some` while editing the
    /// active cell; cleared on commit / cancel. Rendered as an overlay child
    /// in `render` when present.
    pub(crate) cell_editor: Option<Entity<crate::grid::cell_editor::CellEditor>>,
    /// Subscription to the active cell editor's `CellEditorEvent`. Stored so
    /// the commit/cancel callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap). Cleared alongside
    /// `cell_editor`.
    pub(crate) cell_editor_sub: Option<Subscription>,
    /// A cursor position requested before an async view-rebind (e.g. Enter-advance),
    /// to be restored when the SelectionModel is rebuilt (which otherwise starts at
    /// the origin). Consumed once, on the next selection rebuild.
    pub(crate) pending_active_cell: Option<crate::grid::selection::CellCoord>,
    /// Marching-ants range set by the most recent copy/cut (T7). Stored in
    /// screen-space; T7 only records the range. T11/polish will render the
    /// animated dashed border and clear this on the next selection change —
    /// until then it persists after a copy/cut.
    // T11/polish: render marching-ants from this stored range + clear on selection change.
    pub(crate) copied_range: Option<crate::grid::selection::CellRange>,
    /// Currently-mounted inline header-rename editor (P4c T7). `Some` while the
    /// user is renaming a column; cleared on commit / cancel. The `usize` is the
    /// screen column index. Rendered in-place inside `render_th` when `Some` for
    /// that column.
    pub(crate) header_rename: Option<(usize, Entity<crate::grid::cell_editor::HeaderRenameEditor>)>,
    /// Subscription to the active header-rename editor's [`HeaderRenameEvent`].
    /// Stored so the commit/cancel callback stays registered — a dropped
    /// `Subscription` deregisters silently (the P4a T10b trap). Cleared
    /// alongside `header_rename`.
    pub(crate) header_rename_sub: Option<Subscription>,
    /// Folded visible columns (source→display, display order, deletes excluded),
    /// recomputed from the active stack whenever it changes (P4c T5). Drives the
    /// header labels + order and the screen-col→source addressing used by every
    /// mutating path. Empty until a data source binds; with no projection ops
    /// active it is the identity over `ds.visible_column_names()`, so screen-col
    /// index == schema index and existing behaviour is unchanged.
    pub(crate) column_view: Vec<dat0_engine::transform::ProjectionColumn>,
    /// GPUI focus handle for the workspace shell (T11). The outer container
    /// element tracks this handle so that `on_key_down` receives key events
    /// when the workspace has focus.  Constructed once in `new`; the element
    /// receives focus on the first click or programmatic request.
    ///
    /// PD-018 note: the grid render-cache work (PD-018) may later gate
    /// fine-grained cell focus; this shell-level handle is sufficient for
    /// T11's keyboard map + selection navigation.
    focus_handle: FocusHandle,
    /// Stable focus handles keyed by static id — hero buttons AND dock-panel
    /// containers (e.g. the `catalog-tree` panel).
    /// Created once and reused across renders (the transient `EmptyState` must NOT
    /// own these — it is rebuilt every frame).
    hero_focus: std::collections::HashMap<&'static str, gpui::FocusHandle>,
    /// Active-row index for keyboard nav of the Home-hero recents list. Held on
    /// the persistent shell because the transient `EmptyState` is rebuilt every
    /// frame; clamped to the recents length at render. Slice: recents-nav.
    /// `pub(crate)`: `empty_state::recents_column` (a sibling module) mutates
    /// this directly from its arrow-key `cx.listener` closure.
    pub(crate) recents_active: usize,
    /// Active-row index for keyboard nav of the Catalog panel (catalog-tree
    /// slice). Held on the persistent shell (the panel render is a free fn,
    /// rebuilt every frame); clamped to the visible-row count at each use.
    /// `pub(crate)`: `catalog::panel` (a sibling module) reaches it from
    /// `cx.listener` closures.
    pub(crate) catalog_active: usize,
    /// Collapsed attach-parent aliases in the Catalog panel (catalog-tree
    /// slice). Empty = all expanded. Mirrored to session v10
    /// `SessionUiState.catalog_collapsed` (restored in the ctor, written back
    /// by `persist_dock_ui` on every toggle).
    pub(crate) catalog_collapsed: std::collections::HashSet<String>,
    /// PipelineBar expanded/collapsed toggle state (P4c T9). The expanded
    /// timeline view is T10 — this stub stores the toggle flag so the `⌄`
    /// button can flip it and be rendered correctly on the next frame.
    pub(crate) pipeline_bar_state: crate::view::pipeline_bar::PipelineBarState,
    /// Currently-mounted Export… dialog (P4c T11). `Some` while the File →
    /// Export… dialog is open; cleared when its `ExportEvent` is routed
    /// (Export → run + dismiss, or Cancel → dismiss). Rendered as an overlay
    /// child in `render` when present.
    export_dialog: Option<Entity<crate::view::export_dialog::ExportDialog>>,
    /// Subscription to the active export dialog's [`ExportEvent`]. Stored so the
    /// Export/Cancel callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap). Cleared alongside
    /// `export_dialog`.
    ///
    /// [`ExportEvent`]: crate::view::export_dialog::ExportEvent
    export_dialog_sub: Option<Subscription>,
    /// SQL Console panel (P5a T5). Lazily constructed on the first
    /// `toggle_sql_console` call (which has the `&mut Window` that the per-tab
    /// code editors need). `None` until first toggled; visibility is gated by
    /// `sql_console_visible` so a second toggle hides without tearing it down.
    pub(crate) sql_console: Option<Entity<crate::view::sql_console::SqlConsole>>,
    /// Subscription to the console's [`SqlConsoleEvent`]. Stored so the
    /// run/cancel/persist callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap).
    ///
    /// Written (never explicitly read); the field's sole purpose is to keep the
    /// `Subscription` alive for the entity's life so `on_sql_console_event` keeps
    /// firing. Dropping a `Subscription` deregisters silently, so this must be a
    /// stored field — hence the lint allowance (a keep-alive, not dead code).
    ///
    /// [`SqlConsoleEvent`]: crate::view::sql_console::SqlConsoleEvent
    #[allow(dead_code)] // keep-alive: storing the Subscription is the read
    pub(crate) sql_console_sub: Option<Subscription>,
    /// Whether the window-close `Persist` backstop has been registered (P5a
    /// T10). Set the first time the console is built so the
    /// `on_window_should_close` hook is installed exactly once per window.
    pub(crate) sql_console_close_hooked: bool,
    /// Cancellation guard for the in-flight SQL console run (P5a T6). `Some`
    /// while a run is executing; dropped/disarmed in `finish_sql_run`. The
    /// guard's `Drop` (or an explicit `cancel()` in T7) fires the engine's
    /// connection-wide `interrupt()`.
    pub(crate) active_query_cancel: Option<crate::query::QueryCancel>,
    /// Shared per-window autocomplete schema cache (P5b T2). Lazily created on
    /// the first `toggle_sql_console` (so it can be cloned into the console's
    /// per-tab providers), then refreshed off the engine on console-open and
    /// after every run. `None` until the console is first opened.
    pub(crate) sql_snapshot: Option<crate::query::completion::SharedSnapshot>,
    /// Currently-mounted Save-query name prompt (P5b T8). `Some` while the
    /// 💾 → Save-query modal is open; cleared when its
    /// [`NamePromptEvent`](crate::view::name_prompt::NamePromptEvent) is routed
    /// (Confirm → save + dismiss, or Cancel → dismiss). Rendered as a window
    /// overlay child in `render` when present.
    name_prompt: Option<Entity<crate::view::name_prompt::NamePrompt>>,
    /// Subscription to the active name prompt's `NamePromptEvent`. Stored so the
    /// Confirm/Cancel callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap). Cleared alongside `name_prompt`.
    name_prompt_sub: Option<Subscription>,
    /// The active tab's SQL captured at the moment 💾 was pressed (P5b T8). Held
    /// while the name prompt is open so a Confirm saves the SQL as it was THEN,
    /// not whatever is in the editor when the user finishes typing the name.
    /// Only the `SaveQuery` intent uses this; `SaveConsoleAsTable` re-reads the
    /// statement-under-cursor on confirm, so it leaves this `None`.
    name_prompt_sql: Option<String>,
    /// What the currently-open name prompt should do on Confirm (P5b T8 + T10).
    /// `Some` exactly while `name_prompt` is mounted; the `Confirm` arm of
    /// [`on_name_prompt_event`](Self::on_name_prompt_event) matches on it to
    /// route to the right handler. Cleared alongside `name_prompt`.
    name_prompt_intent: Option<NamePromptIntent>,
    /// Focus to return to when the currently-open modal dismisses (B1). Set from
    /// `window.focused(cx)` in each modal's open path, BEFORE `NamePrompt::new`
    /// moves focus to the field; `take`n in the dismiss path so a double dismiss
    /// cannot re-focus a stale handle.
    modal_restore_focus: Option<gpui::FocusHandle>,
    /// Set by a modal open path that has NO `&mut Window` — in practice only
    /// [`open_export_dialog`](Self::open_export_dialog), which
    /// `view_actions::dispatch_export` reaches from a bare `&mut App` (the
    /// window registry stores no focused-window handle, and
    /// `App::active_window` is the platform's, untrustworthy under
    /// `TestPlatform`). `render` drains it: it captures the restore target and
    /// focuses the modal's first stop.
    ///
    /// Same shape as `SqlConsole::queue_load` — enqueue windowless, drain in a
    /// render that holds a real `Window`. LOAD-BEARING: with nothing focused
    /// the dispatch path is the window root alone and Tab is completely inert
    /// (B1, measured), so a modal that opens without taking focus is
    /// keyboard-dead.
    pending_modal_focus: bool,
    /// The mirror image of [`Self::pending_modal_focus`]: set by a dismiss path
    /// that has no `&mut Window`. Only the export dialog needs it —
    /// `cx.subscribe_in` requires a `Window` at SUBSCRIPTION time, which
    /// `open_export_dialog` does not have, so its handler cannot be given one.
    /// `render` drains this into `restore_modal_focus`.
    pending_modal_restore: bool,
    /// Currently-mounted saved-query picker (P5b T8, rebuilt as a modal entity
    /// in B2). `Some` while the picker is open; cleared when its
    /// `SavedQueryPickerEvent` routes to a pick or a dismiss. The picker reads
    /// `session.saved_queries()` live, so no snapshot is stored here.
    saved_picker: Option<Entity<crate::view::saved_query_picker::SavedQueryPicker>>,
    /// Subscription to the picker's `SavedQueryPickerEvent`. Stored so the
    /// callback stays registered — a dropped `Subscription` deregisters
    /// silently (the P4a T10b trap). Cleared alongside `saved_picker`.
    saved_picker_sub: Option<Subscription>,
    /// The command palette modal (B4). `Some` while it is open.
    command_palette: Option<Entity<crate::view::command_palette::CommandPalette>>,
    /// Subscription to the palette's `CommandPaletteEvent`. Stored for the same
    /// reason as `saved_picker_sub`; cleared alongside `command_palette`.
    command_palette_sub: Option<Subscription>,
    /// Set by `command_palette::open`, which reaches this shell from a bare
    /// `&mut App` (the global ⌘⇧P handler) and so has no `Window` —
    /// `InputState::new` needs one. Drained at the TOP of `render`, before the
    /// [`pending_modal_focus`](Self::pending_modal_focus) block, so the palette
    /// is mounted in time for that block to focus its first stop this frame.
    pending_palette_open: bool,
    /// Runtime connection state (MotherDuck status + sqlite attachments) for this
    /// window (P5c T6/T10). The persisted projection lives in
    /// `SessionState.attachments` (T7); this is the live UI-facing copy the
    /// Connections panel renders from.
    pub(crate) connections: crate::connections::ConnectionManager,
    /// Whether the left-dock Connections panel is shown (P5c T10/T11). Toggled by
    /// the `ConnectionsToggle` action; gates the panel in `render`.
    pub(crate) connections_panel_visible: bool,
    /// Whether the left-dock Catalog panel is shown (P6a T7). Toggled by the
    /// `CatalogToggle` action; gates the catalog dock in `render`.
    pub(crate) catalog_panel_visible: bool,
    /// Live catalog tree rendered by the Catalog dock (P6a T7). Rebuilt off-thread
    /// by [`Self::refresh_catalog`] whenever the catalog could change (toggle /
    /// import / create / drop).
    pub(crate) catalog_tree: crate::catalog::CatalogTree,
    /// Raw table list last fetched by [`Self::refresh_catalog`] (P6a T11).
    /// Stored so `recompute_lineage` can build the lineage graph without another
    /// engine round-trip. The `CatalogTree` discards origin/parent info, so we
    /// keep the full `Vec<TableInfo>` separately.
    pub(crate) catalog_tables: Vec<dat0_engine::TableInfo>,
    /// Sql-table → referenced base tables (lineage parents), resolved off-thread
    /// by the engine in `refresh_catalog`. Cached so `recompute_lineage` stays
    /// synchronous. Keyed by table name; only Sql-origin tables appear (P6b).
    pub(crate) sql_parents: std::collections::HashMap<String, Vec<String>>,
    /// Whether the right-dock Inspector panel is shown (P6a T9). Toggled by the
    /// `InspectorToggle` action; gates the inspector dock in `render`.
    pub(crate) inspector_panel_visible: bool,
    /// Whether the right-dock Charts panel is shown (P9a T7). Toggled by the
    /// `ChartVisualize` action; gates the chart dock in `render`.
    pub(crate) chart_panel_visible: bool,
    /// Live chart panel state (type + axis picks + last data/error). Bound to
    /// the active grid's base table when the panel opens (P9a T7).
    pub(crate) chart_panel: crate::charts::panel::ChartPanel,
    /// Last rendered chart image (BGRA → gpui `RenderImage`), refreshed by
    /// [`Self::run_plot_query`]. `None` until the first plot query returns.
    pub(crate) chart_image: Option<std::sync::Arc<gpui::RenderImage>>,
    /// Monotonic id incremented on every plot-query kickoff (P9a T7). A spawned
    /// plot result writes its image only if it carries the latest id, so a fast
    /// sequence of type/axis changes never lands a stale chart (mirrors the
    /// inspector's load-supersede guard).
    pub(crate) chart_load_id: u64,
    /// Monotonic id incremented on every Test-connection kickoff AND on every
    /// config-mutation that would change what a test means (provider switch, key
    /// change, model change, toggle flip). A spawned test result is only written
    /// back if it still carries the current id — mirrors `chart_load_id`.
    pub(crate) ai_test_load_id: u64,
    /// Monotonic id incremented on every NL→SQL stream kickoff (P9c-2 T6).
    /// Supersede guard: dispatched deltas only write if the id still matches.
    pub(crate) ai_stream_load_id: u64,
    /// Whether the left-dock AI panel is shown (P9c-1 T9). Toggled by the
    /// `AiPanelToggle` action; gates the AI dock in `render`.
    pub(crate) ai_panel_visible: bool,
    /// AI key/model entry modal (reuses [`NamePrompt`](crate::view::name_prompt::NamePrompt)).
    /// `Some` while the "Set API key…" / "Set model…" prompt is open; cleared on
    /// Confirm / Cancel. Rendered as a window overlay child in `render` (P9c-1 T9).
    pub(crate) ai_entry_prompt: Option<gpui::Entity<crate::view::name_prompt::NamePrompt>>,
    /// Subscription to the AI entry prompt's `NamePromptEvent`. Stored so the
    /// Confirm/Cancel callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap). Cleared alongside `ai_entry_prompt`.
    pub(crate) ai_entry_prompt_sub: Option<gpui::Subscription>,
    /// Live AI-panel draft state (provider/model/toggles + key-set indicator +
    /// transient test-result). Loaded from `AiSettings` + a keychain key-presence
    /// probe when the panel opens (P9c-1 T9). The API KEY itself is never held
    /// here — only a "key is set" boolean.
    pub(crate) ai_panel: crate::ai::panel::AiPanel,
    /// Inspector state: profile target + (table,epoch)-keyed profile cache +
    /// load supersede (P6a T8). Profiles are loaded off-thread by
    /// [`Self::load_inspector_profile`].
    pub(crate) inspector: crate::inspector::InspectorModel,
    /// Per-window live banner list (PD-021). Drained from `error_ux::banner::PENDING`
    /// on each render; rendered as a host strip atop the shell.
    pub(crate) banners: Vec<crate::error_ux::banner::Banner>,
    /// Token-entry modal (reuses [`NamePrompt`](crate::view::name_prompt::NamePrompt)).
    /// `Some` while the MotherDuck token prompt is open; cleared on Confirm /
    /// Cancel. Rendered as a window overlay child in `render` when present.
    pub(crate) md_token_prompt: Option<gpui::Entity<crate::view::name_prompt::NamePrompt>>,
    /// Subscription to the token prompt's `NamePromptEvent`. Stored so the
    /// Confirm/Cancel callback stays registered — a dropped `Subscription`
    /// deregisters silently (the P4a T10b trap). Cleared alongside
    /// `md_token_prompt`.
    pub(crate) md_token_prompt_sub: Option<gpui::Subscription>,
    /// Whether the "Save as workspace?" prompt has been shown this session
    /// (in-memory only — never persisted; shows at most once per launch).
    workspace_prompt_shown: bool,
    /// Whether the first-run tour has been auto-scheduled this per-window
    /// lifetime (in-memory only — never persisted). The persisted
    /// `first_run_done` flag is the authoritative gate across launches; this
    /// bool prevents the render-driven trigger from re-queuing the open on
    /// every subsequent frame before `first_run_done` flips to `true`.
    tour_auto_shown: bool,
    /// Live watcher over the active table's source file (P7c). Re-created
    /// whenever the active table changes (see [`Self::retarget_source_watch`]);
    /// `None` when the active table has no `File` origin. Dropping the field
    /// stops the watch.
    pub(crate) source_watcher: Option<crate::workspace::source_watcher::SourceWatcher>,
    /// When `true` this shell is open in Inspect mode (read-only package).
    /// Every data-mutation entry point (`commit_cell_edit`, `cut_selection`,
    /// `paste_clipboard`, `fill_down`, `set_null_selection`,
    /// `set_value_selection`, `delete_selected_rows`, `delete_column`,
    /// `commit_column_rename`, `save_view_as_table`, and the SQL-console DDL/DML
    /// path) checks this flag via [`crate::grid::edit_ops::mutation_blocked`]
    /// and returns early without executing when it is set. T9 sets this to `true`
    /// immediately after constructing the shell for an Inspect open; the default
    /// is `false` (normal edit-enabled workspace).
    pub(crate) read_only: bool,
}

impl WorkspaceShell {
    pub fn new(session: Arc<Mutex<Session>>, cx: &mut Context<Self>) -> Self {
        // Restore persisted catalog/inspector dock visibility (P6a T13, session
        // v8 `ui`). Read into a local BEFORE building the struct so we don't hold
        // the session lock across the whole ctor.
        let ui = session.lock().ui().clone();
        // B9 precedence: the SESSION is authoritative when it carries a layout
        // of its own — a workspace keeps its own arrangement. Otherwise fall
        // back to the settings-level seed, so a plain launch (which always
        // creates a FRESH scratch session) still opens the way the user left
        // it. Neither present → the mount constants.
        let layout = session.lock().dock_layout().cloned().or_else(|| {
            Self::settings_store()?
                .load_or_default()
                .ok()?
                .ui
                .dock_layout
        });
        Self {
            session,
            data_source: None,
            table_state: None,
            dock_area: None,
            grid_panel: None,
            inspector_panel: None,
            charts_panel: None,
            right_dock_state: (false, false),
            left_dock_state: (false, false, false),
            restored_layout: layout.clone(),
            rail_cursor: 0,
            catalog_panel: None,
            connections_panel: None,
            ai_dock_panel: None,
            theme_subscription: None,
            view_model: None,
            active_popover: None,
            popover_sub: None,
            selection: None,
            cell_editor: None,
            cell_editor_sub: None,
            pending_active_cell: None,
            copied_range: None,
            column_view: Vec::new(),
            focus_handle: cx.focus_handle(),
            hero_focus: std::collections::HashMap::new(),
            recents_active: 0,
            header_rename: None,
            header_rename_sub: None,
            pipeline_bar_state: crate::view::pipeline_bar::PipelineBarState::default(),
            export_dialog: None,
            export_dialog_sub: None,
            sql_console: None,
            sql_console_sub: None,
            sql_console_close_hooked: false,
            active_query_cancel: None,
            sql_snapshot: None,
            name_prompt: None,
            name_prompt_sub: None,
            name_prompt_sql: None,
            name_prompt_intent: None,
            modal_restore_focus: None,
            pending_modal_focus: false,
            pending_modal_restore: false,
            saved_picker: None,
            saved_picker_sub: None,
            command_palette: None,
            command_palette_sub: None,
            pending_palette_open: false,
            connections: Default::default(),
            connections_panel_visible: layout
                .as_ref()
                .is_some_and(|l| l.left_panel == Some(LeftPanel::Connections)),
            catalog_panel_visible: layout
                .as_ref()
                .is_some_and(|l| l.left_panel == Some(LeftPanel::Catalog)),
            catalog_active: 0,
            catalog_collapsed: ui.catalog_collapsed.iter().cloned().collect(),
            catalog_tree: crate::catalog::CatalogTree::default(),
            catalog_tables: Vec::new(),
            sql_parents: Default::default(),
            inspector_panel_visible: layout.as_ref().is_some_and(|l| l.inspector_visible),
            inspector: crate::inspector::InspectorModel::new(),
            ai_panel_visible: layout
                .as_ref()
                .is_some_and(|l| l.left_panel == Some(LeftPanel::Ai)),
            ai_panel: crate::ai::panel::AiPanel::default(),
            ai_entry_prompt: None,
            ai_entry_prompt_sub: None,
            chart_panel_visible: layout.as_ref().is_some_and(|l| l.charts_visible),
            chart_panel: crate::charts::panel::ChartPanel::new(),
            chart_image: None,
            chart_load_id: 0,
            ai_test_load_id: 0,
            ai_stream_load_id: 0,
            banners: Vec::new(),
            md_token_prompt: None,
            md_token_prompt_sub: None,
            workspace_prompt_shown: false,
            tour_auto_shown: false,
            source_watcher: None,
            read_only: false,
        }
    }

    pub fn set_data_source(&mut self, ds: Arc<GridDataSource>) {
        // Drop any stale TableState — it was built around the previous
        // delegate's `Arc<GridDataSource>` and would render stale rows.
        // The next `render` call rebuilds one against the new source.
        self.table_state = None;
        // Clear the selection so it is rebuilt against the new source's
        // dimensions on the next render.  Without this a second file drop
        // would leave SelectionModel with the old row/column counts, and
        // `selection.active().col` could point past the new schema.
        self.selection = None;
        // A brand-new source has no prior cursor to restore.
        self.pending_active_cell = None;
        self.data_source = Some(ds);
        // Re-derive the ColumnView from the new source's visible columns + the
        // active stack (P4c T5). On a fresh bind this is the identity over the
        // visible columns (no projection ops yet); after a rebind that carries
        // an active stack (e.g. a filter view) the source columns are unchanged,
        // so the fold is still identity unless a projection op is present.
        self.refresh_column_view();
    }

    /// Install or replace the active `GridDataSource` after a `ViewChange`
    /// round-trip completes (T13). Clears the stale `TableState` so the
    /// next `render` promotes the new source into a fresh `Entity<TableState>`.
    pub fn apply_view_change(&mut self, new_ds: Arc<GridDataSource>, cx: &mut Context<Self>) {
        self.table_state = None;
        // Defensively clear the selection — a view-change is the rebind path
        // and, while P4b preserves the schema, clearing keeps the selection
        // model consistent and prevents stale-dimension bugs if column count
        // ever changes (e.g., a future hide-column transform).
        self.selection = None;
        self.data_source = Some(new_ds);
        // A view-change rebind re-derives the source columns; recompute the
        // ColumnView so the header labels/order and screen-col→source addressing
        // track the (possibly new) active stack (P4c T5).
        self.refresh_column_view();
        // PD-022: a rebind (undo/redo or SQL-console bind) may change the
        // inspected table's data; refresh its profile + lineage so the dock is
        // not stale. on_table_mutated_structural bumps the epoch, re-profiles,
        // and notifies; recompute_lineage rebuilds the chain.
        if let Some(target) = self.inspector.target_table.clone() {
            self.recompute_lineage();
            self.on_table_mutated_structural(&target, cx); // bumps epoch + reprofiles + notifies
        }
        cx.notify();
        self.maybe_prompt_save_workspace();
    }

    /// Prefetch the page(s) covering screen rows `[start, end)` into the MAIN
    /// grid's `GridDataSource` LRU so the grid's synchronous `render_td` paints
    /// real values for the rows the user can see (PD-018).
    ///
    /// Thin wrapper over [`Self::prefetch_rows_for`] bound to `self.data_source`.
    /// Callers that page a DIFFERENT source (e.g. the console results pane, which
    /// owns a separate `GridDataSource` with its own LRU) must call
    /// `prefetch_rows_for(&that_source, …)` directly so the right cache is
    /// populated (P5a T9).
    pub fn prefetch_visible_rows(&self, start: usize, end: usize, cx: &mut Context<Self>) {
        if let Some(ds) = self.data_source.as_ref() {
            let ds = Arc::clone(ds);
            self.prefetch_rows_for(&ds, start, end, cx);
        }
    }

    /// Source-parameterized prefetch: load the page(s) covering screen rows
    /// `[start, end)` into `ds`'s OWN LRU, then notify the shell so the mounted
    /// view repaints with real values.
    ///
    /// Each [`crate::grid::GridDataSource`] owns a SEPARATE `Mutex<LruCache>`, so
    /// a view's `render_td` only ever finds pages that were fetched into THAT
    /// view's source. The main grid drives this via
    /// [`Self::prefetch_visible_rows`] (passing `self.data_source`); the
    /// console-owned results pane drives it via the delegate's
    /// `visible_rows_changed` hook (passing the PANE's source). Routing both
    /// through this one method means pane scrolling loads the pane's cache and
    /// leaves the main grid's cache untouched (P5a T9 fix).
    ///
    /// The fetch runs OFF the GPUI main thread — `GridDataSource::page_for` is
    /// async DuckDB I/O and must never block the 60 fps render loop. Once the
    /// page is in the LRU, the re-render `notify` is posted back onto the main
    /// thread via the [`crate::main_bridge::MainThreadDispatcher`] (the canonical
    /// `spawn_view_change` discipline — NEVER `cx.update` from the tokio task).
    pub(crate) fn prefetch_rows_for(
        &self,
        ds: &Arc<crate::grid::GridDataSource>,
        start: usize,
        end: usize,
        cx: &mut Context<Self>,
    ) {
        // Cheap resident guard: if both boundary pages are already in the LRU
        // cache, the synchronous `render_td` will already paint real values —
        // there is nothing to fetch and no notify to post.  This eliminates the
        // gratuitous task + notify storm when the user scrolls quickly over
        // pages that were prefetched on an earlier tick.
        //
        // The guard does NOT perturb LRU eviction order (`contains` is
        // non-mutating) and is O(1).
        //
        // Prefetch-on-bind path: on first render, page 0 is absent, so
        // `pages_resident` returns false and the spawn proceeds as normal.
        let last = end.saturating_sub(1);
        if ds.pages_resident(start, last) {
            return;
        }

        let ds = Arc::clone(ds);
        let ws_weak = cx.entity().downgrade();

        // Page-align the range to the rows actually requested; `page_for`
        // internally aligns each `row` to its `PAGE_ROWS` boundary, so issuing
        // one fetch per visible row would be wasteful. We sample the start and
        // (inclusive) last row so a visible range that straddles a page boundary
        // loads both pages.
        let start = start as u64;
        let last = last as u64;

        tokio::spawn(async move {
            // Load the page covering the first visible row, then (if different)
            // the page covering the last visible row. `page_for` is idempotent
            // (cache hit on the second call for the same page).
            let mut any_loaded = false;
            for row in [start, last] {
                match ds.page_for(row).await {
                    Ok(_) => any_loaded = true,
                    Err(e) => {
                        tracing::warn!(row, error = %e, "prefetch_rows_for: page_for failed");
                    }
                }
            }
            if !any_loaded {
                return;
            }
            // Post the re-render onto the GPUI main thread via the dispatcher.
            if let Some(dispatcher) = crate::window_registry::dispatcher() {
                let _ = dispatcher.dispatch(move |app_cx: &mut gpui::App| {
                    if let Some(h) = ws_weak.upgrade() {
                        h.update(app_cx, |_ws, cx| cx.notify());
                    }
                });
            } else {
                tracing::warn!(
                    "prefetch_rows_for: no MainThreadDispatcher installed; grid will not refresh"
                );
            }
        });
    }

    /// Mutable access to the per-tab `ViewModel` (T13). Returns `None` if
    /// no table has been registered yet (pre-file-drop state).
    pub fn view_model_mut(&mut self) -> Option<&mut ViewModel> {
        self.view_model.as_mut()
    }

    /// The `Arc<DuckDBEngine>` bound to this session (T13 helper).
    pub fn engine(&self) -> Arc<dat0_engine::DuckDBEngine> {
        Arc::clone(&self.session.lock().engine)
    }

    /// Return a clone of the `Arc<Mutex<Session>>` so workspace flows can
    /// promote the session without holding a borrow on `self`.
    pub fn session_arc(&self) -> Arc<Mutex<Session>> {
        Arc::clone(&self.session)
    }

    /// The base table name (already-quoted, suitable for ViewModel construction).
    /// Returns `None` if no file has been registered yet.
    pub fn base_table(&self) -> Option<String> {
        self.view_model
            .as_ref()
            .map(|vm| vm.base_table().to_string())
    }

    /// Drive the engine round-trip + grid rebind for a [`ViewChange`] (T6 —
    /// extracted from `on_sort_zone_click` / `route_filter_outcome` so the
    /// `spawn_view_change` + `apply_view_change` boilerplate is written once;
    /// reused by T6/T7/T8 mutation handlers).
    ///
    /// Reads the base-table name from the active `ViewModel` (the round-trip
    /// rebinds to it when `change` clears the stack). No-op if no `ViewModel`
    /// is mounted yet.
    ///
    /// Preserves the dispatcher discipline established by `spawn_view_change`:
    /// the closure runs on the GPUI main thread via the `MainThreadDispatcher`,
    /// never `cx.update` from the tokio task.
    pub(crate) fn spawn_rebind(&mut self, change: crate::view::ViewChange, cx: &mut Context<Self>) {
        // The ViewModel stack has already been mutated by the caller (set_sort /
        // set_filter / edit_cells / delete_rows / a projection op). Refresh the
        // ColumnView so the header labels/order + screen-col→source addressing
        // reflect the new active stack immediately — a display-only change
        // (Rename/Reorder/DeleteColumn, T6+) never round-trips through
        // `apply_view_change`, so this is the only refresh hook for those. For a
        // real data-view change this is harmless (the source columns are
        // unchanged) and `apply_view_change` refreshes again on rebind (P4c T5).
        self.refresh_column_view();
        let Some(base_table) = self.base_table() else {
            return;
        };
        let engine = self.engine();
        let ws_weak = cx.entity().downgrade();
        crate::view::spawn_view_change(
            engine,
            base_table,
            change,
            Arc::new(move |new_ds, app_cx| {
                if let Some(h) = ws_weak.upgrade() {
                    h.update(app_cx, |ws, cx| ws.apply_view_change(new_ds, cx));
                }
            }),
        );
    }

    /// Sort-zone click (T0 / PD-016). Reads the current sort, cycles the
    /// clicked column (plain `click` or `shift_click` extend), writes it back
    /// via [`ViewModel::set_sort`], and drives the engine round-trip exactly
    /// like `dispatch_undo` in `actions/view_actions.rs`.
    pub fn on_sort_zone_click(&mut self, col_ix: usize, shift: bool, cx: &mut Context<Self>) {
        let Some(column) = self.column_name(col_ix) else {
            return;
        };
        let Some(vm) = self.view_model.as_mut() else {
            return;
        };
        let active = vm.current_sort_as_active();
        let active = if shift {
            active.shift_click(&column)
        } else {
            active.click(&column)
        };
        let change = vm.set_sort(active.keys().to_vec());
        self.spawn_rebind(change, cx);
    }

    /// Funnel-zone click (T0 / PD-016). Mounts the filter popover for
    /// `col_ix`, pre-populated from any active filter on that column, and
    /// subscribes to its `FilterPopoverEvent` so the terminal `Outcome` is
    /// routed back into the `ViewModel` + engine round-trip.
    pub fn on_funnel_click(&mut self, col_ix: usize, _window: &mut Window, cx: &mut Context<Self>) {
        use crate::view::filter_popover_entity::{FilterPopoverEntity, FilterPopoverEvent};

        let Some(column) = self.column_name(col_ix) else {
            return;
        };
        let Some(ds) = self.data_source.as_ref() else {
            return;
        };
        // Type the popover off the SOURCE column (resolved via the ColumnView)
        // so a display-only reorder can't hand the funnel the wrong column's
        // operator surface (P4c T5). Identity with no projection ops.
        let column_type = ds
            .column_type_for_source(&column)
            .unwrap_or(crate::view::filter_popover::ColumnType::String);

        // Pre-populate from any active filter on this column (edit-existing flow).
        let pre = self
            .view_model
            .as_ref()
            .and_then(|vm| vm.find_filter_for(&column).cloned());

        let popover = cx.new(|_| match &pre {
            Some(existing) => {
                FilterPopoverEntity::from_existing(column.clone(), column_type, existing)
            }
            None => FilterPopoverEntity::new(column.clone(), column_type),
        });

        // STORE the subscription — callbacks fire silently if the returned
        // Subscription is dropped (P4a T10b post-review lesson).
        let sub = cx.subscribe(
            &popover,
            move |ws: &mut Self, _pop, ev: &FilterPopoverEvent, cx| {
                let FilterPopoverEvent::OutcomeEmitted(outcome) = ev;
                ws.route_filter_outcome(outcome.clone(), cx);
            },
        );
        self.popover_sub = Some(sub);
        self.active_popover = Some(popover);
        cx.notify();
    }

    /// Route a filter-popover [`Outcome`] into the ViewModel + engine
    /// round-trip, then dismiss the popover (T0 / PD-016).
    ///
    /// [`Outcome`]: crate::view::filter_popover_entity::Outcome
    fn route_filter_outcome(
        &mut self,
        outcome: crate::view::filter_popover_entity::Outcome,
        cx: &mut Context<Self>,
    ) {
        // Dismiss the popover regardless of the outcome.
        self.active_popover = None;
        self.popover_sub = None;

        let change = {
            let Some(vm) = self.view_model.as_mut() else {
                cx.notify();
                return;
            };
            // Pure decision lives in `view::route_outcome` (shared with the
            // click_wiring integration test); the engine round-trip below stays
            // in this GPUI handler.
            crate::view::route_outcome(vm, outcome)
        };
        let Some(change) = change else {
            cx.notify();
            return;
        };
        self.spawn_rebind(change, cx);
    }

    // ── Export… dialog + native save panel + streaming COPY (P4c T11) ─────────

    // ── SQL Console panel (P5a T5) ────────────────────────────────────────────

    // ── Charts (P9a T7) ────────────────────────────────────────────────────

    /// PipelineBar scrubber: jump to state `k` (keep first `k` ops) as one undo
    /// step (P4c T9). Refreshes the `ColumnView` and routes the resulting
    /// `ViewChange` — display-only ops re-render immediately; data-view changes
    /// spawn an engine round-trip. No-op when no `ViewModel` is mounted.
    pub fn pipeline_jump_to(&mut self, k: usize, cx: &mut Context<Self>) {
        let Some(vm) = self.view_model.as_mut() else {
            return;
        };
        let change = vm.jump_to(k);
        self.refresh_column_view();
        self.route_change(change, cx);
    }

    /// PipelineBar expanded timeline: remove the transform at stack position `i`
    /// in ONE undo step (P4c T10). Refreshes the `ColumnView` and routes the
    /// resulting `ViewChange` — display-only ops re-render immediately; data-view
    /// changes spawn an engine round-trip. No-op when no `ViewModel` is mounted.
    pub fn pipeline_remove_at(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(vm) = self.view_model.as_mut() else {
            return;
        };
        let change = vm.remove_at(i);
        self.refresh_column_view();
        self.route_change(change, cx);
    }

    /// Return the active inline header-rename editor for `col_ix`, if one is
    /// mounted for that column. Used by `GridTableDelegate::render_th` to render
    /// the editor in-place instead of the column label (P4c T7).
    pub fn header_rename_for(
        &self,
        col_ix: usize,
    ) -> Option<Entity<crate::grid::cell_editor::HeaderRenameEditor>> {
        self.header_rename
            .as_ref()
            .filter(|(c, _)| *c == col_ix)
            .map(|(_, e)| e.clone())
    }

    // ── AI panel (P9c-1 T9) ────────────────────────────────────────────────

    // ─── P11a T3: Hero open helpers ──────────────────────────────────────────
}

// ---------------------------------------------------------------------------
// SQL console run-path support types (P5a T6)
// ---------------------------------------------------------------------------

impl WorkspaceShell {}
