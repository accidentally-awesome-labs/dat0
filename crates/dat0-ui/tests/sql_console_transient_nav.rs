//! The SQL console's transient bars: the NL→SQL draft, the Explain, the
//! failed-run strip, and the Escape ladder that closes them.
//!
//! Ported from `dat0-app/tests/sql_console_transient_nav.rs`.
//!
//! # What the harness can prove, and what it cannot
//!
//! Half of the original suite was about **focus**: a streaming bar had to take
//! the keyboard on appear, re-home across the Stop→Insert swap, and hand it
//! back to the editor on close. That was not a nicety — gpui-component's
//! `Input` bound Tab to indent, so a Tab-walk that reached the editor never
//! left it, and a bar that did not grab focus was unreachable. The console
//! carried a `pending_focus` queue drained during render to work around it.
//!
//! That queue is now [`focus_target`]: one function of the state, naming the
//! control the console hands the keyboard to — and naming nothing when the only
//! bar up is the failed-run strip, because a failed Run must not yank the caret
//! out of the statement you are fixing. So this suite asserts **the decision**,
//! plus that the control it names is actually on screen in that state. That the
//! browser then honours it, and that closing a bar returns the caret to
//! CodeMirror (`EditorCmd::Focus`), is asserted against a real window in
//! `examples/console_probe.rs` — the harness has no focus ring and no webview,
//! and a test that pretended otherwise would be asserting its own fiction.
//!
//! The editor's Tab trap did not go away either: CodeMirror binds
//! `indentWithTab`, so Escape-lands-on-Run is still the way out, and that too
//! is the probe's to prove.
//!
//! The history overlay's own rungs are not here: history is a modal now
//! (`Modal::QueryLibrary`), so its list behaviour is `tests/views_b.rs` and its
//! Escape is the single modal slot's. What the console still owns is the entry
//! point — covered in `sql_console_nav.rs` — and the precedence rule, which is
//! ported below as "the preview strip outranks the error".

mod support;

use dioxus::prelude::*;
use support::{Harness, Key, Modifiers};

use dat0_core::query::ResultTarget;
use dat0_core::query::completion::new_shared_snapshot;
use dat0_ui::components::ai::{StreamKind, StreamPhase, StreamView};
use dat0_ui::components::sql_console::tabs::Tabs;
use dat0_ui::components::sql_console::{
    ConsoleIntent, ERROR_DISMISS, STREAM_CLOSE, STREAM_INSERT, STREAM_STOP, SqlConsole,
    focus_target,
};
use dat0_ui::theme::Theme;

// ─────────────────────────────────────────────────────────────────────────────
// Host
// ─────────────────────────────────────────────────────────────────────────────

fn note(i: &ConsoleIntent) -> String {
    match i {
        ConsoleIntent::Run { target, .. } => match target {
            ResultTarget::MainGrid => "run:grid".into(),
            ResultTarget::Pane => "run:pane".into(),
        },
        ConsoleIntent::Cancel { .. } => "cancel".into(),
        ConsoleIntent::DocChanged { .. } => "doc".into(),
        ConsoleIntent::NewTab => "new-tab".into(),
        ConsoleIntent::CloseTab => "close-tab".into(),
        ConsoleIntent::ShowHistory => "history".into(),
        ConsoleIntent::SaveQuery { .. } => "save".into(),
        ConsoleIntent::LoadQuery => "load".into(),
        ConsoleIntent::SaveAsTable { .. } => "as-table".into(),
        ConsoleIntent::StopStream => "stop".into(),
        ConsoleIntent::InsertGenerated { sql } => format!("insert:{sql}"),
        ConsoleIntent::DiscardStream => "discard".into(),
        ConsoleIntent::CloseExplain => "close-explain".into(),
        ConsoleIntent::DismissError => "dismiss".into(),
    }
}

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    #[props(default)]
    stream: StreamView,
    #[props(default)]
    error: Option<String>,
}

/// The shell's half: it owns the tab list and the transient state, and applies
/// what the console asks for. Stopping a stream is the one intent it does
/// **not** act on — the provider does, later, and the console must not assume
/// the bar closes the moment Stop is pressed.
#[component]
fn Host(props: HostProps) -> Element {
    Theme::provide(None);
    let schema = use_hook(new_shared_snapshot);
    let mut tabs = use_signal(Tabs::new);
    let mut stream = use_signal(|| props.stream.clone());
    let mut error = use_signal(|| props.error.clone());
    let mut log = use_signal(Vec::<String>::new);

    rsx! {
        SqlConsole {
            tabs: tabs.read().all().to_vec(),
            active: tabs.read().active(),
            schema,
            stream: stream(),
            error: error(),
            on_intent: move |i: ConsoleIntent| {
                log.write().push(note(&i));
                match i {
                    ConsoleIntent::InsertGenerated { sql } => {
                        tabs.write().open_with(sql);
                        stream.set(StreamView::default());
                    }
                    ConsoleIntent::DiscardStream | ConsoleIntent::CloseExplain => {
                        stream.set(StreamView::default());
                    }
                    ConsoleIntent::DismissError => error.set(None),
                    _ => {}
                }
            },
            on_select_tab: move |i: usize| tabs.write().select(i),
        }
        div { "data-a11y-id": "log", "{log.read().join(\"|\")}" }
        div { "data-a11y-id": "titles", "{tabs.read().titles().join(\",\")}" }
        div { "data-a11y-id": "docs", "{tabs.read().all().iter().map(|t| t.doc.clone()).collect::<Vec<_>>().join(\",\")}" }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn stream(kind: StreamKind, phase: StreamPhase, text: &str) -> StreamView {
    StreamView {
        kind: Some(kind),
        prompt: "top users".into(),
        text: text.into(),
        phase,
        error: None,
    }
}

fn host(s: StreamView, error: Option<String>) -> Harness {
    Harness::new(Host, HostProps { stream: s, error })
}

/// A still-arriving NL→SQL draft.
fn streaming_draft() -> StreamView {
    stream(StreamKind::NlToSql, StreamPhase::Streaming, "SELECT")
}

/// A finished NL→SQL draft.
fn finished_draft() -> StreamView {
    stream(
        StreamKind::NlToSql,
        StreamPhase::Done,
        "SELECT user, n FROM t",
    )
}

/// A console showing a still-arriving NL→SQL draft.
fn streaming() -> Harness {
    host(streaming_draft(), None)
}

/// A console showing a finished NL→SQL draft.
fn drafted() -> Harness {
    host(finished_draft(), None)
}

fn log(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("log").expect("the log is rendered"))
}

fn titles(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("titles").expect("the titles are rendered"))
}

fn docs(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("docs").expect("the docs are rendered"))
}

/// Which control the console hands the keyboard to — and a check that the
/// control it names is actually on screen.
///
/// Naming a control that is not rendered would be worse than naming none:
/// focus goes nowhere and nothing in the DOM says so.
fn keyboard_goes_to(h: &Harness, s: &StreamView, error: Option<&str>) -> Option<&'static str> {
    let want = focus_target(s, error)?;
    assert!(
        h.by_a11y_id(want).is_some(),
        "{want} is named as the focus target but is not rendered"
    );
    Some(want)
}

/// Escape, at the console root — where the ladder lives.
fn escape(h: &mut Harness) {
    h.key_at("sql-console", Key::Escape, Modifiers::empty());
}

fn t(key: &str) -> String {
    dat0_i18n::t(key)
}

// ─────────────────────────────────────────────────────────────────────────────
// The NL→SQL draft
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn a_streaming_draft_offers_only_a_stop_and_claims_the_keyboard() {
    let s = streaming_draft();
    let h = host(s.clone(), None);
    assert!(h.has_label(&t("sql.ai.stop")), "Stop is the only control");
    assert!(!h.has_label(&t("sql.nl2sql.insert")));
    assert!(!h.has_label(&t("sql.nl2sql.discard")));
    assert_eq!(
        keyboard_goes_to(&h, &s, None),
        Some(STREAM_STOP),
        "an answer you cannot stop without hunting for the button is the thing \
         `pending_focus` existed to prevent"
    );
}

#[test]
fn the_draft_shows_what_has_arrived_so_far() {
    let h = streaming();
    let text = h
        .by_a11y_id("console-stream-text")
        .expect("the strip renders its text");
    assert_eq!(h.text_of(text), "SELECT");
}

#[test]
fn finishing_swaps_stop_for_insert_and_re_homes_the_keyboard() {
    let s = finished_draft();
    let h = host(s.clone(), None);
    assert!(
        !h.has_label(&t("sql.ai.stop")),
        "a finished stream cannot be stopped"
    );
    assert!(h.has_label(&t("sql.nl2sql.insert")));
    assert!(h.has_label(&t("sql.nl2sql.discard")));
    assert_eq!(
        keyboard_goes_to(&h, &s, None),
        Some(STREAM_INSERT),
        "focus must survive the Stop -> Insert swap rather than fall to nowhere \
         — and land on Insert, not on the destructive Discard beside it"
    );
}

#[test]
fn stopping_a_draft_asks_the_host_to_stop_it() {
    let mut h = streaming();
    h.click_label(&t("sql.ai.stop"));
    assert_eq!(log(&h), "stop");
    assert!(
        h.has_label(&t("sql.ai.stop")),
        "the bar stays up until the provider actually stops — the console does \
         not get to assume it did"
    );
}

#[test]
fn insert_takes_the_generated_sql_into_its_own_tab() {
    // Its own tab, deliberately: overwriting the statement you were editing
    // with a machine's guess is not an undoable mistake anyone forgives.
    let mut h = drafted();
    h.click_label(&t("sql.nl2sql.insert"));
    assert_eq!(log(&h), "insert:SELECT user, n FROM t");
    assert_eq!(titles(&h), "Query 1,Query 2");
    assert_eq!(docs(&h), ",SELECT user, n FROM t");
    assert!(
        !h.has_label(&t("sql.nl2sql.insert")),
        "Insert consumes the draft"
    );
}

#[test]
fn discard_drops_the_draft_and_opens_nothing() {
    let mut h = drafted();
    h.click_label(&t("sql.nl2sql.discard"));
    assert_eq!(log(&h), "discard");
    assert_eq!(titles(&h), "Query 1", "a discarded draft leaves no trace");
    assert!(!h.has_label(&t("sql.nl2sql.discard")));
}

#[test]
fn discard_is_the_stop_after_insert() {
    // The original walked Tab from Insert to Discard. The order still matters:
    // Discard first would put the destructive choice under the returning
    // keyboard.
    let h = drafted();
    let order: Vec<String> = h
        .tab_order()
        .into_iter()
        .filter_map(|n| h.attr(n, "aria-label"))
        .collect();
    let insert = order.iter().position(|l| *l == t("sql.nl2sql.insert"));
    let discard = order.iter().position(|l| *l == t("sql.nl2sql.discard"));
    assert!(
        matches!((insert, discard), (Some(i), Some(d)) if d == i + 1),
        "Insert then Discard, adjacent; got {order:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Explain
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn a_streaming_explain_offers_a_stop_like_any_other_stream() {
    let s = stream(StreamKind::Explain, StreamPhase::Streaming, "This query");
    let h = host(s.clone(), None);
    assert!(h.has_label(&t("sql.ai.stop")));
    assert_eq!(keyboard_goes_to(&h, &s, None), Some(STREAM_STOP));
}

#[test]
fn a_finished_explain_offers_only_a_close() {
    // An Explain is prose, not SQL: there is nothing to insert, and offering
    // Insert would put a paragraph of English into a query tab.
    let s = stream(StreamKind::Explain, StreamPhase::Done, "This query scans t");
    let h = host(s.clone(), None);
    assert!(h.has_label(&t("sql.explain.close")));
    assert!(!h.has_label(&t("sql.nl2sql.insert")));
    assert!(!h.has_label(&t("sql.nl2sql.discard")));
    assert_eq!(keyboard_goes_to(&h, &s, None), Some(STREAM_CLOSE));
}

#[test]
fn closing_a_finished_explain_reports_it_and_clears_the_bar() {
    let mut h = host(
        stream(StreamKind::Explain, StreamPhase::Done, "This query scans t"),
        None,
    );
    h.click_label(&t("sql.explain.close"));
    assert_eq!(log(&h), "close-explain");
    assert!(!h.has_label(&t("sql.explain.close")));
}

#[test]
fn a_failed_stream_says_why_and_can_still_be_dismissed() {
    // Failed is finished, not absent. The original had no test for this and
    // the strip's own state machine makes it reachable: a provider error must
    // leave something on screen to read and something to press.
    let mut h = host(
        StreamView {
            kind: Some(StreamKind::Explain),
            prompt: "SELECT 1".into(),
            text: String::new(),
            phase: StreamPhase::Failed,
            error: Some("401 unauthorized".into()),
        },
        None,
    );
    let err = h
        .by_a11y_id("console-stream-error")
        .expect("the reason is shown");
    assert_eq!(h.text_of(err), "401 unauthorized");
    h.click_label(&t("sql.explain.close"));
    assert!(!h.has_label(&t("sql.explain.close")));
}

// ─────────────────────────────────────────────────────────────────────────────
// The failed-run strip
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_error_strip_does_not_claim_the_keyboard() {
    // The one bar that must not: a Run that fails while you are mid-statement
    // must leave the caret where it is.
    let h = host(StreamView::default(), Some("boom".into()));
    assert!(h.has_label(&t("sql.error.dismiss")), "the strip is up");
    assert_eq!(
        keyboard_goes_to(&h, &StreamView::default(), Some("boom")),
        None,
        "a failed run must not yank the caret out of the editor"
    );
    assert!(
        h.by_a11y_id(ERROR_DISMISS).is_some(),
        "and it is still there to be reached by Tab"
    );
}

#[test]
fn the_error_strip_says_what_failed() {
    let h = host(StreamView::default(), Some("Parser Error: near FRM".into()));
    let text = h
        .by_a11y_id("console-error-text")
        .expect("the strip renders its message");
    assert_eq!(h.text_of(text), "Parser Error: near FRM");
}

#[test]
fn the_error_dismiss_is_a_labelled_stop_that_dismisses() {
    let mut h = host(StreamView::default(), Some("boom".into()));
    let x = h.by_a11y_id("console-error-dismiss").expect("the ✕ exists");
    assert_eq!(h.attr(x, "aria-label"), Some(t("sql.error.dismiss")));
    assert_eq!(
        h.attr(x, "tabindex"),
        Some("0".to_string()),
        "reachable by Tab even though it does not grab focus"
    );
    h.click("console-error-dismiss");
    assert_eq!(log(&h), "dismiss");
    assert!(!h.has_label(&t("sql.error.dismiss")), "and it is gone");
}

// ─────────────────────────────────────────────────────────────────────────────
// The Escape ladder
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn escape_stops_a_draft_that_is_still_arriving() {
    let mut h = streaming();
    escape(&mut h);
    assert_eq!(log(&h), "stop");
}

#[test]
fn escape_stops_an_explain_that_is_still_arriving() {
    let mut h = host(
        stream(StreamKind::Explain, StreamPhase::Streaming, "This"),
        None,
    );
    escape(&mut h);
    assert_eq!(log(&h), "stop");
}

#[test]
fn escape_discards_a_finished_draft() {
    let mut h = drafted();
    escape(&mut h);
    assert_eq!(log(&h), "discard");
    assert!(!h.has_label(&t("sql.nl2sql.insert")));
}

#[test]
fn escape_closes_a_finished_explain() {
    let mut h = host(
        stream(StreamKind::Explain, StreamPhase::Done, "This query scans t"),
        None,
    );
    escape(&mut h);
    assert_eq!(log(&h), "close-explain");
}

#[test]
fn escape_dismisses_the_error_when_no_bar_is_open() {
    let mut h = host(StreamView::default(), Some("boom".into()));
    escape(&mut h);
    assert_eq!(log(&h), "dismiss");
    assert!(!h.has_label(&t("sql.error.dismiss")));
}

#[test]
fn a_preview_bar_outranks_the_error_on_escape() {
    // The original spelled this as "history beats error". The ladder is the
    // same shape with history moved out to the modal slot: innermost surface
    // first, and the error survives to be dismissed by the next press.
    let mut h = host(
        stream(StreamKind::NlToSql, StreamPhase::Done, "SELECT 1"),
        Some("boom".into()),
    );
    escape(&mut h);
    assert_eq!(log(&h), "discard", "the draft goes first");
    assert!(
        h.has_label(&t("sql.error.dismiss")),
        "the error survives the draft-closing Escape"
    );

    escape(&mut h);
    assert_eq!(log(&h), "discard|dismiss", "the next press takes the error");
    assert!(!h.has_label(&t("sql.error.dismiss")));
}

#[test]
fn escape_with_nothing_open_is_not_the_consoles_key() {
    // Non-vacuity, and load-bearing: the shell's cascade has its own Escape
    // rungs below this one, so a console that swallowed every Escape would
    // silently break them.
    let mut h = host(StreamView::default(), None);
    escape(&mut h);
    assert_eq!(log(&h), "", "nothing was consumed");
}

#[test]
fn no_transient_control_exists_when_every_bar_is_closed() {
    let h = host(StreamView::default(), None);
    for key in [
        "sql.ai.stop",
        "sql.nl2sql.insert",
        "sql.nl2sql.discard",
        "sql.explain.close",
        "sql.error.dismiss",
    ] {
        assert!(
            !h.has_label(&t(key)),
            "{key} must not exist while its bar is closed"
        );
    }
}

#[test]
fn an_idle_stream_with_a_kind_still_shows_no_bar() {
    // The state a finished-and-consumed stream sits in. `Idle` is not a phase
    // that renders, or dismissing a draft would leave an empty bar behind.
    let h = host(
        stream(StreamKind::NlToSql, StreamPhase::Idle, "SELECT 1"),
        None,
    );
    assert!(h.by_a11y_id("console-stream").is_none());
}
