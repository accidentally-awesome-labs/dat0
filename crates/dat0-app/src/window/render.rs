//! `Render` for `WorkspaceShell`: the frame the window paints each time
//! gpui asks for one, plus the grid body it hosts.
//!
//! A foreign trait implemented on a local type is coherent per crate, not
//! per module, so this lives here rather than in `mod.rs` — which leaves
//! `mod.rs` as the shell's type, constructor, and view wiring.

use super::*;

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
