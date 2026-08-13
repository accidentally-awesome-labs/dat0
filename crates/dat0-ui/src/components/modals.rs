//! The single modal slot.
//!
//! # One slot, not a stack
//!
//! GPUI's `WindowExt::open_dialog` **pushed** (`active_dialogs.push`) and
//! painted each new layer 16 px down and right of the one below it, so a
//! sequence of dialogs piled up on screen. The 7-panel onboarding carousel had
//! to work around that by calling `close_dialog` and then `open_dialog` again
//! for every Back/Next press — a close-then-open per step that the module doc
//! at `onboarding/mod.rs:12-19` calls "the load-bearing detail". Stepping was
//! therefore an unmount, so nothing in a dialog could hold state across a step.
//!
//! Here the slot is one `Signal<Option<Modal>>`. Opening is
//! `workspace.modal.set(Some(Modal::X))`, closing is `set(None)`, and switching
//! panels is a single `set` — the previous one cannot survive underneath.
//! Because a re-set to the same variant keeps the body at the same position in
//! the tree, Dioxus **diffs** it instead of remounting: the tour keeps its
//! step, the export dialog keeps its typed file name while the caller fills in
//! a destination. The workaround has no reason to exist.
//!
//! # The keyboard trap
//!
//! Two mechanisms with two distinct jobs:
//!
//! * **Containment** is `inert` + `aria-hidden` on everything beside the scrim
//!   ([`CAPTURE_JS`]). That is the browser's own mechanism: it removes the
//!   background from the tab order *and* from the accessibility tree, for any
//!   content, however dynamic — which no Rust-side list of stops could do.
//! * **Wrap-around** is [`trap_action`] over [`crate::keys::Cascade`]: Tab and
//!   Shift-Tab are intercepted, and [`CYCLE_JS`] moves focus one stop through
//!   the dialog's own focusables, wrapping at both ends. It reproduces
//!   `overlay.rs::next_index`, including the rule that focus currently
//!   *outside* the dialog is pulled back to the first (or last) stop rather
//!   than left to wander. `overlay.rs` could keep that list in Rust because
//!   every GPUI modal declared a `Vec<FocusHandle>`; here the panels belong to
//!   other modules and several of them grow controls with their data, so the
//!   DOM is asked instead — a list this file kept could only go stale.
//!
//! Resolving through `Cascade` rather than matching keys here is what makes the
//! precedence claim true rather than asserted: the modal scope is the first one
//! `Cascade` tries, so a modal wins Escape over the palette and over the SQL
//! console, and it wins it from one table both this file and the shell read.
//! Anything the modal scope does not bind — ⌘K, ⌘N — is left to bubble to the
//! shell's own handler, exactly as GPUI let an unconsumed keystroke fall
//! through to the global scope.
//!
//! # Dismissability
//!
//! Per dialog, from the GPUI original. `gpui_component::Dialog::confirm()` and
//! `alert()` both set `overlay_closable(false)` **and** `close_button(false)`,
//! and `overlay::modal_host`'s scrim never dismissed at all because "all three
//! prompts hold typed text that a stray click must not discard". So the rule
//! is one bit, [`scrim_dismissable`], and it governs the ✕ as well as the
//! scrim — upstream coupled them and so do we. Escape closes regardless:
//! `Dialog::keyboard` defaulted to `true`, so Escape cancelled even a confirm.

use std::path::PathBuf;
use std::rc::Rc;

use dioxus::prelude::*;

use crate::a11y::AccessRole;
use crate::keys::Cascade;
use crate::state::{Modal, Workspace};

use dat0_core::update::manifest::ArtifactEntry;

use super::{
    about, ai, connections, crash_report, export_dialog, import_wizard, live_refresh, name_prompt,
    onboarding, query_library, recovery, saved_queries, update_ui, workspace_in_use,
};

/// The scrim. Also the selector [`CAPTURE_JS`] anchors on, so renaming it
/// silently disarms the containment half of the trap.
pub const SCRIM_ID: &str = "modal-scrim";
/// The `role="dialog"` panel.
pub const DIALOG_ID: &str = "modal";
/// The header ✕.
pub const CLOSE_ID: &str = "modal-close";

// ── What a modal hands back ──────────────────────────────────────────────────

/// The decision a modal produced.
///
/// The slot carries data; the *consequences* belong to whoever opened it, so
/// they travel back through a [`ModalReply`] the opener supplies. This is the
/// GPUI shape — `open_conflict_dialog(cx, holder, on_open_anyway)`,
/// `route_export_event` — with one payload type instead of six event enums.
#[derive(Debug, Clone)]
pub enum ModalOutcome {
    /// Dismissed without choosing: Escape, the ✕, the scrim, or a Cancel
    /// button. Always the safe branch.
    Cancelled,
    /// A yes/no gate was confirmed: "Refresh anyway", "Open anyway".
    Confirmed,
    /// A name prompt was confirmed with this value.
    Named(String),
    /// The export dialog wants the caller's directory picker. The slot stays
    /// open; the caller re-sets it with `destination: Some(..)`.
    BrowseDestination,
    Export(export_dialog::ExportRequest),
    Import(import_wizard::WizardModel),
    /// Open this recovered workspace root.
    RecoveryOpen(PathBuf),
    /// Resume this interrupted promotion.
    RecoveryResume(PathBuf),
    QueryPicked(dat0_core::session::queries::SavedQuery),
    QueryDeleted(uuid::Uuid),
    /// SQL taken from the history library.
    SqlPicked(String),
    Connections(connections::ConnectionsEvent),
    /// The updater's artifact to download and apply. The host must not run
    /// this on the UI thread: `perform_install` downloads, applies and — on
    /// success — never returns.
    Install(ArtifactEntry),
}

/// The opener's handler for a [`ModalOutcome`].
///
/// `Rc<dyn Fn>` rather than an `EventHandler`: the reply is stored in
/// application state (the slot) rather than passed as a prop, and it outlives
/// any one render. Equality is pointer equality, which is what a signal needs
/// to decide "the slot changed" without demanding that a closure be comparable.
#[derive(Clone)]
pub struct ModalReply(Rc<dyn Fn(ModalOutcome)>);

impl ModalReply {
    pub fn new(f: impl Fn(ModalOutcome) + 'static) -> Self {
        Self(Rc::new(f))
    }

    pub fn call(&self, outcome: ModalOutcome) {
        (self.0)(outcome)
    }
}

impl PartialEq for ModalReply {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for ModalReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ModalReply")
    }
}

// ── Per-variant policy ───────────────────────────────────────────────────────

/// The header's small `.d0-label`, matching the pane header's id convention.
pub fn slug(modal: &Modal) -> &'static str {
    match modal {
        Modal::About { .. } => "about",
        Modal::Onboarding => "tour",
        Modal::NamePrompt { .. } => "name",
        Modal::Export { .. } => "export",
        Modal::Connections { .. } => "connections",
        Modal::CrashReport { .. } => "report",
        Modal::WorkspaceInUse { .. } => "workspace",
        Modal::LiveRefresh { .. } => "refresh",
        Modal::SavedQueries { .. } => "saved",
        Modal::QueryLibrary { .. } => "history",
        Modal::ImportWizard { .. } => "import",
        Modal::Recovery { .. } => "recovery",
        Modal::Ai { .. } => "ai",
        Modal::Update { .. } => "update",
    }
}

/// The dialog's accessible name, which is also the header title.
///
/// Delegated to each panel's own `title()` wherever one exists, so the string
/// has a single definition beside the panel that has to agree with it.
pub fn title(modal: &Modal) -> String {
    match modal {
        Modal::About { .. } => about::title(),
        Modal::Onboarding => onboarding::title(),
        Modal::NamePrompt { title, .. } => title.clone(),
        Modal::Export { .. } => dat0_i18n::t("export.title"),
        Modal::Connections { .. } => dat0_i18n::t("connections.title"),
        Modal::CrashReport { staged, .. } => crash_report::title(staged.as_ref()),
        Modal::WorkspaceInUse { kind, .. } => workspace_in_use::title(kind),
        Modal::LiveRefresh { .. } => live_refresh::title(),
        Modal::SavedQueries { .. } => dat0_i18n::t("sql.load_query"),
        Modal::QueryLibrary { .. } => dat0_i18n::t("sql.history"),
        Modal::ImportWizard { .. } => dat0_i18n::t("wizard.title"),
        Modal::Recovery { .. } => recovery::title(),
        Modal::Ai { .. } => dat0_i18n::t("ai.title"),
        Modal::Update { state, .. } => update_ui::title(state),
    }
}

/// Whether a click on the scrim — and therefore whether the header ✕ — closes
/// this dialog.
///
/// `false` means the body holds something a stray click must not discard, or
/// gates a decision the flow behind it needs an answer to. Each panel that has
/// an opinion publishes its own bit beside itself; the rest are judged here by
/// the same question — is there work to lose?
pub fn scrim_dismissable(modal: &Modal) -> bool {
    match modal {
        Modal::About { .. } => about::SCRIM_DISMISSABLE,
        Modal::CrashReport { .. } => crash_report::SCRIM_DISMISSABLE,
        Modal::LiveRefresh { .. } => live_refresh::SCRIM_DISMISSABLE,
        Modal::Onboarding => onboarding::SCRIM_DISMISSABLE,
        Modal::Recovery { .. } => recovery::SCRIM_DISMISSABLE,
        Modal::WorkspaceInUse { .. } => workspace_in_use::SCRIM_DISMISSABLE,
        Modal::Update { .. } => update_ui::SCRIM_DISMISSABLE,
        // Pickers: a click outside is an unambiguous "never mind", and the
        // filter text they hold is not work.
        Modal::SavedQueries { .. } | Modal::QueryLibrary { .. } => true,
        // The AI panel writes straight to the settings store and the keychain,
        // so it has no unsaved work for a stray click to discard — its secrets
        // are typed into a separate `NamePrompt`, which is the one that must
        // not be dismissed.
        Modal::Ai { .. } => true,
        // Typed text (a name, a token), a configured destination, or a
        // half-finished multi-step form.
        Modal::NamePrompt { .. }
        | Modal::Export { .. }
        | Modal::Connections { .. }
        | Modal::ImportWizard { .. } => false,
    }
}

// ── The trap ─────────────────────────────────────────────────────────────────

const ESCAPE_ACTION: &str = "gpui_component::input::Escape";
const TAB_ACTION: &str = "dat0_modal::ModalTab";
const TAB_PREV_ACTION: &str = "dat0_modal::ModalTabPrev";

/// What the dialog's `onkeydown` should do with a keystroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapAction {
    /// Dismiss, as Cancel.
    Close,
    /// Move focus this many stops through the dialog's own focusables,
    /// wrapping at both ends. See [`CYCLE_JS`].
    Cycle(isize),
    /// Not the modal's key. Let it bubble to the shell's cascade.
    Fallthrough,
}

/// Resolve a keystroke against the modal scope.
///
/// `cascade` must have `modal_open` set — it is the modal scope's presence in
/// the cascade that makes the modal outrank the palette and the console, and a
/// cascade without it would resolve Escape in whichever scope came next.
pub fn trap_action(cascade: Cascade, key: &Key, mods: Modifiers) -> TrapAction {
    debug_assert!(
        cascade.modal_open,
        "the trap only runs while a modal is open"
    );
    match cascade.resolve_binding(key, mods).and_then(|b| b.action) {
        Some(ESCAPE_ACTION) => TrapAction::Close,
        Some(TAB_ACTION) => TrapAction::Cycle(1),
        Some(TAB_PREV_ACTION) => TrapAction::Cycle(-1),
        _ => TrapAction::Fallthrough,
    }
}

/// Everything a browser will let Tab reach, restricted to the dialog.
///
/// The tab order is a property of the rendered panel, not of a list this file
/// keeps: the panels are built by other modules and several of them (the
/// import wizard's per-column controls, the recovery rows, the saved-query
/// list) grow and shrink with their data. `overlay.rs` could enumerate its
/// stops because every modal declared a `Vec<FocusHandle>`; here the DOM
/// already is that list, so asking it is both shorter and impossible to let
/// drift.
///
/// **Every branch excludes `tabindex="-1"`, and that is load-bearing.** A
/// selector list is a union, so a bare `button:not([disabled])` matches a
/// `<button tabindex="-1">` — which is how a roving-tabindex group is built.
/// Without the exclusion the export dialog's five radios and every
/// saved-query row would each be a stop, so choosing one of three formats
/// would cost three Tabs; GPUI spelled the same rule `.tab_stop(false)` on
/// each child. It is also simply what the browser does: `tabindex="-1"`
/// means focusable by script, never by Tab.
pub const FOCUSABLE_SELECTOR: &str = "button:not([disabled]):not([tabindex=\"-1\"]), \
     input:not([disabled]):not([tabindex=\"-1\"]), \
     select:not([disabled]):not([tabindex=\"-1\"]), \
     textarea:not([disabled]):not([tabindex=\"-1\"]), \
     a[href]:not([tabindex=\"-1\"]), \
     [tabindex]:not([tabindex=\"-1\"])";

/// Move focus one stop through the dialog, wrapping.
///
/// `{delta}` is substituted with `1` or `-1`. The wrap is
/// `overlay.rs::next_index`'s rule, in the language that can see the stops:
/// `rem_euclid` on the current index, and focus that is currently *outside*
/// the dialog (index `-1` from `indexOf`) is pulled back to the first stop
/// going forwards and the last going backwards, rather than left to wander.
pub const CYCLE_JS: &str = r#"
const dialog = document.querySelector('[data-a11y-id="modal"]');
if (dialog) {
  const stops = [...dialog.querySelectorAll(SELECTOR)]
    .filter((n) => n.offsetParent !== null || n === document.activeElement);
  if (stops.length) {
    const d = DELTA;
    const at = stops.indexOf(document.activeElement);
    const next = at < 0 ? (d > 0 ? 0 : stops.length - 1)
                        : ((at + d) % stops.length + stops.length) % stops.length;
    stops[next].focus();
  }
}
"#;

// ── The browser half ─────────────────────────────────────────────────────────

/// Remove everything beside the scrim from the tab order and the accessibility
/// tree, and remember where focus was so it can be handed back.
///
/// `inert` is the containment mechanism: a Rust-side ring can only cycle the
/// stops it was told about, whereas `inert` makes *every* background control —
/// a grid cell, a sidebar row, a control a panel grew last week — unreachable.
/// It also closes what GPUI could not: `modal_host`'s `occlude` blocked the
/// mouse only, so a screen reader still walked the shell behind the dialog.
pub const CAPTURE_JS: &str = r#"
const scrim = document.querySelector('[data-a11y-id="modal-scrim"]');
if (scrim && scrim.parentElement) {
  // Only the FIRST capture records the return target. A variant swap re-runs
  // this with focus already inside the dialog, and recording that would hand
  // focus back to a node that is about to be unmounted.
  if (window.__d0ModalReturn == null) window.__d0ModalReturn = document.activeElement;
  for (const n of scrim.parentElement.children) {
    if (n !== scrim) { n.inert = true; n.setAttribute("aria-hidden", "true"); }
  }
}
"#;

/// Undo [`CAPTURE_JS`] and restore the pre-modal focus.
///
/// The restore is `overlay.rs`'s `modal_restore_focus`: a dismissed modal must
/// hand the keyboard back to the control that opened it, or a keyboard user is
/// dropped at the top of the document every time they press Escape.
pub const RELEASE_JS: &str = r#"
for (const n of document.querySelectorAll("[inert]")) {
  n.inert = false; n.removeAttribute("aria-hidden");
}
if (window.__d0ModalReturn) { window.__d0ModalReturn.focus?.(); window.__d0ModalReturn = null; }
"#;

/// Whether a renderer supplied a document.
///
/// The headless harness does not, and `document::eval` without one logs an
/// error per call. Asking first keeps a test run's output honest about what
/// actually failed.
fn has_document() -> bool {
    dioxus::prelude::try_consume_context::<Rc<dyn dioxus::document::Document>>().is_some()
}

fn run_js(script: &str) {
    if has_document() {
        let _ = document::eval(script);
    }
}

/// Move keyboard focus one stop through the dialog.
fn cycle_focus(delta: isize) {
    run_js(
        &CYCLE_JS
            .replace("SELECTOR", &format!("{FOCUSABLE_SELECTOR:?}"))
            .replace("DELTA", &delta.to_string()),
    );
}

// ── The host ─────────────────────────────────────────────────────────────────

/// Renders the one open modal, or nothing.
#[component]
pub fn ModalHost() -> Element {
    let mut ws = Workspace::use_current();

    // The read is INSIDE the closure on purpose: `use_effect` subscribes to
    // whatever the closure touches, so hoisting `is_some()` out of it would
    // capture a plain `bool`, leave the effect with no dependency, and run it
    // exactly once — the background would be inerted at mount and never
    // released.
    use_effect(move || {
        if ws.modal.read().is_some() {
            run_js(CAPTURE_JS);
        } else {
            run_js(RELEASE_JS);
        }
    });

    let Some(modal) = ws.modal.read().clone() else {
        return rsx! {};
    };

    let dismissable = scrim_dismissable(&modal);
    let heading = title(&modal);
    let label = slug(&modal);

    // Cancel: every exit that is not the panel's own affirmative button. The
    // reply runs before the slot is cleared so a handler is free to open the
    // next modal, which the single slot then simply replaces.
    let close = {
        let modal = modal.clone();
        move |_| {
            cancel(&modal);
            ws.modal.set(None);
        }
    };

    let keydown = {
        let modal = modal.clone();
        let palette_open = *ws.palette.read();
        move |e: KeyboardEvent| {
            let cascade = Cascade {
                modal_open: true,
                palette_open,
                sql_console_focused: false,
            };
            match trap_action(cascade, &e.key(), e.modifiers()) {
                TrapAction::Close => {
                    e.stop_propagation();
                    e.prevent_default();
                    cancel(&modal);
                    ws.modal.set(None);
                }
                TrapAction::Cycle(delta) => {
                    // Consumed unconditionally. Letting Tab through as well as
                    // moving focus ourselves would advance it twice, and the
                    // browser's own wrap at the last stop leaves the document
                    // rather than returning to the first.
                    e.stop_propagation();
                    e.prevent_default();
                    cycle_focus(delta);
                }
                // ⌘K, ⌘N and friends still belong to the shell.
                TrapAction::Fallthrough => {}
            }
        }
    };

    rsx! {
        div {
            class: "d0-scrim",
            "data-a11y-id": SCRIM_ID,
            onclick: {
                let mut close = close.clone();
                move |e: MouseEvent| {
                    // Only the scrim itself. A click that bubbled up from
                    // inside the panel is not a click outside it.
                    e.stop_propagation();
                    if dismissable {
                        close(());
                    }
                }
            },

            div {
                class: "d0-modal",
                "data-a11y-id": DIALOG_ID,
                role: AccessRole::Dialog.aria(),
                "aria-modal": "true",
                "aria-label": "{heading}",
                // Focusable so the dialog itself can hold the keyboard the
                // moment it opens, before the user has reached a control.
                // Excluded from the tab order: it is a container, not a stop.
                tabindex: "-1",
                onkeydown: keydown,
                onclick: move |e: MouseEvent| e.stop_propagation(),

                div { class: "d0-modal-head",
                    span { class: "d0-label", "{label}" }
                    span { class: "d0-head-title", "{heading}" }
                    if dismissable {
                        button {
                            class: "d0-btn is-ghost d0-modal-close",
                            "data-a11y-id": CLOSE_ID,
                            role: AccessRole::Button.aria(),
                            "aria-label": dat0_i18n::t("common.close"),
                            onclick: {
                                let mut close = close.clone();
                                move |_| close(())
                            },
                            "✕"
                        }
                    }
                }

                div { class: "d0-modal-body", "data-a11y-id": "modal-body",
                    {body(&modal, ws)}
                }
            }
        }
    }
}

/// Run the variant's dismiss side effects.
///
/// Three kinds: telling the opener (`reply(Cancelled)`), and the two panels
/// whose dismissal has to reach disk. A crash report that is merely unmounted
/// leaves its payload staged and asks again next launch
/// (`crash_report.rs:33-37`); a tour that is merely unmounted has not recorded
/// that the user is done with it, and comes back every launch.
///
/// Escape is the tour's only host-driven exit — its scrim is inert and it has
/// no ✕ — so this is where "I have seen enough" is written down. The GPUI
/// build let Escape through `Dialog::keyboard` straight to `close_dialog`,
/// which skipped `mark_first_run_done` and re-showed the tour; that is a bug
/// the port declines to reproduce.
fn cancel(modal: &Modal) {
    match modal {
        Modal::CrashReport { data_dir, .. } => crash_report::dismiss(data_dir),
        Modal::Onboarding => onboarding::mark_first_run_done(),
        Modal::NamePrompt { reply, .. }
        | Modal::Export { reply, .. }
        | Modal::Connections { reply, .. }
        | Modal::WorkspaceInUse { reply, .. }
        | Modal::LiveRefresh { reply, .. }
        | Modal::SavedQueries { reply, .. }
        | Modal::QueryLibrary { reply, .. }
        | Modal::ImportWizard { reply, .. }
        | Modal::Recovery { reply, .. }
        | Modal::Update { reply, .. } => reply.call(ModalOutcome::Cancelled),
        Modal::About { .. } | Modal::Ai { .. } => {}
    }
}

/// Clear the slot from inside a panel's own handler.
///
/// Takes the workspace rather than reaching for it: `Workspace::use_current`
/// is `use_context`, which is a *hook*, and a hook called from an event
/// handler runs outside the render that owns the hook list.
fn close_slot(ws: Workspace) {
    let mut ws = ws;
    ws.modal.set(None);
}

/// The variant's panel.
///
/// A plain function rather than a component so that swapping variants swaps
/// only the body: the scrim, the dialog element and the header keep their
/// identity, and the panel at this position is diffed rather than remounted.
fn body(modal: &Modal, ws: Workspace) -> Element {
    match modal.clone() {
        Modal::About {
            newer,
            check_latest,
        } => rsx! {
            about::About { newer, check_latest, on_close: move |_| close_slot(ws) }
        },

        Modal::Onboarding => rsx! {
            // `initial_step` is read once at mount. The slot never rewrites it:
            // the carousel owns its step, and a slot-driven step would be a
            // remount — which is precisely the GPUI behaviour this replaces.
            onboarding::OnboardingTour { initial_step: 0, on_finish: move |_| close_slot(ws) }
        },

        Modal::CrashReport { staged, data_dir } => rsx! {
            crash_report::CrashReport { staged, data_dir, on_close: move |_| close_slot(ws) }
        },

        Modal::NamePrompt {
            title,
            initial,
            placeholder,
            confirm_label,
            secret,
            reply,
        } => {
            let confirm = reply.clone();
            rsx! {
                name_prompt::NamePrompt {
                    title, initial, placeholder, confirm_label, secret,
                    on_confirm: move |name: String| {
                        confirm.call(ModalOutcome::Named(name));
                        close_slot(ws);
                    },
                    on_cancel: move |_| {
                        reply.call(ModalOutcome::Cancelled);
                        close_slot(ws);
                    },
                }
            }
        }

        Modal::Export { destination, reply } => {
            let browse = reply.clone();
            let export = reply.clone();
            rsx! {
                export_dialog::ExportDialog {
                    destination,
                    // Deliberately does NOT close: the caller runs its
                    // directory picker and re-sets the slot with the answer,
                    // and the dialog keeps the file name already typed into it.
                    on_browse: move |_| browse.call(ModalOutcome::BrowseDestination),
                    on_export: move |req: export_dialog::ExportRequest| {
                        export.call(ModalOutcome::Export(req));
                        close_slot(ws);
                    },
                    on_cancel: move |_| {
                        reply.call(ModalOutcome::Cancelled);
                        close_slot(ws);
                    },
                }
            }
        }

        Modal::Connections { state, reply } => rsx! {
            connections::ConnectionsPanel {
                state,
                // The panel stays open: connecting, attaching and testing are
                // things a user does several of in one visit.
                on_event: move |ev: connections::ConnectionsEvent| {
                    reply.call(ModalOutcome::Connections(ev))
                },
            }
        },

        Modal::WorkspaceInUse { kind, reply } => {
            let proceed = reply.clone();
            rsx! {
                workspace_in_use::WorkspaceInUse {
                    kind,
                    on_proceed: move |_| {
                        proceed.call(ModalOutcome::Confirmed);
                        close_slot(ws);
                    },
                    on_cancel: move |_| {
                        reply.call(ModalOutcome::Cancelled);
                        close_slot(ws);
                    },
                }
            }
        }

        Modal::LiveRefresh {
            dropped_edits,
            dropped_deletes,
            reply,
        } => {
            let confirm = reply.clone();
            rsx! {
                live_refresh::LiveRefreshConfirm {
                    dropped_edits, dropped_deletes,
                    on_confirm: move |_| {
                        confirm.call(ModalOutcome::Confirmed);
                        close_slot(ws);
                    },
                    on_cancel: move |_| {
                        reply.call(ModalOutcome::Cancelled);
                        close_slot(ws);
                    },
                }
            }
        }

        Modal::SavedQueries { queries, reply } => {
            let pick = reply.clone();
            rsx! {
                saved_queries::SavedQueriesPicker {
                    queries,
                    on_pick: move |q: dat0_core::session::queries::SavedQuery| {
                        pick.call(ModalOutcome::QueryPicked(q));
                        close_slot(ws);
                    },
                    // Deleting is a list edit, not a choice: the picker stays
                    // up so the user can carry on picking.
                    on_delete: move |id: uuid::Uuid| reply.call(ModalOutcome::QueryDeleted(id)),
                }
            }
        }

        Modal::QueryLibrary { entries, reply } => rsx! {
            query_library::QueryLibrary {
                entries,
                on_pick: move |sql: String| {
                    reply.call(ModalOutcome::SqlPicked(sql));
                    close_slot(ws);
                },
            }
        },

        Modal::ImportWizard { model, reply } => {
            let import = reply.clone();
            rsx! {
                import_wizard::ImportWizard {
                    model,
                    on_import: move |m: import_wizard::WizardModel| {
                        import.call(ModalOutcome::Import(m));
                        close_slot(ws);
                    },
                    on_cancel: move |_| {
                        reply.call(ModalOutcome::Cancelled);
                        close_slot(ws);
                    },
                }
            }
        }

        Modal::Recovery {
            scratch_root,
            recent_roots,
            reply,
        } => {
            let open = reply.clone();
            let resume = reply.clone();
            rsx! {
                recovery::RecoveryPanel {
                    scratch_root, recent_roots,
                    on_open: move |p: PathBuf| {
                        open.call(ModalOutcome::RecoveryOpen(p));
                        close_slot(ws);
                    },
                    on_resume: move |p: PathBuf| {
                        resume.call(ModalOutcome::RecoveryResume(p));
                        close_slot(ws);
                    },
                    on_close: move |_| {
                        reply.call(ModalOutcome::Cancelled);
                        close_slot(ws);
                    },
                }
            }
        }

        Modal::Ai { controller } => rsx! {
            ai::AiPanel { controller }
        },

        Modal::Update {
            state,
            is_manual,
            reply,
        } => {
            let install = reply.clone();
            rsx! {
                update_ui::UpdatePrompt {
                    state, is_manual,
                    on_install: move |a: ArtifactEntry| {
                        install.call(ModalOutcome::Install(a));
                        close_slot(ws);
                    },
                    on_close: move |_| {
                        reply.call(ModalOutcome::Cancelled);
                        close_slot(ws);
                    },
                }
            }
        }
    }
}
