//! SH3: the empty-state hero must not touch the disk on every frame.
//!
//! Under GPUI the hero arm of `render_grid_body` opened `recents.json` AND
//! `settings.toml` on every frame it painted, and `empty_state::recents_column`
//! opened `recents.json` a second time in the same frame — three file opens per
//! paint, on the one screen a user sits on before they have opened anything.
//! The fix was a snapshot taken on `WorkspaceShell` plus a `refresh_hero_state`
//! that re-took it.
//!
//! Both halves survive the move to Dioxus, with different owners:
//!
//! - **recents** now come from the process-wide store the boot path installs
//!   (`dat0_core::globals::recents_snapshot`), so a render reads memory. The
//!   only way to prove that is still to make the disk disagree: mutate
//!   `recents.json` underneath a live window, force repaints, and assert the
//!   hero shows what the store holds. A render that reopened the file would
//!   show the new rows and fail here.
//! - **`first_run_done`** is a `use_signal` initializer in `Shell`, which runs
//!   once per window. The refresh half is no longer a method: a window's copy
//!   is fixed for its lifetime and the next window reads the file again. So the
//!   second test asserts exactly that, rather than a `refresh_*` call that
//!   would have no production caller.
//!
//! Written against the real `Shell`, because the cadence of the read is a
//! property of the shell and not of `EmptyState`, which is handed both values
//! as props.

mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;
use dat0_core::recents::{RecentEntry, Recents};
use dat0_ui::components::shell::Shell;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::Harness;

/// Point `config_dir()` at a scratch directory for the body of `f`, then put
/// the environment back.
///
/// Copied from `tests/onboarding.rs`. `DAT0_CONFIG_DIR` is process-global, so
/// every test that touches it is `#[serial]` — and a Shell mount that reads
/// the developer's real `settings.toml` decides which hero it renders from a
/// file nobody in the test wrote.
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

fn store(dir: &tempfile::TempDir) -> dat0_core::settings::store::SettingsStore {
    dat0_core::settings::store::SettingsStore::with_path(dir.path().join("settings.toml"))
}

fn seed_first_run_done(dir: &tempfile::TempDir) {
    dat0_core::settings::set_first_run_done(&store(dir), true).expect("seed first_run_done");
}

/// The installed recents store, reset to whatever `recents.json` holds now.
///
/// `install_recents` is a write-once `OnceLock`, so the first test in this
/// binary installs it and every later one re-seeds through the handle.
fn install_recents(cfg: &Path) -> Arc<Mutex<Recents>> {
    let store = dat0_core::globals::recents().unwrap_or_else(|| {
        dat0_core::globals::install_recents(Arc::new(Mutex::new(Recents::with_path(
            cfg.join("recents.json"),
        ))));
        dat0_core::globals::recents().expect("just installed")
    });
    *store.lock().expect("recents lock") = Recents::with_path(cfg.join("recents.json"));
    store
}

/// Write an entry into `recents.json` through a store handle the window does
/// NOT hold — this is the disk disagreeing with the snapshot.
fn push_behind_the_windows_back(cfg: &Path, path: &str) {
    let mut disk = Recents::with_path(cfg.join("recents.json"));
    disk.push(RecentEntry::Workspace {
        path: PathBuf::from(path),
    })
    .expect("push recent");
}

#[component]
fn Host() -> Element {
    Workspace::provide();
    Theme::provide(None);
    use_context_provider(ActionRegistry::new);
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

/// Force `n` real re-renders of the shell by opening and closing the palette.
///
/// The GPUI original called `A11ySnapshot::capture`, which forced a
/// `window.refresh()`. There is no refresh here, so the repaint is driven the
/// way a user would drive one: two clicks on live controls, each of which
/// writes a signal the shell reads.
fn repaint(h: &mut Harness, n: usize) {
    for _ in 0..n {
        h.click("command-launcher");
        h.click("palette-scrim");
    }
}

/// The recent paths the hero is showing.
fn rows(h: &Harness) -> Vec<String> {
    (0..)
        .map_while(|i| h.by_a11y_id(&format!("hero-recent-{i}")))
        .map(|k| h.attr(k, "aria-label").unwrap_or_default())
        .collect()
}

/// A window whose store holds one entry, over a scratch config dir.
fn window_with_one_recent(dir: &tempfile::TempDir) -> (Arc<Mutex<Recents>>, Harness) {
    seed_first_run_done(dir);
    let store = install_recents(dir.path());
    store
        .lock()
        .unwrap()
        .push(RecentEntry::Workspace {
            path: PathBuf::from("/recent/original"),
        })
        .expect("seed");
    let h = shell();
    // Precondition: without this the assertions below could pass over a hero
    // that shows nothing at all.
    assert_eq!(rows(&h), vec!["/recent/original"]);
    (store, h)
}

#[test]
#[serial]
fn repaints_do_not_re_read_recents_from_disk() {
    with_config_dir(|dir| {
        let (_store, mut h) = window_with_one_recent(dir);

        push_behind_the_windows_back(dir.path(), "/recent/added-behind-your-back");
        repaint(&mut h, 8);

        assert_eq!(
            rows(&h),
            vec!["/recent/original"],
            "eight forced repaints must NOT have re-read recents.json — the \
             hero arm and `recents_column` both used to open it every frame, \
             and this assertion is the whole point of SH3. A second entry here \
             means a disk read crept back into the render path"
        );
    });
}

#[test]
#[serial]
fn a_push_through_the_installed_store_does_reach_the_next_repaint() {
    // The other half: a snapshot that never moves is not a cache, it is a
    // staleness bug. The production write path is the store the boot installs,
    // and the hero must follow it.
    with_config_dir(|dir| {
        let (store, mut h) = window_with_one_recent(dir);

        store
            .lock()
            .unwrap()
            .push(RecentEntry::Workspace {
                path: PathBuf::from("/recent/opened-just-now"),
            })
            .expect("push");
        repaint(&mut h, 1);

        assert_eq!(
            rows(&h),
            vec!["/recent/opened-just-now", "/recent/original"],
            "the store is the shell's source of truth, most recent first"
        );
    });
}

#[test]
#[serial]
fn a_window_reads_first_run_done_once_and_the_next_window_reads_it_again() {
    // Deliberately NOT seeded: this is the first-run window, and the flag it
    // reads at mount is the one the scratch dir does not have yet.
    with_config_dir(|dir| {
        install_recents(dir.path());

        let mut first = shell();
        assert!(
            first.by_a11y_id("hero-take-tour").is_some(),
            "precondition: a fresh config dir means the first-run band is up"
        );

        seed_first_run_done(dir);

        repaint(&mut first, 8);
        assert!(
            first.by_a11y_id("hero-take-tour").is_some(),
            "eight forced repaints must NOT have re-read settings.toml — this \
             window resolved the flag once, at mount"
        );

        // …and the cache is per window, not permanent: the next one sees it.
        let next = shell();
        assert!(
            next.by_a11y_id("hero-take-tour").is_none(),
            "a window opened after the flag was persisted must render the \
             plain hero — a value cached for the process would leave the band \
             up forever"
        );
    });
}
