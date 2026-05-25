//! T0 spike covers `cx.set_global` + `cx.observe_global` semantics
//! (see `docs/internal/gpui-api-notes.md` §0.A). This test exercises
//! the pure decision logic surrounding `Theme::switch` — the loaded
//! built-ins differ from each other, and unknown ids fall back to
//! `"dark"`. Full GPUI integration runs through the manual UAT
//! (T13 retro runbook + `docs/p3b-uat-checklist.md`).

use dat0_app::theme::Theme;

#[test]
fn load_builtin_dark_and_light_differ() {
    let dark = Theme::load_builtin("dark").expect("dark builtin parses");
    let light = Theme::load_builtin("light").expect("light builtin parses");
    assert_ne!(
        dark.background(),
        light.background(),
        "dark and light themes must paint with different backgrounds — \
         otherwise observers can't visually distinguish a live switch"
    );
}

#[test]
fn load_builtin_unknown_falls_back() {
    let unknown = Theme::load_builtin_or_default("does-not-exist");
    let dark = Theme::load_builtin("dark").expect("dark builtin parses");
    assert_eq!(
        unknown.id(),
        dark.id(),
        "unknown id must fall back to the default 'dark' theme"
    );
}
