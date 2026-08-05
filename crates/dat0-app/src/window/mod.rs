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

impl WorkspaceShell {
    /// B5: the grid center's element tree — the real `Table`, the promotion
    /// placeholder, or the empty-state hero.
    ///
    /// Extracted from `render` VERBATIM so [`crate::panels::grid_panel::GridPanel`]
    /// can call it: the panel is a thin wrapper and this shell still owns every
    /// piece of grid state. It takes `&mut self` because the hero arm mints the
    /// hero focus handles through `hero_focus_handle` and can flip
    /// `tour_auto_shown`.
    pub(crate) fn render_grid_body(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        // `Table<D>` and the empty-state hero are different concrete
        // types, so we widen every arm with `.into_any_element()` to
        // satisfy `impl IntoElement`'s single-return-type requirement.
        //
        // P3b T7 adds the empty-state hero branch: when no data source is
        // mounted (or the mounted source is empty), pick between the
        // "samples picker" hero (recents empty) and the recents-only hero
        // (recents non-empty). Recents emptiness is read directly from
        // disk here so the view doesn't need a plumbed-in `Arc<Mutex<Recents>>`
        // — `Recents::with_path` is a cheap JSON read and the empty-state
        // render is not on the per-row hot path.
        match (self.data_source.as_ref(), self.table_state.as_ref()) {
            (Some(ds), Some(state)) if !ds.is_empty() => {
                // Real Table mount — closes the P3a T10 placeholder.
                // Per `docs/internal/gpui-table-api-notes.md` §3:
                //   `Table::new(state: &Entity<TableState<D>>) -> Self`
                // Theming flows implicitly via `cx.theme()` inside the
                // widget (spike §1.3); no prop to pass.
                let table = Table::new(state).stripe(true).bordered(true);

                // T9: mount the selection-aware right-click context menu on the
                // grid body. `ContextMenuExt::context_menu` requires
                // `ParentElement + Styled`, which the `Table` (a `RenderOnce`
                // widget) does not implement directly — so we wrap it in a
                // `div` and hang the menu off that. `build_menu` snapshots the
                // current selection flag and captures a weak handle to this
                // shell so the items dispatch into the live edit handlers.
                use crate::grid::context_menu::{ContextMenuExt, build_menu};
                let ws_weak = cx.entity().downgrade();
                // Use the active cell's column as the fallback for "Delete
                // Column" when no column selection is active (body-level menu;
                // the header right-click handler passes the header's col_ix
                // directly when that wiring lands in a later task).
                let active_col = self.selection.as_ref().map(|s| s.active().col).unwrap_or(0);
                let menu_builder = build_menu(ws_weak, self.selection.as_ref(), active_col);
                div()
                    .size_full()
                    .child(table)
                    .context_menu(menu_builder)
                    .into_any_element()
            }
            (Some(_), None) => {
                // Data source landed but TableState hasn't been promoted
                // yet (the next frame promotes it). Brief placeholder.
                div().child("Loading grid…").into_any_element()
            }
            // Either no data source, or a data source with zero rows —
            // both fall back to the empty-state hero. `recents_empty`
            // toggles the right-column content (samples vs. recents).
            _ => {
                // One config_dir() call feeds both recents and the
                // first_run_done read. On any error (config dir unavailable
                // OR settings parse failure) both default conservatively:
                // recents=empty, first_run_done=true (suppresses tour).
                let (recents_empty, first_run_done) = match crate::platform::config_dir() {
                    Ok(cfg) => {
                        let re = Recents::with_path(cfg.join("recents.json"))
                            .list()
                            .is_empty();
                        let frd = crate::settings::store::SettingsStore::with_path(
                            cfg.join("settings.toml"),
                        )
                        .load_or_default()
                        .map(|s| s.first_run_done)
                        .unwrap_or(true); // suppress tour on load error
                        (re, frd)
                    }
                    Err(_) => (true, true),
                };

                // One-shot auto-open: schedule the tour exactly once per
                // process. `tour_auto_shown` is set SYNCHRONOUSLY before
                // scheduling so that subsequent render frames (which
                // re-enter this branch before `first_run_done` persists)
                // cannot re-queue a second open. The dispatcher hop defers
                // `onboarding::open` out of the render frame, mirroring the
                // `about::open` pattern (`window_registry::dispatcher()` +
                // `dispatcher.dispatch`).
                if !self.tour_auto_shown && crate::empty_state::should_auto_tour(first_run_done) {
                    self.tour_auto_shown = true;
                    if let Some(dispatcher) = crate::window_registry::dispatcher() {
                        let _ = dispatcher.dispatch(|cx: &mut gpui::App| {
                            crate::onboarding::open(cx);
                        });
                    }
                }

                // Pre-register the stable per-hero-button focus handles on the
                // persistent shell, then hand them down to the transient
                // `EmptyState` (which must NOT mint focus handles — it is rebuilt
                // every frame, so a fresh handle each render would lose focus on
                // the harness's forced re-render). Slice 6. Registering all five
                // fixed ids unconditionally is fine — `HeroHandles::get` is only
                // invoked by whichever branch actually renders (`sample_column`
                // looks up `hero-open-file-samples`, `recents_column` looks up
                // `hero-open-file-recents`; only one of the two ever runs per
                // frame), so both branches always find their handles pre-registered.
                let hero_ids: [&'static str; 5] = [
                    "hero-take-tour",
                    "hero-open-demo",
                    "hero-open-file-samples",
                    "hero-open-file-recents",
                    "recents-list",
                ];
                let mut map = std::collections::HashMap::new();
                for id in hero_ids {
                    map.insert(id, self.hero_focus_handle(id, cx));
                }
                for entry in crate::sample_data::entries() {
                    let id = crate::empty_state::sample_static_id(&entry.kind);
                    map.insert(id, self.hero_focus_handle(id, cx));
                }
                let hero = crate::empty_state::HeroHandles { map };
                EmptyState::new(recents_empty, first_run_done, self.recents_active)
                    .render(&hero, cx)
            }
        }
    }

    /// B5: the shell's root focus handle — the grid's tab stop and the host of
    /// the arrow-key handler. `GridPanel::focus_handle` returns this so a focus
    /// request routed at the panel lands on the real grid rather than on an
    /// untracked handle that would silently swallow it.
    pub(crate) fn grid_focus_handle(&self) -> gpui::FocusHandle {
        self.focus_handle.clone()
    }
}

/// Inclusive bounding rectangle `(r0, c0, r1, c1)` over a set of `(row, col)`
/// cells, or `None` when the set is empty (T7 copy/cut). Used to build the
/// dense bounding-rect grid a discontiguous selection serializes to (gaps in
/// the rect become empty cells).
pub(crate) fn bounding_rect(cells: &[(usize, usize)]) -> Option<(usize, usize, usize, usize)> {
    let mut it = cells.iter();
    let &(r, c) = it.next()?;
    let (mut r0, mut c0, mut r1, mut c1) = (r, c, r, c);
    for &(row, col) in it {
        r0 = r0.min(row);
        c0 = c0.min(col);
        r1 = r1.max(row);
        c1 = c1.max(col);
    }
    Some((r0, c0, r1, c1))
}

impl Render for WorkspaceShell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // B4: build the palette here, where a `Window` exists. Deliberately
        // BEFORE the `pending_modal_focus` block below — that block then finds
        // the palette in `mounted_modals()` and focuses its first stop (the
        // query field) in this same frame, rather than a frame later.
        if self.pending_palette_open {
            // Cleared unconditionally, for the same reason `pending_modal_focus`
            // is: a stale flag surviving into a later frame would re-open the
            // palette over whatever modal is up then.
            self.pending_palette_open = false;
            // ⚠ ⌘⇧P is a GLOBAL binding, so it fires even while another modal
            // owns the screen (a NamePrompt, the export dialog, …). Mounting on
            // top would make two modals: `render` traps only `mounted_modals()
            // .first()`, so the second one would be UNTRAPPED in release, and
            // the `debug_assert!` in `mount_command_palette` panics in debug.
            // Measured — `the_chord_is_inert_while_another_modal_is_open`
            // panicked before this guard existed.
            if self.open_modal_count(cx) == 0 {
                self.mount_command_palette(window, cx);
                self.pending_modal_focus = true;
            } else {
                tracing::debug!("command palette: another modal is open; ignoring ⌘⇧P");
            }
        }
        // B2: drain a windowless modal open (see `pending_modal_focus`). The
        // handle lookup is sequenced into a local so its immutable borrow ends
        // before the field writes.
        if self.pending_modal_focus {
            let first = self
                .mounted_modals(cx)
                .first()
                .and_then(|m| m.focus_order.first().cloned());
            if let Some(fh) = first {
                self.modal_restore_focus = window.focused(cx);
                window.focus(&fh);
            }
            // Cleared unconditionally, INCLUDING when no modal turned out to be
            // mounted. A stale flag surviving into a later frame would fire on
            // the NEXT modal and overwrite `modal_restore_focus` with that
            // modal's own first stop, so dismissing it would hand focus to a
            // handle that no longer exists instead of the pre-modal stop. The
            // open paths make that unreachable today (they `cx.notify()`, and a
            // dismissal needs user input, which needs a paint); this makes it
            // unrepresentable rather than merely argued.
            self.pending_modal_focus = false;
        }
        // …and the mirror image on dismiss (see `pending_modal_restore`).
        if self.pending_modal_restore {
            self.restore_modal_focus(window);
            self.pending_modal_restore = false;
        }

        // Subscribe to Theme global changes once, on the first render. The
        // subscription returns a `Subscription` that must be kept alive
        // (drop = unregister) per `gpui-api-notes.md` §0.A.2.
        //
        // P3b T12 flipped the type parameter from the T4 placeholder
        // `gpui_component::Theme` to `crate::theme::Theme` — dat0's own
        // theme type is now a `gpui::Global` (see `theme/mod.rs`), so the
        // Settings dropdown's `Theme::switch` fans out here.
        if self.theme_subscription.is_none() {
            let sub = cx.observe_global::<crate::theme::Theme>(|_view, cx| {
                cx.notify();
            });
            self.theme_subscription = Some(sub);
        }

        // PD-021 banner host: drain any globally-stashed banners into this
        // window's live list, then build an OWNED host element. Computing
        // `banner_host` here (after the `&mut self.banners` drain, before the
        // builder chain) keeps the `self.banners.iter()` borrow from outliving
        // the later `&mut self` mutations in this render.
        crate::error_ux::banner::merge_pending(&mut self.banners);
        let banner_host: Option<gpui::AnyElement> = (!self.banners.is_empty()).then(|| {
            gpui::div()
                .flex()
                .flex_col()
                .gap_sp(Sp::S4)
                .p_sp(Sp::S4)
                .children(
                    self.banners
                        .iter()
                        .map(|b| crate::error_ux::banner::render_banner(b, cx).into_any_element()),
                )
                .into_any_element()
        });

        // Lazily promote `Arc<GridDataSource>` → `Entity<TableState<…>>`
        // on the first render after the data source landed. `TableState::new`
        // requires `&mut Window`, which is only available inside `render`
        // — the async drop handler stores the `Arc` then asks the view to
        // re-render so this branch can build the stateful entity.
        if let Some(ds) = self.data_source.as_ref() {
            let needs_rebuild = match self.table_state.as_ref() {
                None => true,
                Some(state_entity) => {
                    // If the stored delegate's source no longer matches the
                    // current one (user dropped a second file), rebuild.
                    !state_entity.read(cx).delegate().source_ptr_eq(ds)
                }
            };
            if needs_rebuild {
                // Build the delegate's columns from the active ColumnView so the
                // header renders display labels in display order (P4c T5). With
                // no projection ops the view is identity over the visible schema,
                // so the columns match the pre-P4c schema-derived ones exactly.
                let delegate = GridTableDelegate::new(
                    Arc::clone(ds),
                    cx.entity().downgrade(),
                    &self.column_view,
                );
                self.table_state = Some(cx.new(|cx| TableState::new(delegate, window, cx)));

                // PD-018 prefetch-on-bind: kick a background fetch of the first
                // visible page so the grid paints real values on the next frame
                // instead of em-dash placeholders. The delegate's
                // `visible_rows_changed` hook takes over on scroll. We seed a
                // generous first window (PAGE_ROWS worth) so the initial viewport
                // is fully covered even before the first scroll event fires.
                let initial_rows = usize::try_from(ds.row_count).unwrap_or(usize::MAX);
                self.prefetch_visible_rows(0, initial_rows.min(1024), cx);
            }

            // Lazily construct the selection model once a non-empty source is
            // mounted (T4/T6). `SelectionModel::new` debug-asserts non-empty
            // dimensions, so we only build it when the grid actually has cells.
            // T11 wires keyboard movers; T6 reads `selection.active()` on edit
            // commit. Rebuilt when the dimensions change (data-source swap).
            let rows = usize::try_from(ds.row_count).unwrap_or(usize::MAX);
            let cols = ds.visible_column_count();
            if rows > 0 && cols > 0 && self.selection.is_none() {
                let mut model = crate::grid::selection::SelectionModel::new(rows, cols);
                if let Some(target) = self.pending_active_cell.take() {
                    // Restore a cursor requested before an async rebind (Enter-advance).
                    // move_active_to clamps to the (possibly new) dims — preserves the
                    // "defensive clear" safety apply_view_change documents.
                    model.move_active_to(target.row, target.col);
                }
                self.selection = Some(model);
            }
        }

        self.ensure_dock_area(window, cx);

        self.sync_left_dock(window, cx);
        self.sync_right_dock(window, cx);

        let session = Arc::clone(&self.session);

        let drop_listener = cx.listener(move |_view, paths: &ExternalPaths, _window, cx| {
            let paths_vec: Vec<std::path::PathBuf> = paths.paths().to_vec();
            let session = Arc::clone(&session);
            cx.spawn(
                async move |weak_shell: gpui::WeakEntity<WorkspaceShell>, async_cx| {
                    let outcomes = handle_drop(paths_vec, session).await;
                    Self::route_drop_outcomes(outcomes, weak_shell, async_cx).await;
                },
            )
            .detach();
        });

        // B5: the grid center is rendered by `GridPanel` inside the DockArea now
        // (`render_grid_body` is the panel's delegate target), so `render` only
        // hands the dock to the body row.
        let dock_el = self.dock_area.clone();

        // Slice 6 Task 3: is a REAL grid mounted this frame (as opposed to the
        // "Loading grid…" placeholder or the empty-state hero)? Mirrors the
        // `body` match's own "real Table mount" guard above exactly, so the
        // shell only becomes Tab-reachable while there is actually a grid to
        // navigate into — the empty-state hero has its OWN tab stops (Tasks
        // 1/1b), and turning the shell root into an extra, unlabeled tab stop
        // while the hero is showing would insert an unexpected stop into
        // `hero_tab_cycle_visits_every_button`'s asserted DOM-order cycle.
        let grid_visible = matches!(
            (self.data_source.as_ref(), self.table_state.as_ref()),
            (Some(ds), Some(_)) if !ds.is_empty()
        );

        // Funnel-click filter popover overlay (T0 / PD-016). Anchored top-right
        // while open; the entity drives its own Apply/Cancel/Clear buttons,
        // whose `Outcome` routes back via the stored subscription.
        //
        // B2 gives it the shared `overlay::anchored_overlay` surface — before
        // this it painted no background at all and read as floating text over
        // the grid. `occlude` also stops a click on its padding from reaching
        // the grid underneath. Anchoring it precisely under the clicked funnel
        // icon is still open (master plan §6 calls it a stretch goal).
        let popover_overlay: Option<gpui::AnyElement> = self.active_popover.as_ref().map(|p| {
            crate::overlay::anchored_overlay(cx)
                .absolute()
                .top_8()
                .right_4()
                .child(p.clone())
                .into_any_element()
        });

        // Inline cell-editor overlay (T6). Mounted by `begin_cell_edit` over the
        // active cell; commits via the stored `cell_editor_sub` subscription.
        // T6 mounts it top-left so the widget is reachable for UAT (T14).
        //
        // Same B2 treatment: its own render is a bare `h_flex().gap_1().p_1()`,
        // so only the inner `Input` had any surface of its own. Anchoring it
        // over the active cell is still open.
        let editor_overlay: Option<gpui::AnyElement> = self.cell_editor.as_ref().map(|e| {
            crate::overlay::anchored_overlay(cx)
                .absolute()
                .top_8()
                .left_4()
                .child(e.clone())
                .into_any_element()
        });

        // B2: every modal — the three `NamePrompt`-backed prompts (Save-query
        // P5b T8, MotherDuck token P5c T11, AI key/model P9c-1 T9) and the
        // Export… dialog (P4c T11) — is mounted, trapped and counted from ONE
        // list. `modal_host` supplies the scrim, the centred card and the
        // `Dialog` a11y node; the Tab trap hangs off the shell root below,
        // because gpui picks the ACTION from the key-context stack but the
        // HANDLER from the dispatch path walked upward from the FOCUSED node,
        // and a scrim is a sibling of the shell's content (B1, measured).
        //
        // At most one modal is ever open (`open_modal_count`), so `first()` is
        // the live one; a second would be the one NOT trapped, which is why the
        // open paths `debug_assert!` the invariant.
        let mut modals = self.mounted_modals(cx);
        let modal = (!modals.is_empty()).then(|| modals.remove(0));
        let modal_focus_order: Option<Vec<FocusHandle>> =
            modal.as_ref().map(|m| m.focus_order.clone());
        let modal_overlay: Option<gpui::AnyElement> = modal.map(|m| {
            crate::overlay::modal_host(m.a11y_id, m.title, m.content, cx).into_any_element()
        });

        // T10: tab-strip with dirty-dot indicator. Shown whenever a ViewModel
        // is mounted (i.e. a file has been loaded). The "•" glyph appears next
        // to the tab label when `vm.is_dirty()` is true — meaning the active
        // transformation stack contains at least one Edit or RowDelete op.
        // Undo clears the stack back past the dirty ops and the dot disappears
        // on the next render (cx.notify() fires after every rebind).
        let tab_strip: Option<gpui::AnyElement> = self.view_model.as_ref().map(|vm| {
            let is_dirty = vm.is_dirty();
            let label = vm.tab_id().to_string();
            let tab_label = h_flex()
                .gap_sp(Sp::S4)
                .items_center()
                .child(div().child(label))
                .children(is_dirty.then(|| div().child("•")));
            h_flex()
                .w_full()
                .px_sp(Sp::S12)
                .py_sp(Sp::S4)
                .border_b_1()
                .child(tab_label)
                .into_any_element()
        });

        // ── T11 / PD-018: focus ring for the active cell ─────────────────────────
        //
        // PD-018 closed the render-cache gap, so the focus ring is now drawn
        // PER-CELL inside `GridTableDelegate::render_td` (a 2-px blue border on
        // the cell at `selection.active()`, plus a lighter tint on selected
        // cells). It reads the live selection through the delegate's weak
        // `WorkspaceShell` handle, so it always tracks the current cursor and
        // re-renders whenever the selection changes (`cx.notify()` after every
        // mover / mutation). The previous bottom-left floating badge is therefore
        // removed — there is no overlay element here anymore.

        // ── T11: key-down handler — navigation keys → SelectionModel movers ──────
        //
        // The handler is attached to the outer container so it fires whenever
        // the shell has focus (tracked via `focus_handle`).
        //
        // Keys handled here:
        //   arrows (plain/shift/cmd) → `apply_key` → `SelectionModel` movers
        //   Escape                   → `apply_key(Escape)` → `SelectionModel::clear`
        //   Cmd/Ctrl+A               → `apply_key(SelectAll)`
        //   Enter / F2               → `begin_cell_edit` (T6)
        //   Cmd/Ctrl+C               → `copy_selection` (T7)
        //   Cmd/Ctrl+X               → `cut_selection` (T7)
        //   Cmd/Ctrl+V               → `paste_clipboard` (T7)
        //   Delete / Backspace       → `set_null_selection` (T8)
        //   Cmd/Ctrl+D               → `fill_down` (T8)
        //
        // Undo/Redo (Cmd-Z / Cmd-Shift-Z) are bound globally via cx.on_action
        // in run_app — do NOT rebind here.
        let key_handler = cx.listener(|ws: &mut Self, ev: &KeyDownEvent, window, cx| {
            use crate::grid::keymap::{Key, apply_key, key_from_event};

            let ks = &ev.keystroke;
            let mods = &ks.modifiers;
            let key_str = ks.key.as_str();

            // ── Check for non-navigation keys first ───────────────────────────
            // secondary = Cmd on macOS, Ctrl on Linux/Windows.
            let secondary = mods.secondary();
            let secondary_only = secondary && !mods.shift && !mods.alt;
            let no_mods = !mods.shift && !mods.platform && !mods.control && !mods.alt;

            // Enter / F2 → begin cell edit (T6) — but only when no editor is already
            // open. When an editor IS open, its inner Input handles Enter (emits
            // PressEnter then cx.propagate()s); the raw key bubbles here, and
            // re-mounting would drop the commit subscription before PressEnter routes
            // CommitAndMove. The open editor owns Enter; the shell must not re-mount it.
            if (key_str == "enter" || key_str == "f2") && no_mods && ws.cell_editor.is_none() {
                ws.begin_cell_edit(window, cx);
                return;
            }

            // Cmd/Ctrl+C → copy (T7).
            if key_str == "c" && secondary_only {
                ws.copy_selection(cx);
                return;
            }

            // Cmd/Ctrl+X → cut (T7).
            if key_str == "x" && secondary_only {
                ws.cut_selection(cx);
                return;
            }

            // Cmd/Ctrl+V → paste (T7).
            if key_str == "v" && secondary_only {
                ws.paste_clipboard(cx);
                return;
            }

            // Delete / Backspace → set null (T8).
            if (key_str == "delete" || key_str == "backspace") && no_mods {
                ws.set_null_selection(cx);
                return;
            }

            // Cmd/Ctrl+D → fill down (T8).
            if key_str == "d" && secondary_only {
                ws.fill_down(cx);
                return;
            }

            // Escape with an open cell editor → cancel the edit and keep the
            // cursor on the cell (do NOT clear the selection). With no editor
            // open, Escape falls through to the keymap below and clears the
            // selection.
            if key_str == "escape" && no_mods && ws.cell_editor.is_some() {
                ws.cell_editor = None;
                ws.cell_editor_sub = None;
                cx.notify();
                return;
            }

            // ── Navigation keys via the pure keymap ───────────────────────────
            if let Some(nav_key) = key_from_event(ev) {
                // SelectAll (Cmd+A) is in the keymap but we still need cx.notify().
                if let Some(sel) = ws.selection.as_mut() {
                    apply_key(sel, nav_key);
                }
                // Marching-ants border (T12): clear ONLY on Escape so the user
                // can navigate to a paste target while the marquee is visible.
                // Paste clears it via `paste_clipboard`; a new copy/cut overwrites
                // it via `build_selection_tsv`.  Plain arrows / Shift+arrow /
                // Cmd+arrow / Cmd+A must NOT clear it.
                if nav_key == Key::Escape {
                    ws.copied_range = None;
                }
                cx.notify();
            }
        });

        // Request focus on click so the shell captures key events.
        let focus_handle_for_click = self.focus_handle.clone();
        let click_to_focus =
            cx.listener(move |_ws: &mut Self, _ev: &gpui::ClickEvent, window, _cx| {
                focus_handle_for_click.focus(window);
            });

        // PipelineBar (P4c T9 collapsed pills / T10 expanded timeline). Shown
        // when the active transform stack is non-empty. The render fn from
        // `view::pipeline_bar` takes the current active stack; pill/row clicks
        // and the ✕ remove use `cx.listener` (which supplies `&mut self`), so no
        // weak handle is threaded. The `⌄`/`⌃` toggle flips
        // `pipeline_bar_state.expanded` (collapsed pills ↔ expanded timeline).
        let pipeline_bar: Option<gpui::AnyElement> = {
            if let Some(vm) = self.view_model.as_ref() {
                let stack = vm.active();
                if !stack.is_empty() {
                    crate::view::pipeline_bar::render_pipeline_bar(
                        stack,
                        &mut self.pipeline_bar_state,
                        cx,
                    )
                } else {
                    None
                }
            } else {
                None
            }
        };

        // B8: the SQL console used to be mounted here as a fixed 260px strip
        // between the PipelineBar and the grid body, spanning the full window
        // width. It is now a `Panel` in the DockArea's bottom dock, so it
        // renders below the grid inside the centre column and this render has
        // nothing left to place — see `toggle_sql_console`.

        // Slice 6 Task 3: make the shell root a genuine Tab stop, but ONLY
        // while `grid_visible` (real a11y fix — Tab must reach the grid so
        // the arrow keys below have somewhere to land; must NOT apply while
        // the empty-state hero is showing, per the module note above).
        //
        // Tab-index/tab-stop metadata MUST be set on the HANDLE itself, not
        // the element: `track_focus` marks this an EXPLICIT tracked handle,
        // and gpui's paint pass only copies an element's `.tab_index()` onto
        // an AUTO-created handle (div.rs `tracked_focus_handle.is_none()`
        // guard) — never onto one already supplied via `track_focus`. See
        // `a11y/mod.rs`'s `FocusStopExt` doc comment for the same T0 finding.
        // `tab_stop`/`tab_index` write into the handle's shared `FocusRef`
        // (keyed by `FocusId`), so any clone of `self.focus_handle` observes
        // the same update — explicitly setting `tab_stop(grid_visible)` on
        // EVERY render (rather than only ever setting it `true`) keeps the
        // flag correct if a workspace is later closed back to the hero
        // within the same window (data source cleared → `grid_visible`
        // flips back to `false`).
        let shell_focus_handle = self
            .focus_handle
            .clone()
            .tab_index(0)
            .tab_stop(grid_visible);

        // B7: the activity rail. Built here because `hero_focus_handle` needs
        // `&mut self`, which is unavailable inside the body row's builder chain.
        // Always rendered, including on the first-run hero — it is chrome, and
        // it is how a user with no data yet reaches Connections.
        let rail_fh = self.hero_focus_handle("activity-rail", cx);
        let rail_cursor = self.rail_cursor;
        let rail_open = self.open_left_panel();
        let rail = crate::view::activity_rail::render_rail(rail_cursor, rail_open, &rail_fh, cx);

        // B3 status bar. Reads cached scalars only — no query, no I/O, and (per
        // `SelectionModel::selected_cell_count`'s doc comment) no per-cell work,
        // which matters because this runs every frame.
        let status_bar_model = {
            let rows = self.data_source.as_ref().map(|ds| ds.row_count);
            let cols = self.data_source.as_ref().map(|ds| {
                // `column_view` is the POST-projection column list — what the
                // grid actually paints. It is refreshed on every rebind and
                // stack change, but fall back to the source's own count rather
                // than rendering "× 0 cols" if it is ever momentarily empty.
                if self.column_view.is_empty() {
                    ds.visible_column_count()
                } else {
                    self.column_view.len()
                }
            });
            let selected_cells = self
                .selection
                .as_ref()
                .filter(|s| s.has_selection())
                .map(|s| s.selected_cell_count());
            let query = match self.sql_console.as_ref().map(|c| c.read(cx)) {
                Some(c) if c.running => crate::view::status_bar::QueryStatus::Running,
                Some(c) => match c.last_elapsed_ms {
                    Some(ms) => crate::view::status_bar::QueryStatus::Done {
                        ms,
                        routing: c.last_routing,
                    },
                    None => crate::view::status_bar::QueryStatus::Idle,
                },
                None => crate::view::status_bar::QueryStatus::Idle,
            };
            crate::view::status_bar::StatusBarModel {
                rows,
                cols,
                selected_cells,
                query,
                connection: crate::view::status_bar::describe_connection(&self.connections),
            }
        };

        div()
            .id("workspace-shell")
            // B1 modal Tab trap. Installed HERE, on the shell root, rather than
            // on the modal's own scrim, because gpui runs two separate lookups
            // per keystroke: the key-context stack picks the ACTION, then the
            // dispatch path (focused node → upward) picks the HANDLER. The scrim
            // is a SIBLING of the shell's content, so neither lookup reaches it
            // when focus sits outside the modal — measured: with the trap on the
            // scrim, focus staged onto a background hero button walked to the
            // next hero button on a real Tab, because the matched action found no
            // handler, left `propagate_event` true, and `Root`'s Tab binding won
            // the fallthrough. See `overlay`'s module docs.
            //
            // `when_some` means no modal → no key context → normal Tab
            // navigation is byte-identical to before.
            .when_some(modal_focus_order, crate::overlay::modal_trap)
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .track_focus(&shell_focus_handle)
            // ── SQL Console actions (P5a T11) ─────────────────────────────────
            // View-scoped (not global `cx.on_action`) because these reach `self`
            // and three of them need a `&mut Window` (which the global App-level
            // dispatch path does NOT supply). gpui dispatches actions up the
            // focus/element tree, so `Cmd+Enter` / `Cmd+.` fired while the console
            // editor has focus still bubble here to the shell root.
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::SqlRun, _window, cx| {
                    if let Some(c) = ws.sql_console.clone() {
                        ws.spawn_sql_run(c, crate::query::ResultTarget::MainGrid, cx);
                    }
                },
            ))
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::SqlCancel, _window, cx| {
                    ws.cancel_sql_run(cx);
                },
            ))
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::SqlConsoleToggle, window, cx| {
                    ws.toggle_sql_console(window, cx);
                },
            ))
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::SqlNewTab, window, cx| {
                    if let Some(c) = ws.sql_console.clone() {
                        c.update(cx, |c, cx| c.new_tab(window, cx));
                    }
                },
            ))
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::SqlCloseTab, _window, cx| {
                    if let Some(c) = ws.sql_console.clone() {
                        let active = c.read(cx).active;
                        c.update(cx, |c, cx| c.close_tab(active, cx));
                    }
                },
            ))
            // ── Connections panel toggle (P5c T11) ────────────────────────────
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::ConnectionsToggle, _window, cx| {
                    // B7: one of three mutually-exclusive left panels now.
                    ws.activate_left_panel(LeftPanel::Connections, cx);
                },
            ))
            // ── Catalog panel toggle (P6a T7) ─────────────────────────────────
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::CatalogToggle, _window, cx| {
                    // B7: the refresh-on-open and the persist both moved into
                    // `activate_left_panel`, so no entry point can lose them.
                    ws.activate_left_panel(LeftPanel::Catalog, cx);
                },
            ))
            // ── Inspector panel toggle (P6a T9) ───────────────────────────────
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::InspectorToggle, _window, cx| {
                    ws.inspector_panel_visible = !ws.inspector_panel_visible;
                    // Persist the dock visibility (session v8 `ui`, v11 layout).
                    ws.persist_dock_ui();
                    ws.persist_dock_layout(cx);
                    cx.notify();
                },
            ))
            // ── Charts panel toggle (P9a T7) ──────────────────────────────────
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::ChartVisualize, _window, cx| {
                    ws.toggle_chart_panel(cx);
                },
            ))
            // ── AI panel toggle (P9c-1 T9) ────────────────────────────────────
            .on_action(cx.listener(
                |ws: &mut Self, _: &crate::menu_macos::AiPanelToggle, _window, cx| {
                    ws.toggle_ai_panel(cx);
                },
            ))
            .on_key_down(key_handler)
            .on_click(click_to_focus)
            // B10: the last colour literal in `src/`. The closure's 4th param is
            // `&mut App` (gpui `elements/div.rs:940`), so the tint is read from
            // the LIVE theme every time it runs — a theme switch mid-drag is
            // handled with nothing captured. High contrast changes most: this
            // used to paint a hardcoded blue that ignored the HC palette.
            .drag_over::<ExternalPaths>(|style, _, _, cx| style.bg(cx.theme().d0().drag_over))
            .on_drop::<ExternalPaths>(drop_listener)
            .children(banner_host)
            .children(tab_strip)
            .children(pipeline_bar)
            // Body row: the Connections panel (left dock, when visible) + the
            // grid/console body (P5c T10/T11). When the panel is hidden this is
            // just the body in a flex_row — identical layout to before.
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    // B6 moved the Inspector and Charts right docks into the
                    // DockArea; B7 moved the Catalog, Connections and AI left
                    // docks the same way. Every dock now lives inside `dock_el`,
                    // which renders `Catalog | body | Inspector | Charts`
                    // itself, so this row has nothing of its own left to place.
                    // The activity rail joins it at T5.
                    .child(rail)
                    .child(div().flex_1().children(dock_el)),
            )
            // B3: the status bar spans the full width UNDER every dock, so it is
            // a sibling of the body row rather than a child of it. The three
            // overlays below are `.absolute()`, so they still paint above it.
            .child(crate::view::status_bar::render_status_bar(
                &status_bar_model,
                cx,
            ))
            .children(popover_overlay)
            .children(editor_overlay)
            .children(modal_overlay)
            // Mount gpui-component's overlay layers (P7c T8). `Root::render`
            // paints ONLY `self.view`; it does NOT auto-mount the sheet/dialog
            // layers, so without these two lines `open_sheet_at` (the Recovery
            // Sheet) and `open_dialog` (the P7b conflict / same-machine modals +
            // the T6 live-refresh confirm) set their `active_*` state but paint
            // NOTHING. Pattern mirrors gpui-component's own `story/src/lib.rs`.
            // Both return `Option<impl IntoElement>` → `.children(...)`.
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_dialog_layer(window, cx))
    }
}
