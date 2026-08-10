//! Settings: the nine-section surface that still has a window to itself.
//!
//! Ported from `dat0-app/tests/settings_window.rs` (the standalone-window
//! mount) and `dat0-app/tests/settings_persist_gate.rs` (the `changed` gate).
//! The GPUI suite mounted `SettingsPanel` in its own `add_window_view` because
//! that is how production opened it; the Dioxus build still opens a real second
//! OS window ([`open_settings_window`]), so the subject is unchanged — only the
//! mount is. `SettingsPanel` is the same component that window's `VirtualDom`
//! roots, so mounting it headless tests the shipped surface, not a stand-in.
//!
//! `tests/settings_import.rs` already proves the persistence spine: each of the
//! three toggles round-tripping through `settings.toml`, the name field, the
//! half-typed budget, one theme step, one log-level step, and the reset
//! confirmation. This file deliberately does **not** restate any of that. What
//! it adds is everything else the GPUI suite proved and that one does not:
//!
//! * the sidebar itself — nine entries, in order, each resolving, with the
//!   active one marked, and switching one *replacing* the body rather than
//!   accumulating panes;
//! * the two cross-window buttons, which the GPUI harness could only prove did
//!   not panic (they reached into `window_registry` for a shell that a
//!   standalone settings window does not have). They post on the event bus now,
//!   so what they do is observable;
//! * the version line in Advanced;
//! * every section's blurb resolving as real copy rather than an echoed key
//!   (D-029);
//! * both cycles walking their whole list and wrapping, not merely advancing
//!   once;
//! * the `changed` gate, in both halves: the predicate, and the write it
//!   suppresses.

mod support;

use std::sync::Arc;

use dat0_core::actions::builtin::ids;
use dat0_core::events::{AppEventRx, AppEvents};
use dat0_core::settings::Settings;
use dat0_core::settings::store::SettingsStore;
use dat0_core::theme::BUILTIN_IDS;

use dat0_ui::components::settings_ui::{
    Bus, CONNECTIONS_OPEN, LOG_LEVELS, SECTIONS, SettingsPanel, SettingsProps, Store, changed,
};

use support::Harness;

/// The blurb key each section paints above its controls. Every section has
/// one; the GPUI suite only ever checked Profile's.
const BLURBS: [(&str, &str); 9] = [
    ("profile", "settings.profile.placeholder"),
    ("theme", "settings.theme.placeholder"),
    ("memory_budget", "settings.memory_budget.placeholder"),
    ("motherduck", "settings.motherduck.placeholder"),
    ("ai", "settings.ai.placeholder"),
    ("telemetry", "settings.telemetry.placeholder"),
    ("workspace", "settings.workspace.placeholder"),
    ("updates", "settings.updates.placeholder"),
    ("advanced", "settings.advanced.placeholder"),
];

/// A form event carrying `value`.
fn form(value: &str) -> dioxus::html::SerializedFormData {
    dioxus::html::SerializedFormData::new(value.to_string(), Vec::new())
}

/// A panel over a throwaway store, with no bus. The default mount: nothing
/// here needs the two cross-window controls.
fn settings() -> (Harness, Arc<SettingsStore>) {
    let (h, store, _bus) = settings_with_bus(Bus(None));
    (h, store)
}

fn settings_with_bus(bus: Bus) -> (Harness, Arc<SettingsStore>, Bus) {
    let store = Arc::new(SettingsStore::open_in_memory());
    let h = Harness::new(
        SettingsPanel,
        SettingsProps {
            store: Store(Arc::clone(&store)),
            events: bus.clone(),
        },
    );
    (h, store, bus)
}

/// A panel wired to a live bus, plus the receiving half. The receiver must be
/// held: dropping it turns every `send` into a logged no-op.
fn settings_on_a_bus() -> (Harness, Arc<SettingsStore>, AppEventRx) {
    let (tx, rx) = AppEvents::channel();
    let (h, store, _) = settings_with_bus(Bus(Some(tx)));
    (h, store, rx)
}

/// Everything the panel has posted so far, as `Debug` strings.
fn posted(rx: &mut AppEventRx) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        out.push(format!("{ev:?}"));
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// the sidebar
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_sidebar_lists_all_nine_sections_in_order_and_names_them() {
    // The GPUI original read the nine labels out of the AccessKit tree. Same
    // claim: nine entries, the order users learned, each one named by copy
    // rather than by an i18n key that failed to resolve.
    let h = settings();
    let (h, _store) = h;

    let walk = h.dom().walk();
    let mut at = Vec::new();
    for s in SECTIONS {
        let key = h
            .by_a11y_id(s.id)
            .unwrap_or_else(|| panic!("the sidebar has no {} entry", s.id));
        assert_eq!(h.dom().get(key).tag(), Some("button"), "{}", s.id);
        let label = h.attr(key, "aria-label").unwrap_or_default();
        assert_eq!(label, dat0_i18n::t(s.name_key), "{}", s.id);
        assert_ne!(
            label, s.name_key,
            "{} echoed its i18n key into the sidebar",
            s.id
        );
        at.push(walk.iter().position(|k| *k == key).unwrap());
    }
    assert_eq!(at.len(), 9);
    assert!(
        at.windows(2).all(|w| w[0] < w[1]),
        "the sidebar must paint in SECTIONS order; got {at:?}"
    );
}

#[test]
fn the_sidebar_marks_which_section_is_showing() {
    let (mut h, _store) = settings();
    let current = |h: &Harness, id: &str| h.attr(h.by_a11y_id(id).unwrap(), "aria-current");

    assert_eq!(current(&h, "profile").as_deref(), Some("page"));
    assert_eq!(current(&h, "advanced"), None);

    h.click("advanced");
    assert_eq!(current(&h, "advanced").as_deref(), Some("page"));
    assert_eq!(
        current(&h, "profile"),
        None,
        "two sections cannot both be the current one"
    );
}

#[test]
fn choosing_a_section_replaces_the_body_rather_than_adding_to_it() {
    // The teeth `settings_import`'s reachability sweep does not have: it only
    // asks whether the new body appeared. A pane host that mounted panes and
    // never unmounted them would pass that and leave nine stacked sections.
    let (mut h, _store) = settings();
    assert!(h.by_a11y_id("settings-body-profile").is_some());
    assert!(h.by_a11y_id("settings-name-input").is_some());

    h.click("telemetry");
    assert!(h.by_a11y_id("settings-body-telemetry").is_some());
    assert!(
        h.by_a11y_id("settings-body-profile").is_none(),
        "the previous pane is still mounted"
    );
    assert!(
        h.by_a11y_id("settings-name-input").is_none(),
        "the previous pane's controls are still reachable"
    );
    assert_eq!(
        h.by_role("switch").len(),
        1,
        "exactly one section's controls may be live at a time"
    );
}

#[test]
fn every_section_blurb_is_real_copy_rather_than_an_echoed_key() {
    // D-029. `dat0_i18n::t` returns the key verbatim when it is missing, so a
    // deleted string paints "settings.workspace.placeholder" at the top of the
    // pane and nothing else notices.
    let (mut h, _store) = settings();
    for (id, blurb_key) in BLURBS {
        h.click(id);
        let text = dat0_i18n::t(blurb_key);
        assert_ne!(text, blurb_key, "{id}'s blurb key does not resolve");
        assert!(
            h.has_label(&text),
            "{id} does not announce its blurb; expected {text:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Advanced
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_advanced_pane_names_the_running_build() {
    // A version a user can read back is the first thing a bug report needs.
    let (mut h, _store) = settings();
    h.click("advanced");
    let b = dat0_core::about::build_info::BuildInfo::current();
    let node = h.by_a11y_id("adv-version").expect("no version line");
    let text = h.text_of(node);
    assert!(text.contains(b.version), "{text:?} omits the version");
    assert!(text.contains(b.git_sha), "{text:?} omits the commit");
    assert_eq!(
        h.attr(node, "aria-label").as_deref(),
        Some(text.as_str()),
        "the version must be announced, not merely painted"
    );
}

#[test]
fn the_log_level_cycle_visits_every_level_and_wraps() {
    // One step is not a cycle. A `next` that stalled on the last entry would
    // pass a single-click assertion and strand the button there forever.
    let (mut h, store) = settings();
    h.click("advanced");
    let mut seen = vec![store.load_or_default().unwrap().log_level];
    for _ in 0..LOG_LEVELS.len() {
        h.click("adv-log-level");
        seen.push(store.load_or_default().unwrap().log_level);
    }
    assert_eq!(
        seen.first(),
        seen.last(),
        "the cycle must return to where it started; saw {seen:?}"
    );
    for level in LOG_LEVELS {
        assert!(seen.iter().any(|s| s == level), "never visited {level:?}");
    }
}

#[test]
fn the_log_level_button_says_which_level_is_set() {
    let (mut h, store) = settings();
    h.click("advanced");
    let level = store.load_or_default().unwrap().log_level;
    let label = h
        .attr(h.by_a11y_id("adv-log-level").unwrap(), "aria-label")
        .unwrap_or_default();
    assert!(label.contains(&level), "{label:?} does not name {level:?}");
    assert!(label.starts_with(&dat0_i18n::t("settings.advanced.log_level")));
}

#[test]
fn the_reset_confirmation_is_a_modal_dialog_of_its_own() {
    // This window has no handle on the shell's modal slot, so the confirmation
    // is built here — which means it has to declare itself as a dialog rather
    // than inheriting that from the slot.
    let (mut h, _store) = settings();
    h.click("advanced");
    h.click("adv-reset");
    let dialog = h.by_a11y_id("adv-reset-confirm").expect("no confirmation");
    assert_eq!(h.attr(dialog, "role").as_deref(), Some("dialog"));
    assert_eq!(h.attr(dialog, "aria-modal").as_deref(), Some("true"));
    assert_eq!(
        h.attr(dialog, "aria-label").as_deref(),
        Some(dat0_i18n::t("settings.advanced.reset.title").as_str())
    );
    assert!(h.by_a11y_id("adv-reset-scrim").is_some());
}

// ─────────────────────────────────────────────────────────────────────────────
// theme
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_theme_cycle_visits_every_builtin_and_wraps() {
    let (mut h, store) = settings();
    h.click("theme");
    let mut seen = Vec::new();
    for _ in 0..BUILTIN_IDS.len() {
        h.click("settings-theme-cycle");
        seen.push(store.get_string("theme.id").expect("a theme was written"));
    }
    for id in BUILTIN_IDS {
        assert!(
            seen.iter().any(|s| s == id),
            "never cycled to {id}; {seen:?}"
        );
    }
    // Round the loop once more and the first one comes back.
    h.click("settings-theme-cycle");
    assert_eq!(
        store.get_string("theme.id").as_deref(),
        Some(seen[0].as_str()),
        "the cycle must wrap; saw {seen:?}"
    );
}

#[test]
fn the_theme_button_says_which_theme_is_set() {
    let (mut h, store) = settings();
    h.click("theme");
    h.click("settings-theme-cycle");
    let now = store.get_string("theme.id").unwrap();
    let label = h
        .attr(h.by_a11y_id("settings-theme-cycle").unwrap(), "aria-label")
        .unwrap_or_default();
    assert!(
        label.ends_with(&now),
        "the button reads {label:?} while {now:?} is persisted"
    );
}

#[test]
fn cycling_the_theme_tells_every_other_window() {
    // A theme is an application-wide choice made in a window that is not the
    // one showing the data. Half a repaint is worse than none.
    let (mut h, store, mut rx) = settings_on_a_bus();
    h.click("theme");
    assert!(
        posted(&mut rx).is_empty(),
        "nothing posted before the click"
    );

    h.click("settings-theme-cycle");
    let next = store.get_string("theme.id").expect("persisted");
    let events = posted(&mut rx);
    assert_eq!(
        events.len(),
        1,
        "expected exactly one event; got {events:?}"
    );
    assert!(
        events[0].contains("ThemeChanged") && events[0].contains(&next),
        "the bus was told {:?}, not about {next:?}",
        events[0]
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// the two cross-window controls
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_ai_pane_asks_the_workbench_window_to_open_the_ai_panel() {
    // The GPUI test could only prove this button did not panic: it called
    // `launch_dock`, which reached `window_registry::focused_workspace_weak()`
    // for a shell the standalone settings window does not have, and
    // early-returned. Posting an action id instead makes the intent
    // observable — and testable — from here.
    let (mut h, _store, mut rx) = settings_on_a_bus();
    h.click("ai");
    let button = h.by_a11y_id("ai-open").expect("no AI button");
    assert_eq!(
        h.attr(button, "aria-label").as_deref(),
        Some(dat0_i18n::t("settings.ai.configure").as_str())
    );
    assert_eq!(
        h.attr(button, "data-action").as_deref(),
        Some(ids::AI_PANEL_OPEN)
    );

    h.click("ai-open");
    let events = posted(&mut rx);
    assert_eq!(events.len(), 1, "got {events:?}");
    assert!(
        events[0].contains(ids::AI_PANEL_OPEN),
        "the bus was told {:?}",
        events[0]
    );
    assert!(
        events[0].contains("window: None"),
        "the target is the focused window, which only the shell knows: {:?}",
        events[0]
    );
}

#[test]
fn the_motherduck_pane_asks_the_workbench_window_to_open_connections() {
    let (mut h, _store, mut rx) = settings_on_a_bus();
    h.click("motherduck");
    let button = h.by_a11y_id("md-open").expect("no MotherDuck button");
    assert_eq!(
        h.attr(button, "aria-label").as_deref(),
        Some(dat0_i18n::t("settings.motherduck.manage").as_str())
    );
    assert_eq!(
        h.attr(button, "data-action").as_deref(),
        Some(CONNECTIONS_OPEN)
    );

    h.click("md-open");
    let events = posted(&mut rx);
    assert_eq!(events.len(), 1, "got {events:?}");
    assert!(events[0].contains(CONNECTIONS_OPEN), "got {:?}", events[0]);
}

#[test]
fn a_cross_window_control_still_renders_without_a_bus() {
    // Headless, and in any window opened before the bus exists. It must paint
    // and be inert rather than panic on an absent sender.
    let (mut h, _store) = settings();
    h.click("ai");
    assert!(h.by_a11y_id("ai-open").is_some());
    h.click("ai-open");
    h.click("motherduck");
    assert!(h.by_a11y_id("md-open").is_some());
    h.click("md-open");
}

// ─────────────────────────────────────────────────────────────────────────────
// the `changed` gate
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn changed_is_true_only_on_a_difference() {
    // Ported verbatim from `settings_persist_gate.rs`. The predicate is what
    // stops a settings write per keystroke on an untouched field.
    assert!(changed("", "alice"));
    assert!(changed("alice", "bob"));
    assert!(!changed("alice", "alice"));
    assert!(!changed("", ""));
}

#[test]
fn a_field_that_did_not_change_is_never_written_back() {
    // The gate, observed rather than asserted about. Another writer — the
    // settings watcher, or a second window — changes the value behind the
    // panel's back; an input event carrying the *unchanged* text must not
    // stamp the panel's stale copy over it.
    let (mut h, store) = settings();
    let field = h.by_a11y_id("settings-name-input").expect("name field");
    h.dispatch(field, "input", form("Ada"));
    assert_eq!(store.get_string("author.name").as_deref(), Some("Ada"));

    // Out of band, as another window would.
    store.set("author.name", "Grace").unwrap();

    let field = h.by_a11y_id("settings-name-input").unwrap();
    h.dispatch(field, "input", form("Ada"));
    assert_eq!(
        store.get_string("author.name").as_deref(),
        Some("Grace"),
        "an unchanged field wrote itself back over a newer value"
    );

    // …and a real edit still lands.
    let field = h.by_a11y_id("settings-name-input").unwrap();
    h.dispatch(field, "input", form("Ada Lovelace"));
    assert_eq!(
        store.get_string("author.name").as_deref(),
        Some("Ada Lovelace")
    );
}

#[test]
fn the_email_field_persists_through_the_store() {
    // `settings_import` covers the name field; the email is the other half of
    // the package author line, and it has its own key.
    let (mut h, store) = settings();
    let field = h.by_a11y_id("settings-email-input").expect("email field");
    h.dispatch(field, "input", form("ada@example.org"));
    assert_eq!(
        store.get_string("author.email").as_deref(),
        Some("ada@example.org")
    );
    assert_eq!(
        store.load_or_default().unwrap().profile.author_email,
        "ada@example.org",
        "the KV facade must reach the document proper"
    );
}

#[test]
fn a_field_is_seeded_from_the_store_rather_than_starting_blank() {
    // Settings that opened empty and then persisted the emptiness on the first
    // keystroke would silently discard whatever was there.
    let store = Arc::new(SettingsStore::open_in_memory());
    let mut s = Settings::default();
    s.profile.author_name = "Ada".into();
    s.profile.author_email = "ada@example.org".into();
    s.memory_budget_mb = 4096;
    store.save(&s).unwrap();

    let mut h = Harness::new(
        SettingsPanel,
        SettingsProps {
            store: Store(Arc::clone(&store)),
            events: Bus(None),
        },
    );
    assert_eq!(
        h.attr(h.by_a11y_id("settings-name-input").unwrap(), "value")
            .as_deref(),
        Some("Ada")
    );
    assert_eq!(
        h.attr(h.by_a11y_id("settings-email-input").unwrap(), "value")
            .as_deref(),
        Some("ada@example.org")
    );
    h.click("memory_budget");
    assert_eq!(
        h.attr(h.by_a11y_id("settings-budget-input").unwrap(), "value")
            .as_deref(),
        Some("4096")
    );
}
