//! dat0's default keymap, expressed as data.
//!
//! Every chord the app binds lives in [`DEFAULT_KEYMAP`]. Before this table the
//! same rows were spread across seven `bind_keys` calls in four files, and an
//! action descriptor carried a hand-typed `keybinding` field beside them — so a
//! command-palette hint could disagree with the live chord and nothing would
//! notice. [`chord_for`] reads the table the bindings are installed from, which
//! makes that class of drift unrepresentable.
//!
//! It lives in `dat0-core` because a keymap is a product decision, not a
//! renderer one: the palette's hints, the menu bar's accelerators and the
//! shell's key cascade all read the same rows.
//!
//! # Not `grid::keymap`
//!
//! [`crate::grid::keymap`] shares the name and is unrelated: it is the grid's
//! modal cursor grammar, driven by a raw key handler on the shell root and
//! never through this table. A cursor mode is not a set of global commands —
//! arrow keys mean something different inside the grid than anywhere else — so
//! it stays out of here on purpose.

/// Key-scope names.
///
/// A "context" is which surface currently owns the keyboard. GPUI matched these
/// strings against a per-element context stack; the Dioxus shell evaluates the
/// same names as an explicit cascade (`Modal -> Palette -> SqlConsole ->
/// Global`, first match wins). Either way the *name* of a scope is a product
/// decision that belongs beside the table that uses it.
pub const PALETTE_CONTEXT: &str = "CommandPalette";
pub const MODAL_CONTEXT: &str = "Dat0Modal";
pub const SQL_CONSOLE_CONTEXT: &str = "SqlConsole";

/// Which entry point owns a row.
///
/// Not the same axis as [`Binding::context`]: `Palette` holds both the global
/// ⌘⇧P chord (no context) and the two `CommandPalette`-scoped arrows, because
/// one function registers all three and every test binary that wants the
/// palette wants the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Installed only by production boot (`window/boot.rs`).
    Global,
    /// [`crate::command_palette::register_command_palette_keys`].
    Palette,
    /// [`crate::overlay::register_modal_keys`].
    Modal,
    /// [`crate::view::sql_console::register_sql_console_keys`].
    SqlConsole,
}

/// One row of dat0's default keymap.
#[derive(Debug)]
pub struct Binding {
    /// Which entry point installs this row.
    pub scope: Scope,
    /// The gpui key context the binding is scoped to; `None` = global.
    pub context: Option<&'static str>,
    /// The macOS chord.
    pub macos: &'static str,
    /// The chord on every other platform.
    ///
    /// `None` marks a binding that exists only on macOS — the window-management
    /// chords (⌘Q / ⌘W / ⌘M) have no non-macOS counterpart today because GPUI's
    /// Linux backend supplies them, so binding them ourselves there would only
    /// shadow the platform's own handling.
    pub other: Option<&'static str>,
    /// The gpui action path, e.g. `"dat0_menu::SqlRun"`. Cross-checked against
    /// the real `actions!` declarations by `tests/keymap.rs`.
    ///
    /// `None` for a chord the GPUI shell does not implement. That is not a
    /// gap to fill: the Dioxus shell routes by
    /// [`action_id`](Self::action_id), and the sidebar toggle (S1) has no
    /// GPUI counterpart because the GPUI left dock was a three-way mode
    /// switch with no hidden state. Declaring a dead gpui action to satisfy
    /// the cross-check would put a permanently unreachable handler in a crate
    /// that is being deleted.
    pub action: Option<&'static str>,
    /// The [`crate::actions::registry::ActionId`] this chord invokes, when the
    /// action has a registry descriptor. `None` for the palette-internal and
    /// modal-internal actions, which are keyboard mechanics rather than
    /// commands a user could search for.
    pub action_id: Option<&'static str>,
}

/// The whole default keymap. Order is registration order.
pub const DEFAULT_KEYMAP: &[Binding] = &[
    // ── Scope::Global — was `window/boot.rs`'s four `cx.bind_keys` sites ──
    Binding {
        scope: Scope::Global,
        context: None,
        macos: "cmd-z",
        other: Some("ctrl-z"),
        action: Some("dat0_menu::Undo"),
        action_id: Some(crate::actions::builtin::ids::VIEW_UNDO),
    },
    Binding {
        scope: Scope::Global,
        context: None,
        macos: "cmd-shift-z",
        other: Some("ctrl-shift-z"),
        action: Some("dat0_menu::Redo"),
        action_id: Some(crate::actions::builtin::ids::VIEW_REDO),
    },
    Binding {
        scope: Scope::Global,
        context: None,
        macos: "cmd-e",
        other: Some("ctrl-e"),
        action: Some("dat0_menu::Export"),
        action_id: Some(crate::actions::builtin::ids::VIEW_EXPORT),
    },
    // The three SQL-console chords are handled VIEW-scoped on the shell root in
    // `render` (they need a `&mut Window` the App-level dispatch path cannot
    // supply); only the keystrokes are registered here. `sql.new_tab` /
    // `sql.close_tab` are deliberately chord-less — see `UNBOUND` in
    // `tests/keymap.rs`.
    Binding {
        scope: Scope::Global,
        context: None,
        macos: "cmd-enter",
        other: Some("ctrl-enter"),
        action: Some("dat0_menu::SqlRun"),
        action_id: Some(crate::actions::builtin::ids::SQL_RUN),
    },
    Binding {
        scope: Scope::Global,
        context: None,
        macos: "cmd-.",
        other: Some("ctrl-."),
        action: Some("dat0_menu::SqlCancel"),
        action_id: Some(crate::actions::builtin::ids::SQL_CANCEL),
    },
    Binding {
        scope: Scope::Global,
        context: None,
        macos: "cmd-shift-c",
        other: Some("ctrl-shift-c"),
        action: Some("dat0_menu::SqlConsoleToggle"),
        action_id: Some(crate::actions::builtin::ids::CONSOLE_TOGGLE),
    },
    // S1: the sidebar toggle. New in the Dioxus shell — the GPUI left dock was
    // a three-way mode switch with no "hidden" mode, so there was nothing to
    // bind. ⌘B is the conventional chord and collides with nothing dat0 binds.
    Binding {
        scope: Scope::Global,
        context: None,
        macos: "cmd-b",
        other: Some("ctrl-b"),
        action: None,
        action_id: Some(crate::actions::builtin::ids::SIDEBAR_TOGGLE),
    },
    // Quit / Close Window / Minimize had no handlers since their menus were
    // added — permanently grayed, ⌘Q included (the key equivalent hangs off the
    // menu item, so a disabled item swallows it). macOS-only: see
    // `Binding::other`.
    Binding {
        scope: Scope::Global,
        context: None,
        macos: "cmd-q",
        other: None,
        action: Some("dat0_menu::Quit"),
        action_id: None,
    },
    Binding {
        scope: Scope::Global,
        context: None,
        macos: "cmd-w",
        other: None,
        action: Some("dat0_menu::CloseWindow"),
        action_id: None,
    },
    Binding {
        scope: Scope::Global,
        context: None,
        macos: "cmd-m",
        other: None,
        action: Some("dat0_menu::Minimize"),
        action_id: None,
    },
    // ── Scope::Palette — was `command_palette::register_command_palette_keys` ──
    //
    // `OpenCommandPalette` is declared unconditionally in `menu_macos.rs`, so
    // this binds on Linux too even though the Linux menu module does not exist:
    // the handler resolves and the keystroke fires without a visible menu item.
    Binding {
        scope: Scope::Palette,
        context: None,
        macos: "cmd-shift-p",
        other: Some("ctrl-shift-p"),
        action: Some("dat0_menu::OpenCommandPalette"),
        action_id: None,
    },
    // dat0 actions under the palette context rather than an interception of
    // upstream's `MoveUp`/`MoveDown`: with focus on the results list the "Input"
    // key context is absent from the stack, so those upstream actions are never
    // produced at all.
    Binding {
        scope: Scope::Palette,
        context: Some(PALETTE_CONTEXT),
        macos: "up",
        other: Some("up"),
        action: Some("dat0_palette::PaletteUp"),
        action_id: None,
    },
    Binding {
        scope: Scope::Palette,
        context: Some(PALETTE_CONTEXT),
        macos: "down",
        other: Some("down"),
        action: Some("dat0_palette::PaletteDown"),
        action_id: None,
    },
    // ── Scope::Modal — was `overlay::register_modal_keys` ──
    Binding {
        scope: Scope::Modal,
        context: Some(MODAL_CONTEXT),
        macos: "tab",
        other: Some("tab"),
        action: Some("dat0_modal::ModalTab"),
        action_id: None,
    },
    Binding {
        scope: Scope::Modal,
        context: Some(MODAL_CONTEXT),
        macos: "shift-tab",
        other: Some("shift-tab"),
        action: Some("dat0_modal::ModalTabPrev"),
        action_id: None,
    },
    Binding {
        scope: Scope::Modal,
        context: Some(MODAL_CONTEXT),
        macos: "escape",
        other: Some("escape"),
        action: Some("gpui_component::input::Escape"),
        action_id: None,
    },
    // ── Scope::SqlConsole — was `view::sql_console::register_sql_console_keys` ──
    //
    // Safe against the editor: when the editor (an Input, a descendant of the
    // console root) is focused, BOTH the deeper "Input" and this "SqlConsole"
    // escape bindings are in the dispatch stack and gpui resolves the deepest.
    Binding {
        scope: Scope::SqlConsole,
        context: Some(SQL_CONSOLE_CONTEXT),
        macos: "escape",
        other: Some("escape"),
        action: Some("gpui_component::input::Escape"),
        action_id: None,
    },
];

/// The chord for `binding` on the platform this build targets, or `None` when
/// the binding is macOS-only and this is not macOS.
pub fn platform_chord(binding: &Binding) -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some(binding.macos)
    } else {
        binding.other
    }
}

/// The chord this platform uses for `action_id`, for `Kbd` hints in the command
/// palette. `None` means the action has no default chord here — either it is
/// deliberately palette-only, or it is macOS-only and this is not macOS.
pub fn chord_for(action_id: &str) -> Option<&'static str> {
    DEFAULT_KEYMAP
        .iter()
        .find(|b| b.action_id == Some(action_id))
        .and_then(platform_chord)
}

/// The chord this platform uses for the gpui action path `action`.
///
/// The sibling of [`chord_for`] for controls that surface an action with no
/// [`crate::actions::registry::ActionDescriptor`]. UI4's tab-strip search
/// gutter is the case that needs it: the palette cannot list "open the
/// palette" as one of its own commands, so that row carries `action_id: None`
/// and [`chord_for`] can never reach it — but the row, and therefore the live
/// chord, is right here.
pub fn chord_for_gpui_action(action: &str) -> Option<&'static str> {
    DEFAULT_KEYMAP
        .iter()
        .find(|b| b.action == Some(action))
        .and_then(platform_chord)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_for_resolves_a_bound_action_and_rejects_an_unbound_one() {
        let undo = chord_for(crate::actions::builtin::ids::VIEW_UNDO);
        assert_eq!(
            undo,
            Some(if cfg!(target_os = "macos") {
                "cmd-z"
            } else {
                "ctrl-z"
            })
        );
        // Palette-only by design; a hint here would be a lie.
        assert_eq!(chord_for(crate::actions::builtin::ids::SQL_NEW_TAB), None);
        assert_eq!(chord_for("nope.not.an.action"), None);
    }
}
