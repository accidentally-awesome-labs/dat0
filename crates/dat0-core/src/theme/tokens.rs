//! The design token set.
//!
//! One field per `--d0-*` custom property, each holding a literal CSS value.
//! This is the **only** place a colour literal is allowed to originate: the
//! three builtin JSONs supply the values, `css_vars` turns a token set into a
//! `:root{…}` block, and every rule in `app.css` reads `var(--d0-…)`. The
//! `design_contract` test asserts both directions of that correspondence, so a
//! token cannot be added without a rule or referenced without existing.
//!
//! ## Two rules the design states, enforced here
//!
//! 1. **Amber is a fill, never body text.** `amber` may only reach
//!    `background`, `fill` or `border-color`. Text that must read as amber uses
//!    `amber_text`; text *on* amber uses `ink_on_amber`, which is deliberately
//!    identical in every theme because the fill it sits on is.
//! 2. **Blue, not amber, is the interaction accent.** Focus rings, the
//!    active-tab underline, the caret, selection and links are all `accent`.
//!
//! ## Why not `gpui_component::ThemeConfig`
//!
//! The old token set was a `ThemeConfig` plus a `Dat0Colors` sidecar plus
//! `Sp`/`TextRole`/`Elevation`/`Density` enums plus three `Styled` blanket
//! traits — roughly 700 lines whose entire job was to name spacing and colour
//! in Rust because GPUI had no stylesheet. CSS has one. All of it is deleted;
//! what survives is this struct and the class names in `app.css`.

use serde::{Deserialize, Serialize};

/// Light or dark. Drives `color-scheme` and the CodeMirror theme flag; it is
/// not a second source of colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
}

/// A complete token set. Every field is a CSS value — a hex colour, an
/// `rgba()`, or a `box-shadow` list.
///
/// Field order follows the design system table so a diff against the spec reads
/// top to bottom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeTokens {
    /// Stable id: `"light" | "dark" | "high-contrast"`. Persisted as `theme.id`.
    pub id: String,
    pub mode: ThemeMode,
    /// Whether panes and overlays cast shadows. High-contrast turns this off:
    /// a shadow is a soft edge, and the whole point of that theme is hard ones.
    pub shadow: bool,

    // ── Grounds ──────────────────────────────────────────────────────────────
    /// Page/window ground.
    pub canvas: String,
    /// Pane body, grid body.
    pub surface: String,
    /// Pane header, grid header.
    pub pane_head: String,
    /// Metric tiles, inset blocks.
    pub panel: String,
    /// Titlebar, sidebar, status bar.
    pub chrome: String,
    /// Chrome buttons, pills.
    pub chrome_raised: String,
    /// Chrome edges.
    pub chrome_border: String,
    /// Chrome button borders.
    pub chrome_border_2: String,
    /// Tab-strip ground.
    pub tabstrip: String,
    /// Active tab fill.
    pub tab_active: String,
    /// Tab hover.
    pub tab_hover: String,
    /// Tab separators.
    pub tab_divider: String,
    /// Grid / sidebar / palette row hover.
    pub row_hover: String,
    /// Selected row, palette selection, accent column.
    pub active_bg: String,
    /// Search-field ground.
    pub field: String,

    // ── Lines ────────────────────────────────────────────────────────────────
    /// Pane and grid-header borders.
    pub rule: String,
    /// Between grid rows; collapsed-pane border.
    pub rule_dim: String,
    /// Inputs, chips.
    pub input_border: String,

    // ── Text ─────────────────────────────────────────────────────────────────
    /// Headings, cell values, active tab text.
    pub ink: String,
    /// Body text, grid text.
    pub fg: String,
    /// Secondary text, mono labels.
    pub muted: String,
    /// Chrome and status-bar text.
    pub chrome_muted: String,

    // ── Semantics ────────────────────────────────────────────────────────────
    /// Focus ring, active-tab underline, caret, links, selection.
    pub accent: String,
    /// `local`, `egress 0 B`, the engine dot.
    pub ok: String,
    /// Failures, destructive actions.
    pub error: String,
    /// Pill text, `sealed`.
    pub warn_text: String,

    // ── Amber: fill only ─────────────────────────────────────────────────────
    /// **Fill only** — logomark, primary CTA, package chip. Never a `color:`.
    pub amber: String,
    /// CTA hover fill.
    pub amber_hover: String,
    /// Amber used *as text*.
    pub amber_text: String,
    /// Text on an amber fill. Identical across themes because the fill is.
    pub ink_on_amber: String,

    // ── SQL syntax ───────────────────────────────────────────────────────────
    pub sql_keyword: String,
    pub sql_number: String,
    pub sql_string: String,
    pub sql_fn: String,
    pub sql_comment: String,

    /// sqlite format swatch.
    pub cyan: String,

    // ── Elevation ────────────────────────────────────────────────────────────
    /// Open pane.
    pub shadow_pane: String,
    /// Palette, popovers.
    pub shadow_overlay: String,
    /// Modal scrim.
    pub scrim: String,
    /// Field inset shadow.
    pub inset: String,
}

/// The `--d0-*` name for a token field. Kept as an explicit table rather than
/// derived from field names by string munging, because the CSS names are a
/// published contract that `app.css` is written against; a rename must be a
/// visible edit here, not a silent consequence of a Rust refactor.
///
/// The `design_contract` test walks this table in both directions.
pub const CSS_NAMES: &[(&str, fn(&ThemeTokens) -> &str)] = &[
    ("--d0-canvas", |t| &t.canvas),
    ("--d0-surface", |t| &t.surface),
    ("--d0-pane-head", |t| &t.pane_head),
    ("--d0-panel", |t| &t.panel),
    ("--d0-chrome", |t| &t.chrome),
    ("--d0-chrome-raised", |t| &t.chrome_raised),
    ("--d0-chrome-border", |t| &t.chrome_border),
    ("--d0-chrome-border-2", |t| &t.chrome_border_2),
    ("--d0-tabstrip", |t| &t.tabstrip),
    ("--d0-tab-active", |t| &t.tab_active),
    ("--d0-tab-hover", |t| &t.tab_hover),
    ("--d0-tab-divider", |t| &t.tab_divider),
    ("--d0-row-hover", |t| &t.row_hover),
    ("--d0-active-bg", |t| &t.active_bg),
    ("--d0-field", |t| &t.field),
    ("--d0-rule", |t| &t.rule),
    ("--d0-rule-dim", |t| &t.rule_dim),
    ("--d0-input-border", |t| &t.input_border),
    ("--d0-ink", |t| &t.ink),
    ("--d0-fg", |t| &t.fg),
    ("--d0-muted", |t| &t.muted),
    ("--d0-chrome-muted", |t| &t.chrome_muted),
    ("--d0-accent", |t| &t.accent),
    ("--d0-ok", |t| &t.ok),
    ("--d0-error", |t| &t.error),
    ("--d0-warn-text", |t| &t.warn_text),
    ("--d0-amber", |t| &t.amber),
    ("--d0-amber-hover", |t| &t.amber_hover),
    ("--d0-amber-text", |t| &t.amber_text),
    ("--d0-ink-on-amber", |t| &t.ink_on_amber),
    ("--d0-sql-keyword", |t| &t.sql_keyword),
    ("--d0-sql-number", |t| &t.sql_number),
    ("--d0-sql-string", |t| &t.sql_string),
    ("--d0-sql-fn", |t| &t.sql_fn),
    ("--d0-sql-comment", |t| &t.sql_comment),
    ("--d0-cyan", |t| &t.cyan),
    ("--d0-shadow-pane", |t| &t.shadow_pane),
    ("--d0-shadow-overlay", |t| &t.shadow_overlay),
    ("--d0-scrim", |t| &t.scrim),
    ("--d0-inset", |t| &t.inset),
];

impl ThemeTokens {
    /// Emit the `:root{…}` declaration block.
    ///
    /// The UI renders this into a `<style id="d0-theme">`, so switching theme is
    /// a signal write — no window refresh, no re-application of a widget-library
    /// config, no restart.
    pub fn css_vars(&self) -> String {
        // 40 declarations averaging ~30 bytes, plus the wrapper.
        let mut s = String::with_capacity(1600);
        s.push_str(":root{color-scheme:");
        s.push_str(match self.mode {
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        });
        s.push(';');
        for (name, get) in CSS_NAMES {
            s.push_str(name);
            s.push(':');
            s.push_str(get(self));
            s.push(';');
        }
        if !self.shadow {
            // High contrast: hard edges only. Overriding here rather than
            // authoring "none" into the JSON keeps the shadow tokens present
            // and inspectable in every theme.
            s.push_str("--d0-shadow-pane:none;--d0-shadow-overlay:none;");
        }
        s.push('}');
        s
    }

    /// Every `(css_name, value)` pair, for tests and for the CodeMirror theme
    /// handshake.
    pub fn pairs(&self) -> impl Iterator<Item = (&'static str, &str)> {
        CSS_NAMES.iter().map(move |(n, get)| (*n, get(self)))
    }

    /// The palette handed to the CodeMirror bundle.
    ///
    /// The editor lives inside the webview but outside the CSS cascade — it
    /// builds its theme in JS through `EditorView.theme` and
    /// `HighlightStyle.define`, so it cannot read `var(--d0-…)`. Rather than
    /// let it fall back to a stock theme (and drift from the app the moment a
    /// token changes), Rust hands it the resolved values under the keys
    /// `vendor/codemirror/src/index.js` reads.
    ///
    /// This is also what makes the SQL syntax tokens *consumed*: they have no
    /// CSS rule anywhere, because nothing in the document is coloured by them.
    pub fn editor_vars(&self) -> Vec<(&'static str, &str)> {
        vec![
            (
                "mode",
                match self.mode {
                    ThemeMode::Light => "light",
                    ThemeMode::Dark => "dark",
                },
            ),
            ("surface", &self.surface),
            ("paneHead", &self.pane_head),
            ("fg", &self.fg),
            ("ink", &self.ink),
            ("muted", &self.muted),
            ("accent", &self.accent),
            ("activeBg", &self.active_bg),
            ("rowHover", &self.row_hover),
            ("ruleDim", &self.rule_dim),
            ("shadowOverlay", &self.shadow_overlay),
            ("sqlKeyword", &self.sql_keyword),
            ("sqlNumber", &self.sql_number),
            ("sqlString", &self.sql_string),
            ("sqlFn", &self.sql_fn),
            ("sqlComment", &self.sql_comment),
        ]
    }

    /// The `--d0-*` names [`editor_vars`] carries. The design-contract test
    /// treats these as consumed even though no CSS rule reads them.
    ///
    /// [`editor_vars`]: Self::editor_vars
    pub const EDITOR_TOKENS: &'static [&'static str] = &[
        "--d0-surface",
        "--d0-pane-head",
        "--d0-fg",
        "--d0-ink",
        "--d0-muted",
        "--d0-accent",
        "--d0-active-bg",
        "--d0-row-hover",
        "--d0-rule-dim",
        "--d0-shadow-overlay",
        "--d0-sql-keyword",
        "--d0-sql-number",
        "--d0-sql-string",
        "--d0-sql-fn",
        "--d0-sql-comment",
    ];
}

fn parse(id: &str, json: &str) -> ThemeTokens {
    // Builtins are compiled in; a parse failure is a programmer error, and the
    // coverage gate in `tests/theme.rs` keeps them well-formed. Loud failure
    // beats a silent fallback that ships the wrong palette.
    serde_json::from_str(json).unwrap_or_else(|e| panic!("built-in theme '{id}' must parse: {e}"))
}

/// The three builtin ids, in picker order.
pub const BUILTIN_IDS: [&str; 3] = ["light", "dark", "high-contrast"];

/// The default theme id.
///
/// **Light**, not dark: the design's explicit build target is the light
/// rendering, and it is the surface the product is marketed on. A persisted
/// `theme.id` still wins, so anyone who chose dark keeps dark.
pub const DEFAULT_ID: &str = "light";

/// The token set for a builtin id, or `None` for an unknown one.
///
/// Users cannot supply theme files — there are exactly three, all compiled in —
/// so this is total over the real input domain.
pub fn builtin(id: &str) -> Option<ThemeTokens> {
    match id {
        "light" => Some(parse("light", include_str!("builtins/light.json"))),
        "dark" => Some(parse("dark", include_str!("builtins/dark.json"))),
        "high-contrast" => Some(parse(
            "high-contrast",
            include_str!("builtins/high-contrast.json"),
        )),
        _ => None,
    }
}

/// The token set for `id`, falling back to [`DEFAULT_ID`] with a warning.
pub fn builtin_or_default(id: &str) -> ThemeTokens {
    builtin(id).unwrap_or_else(|| {
        tracing::warn!(requested = id, "unknown theme id; falling back to default");
        builtin(DEFAULT_ID).expect("the default theme must exist")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_parses_and_keeps_its_id() {
        for id in BUILTIN_IDS {
            let t = builtin(id).unwrap_or_else(|| panic!("{id} is a builtin"));
            assert_eq!(t.id, id, "the JSON's own id must match the one it is under");
        }
    }

    #[test]
    fn an_unknown_id_falls_back_to_the_default() {
        assert!(builtin("solarized").is_none());
        assert_eq!(builtin_or_default("solarized").id, DEFAULT_ID);
    }

    #[test]
    fn css_vars_declares_every_token_exactly_once() {
        let t = builtin("light").unwrap();
        let css = t.css_vars();
        for (name, _) in CSS_NAMES {
            assert_eq!(
                css.matches(&format!("{name}:")).count(),
                1,
                "{name} must appear exactly once in :root"
            );
        }
        assert!(css.starts_with(":root{"), "{css}");
        assert!(css.ends_with('}'), "{css}");
    }

    #[test]
    fn css_vars_carries_the_colour_scheme() {
        assert!(
            builtin("light")
                .unwrap()
                .css_vars()
                .contains("color-scheme:light")
        );
        assert!(
            builtin("dark")
                .unwrap()
                .css_vars()
                .contains("color-scheme:dark")
        );
    }

    #[test]
    fn a_shadowless_theme_overrides_both_shadow_tokens() {
        let mut t = builtin("light").unwrap();
        t.shadow = false;
        let css = t.css_vars();
        assert!(css.contains("--d0-shadow-pane:none"));
        assert!(css.contains("--d0-shadow-overlay:none"));
    }
}
