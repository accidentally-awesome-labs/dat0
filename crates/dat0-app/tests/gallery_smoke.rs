//! Anti-rot gate for the A4 token gallery.
//!
//! The gallery exists to be LOOKED at, which no test can do — but a section
//! silently disappearing (a token renamed at A5/A6, a `render` early-return)
//! would go unnoticed until someone booted the example. Mounting the view
//! headlessly and asserting every section's a11y seam is the cheap half of that
//! problem, and it is the whole reason the gallery lives in the lib instead of
//! inside `examples/gallery.rs` (an example body is unreachable from any test).

mod support;

use gpui::{AppContext as _, TestAppContext};
use gpui_component::Root;

use dat0_app::gallery::GalleryView;
use dat0_app::theme::Theme;
use support::A11ySnapshot;

/// Every section seam the gallery is contracted to render.
const SECTIONS: &[&str] = &[
    "gallery.theme",
    "gallery.colors",
    "gallery.scales",
    "gallery.elevation",
    "gallery.components",
];

#[gpui::test]
fn gallery_renders_all_sections(cx: &mut TestAppContext) {
    // Required before any gpui-component widget is built; `install_default`
    // then applies the dark builtin so `cx.theme()` is the real A1 palette.
    cx.update(gpui_component::init);
    cx.update(Theme::install_default);

    let (_root, vcx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        let view = cx.new(|cx| GalleryView::new(window, cx));
        Root::new(view, window, cx)
    });

    let snap = A11ySnapshot::capture(vcx);
    for seam in SECTIONS {
        assert!(snap.has_label(seam), "gallery section missing: {seam}");
    }
}
