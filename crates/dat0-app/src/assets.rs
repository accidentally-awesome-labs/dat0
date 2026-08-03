//! Icon assets for dat0.
//!
//! `gpui::Application::with_assets` takes exactly ONE `AssetSource`, so
//! [`Dat0Assets`] serves both dat0's own icons and the 86 Lucide SVGs that
//! `gpui-component-assets` bundles for `gpui_component::IconName`.
//!
//! Lookup is own-first, then delegate. That ordering is what makes
//! `dat0_icons_do_not_shadow_bundled` (tests/icon_assets.rs) load-bearing: if a
//! future gpui-component rev ships one of our filenames, our copy wins silently
//! and dat0's icon diverges from every other consumer's.
//!
//! Missing assets stay a silent no-render rather than a panic — that is gpui's
//! existing behaviour (A0 spike), and `load` deliberately delegates upstream's
//! not-found `Err` rather than flattening it to `Ok(None)`.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use gpui_component::IconNamed;
use rust_embed::RustEmbed;

/// dat0's own icon files. Eight Lucide SVGs that `gpui-component-assets` does
/// not bundle at the pinned rev.
///
/// The `include` filter is load-bearing: `assets/` also holds `chinook.sqlite`,
/// `demo.dat0` and the onboarding PNGs (~1.5 MB). Without it every one of them
/// would be embedded in the release binary.
#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
struct Dat0Embedded;

/// The single `AssetSource` registered on every `Application`.
pub struct Dat0Assets;

impl AssetSource for Dat0Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(f) = Dat0Embedded::get(path) {
            return Ok(Some(f.data));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out: Vec<SharedString> = Dat0Embedded::iter()
            .filter(|p| p.starts_with(path))
            .map(|p| SharedString::from(p.to_string()))
            .collect();
        for up in gpui_component_assets::Assets.list(path)? {
            if !out.contains(&up) {
                out.push(up);
            }
        }
        Ok(out)
    }
}

/// dat0-owned icon names, usable anywhere `gpui_component::IconName` is.
///
/// The blanket `impl<T: IconNamed> From<T> for Icon` upstream means
/// `Icon::new(Dat0IconName::Filter)` works exactly like
/// `Icon::new(IconName::Close)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dat0IconName {
    Filter,
    Play,
    Layers,
    Bookmark,
    History,
    Database,
    Plug,
    Sparkles,
}

impl Dat0IconName {
    /// Every variant — the tests and the gallery iterate this so a new icon
    /// cannot be added without being covered and displayed.
    pub const ALL: &[Dat0IconName; 8] = &[
        Dat0IconName::Filter,
        Dat0IconName::Play,
        Dat0IconName::Layers,
        Dat0IconName::Bookmark,
        Dat0IconName::History,
        Dat0IconName::Database,
        Dat0IconName::Plug,
        Dat0IconName::Sparkles,
    ];
}

impl IconNamed for Dat0IconName {
    fn path(self) -> SharedString {
        match self {
            // Upstream Lucide renamed `filter` to `funnel`; the vendored file
            // keeps the current upstream name so it stays a verbatim copy.
            Self::Filter => "icons/funnel.svg",
            Self::Play => "icons/play.svg",
            Self::Layers => "icons/layers.svg",
            Self::Bookmark => "icons/bookmark.svg",
            Self::History => "icons/clock.svg",
            // B7: the activity rail's three items.
            Self::Database => "icons/database.svg",
            Self::Plug => "icons/plug.svg",
            Self::Sparkles => "icons/sparkles.svg",
        }
        .into()
    }
}

/// Upstream icon paths dat0 actually references. Kept as an explicit list so
/// `bundled_icons_resolve_through_fallback` fails if a rev bump drops one.
pub const BUNDLED_USED: &[&str; 5] = &[
    "icons/close.svg",
    "icons/chevron-down.svg",
    "icons/chevron-up.svg",
    "icons/chevron-right.svg",
    "icons/chevrons-up-down.svg",
];
