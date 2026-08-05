//! The dat0 workspace window.
//!
//! B11 is splitting this module into `window/*`; the directory map lands
//! in T16 once every child module exists and can be described accurately.

mod ai;
mod boot;
mod catalog_inspector;
mod charts;
mod connections;
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

/// B6: the right dock's per-panel widths, in logical pixels.
///
/// Carried over verbatim from the hand-rolled fixed docks these replaced
/// (`.w_72()` = 288 and `.w(px(560.))`), so no combination of open panels
/// changes how much room the grid gets. `sync_right_dock` sums them when both
/// are visible.
const INSPECTOR_DOCK_WIDTH: f32 = 288.0;
const CHARTS_DOCK_WIDTH: f32 = 560.0;

/// B7: the left dock's fixed width.
///
/// Fixed because `set_left_dock` may be called only once — it leaks
/// subscriptions, see the mount site — and `DockArea` keeps `left_dock` private
/// with no size setter. 384 rather than the 256 each hand-rolled dock used:
/// with one panel showing at a time the old sum has no meaning, and the catalog
/// tree reads better with the extra room.
const LEFT_DOCK_WIDTH: f32 = 384.0;

/// B8: the SQL console bottom dock's initial height.
///
/// 320 rather than the 260 the fixed strip used: the console now shares the
/// centre column's vertical space with the grid instead of spanning the whole
/// window above it, and it gained a 30px title bar of its own. Unlike the side
/// docks this is only an INITIAL height — the bottom dock ships upstream's
/// resize handle, and B9 will persist whatever the user drags it to.
const SQL_CONSOLE_DOCK_HEIGHT: f32 = 320.0;

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
    fn route_export_event(
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

    // ── SQL Console panel (P5a T5) ────────────────────────────────────────────

    // ── Charts (P9a T7) ────────────────────────────────────────────────────

    /// Persist the catalog TREE state to `session.json` (P6a T13; v10 added the
    /// collapse set; v11 moved dock visibility out to
    /// [`persist_dock_layout`](Self::persist_dock_layout)). Sorted for a
    /// deterministic wire format (the insta snapshot gates it).
    pub(crate) fn persist_dock_ui(&self) {
        let mut catalog_collapsed: Vec<String> = self.catalog_collapsed.iter().cloned().collect();
        catalog_collapsed.sort();
        let ui = crate::session::SessionUiState { catalog_collapsed };
        if let Err(e) = self.session.lock().set_ui(ui) {
            tracing::warn!(error = %e, "persist_dock_ui: set_ui failed");
        }
    }

    /// B9: the live dock layout — what is open, and how big.
    ///
    /// Sizes come from `DockArea::dump()` because that is the ONLY public route
    /// to them at rev `0f0ab35`: `DockArea` keeps `left_dock` / `right_dock` /
    /// `bottom_dock` private with no getter, so `Dock::size()` — which is `pub`
    /// — is unreachable from here. Everything else in the dump, including the
    /// whole `center`, is discarded by `DumpMirror`, which has no field for it.
    ///
    /// Open state does NOT come from the dump for the left and right docks: the
    /// shell's own bools are the source of truth those docks are reconciled
    /// against each frame (`sync_left_dock` / `sync_right_dock`), so reading the
    /// dock back would just observe the previous frame's reconciliation. The
    /// console is the exception — B8 deleted its shell bool and derives
    /// visibility from the dock itself, because upstream owns two toggle paths
    /// dat0 does not.
    pub(crate) fn current_dock_layout(
        &self,
        cx: &gpui::App,
    ) -> crate::session::dock_layout::DockLayout {
        use crate::session::dock_layout::{DockLayout, mirror_from_dump, size_to_persist};

        let mirror = self
            .dock_area
            .as_ref()
            .and_then(|dock| serde_json::to_value(dock.read(cx).dump(cx)).ok())
            .map(|v| mirror_from_dump(&v))
            .unwrap_or_default();

        DockLayout {
            left_panel: self.open_left_panel(),
            left_size: mirror.left_dock.and_then(|d| size_to_persist(d.size)),
            inspector_visible: self.inspector_panel_visible,
            charts_visible: self.chart_panel_visible,
            right_size: mirror.right_dock.and_then(|d| size_to_persist(d.size)),
            console_open: mirror.bottom_dock.is_some_and(|d| d.open),
            bottom_size: mirror.bottom_dock.and_then(|d| size_to_persist(d.size)),
        }
    }

    /// B9: persist the live dock layout to `session.json`.
    ///
    /// Called from the sites that already persist dock UI, plus the Quit /
    /// CloseWindow flush. Open and close need no upstream event — every one of
    /// them goes through dat0's own toggles — but SIZE is a pure upstream mouse
    /// drag with no dat0 code in the loop, which is what makes the close
    /// backstop load-bearing rather than belt-and-braces: it is the only thing
    /// that captures a resize not followed by any toggle.
    ///
    /// ⚠ `DockEvent::LayoutChanged` is NOT usable for this, and the master plan
    /// was wrong to propose it: `Dock` is not an `EventEmitter` at all
    /// (`dock/dock.rs` contains zero `cx.emit`; `resize` and `set_open` only
    /// `cx.notify()`), so neither a resize nor an open/close ever emits it. Its
    /// only sources are `StackPanel` and `TabPanel`.
    pub(crate) fn persist_dock_layout(&self, cx: &gpui::App) {
        let layout = self.current_dock_layout(cx);
        if let Err(e) = self.session.lock().set_dock_layout(Some(layout)) {
            tracing::warn!(error = %e, "persist_dock_layout: set_dock_layout failed");
        }
    }

    /// B9: mirror the live dock layout into `settings.toml` as the seed a fresh
    /// scratch window starts from. Load → mutate → atomic save, the same shape
    /// as `update_ai_settings`.
    ///
    /// ⚠ Called ONLY from the close/quit flush, never from a toggle.
    /// `SettingsWatcher` re-reads settings.toml on every write
    /// (`settings/watcher.rs:20-26`). That callback is benign — it swaps the
    /// in-memory `Settings` under an `RwLock` and re-applies nothing — but this
    /// file is otherwise written only on deliberate user action, and writing it
    /// on every dock toggle would turn it into a file written dozens of times a
    /// session, widening the window in which this load-mutate-save clobbers a
    /// hand-edit in flight. The seed only has to be right at quit.
    fn persist_dock_layout_seed(&self, cx: &gpui::App) {
        let Some(store) = Self::settings_store() else {
            return;
        };
        let mut settings = match store.load_or_default() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    ?e,
                    "persist_dock_layout_seed: load failed; seed not updated"
                );
                return;
            }
        };
        settings.ui.dock_layout = Some(self.current_dock_layout(cx));
        if let Err(e) = store.save(&settings) {
            tracing::warn!(
                ?e,
                "persist_dock_layout_seed: save failed; seed not updated"
            );
        }
    }

    /// Short, stable per-window discriminator for the TEMP VIEW name. The
    /// session `window_id` is a `Uuid`; its canonical `to_string()` always
    /// renders `8-4-4-4-12` hex, so the first 4 chars are always ASCII hex.
    fn window_disc(&self) -> String {
        self.session.lock().window_id.to_string()[..4].to_string()
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

    // ── AI panel (P9c-1 T9) ────────────────────────────────────────────────

    // ─── P11a T3: Hero open helpers ──────────────────────────────────────────

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
    async fn route_drop_outcomes(
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

// ---------------------------------------------------------------------------
// SQL console run-path support types (P5a T6)
// ---------------------------------------------------------------------------

impl WorkspaceShell {
    /// Return the static type name of the widget the shell mounts when a
    /// data source is present. Used by `tests/file_drop_formats.rs` to
    /// assert the P3a T10 placeholder (`div`) has been replaced by a real
    /// `gpui_component::table::Table` mount.
    ///
    /// Lives outside `#[cfg(test)]` because Rust integration tests (in
    /// `tests/`) build the library crate without the `test` cfg flag and
    /// therefore can't see `#[cfg(test)]` items. The helper is a static
    /// no-op — `std::any::type_name` is resolved at compile time and
    /// carries no runtime cost.
    ///
    /// This is an intent-level assertion (no real render loop needed) —
    /// see the test docstring in `tests/file_drop_formats.rs` for the
    /// rationale.
    pub fn child_widget_type_name() -> &'static str {
        std::any::type_name::<Table<GridTableDelegate>>()
    }

    /// Get (lazily creating, once) the stable focus handle for hero button `id`.
    /// Handles live on the persistent `WorkspaceShell` (not the transient
    /// `EmptyState`, which is rebuilt every render), so a focused hero control
    /// keeps focus across the harness's forced re-render (Slice 6).
    fn hero_focus_handle(&mut self, id: &'static str, cx: &mut gpui::App) -> gpui::FocusHandle {
        self.hero_focus
            .entry(id)
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// B6: the Inspector panel's element tree, extracted from the body row's
    /// `.w_72()` block so [`crate::panels::inspector_panel::InspectorPanel`]
    /// can call it.
    ///
    /// The sizing and left border the block used to carry are the dock's job
    /// now, and the inspector's own title row moved into `InspectorPanel::title`
    /// so the dock's 30px title bar does not show the word twice.
    pub(crate) fn render_inspector_body(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        crate::inspector::panel::render_inspector(&self.inspector, self.inspector_projection(), cx)
    }

    /// Lazily build the `DockArea` and everything mounted at construction time,
    /// returning it.
    ///
    /// Extracted from `render` at B8 because `toggle_sql_console` needs a dock
    /// too: it mounts the bottom dock on the console's first open, and relying
    /// on `render` having run first would make a toggle-before-first-draw
    /// silently no-op — safe in production (the action handler hangs off the
    /// shell root element, which cannot exist before a render) but a trap for
    /// test authors.
    ///
    /// ⚠ This runs with the shell LEASED — both callers hold `&mut self` — so
    /// B7's constraint applies in full: a `DockItem::tabs` of MORE THAN ONE
    /// panel cannot be built here. Every `add_panel` after the first calls
    /// `set_active_ix`, which reaches `Panel::visible` → `shell.read` and
    /// panics with "cannot read WorkspaceShell while it is already being
    /// updated". A single-panel `DockItem::tab` is immune because
    /// `set_active_ix(0)` early-returns on an unchanged index
    /// (`tab_panel.rs:208-211`) — which is why the bottom dock added by
    /// `toggle_sql_console` holds exactly one panel.
    fn ensure_dock_area(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Entity<gpui_component::dock::DockArea> {
        // B5: lazily build the DockArea and its center GridPanel. `DockArea::new`
        // needs a `&mut Window`, available only here — the same reason the
        // `table_state` promotion above lives in `render`.
        //
        // The center is `DockItem::Panel`, NOT `DockItem::Tabs`. `Tabs` renders a
        // `TabPanel`, which ALWAYS paints a title bar: under `PanelStyle::Auto` a
        // single visible panel gets a 30px title row rather than no chrome. It
        // also wraps the panel in a scroll container plus a cached element and
        // marks the container a tab group — a nested scroll around a virtualized
        // `Table`, a cached child against the single-frame a11y capture, and a
        // tab group that reorders Tab traversal. `DockItem::Panel` renders the
        // panel's raw view instead, putting ZERO elements between this shell and
        // the `Table`. Measured by the B5 T0 chrome gate: panel-body bounds equal
        // host bounds, same origin and same height.
        if self.dock_area.is_none() {
            let weak_shell = cx.entity().downgrade();

            // B9: resolve the mount sizes ONCE, here.
            //
            // Sizes are decided at mount and never re-set: `set_*_dock` runs
            // `subscribe_item`, which pushes onto the `DockArea`'s
            // `_subscriptions` and recurses over the item tree, and nothing ever
            // removes them (B6). Re-setting a dock to resize it would leak
            // forever, so a persisted size can only be applied at this moment.
            //
            // Clamped against the live viewport so a layout saved on a large
            // display cannot restore into a window with no usable centre.
            let viewport = window.viewport_size();
            let layout = self.restored_layout.clone();
            let restored_size =
                |pick: fn(&crate::session::dock_layout::DockLayout) -> Option<u32>,
                 default_size: f32,
                 axis_extent: gpui::Pixels| {
                    crate::session::dock_layout::clamped_size(
                        layout.as_ref().and_then(pick),
                        default_size,
                        f32::from(axis_extent),
                    )
                };
            let right_width = restored_size(
                |l| l.right_size,
                INSPECTOR_DOCK_WIDTH + CHARTS_DOCK_WIDTH,
                viewport.width,
            );
            let left_width = restored_size(|l| l.left_size, LEFT_DOCK_WIDTH, viewport.width);
            let panel = cx.new(|_| crate::panels::grid_panel::GridPanel::new(weak_shell.clone()));
            let dock = cx.new(|cx| {
                let mut dock =
                    gpui_component::dock::DockArea::new("dat0-workspace", Some(1), window, cx);
                // v1 is resize + collapse only, never drag-rearrange. With
                // `DockItem::Panel` there is no tab bar to drag; this is for B6+.
                dock.set_locked(true, window, cx);
                dock
            });
            let item = gpui_component::dock::DockItem::panel(Arc::new(panel.clone()));
            dock.update(cx, |dock, cx| dock.set_center(item, window, cx));

            // B6: the right dock's split, built once and re-used. Its children
            // MUST come from `DockItem::tab`, not `DockItem::panel` —
            // `StackPanel::insert_panel` hard-asserts that a split's children
            // are a `TabPanel` or a `StackPanel` (`stack_panel.rs:106-112`), so
            // the 30px title bar is structural here rather than a style choice.
            //
            // The dock itself is attached by `sync_right_dock` below, which owns
            // both its size and its open state.
            let weak_dock = dock.downgrade();
            let inspector =
                cx.new(|_| crate::panels::inspector_panel::InspectorPanel::new(weak_shell.clone()));
            let charts =
                cx.new(|_| crate::panels::charts_panel::ChartsPanel::new(weak_shell.clone()));
            let right = gpui_component::dock::DockItem::split(
                gpui::Axis::Horizontal,
                vec![
                    gpui_component::dock::DockItem::tab(inspector.clone(), &weak_dock, window, cx)
                        .size(gpui::px(INSPECTOR_DOCK_WIDTH)),
                    gpui_component::dock::DockItem::tab(charts.clone(), &weak_dock, window, cx)
                        .size(gpui::px(CHARTS_DOCK_WIDTH)),
                ],
                &weak_dock,
                window,
                cx,
            );

            // ⚠ `set_right_dock` is called EXACTLY ONCE, here, and must stay
            // that way. It runs `subscribe_item`, which `push`es onto the
            // `DockArea`'s `_subscriptions` and recurses over the whole split
            // (`dock/mod.rs:955-963`); nothing ever removes those. Calling it
            // again per toggle — which is what dynamic dock widths would need,
            // since `DockArea` keeps `right_dock` private and exposes no size
            // setter — leaks ~3 subscriptions every time, forever, each one
            // spawning its own task on subsequent `LayoutChanged` events.
            // `sync_right_dock` therefore only ever opens and closes.
            let want = (self.inspector_panel_visible, self.chart_panel_visible);
            dock.update(cx, |dock, cx| {
                dock.set_right_dock(
                    right,
                    Some(gpui::px(right_width)),
                    want.0 || want.1,
                    window,
                    cx,
                );
            });
            self.right_dock_state = want;

            // B7: the left dock — three panels of which the at-most-one
            // invariant keeps exactly zero or one visible at a time.
            //
            // ⚠⚠ THIS IS A SPLIT OF THREE SINGLE-PANEL TABS, NOT ONE
            // `DockItem::tabs` OF THREE, and the reason is re-entrancy rather
            // than layout. `DockItem::tabs` calls `TabPanel::add_panel` per
            // panel, and every add after the first calls `set_active_ix`, which
            // does real work and ends up reading `Panel::visible` — which reads
            // THIS shell. All of it runs inside `WorkspaceShell::render`, where
            // the shell is already leased, so it panics with "cannot read
            // WorkspaceShell while it is already being updated". A single-panel
            // `DockItem::tab` never hits it because `set_active_ix(0)`
            // early-returns when the index is unchanged (`tab_panel.rs:208-211`)
            // — which is exactly why B6's two-panel right dock was fine.
            //
            // The split costs one `.tab_group()` per child instead of one total,
            // but that is moot here: the invariant means at most ONE child is
            // ever visible, so at most one group is ever populated. A hidden
            // child collapses and yields its space to its sibling
            // (`stack_panel.rs:427-431`), so the visible panel gets the full
            // dock width and the result is what the rail model wants.
            let catalog =
                cx.new(|_| crate::panels::catalog_panel::CatalogPanel::new(weak_shell.clone()));
            let connections = cx.new(|_| {
                crate::panels::connections_panel::ConnectionsPanel::new(weak_shell.clone())
            });
            let ai_dock =
                cx.new(|_| crate::panels::ai_dock_panel::AiDockPanel::new(weak_shell.clone()));
            let left = gpui_component::dock::DockItem::split(
                gpui::Axis::Horizontal,
                vec![
                    gpui_component::dock::DockItem::tab(catalog.clone(), &weak_dock, window, cx)
                        .size(gpui::px(LEFT_DOCK_WIDTH)),
                    gpui_component::dock::DockItem::tab(
                        connections.clone(),
                        &weak_dock,
                        window,
                        cx,
                    )
                    .size(gpui::px(LEFT_DOCK_WIDTH)),
                    gpui_component::dock::DockItem::tab(ai_dock.clone(), &weak_dock, window, cx)
                        .size(gpui::px(LEFT_DOCK_WIDTH)),
                ],
                &weak_dock,
                window,
                cx,
            );

            // ⚠ `set_left_dock` leaks exactly like `set_right_dock` above: it
            // runs `subscribe_item`, which pushes onto `_subscriptions` and
            // recurses over the item tree (`dock/mod.rs:955-963`), and nothing
            // ever removes them. Called EXACTLY ONCE; `sync_left_dock` only ever
            // toggles.
            let want_left = (
                self.catalog_panel_visible,
                self.connections_panel_visible,
                self.ai_panel_visible,
            );
            let left_open = want_left.0 || want_left.1 || want_left.2;
            dock.update(cx, |dock, cx| {
                dock.set_left_dock(left, Some(gpui::px(left_width)), left_open, window, cx);
            });
            self.left_dock_state = want_left;

            self.catalog_panel = Some(catalog);
            self.connections_panel = Some(connections);
            self.ai_dock_panel = Some(ai_dock);

            self.grid_panel = Some(panel);
            self.inspector_panel = Some(inspector);
            self.charts_panel = Some(charts);
            self.dock_area = Some(dock.clone());

            // B9: restore an open console at its persisted height.
            //
            // The bottom dock stays LAZY for everyone else. B8 mounts it on the
            // first console open because upstream keeps a CLOSED bottom dock on
            // screen at `h(px(29.))` so its title bar can be clicked to reopen
            // (`dock.rs:372-380`) — so a user who never opens the console must
            // never see that bar, and the first-run hero carries no persisted
            // layout, which is what keeps the hero untouched.
            //
            // Mounting here, during the first render, was the slice's top risk:
            // it is exactly where B7's `set_active_ix` → `Panel::visible` →
            // `shell.read` re-entrancy panic lives. T0 probe 3 measured it and
            // found it absent — a single-panel `DockItem::tab` early-returns
            // from `set_active_ix(0)` (`tab_panel.rs:208-211`), and that
            // immunity holds at the earliest moment this can run.
            if let Some(l) = self.restored_layout.clone().filter(|l| l.console_open) {
                let height = crate::session::dock_layout::clamped_size(
                    l.bottom_size,
                    SQL_CONSOLE_DOCK_HEIGHT,
                    f32::from(viewport.height),
                );
                self.mount_sql_console(&dock, height, window, cx);
                // The toggle path refreshes the autocomplete schema whenever the
                // console is (re)shown; a restored console must not come back
                // with an empty one.
                self.refresh_completion_snapshot(cx);
            }

            // ⚠ Restoring a panel means seeding its visibility bool in the
            // constructor, which does NOT go through `activate_left_panel` — so
            // the side effects B7 centralised there would otherwise be skipped
            // and a restored panel would come back EMPTY: the catalog with no
            // tree, the AI dock with an unhydrated key/provider state. The docks
            // opened correctly in every test regardless, because those assert
            // visibility rather than contents, which is why this needed the
            // whole-branch pass to find.
            if let Some(target) = self.open_left_panel() {
                self.on_left_panel_shown(target, cx);
            }
        }

        self.dock_area.clone().expect("built directly above")
    }

    /// B6: reconcile the right dock with the visibility bools, which are the
    /// single source of truth.
    ///
    /// This lives in `render` rather than in the toggles because both the dock's
    /// open state and its size need a `&mut Window`, and `toggle_chart_panel`
    /// has only a `Context`. Reconciling here also means every
    /// `a11y-capture` test shim that writes a bool directly keeps working with
    /// no shim change at all: they write, the next frame reconciles.
    ///
    /// Which panel shows is `Panel::visible`, read straight off these bools, so
    /// this only has to decide whether the dock as a whole is open — a hidden
    /// panel's `resizable_panel` already collapses and yields its space to its
    /// sibling (`stack_panel.rs:427-431`).
    ///
    /// It uses `toggle_dock`, which just flips `Dock::open` and re-subscribes
    /// nothing. The dock's WIDTH is deliberately fixed at construction: sizing
    /// it per visible-combination would mean re-running `set_right_dock`, whose
    /// `subscribe_item` leaks (see the mount site). So both panels open matches
    /// the old fixed docks exactly; one panel open is wider than it used to be,
    /// and the user can drag it back — which B9's `dock_layout` blob will then
    /// persist.
    /// B7: reconcile the LEFT dock with the three visibility bools, which are
    /// the single source of truth. Mirrors `sync_right_dock` exactly — runs in
    /// `render` because `toggle_dock` needs a `&mut Window`, guarded by a state
    /// tuple so it acts only on a real change, and it never re-runs
    /// `set_left_dock` (which leaks; see the mount site).
    ///
    /// WHICH panel shows is `Panel::visible`, read straight off these bools, so
    /// this only has to decide whether the dock as a whole is open. With the
    /// at-most-one invariant there is never more than one visible panel for
    /// upstream to choose between.
    fn sync_left_dock(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        let want = (
            self.catalog_panel_visible,
            self.connections_panel_visible,
            self.ai_panel_visible,
        );
        if want == self.left_dock_state {
            return;
        }
        self.left_dock_state = want;
        let Some(dock) = self.dock_area.clone() else {
            return;
        };
        let want_open = want.0 || want.1 || want.2;
        dock.update(cx, |dock, cx| {
            if dock.is_dock_open(gpui_component::dock::DockPlacement::Left, cx) != want_open {
                dock.toggle_dock(gpui_component::dock::DockPlacement::Left, window, cx);
            }
        });
    }

    fn sync_right_dock(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        let want = (self.inspector_panel_visible, self.chart_panel_visible);
        if want == self.right_dock_state {
            return;
        }
        self.right_dock_state = want;
        let Some(dock) = self.dock_area.clone() else {
            return;
        };
        let want_open = want.0 || want.1;
        dock.update(cx, |dock, cx| {
            if dock.is_dock_open(gpui_component::dock::DockPlacement::Right, cx) != want_open {
                dock.toggle_dock(gpui_component::dock::DockPlacement::Right, window, cx);
            }
        });
    }

    /// B6: the Charts panel's element tree, extracted from the body row's
    /// `.w(px(560.))` block so [`crate::panels::charts_panel::ChartsPanel`] can
    /// call it.
    ///
    /// The width and left border the block carried are the dock's job now, and
    /// the two export buttons moved to `ChartsPanel::toolbar_buttons`.
    pub(crate) fn render_charts_body(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .child(self.render_chart_toolbar(cx))
            .child(crate::charts::panel::render_chart_body(
                &self.chart_panel,
                self.chart_image.clone(),
                (520.0, 360.0),
                cx,
            ))
            .into_any_element()
    }

    /// B6: the Charts panel's visibility, read by `ChartsPanel::visible`.
    pub(crate) fn chart_visible(&self) -> bool {
        self.chart_panel_visible
    }

    /// B6: the Inspector's visibility, read by `InspectorPanel::visible`.
    ///
    /// The bool stays the single source of truth and the dock derives from it —
    /// see `sync_right_dock`. Deliberately a getter rather than making the field
    /// `pub(crate)`: the panel lives in another module and a getter keeps the
    /// direction of the dependency legible.
    pub(crate) fn inspector_visible(&self) -> bool {
        self.inspector_panel_visible
    }

    /// B8: whether the SQL console is showing — **derived from the dock**,
    /// never a parallel bool, and the one place in the dock series where the
    /// direction of the dependency is inverted.
    ///
    /// The left and right docks derive from shell bools because dat0 owns
    /// every writer. The bottom dock has two writers dat0 does NOT own: the
    /// title-bar collapse chevron (`tab_panel.rs:616`) and clicking a tab
    /// while the dock is collapsed (`tab_panel.rs:740-752`). Either flips
    /// `Dock::open` behind the shell's back, so a cached bool would desync and
    /// the next `SqlConsoleToggle` would move BACKWARDS — it would toggle a
    /// stale value. Making the dock the single source of truth removes the
    /// class rather than patching it.
    ///
    /// Safe to read in the same call that toggles: `Dock::set_open` assigns
    /// `self.open` synchronously and defers only `set_collapsed`
    /// (`dock.rs:259-266`), measured at B8's T0. `is_dock_open` returns false
    /// while `bottom_dock` is `None`, so the pre-mount state needs no special
    /// case — the dock does not exist until the console's first open.
    pub(crate) fn sql_console_visible(&self, cx: &gpui::App) -> bool {
        self.dock_area.as_ref().is_some_and(|d| {
            d.read(cx)
                .is_dock_open(gpui_component::dock::DockPlacement::Bottom, cx)
        })
    }

    /// B7: the Catalog panel's element tree, extracted from the body row so
    /// [`crate::panels::catalog_panel::CatalogPanel`] can call it.
    ///
    /// `&mut self` because `hero_focus_handle` needs it. Minting the
    /// `catalog-tree` handle HERE rather than threading one in keeps it on the
    /// shell's `hero_focus` map, so the same `FocusHandle` instance lands on the
    /// same element after the move to the dock — which is what keeps
    /// `catalog_nav` meaningful across this slice.
    pub(crate) fn render_catalog_body(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let catalog_fh = self.hero_focus_handle("catalog-tree", cx);
        crate::catalog::panel::render_catalog(
            &self.catalog_tree,
            &self.catalog_collapsed,
            self.catalog_active,
            &catalog_fh,
            cx,
        )
    }

    /// B7: the Connections panel's element tree, extracted from the body row.
    pub(crate) fn render_connections_body(
        &mut self,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        crate::connections::panel::render_connections(&self.connections, cx)
    }

    /// B7: the AI dock's element tree, extracted from the body row.
    ///
    /// Registering all eight ids unconditionally is fine — `HeroHandles::get` is
    /// only invoked by whichever buttons actually render, and `ai-key-forget` is
    /// only looked up when `key_set`.
    pub(crate) fn render_ai_body(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let ai_handles = {
            let ids: [&'static str; 8] = [
                "ai-toggle-enabled",
                "ai-provider-cycle",
                "ai-key-set",
                "ai-key-forget",
                "ai-model-set",
                "ai-toggle-advanced",
                "ai-toggle-sample-rows",
                "ai-test-connection",
            ];
            let mut map = std::collections::HashMap::new();
            for id in ids {
                map.insert(id, self.hero_focus_handle(id, cx));
            }
            crate::empty_state::HeroHandles { map }
        };
        crate::ai::panel::render_ai_panel(&self.ai_panel, &ai_handles, cx)
    }

    /// B7: read by `CatalogPanel::visible`. A getter rather than a `pub(crate)`
    /// field keeps the direction of the dependency legible (B6).
    pub(crate) fn catalog_visible(&self) -> bool {
        self.catalog_panel_visible
    }

    /// B7: read by `ConnectionsPanel::visible`.
    pub(crate) fn connections_visible(&self) -> bool {
        self.connections_panel_visible
    }

    /// B7: read by `AiDockPanel::visible`.
    pub(crate) fn ai_visible(&self) -> bool {
        self.ai_panel_visible
    }

    /// B7: move the rail's keyboard cursor. Clamps rather than wraps, matching
    /// the catalog tree.
    pub(crate) fn rail_move_cursor(&mut self, delta: isize, cx: &mut gpui::Context<Self>) {
        let len = crate::view::activity_rail::ITEMS.len() as isize;
        let next = (self.rail_cursor as isize + delta).clamp(0, len - 1);
        self.rail_cursor = next as usize;
        cx.notify();
    }

    /// B7: activate the panel under the cursor. Enter on the panel that is
    /// already open collapses the dock, matching what a click does.
    pub(crate) fn rail_activate_cursor(&mut self, cx: &mut gpui::Context<Self>) {
        let target = crate::view::activity_rail::ITEMS[self.rail_cursor].panel;
        self.activate_left_panel(target, cx);
    }

    /// B7: a click both moves the cursor and activates, so the two never drift
    /// after a mouse interaction.
    pub(crate) fn rail_click(&mut self, index: usize, cx: &mut gpui::Context<Self>) {
        self.rail_cursor = index.min(crate::view::activity_rail::ITEMS.len() - 1);
        self.rail_activate_cursor(cx);
    }

    /// B7: the rail's keyboard cursor, for `tests/left_dock.rs`.
    #[cfg(feature = "a11y-capture")]
    pub fn rail_cursor_for_test(&self) -> usize {
        self.rail_cursor
    }

    /// B7: the ONLY writer of the three left-panel bools.
    ///
    /// Being the only writer is what makes the at-most-one-visible invariant
    /// structural rather than a convention every call site has to remember.
    /// Upstream paints a horizontal tab bar the moment two panels in a
    /// `DockItem::tabs` are visible (`tab_panel.rs:623-625`), which would put a
    /// second selector right beside the activity rail.
    fn set_left_panel_exclusive(&mut self, target: Option<LeftPanel>) {
        self.catalog_panel_visible = target == Some(LeftPanel::Catalog);
        self.connections_panel_visible = target == Some(LeftPanel::Connections);
        self.ai_panel_visible = target == Some(LeftPanel::Ai);
    }

    /// B7: is this left panel the one currently showing?
    pub fn left_panel_visible(&self, p: LeftPanel) -> bool {
        match p {
            LeftPanel::Catalog => self.catalog_panel_visible,
            LeftPanel::Connections => self.connections_panel_visible,
            LeftPanel::Ai => self.ai_panel_visible,
        }
    }

    /// B7: the open left panel, or `None` when the dock is collapsed.
    pub fn open_left_panel(&self) -> Option<LeftPanel> {
        LeftPanel::ALL
            .iter()
            .copied()
            .find(|p| self.left_panel_visible(*p))
    }

    /// B7: the user-facing left-panel transition, and the only one production
    /// code should call.
    ///
    /// Activating the panel that is already open collapses the dock — the
    /// VSCode behaviour — and it falls out of the invariant rather than being a
    /// special case.
    ///
    /// The side effects of a left panel BECOMING VISIBLE: Catalog refreshes so
    /// the dock always shows fresh tables, AI hydrates its draft from settings
    /// plus keychain.
    ///
    /// B7 folded these out of the individual toggle handlers and into
    /// `activate_left_panel` so that no entry point could lose them. B9 adds a
    /// SECOND entry point — the constructor seeds the visibility bools straight
    /// from the persisted layout, which never goes through `activate_left_panel`
    /// — so they move again, into a helper both call. Duplicating the match
    /// would have re-created exactly the drift B7 removed.
    pub(crate) fn on_left_panel_shown(&mut self, target: LeftPanel, cx: &mut gpui::Context<Self>) {
        match target {
            LeftPanel::Catalog => self.refresh_catalog(cx),
            LeftPanel::Ai => self.hydrate_ai_panel(),
            LeftPanel::Connections => {}
        }
    }

    pub fn activate_left_panel(&mut self, target: LeftPanel, cx: &mut gpui::Context<Self>) {
        let already_open = self.left_panel_visible(target);
        self.set_left_panel_exclusive((!already_open).then_some(target));
        if !already_open {
            self.on_left_panel_shown(target, cx);
        }
        // Session v11 persists the whole rail choice, not just the catalog:
        // `dock_layout.left_panel` is `Option<LeftPanel>`, so the at-most-one
        // invariant is unrepresentable-if-violated on the wire.
        self.persist_dock_ui();
        self.persist_dock_layout(cx);
        cx.notify();
    }

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
