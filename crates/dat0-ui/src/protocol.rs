//! The `dat0` custom asset protocol.
//!
//! Every web asset dat0 serves — the stylesheet, the icons, the eight Geist
//! faces, the CodeMirror bundle — is compiled into the binary by `rust-embed`
//! and handed to the webview from Rust. Nothing is read off disk.
//!
//! That is not a preference. A bundled `.app` or `.AppImage` has no `assets/`
//! directory beside the executable, so a relative file URL resolves to nothing
//! precisely in the configuration users install. Serving from the embed means
//! `cargo build` alone produces a self-contained binary and `xtask` bundling
//! stays a plain copy — no `dx` CLI, no `manganis`, no asset manifest.
//!
//! # URL shape
//!
//! `dioxus-desktop` dispatches to a handler by the **first path segment**
//! (`protocol.rs:76-81` upstream), so the handler registered as `"dat0"` sees
//! every request under `/dat0/…` and the remainder is the path within
//! `assets/`:
//!
//! ```text
//! /dat0/app.css                    -> assets/app.css
//! /dat0/icons/database.svg         -> assets/icons/database.svg
//! /dat0/fonts/Geist-Regular.ttf    -> assets/fonts/Geist-Regular.ttf
//! /dat0/codemirror.js              -> assets/codemirror.js
//! ```

use dioxus::desktop::wry::http::{Response, StatusCode};
use dioxus::desktop::{AssetRequest, RequestAsyncResponder};

/// The embedded asset set.
///
/// The `include` filters are load-bearing: `assets/` also holds
/// `fonts/LICENSE-geist` and `icons/LICENSE-lucide`, which are redistribution
/// artefacts for the source tree and `NOTICE.md`, not runtime assets. Embedding
/// them would put two licence files in the binary and nothing would notice.
#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
#[include = "fonts/**/*.ttf"]
#[include = "*.css"]
#[include = "*.js"]
pub struct Embedded;

/// Every embedded icon's file name, sorted.
///
/// Derived from the embed rather than a hand-kept list: the token gallery
/// renders whatever this returns, so an icon added to `assets/icons/` appears
/// there with no edit and one that is deleted stops being advertised.
pub fn icon_names() -> Vec<String> {
    let mut names: Vec<String> = Embedded::iter()
        .filter_map(|p| p.strip_prefix("icons/").map(str::to_string))
        .collect();
    names.sort();
    names
}

/// Content type for an asset path, by extension.
///
/// Exhaustive over what [`Embedded`] and [`panel_png`] can produce, so there
/// is no `application/octet-stream` fallback to hide a mistake behind: a
/// webview that receives a stylesheet as an octet-stream simply ignores it,
/// silently.
fn content_type(path: &str) -> Option<&'static str> {
    Some(match path.rsplit_once('.')?.1 {
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "svg" => "image/svg+xml",
        "ttf" => "font/ttf",
        // The onboarding tour illustrations, and nothing else — see `panel_png`.
        "png" => "image/png",
        _ => return None,
    })
}

/// The onboarding tour illustration for `onboarding/pN.png`, 1-based.
///
/// These seven PNGs are *not* in this crate's `assets/`.
/// `dat0_core::onboarding::panels::PANELS` already carries them with
/// `include_bytes!`, so embedding a copy here would put 432 KB of identical
/// art in the binary twice and leave two sets to keep in step.
pub fn panel_png(key: &str) -> Option<&'static [u8]> {
    use dat0_core::onboarding::panels::PANELS;
    let n: usize = key
        .strip_prefix("onboarding/p")?
        .strip_suffix(".png")?
        .parse()
        .ok()?;
    PANELS.get(n.checked_sub(1)?).map(|p| p.image)
}

/// Strip the leading `/dat0/` handler segment from a request path.
///
/// Returns `None` for a path with no segment after the handler name, and
/// rejects any `..` component: the embed is a fixed set, but a traversal
/// attempt is a bug worth failing loudly rather than resolving to nothing.
fn embed_key(uri_path: &str) -> Option<&str> {
    let rest = uri_path.trim_start_matches('/').split_once('/')?.1;
    if rest.is_empty() || rest.split('/').any(|c| c == "..") {
        return None;
    }
    Some(rest)
}

/// Serve one asset request. Registered once per window with
/// `use_asset_handler("dat0", protocol::serve)`.
pub fn serve(req: AssetRequest, responder: RequestAsyncResponder) {
    let path = req.uri().path().to_string();

    let Some(key) = embed_key(&path) else {
        return responder.respond(not_found());
    };
    let Some(mime) = content_type(key) else {
        return responder.respond(not_found());
    };
    // Checked before the embed: the tour art lives in `dat0-core`, not in
    // `assets/`, so an `Embedded::get` miss here is expected, not a warning.
    let bytes = match panel_png(key) {
        Some(png) => png.to_vec(),
        None => match Embedded::get(key) {
            Some(file) => file.data.into_owned(),
            None => {
                tracing::warn!(asset = %key, "asset not embedded");
                return responder.respond(not_found());
            }
        },
    };

    responder.respond(
        Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", mime)
            // The embed is immutable for the life of the binary, so the
            // webview may cache it as hard as it likes. This matters for the
            // fonts: without it every new window re-fetches ~1 MB of TTF.
            .header("Cache-Control", "public, max-age=31536000, immutable")
            .body(bytes)
            .expect("a static response builds"),
    );
}

fn not_found() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Vec::new())
        .expect("a static response builds")
}

/// The URL the webview should request for an embedded asset.
///
/// One definition, so a component cannot hand-roll a path that the handler's
/// prefix logic does not match.
pub fn url(key: &str) -> String {
    format!("/dat0/{key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stylesheet_and_every_font_and_icon_are_embedded() {
        let names: Vec<String> = Embedded::iter().map(|s| s.to_string()).collect();
        assert!(names.iter().any(|n| n == "app.css"), "{names:?}");
        assert!(names.iter().any(|n| n == "codemirror.js"), "{names:?}");

        // The eight faces `app.css` declares @font-face rules for.
        for face in [
            "Geist-Regular",
            "Geist-Medium",
            "Geist-SemiBold",
            "Geist-Bold",
            "GeistMono-Regular",
            "GeistMono-Medium",
            "GeistMono-SemiBold",
            "GeistMono-Bold",
        ] {
            let want = format!("fonts/{face}.ttf");
            assert!(names.contains(&want), "missing {want}");
        }

        // The six icons the widget library used to supply, plus dat0's own.
        for icon in [
            "close",
            "chevron-down",
            "chevron-up",
            "chevron-right",
            "chevrons-up-down",
            "search",
            "funnel",
            "play",
            "layers",
            "bookmark",
            "clock",
            "database",
            "plug",
            "sparkles",
        ] {
            let want = format!("icons/{icon}.svg");
            assert!(names.contains(&want), "missing {want}");
        }
    }

    #[test]
    fn licence_files_are_not_embedded() {
        // They are redistribution artefacts for the source tree, not runtime
        // assets; shipping them in the binary is dead weight nothing reads.
        for n in Embedded::iter() {
            assert!(!n.contains("LICENSE"), "{n} should not be embedded");
        }
    }

    #[test]
    fn every_embedded_file_has_a_content_type() {
        for n in Embedded::iter() {
            assert!(
                content_type(&n).is_some(),
                "{n} has no Content-Type; the webview would ignore it silently"
            );
        }
    }

    #[test]
    fn embed_key_strips_the_handler_segment() {
        assert_eq!(embed_key("/dat0/app.css"), Some("app.css"));
        assert_eq!(
            embed_key("/dat0/icons/database.svg"),
            Some("icons/database.svg")
        );
        assert_eq!(
            embed_key("/dat0/fonts/Geist-Bold.ttf"),
            Some("fonts/Geist-Bold.ttf")
        );
    }

    #[test]
    fn embed_key_rejects_traversal_and_empty_paths() {
        assert_eq!(embed_key("/dat0/"), None);
        assert_eq!(embed_key("/dat0"), None);
        assert_eq!(embed_key("/dat0/../../etc/passwd"), None);
        assert_eq!(embed_key("/dat0/icons/../../Cargo.toml"), None);
    }

    #[test]
    fn url_matches_what_the_handler_parses() {
        for key in ["app.css", "icons/close.svg", "fonts/Geist-Regular.ttf"] {
            assert_eq!(embed_key(&url(key)), Some(key));
        }
    }

    /// Drop `/* … */` blocks. CSS has no nested comments, so a scan for the
    /// next `*/` is exact rather than approximate.
    fn strip_css_comments(src: &str) -> String {
        let mut out = String::with_capacity(src.len());
        let mut rest = src;
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

    /// Both directions between `app.css` and the token set: a rule may not
    /// reference a `--d0-` name the theme does not define, and every token must
    /// be used. This is the half of the design contract that can be checked
    /// without rendering.
    #[test]
    fn app_css_and_the_token_set_agree() {
        use dat0_core::theme::tokens::{CSS_NAMES, ThemeTokens};
        use std::collections::BTreeSet;

        let raw = std::str::from_utf8(&Embedded::get("app.css").expect("embedded").data)
            .expect("utf-8")
            .to_string();
        // Comments explain the tokens *by name*, so they must not be scanned:
        // the file header alone mentions `var(--d0-…)`.
        let css = strip_css_comments(&raw);

        let defined: BTreeSet<&str> = CSS_NAMES.iter().map(|(n, _)| *n).collect();

        // Names app.css declares itself (geometry, motion, font stacks). They
        // are not colours and so are not in the token set.
        let local: BTreeSet<&str> = css
            .lines()
            .filter_map(|l| l.trim().strip_prefix("--d0-"))
            .filter_map(|l| l.split(':').next())
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
            .collect();

        let mut missing = Vec::new();
        let mut rest = css.as_str();
        while let Some(i) = rest.find("var(--d0-") {
            rest = &rest[i + 4..];
            let end = rest.find([')', ',', ' ']).unwrap_or(rest.len());
            let name = &rest[..end];
            if !defined.contains(name) && !local.contains(&name[5..]) {
                missing.push(name.to_string());
            }
        }
        missing.sort();
        missing.dedup();
        assert!(
            missing.is_empty(),
            "app.css references --d0- names that ThemeTokens does not define: {missing:?}"
        );

        // A token is consumed either by a CSS rule or by the editor palette
        // (`ThemeTokens::editor_vars`): CodeMirror builds its theme in JS,
        // outside the cascade, so the SQL syntax tokens have no rule anywhere
        // and never will. Naming that set in code beats an exemption list,
        // which would rot the moment the editor stopped reading one.
        let unused: Vec<&str> = defined
            .iter()
            .copied()
            .filter(|n| !css.contains(&format!("var({n})")))
            .filter(|n| !ThemeTokens::EDITOR_TOKENS.contains(n))
            .collect();
        assert!(
            unused.is_empty(),
            "these tokens exist but nothing reads them — neither a CSS rule nor \
             the editor palette: {unused:?}"
        );
    }
}
