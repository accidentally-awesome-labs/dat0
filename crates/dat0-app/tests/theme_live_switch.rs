//! Live theme switching (UI-redesign A1) — the PRODUCTION
//! `Theme::switch` path drives the `gpui_component::Theme` global:
//! dark → light → high-contrast → dark round-trip, full-coverage
//! anti-leak, unknown-id fallback, and façade-global tracking.
//! Ports the A0 spike round-trip (`tests/spike_a0.rs` on
//! `spike/ui-redesign-a0`) onto the shipped façade.

use gpui::TestAppContext;
use gpui_component::ActiveTheme as _;

use dat0_app::theme::{Theme, builtin_config};

#[test]
fn builtin_dark_and_light_differ() {
    let dark = serde_json::to_value(&builtin_config("dark").expect("dark").colors).unwrap();
    let light = serde_json::to_value(&builtin_config("light").expect("light").colors).unwrap();
    assert_ne!(
        dark["background"], light["background"],
        "dark and light must paint different backgrounds — otherwise a live \
         switch is visually indistinguishable"
    );
}

#[gpui::test]
fn switch_round_trip_restyles_gpui_component(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);

    // dark
    cx.update(|cx| {
        Theme::switch(cx, "dark");
        let t = cx.theme();
        assert!(t.mode.is_dark(), "dark config must set dark mode");
        assert_eq!(t.font_size, gpui::px(14.), "font.size 14 must apply");
        assert!(
            t.background.l < 0.15 && t.background.l > 0.0,
            "dark bg (#0e1116) lightness, got {}",
            t.background.l
        );
        assert_eq!(cx.global::<Theme>().id, "dark");
    });

    // light on top of dark
    cx.update(|cx| {
        Theme::switch(cx, "light");
        let t = cx.theme();
        assert!(!t.mode.is_dark(), "light config must set light mode");
        assert!(
            t.background.l > 0.95,
            "light bg (#ffffff), got {}",
            t.background.l
        );
        assert_eq!(t.font_size, gpui::px(14.));
        assert_eq!(cx.global::<Theme>().id, "light");
    });

    // high-contrast (third config, mode=dark) on top of light
    cx.update(|cx| {
        Theme::switch(cx, "high-contrast");
        let t = cx.theme();
        assert!(t.mode.is_dark());
        assert_eq!(t.background.l, 0.0, "HC bg must be pure black");
        assert!(
            t.ring.l > 0.45 && t.ring.l < 0.55,
            "HC ring must be yellow (#ffff00), got l={}",
            t.ring.l
        );
        // FULL-COVERAGE anti-leak: with every key specified, nothing falls
        // back to the shadcn dark defaults (the A0 sparse-config bug that
        // produced an illegible HC theme).
        let shadcn_dark = gpui_component::ThemeColor::dark();
        assert_ne!(
            t.secondary, shadcn_dark.secondary,
            "HC secondary must be authored (#1a1a1a), not the shadcn default"
        );
        assert_eq!(t.font_size, gpui::px(14.), "HC must keep font.size 14");
        assert_eq!(cx.global::<Theme>().id, "high-contrast");
    });

    // back to dark — round-trip complete
    cx.update(|cx| {
        Theme::switch(cx, "dark");
        let t = cx.theme();
        assert!(t.mode.is_dark());
        assert_eq!(t.font_size, gpui::px(14.));
        assert!(t.background.l < 0.15);
        assert_eq!(cx.global::<Theme>().id, "dark");
    });

    // unknown id falls back to dark (load_builtin_or_default semantics
    // preserved across the façade rewrite)
    cx.update(|cx| {
        Theme::switch(cx, "does-not-exist");
        assert_eq!(cx.global::<Theme>().id, "dark");
        assert!(cx.theme().mode.is_dark());
    });
}

#[gpui::test]
fn switch_without_component_global_still_installs_facade(cx: &mut TestAppContext) {
    // Pure-test contexts never run gpui_component::init — the forward must
    // no-op while the façade global (and its observers) still work.
    cx.update(|cx| {
        assert!(!cx.has_global::<gpui_component::Theme>());
        Theme::switch(cx, "light");
        assert_eq!(cx.global::<Theme>().id, "light");
        assert!(!cx.has_global::<gpui_component::Theme>());
    });
}
