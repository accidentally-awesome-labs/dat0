//! Gates for the icon asset chain.
//!
//! Port of `dat0-app/tests/icon_assets.rs`. The central failure is still
//! silent: a missing or misnamed icon does not raise anything. Under GPUI an
//! unresolved `AssetSource` path rendered as nothing; over the `dat0` protocol
//! it is a 404 inside the webview, which surfaces to no Rust code at all.
//!
//! Two of the original gates lost their subject and are not reproduced here:
//!
//! * `dat0_icons_do_not_shadow_bundled` watched for a name collision between
//!   dat0's own icons and `gpui-component-assets`'s, because the loader tried
//!   dat0's embed first and would silently shadow an upstream icon. There is no
//!   upstream embed and no fallback arm — dat0 vendors all fourteen — so there
//!   is nothing left to shadow.
//! * `empty_path_is_none` is `protocol.rs`'s own
//!   `embed_key_rejects_traversal_and_empty_paths`, which covers the empty and
//!   the `..` cases together.

use std::collections::BTreeSet;

use dat0_ui::protocol::{Embedded, url};

/// The icon set, by design: the six the widget library used to supply, plus
/// dat0's own eight.
///
/// Listed here as a *closed* set. `protocol.rs` already asserts each of these
/// is present; what it cannot assert from the inside is that nothing else
/// crept in — a stray SVG dropped into `assets/icons/` is embedded by the
/// `icons/**/*.svg` filter and ships in every binary thereafter.
const ICONS: [&str; 14] = [
    "bookmark",
    "chevron-down",
    "chevron-right",
    "chevron-up",
    "chevrons-up-down",
    "clock",
    "close",
    "database",
    "funnel",
    "layers",
    "play",
    "plug",
    "search",
    "sparkles",
];

fn embedded_icons() -> BTreeSet<String> {
    Embedded::iter()
        .filter(|n| n.starts_with("icons/"))
        .map(|n| {
            n.trim_start_matches("icons/")
                .trim_end_matches(".svg")
                .to_string()
        })
        .collect()
}

/// The embed carries exactly the design's icon set — no gaps, no strays.
#[test]
fn the_embedded_icon_set_is_exactly_the_one_the_design_names() {
    let want: BTreeSet<String> = ICONS.iter().map(|s| s.to_string()).collect();
    let got = embedded_icons();

    let missing: Vec<&String> = want.difference(&got).collect();
    assert!(
        missing.is_empty(),
        "icons the design names but the embed lacks: {missing:?}"
    );

    let extra: Vec<&String> = got.difference(&want).collect();
    assert!(
        extra.is_empty(),
        "icons in the embed that nothing named: {extra:?} — every file under \
         assets/icons/ ships in the binary, so an experiment left there is dead \
         weight in every install"
    );
}

/// A truncated, mis-vendored or LFS-pointer file resolves perfectly well and
/// draws nothing.
#[test]
fn every_icon_is_svg_markup() {
    for name in Embedded::iter().filter(|n| n.starts_with("icons/")) {
        let bytes = Embedded::get(&name).expect("iter yielded it").data;
        assert!(!bytes.is_empty(), "{name} resolved to an empty payload");

        let text =
            std::str::from_utf8(&bytes).unwrap_or_else(|e| panic!("{name} is not utf-8: {e}"));
        assert!(
            text.trim_start().starts_with("<svg"),
            "{name} does not begin with <svg: {:?}",
            &text[..text.len().min(64)]
        );
        assert!(text.trim_end().ends_with("</svg>"), "{name} is truncated");
    }
}

/// Every icon takes its colour from the text it sits with.
///
/// An icon with a baked-in stroke is the theme bug that survives a theme
/// switch: it renders, it is the right shape, and it stays the old palette's
/// grey on the new palette's ground. `currentColor` is what makes one file
/// serve light, dark and high-contrast, and it is a one-character edit away
/// from being lost when an icon is re-exported from a design tool.
#[test]
fn every_icon_inherits_its_colour() {
    for name in Embedded::iter().filter(|n| n.starts_with("icons/")) {
        let bytes = Embedded::get(&name).expect("iter yielded it").data;
        let text = std::str::from_utf8(&bytes).expect("utf-8");
        assert!(
            text.contains("currentColor"),
            "{name} paints itself instead of inheriting — it will not follow a \
             theme switch"
        );
        // A hard-coded hex beside `currentColor` is the same bug wearing a
        // disguise: the parts that carry it stay put.
        assert!(
            !text.contains('#'),
            "{name} carries a colour literal; icons are tinted by the cascade"
        );
    }
}

/// Every icon is reachable at the URL the app would build for it.
///
/// The GPUI original asserted `Dat0IconName::path()` resolved through the asset
/// source. There is no icon enum now — a call site names the file — so the gate
/// is on the pairing that replaced it: `protocol::url` must produce a path the
/// handler resolves back to the embed key it was given. A hand-rolled `/dat0/`
/// path that misses by a segment is otherwise a 404 nobody sees.
#[test]
fn every_icon_is_reachable_at_the_url_the_app_builds() {
    for icon in ICONS {
        let key = format!("icons/{icon}.svg");
        assert_eq!(url(&key), format!("/dat0/{key}"));
        assert!(
            Embedded::get(&key).is_some(),
            "{key} is not resolvable, so <img src=\"{}\"> would 404",
            url(&key)
        );
    }
}

/// A path that resolves to nothing must *be* nothing, not a panic.
///
/// The production consequence of a typo is a blank button, which is what the
/// rest of this file exists to prevent; the consequence must never be a dead
/// window. The handler's own 404 path is exercised through the same lookup it
/// performs.
#[test]
fn an_unknown_icon_resolves_to_nothing_rather_than_panicking() {
    assert!(Embedded::get("icons/definitely-not-an-icon.svg").is_none());
    assert!(Embedded::get("icons/").is_none());
    assert!(Embedded::get("").is_none());
}
