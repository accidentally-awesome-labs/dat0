//! dat0 design-system scales (UI-redesign A2, master plan §3/§5).
//!
//! Everything here is a pure function of the single `gpui_component::Theme`
//! global — colors are DERIVED ON READ (`cx.theme().d0().focus_ring`), never
//! cached in a second global, so theme switches can never go stale and the
//! high-contrast palette propagates automatically. Strict zero-literal
//! policy: no color constructors in this file (self-lint test below).

use gpui::{FontWeight, Hsla, Pixels, Styled, px, relative};
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
    fn tokens_module_stays_literal_free() {
        // Zero-literal policy (owner decision 2026-07-23): colors in this
        // module must derive from theme tokens. Patterns are assembled by
        // concatenation so this test can't match itself. Forerunner of the
        // A4 repo-wide style lint.
        let src = include_str!("tokens.rs");
        let banned = [
            format!("rgb{}", "(0x"),
            format!("rgba{}", "(0x"),
            format!("parse{}", "_hex"),
            format!("hsla{}", "("),
            format!("rgb{}", "a("),
        ];
        for pat in &banned {
            assert!(
                !src.contains(pat.as_str()),
                "tokens.rs must stay color-literal-free; found `{pat}`"
            );
        }
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
        assert_eq!(style.padding.top, Some(Sp::S8.pixels().into()));
        assert_eq!(style.padding.left, Some(Sp::S8.pixels().into()));
        assert_eq!(style.gap.width, Some(Sp::S4.pixels().into()));

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
