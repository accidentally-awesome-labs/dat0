//! UAT "Crash / Report-a-Bug dialogs" slice (P10c crash-reporting UI).
//!
//! Tests the crash/bug-report modal (`view/crash_report.rs::open_report`): real
//! rendered content (the dialog BODY) and the Send/Dismiss behavioral contract
//! (both clear the staged crash sentinel; Send additionally *would* submit).
//! Invokes the App-level dialog DIRECTLY from a plain `&mut App` over the shared
//! `DialogHost` (`support::open_dialog_host`) — the same `cx.active_window()` +
//! `window.open_dialog` path proven by the Update/About slice.
//!
//! SAFETY SPINE: every test asserts `!telemetry::is_active()` at entry. With no
//! telemetry client bound, `submit_staged`/`submit_report` early-return
//! (`telemetry/mod.rs::capture`), so pressing **Send** transmits NOTHING and does
//! not block on the 5s `sentry::flush`. This is what lets us drive the Send path
//! that the Update/About slice (Send → real browser) had to leave to a human.
//!
//! HERMETIC: the modal takes an injected `data_dir: PathBuf`, so each test passes
//! its own `tempfile::tempdir()` — no `DAT0_CONFIG_DIR` seam, no `#[serial]`.
//!
//! SEAM SCOPE (settled by the T0 spike): the Dialog TITLE and the note-field
//! PLACEHOLDER are not surfaced to AccessKit by gpui-component (title renders in
//! Dialog chrome; the `Input` placeholder is not exposed). Only the BODY is
//! seamed (`view/crash_report.rs`, `cfg(feature = "a11y-capture")`). The body
//! carries the full semantic content AND the crash-vs-bug differential, so a
//! title seam would be redundant and a note-field seam would assert a low-value
//! presence whose submit path is unobservable — both intentionally omitted.

mod support;

use std::time::Duration;

use gpui::TestAppContext;

use dat0_app::telemetry::crash::{self, StagedCrash};
use dat0_app::telemetry::report_logic::ReportKind;
use support::{A11ySnapshot, dialog_open, open_dialog_host};

/// A representative staged crash payload (the values are opaque to the dialog —
/// it renders the i18n body, not the payload).
fn sample_staged() -> StagedCrash {
    StagedCrash {
        message: "boom at <redacted>/foo.rs:42".into(),
        backtrace: "0: <redacted>::bar".into(),
        version: "9.9.9".into(),
    }
}

// ----------------------------------------------------------------------------
// Task 0 — SPIKE HARD-GATE (spikes EVERY asserted surface, per the Slice-3
// "a T0 spike only proves the surfaces it exercises" lesson): crash mount →
// BODY captured under DialogHost, and enter → on_ok → seeded last-crash.json
// removed (behavioral reach + submit stays a no-op with telemetry inactive).
// ----------------------------------------------------------------------------

#[gpui::test]
fn spike_crash_dialog_captures_body_and_send_clears_staged(cx: &mut TestAppContext) {
    assert!(
        !dat0_app::telemetry::is_active(),
        "SAFETY: telemetry must be inactive so Send is a no-op"
    );

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    crash::write_staged(&dir, &sample_staged()).unwrap(); // seed last-crash.json

    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "clean baseline: no dialog before open");

    let dir_open = dir.clone();
    vcx.cx.update(|app| {
        dat0_app::view::crash_report::open_report(app, ReportKind::Crash(sample_staged()), dir_open)
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "crash dialog must open");

    // Content GATE: the crash body must be captured via the seam.
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains(&dat0_i18n::t("crash.dialog.body")),
        "GATE: crash dialog body must be captured by A11ySnapshot"
    );
    // Teeth: a fabricated string the dialog never rendered must be absent.
    assert!(
        !snap.has_label_contains("NOTAREALCRASHZZZ"),
        "teeth: a never-rendered string must not be found"
    );

    // Behavioral GATE: Send = enter → on_ok → submit (no-op) → clear_staged → close.
    assert!(
        crash::read_staged(&dir).is_some(),
        "staged payload present before Send"
    );
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "enter must close the crash dialog");
    assert!(
        crash::read_staged(&dir).is_none(),
        "Send must clear the staged last-crash.json"
    );
    assert!(
        !dat0_app::telemetry::is_active(),
        "still inactive → nothing was transmitted"
    );
}

// ----------------------------------------------------------------------------
// Content — the Bug variant is a distinct paint path (not just a different key).
// ----------------------------------------------------------------------------

#[gpui::test]
fn bug_dialog_renders_distinct_body(cx: &mut TestAppContext) {
    assert!(
        !dat0_app::telemetry::is_active(),
        "SAFETY: telemetry inactive"
    );

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();

    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    vcx.cx
        .update(|app| dat0_app::view::crash_report::open_report(app, ReportKind::Bug, dir));
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "bug dialog must open");

    let snap = A11ySnapshot::capture(vcx);
    // Bug body captured, and DISTINCT from the crash body.
    assert!(
        snap.has_label_contains(&dat0_i18n::t("report.dialog.body")),
        "bug dialog body must be captured"
    );
    assert!(
        !snap.has_label_contains(&dat0_i18n::t("crash.dialog.body")),
        "bug dialog must NOT show the crash body (distinct paint path)"
    );

    // Dismiss via escape (harmless on_cancel) to leave a clean window.
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "escape must dismiss the bug dialog");
}

// ----------------------------------------------------------------------------
// Behavioral — Dismiss discards the staged crash WITHOUT submitting.
// ----------------------------------------------------------------------------

#[gpui::test]
fn dismiss_crash_discards_staged_without_send(cx: &mut TestAppContext) {
    assert!(
        !dat0_app::telemetry::is_active(),
        "SAFETY: telemetry inactive"
    );

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    crash::write_staged(&dir, &sample_staged()).unwrap();

    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    let dir_open = dir.clone();
    vcx.cx.update(|app| {
        dat0_app::view::crash_report::open_report(app, ReportKind::Crash(sample_staged()), dir_open)
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "crash dialog must open");

    // Dismiss = escape → on_cancel → clear_staged (never submits).
    vcx.simulate_keystrokes("escape");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "escape must close the crash dialog");
    assert!(
        crash::read_staged(&dir).is_none(),
        "Dismiss must discard the staged last-crash.json"
    );
    assert!(
        !dat0_app::telemetry::is_active(),
        "Dismiss must not transmit (telemetry stays inactive)"
    );
}

// ----------------------------------------------------------------------------
// Behavioral — Report-a-Bug must NOT create a crash sentinel (UAT §2.5).
// ----------------------------------------------------------------------------

#[gpui::test]
fn send_bug_report_creates_no_crash_sentinel(cx: &mut TestAppContext) {
    assert!(
        !dat0_app::telemetry::is_active(),
        "SAFETY: telemetry inactive"
    );

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    assert!(
        crash::read_staged(&dir).is_none(),
        "precondition: no staged crash before a bug report"
    );

    let vcx = open_dialog_host(cx);
    vcx.run_until_parked();

    let dir_open = dir.clone();
    vcx.cx
        .update(|app| dat0_app::view::crash_report::open_report(app, ReportKind::Bug, dir_open));
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();
    assert!(dialog_open(vcx), "bug dialog must open");

    // Send = enter → on_ok → submit_report (no-op) → clear_staged → close.
    vcx.simulate_keystrokes("enter");
    vcx.run_until_parked();
    assert!(!dialog_open(vcx), "enter must close the bug dialog");
    // Report-a-Bug must never CREATE a crash sentinel (clear_staged on a dir with
    // none is a harmless no-op remove).
    assert!(
        crash::read_staged(&dir).is_none(),
        "Report-a-Bug must not create a last-crash.json"
    );
    assert!(
        !dat0_app::telemetry::is_active(),
        "Report-a-Bug must not transmit (telemetry stays inactive)"
    );
}
