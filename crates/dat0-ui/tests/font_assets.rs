//! Gates for the bundled-font chain.
//!
//! Port of `dat0-app/tests/font_assets.rs`. The failure mode is unchanged and
//! still the worst kind: **nothing errors.** A face that fails to reach the
//! renderer does not panic and does not render as nothing — the next family in
//! the stack takes over and the window still looks like a window. Only someone
//! who knows what Geist looks like would catch it.
//!
//! What *causes* that failure moved, so the gates moved with it. Under GPUI the
//! chain was `register_fonts` → `AssetSource::load` → `add_fonts`, and the
//! break was a mismatch between the family name inside the `.ttf`'s `name`
//! table and the string `font.family` asked for. There is no `add_fonts` now:
//! `app.css` declares each face with `@font-face`, which **names the family
//! itself** and fetches the bytes by URL. The internal name table is no longer
//! consulted, and the new break is a `src: url(…)` the asset protocol cannot
//! serve — a 404 the webview reports to nobody, followed by a silent fallback
//! to `ui-sans-serif`.
//!
//! So the load-bearing gate here is that every URL `app.css` asks for resolves
//! through [`Embedded`], and that the family names the type classes ask for are
//! the ones `@font-face` declares.

use std::collections::BTreeSet;

use dat0_ui::protocol::{Embedded, url};

/// sfnt version tags a `.ttf` may legally start with (Apple's spec, "Font
/// Tables"). `OTTO` is deliberately absent: dat0 vendors the glyf-outline TTFs,
/// and a CFF build appearing here would mean the vendor step pulled from the
/// wrong directory of `vercel/geist-font`.
const SFNT_MAGIC: [&[u8; 4]; 3] = [&[0x00, 0x01, 0x00, 0x00], b"ttcf", b"true"];

/// The stylesheet, with `/* … */` removed.
///
/// The comments discuss the rules *by name* — the file header talks about "the
/// `@font-face` block" — so a scan that does not strip them parses prose as
/// CSS. No nested comments in CSS, so finding the next `*/` is exact.
fn app_css() -> String {
    let bytes = Embedded::get("app.css").expect("app.css is embedded");
    let src = String::from_utf8(bytes.data.to_vec()).expect("app.css is utf-8");

    let mut out = String::with_capacity(src.len());
    let mut rest = src.as_str();
    while let Some(open) = rest.find("/*") {
        out.push_str(&rest[..open]);
        match rest[open + 2..].find("*/") {
            Some(close) => rest = &rest[open + 2 + close + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// One `@font-face` rule, reduced to what can go wrong.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Face {
    family: String,
    weight: u32,
    src: String,
}

/// Parse the `@font-face` block out of `app.css`.
///
/// Deliberately parsed rather than hand-listed: a hand-list is a second copy of
/// the stylesheet that rots the moment someone adds a weight, and this file's
/// whole job is to notice when the stylesheet and the embed disagree.
fn declared_faces(css: &str) -> Vec<Face> {
    let mut out = Vec::new();
    let mut rest = css;
    while let Some(at) = rest.find("@font-face") {
        rest = &rest[at..];
        let open = rest.find('{').expect("@font-face has a body");
        let close = rest.find('}').expect("@font-face body is closed");
        let body = &rest[open + 1..close];
        rest = &rest[close + 1..];

        let mut family = None;
        let mut weight = None;
        let mut src = None;
        for decl in body.split(';') {
            let (prop, value) = match decl.split_once(':') {
                Some(pair) => (pair.0.trim(), pair.1.trim()),
                None => continue,
            };
            match prop {
                "font-family" => family = Some(value.trim_matches('"').to_string()),
                "font-weight" => weight = value.parse().ok(),
                "src" => {
                    let start = value.find("url(").expect("src uses url()") + 4;
                    let end = value[start..].find(')').expect("url() is closed") + start;
                    src = Some(value[start..end].trim_matches('"').to_string());
                }
                _ => {}
            }
        }
        out.push(Face {
            family: family.expect("@font-face names a family"),
            weight: weight.expect("@font-face names a weight"),
            src: src.expect("@font-face names a src"),
        });
    }
    out
}

/// Strip the handler segment the way `protocol::serve` does, so a face whose
/// URL is well-formed-but-wrong fails here rather than at run time.
fn embed_key_of(src: &str) -> &str {
    src.strip_prefix("/dat0/")
        .unwrap_or_else(|| panic!("{src} is not served by the dat0 protocol"))
}

/// THE gate: every URL the stylesheet fetches resolves to real bytes.
///
/// A missing face is a 404 inside the webview. Nothing surfaces it — not a
/// panic, not a log, not a visibly broken layout.
#[test]
fn every_declared_face_resolves_through_the_asset_protocol() {
    let css = app_css();
    let faces = declared_faces(&css);
    assert!(!faces.is_empty(), "app.css declares no @font-face at all");

    for face in &faces {
        let key = embed_key_of(&face.src);
        assert_eq!(
            url(key),
            face.src,
            "the stylesheet hand-rolled a URL that protocol::url would not produce"
        );
        let bytes = Embedded::get(key)
            .unwrap_or_else(|| {
                panic!(
                    "{} asks for {key}, which is not in the embed — the webview \
                     would 404 and fall back to a system font, silently",
                    face.family
                )
            })
            .data;
        assert!(!bytes.is_empty(), "{key} resolved to an empty payload");
    }
}

/// A truncated file, or an LFS pointer committed in place of one, resolves
/// perfectly well and registers as nothing.
#[test]
fn every_payload_is_a_truetype_font() {
    let mut seen = 0;
    for name in Embedded::iter().filter(|n| n.starts_with("fonts/")) {
        let bytes = Embedded::get(&name).expect("iter yielded it").data;
        let head: [u8; 4] = bytes
            .get(..4)
            .and_then(|s| s.try_into().ok())
            .unwrap_or_else(|| panic!("{name} is shorter than an sfnt header"));
        assert!(
            SFNT_MAGIC.contains(&&head),
            "{name} does not start with an sfnt magic: {head:02x?}"
        );
        seen += 1;
    }
    assert_eq!(
        seen, 8,
        "both families at four weights, or the embed lost one"
    );
}

/// Both families, all four weights.
///
/// The type scale asks for 600 (`.d0-h1`, `.d0-h2`, `.d0-head-title` at 500,
/// `.d0-wordmark` at 700). A weight with no `@font-face` is synthesised by the
/// shaper or silently rounded to the nearest declared one — a difference no
/// other assertion here would see.
#[test]
fn both_families_are_declared_at_all_four_weights() {
    let css = app_css();
    let declared: BTreeSet<(String, u32)> = declared_faces(&css)
        .into_iter()
        .map(|f| (f.family, f.weight))
        .collect();

    for family in ["Geist", "Geist Mono"] {
        for weight in [400, 500, 600, 700] {
            assert!(
                declared.contains(&(family.to_string(), weight)),
                "no @font-face for {family} {weight}; declared: {declared:?}"
            );
        }
    }
    assert_eq!(declared.len(), 8, "an unexpected face joined: {declared:?}");
}

/// The family *strings* are the contract, exactly as they were under GPUI —
/// only the place they are written down changed.
///
/// Every type class reads `var(--d0-sans)` or `var(--d0-monospace)`, and those
/// two stacks must lead with a family `@font-face` actually declares. A typo in
/// either is not an error; it is a silent fall-through to the next entry in the
/// stack, which is why both stacks deliberately end in a system fallback and
/// why that fallback must never be what renders.
#[test]
fn the_type_stacks_lead_with_a_declared_family() {
    let css = app_css();
    let declared: BTreeSet<String> = declared_faces(&css).into_iter().map(|f| f.family).collect();

    for (var, want) in [("--d0-sans", "Geist"), ("--d0-monospace", "Geist Mono")] {
        let line = css
            .lines()
            .find(|l| l.trim_start().starts_with(&format!("{var}:")))
            .unwrap_or_else(|| panic!("{var} is not declared"));
        let value = line.split_once(':').unwrap().1.trim().trim_end_matches(';');
        let lead = value
            .split(',')
            .next()
            .unwrap()
            .trim()
            .trim_matches('"')
            .to_string();
        assert_eq!(lead, want, "{var} no longer leads with the bundled family");
        assert!(
            declared.contains(&lead),
            "{var} leads with {lead:?}, which no @font-face declares"
        );
        assert!(
            value.contains(','),
            "{var} has no system fallback: a failed fetch would render nothing"
        );
    }

    // And nothing asks for a bare family name behind the two stacks' backs.
    for (i, line) in css.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("font-family:") {
            let value = value.trim().trim_end_matches(';');
            assert!(
                value.starts_with("var(--d0-sans")
                    || value.starts_with("var(--d0-monospace")
                    || declared.contains(&value.trim_matches('"').to_string()),
                "app.css:{} asks for {value} directly; use var(--d0-sans) or \
                 var(--d0-monospace) so one edit moves the whole app",
                i + 1
            );
        }
    }
}

/// The include filter is a whitelist and must stay one.
///
/// The GPUI embed had to keep `chinook.sqlite`, `demo.dat0`, `iris.csv` and
/// seven onboarding PNGs out of the binary by name. This crate's `assets/`
/// holds none of those, so the assertion is the stronger categorical one: an
/// embedded file is a stylesheet, a script, an icon or a font, and nothing
/// else. That also keeps the two vendored licence texts out — they are
/// redistribution artefacts for the source tree, not runtime assets.
#[test]
fn the_embed_admits_only_the_four_asset_kinds() {
    for name in Embedded::iter() {
        let ok = name.ends_with(".css")
            || name.ends_with(".js")
            || (name.starts_with("icons/") && name.ends_with(".svg"))
            || (name.starts_with("fonts/") && name.ends_with(".ttf"));
        assert!(
            ok,
            "{name} is embedded but is not a stylesheet, script, icon or font"
        );
        assert!(
            !name.contains("LICENSE"),
            "{name} is a licence, not a runtime asset"
        );
    }
}

/// SIL OFL 1.1 §2 requires the notice to travel with the fonts, and
/// `NOTICE.md` points at this path. It is not embedded (nothing reads it at run
/// time) but it must exist, which is precisely the sort of file that gets
/// deleted in a tidy-up.
#[test]
fn the_open_font_licence_travels_with_the_fonts() {
    let licence = include_str!("../assets/fonts/LICENSE-geist");
    assert!(
        licence.contains("SIL OPEN FONT LICENSE Version 1.1"),
        "assets/fonts/LICENSE-geist is not the OFL 1.1 text"
    );
    assert!(
        licence.contains("The Geist Project Authors"),
        "assets/fonts/LICENSE-geist lost its copyright line"
    );
}

/// The base size and the corner radius the builtin theme documents used to
/// pin.
///
/// They were `font.size: 14` and `radius: 5` inside every `ThemeConfig`,
/// re-asserted per theme because a widget library read them from there. CSS
/// owns both now and there is exactly one of each, but they are still the two
/// numbers the whole type scale and every pane corner are derived from, so they
/// keep their gate.
#[test]
fn the_stylesheet_pins_the_base_size_and_radius_the_theme_documents_used_to() {
    let css = app_css();
    assert!(
        css.contains("font-size: 14px"),
        "the 14px root size is gone; every rem-derived size moves with it"
    );
    assert!(
        css.contains("--d0-r: 5px"),
        "the 5px pane/button radius is gone"
    );
    assert!(
        css.contains("--d0-r-sm: 3px"),
        "the 3px keycap/swatch radius is gone"
    );
}
