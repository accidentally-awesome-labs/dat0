//! Boots the dat0 token gallery in a real window.
//!
//! ```text
//! cargo run --features gallery --example gallery
//! ```
//!
//! Deliberately thin: everything renderable lives in `dat0_ui::gallery` so a
//! test can mount it. An example body is unreachable from any test — logic here
//! would rot unseen.

use dioxus::prelude::*;

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(dat0_ui::launch::config())
        .launch(Root);
}

#[component]
fn Root() -> Element {
    // The gallery renders icons through the `dat0://` protocol, so the handler
    // has to exist here too — a missing one is a silent no-render, not an error.
    dioxus::desktop::use_asset_handler("dat0", dat0_ui::protocol::serve);
    dat0_ui::theme::Theme::provide(None);
    rsx! { dat0_ui::gallery::Gallery {} }
}
