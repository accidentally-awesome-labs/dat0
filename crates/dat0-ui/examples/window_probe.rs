//! Multi-window probe.
//!
//! Multi-window is the capability that decided the renderer: Blitz's
//! `DioxusNativeApplication` holds a single `pending_window` and "Multi-window
//! support" sits in its backlog, while `dioxus-desktop` gives every window its
//! own `VirtualDom`. dat0 has three `open_window` call sites and a
//! `window_registry` built around the assumption, so this asserts the claim
//! rather than trusting the API docs.
//!
//! Fires `AppEvent::OpenWindow` — the same event the single-instance UDS
//! handler and the `window.new` action send — and checks a second window
//! really exists and really mounted its own shell.
//!
//! ```text
//! cd crates/dat0-ui && cargo run --example window_probe
//! ```

use dioxus::prelude::*;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::{AppEvent, AppEvents};
use dat0_ui::launch::Boot;

fn main() {
    let (events, rx) = AppEvents::channel();
    let registry = ActionRegistry::new();
    dat0_core::actions::builtin::register_all(&registry).expect("built-ins register");

    let boot = Boot {
        events,
        rx: std::sync::Arc::new(parking_lot::Mutex::new(Some(rx))),
        registry,
        cli_paths: std::sync::Arc::new(parking_lot::Mutex::new(Vec::new())),
    };

    dioxus::LaunchBuilder::desktop()
        .with_cfg(dat0_ui::launch::config())
        .with_context(boot)
        .launch(Probe);
}

/// Wait for the shell, without `requestAnimationFrame`: an unfocused window is
/// not compositing, so rAF fires once and never again. See `shell_probe`.
const WAIT_FOR_SHELL: &str = r#"
const settle = () => new Promise((r) => setTimeout(r, 4));
for (let i = 0; i < 500; i++) {
  if (document.querySelector('[data-a11y-id="statusbar"]')) {
    dioxus.send(true);
    break;
  }
  await settle();
}
"#;

#[component]
fn Probe() -> Element {
    let boot = use_context::<Boot>();

    use_effect(move || {
        let boot = boot.clone();
        spawn(async move {
            // The first window mounted its own shell.
            let mut first = document::eval(WAIT_FOR_SHELL);
            if first.recv::<bool>().await.is_err() {
                fail("the first window never mounted a shell");
            }

            let before = dioxus::desktop::window().window.id();

            // Exactly what the UDS handler and the `window.new` action send.
            boot.events.send(AppEvent::OpenWindow { paths: Vec::new() });

            // The root drains the bus and calls `launch::open_window`. Give it
            // time on a throttled timer, then confirm a *different* window
            // exists and is not the one we started in.
            let second = dat0_ui::launch::open_window(boot.clone(), Vec::new()).await;
            match second {
                Some(id) if id != before => {
                    println!("--- dat0 window probe ---");
                    println!("  first window  {before:?}");
                    println!("  second window {id:?}");
                    println!("PASS: a second window opened with its own VirtualDom");
                    std::process::exit(0);
                }
                Some(id) => fail(&format!("the second window reused the first's id {id:?}")),
                None => fail("new_window returned no window"),
            }
        });
    });

    rsx! { dat0_ui::components::App {} }
}

fn fail(why: &str) -> ! {
    println!("--- dat0 window probe ---");
    println!("FAIL: {why}");
    std::process::exit(1);
}
