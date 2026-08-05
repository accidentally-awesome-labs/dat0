//! Test-only accessors on `WorkspaceShell`, gated behind `a11y-capture`.
//!
//! Integration tests live in another crate and cannot see `pub(crate)`
//! fields, so these shims expose the chart / inspector / catalog / dock /
//! modal state they assert on. Identity-gated behind the feature — zero
//! surface in release builds.
//!
//! B11 kept these as ONE block rather than scattering them across the
//! modules they reach into: 118 test binaries depend on this surface, and
//! one file keeps it auditable. It also makes clippy's
//! `items-after-test-module` ordering constraint structural rather than
//! hand-maintained: there is no `#[cfg(test)] mod` in this file for it to
//! be ordered against.

use super::*;

// UAT (Charts save/persist/lineage slice) T0: test-only shims exposing the
// `pub(crate)` chart/inspector/catalog state needed by `tests/chart_uat_window.rs`
// (an integration-test crate, so it cannot see `pub(crate)` fields directly).
// Identity-gated behind `a11y-capture` — zero surface in release builds.
// B11: the ordering rule this comment used to enforce by hand — keep the
// block ahead of any `#[cfg(test)] mod`, because clippy's
// `items-after-test-module` (under `-D warnings`) rejects items that follow a
// test module — is now structural. This file contains no test module.
#[cfg(feature = "a11y-capture")]
impl WorkspaceShell {
    /// B5: has the shell built its DockArea? The dock is an implementation
    /// detail of `render`, and the integration tests live in another crate.
    pub fn dock_mounted_for_test(&self) -> bool {
        self.dock_area.is_some()
    }

    /// B6: hide both right-dock panels — the reverse of the `seed_*` /
    /// `chart_bind_*` helpers above, which only ever show them.
    ///
    /// Without a way to drive the reconcile loop DOWN as well as up, a
    /// `sync_right_dock` that could only ever open the dock would pass every
    /// other test in `tests/right_dock.rs`.
    pub fn hide_right_dock_panels_for_test(&mut self, cx: &mut Context<Self>) {
        self.inspector_panel_visible = false;
        self.chart_panel_visible = false;
        cx.notify();
    }

    /// B6: is the right dock open? Derived from the dock itself rather than the
    /// bools, so a test asserting on it is checking that `sync_right_dock`
    /// actually ran — not just re-reading the input it was given.
    /// B7: the DOCK's own open flag, deliberately not the bools the test wrote
    /// — re-reading those would prove only that assignment works, not that
    /// `sync_left_dock` ran.
    pub fn left_dock_open_for_test(&self, cx: &gpui::App) -> bool {
        self.dock_area
            .as_ref()
            .map(|d| {
                d.read(cx)
                    .is_dock_open(gpui_component::dock::DockPlacement::Left, cx)
            })
            .unwrap_or(false)
    }

    pub fn right_dock_open_for_test(&self, cx: &gpui::App) -> bool {
        self.dock_area
            .as_ref()
            .map(|d| {
                d.read(cx)
                    .is_dock_open(gpui_component::dock::DockPlacement::Right, cx)
            })
            .unwrap_or(false)
    }

    /// B8: is the SQL console's bottom dock open? Same shape as the left and
    /// right accessors above — the DOCK's own flag, never a bool the test
    /// wrote.
    pub fn bottom_dock_open_for_test(&self, cx: &gpui::App) -> bool {
        self.sql_console_visible(cx)
    }

    /// B8: the ⌘⇧C / menu / palette path, for tests.
    ///
    /// `open_console_for_test` only ever OPENS (it early-returns when the
    /// console is already visible), so it cannot exercise the close half or the
    /// direction of a toggle after an external one.
    pub fn toggle_sql_console_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toggle_sql_console(window, cx);
    }

    /// B8: the shell's `DockArea`, so a test can drive the two toggle paths
    /// dat0 does NOT own.
    ///
    /// Upstream's title-bar chevron and its click-a-tab-while-collapsed handler
    /// both do exactly `dock_area.toggle_dock(DockPlacement::Bottom, ..)`
    /// (`tab_panel.rs:746-751`). Handing the `DockArea` to a test lets it make
    /// that same call rather than hunting for the chevron's pixels, which carry
    /// no debug selector.
    pub fn dock_area_for_test(&self) -> Option<gpui::Entity<gpui_component::dock::DockArea>> {
        self.dock_area.clone()
    }

    /// B9: the menu / palette Charts toggle, for tests.
    ///
    /// [`chart_bind_for_test`](Self::chart_bind_for_test) assigns
    /// `chart_panel_visible` directly — the a11y-shim pattern, where a test
    /// writes the bool and the next frame reconciles the dock. That bypasses
    /// [`toggle_chart_panel`](Self::toggle_chart_panel) and therefore also
    /// bypasses the layout persist inside it, so it cannot exercise this path.
    pub fn toggle_chart_panel_for_test(&mut self, cx: &mut Context<Self>) {
        self.toggle_chart_panel(cx);
    }

    pub fn chart_bind_for_test(&mut self, source: String, cols: Vec<(String, String)>) {
        self.chart_panel.bind(source, cols);
        self.chart_panel_visible = true;
    }
    pub fn chart_set_axes_for_test(
        &mut self,
        chart_type: crate::charts::spec::ChartType,
        x: Option<String>,
        y: Option<String>,
        title: String,
    ) {
        self.chart_panel.spec.chart_type = chart_type;
        self.chart_panel.spec.x = x;
        self.chart_panel.spec.y = y;
        self.chart_panel.spec.title = title;
    }
    pub fn chart_visible_for_test(&self) -> bool {
        self.chart_panel_visible
    }
    pub fn chart_spec_for_test(&self) -> crate::charts::spec::ChartSpec {
        self.chart_panel.spec.clone()
    }
    pub fn save_named_chart_for_test(&mut self, name: String, cx: &mut Context<Self>) {
        self.save_named_chart(name, cx);
    }
    pub fn seed_catalog_for_test(&mut self, tables: Vec<dat0_engine::TableInfo>) {
        self.catalog_tables = tables;
    }
    pub fn catalog_active_for_test(&self) -> usize {
        self.catalog_active
    }
    pub fn catalog_collapsed_for_test(&self) -> Vec<String> {
        let mut v: Vec<String> = self.catalog_collapsed.iter().cloned().collect();
        v.sort();
        v
    }
    /// Build the catalog tree DIRECTLY from seeded fakes and show the catalog dock.
    /// Bypasses `refresh_catalog`'s off-thread `get_tables` (window.rs:2999), which
    /// would clobber the fakes with the empty test engine's real (empty) tables.
    /// Seed an `md:`-origin `TableInfo` to populate the "Cloud" group.
    pub fn seed_catalog_tree_for_test(&mut self, tables: Vec<dat0_engine::TableInfo>) {
        self.catalog_tree = crate::catalog::CatalogTree::build(&tables);
        // B7: through the single writer, so the at-most-one-visible invariant
        // holds for tests too — but NOT through `activate_left_panel`, whose
        // `refresh_catalog` is exactly what this shim exists to bypass.
        self.set_left_panel_exclusive(Some(LeftPanel::Catalog));
    }
    /// Show the Connections dock and hand back the `ConnectionManager` so the test
    /// can drive `set_md_status` / `set_md_test_result` / `set_md_databases` (all
    /// already `pub`). No live connection, token, or keychain touched.
    pub fn open_connections_for_test(&mut self) -> &mut crate::connections::ConnectionManager {
        // B7: via the single writer — see `seed_catalog_tree_for_test`.
        self.set_left_panel_exclusive(Some(LeftPanel::Connections));
        &mut self.connections
    }
    /// Build + show the SQL console, then seed the timing chip's elapsed + routing
    /// so the chip renders its routing suffix without a real query run. The console
    /// is lazily built by `toggle_sql_console` (needs `&mut Window`); `set_last_elapsed`
    /// (sql_console.rs:340) sets `last_elapsed_ms` + `last_routing`, which is all the
    /// chip's render gate `(running == false, Some(ms))` needs.
    pub fn seed_routing_chip_for_test(
        &mut self,
        ms: u64,
        routing: crate::connections::routing::Routing,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if !self.sql_console_visible(cx) {
            self.toggle_sql_console(window, cx);
        }
        if let Some(console) = self.sql_console.clone() {
            console.update(cx, |c, cx| c.set_last_elapsed(ms, routing, cx));
        }
    }
    pub fn seed_lineage_target_for_test(&mut self, name: String, cx: &mut Context<Self>) {
        self.inspector.set_target(name);
        self.recompute_lineage();
        self.inspector_panel_visible = true;
        cx.notify();
    }
    pub fn open_saved_chart_for_test(
        &mut self,
        name: String,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.open_saved_chart(name, window, cx);
    }
    /// Slice 6 Task 3: the grid's live active cell, read straight off the
    /// shell's own `SelectionModel` — there is no separate `GridView` entity;
    /// `selection` lives directly on `WorkspaceShell` (see the field above),
    /// lazily built once a non-empty data source is mounted (`render`).
    pub fn grid_active_cell_for_test(&self) -> crate::grid::selection::CellCoord {
        self.selection
            .as_ref()
            .expect(
                "grid_active_cell_for_test called with no SelectionModel mounted \
                 (no non-empty data source bound yet?)",
            )
            .active()
    }
    /// Cell-editor coverage slice: is the inline cell editor currently mounted?
    pub fn cell_editor_open_for_test(&self) -> bool {
        self.cell_editor.is_some()
    }

    /// The live inline cell-editor entity (to reach its inner `InputState` /
    /// column type from a test). `None` when no editor is mounted.
    pub fn cell_editor_for_test(&self) -> Option<Entity<crate::grid::cell_editor::CellEditor>> {
        self.cell_editor.clone()
    }

    /// Read a rendered cell's display string off the LIVE data source (which, after
    /// a commit, is the rebound overlay view), by screen `(row, visible-col)`.
    /// `None` when no data source is mounted or the cell isn't resident.
    pub fn cell_display_for_test(&self, row: usize, col: usize) -> Option<String> {
        self.data_source.as_ref()?.cell_display(row, col)
    }

    /// Cell-editor coverage slice (T0 gate finding, NOT in the original plan's
    /// accessor list): install a fresh `ViewModel` for `table` so cell-edit
    /// commits aren't silently no-op'd by `commit_cell_edit`'s
    /// `self.view_model.is_some()` guard. `set_data_source` deliberately never
    /// touches `view_model` (T13: one `ViewModel` at a time, orthogonal to which
    /// data source is bound), so a grid mounted via `set_data_source` alone
    /// (sufficient for nav-only tests like `keyboard_nav.rs`'s grid test) cannot
    /// commit an edit. Mirrors the production `open_table_tab` /
    /// `route_drop_outcomes` pattern exactly (`ViewModel::new(name, quoted)`) —
    /// both of those are `pub(crate)`/private and unreachable from this
    /// integration-test crate, so there is no existing public path to seed this.
    pub fn seed_view_model_for_test(&mut self, table: impl Into<String>) {
        let table = table.into();
        let quoted = format!("\"{}\"", table.replace('"', "\"\""));
        self.view_model = Some(crate::view::ViewModel::new(table, quoted));
    }
    /// Test oracle for recents-list arrow nav (mirrors `grid_active_cell_for_test`).
    #[cfg(feature = "a11y-capture")]
    pub fn recents_active_for_test(&self) -> usize {
        self.recents_active
    }
    /// Seed the AI dock draft state directly (bypassing `hydrate_ai_panel`, which
    /// probes the OS keychain + settings.toml — the hermeticity trap) and open the
    /// dock. Test-only.
    #[cfg(feature = "a11y-capture")]
    pub fn seed_ai_panel_for_test(&mut self, panel: crate::ai::panel::AiPanel) {
        self.ai_panel = panel;
        // B7: NOT `activate_left_panel` — that calls `hydrate_ai_panel`, which
        // probes the OS keychain plus settings.toml and is the hermeticity trap
        // this shim exists to avoid.
        self.set_left_panel_exclusive(Some(LeftPanel::Ai));
    }
    /// Read the AI dock's draft `enabled` flag (proves Enter-operability flipped it).
    #[cfg(feature = "a11y-capture")]
    pub fn ai_panel_enabled_for_test(&self) -> bool {
        self.ai_panel.enabled
    }
    /// Toggle the SQL console visible, set its `ai_ready` gate, and return the console
    /// entity so a test can subscribe to its `SqlConsoleEvent`s. Test-only.
    ///
    /// `ai_ready` is the gate the NL→SQL chip and the Explain button key their
    /// `if enabled` branch on: `true` → they render as real keyboard controls (tab
    /// stops with an `.a11y` twin); `false` → they stay plain, non-interactive divs
    /// with no `focus_stop` and no a11y node. Both arms are exercised by tests.
    #[cfg(feature = "a11y-capture")]
    pub fn open_console_for_test(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        ai_ready: bool,
    ) -> gpui::Entity<crate::view::sql_console::SqlConsole> {
        if !self.sql_console_visible(cx) {
            self.toggle_sql_console(window, cx);
        }
        let console = self.sql_console.clone().expect("console built by toggle");
        console.update(cx, |c, _cx| c.ai_ready = ai_ready);
        console
    }

    /// [`open_console_for_test`](Self::open_console_for_test) with AI ready — the
    /// common case (chip + Explain are operable tab stops).
    #[cfg(feature = "a11y-capture")]
    pub fn open_console_ready_for_test(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Entity<crate::view::sql_console::SqlConsole> {
        self.open_console_for_test(window, cx, true)
    }

    /// Open a `NamePrompt` from a test using a side-effect-free intent
    /// (`SaveQuery` with no stashed SQL → `Confirm` is a clean no-op dismiss),
    /// so the generic prompt keyboard behavior can be driven without AI/engine.
    pub fn open_name_prompt_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name_prompt_sql = None;
        self.open_name_prompt_with("Test", "", NamePromptIntent::SaveQuery, window, cx);
    }

    /// Whether the name-prompt overlay is currently mounted.
    pub fn name_prompt_open_for_test(&self) -> bool {
        self.name_prompt.is_some()
    }

    /// How many modals are mounted — the B1 single-modal invariant.
    pub fn open_modal_count_for_test(&self, cx: &App) -> usize {
        self.open_modal_count(cx)
    }

    /// The live command palette (B4), so a test can read its active row or
    /// assert it is mounted/dismissed.
    pub fn command_palette_for_test(
        &self,
    ) -> Option<gpui::Entity<crate::view::command_palette::CommandPalette>> {
        self.command_palette.clone()
    }

    /// Unmount every modal and restore focus, so a test that walks several
    /// routed commands in a loop never violates the single-modal invariant
    /// between iterations. Mirrors what each dismiss arm does, minus the
    /// per-modal event routing.
    pub fn dismiss_all_modals_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.name_prompt = None;
        self.name_prompt_sub = None;
        self.md_token_prompt = None;
        self.md_token_prompt_sub = None;
        self.ai_entry_prompt = None;
        self.ai_entry_prompt_sub = None;
        self.export_dialog = None;
        self.export_dialog_sub = None;
        self.saved_picker = None;
        self.saved_picker_sub = None;
        self.command_palette = None;
        self.command_palette_sub = None;
        self.restore_modal_focus(window);
        debug_assert_eq!(self.open_modal_count(cx), 0, "a modal slot was missed");
        cx.notify();
    }

    /// Whether the SQL console is mounted — proves a routed `console.toggle`
    /// did real work rather than logging a breadcrumb.
    pub fn sql_console_is_mounted_for_test(&self) -> bool {
        self.sql_console.is_some()
    }

    /// Run a palette command through the production router (B4 T4).
    pub fn run_palette_action_for_test(
        &mut self,
        id: &crate::actions::ActionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.run_palette_action(id, window, cx)
    }

    /// The live prompt entity — lets a test subscribe to its `NamePromptEvent`
    /// and read/seed its input.
    pub fn name_prompt_entity_for_test(
        &self,
    ) -> Option<gpui::Entity<crate::view::name_prompt::NamePrompt>> {
        self.name_prompt.clone()
    }

    /// Open the export dialog the way `view_actions::dispatch_export` does —
    /// with NO `Window`, which is the whole point of the render-drain.
    ///
    /// The production path returns early when no `ViewModel` is mounted; a nav
    /// test has no file loaded, so this builds the entity directly. Everything
    /// the trap and the keyboard path touch is identical.
    pub fn open_export_dialog_for_test(&mut self, cx: &mut Context<Self>) {
        use crate::view::export_dialog::{ExportDialog, ExportEvent};
        let dialog = cx.new(ExportDialog::new);
        let sub = cx.subscribe(&dialog, |ws: &mut Self, _dialog, ev: &ExportEvent, cx| {
            ws.route_export_event(ev.clone(), cx);
        });
        self.export_dialog_sub = Some(sub);
        self.export_dialog = Some(dialog);
        self.pending_modal_focus = true;
        cx.notify();
    }

    /// The mounted export dialog, so a test can reach its focus handles.
    pub fn export_dialog_entity_for_test(
        &self,
    ) -> Option<gpui::Entity<crate::view::export_dialog::ExportDialog>> {
        self.export_dialog.clone()
    }

    /// Mount the saved-query picker. `show_saved_picker` itself is
    /// `pub(crate)`, and the integration tests are a separate crate.
    pub fn show_saved_picker_for_test(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_saved_picker(window, cx);
    }

    /// The mounted saved-query picker, so a test can reach its focus handles
    /// and active index.
    pub fn saved_picker_entity_for_test(
        &self,
    ) -> Option<gpui::Entity<crate::view::saved_query_picker::SavedQueryPicker>> {
        self.saved_picker.clone()
    }
}
