//! Chord matching and the shell's key cascade.
//!
//! # Why there is a cascade at all
//!
//! GPUI dispatched keys through an ambient action tree: an element declared a
//! *key context*, the binding named that context, and the event bubbled until
//! something matched. Dioxus has no such tree — a `onkeydown` handler is just a
//! handler. So the same precedence is written out, once, in
//! [`Cascade::resolve`], and evaluated by a single `onkeydown` on the shell
//! root:
//!
//! ```text
//! Modal (if one is open) -> Palette (if open) -> SqlConsole (if focused) -> Global
//! ```
//!
//! First match wins and the handler stops propagation. This is a
//! behaviour-preserving reimplementation of the Escape ladder that
//! `view/sql_console.rs` grew, and it deletes the `InstalledScopes`
//! double-install guard entirely: there is nothing global to install, so a
//! scope cannot be installed twice.
//!
//! # Why chords are parsed rather than pre-compiled
//!
//! The table stores chords as GPUI-style strings (`"cmd-shift-p"`). Parsing one
//! is a handful of `split('-')`, and doing it at match time keeps
//! `dat0_core::keymap` the single source of truth for both the live binding and
//! the palette's hint. A pre-compiled table is a second representation to drift.

use dioxus::prelude::{Key, Modifiers};

use dat0_core::keymap::{
    Binding, DEFAULT_KEYMAP, MODAL_CONTEXT, PALETTE_CONTEXT, SQL_CONSOLE_CONTEXT, platform_chord,
};

/// A parsed chord: the modifiers, plus the key that completes it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Chord {
    meta: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
    key: String,
}

impl Chord {
    /// Parse `"cmd-shift-p"`, `"ctrl-enter"`, `"escape"`.
    ///
    /// Returns `None` for a chord with no key part, which would otherwise match
    /// every press of its modifiers.
    fn parse(s: &str) -> Option<Self> {
        let mut c = Chord {
            meta: false,
            ctrl: false,
            alt: false,
            shift: false,
            key: String::new(),
        };
        for part in s.split('-') {
            match part {
                "cmd" | "super" | "win" => c.meta = true,
                "ctrl" => c.ctrl = true,
                "alt" | "option" => c.alt = true,
                "shift" => c.shift = true,
                "" => return None,
                other => c.key = other.to_ascii_lowercase(),
            }
        }
        (!c.key.is_empty()).then_some(c)
    }

    fn matches(&self, key: &Key, mods: Modifiers) -> bool {
        self.meta == mods.meta()
            && self.ctrl == mods.ctrl()
            && self.alt == mods.alt()
            && self.shift == mods.shift()
            && self.key == normalize(key)
    }
}

/// A `Key` as the keymap table spells it.
///
/// The table uses GPUI's vocabulary (`"escape"`, `"enter"`, `"backspace"`);
/// the DOM uses the UI Events one (`"Escape"`, `"Enter"`, `"Backspace"`). A
/// single lowercase mapping covers both, because the two only disagree on case
/// for the named keys.
fn normalize(key: &Key) -> String {
    match key {
        Key::Character(c) => c.to_ascii_lowercase(),
        other => {
            let s = other.to_string().to_ascii_lowercase();
            // GPUI spells the arrows without the prefix.
            s.strip_prefix("arrow").map(str::to_string).unwrap_or(s)
        }
    }
}

/// Which surfaces currently want the keyboard, most specific first.
///
/// Constructed from the shell's own state each keystroke, so it cannot go
/// stale: there is no registration step to forget to undo when a modal closes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cascade {
    pub modal_open: bool,
    pub palette_open: bool,
    pub sql_console_focused: bool,
}

/// The gpui action name the ⌘⇧P keymap row carries.
///
/// Named here rather than spelled at the match site for the same reason the
/// chord is not: `dat0_core::keymap` is the one table, and a second literal is
/// a second thing to drift.
pub const OPEN_PALETTE: &str = "dat0_menu::OpenCommandPalette";

impl Cascade {
    /// The scopes to try, in precedence order. `Global` is always last and
    /// always present.
    fn contexts(self) -> impl Iterator<Item = Option<&'static str>> {
        [
            self.modal_open.then_some(MODAL_CONTEXT),
            self.palette_open.then_some(PALETTE_CONTEXT),
            self.sql_console_focused.then_some(SQL_CONSOLE_CONTEXT),
            // `None` context = global.
            Some(""),
        ]
        .into_iter()
        .flatten()
        .map(|c| if c.is_empty() { None } else { Some(c) })
    }

    /// The action id for this keystroke, or `None` if nothing is bound.
    ///
    /// A row with no `action_id` — the palette's own arrow keys, the modal's
    /// Tab — is keyboard *mechanics* rather than a command, and is handled by
    /// the surface that owns it. [`resolve_binding`] exposes those.
    pub fn resolve(self, key: &Key, mods: Modifiers) -> Option<&'static str> {
        self.resolve_binding(key, mods)?.action_id
    }

    /// The matching row, including rows with no registry action.
    pub fn resolve_binding(self, key: &Key, mods: Modifiers) -> Option<&'static Binding> {
        for context in self.contexts() {
            if let Some(b) = match_in_context(context, key, mods) {
                return Some(b);
            }
        }
        None
    }

    /// Does this row open the command palette?
    ///
    /// The ⌘⇧P row carries no `action_id` — the palette cannot list "open the
    /// palette" among its own commands — so the shell cannot reach it through
    /// [`resolve`] and matches on the gpui action name instead.
    ///
    /// The chord is GLOBAL, so it fires while a dialog owns the screen too.
    /// Answering that here rather than at the call site keeps the rule beside
    /// the state it is about: [`Cascade`] is what knows a modal is up, and a
    /// palette mounted on top of a dialog is a second overlay nobody trapped.
    ///
    /// [`resolve`]: Self::resolve
    pub fn opens_palette(self, binding: &Binding) -> bool {
        binding.action == Some(OPEN_PALETTE) && !self.modal_open
    }
}

/// The first row in `context` whose platform chord matches.
fn match_in_context(
    context: Option<&'static str>,
    key: &Key,
    mods: Modifiers,
) -> Option<&'static Binding> {
    DEFAULT_KEYMAP.iter().find(|b| {
        b.context == context
            && platform_chord(b)
                .and_then(Chord::parse)
                .is_some_and(|c| c.matches(key, mods))
    })
}

/// Translate a keystroke into the grid's cursor grammar.
///
/// This is the Dioxus twin of the GPUI `key_from_event`, and it is deliberately
/// *not* part of [`Cascade`]. The grid is a modal surface: arrow keys mean
/// "move the cursor" inside it and something else everywhere else, so its
/// grammar cannot live in a table of global chords. `dat0_core::grid::keymap`
/// owns what each key then does to the `SelectionModel`.
pub fn grid_key(key: &Key, mods: Modifiers) -> Option<GridKey> {
    let shift = mods.shift();
    // Cmd on macOS, Ctrl elsewhere — the same "secondary" notion GPUI encoded.
    let jump = if cfg!(target_os = "macos") {
        mods.meta()
    } else {
        mods.ctrl()
    };

    Some(match key {
        Key::ArrowUp if jump => GridKey::JumpTop,
        Key::ArrowDown if jump => GridKey::JumpBottom,
        Key::ArrowLeft if jump => GridKey::JumpLeft,
        Key::ArrowRight if jump => GridKey::JumpRight,

        Key::ArrowUp if shift => GridKey::ShiftUp,
        Key::ArrowDown if shift => GridKey::ShiftDown,
        Key::ArrowLeft if shift => GridKey::ShiftLeft,
        Key::ArrowRight if shift => GridKey::ShiftRight,

        Key::ArrowUp => GridKey::Up,
        Key::ArrowDown => GridKey::Down,
        Key::ArrowLeft => GridKey::Left,
        Key::ArrowRight => GridKey::Right,

        Key::Escape => GridKey::Escape,
        Key::Character(c) if jump && c.eq_ignore_ascii_case("a") => GridKey::SelectAll,

        // Space selects the row, Shift-Space the column — the spreadsheet
        // convention, and the only structural selection reachable by keyboard.
        Key::Character(c) if c == " " && shift => GridKey::SelectColumn,
        Key::Character(c) if c == " " => GridKey::SelectRow,

        _ => return None,
    })
}

pub use dat0_core::grid::keymap::Key as GridKey;

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers::empty();

    /// The platform's "jump" modifier, so these tests read the same on macOS
    /// and Linux instead of silently checking nothing on one of them.
    fn jump() -> Modifiers {
        if cfg!(target_os = "macos") {
            Modifiers::META
        } else {
            Modifiers::CONTROL
        }
    }

    #[test]
    fn a_bare_arrow_moves_the_cursor() {
        assert_eq!(grid_key(&Key::ArrowDown, NONE), Some(GridKey::Down));
        assert_eq!(grid_key(&Key::ArrowRight, NONE), Some(GridKey::Right));
    }

    #[test]
    fn shift_extends_and_the_platform_modifier_jumps() {
        assert_eq!(
            grid_key(&Key::ArrowDown, Modifiers::SHIFT),
            Some(GridKey::ShiftDown)
        );
        assert_eq!(grid_key(&Key::ArrowDown, jump()), Some(GridKey::JumpBottom));
    }

    #[test]
    fn jump_beats_shift_when_both_are_held() {
        // Shift-Cmd-Down is "extend to the bottom" in a spreadsheet, but the
        // model has no combined variant; jumping is the less destructive of the
        // two readings, and this pins which one we chose.
        assert_eq!(
            grid_key(&Key::ArrowDown, jump() | Modifiers::SHIFT),
            Some(GridKey::JumpBottom)
        );
    }

    #[test]
    fn select_all_is_case_insensitive() {
        assert_eq!(
            grid_key(&Key::Character("a".into()), jump()),
            Some(GridKey::SelectAll)
        );
        assert_eq!(
            grid_key(&Key::Character("A".into()), jump() | Modifiers::SHIFT),
            Some(GridKey::SelectAll)
        );
    }

    #[test]
    fn a_bare_letter_is_not_a_grid_command() {
        // Otherwise typing into a cell editor would move the cursor instead.
        assert_eq!(grid_key(&Key::Character("a".into()), NONE), None);
        assert_eq!(grid_key(&Key::Character("z".into()), NONE), None);
    }

    #[test]
    fn space_selects_a_row_and_shift_space_a_column() {
        assert_eq!(
            grid_key(&Key::Character(" ".into()), NONE),
            Some(GridKey::SelectRow)
        );
        assert_eq!(
            grid_key(&Key::Character(" ".into()), Modifiers::SHIFT),
            Some(GridKey::SelectColumn)
        );
    }

    #[test]
    fn escape_clears() {
        assert_eq!(grid_key(&Key::Escape, NONE), Some(GridKey::Escape));
    }

    #[test]
    fn an_unrelated_key_is_not_translated() {
        assert_eq!(grid_key(&Key::F1, NONE), None);
        assert_eq!(grid_key(&Key::Enter, NONE), None);
        assert_eq!(grid_key(&Key::Tab, NONE), None);
    }

    fn ch(c: &str) -> Key {
        Key::Character(c.to_string())
    }

    #[test]
    fn every_row_in_the_table_parses_into_a_chord() {
        // A row whose chord does not parse is bound to nothing and would fail
        // silently — the exact drift this table exists to prevent.
        for b in DEFAULT_KEYMAP {
            if let Some(chord) = platform_chord(b) {
                assert!(
                    Chord::parse(chord).is_some(),
                    "{:?} has an unparseable chord {chord:?}",
                    b.action
                );
            }
        }
    }

    #[test]
    fn a_chord_needs_a_key_not_just_modifiers() {
        assert!(
            Chord::parse("cmd-").is_none(),
            "a trailing separator is not a key"
        );
        assert!(Chord::parse("").is_none());
        // A bare modifier name in the key slot is nonsense but parseable; the
        // table never contains one, and `every_row_in_the_table_parses_into_a_chord`
        // is what keeps it that way.
        assert_eq!(Chord::parse("cmd-p").map(|c| c.key), Some("p".to_string()));
    }

    #[test]
    fn modifiers_must_match_exactly() {
        let c = Chord::parse("cmd-z").unwrap();
        assert!(c.matches(&ch("z"), Modifiers::META));
        // Cmd-Shift-Z is redo, a different row: a chord that ignored extra
        // modifiers would swallow it.
        assert!(!c.matches(&ch("z"), Modifiers::META | Modifiers::SHIFT));
        assert!(!c.matches(&ch("z"), NONE));
        assert!(!c.matches(&ch("y"), Modifiers::META));
    }

    #[test]
    fn named_keys_match_regardless_of_dom_casing() {
        let c = Chord::parse("escape").unwrap();
        assert!(c.matches(&Key::Escape, NONE));
        let c = Chord::parse("enter").unwrap();
        assert!(c.matches(&Key::Enter, NONE));
    }

    #[test]
    fn arrows_lose_the_dom_prefix() {
        assert_eq!(normalize(&Key::ArrowDown), "down");
        assert_eq!(normalize(&Key::ArrowUp), "up");
    }

    #[test]
    fn a_global_chord_resolves_when_nothing_else_is_open() {
        let c = Cascade::default();
        // `jump()`, not `META`: `Cascade::default()` is built from the platform
        // keymap, where undo is `cmd-z` on macOS and `ctrl-z` everywhere else.
        // Hardcoding META passed on macOS and resolved to nothing on Linux.
        let undo = c.resolve(&ch("z"), jump());
        assert_eq!(undo, Some(dat0_core::actions::builtin::ids::VIEW_UNDO));
    }

    #[test]
    fn an_unbound_key_resolves_to_nothing() {
        let c = Cascade::default();
        assert_eq!(c.resolve(&ch("q"), NONE), None);
        assert_eq!(c.resolve(&Key::F1, NONE), None);
    }

    #[test]
    fn a_modal_takes_escape_from_the_console_beneath_it() {
        // The Escape ladder: the outermost open surface closes first. Both the
        // modal and the console bind Escape, so with a modal over a focused
        // console the modal must win — otherwise dismissing a dialog would
        // cancel the query running behind it.
        let both = Cascade {
            modal_open: true,
            sql_console_focused: true,
            ..Default::default()
        };
        assert_eq!(
            both.resolve_binding(&Key::Escape, NONE).map(|b| b.context),
            Some(Some(MODAL_CONTEXT))
        );

        let console_only = Cascade {
            sql_console_focused: true,
            ..Default::default()
        };
        assert_eq!(
            console_only
                .resolve_binding(&Key::Escape, NONE)
                .map(|b| b.context),
            Some(Some(SQL_CONSOLE_CONTEXT))
        );
    }

    #[test]
    fn escape_is_inert_with_nothing_open() {
        // No global Escape row exists, and inventing one here would make Escape
        // do something arbitrary in the grid.
        assert!(
            Cascade::default()
                .resolve_binding(&Key::Escape, NONE)
                .is_none()
        );
    }

    #[test]
    fn the_palette_owns_its_own_escape() {
        // The table has no palette-scoped Escape: the palette component closes
        // itself. Asserting the absence keeps a future "helpful" global Escape
        // row from silently taking over that job.
        let palette = Cascade {
            palette_open: true,
            ..Default::default()
        };
        assert!(palette.resolve_binding(&Key::Escape, NONE).is_none());
    }

    #[test]
    fn a_scoped_chord_is_inert_when_its_surface_is_closed() {
        // The palette's arrow keys must not steal arrows from the grid.
        let closed = Cascade::default();
        let b = closed.resolve_binding(&Key::ArrowDown, NONE);
        assert!(
            b.is_none_or(|b| b.context.is_none()),
            "an arrow with no palette open must not hit a palette row: {:?}",
            b.map(|b| b.action)
        );

        let open = Cascade {
            palette_open: true,
            ..Default::default()
        };
        let b = open.resolve_binding(&Key::ArrowDown, NONE);
        assert_eq!(
            b.map(|b| b.context),
            Some(Some(PALETTE_CONTEXT)),
            "with the palette open the arrow belongs to it"
        );
    }

    #[test]
    fn the_console_scope_only_applies_when_the_console_has_focus() {
        let focused = Cascade {
            sql_console_focused: true,
            ..Default::default()
        };
        let unfocused = Cascade::default();
        let console_rows: Vec<_> = DEFAULT_KEYMAP
            .iter()
            .filter(|b| b.context == Some(SQL_CONSOLE_CONTEXT))
            .collect();
        assert!(!console_rows.is_empty(), "the table has console rows");

        for row in console_rows {
            let Some(chord) = platform_chord(row).and_then(Chord::parse) else {
                continue;
            };
            let key = Key::Character(chord.key.clone());
            let mods = mods_of(&chord);
            let hit = focused.resolve_binding(&key, mods);
            assert!(
                hit.is_some(),
                "{:?} should resolve with the console focused",
                row.action
            );
            // Unfocused, it must not resolve *into the console scope*.
            assert!(
                unfocused
                    .resolve_binding(&key, mods)
                    .is_none_or(|b| b.context != Some(SQL_CONSOLE_CONTEXT)),
                "{:?} leaked out of the console scope",
                row.action
            );
        }
    }

    fn mods_of(c: &Chord) -> Modifiers {
        let mut m = Modifiers::empty();
        if c.meta {
            m |= Modifiers::META;
        }
        if c.ctrl {
            m |= Modifiers::CONTROL;
        }
        if c.alt {
            m |= Modifiers::ALT;
        }
        if c.shift {
            m |= Modifiers::SHIFT;
        }
        m
    }
}
