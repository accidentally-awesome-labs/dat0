//! The ⌘⇧P command palette.
//!
//! Ranking and visibility live in [`dat0_core::command_palette`], which is
//! toolkit-free and unit-tested there; this file is the surface. The split is
//! the GPUI one, kept for the same reason: what the palette *contains* is a
//! product rule, and a product rule tested through a renderer is a product rule
//! nobody re-tests.
//!
//! # The list is windowed
//!
//! The palette can list every registered action, so it uses the grid's
//! [`visible_range`] — the same arithmetic, the same overscan — over a canvas
//! sized to the whole list. This was the app's only `uniform_list`; without it
//! the DOM would carry a node per action on every keystroke.
//!
//! Row pitch is the grid's [`ROW_H`]; a row is one pixel shorter, and that
//! pixel is the design's 1px gap between rows.
//!
//! # Keys
//!
//! ↑/↓ resolve through [`Cascade`] against the real keymap table rather than
//! matching `Key::ArrowUp` here, because `dat0_core::keymap` is where the
//! palette's arrows are declared and a second spelling of them is a second
//! thing to drift. Enter and Escape are matched literally: in the GPUI build
//! neither was a palette row either — Enter came from the `Input` widget's
//! `PressEnter` and Escape from the modal trap's `Dat0Modal` context, and the
//! palette is not in the Dioxus shell's modal slot.
//!
//! The selection **clamps** at both ends rather than wrapping, matching the
//! GPUI original: a list surface clamps, only a radio group wraps. Changing it
//! here would put the ring on a command the user did not walk to.

use std::rc::Rc;

use dioxus::html::geometry::PixelsVector2D;
use dioxus::prelude::*;

use dat0_core::actions::registry::{ActionGroup, ActionRegistry};
use dat0_core::command_palette::visible_items;
use dat0_core::events::AppEvents;
use dat0_core::keymap::chord_for;

use crate::a11y::AccessRole;
use crate::components::grid::{ROW_H, Viewport, visible_range};
use crate::keys::Cascade;
use crate::state::Workspace;

/// The list's height before the first real scroll event reports one.
///
/// `max-height: 46vh` is a CSS fact the harness cannot know and the first paint
/// has not measured yet, so the window is computed against a plausible value
/// and corrected by the first `onscroll`. 320px is what the GPUI list reserved.
const LIST_H: f64 = 320.0;

/// One rendered row.
///
/// A flattened, comparable projection of `ActionDescriptor`: the descriptor
/// itself carries an `Arc<dyn Fn>` and implements no `PartialEq`, so it cannot
/// be memoised. Actions are named by id and performed by the shell's router, so
/// the view never needs the closure.
#[derive(Clone, PartialEq, Eq)]
struct Row {
    id: String,
    title: String,
    /// The group, as a decorative glyph.
    glyph: &'static str,
    /// The group's name, for the row tooltip. The design's row has three slots
    /// and the glyph takes the group's; this is how the group stays readable
    /// without a fourth.
    group: String,
    /// The live chord, pretty-printed. `None` for a deliberately chord-less
    /// action.
    hint: Option<String>,
}

/// The palette, mounted unconditionally by the shell and gated on the signal.
#[component]
pub fn CommandPalette() -> Element {
    let ws = Workspace::use_current();
    // A fresh scope per open, so the query and the ring reset without an
    // effect that has to remember to fire — the GPUI palette was a new entity
    // every time too.
    rsx! {
        if *ws.palette.read() {
            PaletteBody {}
        }
    }
}

#[component]
fn PaletteBody() -> Element {
    let mut ws = Workspace::use_current();
    let registry = crate::components::registry();
    let events = crate::components::events();

    let mut query = use_signal(String::new);
    let mut active = use_signal(|| 0usize);
    let mut viewport = use_signal(|| Viewport {
        scroll_top: 0.0,
        scroll_left: 0.0,
        width: 640.0,
        height: LIST_H,
    });
    // The scrolling container, once the renderer has one. `None` in the
    // headless harness, which has no layout and therefore nothing to scroll.
    let mut list_el = use_signal(|| Option::<Rc<MountedData>>::None);

    // Rebuilt on query change, never per frame: `ActionRegistry::iter` clones
    // every descriptor (an `Arc` per dispatch closure) and a render-time
    // rebuild would pay that on every frame.
    let reg_for_items = registry.clone();
    let items = use_memo(move || rows(&reg_for_items, &query()));

    let all = items();
    let count = all.len();
    let vp = viewport();
    let range = visible_range(vp, count, &[vp.width]);
    let canvas_h = count as f64 * ROW_H;
    let selected = active();

    // Palette scope only: anything this handler does not claim keeps bubbling
    // to the shell root, which is where the global chords live.
    let cascade = Cascade {
        palette_open: true,
        ..Cascade::default()
    };

    let reg_key = registry.clone();
    let ev_key = events.clone();

    let mut step = move |delta: isize| {
        let n = items.read().len();
        if n == 0 {
            return;
        }
        let last = (n - 1) as isize;
        let next = (active() as isize + delta).clamp(0, last) as usize;
        active.set(next);

        // Keep the ring on screen. Without this the selection walks off the
        // fold and a keyboard-first surface loses its user.
        let mut vp = viewport();
        let top = scroll_top_showing(vp, next);
        if top != vp.scroll_top {
            vp.scroll_top = top;
            viewport.set(vp);
            if let Some(el) = list_el() {
                spawn(async move {
                    let _ = el
                        .scroll(PixelsVector2D::new(0.0, top), ScrollBehavior::Instant)
                        .await;
                });
            }
        }
    };

    rsx! {
        div {
            class: "d0-scrim",
            "data-a11y-id": "palette-scrim",
            onclick: move |_| ws.palette.set(false),

            div {
                class: "d0-palette",
                "data-a11y-id": "palette",
                role: AccessRole::Dialog.aria(),
                "aria-modal": "true",
                "aria-label": dat0_i18n::t("palette.title"),
                tabindex: "-1",
                // The scrim closes on click; a click inside the panel is not a
                // click on the scrim.
                onclick: move |e| e.stop_propagation(),
                onkeydown: move |e| {
                    match palette_key(cascade, &e.key(), e.modifiers()) {
                        PaletteKey::Close => {
                            e.stop_propagation();
                            ws.palette.set(false);
                        }
                        PaletteKey::Run => {
                            e.stop_propagation();
                            if let Some(row) = items.read().get(active()) {
                                run_action(ws, &reg_key, &ev_key, &row.id);
                            }
                        }
                        PaletteKey::Step(delta) => {
                            e.prevent_default();
                            e.stop_propagation();
                            step(delta);
                        }
                        PaletteKey::Trap => {
                            e.prevent_default();
                            e.stop_propagation();
                        }
                        // Everything else belongs to whoever is above us — do
                        // not swallow it.
                        PaletteKey::Fallthrough => {}
                    }
                },

                div { class: "d0-palette-input",
                    span { class: "d0-palette-prompt", "aria-hidden": "true", "›" }
                    input {
                        class: "d0-mono",
                        "data-a11y-id": "palette-query",
                        "aria-label": dat0_i18n::t("palette.search"),
                        placeholder: dat0_i18n::t("palette.placeholder"),
                        value: "{query}",
                        autofocus: true,
                        oninput: move |e| {
                            query.set(e.value());
                            // Reset rather than clamp: after a narrowing
                            // keystroke, row 2 of the OLD list is a different
                            // command than row 2 of the new one, and Enter
                            // would run it.
                            active.set(0);
                            let mut vp = viewport();
                            vp.scroll_top = 0.0;
                            viewport.set(vp);
                        },
                    }
                    span { class: "d0-mono d0-hint", "aria-hidden": "true",
                        {dat0_i18n::t("palette.hint.dismiss")}
                    }
                }

                div {
                    class: "d0-palette-list",
                    "data-a11y-id": "palette-list",
                    onmounted: move |e| list_el.set(Some(e.data())),
                    onscroll: move |e| {
                        let d = e.data();
                        viewport
                            .set(Viewport {
                                scroll_top: d.scroll_top(),
                                scroll_left: d.scroll_left(),
                                width: f64::from(d.client_width()),
                                height: f64::from(d.client_height()),
                            });
                    },

                    if count == 0 {
                        p {
                            class: "d0-palette-empty d0-mono",
                            "data-a11y-id": "palette-empty",
                            role: AccessRole::Label.aria(),
                            "aria-label": dat0_i18n::t("palette.no_results"),
                            {dat0_i18n::t("palette.no_results")}
                        }
                    } else {
                        div {
                            class: "d0-palette-canvas",
                            style: "height: {canvas_h}px;",

                            for i in range.rows.clone() {
                                {
                                    let row = all[i].clone();
                                    let id = row.id.clone();
                                    let reg_row = registry.clone();
                                    let ev_row = events.clone();
                                    rsx! {
                                        button {
                                            key: "{row.id}",
                                            class: if i == selected { "d0-palette-row is-selected" } else { "d0-palette-row" },
                                            "data-a11y-id": "palette-row-{i}",
                                            role: AccessRole::Button.aria(),
                                            "aria-label": "{row.title}",
                                            "aria-selected": if i == selected { "true" } else { "false" },
                                            title: "{row.group}",
                                            tabindex: "-1",
                                            style: "top: {i as f64 * ROW_H}px;",
                                            onclick: move |e| {
                                                e.stop_propagation();
                                                run_action(ws, &reg_row, &ev_row, &id);
                                            },
                                            span { class: "d0-palette-glyph", "aria-hidden": "true", "{row.glyph}" }
                                            span { class: "d0-palette-label", "{row.title}" }
                                            if let Some(hint) = row.hint.clone() {
                                                span { class: "d0-hint", "aria-hidden": "true", "{hint}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                div { class: "d0-palette-foot d0-mono",
                    span { {dat0_i18n::t("palette.hint.move")} }
                    span { {dat0_i18n::t("palette.hint.open")} }
                }
            }
        }
    }
}

/// What the palette does with a keystroke.
///
/// A resolver rather than a `match` buried in the handler, for the reason
/// [`crate::components::modals::trap_action`] is one: the precedence is the
/// interesting part, and a rule that exists only inside an `onkeydown` closure
/// can only be tested through a rendered tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKey {
    /// Dismiss without running anything.
    Close,
    /// Run the row the ring is on.
    Run,
    /// Move the ring by this many rows.
    Step(isize),
    /// Consume it and change nothing.
    Trap,
    /// Not the palette's key — let it bubble to the shell's cascade.
    Fallthrough,
}

/// Resolve a keystroke against the open palette.
///
/// Escape and Enter are matched literally: neither was a palette row under
/// GPUI either — Enter came from the `Input` widget's `PressEnter` and Escape
/// from the modal trap's `Dat0Modal` context — and the arrows come from
/// `dat0_core::keymap` so the live binding and the palette's own hint cannot
/// disagree.
///
/// Tab is [`PaletteKey::Trap`]: the panel declares `aria-modal`, so Tab must
/// not hand the keyboard to the surface it obscures (WCAG 2.4.3 — the GPUI
/// palette inherited that trap by living in `mounted_modals`). The query field
/// is the palette's only tab stop — the rows are `tabindex="-1"` and are
/// reached with the arrows — so consuming the key *is* the cycle.
pub fn palette_key(cascade: Cascade, key: &Key, mods: Modifiers) -> PaletteKey {
    debug_assert!(
        cascade.palette_open,
        "the palette's grammar only applies while it is open"
    );
    match key {
        Key::Escape => PaletteKey::Close,
        Key::Enter => PaletteKey::Run,
        Key::Tab => PaletteKey::Trap,
        _ => match cascade.resolve_binding(key, mods).and_then(|b| b.action) {
            Some("dat0_palette::PaletteUp") => PaletteKey::Step(-1),
            Some("dat0_palette::PaletteDown") => PaletteKey::Step(1),
            _ => PaletteKey::Fallthrough,
        },
    }
}

/// Dismiss, then perform.
///
/// The order is load-bearing: a routed action may open a modal of its own, and
/// a palette left mounted underneath it is an overlay nobody can reach.
fn run_action(mut ws: Workspace, reg: &ActionRegistry, events: &AppEvents, id: &str) {
    ws.palette.set(false);
    reg.dispatch(id, events);
}

/// Project the registry's ranked descriptors into rows.
fn rows(reg: &ActionRegistry, query: &str) -> Vec<Row> {
    visible_items(reg, query)
        .into_iter()
        .map(|d| Row {
            glyph: glyph(d.group),
            group: group_label(d.group),
            // The chord comes from the same table the bindings are installed
            // from, so a hint here cannot disagree with the live key path. The
            // descriptor's old hand-typed `keybinding` field could.
            hint: chord_for(d.id.as_str()).map(pretty_chord),
            id: d.id.as_str().to_string(),
            title: d.title,
        })
        .collect()
}

/// The scroll offset that brings row `ix` fully into view, or the current one
/// when it already is.
///
/// Pure so the "the ring never leaves the fold" rule is testable without a
/// layout engine, which the headless harness does not have.
pub fn scroll_top_showing(vp: Viewport, ix: usize) -> f64 {
    let top = ix as f64 * ROW_H;
    let bottom = top + ROW_H;
    if top < vp.scroll_top {
        top
    } else if bottom > vp.scroll_top + vp.height {
        (bottom - vp.height).max(0.0)
    } else {
        vp.scroll_top
    }
}

/// A decorative mark for the group, filling the design row's first slot.
/// `aria-hidden` at the call site — the group reaches a reader through the
/// row's tooltip, not through a symbol nobody can pronounce.
fn glyph(group: ActionGroup) -> &'static str {
    match group {
        ActionGroup::Navigation => "→",
        ActionGroup::Theme => "◐",
        ActionGroup::File => "▤",
        ActionGroup::Settings => "⚙",
        ActionGroup::Recovery => "↺",
        ActionGroup::Import => "⤓",
        ActionGroup::Edit => "✎",
    }
}

fn group_label(group: ActionGroup) -> String {
    dat0_i18n::t(match group {
        ActionGroup::Navigation => "palette.group.navigation",
        ActionGroup::Theme => "palette.group.theme",
        ActionGroup::File => "palette.group.file",
        ActionGroup::Settings => "palette.group.settings",
        ActionGroup::Recovery => "palette.group.recovery",
        ActionGroup::Import => "palette.group.import",
        ActionGroup::Edit => "palette.group.edit",
    })
}

/// `"cmd-shift-p"` -> `"⌘⇧P"`.
///
/// Modifier order is the platform's, not the table's: the keymap spells chords
/// in whatever order reads well in source, and a hint that renders `⇧⌘P` looks
/// like a different chord than the menu bar's.
pub fn pretty_chord(chord: &str) -> String {
    let (mut ctrl, mut alt, mut shift, mut meta) = (false, false, false, false);
    let mut key = "";
    for part in chord.split('-') {
        match part {
            "ctrl" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            "cmd" | "super" | "win" => meta = true,
            other => key = other,
        }
    }

    let mut out = String::new();
    if cfg!(target_os = "macos") {
        for (on, sym) in [(ctrl, "⌃"), (alt, "⌥"), (shift, "⇧"), (meta, "⌘")] {
            if on {
                out.push_str(sym);
            }
        }
        out.push_str(&key_symbol(key));
    } else {
        for (on, word) in [
            (ctrl, "Ctrl"),
            (alt, "Alt"),
            (shift, "Shift"),
            (meta, "Super"),
        ] {
            if on {
                out.push_str(word);
                out.push('+');
            }
        }
        out.push_str(&key_symbol(key));
    }
    out
}

fn key_symbol(key: &str) -> String {
    match key {
        "enter" => "⏎".to_string(),
        "escape" => "⎋".to_string(),
        "backspace" => "⌫".to_string(),
        "delete" => "⌦".to_string(),
        "tab" => "⇥".to_string(),
        "space" => "␣".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        other => other.to_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp(scroll_top: f64, height: f64) -> Viewport {
        Viewport {
            scroll_top,
            scroll_left: 0.0,
            width: 640.0,
            height,
        }
    }

    #[test]
    fn a_row_already_in_view_does_not_scroll() {
        let v = vp(0.0, 260.0);
        assert_eq!(scroll_top_showing(v, 0), 0.0);
        assert_eq!(scroll_top_showing(v, 5), 0.0);
    }

    #[test]
    fn a_row_below_the_fold_scrolls_it_to_the_bottom_edge() {
        // 260px / 26px = exactly 10 rows, so row 10 is the first one out.
        let v = vp(0.0, 260.0);
        assert_eq!(scroll_top_showing(v, 10), ROW_H * 11.0 - 260.0);
    }

    #[test]
    fn a_row_above_the_fold_scrolls_it_to_the_top_edge() {
        let v = vp(ROW_H * 20.0, 260.0);
        assert_eq!(scroll_top_showing(v, 3), ROW_H * 3.0);
    }

    #[test]
    fn the_offset_never_goes_negative() {
        // A viewport taller than the whole list must not scroll backwards.
        let v = vp(0.0, 4000.0);
        assert_eq!(scroll_top_showing(v, 2), 0.0);
    }

    #[test]
    fn chords_render_in_platform_modifier_order() {
        // The table spells this one `cmd-shift-p`; the hint must not.
        let p = pretty_chord("cmd-shift-p");
        if cfg!(target_os = "macos") {
            assert_eq!(p, "⇧⌘P");
        } else {
            assert_eq!(p, "Shift+Super+P");
        }
    }

    #[test]
    fn named_keys_get_their_symbol() {
        let run = pretty_chord(if cfg!(target_os = "macos") {
            "cmd-enter"
        } else {
            "ctrl-enter"
        });
        assert!(run.ends_with('⏎'), "{run}");
    }

    #[test]
    fn every_group_has_its_own_glyph() {
        use ActionGroup::*;
        let all = [Navigation, Theme, File, Settings, Recovery, Import, Edit];
        let mut seen: Vec<&str> = all.iter().map(|g| glyph(*g)).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), all.len(), "two groups share a glyph");
    }
}
