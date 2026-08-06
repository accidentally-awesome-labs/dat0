//! The DockArea (B5 through B9): building it, the left rail and its panels,
//! the right and bottom docks, the panel body renders, and B9's layout
//! capture and persist.
//!
//! `ensure_dock_area` builds the left dock as a split of three single-panel
//! tabs rather than one three-panel tab group: every `add_panel` after the
//! first reads `Panel::visible`, which re-enters the shell while it is
//! already leased, and panics. B7 found that the hard way.

use super::*;

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
pub(super) const SQL_CONSOLE_DOCK_HEIGHT: f32 = 320.0;

impl WorkspaceShell {
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
    pub(super) fn persist_dock_layout_seed(&self, cx: &gpui::App) {
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
    pub(super) fn window_disc(&self) -> String {
        self.session.lock().window_id.to_string()[..4].to_string()
    }

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
    pub(super) fn hero_focus_handle(
        &mut self,
        id: &'static str,
        cx: &mut gpui::App,
    ) -> gpui::FocusHandle {
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
    pub(super) fn ensure_dock_area(
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
    pub(super) fn sync_left_dock(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
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

    pub(super) fn sync_right_dock(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
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
    pub(super) fn set_left_panel_exclusive(&mut self, target: Option<LeftPanel>) {
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
}
