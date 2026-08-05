//! Modal mounting and the surfaces that use it: the B1/B2 `ModalHost` name
//! prompt, the saved-query picker, and B4's command palette.
//!
//! `push_modal` is the single mount point; the B1/B2 single-modal invariant
//! and the focus restore that pairs with it live here too, which is what
//! makes this a topic rather than a grab-bag.

use super::*;

/// Session-backed workspace shell rendered inside `gpui_component::Root`.
///
/// What a confirmed [`NamePrompt`](crate::view::name_prompt::NamePrompt)
/// should do (P5b T8 + T10). The shared single-line name modal is reused for
/// several "name this thing" flows; the intent is the single routing point for
/// the `Confirm(name)` arm in
/// [`on_name_prompt_event`](WorkspaceShell::on_name_prompt_event), so adding a
/// new flow is a new variant + a new match arm — nothing else moves.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NamePromptIntent {
    /// Save the captured SQL (`name_prompt_sql`) as a named saved query (T8).
    SaveQuery,
    /// Promote the console statement-under-cursor to a derived table (T10).
    SaveConsoleAsTable,
    /// Promote the active grid view's transform stack to a derived table,
    /// recording its lineage as `DerivedOrigin::Transform { parent, ops }`
    /// (T11). The handler re-reads the `ViewModel` on confirm, so no per-intent
    /// state is captured up front.
    SaveViewAsTable,
    /// Save the currently-rendered chart under a user name (P9a-2). The
    /// generated default is seeded into the prompt by the opener
    /// (`open_chart_save_prompt`), so the confirm handler just reads the edited
    /// `name` — no per-intent state is captured here.
    SaveChart,
    /// NL prompt confirmed — `spawn_ai_nl2sql` gets the entered text as the NL
    /// prompt (P9c-2 T6).
    Nl2SqlPrompt,
}

/// Owns the session for this window and an optional data source (set once
/// the user drops a file or opens a table). When no data source is present,
/// renders a "Drop a file here" placeholder. When a data source is present
/// the shell mounts a real `gpui_component::table::Table` over a
/// [`GridTableDelegate`] wrapper (P3b T4 — closes the P3a T10 placeholder).
///
/// `table_state` is built lazily on the first render after `set_data_source`
/// — `TableState::new` requires `&mut Window`, which is only available
/// inside `Render::render`. The drop handler runs off-thread and so cannot
/// touch the window; it just stores the new `Arc<GridDataSource>` and asks
/// the view to re-render via `cx.notify()`. The next frame promotes that
/// `Arc` into an `Entity<TableState<…>>`.
/// One mounted modal: everything `render`, the Tab trap and the modal count
/// need, collected by [`WorkspaceShell::mounted_modals`] (B2).
pub(super) struct MountedModal {
    /// Static id for the `Dialog` a11y node `overlay::modal_host` paints.
    pub(super) a11y_id: &'static str,
    /// Accessible name of that node.
    pub(super) title: gpui::SharedString,
    /// The modal's stops in VISUAL order — the trap's source of truth.
    pub(super) focus_order: Vec<gpui::FocusHandle>,
    /// The modal body, ready to hand to `modal_host`.
    pub(super) content: gpui::AnyElement,
}

/// Push `slot`'s modal onto `out` if it is mounted.
///
/// Generic over the entity type so each call monomorphizes — no `dyn`, no
/// boxing at the slot level. `into_any_element` only WRAPS the entity; it does
/// not render it, so building this list to count modals is cheap.
fn push_modal<T: crate::overlay::ModalContent + Render>(
    out: &mut Vec<MountedModal>,
    a11y_id: &'static str,
    slot: &Option<Entity<T>>,
    cx: &App,
) {
    if let Some(entity) = slot {
        let view = entity.read(cx);
        out.push(MountedModal {
            a11y_id,
            title: view.modal_title(cx),
            focus_order: view.modal_focus_order(cx),
            content: entity.clone().into_any_element(),
        });
    }
}

impl WorkspaceShell {
    /// Mount the Save-query name-prompt overlay (P5b T8). Thin wrapper over the
    /// generalized [`open_name_prompt_with`](Self::open_name_prompt_with): it
    /// captures the active tab's SQL (held in `name_prompt_sql` so a later
    /// Confirm saves THAT text, not whatever is in the editor by then) and opens
    /// the modal with the [`SaveQuery`](NamePromptIntent::SaveQuery) intent.
    pub(crate) fn open_name_prompt(
        &mut self,
        sql: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.name_prompt_sql = Some(sql);
        self.open_name_prompt_with(
            "Save query as…",
            "",
            NamePromptIntent::SaveQuery,
            window,
            cx,
        );
    }

    /// Mount the shared single-line name-prompt overlay for a given `intent`
    /// (P5b T8 generalized; T10). The `intent` is the ONLY thing that varies the
    /// Confirm behaviour — it is stashed in `name_prompt_intent` and matched in
    /// [`on_name_prompt_event`](Self::on_name_prompt_event).
    ///
    /// Mirrors [`open_export_dialog`](Self::open_export_dialog): build the entity
    /// via `cx.new`, subscribe to its `NamePromptEvent`, and STORE the
    /// subscription in `name_prompt_sub` (a dropped `Subscription` deregisters
    /// the callback silently — the P4a T10b trap).
    ///
    /// Per-intent inputs (e.g. the captured SQL for `SaveQuery`) are set by the
    /// caller BEFORE calling this; the `SaveConsoleAsTable` intent needs none
    /// (it re-reads the statement-under-cursor on confirm).
    ///
    /// `initial` seeds the name field (editable). Pass `""` for the flows that
    /// start blank (Save query / Save as table); the Save-chart flow passes the
    /// generated default name (P9a-2).
    ///
    /// Needs `&mut Window` because `NamePrompt::new` builds an `InputState`
    /// (single-line name field) eagerly.
    pub(super) fn open_name_prompt_with(
        &mut self,
        title: impl Into<gpui::SharedString>,
        initial: impl Into<gpui::SharedString>,
        intent: NamePromptIntent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::view::name_prompt::{NamePrompt, NamePromptEvent};
        // B1: remember where focus was BEFORE `NamePrompt::new` moves it to the
        // field, so dismissing the modal can hand focus back.
        self.modal_restore_focus = window.focused(cx);
        let prompt = cx.new(|cx| NamePrompt::new(title, initial, window, cx));
        // `subscribe_in` (not `subscribe`) so the dismiss path has a `&mut
        // Window` for the B1 focus restore — the form the AI and MotherDuck
        // prompts already use.
        let sub = cx.subscribe_in(
            &prompt,
            window,
            |ws: &mut Self, _prompt, ev: &NamePromptEvent, window, cx| {
                ws.on_name_prompt_event(ev.clone(), window, cx);
            },
        );
        self.name_prompt_sub = Some(sub);
        self.name_prompt_intent = Some(intent);
        self.name_prompt = Some(prompt);
        debug_assert!(
            self.open_modal_count(cx) <= 1,
            "two modals mounted at once ({}) — B1 assumes a single modal; see \
             docs/plans/2026-07-28-dat0-ui-redesign-b1-modal-host-design.md §2.7",
            self.open_modal_count(cx)
        );
        cx.notify();
    }

    /// Return focus to the stop that held it before the modal opened (B1). No-op
    /// when nothing was focused (e.g. the modal was opened from a menu action).
    pub(super) fn restore_modal_focus(&mut self, window: &mut Window) {
        if let Some(fh) = self.modal_restore_focus.take() {
            window.focus(&fh);
        }
    }

    /// EVERY mounted modal, in priority order (B2). The single source of truth:
    /// the render mount, `overlay::modal_trap`'s focus order and
    /// [`open_modal_count`](Self::open_modal_count) all derive from this list.
    ///
    /// B1 kept three hand-maintained places in sync instead — an `or` chain, a
    /// count, and the mount site — so a modal added to one but not the others
    /// was styled by `modal_host` yet silently NOT trapped, and two of the three
    /// edits were invisible to the compiler. Adding a modal is now ONE line
    /// here.
    pub(super) fn mounted_modals(&self, cx: &App) -> Vec<MountedModal> {
        let mut v = Vec::new();
        push_modal(&mut v, "name-prompt-modal", &self.name_prompt, cx);
        push_modal(&mut v, "md-token-prompt-modal", &self.md_token_prompt, cx);
        push_modal(&mut v, "ai-entry-prompt-modal", &self.ai_entry_prompt, cx);
        push_modal(&mut v, "export-modal", &self.export_dialog, cx);
        push_modal(&mut v, "saved-picker-modal", &self.saved_picker, cx);
        push_modal(&mut v, "command-palette-modal", &self.command_palette, cx);
        v
    }

    /// How many modals are mounted. Load-bearing for the trap, not just
    /// hygiene: `render` traps `mounted_modals().first()`, so a second mounted
    /// modal would be the one NOT trapped. Each open path `debug_assert!`s this
    /// is never > 1, rather than the app growing a modal stack nothing needs.
    pub(crate) fn open_modal_count(&self, cx: &App) -> usize {
        self.mounted_modals(cx).len()
    }

    /// Route a `NamePromptEvent` from the shared name modal (P5b T8 + T10).
    /// `Confirm` dispatches on the stored [`NamePromptIntent`] to the right
    /// handler (the single routing point — a new flow is one new arm here);
    /// `Cancel` just dismisses. Either way the entity + subscription + per-intent
    /// state are dropped (closes the overlay).
    fn on_name_prompt_event(
        &mut self,
        ev: crate::view::name_prompt::NamePromptEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::view::name_prompt::NamePromptEvent;
        if let NamePromptEvent::Confirm(name) = ev {
            match self.name_prompt_intent {
                Some(NamePromptIntent::SaveQuery) => {
                    if let Some(sql) = self.name_prompt_sql.clone() {
                        self.save_named_query(name, sql, cx);
                    }
                }
                Some(NamePromptIntent::SaveConsoleAsTable) => {
                    self.save_console_as_table(name, cx);
                }
                Some(NamePromptIntent::SaveViewAsTable) => {
                    self.save_view_as_table(name, cx);
                }
                Some(NamePromptIntent::SaveChart) => {
                    self.save_named_chart(name, cx);
                }
                Some(NamePromptIntent::Nl2SqlPrompt) => {
                    self.spawn_ai_nl2sql(name, cx);
                }
                None => {}
            }
        }
        self.name_prompt = None;
        self.name_prompt_sub = None;
        self.name_prompt_sql = None;
        self.name_prompt_intent = None;
        self.restore_modal_focus(window);
        cx.notify();
    }

    /// Mount the saved-query picker modal (P5b T8, rebuilt in B2).
    ///
    /// Takes a `&mut Window` — unlike the export dialog, this path has one (its
    /// only caller is the `ShowSaved` console-event arm, right next to the
    /// `SaveQuery` arm that already passes `window` to `open_name_prompt`), so
    /// it captures the restore target and focuses the list directly rather than
    /// going through the render drain.
    pub(crate) fn show_saved_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::view::saved_query_picker::{SavedQueryPicker, SavedQueryPickerEvent};
        self.modal_restore_focus = window.focused(cx);
        let session = self.session.clone();
        let picker = cx.new(|cx| SavedQueryPicker::new(session, cx));
        // `subscribe_in` (not `subscribe`) so the dismiss path has a `&mut
        // Window` for the focus restore.
        let sub = cx.subscribe_in(
            &picker,
            window,
            |ws: &mut Self, _p, ev: &SavedQueryPickerEvent, window, cx| {
                ws.on_saved_picker_event(ev.clone(), window, cx);
            },
        );
        window.focus(&picker.read(cx).list_focus_handle());
        self.saved_picker_sub = Some(sub);
        self.saved_picker = Some(picker);
        debug_assert!(
            self.open_modal_count(cx) <= 1,
            "two modals mounted at once ({}) — B1 assumes a single modal; see \
             docs/plans/2026-07-28-dat0-ui-redesign-b1-modal-host-design.md §2.7",
            self.open_modal_count(cx)
        );
        cx.notify();
    }

    /// Ask for the command palette on the next frame (B4).
    ///
    /// Windowless by construction: the only caller is the global ⌘⇧P handler in
    /// `command_palette::open`, which reaches this shell through
    /// `focused_workspace_weak` with nothing but a `&mut App`.
    pub(crate) fn request_command_palette(&mut self, cx: &mut Context<Self>) {
        self.pending_palette_open = true;
        cx.notify();
    }

    /// Mount the palette. Called from `render`, which is where the `&mut Window`
    /// that `InputState::new` needs comes from.
    pub(super) fn mount_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use crate::view::command_palette::{CommandPalette, CommandPaletteEvent};
        let Some(reg) = crate::window_registry::action_registry().cloned() else {
            tracing::warn!("command palette: no ActionRegistry installed; not opening");
            return;
        };
        let palette = cx.new(|cx| CommandPalette::new(reg, window, cx));
        // `subscribe_in` (not `subscribe`) so the dismiss path has a `&mut
        // Window` for the focus restore and for the routed actions.
        let sub = cx.subscribe_in(
            &palette,
            window,
            |ws: &mut Self, _p, ev: &CommandPaletteEvent, window, cx| {
                ws.on_command_palette_event(ev.clone(), window, cx);
            },
        );
        self.command_palette_sub = Some(sub);
        self.command_palette = Some(palette);
        debug_assert!(
            self.open_modal_count(cx) <= 1,
            "two modals mounted at once ({}) — B1 assumes a single modal; see \
             docs/plans/2026-07-28-dat0-ui-redesign-b1-modal-host-design.md §2.7",
            self.open_modal_count(cx)
        );
    }

    /// Route a `CommandPaletteEvent` (B4).
    ///
    /// ⚠ ORDER IS LOAD-BEARING: dismiss BEFORE running, for two independent
    /// reasons. `sql.save_query` and `sql.save_as_table` open a `NamePrompt`, and
    /// with the palette still mounted that is two modals — which the
    /// `debug_assert!` above rejects and which would leave the second one
    /// untrapped in release. And `InputState::enter` propagates on a single-line
    /// field, so the Enter that got us here also drops a `"\n"` into the query
    /// buffer; unmounting first means that buffer is never rendered again
    /// (gpui's text system panics on a newline in single-line text).
    fn on_command_palette_event(
        &mut self,
        ev: crate::view::command_palette::CommandPaletteEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::view::command_palette::CommandPaletteEvent as E;
        let run = match ev {
            E::Run(id) => Some(id),
            E::Cancel => None,
        };
        self.command_palette = None;
        self.command_palette_sub = None;
        self.restore_modal_focus(window);
        let Some(id) = run else {
            cx.notify();
            return;
        };
        if !self.run_palette_action(&id, window, cx) {
            // Not window-routed: the registry closure is the real handler.
            //
            // ⚠ It MUST be deferred. We are inside this shell's own update, and
            // most registry closures reach the focused workspace right back
            // (`dispatch_undo`, `dispatch_export`, `dispatch_visualize`, …) —
            // calling one synchronously panics with "cannot read WorkspaceShell
            // while it is already being updated". `App::defer` runs it once this
            // update has finished, outside any entity borrow. Measured, not
            // theorised: `enter_runs_a_command_through_the_real_keymap` panicked
            // exactly this way before the defer went in.
            match crate::window_registry::action_registry().and_then(|r| r.get(&id)) {
                Some(desc) => {
                    let dispatch = desc.dispatch.clone();
                    cx.defer(move |app| dispatch(app));
                }
                None => tracing::warn!("command palette: no descriptor for {id}"),
            }
        }
        cx.notify();
    }

    /// Run a `WINDOW_ROUTED` palette command with the `&mut Window` the registry
    /// closure cannot have, returning whether this id was ours.
    ///
    /// These seven descriptors have shipped since P5a/P5b as breadcrumbs — their
    /// dispatch bodies literally say *"handled view-scoped (needs Window); no-op
    /// from App path"* — because `DispatchFn` is `Fn(&mut App)` and the work
    /// needs a `Window`. The palette is a modal INSIDE the window, so it has
    /// one, and every arm below calls the same shell method the corresponding
    /// console event or menu item already calls. The palette is a third entry
    /// point, not a second implementation.
    ///
    /// `false` means "not mine", and the caller falls back to the registry
    /// closure — an unknown id must never be silently swallowed.
    ///
    /// The ids here are exactly [`crate::command_palette::WINDOW_ROUTED`];
    /// `every_window_routed_id_is_actually_handled` fails if the two drift.
    ///
    /// The console-dependent arms no-op when no console is mounted, mirroring
    /// the existing `SqlConsoleEvent` arms. They still return `true`: the id IS
    /// routed, there was simply nothing to act on.
    pub(crate) fn run_palette_action(
        &mut self,
        id: &crate::actions::ActionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::actions::builtin::ids;
        match id.as_str() {
            ids::CONSOLE_TOGGLE => self.toggle_sql_console(window, cx),
            ids::SQL_NEW_TAB => {
                if let Some(c) = self.sql_console.clone() {
                    c.update(cx, |c, cx| c.new_tab(window, cx));
                }
            }
            ids::SQL_HISTORY => {
                if let Some(c) = self.sql_console.clone() {
                    let entries = self.session.lock().query_history().to_vec();
                    c.update(cx, |c, cx| c.show_history(entries, cx));
                }
            }
            ids::SQL_SAVE_QUERY => {
                if let Some(c) = self.sql_console.clone() {
                    let sql = c.read(cx).active_sql_and_cursor(cx).0;
                    self.open_name_prompt(sql, window, cx);
                }
            }
            ids::SQL_LOAD_QUERY => self.show_saved_picker(window, cx),
            ids::SQL_SAVE_AS_TABLE => self.open_name_prompt_with(
                "Save as table…",
                "",
                NamePromptIntent::SaveConsoleAsTable,
                window,
                cx,
            ),
            ids::VIEW_SAVE_AS_TABLE => self.open_save_view_as_table(window, cx),
            _ => return false,
        }
        true
    }

    /// Route a `SavedQueryPickerEvent`. The picker only READS the session;
    /// every mutation happens here.
    fn on_saved_picker_event(
        &mut self,
        ev: crate::view::saved_query_picker::SavedQueryPickerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::view::saved_query_picker::SavedQueryPickerEvent as E;
        match ev {
            E::Pick(sql) => {
                // Windowless load: the console drains `queue_load` in its own
                // render, which holds the `&mut Window` that
                // `load_into_new_tab` needs.
                if let Some(console) = self.sql_console.clone() {
                    console.update(cx, |c, cx| c.queue_load(sql, cx));
                }
                self.dismiss_saved_picker(window, cx);
            }
            E::Delete(id) => {
                self.delete_named_query(id, cx);
                // The picker reads the session live, so re-notify it and the
                // deleted row is gone on its next render.
                if let Some(p) = self.saved_picker.clone() {
                    p.update(cx, |_p, cx| cx.notify());
                }
                cx.notify();
            }
            E::Cancel => self.dismiss_saved_picker(window, cx),
        }
    }

    /// Tear the picker down and hand focus back to the pre-modal stop.
    fn dismiss_saved_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.saved_picker = None;
        self.saved_picker_sub = None;
        self.restore_modal_focus(window);
        cx.notify();
    }

    // -----------------------------------------------------------------------
    // Connections panel event handling (P5c T10/T11)
    // -----------------------------------------------------------------------
}
