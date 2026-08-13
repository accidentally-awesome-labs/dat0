//! First-run onboarding, end to end through the real shell.
//!
//! `views_c.rs` proves the carousel *component*: it steps, it clamps, and each
//! of its two exits persists `first_run_done`. What is proven here is
//! everything around it — the three ways the tour gets on screen and the one
//! way it stops coming back:
//!
//! * the shell opens it by itself on the very first run, and never again;
//! * Help → Take a Tour opens it through the action router;
//! * the first-run hero's *Take a tour* button opens it through the shell;
//! * an exit writes a settings file whose exact bytes are gated.
//!
//! # What the GPUI suite proved that no longer exists
//!
//! `onboarding_gpui.rs` spent most of its length on a mechanism rather than a
//! behaviour. `WindowExt::open_dialog` **pushed** onto a stack, so the
//! auto-show needed a `tour_auto_shown` bool *and* a dispatcher hop
//! (`open_deferred` → `MainThreadDispatcher` → drain) purely to escape the
//! window-update re-entrancy that made a direct `onboarding::open` silently
//! no-op. Every entry point — the auto-show render, the `TakeTour` action, the
//! hero button — went through that hop, and three of the nine tests existed to
//! prove the hop worked.
//!
//! There is no hop here. Opening a modal is `ws.modal.set(Some(..))`, which is
//! a signal write from anywhere, so the three entry points collapse to three
//! one-line assertions that the slot ends up holding `Modal::Onboarding`. The
//! stacking half is gone with it: one slot cannot hold two dialogs, so
//! "a re-render must not stack a second tour" is reframed as what a user would
//! actually notice — a re-render must not *reset* the tour, which the old
//! close-then-reopen build could not have promised at all.

mod support;

use std::sync::LazyLock;

use dioxus::prelude::*;
use serial_test::serial;
use tempfile::TempDir;

use dat0_core::actions::builtin::ids;
use dat0_core::actions::registry::ActionRegistry;
use dat0_core::events::AppEvents;

use dat0_ui::components::modals::{DIALOG_ID, ModalHost};
use dat0_ui::components::shell::Shell;
use dat0_ui::state::{Modal, Workspace};
use dat0_ui::theme::Theme;
use support::{Harness, Key, Modifiers};

// ── config-dir seam ──────────────────────────────────────────────────────────

/// Point `config_dir()` at a scratch directory for the body of `f`.
///
/// `DAT0_CONFIG_DIR` is process-global, so every test that touches it is
/// `#[serial]` — the same rule `onboarding_gpui.rs` lived under.
fn with_config_dir<R>(f: impl FnOnce(&TempDir) -> R) -> R {
    let tmp = TempDir::new().unwrap();
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

fn store(dir: &TempDir) -> dat0_core::settings::store::SettingsStore {
    dat0_core::settings::store::SettingsStore::with_path(dir.path().join("settings.toml"))
}

fn first_run_done(dir: &TempDir) -> bool {
    store(dir).load_or_default().unwrap().first_run_done
}

fn seed_first_run_done(dir: &TempDir) {
    dat0_core::settings::set_first_run_done(&store(dir), true).unwrap();
}

// ── the shell, mounted for real ──────────────────────────────────────────────

/// The four contexts `Shell` reads, plus a button that dirties a signal the
/// shell subscribes to so a test can force a re-render of the real thing.
#[component]
fn ShellHost() -> Element {
    let ws = Workspace::provide();
    Theme::provide(None);
    use_context_provider(ActionRegistry::new);
    let (events, rx) = AppEvents::channel();
    // The receiver has to outlive the sender or every `AppEvent` is dropped on
    // the floor and a posted action silently disappears. `use_hook` demands a
    // `Clone` state, and a channel receiver is deliberately not one.
    use_hook(move || std::rc::Rc::new(rx));
    use_context_provider(move || events);

    rsx! {
        button {
            "data-a11y-id": "rerender",
            onclick: move |_| {
                let mut ws = ws;
                let over = *ws.drag_over.peek();
                ws.drag_over.set(!over);
            },
            "rerender"
        }
        Shell {}
    }
}

fn shell() -> Harness {
    let mut h = Harness::new(ShellHost, ());
    // `Harness::new` only runs the initial build. The auto-tour hook writes the
    // slot during that build, so the dialog appears on the pass after it.
    h.settle();
    h
}

fn dialogs(h: &Harness) -> usize {
    h.by_role("dialog").len()
}

fn tour_headline(h: &Harness) -> String {
    let node = h
        .by_a11y_id("tour-headline")
        .expect("the tour carousel is on screen");
    h.text_of(node)
}

// ── the shell opens it by itself ─────────────────────────────────────────────

#[test]
#[serial]
fn the_first_run_shell_opens_the_tour_by_itself() {
    with_config_dir(|_dir| {
        let h = shell();

        assert_eq!(dialogs(&h), 1, "the first run must land on the tour");
        assert!(
            h.by_a11y_id("tour").is_some(),
            "the dialog the shell opened is the carousel, not something else"
        );
    });
}

#[test]
#[serial]
fn a_returning_user_is_never_shown_the_tour() {
    // Teeth: `the_first_run_shell_opens_the_tour_by_itself` proves the same
    // mount DOES open a dialog when the flag is unset, so this negative is
    // meaningful rather than vacuous.
    with_config_dir(|dir| {
        seed_first_run_done(dir);
        let h = shell();

        assert_eq!(dialogs(&h), 0, "the tour must not auto-open a second time");
        assert!(h.by_a11y_id("tour").is_none());
    });
}

#[test]
#[serial]
fn a_shell_re_render_neither_stacks_nor_resets_the_tour() {
    // The GPUI original asserted the *dual guard*: a forced re-render must not
    // push a second dialog onto `active_dialogs`. Stacking is unrepresentable
    // now — one `Signal<Option<Modal>>` — so the surviving half is the one a
    // user would feel. Under `open_dialog` every Back and Next was a
    // close-then-reopen, so a re-render arriving mid-tour rebuilt the panel
    // from scratch; here it must not move.
    with_config_dir(|_dir| {
        let mut h = shell();
        h.click("tour-next");
        h.click("tour-next");
        let stepped = tour_headline(&h);

        h.click("rerender");

        assert_eq!(dialogs(&h), 1, "a re-render mounted a second tour");
        assert_eq!(
            tour_headline(&h),
            stepped,
            "a re-render sent the tour back to panel one"
        );
    });
}

#[test]
#[serial]
fn finishing_the_tour_the_shell_opened_stops_it_coming_back() {
    // The whole loop the auto-show exists inside: shown once, answered once,
    // gone. `views_c` proves the carousel persists the flag; what is proved
    // here is that the shell's own gate then reads it back.
    with_config_dir(|dir| {
        let mut h = shell();
        assert!(!first_run_done(dir), "precondition: the flag starts unset");

        h.click("tour-skip");

        assert_eq!(dialogs(&h), 0, "Skip must empty the slot");
        assert!(
            first_run_done(dir),
            "Skip is an answer, and the shell reads it from settings.toml"
        );

        // A fresh window over the same config directory.
        let next = shell();
        assert_eq!(
            dialogs(&next),
            0,
            "the tour came back after being dismissed"
        );
    });
}

#[test]
#[serial]
fn escaping_the_tour_also_answers_it() {
    // GPUI let Escape through `Dialog::keyboard` straight to `close_dialog`,
    // which skipped `mark_first_run_done` — so the tour returned on the next
    // launch for a user who had plainly dismissed it. The port declines to
    // reproduce that, and the host's `cancel` arm is where the difference is.
    with_config_dir(|dir| {
        let mut h = shell();
        h.key_at(DIALOG_ID, Key::Escape, Modifiers::empty());

        assert_eq!(dialogs(&h), 0);
        assert!(
            first_run_done(dir),
            "Escape is a dismissal, and a dismissal is an answer"
        );
    });
}

// ── the two manual entry points ──────────────────────────────────────────────

/// The slot plus the router, which is all the Take-a-Tour menu item is.
#[component]
fn RouterHost() -> Element {
    let ws = Workspace::provide();
    let (events, rx) = AppEvents::channel();
    use_hook(move || std::rc::Rc::new(rx));
    // No Shell here, so nothing installs a handler — the tour id is one the
    // router claims itself, which is the point of this host.
    let surface = use_context_provider(|| Signal::new(Option::<dat0_ui::router::Surface>::None));

    rsx! {
        button {
            "data-a11y-id": "take-tour-action",
            onclick: move |_| {
                assert!(
                    dat0_ui::router::route(ws, &events, surface, ids::ONBOARDING_TAKE_TOUR),
                    "the router must claim the Take-a-Tour id"
                );
            },
            "take tour"
        }
        ModalHost {}
    }
}

#[test]
fn the_take_a_tour_action_opens_the_carousel() {
    // GPUI needed `open_deferred` here: the action handler ran inside a
    // `window.update`, so a direct `onboarding::open` re-entered the taken
    // window and did nothing at all. The whole dispatcher hop — and the test
    // that proved it — collapses to a signal write the router performs
    // wherever it is called from.
    let mut h = Harness::new(RouterHost, ());
    assert_eq!(h.by_role("dialog").len(), 0, "clean baseline");

    h.click("take-tour-action");

    assert_eq!(h.by_role("dialog").len(), 1);
    assert!(h.by_a11y_id("tour").is_some());
}

#[test]
#[serial]
fn the_hero_take_tour_button_reopens_the_carousel() {
    // The enriched hero band renders only while `first_run_done` is false,
    // which is also what fires the auto-show — so, exactly as the GPUI test
    // did, dismiss that first and attribute the second opening to the click
    // alone. The shell's `first_run_done` is read once at mount, so the band
    // is still there after the dismissal wrote the flag.
    with_config_dir(|_dir| {
        let mut h = shell();
        h.key_at(DIALOG_ID, Key::Escape, Modifiers::empty());
        assert_eq!(dialogs(&h), 0, "baseline: the auto-shown tour is dismissed");

        h.click("hero-take-tour");

        assert_eq!(dialogs(&h), 1);
        assert!(
            h.by_a11y_id("tour").is_some(),
            "the hero button must reach the same carousel"
        );
    });
}

// ── the first hero sample ────────────────────────────────────────────────────

/// The process-wide state root. `globals::install_state_root` is a write-once
/// `OnceLock`, so the whole binary shares one directory; it is held in a
/// `LazyLock` here so the `TempDir` outlives every test rather than deleting
/// itself out from under the sample the shell just extracted.
static SAMPLES: LazyLock<TempDir> = LazyLock::new(|| TempDir::new().unwrap());

fn state_root() -> &'static std::path::Path {
    dat0_core::globals::install_state_root(SAMPLES.path().to_path_buf());
    dat0_core::globals::state_root().expect("the state root is installed")
}

#[test]
#[serial]
fn the_iris_sample_card_extracts_the_bundled_csv() {
    // `views_a` proves the hero card calls `on_open_sample`; this is the other
    // half — that the shell's handler is wired to the real
    // `sample_data::ensure_bundled_extracted`, so the bundled CSV lands on
    // disk. The import that follows is a `session_boot` concern and is covered
    // engine-first by `grid_nav.rs`.
    with_config_dir(|dir| {
        // The plain hero (no first-run band) is the one that leads with the
        // sample column, and it posts no auto-show to get in the way.
        seed_first_run_done(dir);
        let root = state_root();
        let iris = root.join("samples").join("iris.csv");
        let _ = std::fs::remove_file(&iris);

        let mut h = shell();
        assert!(!iris.exists(), "precondition: nothing extracted yet");

        h.click("hero-sample-iris");

        assert!(
            iris.exists(),
            "the Iris card must extract the bundled CSV into {}",
            iris.display()
        );
    });
}

// ── the serialized answer ────────────────────────────────────────────────────

#[test]
#[serial]
fn the_first_run_flag_is_persisted_as_exactly_this_settings_file() {
    // Carried over from `onboarding_gpui::persisted_settings_toml_is_snapshot_gated`,
    // and pointed at the UI path rather than the store directly: this is what
    // `mark_first_run_done` — the function every tour exit calls — actually
    // leaves on disk. Gating the whole file rather than the one boolean means
    // a change to the settings schema, a field default or the TOML formatting
    // reddens the build instead of passing unnoticed.
    with_config_dir(|dir| {
        dat0_ui::components::onboarding::mark_first_run_done();

        let toml = std::fs::read_to_string(dir.path().join("settings.toml")).unwrap();
        insta::assert_snapshot!("persisted_settings_toml", toml);
    });
}

// ── the slot's own promise, for the tour specifically ────────────────────────

#[test]
fn the_tour_is_the_one_dialog_a_stray_click_cannot_dismiss() {
    // Not because the tour matters — it is skippable by design — but because
    // every exit has to record that it was seen. A scrim click that merely
    // unmounted would re-show it next launch, which reads as the app having
    // forgotten.
    assert!(!dat0_ui::components::modals::scrim_dismissable(
        &Modal::Onboarding
    ));
}
