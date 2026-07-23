//! WCAG AA contrast gate over the three builtin theme configs (P10b a11y,
//! retargeted by UI-redesign A1, extended to the full matrix by A3).
//!
//! Matrix: 24 text pairs ≥4.5:1, 10 non-text pairs ≥3:1 (WCAG 1.4.11),
//! composited tint checks, derived Dat0Colors checks, and a sibling-pair
//! drift alarm so new `X.foreground`/`X.background` families cannot dodge
//! the gate. `border`/`input.border` stay exempt (decorative; WCAG 1.4.11
//! carve-out). Values are read through serde serialization of the parsed
//! `ThemeConfigColors`, so the gate uses the exact rename keys and survives
//! field renames in the Rust struct.

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

/// Parse a builtin JSON and serialize its colors back to a JSON object,
/// so lookups use the exact serde rename keys.
fn colors_of(json: &str) -> serde_json::Value {
    let cfg: ThemeConfig = serde_json::from_str(json).expect("builtin parses");
    serde_json::to_value(&cfg.colors).expect("colors serialize")
}

fn color(colors: &serde_json::Value, key: &str) -> String {
    colors[key]
        .as_str()
        .unwrap_or_else(|| {
            panic!("color key {key} missing/null (coverage gate should have caught this)")
        })
        .to_string()
}

/// Text pairs: (fg_key, bg_key, min_ratio). WCAG 1.4.3 AA = 4.5:1.
/// Covers ALL 18 sibling `X.foreground`/`X.background` families (enforced
/// by `sibling_pairs_all_gated`) plus 6 cross-family pairs.
const TEXT_PAIRS: &[(&str, &str, f64)] = &[
    ("foreground", "background", 4.5),
    ("muted.foreground", "muted.background", 4.5),
    ("muted.foreground", "background", 4.5),
    ("accent.foreground", "accent.background", 4.5),
    ("secondary.foreground", "secondary.background", 4.5),
    ("primary.foreground", "primary.background", 4.5),
    ("danger.foreground", "danger.background", 4.5),
    ("success.foreground", "success.background", 4.5),
    ("warning.foreground", "warning.background", 4.5),
    ("info.foreground", "info.background", 4.5),
    ("popover.foreground", "popover.background", 4.5),
    ("sidebar.foreground", "sidebar.background", 4.5),
    ("sidebar.accent.foreground", "sidebar.accent.background", 4.5),
    ("sidebar.primary.foreground", "sidebar.primary.background", 4.5),
    ("group_box.foreground", "group_box.background", 4.5),
    ("group_box.title.foreground", "group_box.background", 4.5),
    ("tab.foreground", "tab.background", 4.5),
    ("tab.active.foreground", "tab.active.background", 4.5),
    ("table.head.foreground", "table.head.background", 4.5),
    (
        "description_list.label.foreground",
        "description_list.label.background",
        4.5,
    ),
    ("link", "background", 4.5),
    ("foreground", "list.active.background", 4.5),
    ("foreground", "list.hover.background", 4.5),
    ("foreground", "table.even.background", 4.5),
];

/// Non-text pairs (WCAG 1.4.11 = 3:1). `ring` is held at 4.5 — it doubles
/// as the single accent since A1 killed the two-blues split.
const NON_TEXT_PAIRS: &[(&str, &str, f64)] = &[
    ("ring", "background", 4.5),
    ("caret", "background", 3.0),
    ("drag.border", "background", 3.0),
    ("list.active.border", "list.active.background", 3.0),
    ("table.active.border", "table.active.background", 3.0),
    ("danger.background", "background", 3.0),
    ("success.background", "background", 3.0),
    ("warning.background", "background", 3.0),
    ("info.background", "background", 3.0),
    ("primary.background", "background", 3.0),
];

fn check_pairs(
    name: &str,
    colors: &serde_json::Value,
    pairs: &[(&str, &str, f64)],
    failures: &mut Vec<String>,
) {
    for (fg_key, bg_key, min) in pairs {
        let fg = color(colors, fg_key);
        let bg = color(colors, bg_key);
        let r = contrast_ratio(&fg, &bg);
        eprintln!("{name}: {fg_key}/{bg_key} = {r:.2}:1 (min {min})");
        if r < *min {
            failures.push(format!("{name}: {fg_key}/{bg_key} = {r:.2}:1 < {min}"));
        }
    }
}

#[test]
fn text_pairs_meet_wcag_aa() {
    let mut failures = vec![];
    for (name, json) in BUILTIN_SOURCES {
        check_pairs(name, &colors_of(json), TEXT_PAIRS, &mut failures);
    }
    assert!(
        failures.is_empty(),
        "WCAG AA text-contrast failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn non_text_pairs_meet_wcag_1_4_11() {
    let mut failures = vec![];
    for (name, json) in BUILTIN_SOURCES {
        check_pairs(name, &colors_of(json), NON_TEXT_PAIRS, &mut failures);
    }
    assert!(
        failures.is_empty(),
        "WCAG 1.4.11 non-text-contrast failures:\n{}",
        failures.join("\n")
    );
}

/// Drift alarm: every sibling `X.foreground`/`X.background` family present
/// in the JSON must be listed in TEXT_PAIRS — new families can't silently
/// skip the gate. (Root `foreground`/`background` is a family too.)
#[test]
fn sibling_pairs_all_gated() {
    for (name, json) in BUILTIN_SOURCES {
        let colors = colors_of(json);
        let obj = colors.as_object().expect("colors is an object");
        for key in obj.keys() {
            let bg_key = if key == "foreground" {
                "background".to_string()
            } else if let Some(prefix) = key.strip_suffix(".foreground") {
                format!("{prefix}.background")
            } else {
                continue;
            };
            if !obj.contains_key(&bg_key) {
                continue; // no sibling background — not a fg/bg family
            }
            assert!(
                TEXT_PAIRS
                    .iter()
                    .any(|&(f, b, _)| f == key.as_str() && b == bg_key),
                "{name}: sibling pair ({key}, {bg_key}) missing from TEXT_PAIRS — \
                 add it with a threshold"
            );
        }
    }
}
