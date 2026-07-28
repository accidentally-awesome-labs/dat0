//! Gates for the A5 icon asset chain.
//!
//! A5's central failure mode is SILENT: a missing or misnamed asset renders as
//! nothing at all — gpui does not panic on an unresolved `AssetSource` path (A0
//! spike). Without these tests a typo in `Dat0IconName::path()` ships a blank
//! button that only a human boot would catch.

use gpui::AssetSource;

use dat0_app::assets::{BUNDLED_USED, Dat0Assets, Dat0IconName};
use gpui_component::IconNamed as _;

/// Every dat0-owned icon resolves to a non-empty payload.
#[test]
fn dat0_icons_resolve() {
    for name in Dat0IconName::ALL {
        let path = name.path();
        let bytes = Dat0Assets
            .load(&path)
            .unwrap_or_else(|e| panic!("{path} failed to load: {e}"))
            .unwrap_or_else(|| panic!("{path} resolved to None"));
        assert!(!bytes.is_empty(), "{path} resolved to an empty payload");
    }
}

/// Every upstream icon dat0 references resolves through the fallback arm.
#[test]
fn bundled_icons_resolve_through_fallback() {
    for path in BUNDLED_USED {
        let bytes = Dat0Assets
            .load(path)
            .unwrap_or_else(|e| panic!("{path} failed to load: {e}"))
            .unwrap_or_else(|| panic!("{path} resolved to None"));
        assert!(!bytes.is_empty(), "{path} resolved to an empty payload");
    }
}

/// Own-first ordering means a dat0 filename that also exists upstream would
/// silently shadow it. We vendor only names absent upstream TODAY; this test
/// turns a future gpui-component rev that adds one into a build failure
/// instead of a silent divergence from everyone else's icon.
#[test]
fn dat0_icons_do_not_shadow_bundled() {
    let upstream: Vec<String> = gpui_component_assets::Assets
        .list("icons/")
        .expect("upstream list() failed")
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    for name in Dat0IconName::ALL {
        let path = name.path().to_string();
        assert!(
            !upstream.contains(&path),
            "{path} now also exists in gpui-component-assets — dat0's copy is \
             shadowing it. Delete the vendored file and point Dat0IconName at \
             the upstream IconName variant instead."
        );
    }
}

/// A truncated or mis-vendored file resolves fine but renders as nothing.
#[test]
fn payloads_are_svg() {
    for name in Dat0IconName::ALL {
        let path = name.path();
        let bytes = Dat0Assets.load(&path).unwrap().unwrap();
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]);
        assert!(
            head.trim_start().starts_with("<svg"),
            "{path} does not begin with <svg: {head:?}"
        );
    }
}

/// An unresolved path must not panic — the production consequence of a typo is
/// a blank icon, and that is what the rest of this file exists to prevent.
#[test]
fn missing_path_is_not_a_panic() {
    let _ = Dat0Assets.load("icons/definitely-not-an-icon.svg");
}

/// The empty path is the documented "no asset" sentinel and must be Ok(None).
#[test]
fn empty_path_is_none() {
    assert!(Dat0Assets.load("").unwrap().is_none());
}
