//! Recents-list keyboard navigation, through the whole window.
//!
//! `views_a.rs` drives `EmptyState` directly with a `Vec<RecentEntry>` prop and
//! proves the column's own grammar: one tab stop, `tabindex="-1"` rows, arrows
//! that move and clamp. What a component-level mount cannot prove is the three
//! risks the GPUI slice was written to retire, all of which live *between*
//! surfaces:
//!
//! - **R2** the entries a user actually has reach the list at all — under GPUI
//!   from `recents.json`, here from the process-wide store the boot path
//!   installs (`dat0_core::globals`), which is what the shell reads;
//! - **R4** the list is a Tab stop *of the window*, not merely of the hero —
//!   the shell has a titlebar, a tab strip, a sidebar and a status bar in front
//!   of it, and a keyboard user has to walk through them to get there;
//! - **R1** an arrow pressed at that stop moves the active row, with the shell
//!   root's own `onkeydown` sitting above it in the bubble path.
//!
//! So this suite mounts the real `Shell` and seeds the real store. The clamp
//! assertions are here rather than left to `views_a` because clamping at the
//! *top* was never covered there, and because a shell-level arrow proves the
//! root cascade does not eat it on the way past.

mod support;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_core::recents::{RecentEntry, Recents};
use dat0_ui::components::shell::Shell;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::{Harness, Key, Modifiers};

/// Point `config_dir()` at a scratch directory for the body of `f`, then put
/// the environment back.
///
/// Copied from `tests/onboarding.rs`, which is where this seam was first
/// written for this crate. `DAT0_CONFIG_DIR` is process-global, so every test
/// that touches it is `#[serial]` — and a Shell mount that reads the
/// developer's real `settings.toml` is nondeterministic whatever it asserts:
/// `first_run_done` decides which hero renders and whether the tour opens over
/// it.
fn with_config_dir<R>(f: impl FnOnce(&tempfile::TempDir) -> R) -> R {
    let tmp = tempfile::TempDir::new().unwrap();
    let previous = std::env::var_os("DAT0_CONFIG_DIR");
    // SAFETY: `#[serial]` keeps every env-touching test off the same clock, and
    // nothing else in this binary reads the variable concurrently.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", tmp.path()) };
    let out = f(&tmp);
    unsafe {
        match previous {
            Some(v) => std::env::set_var("DAT0_CONFIG_DIR", v),
            None => std::env::remove_var("DAT0_CONFIG_DIR"),
        }
    }
    out
}

/// Put the first run behind us, so the hero renders its plain variant and the
/// auto-opened tour does not cover it.
fn seed_first_run_done(dir: &tempfile::TempDir) {
    let store =
        dat0_core::settings::store::SettingsStore::with_path(dir.path().join("settings.toml"));
    dat0_core::settings::set_first_run_done(&store, true).expect("seed first_run_done");
}

/// Seed the process-wide recents store with `n` **workspace** roots, most
/// recent first: `push` inserts at the front, so pushing in reverse leaves row
/// 0 as `/recent/0`.
///
/// `globals::install_recents` is a write-once `OnceLock`, so the first test in
/// this binary installs the store every later one mutates. Each test clears it
/// and re-seeds rather than assuming an order.
fn seed_recents(cfg: &std::path::Path, n: usize) {
    let store = dat0_core::globals::recents().unwrap_or_else(|| {
        let r = Arc::new(Mutex::new(Recents::with_path(cfg.join("recents.json"))));
        dat0_core::globals::install_recents(r);
        dat0_core::globals::recents().expect("just installed")
    });
    let mut guard = store.lock().expect("recents lock");
    *guard = Recents::with_path(cfg.join("recents.json"));
    for i in (0..n).rev() {
        guard
            .push(RecentEntry::Workspace {
                path: PathBuf::from(format!("/recent/{i}")),
            })
            .expect("push recent");
    }
}

#[component]
fn Host() -> Element {
    Workspace::provide();
    Theme::provide(None);
    use_context_provider(ActionRegistry::new);
    // The receiver is kept, not dropped: an action posted by a chord would
    // otherwise hit a closed channel.
    let (events, rx) = AppEvents::channel();
    use_hook(|| std::rc::Rc::new(rx));
    use_context_provider(|| events);
    rsx! { Shell {} }
}

/// Mount and let the first frame's own state writes land, the way a real
/// window's second frame would.
fn shell() -> Harness {
    let mut h = Harness::new(Host, ());
    h.settle();
    h
}

/// The rows the hero is currently showing, by their accessible name.
fn rows(h: &Harness) -> Vec<String> {
    (0..)
        .map_while(|i| h.by_a11y_id(&format!("hero-recent-{i}")))
        .map(|k| h.attr(k, "aria-label").unwrap_or_default())
        .collect()
}

/// The index of the row carrying the active ring.
fn active(h: &Harness) -> usize {
    let n = rows(h).len();
    (0..n)
        .find(|i| {
            h.attr(h.by_a11y_id(&format!("hero-recent-{i}")).unwrap(), "class")
                .is_some_and(|c| c.contains("is-active"))
        })
        .expect("exactly one recents row is active")
}

/// Tab from the top of the window until the recents list is the focused stop,
/// or fail after a bounded walk. The oracle names the stop by its accessible
/// name, exactly as the GPUI original did.
fn tab_to_recents_list(h: &mut Harness) {
    let want = dat0_i18n::t("hero.recent_label");
    for _ in 0..24 {
        h.press_tab();
        if h.focused_label().as_deref() == Some(want.as_str()) {
            return;
        }
    }
    panic!(
        "the recents list was never a Tab stop within 24 hops; stops were {:?}",
        h.tab_order()
            .into_iter()
            .map(|k| h.attr(k, "data-a11y-id"))
            .collect::<Vec<_>>()
    );
}

/// A window over a scratch config dir with `n` recents installed.
fn seeded(dir: &tempfile::TempDir, n: usize) -> Harness {
    seed_first_run_done(dir);
    seed_recents(dir.path(), n);
    shell()
}

#[test]
#[serial]
fn the_installed_recents_store_is_what_the_hero_lists() {
    // R2. Nothing here hands the shell a prop: the entries come from the store
    // the boot path installs, which is the only route a real user's history
    // takes to the screen.
    with_config_dir(|dir| {
        let h = seeded(dir, 2);
        assert!(
            h.by_a11y_id("hero-recents").is_some(),
            "with recents installed the hero shows the list, not the samples"
        );
        assert_eq!(rows(&h), vec!["/recent/0", "/recent/1"]);
        assert!(
            h.has_label(&dat0_i18n::t("hero.recent_label")),
            "the list's own label is what a Tab stop is named by"
        );
    });
}

#[test]
#[serial]
fn an_empty_store_shows_the_samples_instead() {
    // The negative control for the test above: with nothing installed the hero
    // must not render an empty list with a heading over it.
    with_config_dir(|dir| {
        let h = seeded(dir, 0);
        assert!(h.by_a11y_id("hero-recents").is_none());
        assert!(h.by_a11y_id("hero-samples").is_some());
    });
}

#[test]
#[serial]
fn tab_reaches_the_recents_list_from_the_top_of_the_window() {
    // R4. The hero sits behind a titlebar, a tab strip and a sidebar; the list
    // being `tabindex="0"` in isolation says nothing about whether a keyboard
    // user can get to it in the assembled window.
    with_config_dir(|dir| {
        let mut h = seeded(dir, 2);
        tab_to_recents_list(&mut h);
        assert_eq!(h.focused_id().as_deref(), Some("recents-list"));
    });
}

#[test]
#[serial]
fn an_arrow_at_that_stop_moves_the_active_row() {
    // R1. The shell root carries its own `onkeydown`; an arrow that reached it
    // first — or that the root's cascade swallowed — would leave the ring where
    // it was with every component-level test still green.
    with_config_dir(|dir| {
        let mut h = seeded(dir, 2);
        tab_to_recents_list(&mut h);
        assert_eq!(active(&h), 0, "the ring starts on the most recent entry");

        let list = h.by_a11y_id("recents-list").expect("the list");
        h.key(list, Key::ArrowDown, Modifiers::empty());
        assert_eq!(active(&h), 1);
    });
}

#[test]
#[serial]
fn the_ring_clamps_at_both_ends_of_the_recents_list() {
    with_config_dir(|dir| {
        let mut h = seeded(dir, 2);
        let list = h.by_a11y_id("recents-list").expect("the list");

        h.key(list, Key::ArrowUp, Modifiers::empty());
        assert_eq!(active(&h), 0, "up from the most recent entry stays there");

        h.key(list, Key::ArrowDown, Modifiers::empty());
        assert_eq!(active(&h), 1);
        h.key(list, Key::ArrowDown, Modifiers::empty());
        assert_eq!(
            active(&h),
            1,
            "down past the oldest entry stays on it — a list surface clamps"
        );

        h.key(list, Key::ArrowUp, Modifiers::empty());
        assert_eq!(active(&h), 0);
    });
}
