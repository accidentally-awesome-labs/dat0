//! The native menu bar's labels.
//!
//! `dat0_i18n::t` returns the key itself when a key is missing, so a typo'd or
//! never-added key does not fail loudly — it ships a menu item labelled
//! `menu.file.open_workspace`. This is the gate that stops that.
//!
//! A `muda::Menu` cannot be built off the platform main thread, so these tests
//! work from [`menu::label_keys`] rather than the live bar. That list is kept
//! honest by [`every_label_key_in_build_is_declared`], which reads `build`'s
//! own source: the list cannot silently fall behind the menu it describes.

use dat0_ui::menu;

#[test]
fn every_menu_label_resolves_to_a_translation() {
    let keys = menu::label_keys();
    assert!(
        keys.len() >= 5,
        "expected at least the five top-level menus, got {}",
        keys.len()
    );
    for key in keys {
        assert_ne!(
            dat0_i18n::t(key),
            key,
            "menu label key `{key}` has no translation — it would ship as the key"
        );
    }
}

#[test]
fn label_keys_are_declared_once_each() {
    let keys = menu::label_keys();
    let mut seen = std::collections::BTreeSet::new();
    for key in &keys {
        assert!(seen.insert(*key), "`{key}` is listed twice");
    }
}

/// The drift gate: every i18n key `build` actually renders must be declared in
/// [`menu::label_keys`], or the resolution test above silently stops covering
/// it. Read from source because the real menu is unbuildable here.
#[test]
fn every_label_key_in_build_is_declared() {
    const SRC: &str = include_str!("../src/menu.rs");
    let declared: std::collections::BTreeSet<&str> = menu::label_keys().into_iter().collect();

    let body = build_body(SRC);
    let used = dotted_literals(body);
    assert!(
        used.len() > 20,
        "the scrape found only {} keys in `build` — it has stopped matching the source",
        used.len()
    );
    for key in used {
        assert!(
            declared.contains(key.as_str()),
            "`build` renders i18n key `{key}` but `label_keys()` does not list it"
        );
    }
}

/// The body of `pub fn build() -> Menu`, up to its closing brace in column 0.
fn build_body(src: &str) -> &str {
    const OPEN: &str = "pub fn build() -> Menu {";
    let start = src.find(OPEN).expect("`build` must exist") + OPEN.len();
    let rest = &src[start..];
    let end = rest.find("\n}").expect("`build` must be closed");
    &rest[..end]
}

/// Every `"a.b.c"` string literal in `body`. Menu labels are the only dotted
/// literals `build` contains — ids arrive as `ids::` / `menu_ids::` paths.
fn dotted_literals(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('"') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('"') else { break };
        let lit = &rest[..close];
        rest = &rest[close + 1..];
        let dotted = lit.contains('.')
            && !lit.starts_with('.')
            && !lit.ends_with('.')
            && lit
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.');
        if dotted {
            out.push(lit.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}
