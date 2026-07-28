//! Boots the dat0 token gallery in a real window (UI redesign A4).
//!
//! `cargo run -p dat0-app --example gallery`
//!
//! Deliberately thin: everything renderable lives in `dat0_app::gallery` so the
//! headless `tests/gallery_smoke.rs` can mount it. An example body is
//! unreachable from any test — logic here would rot unseen.

use gpui::{
    AppContext as _, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use gpui_component::Root;

use dat0_app::gallery::GalleryView;
use dat0_app::theme::Theme;

fn main() {
    // A5: without this the icon section renders blank — silently, since a missing
    // AssetSource is a no-render rather than a panic (A0 spike).
    Application::new()
        .with_assets(dat0_app::assets::Dat0Assets)
        .run(|cx| {
            // Required before any gpui-component widget is built.
            gpui_component::init(cx);
            // Applies the dark builtin so `cx.theme()` is the real A1 palette; the
            // in-gallery buttons switch from here via the same facade.
            Theme::install_default(cx);

            let bounds = Bounds::centered(None, size(px(1100.), px(860.)), cx);
            let _ = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("dat0 — token gallery".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| GalleryView::new(window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            );
            cx.activate(true);
        });
}
