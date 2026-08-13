//! SQL console toolbar and tab-strip behaviour.
//!
//! Ported from `dat0-app/tests/sql_console_nav.rs`, which drove the shipped
//! GPUI console through a real window: Tab-walk to a control, press Enter,
//! assert the `SqlConsoleEvent` that came out.
//!
//! Three things changed, and none of them is the guarantee:
//!
//! * **Activation.** Every control is a `<button>`, so Enter and Space are the
//!   platform's job and the harness activates by click. GPUI's `focus_stop`
//!   hand-rolled that listener and the old tests had to press Enter to prove it
//!   was wired at all.
//! * **The event.** `SqlConsoleEvent` became [`ConsoleIntent`], a callback the
//!   host wires rather than an entity subscription.
//! * **The tab strip.** It was one focus handle with its own arrow/Delete
//!   keymap; it is now a `role="tablist"` of `role="tab"` buttons under a
//!   roving tab index, which is the same "one tab stop, arrows move inside it"
//!   contract written in ARIA.
//!
//! `SqlConsoleEvent::Persist` has no counterpart: the console reports the
//! selection and the shell persists it. That half is
//! `dat0-core/tests/sql_console_integration.rs::session_round_trips_sql_tabs_via_setter`.

mod support;

use dioxus::prelude::*;
use support::{Harness, Key, Modifiers};

use dat0_core::query::ResultTarget;
use dat0_core::query::completion::new_shared_snapshot;
use dat0_ui::components::sql_console::tabs::Tabs;
use dat0_ui::components::sql_console::{ConsoleIntent, SqlConsole};
use dat0_ui::theme::Theme;

// ─────────────────────────────────────────────────────────────────────────────
// Host
// ─────────────────────────────────────────────────────────────────────────────

/// One line per intent, in the order the console raised them.
///
/// A rendered string rather than a `Vec` a test reaches into: the harness can
/// only see the DOM, and reading the log the same way a user would read the
/// screen keeps the assertion honest about what actually reached the host.
fn note(i: &ConsoleIntent) -> String {
    match i {
        ConsoleIntent::Run { sql, target, .. } => {
            let t = match target {
                ResultTarget::MainGrid => "grid",
                ResultTarget::Pane => "pane",
            };
            format!("run:{t}:{sql}")
        }
        ConsoleIntent::Cancel { .. } => "cancel".into(),
        ConsoleIntent::DocChanged { .. } => "doc".into(),
        ConsoleIntent::NewTab => "new-tab".into(),
        ConsoleIntent::CloseTab => "close-tab".into(),
        ConsoleIntent::ShowHistory => "history".into(),
        ConsoleIntent::SaveQuery { sql, .. } => format!("save:{sql}"),
        ConsoleIntent::LoadQuery => "load".into(),
        ConsoleIntent::SaveAsTable { sql, .. } => format!("as-table:{sql}"),
        ConsoleIntent::StopStream => "stop".into(),
        ConsoleIntent::InsertGenerated { sql } => format!("insert:{sql}"),
        ConsoleIntent::DiscardStream => "discard".into(),
        ConsoleIntent::CloseExplain => "close-explain".into(),
        ConsoleIntent::DismissError => "dismiss".into(),
    }
}

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    /// How many tabs to open before mounting.
    #[props(default = 1)]
    seed: usize,
    #[props(default = false)]
    running: bool,
    /// False renders the console's absence — the shell with the pane shut.
    #[props(default = true)]
    open: bool,
}

/// The shell's half of the contract: it owns the tab list and applies what the
/// console asks for. Tab lifecycle rules live in [`Tabs`], so this is the same
/// code the real router runs.
#[component]
fn Host(props: HostProps) -> Element {
    Theme::provide(None);
    let schema = use_hook(new_shared_snapshot);
    let seed = props.seed;
    let mut tabs = use_signal(|| {
        let mut t = Tabs::new();
        for i in 1..seed {
            t.open_with(format!("SELECT {i}"));
        }
        t.select(0);
        t
    });
    let mut log = use_signal(Vec::<String>::new);

    rsx! {
        if props.open {
            SqlConsole {
                tabs: tabs.read().all().to_vec(),
                active: tabs.read().active(),
                schema,
                running: props.running,
                on_intent: move |i: ConsoleIntent| {
                    log.write().push(note(&i));
                    match i {
                        ConsoleIntent::NewTab => {
                            tabs.write().open();
                        }
                        ConsoleIntent::CloseTab => {
                            tabs.write().close_active();
                        }
                        ConsoleIntent::InsertGenerated { sql } => {
                            tabs.write().open_with(sql);
                        }
                        _ => {}
                    }
                },
                on_select_tab: move |i: usize| tabs.write().select(i),
            }
        } else {
            div { "data-a11y-id": "console-shut" }
        }
        div { "data-a11y-id": "log", "{log.read().join(\"|\")}" }
        div { "data-a11y-id": "titles", "{tabs.read().titles().join(\",\")}" }
        div { "data-a11y-id": "active", "{tabs.read().active()}" }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn console(seed: usize) -> Harness {
    Harness::new(
        Host,
        HostProps {
            seed,
            running: false,
            open: true,
        },
    )
}

fn log(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("log").expect("the log is rendered"))
}

fn titles(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("titles").expect("the titles are rendered"))
}

fn active(h: &Harness) -> usize {
    h.text_of(h.by_a11y_id("active").expect("the index is rendered"))
        .trim()
        .parse()
        .expect("a number")
}

/// The labels Tab visits, in order, up to `n` hops. Mirrors the GPUI helper of
/// the same name, including its "skip an immediate repeat" rule.
fn tab_labels(h: &mut Harness, n: usize) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for _ in 0..n {
        h.press_tab();
        if let Some(l) = h.focused_label() {
            if seen.last() != Some(&l) {
                seen.push(l);
            }
        }
    }
    seen
}

/// Press a key on the tab that is currently showing — the one the roving tab
/// index makes the strip's single focus stop.
fn key_the_strip(h: &mut Harness, k: Key) {
    let tab = h
        .by_role("tab")
        .into_iter()
        .find(|n| h.attr(*n, "aria-selected").as_deref() == Some("true"))
        .expect("exactly one tab is showing");
    h.key(tab, k, Modifiers::empty());
}

fn t(key: &str) -> String {
    dat0_i18n::t(key)
}

// ─────────────────────────────────────────────────────────────────────────────
// The gate
// ─────────────────────────────────────────────────────────────────────────────

/// The whole console, driven the way the T0 gate drove it: reach Run by Tab,
/// activate it, walk the strip with an arrow, close a tab with Delete.
///
/// The individual guarantees each have their own test below; this one exists
/// because they have to hold *together* — a Run that is reachable only when no
/// second tab is open, or a strip that stops responding once the toolbar has
/// been used, passes every test below and is still broken.
#[test]
fn the_console_is_drivable_from_the_keyboard_end_to_end() {
    let mut h = console(2);
    assert_eq!(titles(&h), "Query 1,Query 2");

    let seen = tab_labels(&mut h, 20);
    assert!(
        seen.contains(&t("sql.run")),
        "Run must be a Tab stop; visited {seen:?}"
    );

    h.click("console-run");
    assert!(log(&h).contains("run:grid:"), "got {:?}", log(&h));

    key_the_strip(&mut h, Key::ArrowRight);
    assert_eq!(active(&h), 1, "→ moves the showing tab");

    key_the_strip(&mut h, Key::Delete);
    assert_eq!(titles(&h), "Query 1", "Delete closes the showing tab");
}

// ─────────────────────────────────────────────────────────────────────────────
// The toolbar
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn every_console_command_is_a_tab_stop() {
    let mut h = console(1);
    let seen = tab_labels(&mut h, 30);
    for key in [
        "sql.run",
        "sql.run_in_pane",
        "sql.new_tab",
        "sql.history",
        "sql.save_query",
        "sql.load_query",
        "sql.save_as_table",
    ] {
        assert!(
            seen.contains(&t(key)),
            "{key} ({:?}) must be Tab-reachable; visited {seen:?}",
            t(key)
        );
    }
}

#[test]
fn running_asks_for_the_main_grid() {
    let mut h = console(1);
    h.click_label(&t("sql.run"));
    assert_eq!(log(&h), "run:grid:", "an empty tab still asks for a run");
}

#[test]
fn running_in_the_pane_asks_for_the_pane_instead() {
    // The two Run controls differ only in where the rows land; a copy-paste
    // that left both on `MainGrid` would look right and quietly ignore the
    // results pane.
    let mut h = console(2);
    h.click_label(&t("sql.run_in_pane"));
    assert_eq!(log(&h), "run:pane:");
}

#[test]
fn a_run_carries_the_showing_statement() {
    let mut h = console(2);
    key_the_strip(&mut h, Key::ArrowRight);
    h.click_label(&t("sql.run"));
    assert_eq!(
        log(&h),
        "run:grid:SELECT 1",
        "the second tab's document, not the first's"
    );
}

#[test]
fn while_a_statement_runs_the_same_control_cancels_it() {
    let mut h = Harness::new(
        Host,
        HostProps {
            seed: 1,
            running: true,
            open: true,
        },
    );
    assert!(
        !h.has_label(&t("sql.run")),
        "Run and Cancel are one control, never two"
    );
    h.click_label(&t("sql.cancel"));
    assert_eq!(log(&h), "cancel");
}

#[test]
fn the_new_tab_command_opens_one() {
    let mut h = console(1);
    h.click_label(&t("sql.new_tab"));
    assert_eq!(titles(&h), "Query 1,Query 2");
    assert_eq!(active(&h), 1, "the new tab is the one you are looking at");
}

#[test]
fn the_reuse_commands_each_report_themselves_and_nothing_else() {
    // Save / Saved / Save-as-Table all sit in one row and all take the showing
    // statement; a mis-wired handler here is invisible until someone loses a
    // query.
    let mut h = console(2);
    key_the_strip(&mut h, Key::ArrowRight);
    h.click_label(&t("sql.save_query"));
    h.click_label(&t("sql.load_query"));
    h.click_label(&t("sql.save_as_table"));
    h.click_label(&t("sql.history"));
    assert_eq!(
        log(&h),
        "save:SELECT 1|load|as-table:SELECT 1|history",
        "each command reports once, with the showing statement"
    );
}

#[test]
fn the_run_control_advertises_its_chord() {
    // ⌘⏎ is bound inside CodeMirror's own keymap, so the button is the only
    // place the shortcut is discoverable.
    let h = console(1);
    let run = h.by_a11y_id("console-run").expect("Run is rendered");
    assert!(h.text_of(run).contains('⏎'), "got {:?}", h.text_of(run));
}

#[test]
fn no_console_command_exists_while_the_pane_is_shut() {
    // Non-vacuity for the reachability test above: the toolbar belongs to the
    // console, not to the window chrome, so closing the pane must take every
    // one of these labels with it.
    let h = Harness::new(
        Host,
        HostProps {
            seed: 1,
            running: false,
            open: false,
        },
    );
    for key in [
        "sql.run",
        "sql.run_in_pane",
        "sql.new_tab",
        "sql.history",
        "sql.save_query",
        "sql.load_query",
        "sql.save_as_table",
    ] {
        assert!(
            !h.has_label(&t(key)),
            "{key} must not exist with the console shut"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The tab strip
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn arrows_move_the_showing_tab_and_clamp_at_both_ends() {
    let mut h = console(3);
    key_the_strip(&mut h, Key::ArrowRight);
    key_the_strip(&mut h, Key::ArrowRight);
    assert_eq!(active(&h), 2);

    key_the_strip(&mut h, Key::ArrowRight);
    assert_eq!(active(&h), 2, "→ clamps at the end");

    key_the_strip(&mut h, Key::ArrowLeft);
    assert_eq!(active(&h), 1);
    key_the_strip(&mut h, Key::ArrowLeft);
    assert_eq!(active(&h), 0);
    key_the_strip(&mut h, Key::ArrowLeft);
    assert_eq!(active(&h), 0, "← clamps at the start");

    key_the_strip(&mut h, Key::ArrowRight);
    assert_eq!(active(&h), 1, "and it still moves afterwards");
}

#[test]
fn the_strip_is_one_tab_stop_however_many_tabs_are_open() {
    // A roving tab index, not five stops: Tab must leave the console in a
    // predictable number of presses no matter how many queries are open.
    let h = console(4);
    let stops = h
        .by_role("tab")
        .into_iter()
        .filter(|n| h.attr(*n, "tabindex").as_deref() == Some("0"))
        .count();
    assert_eq!(stops, 1, "exactly the showing tab is in the tab ring");
    assert_eq!(h.by_role("tab").len(), 4, "and all four are still there");
}

#[test]
fn delete_closes_the_showing_tab() {
    let mut h = console(2);
    assert_eq!(active(&h), 0, "the first tab is showing");
    key_the_strip(&mut h, Key::Delete);
    assert_eq!(titles(&h), "Query 2", "the one that was showing is gone");
}

#[test]
fn delete_closes_the_showing_tab_not_the_first_one() {
    // Hardens the count-only assertion above: a handler that always closed tab
    // 0 would keep the count right and lose the wrong query.
    let mut h = console(3);
    key_the_strip(&mut h, Key::ArrowRight);
    assert_eq!(active(&h), 1);
    key_the_strip(&mut h, Key::Delete);
    assert_eq!(
        titles(&h),
        "Query 1,Query 3",
        "the middle tab goes, its neighbours stay"
    );
}

#[test]
fn delete_on_the_last_tab_leaves_the_console_standing() {
    let mut h = console(1);
    key_the_strip(&mut h, Key::Delete);
    assert_eq!(titles(&h), "Query 1", "the console is never empty");
    assert_eq!(
        log(&h),
        "close-tab",
        "the console still reports the request; the rule that refuses it is \
         the host's, in one place"
    );
}

#[test]
fn backspace_closes_a_tab_the_same_way_delete_does() {
    // The GPUI strip bound only `delete`, which on an Apple keyboard is the
    // key labelled ⌫ — and on every other keyboard is not. Both reach here.
    let mut h = console(2);
    key_the_strip(&mut h, Key::Backspace);
    assert_eq!(titles(&h), "Query 2");
}

#[test]
fn clicking_a_tab_shows_it() {
    let mut h = console(3);
    h.click_label("Query 3");
    assert_eq!(active(&h), 2);
}

#[test]
fn exactly_one_tab_is_marked_as_showing() {
    let mut h = console(3);
    key_the_strip(&mut h, Key::ArrowRight);
    let selected: Vec<String> = h
        .by_role("tab")
        .into_iter()
        .filter(|n| h.attr(*n, "aria-selected").as_deref() == Some("true"))
        .filter_map(|n| h.attr(n, "aria-label"))
        .collect();
    assert_eq!(selected, vec!["Query 2".to_string()]);
}
