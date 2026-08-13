//! The MotherDuck surface: what the panel says the connection is doing.
//!
//! Ported from `dat0-app/tests/motherduck_window.rs`, which mounted a whole
//! `WorkspaceShell` and injected state into three P9b surfaces. Two of those
//! three moved out of the UI entirely:
//!
//! * the Catalog "Cloud" group became `CatalogTree`'s CONNECTIONS group, whose
//!   classification (`md:` attach → CONNECTIONS, local file → FILES) is a pure
//!   function tested beside itself in `dat0_core::catalog::tree`;
//! * the SQL routing chip has no equivalent in the rebuilt console, and the
//!   half of it that survived — a MotherDuck query is *labelled* as one — is
//!   ported to `dat0-core/tests/routing_chip.rs`.
//!
//! What is left is the third: the connect panel's per-status button set, its
//! transient test-result line, and the supersede rule that keeps a slow probe
//! from reporting about a connection the user has already left.
//!
//! `tests/views_b.rs` already proves the status dot, the button sets for
//! Disconnected / Connecting / Connected, the error reason, the intent each
//! button emits, the attachment list, and the supersede rule *at the model*.
//! This file covers what that leaves: the status pill's own copy, the Error
//! arm's teeth, the test-result line surviving a disconnect it should survive
//! and not surviving one it should not, and the supersede rule as the user
//! meets it — on screen.

mod support;

use dioxus::prelude::*;

use dat0_core::connections::ConnectionStatus;
use dat0_core::connections::token_store::{MemoryTokenStore, TokenStore as _};

use dat0_ui::a11y::format_swatch;
use dat0_ui::components::connections::{
    Connections, ConnectionsEvent, ConnectionsPanel, Outcome, route, status_label,
};

use support::Harness;

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    initial: Connections,
}

#[component]
fn Host(props: HostProps) -> Element {
    let state = use_signal(|| props.initial.clone());
    rsx! {
        ConnectionsPanel { state, on_event: move |_e: ConnectionsEvent| {} }
    }
}

fn panel(initial: Connections) -> Harness {
    Harness::new(Host, HostProps { initial })
}

/// A store with a token in it, so `precheck` reaches `Ready` rather than
/// asking for one.
fn tokened() -> MemoryTokenStore {
    let store = MemoryTokenStore::default();
    store.set("md-token").unwrap();
    store
}

/// Drive a real Test-connection probe to completion and hand back the state
/// the panel would be rendered over.
fn after_a_successful_test() -> Connections {
    let mut c = Connections::default();
    let store = tokened();
    let Outcome::Test { probe, .. } = route(&mut c, ConnectionsEvent::TestMd, &store) else {
        panic!("a stored token must produce a probe");
    };
    assert!(c.finish_probe(
        probe,
        ConnectionStatus::Connected,
        vec!["sample_data".into()],
        "Connection OK".into(),
    ));
    c
}

fn with_status(s: ConnectionStatus) -> Connections {
    let mut c = Connections::default();
    c.set_md_status(s);
    c
}

// ─────────────────────────────────────────────────────────────────────────────
// the status pill
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_status_pill_names_each_state_in_words_of_its_own() {
    // The dot is a colour and the pill is the text beside it. A screen-reader
    // user has only the text, so two states sharing a label — or one painting
    // its raw i18n key — is the whole surface failing for them.
    let states = [
        ConnectionStatus::Disconnected,
        ConnectionStatus::Connecting,
        ConnectionStatus::Connected,
        ConnectionStatus::Error("nope".into()),
    ];
    let mut seen: Vec<String> = Vec::new();
    for s in states {
        let label = status_label(&s);
        assert!(!label.trim().is_empty(), "{s:?} has no label");
        assert!(
            !label.starts_with("connections.md.status."),
            "{s:?} painted its raw i18n key: {label:?}"
        );
        assert!(!seen.contains(&label), "{s:?} reuses the label {label:?}");
        seen.push(label.clone());

        let h = panel(with_status(s.clone()));
        let pill = h
            .by_a11y_id("connections-md-status")
            .expect("no status pill");
        assert_eq!(h.text_of(pill), label, "{s:?}");
        assert_eq!(
            h.attr(pill, "aria-label").as_deref(),
            Some(label.as_str()),
            "{s:?} is painted but not announced"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// the transient test-connection line
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn a_successful_test_shows_its_message_beside_the_databases_it_found() {
    let h = panel(after_a_successful_test());
    let msg = h
        .by_a11y_id("connections-md-test-result")
        .expect("no test-result line");
    assert_eq!(h.text_of(msg), "Connection OK");
    assert_eq!(
        h.attr(msg, "aria-label").as_deref(),
        Some("Connection OK"),
        "the result is announced, not merely painted"
    );
    assert!(h.has_label("sample_data"));
    // The Connected arm, which is what the message is a message about.
    assert!(h.by_a11y_id("connections-md-disconnect").is_some());
    assert!(h.by_a11y_id("connections-md-forget").is_some());
    assert!(h.by_a11y_id("connections-md-test").is_some());
}

#[test]
fn a_disconnected_panel_still_reports_what_the_last_test_found() {
    // The GPUI original's `test_result_renders_disconnected`: probing a stored
    // token does not connect you, so the answer has to be readable next to the
    // Connect button rather than only under a Connected pill.
    let mut c = Connections::default();
    let store = tokened();
    let Outcome::Test { probe, .. } = route(&mut c, ConnectionsEvent::TestMd, &store) else {
        panic!("expected a probe");
    };
    assert!(c.finish_probe(
        probe,
        ConnectionStatus::Disconnected,
        Vec::new(),
        "Connection OK".into(),
    ));

    let h = panel(c);
    assert_eq!(
        h.text_of(h.by_a11y_id("connections-md-test-result").unwrap()),
        "Connection OK"
    );
    assert!(h.by_a11y_id("connections-md-connect").is_some());
    assert!(h.by_a11y_id("connections-md-test").is_some());
    assert!(
        h.by_a11y_id("connections-md-disconnect").is_none(),
        "teeth: a passing probe is not a connection"
    );
    assert!(
        h.by_a11y_id("connections-db-0").is_none(),
        "teeth: no databases are attached, so none may be listed"
    );
}

#[test]
fn disconnecting_takes_the_message_and_the_databases_with_it() {
    let mut c = after_a_successful_test();
    let store = tokened();
    route(&mut c, ConnectionsEvent::DisconnectMd, &store);

    let h = panel(c);
    assert!(
        h.by_a11y_id("connections-md-test-result").is_none(),
        "the message describes a connection that no longer exists"
    );
    assert!(h.by_a11y_id("connections-md-databases").is_none());
    assert!(!h.has_label("sample_data"));
    assert!(h.by_a11y_id("connections-md-connect").is_some());
}

// ─────────────────────────────────────────────────────────────────────────────
// the error arm
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn the_error_arm_offers_a_retry_and_nothing_else() {
    // `error_arm_hides_test_shows_retry`, teeth included. Offering Test beside
    // a failure invites the user to probe a connection that is already known
    // to be broken, and Disconnect offers to leave one they are not in.
    let h = panel(with_status(ConnectionStatus::Error("Auth failed".into())));

    assert!(h.by_a11y_id("connections-md-retry").is_some());
    assert!(h.has_label("Auth failed"));
    assert_eq!(
        h.attr(h.by_a11y_id("connections-md-error").unwrap(), "role")
            .as_deref(),
        Some("alert"),
        "a failure the user did not ask about must announce itself"
    );

    assert!(
        h.by_a11y_id("connections-md-test").is_none(),
        "teeth: Test is absent in the Error arm"
    );
    assert!(
        h.by_a11y_id("connections-md-disconnect").is_none(),
        "teeth: Disconnect is absent in the Error arm"
    );
    assert!(
        h.by_a11y_id("connections-md-connect").is_none(),
        "teeth: the plain Connect button gives way to Retry"
    );
    assert!(h.by_a11y_id("connections-md-databases").is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// the supersede rule, as the user meets it
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn a_probe_that_answers_after_a_disconnect_never_reaches_the_screen() {
    // The rule `views_b` proves at the model, rendered: "Connected as md" under
    // a panel that says Disconnected is the failure this guard exists to stop,
    // and it is a failure about pixels.
    let mut c = Connections::default();
    let store = tokened();
    let Outcome::Test { probe, .. } = route(&mut c, ConnectionsEvent::TestMd, &store) else {
        panic!("expected a probe");
    };

    // The user gives up while the ATTACH is still out.
    route(&mut c, ConnectionsEvent::DisconnectMd, &store);

    // …and it finally answers, in the affirmative, about the past.
    assert!(
        !c.finish_probe(
            probe,
            ConnectionStatus::Connected,
            vec!["sample_data".into()],
            "Connection OK".into(),
        ),
        "the stale result was accepted"
    );

    let h = panel(c);
    assert_eq!(
        h.text_of(h.by_a11y_id("connections-md-status").unwrap()),
        status_label(&ConnectionStatus::Disconnected),
        "the pill was overwritten by a result about the previous state"
    );
    assert!(h.by_a11y_id("connections-md-test-result").is_none());
    assert!(h.by_a11y_id("connections-db-0").is_none());
    assert!(h.by_a11y_id("connections-md-disconnect").is_none());
}

#[test]
fn only_the_newest_of_two_overlapping_probes_may_paint() {
    let mut c = Connections::default();
    let store = tokened();
    let Outcome::Test { probe: first, .. } = route(&mut c, ConnectionsEvent::TestMd, &store) else {
        panic!("expected a probe");
    };
    let Outcome::Test { probe: second, .. } = route(&mut c, ConnectionsEvent::TestMd, &store)
    else {
        panic!("expected a probe");
    };

    assert!(!c.finish_probe(
        first,
        ConnectionStatus::Error("stale timeout".into()),
        Vec::new(),
        "✗ stale timeout".into(),
    ));
    let h = panel(c.clone());
    assert!(
        !h.has_label("stale timeout"),
        "an abandoned probe painted its failure over a live one"
    );
    assert_eq!(
        h.text_of(h.by_a11y_id("connections-md-status").unwrap()),
        status_label(&ConnectionStatus::Connecting),
        "the panel must still be showing the second probe in flight"
    );

    assert!(c.finish_probe(
        second,
        ConnectionStatus::Connected,
        vec!["sample_data".into()],
        "Connection OK".into(),
    ));
    let h = panel(c);
    assert_eq!(
        h.text_of(h.by_a11y_id("connections-md-test-result").unwrap()),
        "Connection OK"
    );
    assert!(h.has_label("sample_data"));
}

// ─────────────────────────────────────────────────────────────────────────────
// attached files
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn an_attached_file_is_identified_by_the_one_format_swatch_helper() {
    // S8: the 7×7 swatch precedes a file name everywhere one is shown, and
    // there is exactly one function deciding its colour. A panel that spelled
    // the class itself would drift the day a format is added.
    let mut c = Connections::default();
    c.add_sqlite("data", "/tmp/archive.sqlite");
    let h = panel(c);

    let row = h
        .by_a11y_id("connections-file-data")
        .expect("no attachment row");
    let html = h.html();
    let expected = format_swatch(std::path::Path::new("/tmp/archive.sqlite"));
    assert_eq!(expected, "sw-sqlite", "the helper itself changed");
    assert!(
        html.contains(&format!("d0-swatch {expected}")),
        "the attachment row carries no {expected} swatch"
    );
    assert!(h.text_of(row).contains("/tmp/archive.sqlite"));
    assert!(h.by_a11y_id("connections-detach-data").is_some());
}
