//! A banner raised at any time is shown — not only the ones that existed when
//! the window mounted.
//!
//! PD-024: the shell drained the process-global banner queue in a `use_effect`
//! whose only signal access was a `write()`. A write does not subscribe, so the
//! effect ran once per mount and every banner raised after the first frame
//! waited in the queue until some *other* window mounted and showed it there.
//! Reproduced on Linux against a release build: opening a `.sqlite` after boot
//! showed nothing at all, and the refusal surfaced later, twice, in the next
//! window to open.
//!
//! These tests mount the real `Shell`, because the defect lived in how the
//! shell consumed the queue — a test of the queue alone passed throughout.

mod support;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::error_ux::{self, Banner};
use dat0_core::events::AppEvents;
use dat0_ui::components::shell::Shell;
use dat0_ui::router::Surface;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;

use support::Harness;

/// The contexts `Shell` needs, and nothing else.
#[component]
fn Host() -> Element {
    Workspace::provide();
    Theme::provide(None);
    use_context_provider(|| Signal::new(Option::<Surface>::None));
    use_context_provider(|| {
        let reg = ActionRegistry::new();
        dat0_core::actions::builtin::register_all(&reg).expect("builtins register");
        reg
    });
    use_context_provider(|| AppEvents::channel().0);
    rsx! { Shell {} }
}

/// Settle until `title` is on screen, or give up after a bounded number of
/// passes. The drain is a future woken by the push, so it lands on the next
/// render pass rather than inside the call that pushed.
fn shows(h: &mut Harness, title: &str) -> bool {
    for _ in 0..32 {
        h.settle();
        if h.has_label(title) {
            return true;
        }
    }
    false
}

#[test]
#[serial]
fn a_banner_raised_after_the_first_frame_is_shown() {
    let _ = error_ux::drain_pending();
    let mut h = Harness::new(Host, ());
    h.settle();
    assert!(
        !h.has_label("raised after mount"),
        "precondition: nothing is showing yet"
    );

    error_ux::push(Banner::warning("raised after mount"));

    assert!(
        shows(&mut h, "raised after mount"),
        "a banner raised after the first frame must reach the screen"
    );
    assert!(
        error_ux::drain_pending().is_empty(),
        "the window took it, so no other window will show it again"
    );
}

#[test]
#[serial]
fn a_banner_raised_before_the_window_exists_is_shown_on_mount() {
    let _ = error_ux::drain_pending();
    // The boot-time case the old drain did handle — it must keep working.
    error_ux::push(Banner::warning("raised at boot"));

    let mut h = Harness::new(Host, ());

    assert!(shows(&mut h, "raised at boot"));
    assert!(error_ux::drain_pending().is_empty());
}

#[test]
#[serial]
fn several_banners_arrive_in_order_and_dismiss_one_at_a_time() {
    let _ = error_ux::drain_pending();
    let mut h = Harness::new(Host, ());
    h.settle();

    error_ux::push(Banner::warning("first"));
    assert!(shows(&mut h, "first"));
    error_ux::push(Banner::warning("second"));
    assert!(shows(&mut h, "second"));
    assert!(
        h.has_label("first"),
        "a later banner must not replace an earlier one"
    );

    h.click("banner-0-dismiss");
    h.settle();
    assert!(!h.has_label("first"), "the dismissed banner goes");
    assert!(h.has_label("second"), "and the other one stays");
}
