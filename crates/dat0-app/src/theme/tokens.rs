//! dat0 design-system scales (UI-redesign A2, master plan §3/§5).
//!
//! Everything here is a pure function of the single `gpui_component::Theme`
//! global — colors are DERIVED ON READ (`cx.theme().d0().focus_ring`), never
//! cached in a second global, so theme switches can never go stale and the
//! high-contrast palette propagates automatically. Strict zero-literal
//! policy: no color constructors in this file, enforced repo-wide by
//! `tests/style_lint.rs` (A4).

use gpui::{FontWeight, Hsla, Pixels, Rems, Styled, px, relative, rems};
use gpui_component::Theme;

/// dat0-specific color semantics, derived from the active
/// [`gpui_component::Theme`] every time [`Dat0Theme::d0`] is called.
/// Field-by-field derivation map + the inline-hex call sites each field
/// replaces in Slice A6: design doc §1
/// (`docs/plans/2026-07-23-dat0-ui-redesign-a2-token-scales-design.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct Dat0Colors {
    pub focus_ring: Hsla,
    pub selection_tint: Hsla,
    /// No production consumer, and that is a finding rather than an oversight.
    /// A6 deviation 1 searched for a fill-handle render site and found none —
    /// `grid/mod.rs:72-76` records why the obvious candidate took `primary` /
    /// `primary_foreground` instead (it is the column-reorder ghost, it paints
    /// text on the fill, so it needs a text pair the contrast gate already
    /// covers; `fill_handle` is ring@0.72, tuned by A3 for the non-text 3:1
    /// bar). Rendered in the gallery only. Kept, on the A5 `Play`/`Bookmark`
    /// precedent: do not invent a consumer, and do not delete a tuned token a
    /// grid fill handle would want. Delete it if that feature is ruled out.
    pub fill_handle: Hsla,
    pub active_cell_tint: Hsla,
    pub marching_ants: Hsla,
    pub null_value_fg: Hsla,
    pub banner_info: Hsla,
    pub banner_warning: Hsla,
    pub banner_error: Hsla,
    pub banner_tint: Hsla,
    pub hover_tint: Hsla,
    pub drag_over: Hsla,
    pub pipeline_pill: Hsla,
    pub pipeline_accent: Hsla,
    pub pipeline_chip: Hsla,
    pub text_muted: Hsla,
    pub text_error: Hsla,
    pub chart_placeholder_a: Hsla,
    pub chart_placeholder_b: Hsla,
    pub pager_dot_active: Hsla,
    pub pager_dot_inactive: Hsla,
}

/// Access trait: `cx.theme().d0().focus_ring`.
pub trait Dat0Theme {
    fn d0(&self) -> Dat0Colors;
}

impl Dat0Theme for Theme {
    fn d0(&self) -> Dat0Colors {
        // `Theme` derefs to `ThemeColor`, so `self.ring` etc. read the
        // active palette. Alpha factors are eyeball-matched to the pre-A6
        // inline values (0x22≈0.13, 0xaa≈0.65→0.72 (A3: light-theme 3:1),
        // 0x11≈0.07, 0x40=0.25, 0x14≈0.08); the A3 contrast matrix is their
        // correctness gate.
        Dat0Colors {
            focus_ring: self.ring,
            selection_tint: self.ring.opacity(0.13),
            fill_handle: self.ring.opacity(0.72),
            active_cell_tint: self.ring.opacity(0.07),
            marching_ants: self.success,
            null_value_fg: self.muted_foreground,
            banner_info: self.info,
            banner_warning: self.warning,
            banner_error: self.danger,
            banner_tint: self.muted_foreground.opacity(0.08),
            hover_tint: self.list_hover,
            drag_over: self.drop_target,
            pipeline_pill: self.ring.opacity(0.25),
            pipeline_accent: self.primary,
            pipeline_chip: self.secondary,
            text_muted: self.muted_foreground,
            text_error: self.danger,
            chart_placeholder_a: self.chart_2,
            chart_placeholder_b: self.chart_1,
            pager_dot_active: self.foreground,
            pager_dot_inactive: self.muted_foreground,
        }
    }
}

/// Spacing scale (px). The ONLY spacing values new dat0 UI should use
/// (master plan §3); A6 migrates magic px call sites onto it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sp {
    S1 = 1,
    S2 = 2,
    S4 = 4,
    S6 = 6,
    S8 = 8,
    S12 = 12,
    S16 = 16,
    S24 = 24,
    S32 = 32,
}

impl Sp {
    /// The scale as a rem-relative length, against the CSS-conventional 16px
    /// rem the gpui helper scale is defined in (`gpui-macros/src/styles.rs`
    /// emits `.gap_1()` as `rems(0.25)`, documented "4px (0.25rem)").
    ///
    /// **The only accessor, deliberately.** dat0's rem is **14px**, not 16:
    /// `gpui_component::Root::render` calls
    /// `window.set_rem_size(cx.theme().font_size)` and A1 set `"font.size": 14`
    /// in all three builtins. An absolute `Sp` therefore sat 14% looser than
    /// every gpui-spaced element beside it, and the codebase runs 196 gpui
    /// helper sites against 26 `Sp` sites. Expressed this way `Sp::S4` **is**
    /// `.gap_1()`, exactly, and the two scales are one.
    ///
    /// `Sp` still earns its keep: it is a restricted, named 9-step subset of
    /// gpui's open scale, and it survives a future `font.size` change without
    /// re-forking. `sp_rems_matches_gpui_helper_scale` is what holds that.
    ///
    /// Absolute lengths — hairline rules, fixed panel widths, type sizes,
    /// corner radii — are not spacing and must not come from here. Use
    /// `gpui::px` directly and say so at the call site.
    pub fn rems(self) -> Rems {
        rems(self as u16 as f32 / 16.)
    }
}

impl From<Sp> for Rems {
    fn from(sp: Sp) -> Rems {
        sp.rems()
    }
}

/// Spacing helpers so call sites stay terse: `.p_sp(Sp::S8)`.
pub trait SpStyled: Styled + Sized {
    fn p_sp(self, sp: Sp) -> Self {
        self.p(sp.rems())
    }
    fn px_sp(self, sp: Sp) -> Self {
        self.px(sp.rems())
    }
    fn py_sp(self, sp: Sp) -> Self {
        self.py(sp.rems())
    }
    fn gap_sp(self, sp: Sp) -> Self {
        self.gap(sp.rems())
    }
    fn m_sp(self, sp: Sp) -> Self {
        self.m(sp.rems())
    }
}

impl<E: Styled> SpStyled for E {}

/// Desktop typography ladder (master plan §3 + owner decision 2026-07-23:
/// size + weight + line-height per role, so surfaces can't half-apply the
/// ladder). Body is 13px against the A1 `font.size` 14 root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRole {
    Caption,
    Small,
    Body,
    BodyLg,
    Title,
    Display,
}

impl TextRole {
    pub fn size(self) -> Pixels {
        px(match self {
            TextRole::Caption => 11.,
            TextRole::Small => 12.,
            TextRole::Body => 13.,
            TextRole::BodyLg => 14.,
            TextRole::Title => 16.,
            TextRole::Display => 20.,
        })
    }

    pub fn weight(self) -> FontWeight {
        match self {
            TextRole::Title => FontWeight::MEDIUM,
            TextRole::Display => FontWeight::SEMIBOLD,
            _ => FontWeight::NORMAL,
        }
    }

    /// Line height as a multiple of the role's font size
    /// (`gpui::relative` fraction semantics).
    pub fn line_height_factor(self) -> f32 {
        match self {
            TextRole::Caption | TextRole::Small => 1.4,
            TextRole::Body | TextRole::BodyLg => 1.5,
            TextRole::Title => 1.3,
            TextRole::Display => 1.2,
        }
    }
}

/// `.text_role(TextRole::Title)` applies size + weight + line-height in one
/// call — the centralized map is the point (no per-site weight drift).
pub trait TypoStyled: Styled + Sized {
    fn text_role(self, role: TextRole) -> Self {
        self.text_size(role.size())
            .font_weight(role.weight())
            .line_height(relative(role.line_height_factor()))
    }
}

impl<E: Styled> TypoStyled for E {}

/// Surface-elevation ladder (master plan §3). One enum drives bg + border +
/// radius + shadow TOGETHER (Zed `ElevationIndex` pattern) so surfaces can't
/// mix rungs. Shadows are gated on `theme.shadow` — the A1 high-contrast
/// builtin sets `shadow:false`, so HC stays flat and the always-painted
/// border carries the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevation {
    Background,
    Surface,
    Raised,
    Overlay,
    Modal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowLevel {
    None,
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElevationStyle {
    pub bg: Hsla,
    pub border: Hsla,
    pub radius: Pixels,
    pub shadow: ShadowLevel,
}

impl Elevation {
    /// Pure resolution — testable without a window.
    pub fn resolve(self, theme: &Theme) -> ElevationStyle {
        let gate = |level| {
            if theme.shadow {
                level
            } else {
                ShadowLevel::None
            }
        };
        match self {
            Elevation::Background => ElevationStyle {
                bg: theme.background,
                border: theme.border,
                radius: px(0.),
                shadow: ShadowLevel::None,
            },
            Elevation::Surface => ElevationStyle {
                bg: theme.sidebar,
                border: theme.sidebar_border,
                radius: px(0.),
                shadow: ShadowLevel::None,
            },
            Elevation::Raised => ElevationStyle {
                bg: theme.popover,
                border: theme.border,
                radius: theme.radius,
                shadow: gate(ShadowLevel::Small),
            },
            Elevation::Overlay => ElevationStyle {
                bg: theme.popover,
                border: theme.border,
                radius: theme.radius,
                shadow: gate(ShadowLevel::Medium),
            },
            Elevation::Modal => ElevationStyle {
                bg: theme.popover,
                border: theme.border,
                radius: theme.radius_lg,
                shadow: gate(ShadowLevel::Large),
            },
        }
    }
}

/// `.elevation(Elevation::Overlay, cx.theme())` — applies the whole resolved
/// rung (bg, border, radius, shadow) in one call.
pub trait ElevationStyled: Styled + Sized {
    fn elevation(self, rung: Elevation, theme: &Theme) -> Self {
        let style = rung.resolve(theme);
        let this = self
            .bg(style.bg)
            .border_1()
            .border_color(style.border)
            .rounded(style.radius);
        match style.shadow {
            ShadowLevel::None => this,
            ShadowLevel::Small => this.shadow_sm(),
            ShadowLevel::Medium => this.shadow_md(),
            ShadowLevel::Large => this.shadow_lg(),
        }
    }
}

impl<E: Styled> ElevationStyled for E {}

/// Global density policy → gpui-component [`Size`](gpui_component::Size).
/// dat0 is a dense data workbench: the grid defaults to Compact (26px table
/// rows). A user-facing density setting is post-v1 (master plan §5 optional).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Density {
    Compact,
    Default,
    Comfortable,
}

impl Density {
    pub fn size(self) -> gpui_component::Size {
        match self {
            Density::Compact => gpui_component::Size::XSmall,
            Density::Default => gpui_component::Size::Medium,
            Density::Comfortable => gpui_component::Size::Large,
        }
    }
}

/// The grid's density policy (applied at A6f via `Table…with_size`).
pub fn grid_density() -> Density {
    Density::Compact
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    /// Standalone Theme styled by a builtin config — no gpui App needed
    /// (`apply_config` is a plain `&mut self` method, verified at rev 0f0ab35).
    pub(super) fn theme_for(id: &str) -> Theme {
        let cfg = crate::theme::builtin_config(id).expect("builtin theme id");
        let mut theme = Theme::default();
        theme.apply_config(&Rc::new(cfg.clone()));
        theme
    }

    #[test]
    fn dat0_colors_derive_from_active_palette() {
        let dark = theme_for("dark");
        let light = theme_for("light");
        let hc = theme_for("high-contrast");

        // Contract: fields are pure functions of the theme's own tokens.
        assert_eq!(dark.d0().focus_ring, dark.ring);
        assert_eq!(hc.d0().focus_ring, hc.ring);
        assert_eq!(hc.d0().marching_ants, hc.success);
        assert_eq!(hc.d0().null_value_fg, hc.muted_foreground);
        assert_eq!(light.d0().drag_over, light.drop_target);
        assert_eq!(dark.d0().hover_tint, dark.list_hover);
        assert_eq!(dark.d0().chart_placeholder_a, dark.chart_2);
        assert_eq!(dark.d0().chart_placeholder_b, dark.chart_1);
        assert_eq!(dark.d0().banner_info, dark.info);
        assert_eq!(dark.d0().banner_warning, dark.warning);
        assert_eq!(dark.d0().banner_error, dark.danger);
        assert_eq!(dark.d0().pipeline_accent, dark.primary);
        assert_eq!(dark.d0().pipeline_chip, dark.secondary);
        assert_eq!(dark.d0().text_error, dark.danger);
        assert_eq!(dark.d0().pager_dot_active, dark.foreground);
        assert_eq!(dark.d0().pager_dot_inactive, dark.muted_foreground);

        // The three palettes actually differ (ring: #58a6ff / #0969da /
        // #ffff00) → derived fields differ. Proves apply_config took effect
        // and high-contrast auto-propagates — the reason d0() exists.
        assert_ne!(dark.d0().focus_ring, light.d0().focus_ring);
        assert_ne!(dark.d0().focus_ring, hc.d0().focus_ring);
        assert_ne!(dark.d0().text_muted, light.d0().text_muted);
    }

    #[test]
    fn alpha_tints_scale_the_source_alpha() {
        let dark = theme_for("dark");
        let d0 = dark.d0();
        // Hsla::opacity multiplies alpha and leaves h/s/l untouched.
        assert!((d0.selection_tint.a - dark.ring.a * 0.13).abs() < 1e-4);
        assert!((d0.fill_handle.a - dark.ring.a * 0.72).abs() < 1e-4);
        assert!((d0.active_cell_tint.a - dark.ring.a * 0.07).abs() < 1e-4);
        assert!((d0.pipeline_pill.a - dark.ring.a * 0.25).abs() < 1e-4);
        assert!((d0.banner_tint.a - dark.muted_foreground.a * 0.08).abs() < 1e-4);
        assert_eq!(d0.selection_tint.h, dark.ring.h);
        assert_eq!(d0.selection_tint.l, dark.ring.l);
    }

    #[test]
    fn sp_scale_exact_values() {
        // The scale's identity is its step values; the unit is rems, against a
        // 16px reference rem (see `Sp::rems`). Resolved pixels at dat0's real
        // 14px rem live in `sp_rems_resolve_at_dat0_rem_size`.
        let expect = [
            (Sp::S1, 1.),
            (Sp::S2, 2.),
            (Sp::S4, 4.),
            (Sp::S6, 6.),
            (Sp::S8, 8.),
            (Sp::S12, 12.),
            (Sp::S16, 16.),
            (Sp::S24, 24.),
            (Sp::S32, 32.),
        ];
        for (sp, v) in expect {
            assert_eq!(sp.rems(), rems(v / 16.), "{sp:?}");
            assert_eq!(Rems::from(sp), rems(v / 16.));
        }
    }

    /// `Sp` and gpui's helper scale are the SAME scale, so an `Sp`-spaced
    /// container and a `.gap_1()`-spaced sibling agree. Without this, the two
    /// silently re-fork the next time `font.size` moves.
    #[test]
    fn sp_rems_matches_gpui_helper_scale() {
        // gpui-macros/src/styles.rs: .gap_1() == rems(0.25), .gap_2() ==
        // rems(0.5), .px_3() == rems(0.75).
        assert_eq!(Sp::S4.rems(), rems(0.25));
        assert_eq!(Sp::S8.rems(), rems(0.5));
        assert_eq!(Sp::S12.rems(), rems(0.75));
        assert_eq!(Sp::S16.rems(), rems(1.0));
        assert_eq!(Sp::S32.rems(), rems(2.0));
    }

    /// What a user actually sees, at dat0's real rem size. The assertion above
    /// only proves two constants match; this one states pixels.
    #[test]
    fn sp_rems_resolve_at_dat0_rem_size() {
        // gpui_component::Root::render sets rem_size from theme.font_size,
        // and A1 pinned "font.size": 14 in all three builtins.
        let rem = px(14.);
        assert_eq!(Sp::S1.rems().to_pixels(rem), px(0.875));
        assert_eq!(Sp::S4.rems().to_pixels(rem), px(3.5));
        assert_eq!(Sp::S8.rems().to_pixels(rem), px(7.));
        assert_eq!(Sp::S12.rems().to_pixels(rem), px(10.5));
        assert_eq!(Sp::S32.rems().to_pixels(rem), px(28.));
    }

    /// THE GATE: `Rems` must flow into every setter `SpStyled` calls —
    /// padding/gap take `impl Into<DefiniteLength>`, margin takes `Length`.
    /// gpui provides `From<Rems>` for both, but compiled beats argued.
    #[test]
    fn rems_flows_through_every_styled_setter() {
        // One element per setter, deliberately. Chaining them would let `py`
        // overwrite the `padding.top` that `p` had just set, which measures
        // gpui's setter semantics rather than the `Rems` conversion this gate
        // is about.
        let mut p = gpui::div().p(Sp::S8.rems());
        assert_eq!(p.style().padding.top, Some(Sp::S8.rems().into()));

        let mut px_el = gpui::div().px(Sp::S8.rems());
        assert_eq!(px_el.style().padding.left, Some(Sp::S8.rems().into()));

        let mut py_el = gpui::div().py(Sp::S4.rems());
        assert_eq!(py_el.style().padding.top, Some(Sp::S4.rems().into()));

        let mut g = gpui::div().gap(Sp::S4.rems());
        assert_eq!(g.style().gap.width, Some(Sp::S4.rems().into()));

        let mut m = gpui::div().m(Sp::S2.rems());
        assert_eq!(m.style().margin.top, Some(Sp::S2.rems().into()));
    }

    #[test]
    fn text_role_ladder_exact_values() {
        use TextRole::*;
        let expect = [
            (Caption, 11., FontWeight::NORMAL, 1.4),
            (Small, 12., FontWeight::NORMAL, 1.4),
            (Body, 13., FontWeight::NORMAL, 1.5),
            (BodyLg, 14., FontWeight::NORMAL, 1.5),
            (Title, 16., FontWeight::MEDIUM, 1.3),
            (Display, 20., FontWeight::SEMIBOLD, 1.2),
        ];
        for (role, size, weight, lh) in expect {
            assert_eq!(role.size(), px(size), "{role:?} size");
            assert_eq!(role.weight(), weight, "{role:?} weight");
            assert!(
                (role.line_height_factor() - lh).abs() < f32::EPSILON,
                "{role:?} line-height"
            );
        }
    }

    #[test]
    fn elevation_shadow_gates_on_theme_shadow() {
        let dark = theme_for("dark"); // shadow: true (A1 builtin)
        let hc = theme_for("high-contrast"); // shadow: false — HC stays flat
        assert!(
            dark.shadow && !hc.shadow,
            "A1 builtin shadow flags moved — update this test's premise"
        );

        assert_eq!(
            Elevation::Background.resolve(&dark).shadow,
            ShadowLevel::None
        );
        assert_eq!(Elevation::Surface.resolve(&dark).shadow, ShadowLevel::None);
        assert_eq!(Elevation::Raised.resolve(&dark).shadow, ShadowLevel::Small);
        assert_eq!(
            Elevation::Overlay.resolve(&dark).shadow,
            ShadowLevel::Medium
        );
        assert_eq!(Elevation::Modal.resolve(&dark).shadow, ShadowLevel::Large);

        for rung in [
            Elevation::Background,
            Elevation::Surface,
            Elevation::Raised,
            Elevation::Overlay,
            Elevation::Modal,
        ] {
            assert_eq!(
                rung.resolve(&hc).shadow,
                ShadowLevel::None,
                "{rung:?} must be flat in HC"
            );
        }
    }

    #[test]
    fn elevation_geometry_and_backgrounds() {
        let dark = theme_for("dark");
        // bg ladder: background → sidebar → popover (A1 palette #0e1116 →
        // #151a21 → #1a2029); floating rungs share popover, differ by
        // shadow strength + radius.
        assert_eq!(Elevation::Background.resolve(&dark).bg, dark.background);
        assert_eq!(Elevation::Surface.resolve(&dark).bg, dark.sidebar);
        assert_eq!(
            Elevation::Surface.resolve(&dark).border,
            dark.sidebar_border
        );
        assert_eq!(Elevation::Raised.resolve(&dark).bg, dark.popover);
        assert_eq!(Elevation::Overlay.resolve(&dark).bg, dark.popover);
        assert_eq!(Elevation::Modal.resolve(&dark).bg, dark.popover);

        assert_eq!(Elevation::Background.resolve(&dark).radius, px(0.));
        assert_eq!(Elevation::Surface.resolve(&dark).radius, px(0.));
        assert_eq!(Elevation::Raised.resolve(&dark).radius, dark.radius);
        assert_eq!(Elevation::Overlay.resolve(&dark).radius, dark.radius);
        assert_eq!(Elevation::Modal.resolve(&dark).radius, dark.radius_lg);
    }

    #[test]
    fn density_maps_to_component_size_row_heights() {
        use gpui_component::Size;
        assert_eq!(Density::Compact.size(), Size::XSmall);
        assert_eq!(Density::Default.size(), Size::Medium);
        assert_eq!(Density::Comfortable.size(), Size::Large);
        // Pin the upstream row heights the dense-workbench policy relies on
        // (styled.rs:250 at rev 0f0ab35) — a rev bump that moves these must
        // fail loudly here, not silently re-density the grid.
        assert_eq!(Density::Compact.size().table_row_height(), px(26.));
        assert_eq!(Density::Default.size().table_row_height(), px(32.));
        assert_eq!(Density::Comfortable.size().table_row_height(), px(40.));
        assert_eq!(grid_density(), Density::Compact);
    }

    #[test]
    fn helper_traits_apply_resolved_styles() {
        // Owner-approved addition (2026-07-23): the *_sp / text_role /
        // elevation helpers are the surface A6 consumes — assert they
        // write the resolved values into the element's StyleRefinement,
        // not just that the underlying maps are right.
        use gpui::Styled as _;
        let dark = theme_for("dark");
        let hc = theme_for("high-contrast");

        // TypoStyled: all three text properties land, with Title's values.
        let mut el = gpui::div().text_role(TextRole::Title);
        let text = el
            .text_style()
            .clone()
            .expect("text_role must set text style");
        assert_eq!(text.font_size, Some(TextRole::Title.size().into()));
        assert_eq!(text.font_weight, Some(TextRole::Title.weight()));
        assert_eq!(
            text.line_height,
            Some(relative(TextRole::Title.line_height_factor()))
        );

        // SpStyled: padding + gap land with the scale value.
        let mut el = gpui::div().p_sp(Sp::S8).gap_sp(Sp::S4);
        let style = el.style();
        assert_eq!(style.padding.top, Some(Sp::S8.rems().into()));
        assert_eq!(style.padding.left, Some(Sp::S8.rems().into()));
        assert_eq!(style.gap.width, Some(Sp::S4.rems().into()));

        // ElevationStyled: border color + radius + shadow presence track resolve().
        let resolved = Elevation::Modal.resolve(&dark);
        let mut el = gpui::div().elevation(Elevation::Modal, &dark);
        let style = el.style();
        assert_eq!(style.border_color, Some(resolved.border));
        assert_eq!(style.corner_radii.top_left, Some(resolved.radius.into()));
        assert!(
            style.background.is_some(),
            "elevation must set a background"
        );
        assert!(
            style.box_shadow.as_ref().is_some_and(|s| !s.is_empty()),
            "Modal in dark casts a shadow"
        );

        let mut el = gpui::div().elevation(Elevation::Modal, &hc);
        assert!(el.style().box_shadow.is_none(), "high-contrast stays flat");
    }
}
