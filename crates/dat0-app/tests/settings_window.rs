//! UAT "Settings-window" slice — Task 0 (T0 SPIKE, HARD GATE).
//!
//! Proves the Gap-2 AccessKit harness (`tests/support/mod.rs`) can be reused,
//! unmodified, against the real P10b `SettingsPanel`:
//!   (a) mount the panel standalone, as its own window (no `WorkspaceShell`);
//!   (b) read its 9 sidebar section labels as AccessKit content;
//!   (c) switch panes on a real simulated sidebar click (behavioral);
//!   (d) round-trip the telemetry toggle through the on-disk `SettingsStore`
//!       (behavioral + persistence);
//!   (e) drive the memory-budget `InputState` via `set_value` and prove it
//!       persists on the next render tick (the input path).
//!
//! If any of these cannot be made green, this is a STOP-and-report gate — see
//! `.superpowers/sdd/task-0-report.md` for the go/no-go verdict.
//!
//! ## Mount pattern
//! Mirrors `a11y_content.rs::open_sql_console_window`: a maximized
//! `add_window_view` wrapping the view in `gpui_component::Root`, exactly the
//! shape `settings_ui::open_settings_window` uses in production (same
//! `SettingsPanel::new(store, window, cx)` constructor), minus the OS window
//! chrome (title bar / explicit bounds) that a real `cx.open_window` call
//! would add — `add_window_view` supplies its own maximized `TestPlatform`
//! window instead.
//!
//! ## Store construction (why NOT `SettingsStore::open_in_memory()`)
//! `open_in_memory()` (used by `tests/settings_ui.rs`) hides its backing
//! tempdir inside the store — there is no way for a caller to build a SECOND
//! `SettingsStore` over the same path to reload state after a click/
//! `set_value`. So this harness owns its own `tempfile::TempDir` per test and
//! builds `SettingsStore::with_path(dir.join("settings.toml"))` twice: once
//! for the panel (inside the mount helper) and once, later, to reload and
//! assert (mirroring how `open_reset_confirm` in `panel.rs` builds a fresh
//! `SettingsStore::with_path` pointed at the same file).
//!
//! ## `A11ySnapshot::click` takes a LABEL, not an id (resolved finding)
//! The brief flagged this as an open question to confirm from
//! `tests/support/mod.rs`. It does: `click(&self, cx, label: &str)` resolves
//! `label` -> its static `.a11y` click id -> `debug_bounds` -> a real
//! `simulate_click` (`click_id_for_label` calls `root().get_by_label(label)`).
//! This matches the established convention in `a11y_spike.rs`
//! (`snap.click(vcx, &take_tour)`, where `take_tour` is the rendered i18n
//! text, not the `"hero-take-tour"` id) — this test follows that same
//! convention rather than the brief's pseudo-code, which used the a11y ids
//! ("theme", "tg-telemetry") as if they were the label argument.
//!
//! ## Hermeticity
//! No `DAT0_CONFIG_DIR` / `#[serial]` needed here (unlike `a11y_content.rs` /
//! `a11y_spike.rs`): `SettingsPanel::new` takes an explicit `SettingsStore`
//! rather than reading `crate::platform::config_dir()` itself, and none of
//! these tests touch the engine/session/tokio machinery those other suites
//! need — each test owns an independent `tempfile::TempDir`, so tests may run
//! in parallel safely.

mod support;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use gpui_component::Root;

use dat0_app::settings::store::SettingsStore;
use dat0_app::settings_ui::panel::SettingsPanel;
use support::A11ySnapshot;

/// Open a real window whose root is a `gpui_component::Root` wrapping a fresh
/// standalone [`SettingsPanel`] — mirrors `a11y_content.rs::open_sql_console_window`.
/// `settings_path` is the `settings.toml` path backing the panel's store; the
/// caller keeps the owning `TempDir` alive (see [`fresh_store_path`]) and can
/// build a second `SettingsStore::with_path(settings_path)` to reload state
/// after a click / `set_value`.
fn open_settings_window(
    cx: &mut TestAppContext,
    settings_path: PathBuf,
) -> (Entity<SettingsPanel>, &mut VisualTestContext) {
    // Required before any view that renders gpui-component widgets (the panel
    // uses `Button`/`Input`/`Dialog`) — mirrors `a11y_content.rs::init_components`.
    cx.update(gpui_component::init);

    let slot: Rc<RefCell<Option<Entity<SettingsPanel>>>> = Rc::new(RefCell::new(None));
    let slot2 = slot.clone();
    let (_root, vcx) = cx.add_window_view(move |window, cx| {
        window.activate_window();
        let store = SettingsStore::with_path(settings_path.clone());
        let panel = cx.new(|c| SettingsPanel::new(store, window, c));
        *slot2.borrow_mut() = Some(panel.clone());
        Root::new(panel, window, cx)
    });
    let panel = slot.borrow().clone().expect("panel captured");
    (panel, vcx)
}

/// A fresh backing store path for one test: an owned `TempDir` (must be kept
/// alive for the test's duration — dropping it removes the directory) plus
/// the `settings.toml` path inside it.
fn fresh_store_path() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("settings.toml");
    (dir, path)
}

/// The theme-cycle button's REAL rendered label (`panel.rs::render_theme`):
/// `format!("{}: {}", t("settings.theme"), current)`. A fresh store has no
/// `theme.id` set, so `current` falls back to `"dark"` — the same fallback
/// `render_theme` itself uses. There is no `settings.theme.cycle` i18n key
/// (checked `crates/dat0-i18n/src/strings/en.json`); the button's label is
/// this dynamic string, not a fixed key, so tests assert against the actual
/// rendered text instead of a key that would silently echo itself back on
/// lookup (`dat0_i18n::t` returns the key verbatim when it's missing).
fn theme_cycle_label_for(theme_id: &str) -> String {
    format!("{}: {}", dat0_i18n::t("settings.theme"), theme_id)
}

// ----------------------------------------------------------------------------
// (b) Sidebar section labels render as AccessKit content.
// ----------------------------------------------------------------------------

#[gpui::test]
fn settings_window_renders_all_nine_sidebar_sections(cx: &mut gpui::TestAppContext) {
    let (_dir, path) = fresh_store_path();
    let (_panel, vcx) = open_settings_window(cx, path);
    let snap = A11ySnapshot::capture(vcx);
    for key in [
        "settings.profile",
        "settings.theme",
        "settings.memory_budget",
        "settings.motherduck",
        "settings.ai",
        "settings.telemetry",
        "settings.workspace",
        "settings.updates",
        "settings.advanced",
    ] {
        let label = dat0_i18n::t(key);
        assert!(
            snap.has_label_any(&label),
            "missing sidebar section: {label}"
        );
    }

    // Teeth: a label no section renders must be absent — proves the loop
    // above is bound to real rendered content, not a tautology.
    assert!(
        !snap.has_label_any("Nonexistent Section Zzz"),
        "a sidebar label that was never rendered must not be found"
    );
}

// ----------------------------------------------------------------------------
// (c) Sidebar click switches the content pane (behavioral).
// ----------------------------------------------------------------------------

#[gpui::test]
fn sidebar_click_switches_content_pane(cx: &mut gpui::TestAppContext) {
    let (_dir, path) = fresh_store_path();
    let (_panel, vcx) = open_settings_window(cx, path);

    // Default section = profile; the theme pane's cycle button is not yet
    // mounted, so its label must be absent.
    let cycle_label = theme_cycle_label_for("dark");
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        !snap.has_label_any(&cycle_label),
        "theme control leaked into the default profile pane"
    );

    // Click the "Theme" sidebar row by its rendered label (the established
    // convention — see the module doc's "A11ySnapshot::click" note).
    let theme_row_label = dat0_i18n::t("settings.theme");
    snap.click(vcx, &theme_row_label);
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_any(&cycle_label),
        "theme pane did not render after the sidebar click"
    );

    // Teeth: the profile pane's placeholder text must be gone now that the
    // section switched away from profile — proves the click actually
    // switched panes rather than merely re-rendering the same one.
    let profile_placeholder = dat0_i18n::t("settings.profile.placeholder");
    assert!(
        !snap.has_label_any(&profile_placeholder),
        "profile pane content must not still be mounted after switching to theme"
    );
}

// ----------------------------------------------------------------------------
// (d) Telemetry toggle round-trips through the on-disk SettingsStore.
// ----------------------------------------------------------------------------

#[gpui::test]
fn telemetry_toggle_click_persists(cx: &mut gpui::TestAppContext) {
    let (_dir, path) = fresh_store_path();
    let (_panel, vcx) = open_settings_window(cx, path.clone());

    // Navigate to the Telemetry pane first — the toggle only renders inside
    // `render_telemetry`, and the default section is Profile.
    let telemetry_row_label = dat0_i18n::t("settings.telemetry");
    let snap = A11ySnapshot::capture(vcx);
    snap.click(vcx, &telemetry_row_label);
    vcx.run_until_parked();

    let reload_flag = || {
        SettingsStore::with_path(path.clone())
            .load_or_default()
            .expect("load settings")
            .telemetry
            .crash_submission_enabled
    };
    let before = reload_flag();
    assert!(
        !before,
        "fresh store must default crash_submission_enabled=false"
    );

    let toggle_label = dat0_i18n::t("settings.telemetry.toggle");
    let snap = A11ySnapshot::capture(vcx);
    snap.click(vcx, &toggle_label);
    vcx.run_until_parked();

    let after = reload_flag();
    assert_ne!(before, after, "telemetry toggle did not persist");
    assert!(after, "telemetry toggle must flip to true on click");

    // Teeth: click it again and confirm it flips back — proves the assertion
    // above is reading real toggle state, not a one-shot fluke.
    let snap = A11ySnapshot::capture(vcx);
    snap.click(vcx, &toggle_label);
    vcx.run_until_parked();
    assert!(
        !reload_flag(),
        "a second click must flip crash_submission_enabled back to false"
    );
}

// ----------------------------------------------------------------------------
// (e) The input path: `InputState::set_value` -> persist_inputs() on render.
// ----------------------------------------------------------------------------

#[gpui::test]
fn budget_input_set_value_persists(cx: &mut gpui::TestAppContext) {
    let (_dir, path) = fresh_store_path();
    let (panel, vcx) = open_settings_window(cx, path.clone());

    panel.update_in(vcx, |p, window, cx| {
        p.set_budget_input_value_for_test("512", window, cx);
    });
    // `persist_inputs()` runs unconditionally at the top of every render tick
    // (panel.rs `Render::render`, regardless of `selected_section`), so this
    // persists even though the default section (profile) is still active.
    vcx.run_until_parked();

    let mb = SettingsStore::with_path(path.clone())
        .load_or_default()
        .expect("load settings")
        .memory_budget_mb;
    assert_eq!(mb, 512, "budget input did not persist via the render tick");

    // Teeth: the persisted value must differ from the untouched default —
    // proves the assertion above is reading the real post-set_value value,
    // not the store's default that would be there regardless.
    assert_ne!(mb, 1024, "sanity: 512 must differ from the default 1024");

    // Content-assertion half of the input path: `render_memory_budget` (and
    // its `.a11y_label` on the input) only runs while that section is
    // selected, so switch to it before capturing.
    let budget_row_label = dat0_i18n::t("settings.memory_budget");
    let snap = A11ySnapshot::capture(vcx);
    snap.click(vcx, &budget_row_label);
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_any("512"),
        "budget input must render its new value as AccessKit content"
    );
    assert!(
        !snap.has_label_any("1024"),
        "the old default value must no longer render once it's been replaced"
    );
}
