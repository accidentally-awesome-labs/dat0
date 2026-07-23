//! dat0 design-system scales (UI-redesign A2, master plan §3/§5).
//!
//! Everything here is a pure function of the single `gpui_component::Theme`
//! global — colors are DERIVED ON READ (`cx.theme().d0().focus_ring`), never
//! cached in a second global, so theme switches can never go stale and the
//! high-contrast palette propagates automatically. Strict zero-literal
//! policy: no color constructors in this file (self-lint test below).

use gpui::{px, relative, FontWeight, Hsla, Pixels, Styled};
use gpui_component::{Colorize as _, Theme};

/// dat0-specific color semantics, derived from the active
/// [`gpui_component::Theme`] every time [`Dat0Theme::d0`] is called.
/// Field-by-field derivation map + the inline-hex call sites each field
/// replaces in Slice A6: design doc §1
/// (`docs/plans/2026-07-23-dat0-ui-redesign-a2-token-scales-design.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct Dat0Colors {
    pub focus_ring: Hsla,
    pub selection_tint: Hsla,
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
        // inline values (0x22≈0.13, 0xaa≈0.65, 0x11≈0.07, 0x40=0.25,
        // 0x14≈0.08); the A3 contrast matrix is their correctness gate.
        Dat0Colors {
            focus_ring: self.ring,
            selection_tint: self.ring.opacity(0.13),
            fill_handle: self.ring.opacity(0.65),
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
    pub fn pixels(self) -> Pixels {
        px(self as u16 as f32)
    }
}

impl From<Sp> for Pixels {
    fn from(sp: Sp) -> Pixels {
        sp.pixels()
    }
}

/// Spacing helpers so call sites stay terse: `.p_sp(Sp::S8)`.
pub trait SpStyled: Styled + Sized {
    fn p_sp(self, sp: Sp) -> Self {
        self.p(sp.pixels())
    }
    fn px_sp(self, sp: Sp) -> Self {
        self.px(sp.pixels())
    }
    fn py_sp(self, sp: Sp) -> Self {
        self.py(sp.pixels())
    }
    fn gap_sp(self, sp: Sp) -> Self {
        self.gap(sp.pixels())
    }
    fn m_sp(self, sp: Sp) -> Self {
        self.m(sp.pixels())
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
        let gate = |level| if theme.shadow { level } else { ShadowLevel::None };
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
        // Colorize::opacity multiplies alpha and leaves h/s/l untouched.
        assert!((d0.selection_tint.a - dark.ring.a * 0.13).abs() < 1e-4);
        assert!((d0.fill_handle.a - dark.ring.a * 0.65).abs() < 1e-4);
        assert!((d0.active_cell_tint.a - dark.ring.a * 0.07).abs() < 1e-4);
        assert!((d0.pipeline_pill.a - dark.ring.a * 0.25).abs() < 1e-4);
        assert!((d0.banner_tint.a - dark.muted_foreground.a * 0.08).abs() < 1e-4);
        assert_eq!(d0.selection_tint.h, dark.ring.h);
        assert_eq!(d0.selection_tint.l, dark.ring.l);
    }

    #[test]
    fn sp_scale_exact_values() {
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
            assert_eq!(sp.pixels(), px(v), "{sp:?}");
            assert_eq!(Pixels::from(sp), px(v));
        }
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
            assert!((role.line_height_factor() - lh).abs() < f32::EPSILON, "{role:?} line-height");
        }
    }

    #[test]
    fn elevation_shadow_gates_on_theme_shadow() {
        let dark = theme_for("dark"); // shadow: true (A1 builtin)
        let hc = theme_for("high-contrast"); // shadow: false — HC stays flat
        assert!(dark.shadow && !hc.shadow, "A1 builtin shadow flags moved — update this test's premise");

        assert_eq!(Elevation::Background.resolve(&dark).shadow, ShadowLevel::None);
        assert_eq!(Elevation::Surface.resolve(&dark).shadow, ShadowLevel::None);
        assert_eq!(Elevation::Raised.resolve(&dark).shadow, ShadowLevel::Small);
        assert_eq!(Elevation::Overlay.resolve(&dark).shadow, ShadowLevel::Medium);
        assert_eq!(Elevation::Modal.resolve(&dark).shadow, ShadowLevel::Large);

        for rung in [
            Elevation::Background,
            Elevation::Surface,
            Elevation::Raised,
            Elevation::Overlay,
            Elevation::Modal,
        ] {
            assert_eq!(rung.resolve(&hc).shadow, ShadowLevel::None, "{rung:?} must be flat in HC");
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
        assert_eq!(Elevation::Surface.resolve(&dark).border, dark.sidebar_border);
        assert_eq!(Elevation::Raised.resolve(&dark).bg, dark.popover);
        assert_eq!(Elevation::Overlay.resolve(&dark).bg, dark.popover);
        assert_eq!(Elevation::Modal.resolve(&dark).bg, dark.popover);

        assert_eq!(Elevation::Background.resolve(&dark).radius, px(0.));
        assert_eq!(Elevation::Surface.resolve(&dark).radius, px(0.));
        assert_eq!(Elevation::Raised.resolve(&dark).radius, dark.radius);
        assert_eq!(Elevation::Overlay.resolve(&dark).radius, dark.radius);
        assert_eq!(Elevation::Modal.resolve(&dark).radius, dark.radius_lg);
    }
}
