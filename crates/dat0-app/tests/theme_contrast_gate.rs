//! WCAG AA contrast gate over the three builtin theme configs (P10b a11y,
//! retargeted by UI-redesign A1 to the gpui_component::ThemeConfig shape —
//! the SAME parsed document production applies via `apply_config`).
//!
//! A1 keeps the P10b 5-pair floor; slice A3 extends the matrix (~15 text
//! pairs, ~10 non-text 3:1 pairs, 8-digit-hex alpha compositing) and does
//! the final palette tuning. `border` stays exempt (decorative; WCAG
//! 1.4.11 carve-out). Values are read through serde serialization of the
//! parsed `ThemeConfigColors`, so the gate uses the exact rename keys and
//! survives field renames in the Rust struct.

use dat0_app::theme::contrast::contrast_ratio;
use gpui_component::ThemeConfig;

const BUILTIN_SOURCES: [(&str, &str); 3] = [
    ("dark", include_str!("../src/theme/builtins/dark.json")),
    ("light", include_str!("../src/theme/builtins/light.json")),
    (
        "high-contrast",
        include_str!("../src/theme/builtins/high-contrast.json"),
    ),
];

fn color(colors: &serde_json::Value, key: &str) -> String {
    colors[key]
        .as_str()
        .unwrap_or_else(|| {
            panic!("color key {key} missing/null (coverage gate should have caught this)")
        })
        .to_string()
}

#[test]
fn builtin_themes_meet_wcag_aa() {
    // (fg_key, min_ratio) — each checked against "background".
    // ring replaces the old `accent` token: it IS the single accent now
    // (the two-blues split died in A1).
    let matrix: &[(&str, f64)] = &[
        ("foreground", 4.5),
        ("danger.background", 4.5),
        ("success.background", 4.5),
        ("warning.background", 4.5),
        ("ring", 4.5),
    ];
    let mut failures = vec![];
    for (name, json) in BUILTIN_SOURCES {
        let cfg: ThemeConfig = serde_json::from_str(json).expect("builtin parses");
        let colors = serde_json::to_value(&cfg.colors).expect("colors serialize");
        let bg = color(&colors, "background");
        for (key, min) in matrix {
            let fg = color(&colors, key);
            let r = contrast_ratio(&fg, &bg);
            eprintln!("{name}: {key}/background = {r:.2}:1 (min {min})");
            if r < *min {
                failures.push(format!("{name}: {key}/background = {r:.2}:1 < {min}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "WCAG AA contrast failures:\n{}",
        failures.join("\n")
    );
}
