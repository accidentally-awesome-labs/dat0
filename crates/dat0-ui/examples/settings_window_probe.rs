//! Settings-window probe.
//!
//! Settings is the one surface that is a *window* rather than a panel or a
//! modal, which is a claim the headless harness cannot check: it can mount
//! `SettingsPanel`, but it has no notion of a second `VirtualDom` on a second
//! `tao` window. This runs the real thing.
//!
//! Opens the workbench window, then calls the exact entry point the shell's
//! action router calls for `ids::SETTINGS_OPEN`, and asserts a distinct window
//! came back.
//!
//! ```text
//! cd crates/dat0-ui && cargo run --example settings_window_probe
//! ```

use dioxus::prelude::*;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
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

/// Wait for the shell without `requestAnimationFrame`: an unfocused window is
/// not compositing, so rAF fires once and never again. Same reason as
/// `window_probe`.
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
            let mut first = document::eval(WAIT_FOR_SHELL);
            if first.recv::<bool>().await.is_err() {
                fail("the workbench window never mounted a shell");
            }

            let workbench = dioxus::desktop::window().window.id();
            let settings =
                dat0_ui::components::settings_ui::open_settings_window(boot.events.clone()).await;

            match settings {
                Some(id) if id != workbench => {
                    println!("--- dat0 settings-window probe ---");
                    println!("  workbench window {workbench:?}");
                    println!("  settings window  {id:?}");
                    println!("PASS: settings opened as its own window with its own VirtualDom");
                    std::process::exit(0);
                }
                Some(id) => fail(&format!("settings reused the workbench window {id:?}")),
                None => fail("open_settings_window returned no window"),
            }
        });
    });

    rsx! { dat0_ui::components::App {} }
}

fn fail(why: &str) -> ! {
    println!("--- dat0 settings-window probe ---");
    println!("FAIL: {why}");
    std::process::exit(1);
}
