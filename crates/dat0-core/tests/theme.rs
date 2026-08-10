//! The three builtin theme documents.
//!
//! Port of `dat0-app/tests/theme.rs`. The subject moved from
//! `gpui_component::ThemeConfig` + a `*.brand.json` sidecar to one
//! [`ThemeTokens`] document per theme, but the failure this file exists to
//! catch is unchanged: **a theme that ships with a hole in it**.
//!
//! Under `ThemeConfig` the hole was silent — every colour was an `Option`, and
//! an unspecified key fell back to a shadcn default (which is how the A0 spike
//! produced an illegible high-contrast theme). `ThemeTokens` has no `Option`
//! fields, so a *missing* key is now a parse error; that half is proven below
//! rather than assumed. The half that is still silent is an *extra* key: serde
//! ignores unknown fields, so a stray or renamed entry in the JSON no-ops while
//! the real token keeps whatever value it had. Hence the key-set equality gate.
//!
//! The brand sidecars are gone: `amber`, `amber_hover`, `amber_text`,
//! `ink_on_amber` and `ok` are ordinary tokens in the same document now, so the
//! six sidecar gates collapse into the ones below plus the contrast matrix in
//! `theme_tokens_contrast.rs`.

use std::collections::BTreeSet;

use dat0_core::theme::tokens::{BUILTIN_IDS, DEFAULT_ID, ThemeTokens, builtin, builtin_or_default};

const BUILTIN_SOURCES: [(&str, &str); 3] = [
    ("light", include_str!("../src/theme/builtins/light.json")),
    ("dark", include_str!("../src/theme/builtins/dark.json")),
    (
        "high-contrast",
        include_str!("../src/theme/builtins/high-contrast.json"),
    ),
];

fn keys(value: &serde_json::Value) -> BTreeSet<String> {
    value
        .as_object()
        .expect("a theme document is a JSON object")
        .keys()
        .cloned()
        .collect()
}

/// The key set a parsed document serialises back to — i.e. exactly the fields
/// `ThemeTokens` knows about.
fn canonical_keys(tokens: &ThemeTokens) -> BTreeSet<String> {
    keys(&serde_json::to_value(tokens).expect("ThemeTokens serialises"))
}

/// Every builtin id resolves to a document that names itself, and nothing else
/// resolves at all.
#[test]
fn every_builtin_loads_under_its_own_id() {
    for (id, _) in BUILTIN_SOURCES {
        let t = builtin(id).unwrap_or_else(|| panic!("{id} must be a builtin"));
        assert_eq!(t.id, id, "{id}.json names itself {:?}", t.id);
    }
    assert_eq!(BUILTIN_IDS.len(), BUILTIN_SOURCES.len());
    assert!(builtin("does-not-exist").is_none());
}

/// The document and the struct name exactly the same fields.
///
/// Both directions matter and they catch different bugs. A key in the file that
/// the struct does not know is silently dropped by serde — the token keeps its
/// value from wherever else it came, and the edit appears to have worked. A
/// field the file does not mention cannot happen (it is a parse error, proven
/// by `a_missing_token_fails_to_parse_rather_than_defaulting`), but asserting
/// the equality rather than the one-way containment means a future
/// `#[serde(default)]` on any field is caught here instead of shipping.
#[test]
fn every_document_names_exactly_the_tokens_the_struct_defines() {
    for (id, json) in BUILTIN_SOURCES {
        let raw: serde_json::Value =
            serde_json::from_str(json).unwrap_or_else(|e| panic!("{id}.json is JSON: {e}"));
        let parsed: ThemeTokens =
            serde_json::from_str(json).unwrap_or_else(|e| panic!("{id}.json parses: {e}"));

        let in_file = keys(&raw);
        let in_struct = canonical_keys(&parsed);

        let unknown: Vec<&String> = in_file.difference(&in_struct).collect();
        assert!(
            unknown.is_empty(),
            "{id}.json declares keys serde silently ignores: {unknown:?}"
        );
        let absent: Vec<&String> = in_struct.difference(&in_file).collect();
        assert!(absent.is_empty(), "{id}.json does not mention: {absent:?}");
    }
}

/// Why the gate above is load-bearing: serde really does accept — and drop — a
/// key the struct does not define.
#[test]
fn an_unknown_key_is_dropped_without_complaint() {
    let (_, light) = BUILTIN_SOURCES[0];
    let mut doc: serde_json::Value = serde_json::from_str(light).unwrap();
    let real = doc["canvas"].clone();
    doc.as_object_mut()
        .unwrap()
        .insert("canvass".into(), serde_json::json!("#ff00ff"));

    let parsed: ThemeTokens = serde_json::from_value(doc).expect("the typo parses fine");
    assert_eq!(
        serde_json::Value::String(parsed.canvas),
        real,
        "the typo'd key must have gone nowhere — which is precisely the failure \
         `every_document_names_exactly_the_tokens_the_struct_defines` catches"
    );
}

/// A token the document omits is a loud failure, not a default.
///
/// This is the direct replacement for the `ThemeConfig` coverage gate: there,
/// every colour was an `Option` and an omission leaked a shadcn default into a
/// hand-tuned palette. Here every field is required — asserted by removing each
/// one in turn, so a `#[serde(default)]` added to any single field fails the
/// gate rather than quietly reintroducing the leak.
#[test]
fn a_missing_token_fails_to_parse_rather_than_defaulting() {
    for (id, json) in BUILTIN_SOURCES {
        let doc: serde_json::Value = serde_json::from_str(json).unwrap();
        for key in keys(&doc) {
            let mut without = doc.clone();
            without.as_object_mut().unwrap().remove(&key);
            assert!(
                serde_json::from_value::<ThemeTokens>(without).is_err(),
                "{id}.json parses with {key} removed — that field has a default, \
                 so a theme can ship without specifying it"
            );
        }
    }
}

/// High contrast is flat; the other two cast shadows. A shadow is a soft edge,
/// and hard edges are that theme's entire reason to exist.
#[test]
fn only_high_contrast_is_flat() {
    for id in BUILTIN_IDS {
        let t = builtin(id).unwrap();
        assert_eq!(
            t.shadow,
            id != "high-contrast",
            "{id}: shadow must be off in high-contrast and on everywhere else"
        );
        // …and the flat theme must actually emit the override, not merely
        // record the intent.
        let css = t.css_vars();
        assert_eq!(
            css.contains("--d0-shadow-pane:none"),
            !t.shadow,
            "{id}: the shadow flag and the emitted :root block disagree"
        );
    }
}

/// Each id resolves to its **own** palette.
///
/// The GPUI original asserted this against three brand sidecars, to catch a
/// mis-wired `match` arm falling through to dark. There is one match now
/// (`builtin`), and this is the same guarantee: the three documents are
/// genuinely distinct where the design says they differ, and identical only
/// where it says they must be.
#[test]
fn each_builtin_carries_its_own_palette() {
    let light = builtin("light").unwrap();
    let dark = builtin("dark").unwrap();
    let hc = builtin("high-contrast").unwrap();

    assert_ne!(
        light.canvas, dark.canvas,
        "light and dark must paint different grounds"
    );
    assert_ne!(light.canvas, hc.canvas, "high-contrast must not be light");
    assert_ne!(light.accent, dark.accent, "the accent is tuned per ground");
    assert_ne!(light.ok, dark.ok, "light must darken the ok green");
    assert_ne!(
        light.amber_text, dark.amber_text,
        "light must darken amber-as-text; #f5a623 on a white ground measures 1.97:1"
    );

    // Deliberately identical, and the design says so: the amber *fill* is one
    // brand colour, and the ink on it therefore cannot move either.
    assert_eq!(light.amber, dark.amber);
    assert_eq!(light.amber, hc.amber);
    assert_eq!(light.ink_on_amber, dark.ink_on_amber);
    assert_eq!(light.ink_on_amber, hc.ink_on_amber);
}

/// S9: the default is **light**, and an unknown id lands there.
///
/// The GPUI build defaulted to dark and fell back to dark. Both changed, so the
/// new behaviour is pinned rather than left to a comment — a persisted
/// `theme.id` still wins, which is what keeps existing dark users on dark.
#[test]
fn the_default_theme_is_light_and_unknown_ids_fall_back_to_it() {
    assert_eq!(DEFAULT_ID, "light");
    assert_eq!(builtin_or_default("does-not-exist").id, "light");
    assert_eq!(
        builtin_or_default("dark").id,
        "dark",
        "a known id still wins"
    );
}
