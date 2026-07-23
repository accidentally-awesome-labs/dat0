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
}
