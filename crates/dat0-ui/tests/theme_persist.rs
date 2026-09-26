//! A theme chosen is kept for the next launch, and every window follows it
//! (PD-023, step 5.9).
//!
//! `App` provided its theme with no settings, so every window opened in the
//! default whatever had been chosen, and View → Toggle Theme repainted only
//! the window it was chosen in, and kept nothing. The Settings window's own
//! control already kept its choice and told every window; the toggle does
//! both now. These mount `App`'s theme and bus over a real `Boot`, and run
//! the toggle as the View menu does.

mod support;

use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::settings::store::SettingsStore;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::Surface;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

thread_local! {
    /// The process's boot, shared by every window a test opens.
    static BOOT: Boot = {
        let reg = ActionRegistry::new();
        register_all(&reg).expect("built-in actions register");
        Boot::new(reg, Vec::new())
    };
}

/// `App`'s theme and bus, the theme it paints, and the toggle as the View
/// menu runs it.
#[component]
fn Window() -> Element {
    let theme = Theme::provide_saved();
    let ws = Workspace::provide();
    let surface = use_context_provider(|| Signal::new(Option::<Surface>::None));
    let boot = use_context_provider(|| BOOT.with(Boot::clone));
    use_context_provider(|| boot.registry.clone());
    let events = use_window_bus(boot, ws, surface);
    rsx! {
        div { "data-a11y-id": "theme", "{theme.tokens().id}" }
        button {
            "data-a11y-id": "toggle",
            onclick: move |_| {
                dat0_ui::router::route(ws, &events, surface, ids::THEME_TOGGLE);
            },
        }
    }
}

/// A config dir of the test's own, where the theme chosen is kept.
fn fresh_config() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    // SAFETY: every test in this binary is `#[serial]`, so no other thread
    // races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", tmp.path()) };
    tmp
}

fn kept(dir: &tempfile::TempDir) -> Option<String> {
    SettingsStore::with_path(dir.path().join("settings.toml")).get_string("theme.id")
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

/// Settle every window, then sleep, until `done` or two minutes.
fn pump(windows: &mut [&mut Harness], done: impl Fn(&[&mut Harness]) -> bool) -> bool {
    for _ in 0..4800 {
        for h in windows.iter_mut() {
            h.settle();
        }
        if done(windows) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn theme(h: &Harness) -> String {
    h.by_a11y_id("theme")
        .map(|k| h.text_of(k))
        .unwrap_or_default()
}

#[test]
#[serial]
fn a_theme_chosen_is_kept_for_the_next_launch() {
    let rt = runtime();
    let _guard = rt.enter();
    let dir = fresh_config();

    let mut h = Harness::new(Window, ());
    h.settle();
    assert_eq!(theme(&h), "light", "the default, with nothing chosen");

    h.click("toggle");
    assert!(pump(&mut [&mut h], |w| theme(w[0]) == "dark"));
    assert_eq!(
        kept(&dir).as_deref(),
        Some("dark"),
        "kept where the next launch reads it"
    );
    drop(h);

    // The next launch.
    let mut next = Harness::new(Window, ());
    next.settle();
    assert_eq!(
        theme(&next),
        "dark",
        "the next launch opens in the theme chosen"
    );
}

#[test]
#[serial]
fn every_window_follows_the_theme_chosen_in_one() {
    let rt = runtime();
    let _guard = rt.enter();
    let _dir = fresh_config();

    let mut first = Harness::new(Window, ());
    let mut second = Harness::new(Window, ());
    assert!(pump(&mut [&mut first, &mut second], |w| w
        .iter()
        .all(|h| theme(h) == "light")));

    second.click("toggle");
    assert!(
        pump(&mut [&mut first, &mut second], |w| w
            .iter()
            .all(|h| theme(h) == "dark")),
        "{} / {}",
        theme(&first),
        theme(&second)
    );
}
