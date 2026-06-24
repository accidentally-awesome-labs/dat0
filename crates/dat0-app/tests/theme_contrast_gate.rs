//! WCAG AA contrast gate over the three builtin themes (P10b a11y).
//! Fails CI if any required fg/bg pair drops below threshold. `border`
//! is intentionally exempt (decorative; WCAG 1.4.11 carve-out).
//!
//! Grid selection uses a hardcoded ring (`border_2().border_color`) plus a
//! translucent tint overlay (`rgba(0x3b82f622)` / `rgba(0x3b82f611)`). The
//! cell text colour is NOT changed for selected/active cells — foreground
//! text still renders on the background colour (branch A). Therefore no
//! `foreground`-on-`accent`-fill text pair exists and no `selection_fg`
//! token is needed; `accent`/`background` at ≥4.5 already covers the
//! ring-vs-background requirement and strictly subsumes any 3.0 check.

use dat0_app::theme::{Theme, contrast::contrast_ratio};

fn style_color<'a>(t: &'a Theme, name: &str) -> &'a str {
    match name {
        "background" => &t.style.background,
        "foreground" => &t.style.foreground,
        "accent" => &t.style.accent,
        "error" => &t.style.error,
        "success" => &t.style.success,
        "warning" => &t.style.warning,
        other => panic!("unknown token {other}"),
    }
}

#[test]
fn builtin_themes_meet_wcag_aa() {
    // (token_a, token_b, min_ratio)
    // Branch A: grid selection is a ring + translucent tint (no fill);
    // foreground text remains on background. No additional pair needed.
    let matrix: &[(&str, &str, f64)] = &[
        ("foreground", "background", 4.5),
        ("error", "background", 4.5),
        ("success", "background", 4.5),
        ("warning", "background", 4.5),
        ("accent", "background", 4.5),
    ];
    let mut failures = vec![];
    for name in ["light", "dark", "high-contrast"] {
        let t = Theme::load_builtin(name).expect("builtin parses");
        for (a, b, min) in matrix {
            let r = contrast_ratio(style_color(&t, a), style_color(&t, b));
            eprintln!("{name}: {a}/{b} = {r:.2}:1 (min {min})");
            if r < *min {
                failures.push(format!("{name}: {a}/{b} = {r:.2}:1 < {min}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "WCAG AA contrast failures:\n{}",
        failures.join("\n")
    );
}
