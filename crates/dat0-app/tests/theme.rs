use dat0_app::theme::Theme;

#[test]
fn dark_loads() {
    let t = Theme::load_builtin("dark").unwrap();
    assert_eq!(t.name, "dark");
}

#[test]
fn light_loads() {
    let t = Theme::load_builtin("light").unwrap();
    assert_eq!(t.name, "light");
}

#[test]
fn high_contrast_loads() {
    let t = Theme::load_builtin("high-contrast").unwrap();
    assert_eq!(t.name, "high-contrast");
}

#[test]
fn unknown_returns_err() {
    let r = Theme::load_builtin("does-not-exist");
    assert!(r.is_err());
}

// ---------------------------------------------------------------------------
// UI-redesign A1: the builtins are gpui_component::ThemeConfig documents and
// must specify EVERY color key. Sparse keys fall back to shadcn defaults at
// rev 0f0ab35 (NOT to other keys in the file) — the A0 spike showed that leak
// producing an illegible high-contrast theme. Serialize-side fact that makes
// this checkable without a hand-maintained key list: ThemeConfigColors derives
// Serialize with no skip_serializing_if, so serializing a parsed config emits
// every field, None as null.
// ---------------------------------------------------------------------------

use gpui_component::ThemeConfig;
use std::collections::BTreeSet;

const BUILTIN_SOURCES: [(&str, &str); 3] = [
    ("dark", include_str!("../src/theme/builtins/dark.json")),
    ("light", include_str!("../src/theme/builtins/light.json")),
    (
        "high-contrast",
        include_str!("../src/theme/builtins/high-contrast.json"),
    ),
];

#[test]
fn builtin_configs_parse_as_theme_config() {
    for (name, json) in BUILTIN_SOURCES {
        let cfg: ThemeConfig = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("{name}.json must parse as ThemeConfig: {e}"));
        assert_eq!(cfg.name.as_ref(), name, "name field must match the id");
    }
}

#[test]
fn builtin_configs_specify_every_color_key() {
    for (name, json) in BUILTIN_SOURCES {
        let cfg: ThemeConfig = serde_json::from_str(json).expect(name);
        let canonical = serde_json::to_value(&cfg.colors).expect("colors serialize");
        let obj = canonical
            .as_object()
            .expect("colors serializes to an object");
        let missing: Vec<&String> = obj
            .iter()
            .filter(|(_, v)| v.is_null())
            .map(|(k, _)| k)
            .collect();
        assert!(
            missing.is_empty(),
            "{name}.json is missing {} color keys (shadcn-default leak): {missing:?}",
            missing.len()
        );
    }
}

#[test]
fn builtin_configs_have_no_unknown_color_keys() {
    // serde silently ignores unknown keys — a typo'd key would otherwise
    // no-op AND pass the null check above (its real key would leak).
    for (name, json) in BUILTIN_SOURCES {
        let cfg: ThemeConfig = serde_json::from_str(json).expect(name);
        let canonical: BTreeSet<String> = serde_json::to_value(&cfg.colors)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let raw: serde_json::Value = serde_json::from_str(json).unwrap();
        let file_keys: BTreeSet<String> = raw["colors"]
            .as_object()
            .expect("colors object in file")
            .keys()
            .cloned()
            .collect();
        let unknown: Vec<&String> = file_keys.difference(&canonical).collect();
        assert!(
            unknown.is_empty(),
            "{name}.json has color keys serde would silently ignore: {unknown:?}"
        );
    }
}

#[test]
fn builtin_configs_pin_font_radius_shadow() {
    for (name, json) in BUILTIN_SOURCES {
        let cfg: ThemeConfig = serde_json::from_str(json).expect(name);
        assert_eq!(
            cfg.font_size,
            Some(14.0),
            "{name}: font.size 14 (A0 verdict)"
        );
        assert_eq!(cfg.radius, Some(5), "{name}: radius 5 (A0 spike value)");
        let expect_shadow = name != "high-contrast";
        assert_eq!(
            cfg.shadow,
            Some(expect_shadow),
            "{name}: shadow (high-contrast is flat)"
        );
    }
}
