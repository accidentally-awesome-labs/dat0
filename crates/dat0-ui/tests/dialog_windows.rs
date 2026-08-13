//! About, the updater and the crash reporter — as the *slot* mounts them.
//!
//! Ported from `update_about_window.rs` and `crash_report_window.rs`, which
//! between them drove twelve GPUI tests over a `DialogHost` window and the
//! `cx.active_window()` + `window.open_dialog` path.
//!
//! `views_c.rs` already covers what these three panels *say*: the About body's
//! version/licence/acknowledgements rows and its nudge line, every manual
//! update outcome and the background silence, the crash-vs-bug body split, and
//! the note field. None of that is repeated. What is left is the half those
//! GPUI suites carried that a bare panel mount cannot reach:
//!
//! * the **safety spine** — these are privacy and installer surfaces, and the
//!   tests drive their affirmative buttons, so "nothing was transmitted" and
//!   "nothing was installed" have to be asserted, not assumed;
//! * the **dismiss** exits, which belong to the host: Escape on a crash report
//!   has to clear the staged payload, and Escape on an available update has to
//!   answer the opener rather than silently drop the flow;
//! * the modal **header** text, which is `title(&Modal)` and not the body
//!   `views_c` reads.
//!
//! # What the GPUI suites proved that no longer exists
//!
//! Both files were shaped by `gpui_component::Dialog`'s implicit keyboard
//! contract: `enter` fired `on_ok`, `escape` fired `on_cancel`, and the
//! comment blocks are mostly a map of which dialogs were unsafe to press
//! `enter` on — About-with-a-newer-release (OK is Download → a real browser)
//! and the update prompt (OK is Install & Restart → the real installer). There
//! is no implicit OK here. Every affirmative is a named button with its own
//! handler, and Escape resolves through `keys::Cascade`, so "do not press
//! enter on this one" is not a rule anybody can break. The three tests that
//! existed to navigate it are gone with it.

mod support;

use std::cell::RefCell;
use std::path::PathBuf;

use dioxus::prelude::*;
use tempfile::TempDir;

use dat0_core::telemetry::crash::{self, StagedCrash};
use dat0_core::update::manifest::ArtifactEntry;

use dat0_ui::components::modals::{
    DIALOG_ID, ModalHost, ModalOutcome, ModalReply, scrim_dismissable, slug, title,
};
use dat0_ui::components::update_ui::UpdateState;
use dat0_ui::state::{Modal, Workspace};
use support::{Harness, Key, Modifiers};

thread_local! {
    static INITIAL: RefCell<Option<Modal>> = const { RefCell::new(None) };
    static REPLIES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn reply() -> ModalReply {
    ModalReply::new(|o: ModalOutcome| REPLIES.with(|r| r.borrow_mut().push(format!("{o:?}"))))
}

fn replies() -> Vec<String> {
    REPLIES.with(|r| r.borrow().clone())
}

#[component]
fn Host() -> Element {
    let ws = Workspace::provide();
    use_hook(move || {
        let mut ws = ws;
        ws.modal.set(INITIAL.with(|c| c.borrow_mut().take()));
    });
    rsx! { ModalHost {} }
}

fn mount(modal: Modal) -> Harness {
    INITIAL.with(|c| *c.borrow_mut() = Some(modal));
    REPLIES.with(|r| r.borrow_mut().clear());
    Harness::new(Host, ())
}

fn dialogs(h: &Harness) -> usize {
    h.by_role("dialog").len()
}

fn escape(h: &mut Harness) {
    h.key_at(DIALOG_ID, Key::Escape, Modifiers::empty());
}

/// The header the host renders above every panel: the small `.d0-label` slug
/// and the dialog's accessible name.
fn header(h: &Harness) -> String {
    let dialog = h.by_a11y_id(DIALOG_ID).expect("a dialog is mounted");
    h.attr(dialog, "aria-label").expect("a named dialog")
}

// ─────────────────────────────────────────────────────────────────────────────
// The crash reporter
// ─────────────────────────────────────────────────────────────────────────────

fn staged() -> StagedCrash {
    StagedCrash {
        message: "boom at <redacted>/foo.rs:42".into(),
        backtrace: "0: <redacted>::bar".into(),
        version: "9.9.9".into(),
    }
}

fn crash_modal(dir: &TempDir, with_crash: bool) -> Modal {
    if with_crash {
        crash::mark_running(dir.path()).unwrap();
        crash::write_staged(dir.path(), &staged()).unwrap();
    }
    Modal::CrashReport {
        staged: with_crash.then(staged),
        data_dir: dir.path().to_path_buf(),
    }
}

/// The safety spine every crash-report test in the GPUI suite opened with.
///
/// With no telemetry client bound, `submit_staged` / `submit_report`
/// early-return inside `telemetry::capture`, so pressing **Send** transmits
/// nothing and does not block on sentry's 5 s flush. That is the only reason
/// a test is allowed to press Send at all; if it ever stops holding, these
/// tests would start posting real crash reports from CI.
fn assert_telemetry_inactive(when: &str) {
    assert!(
        !dat0_core::telemetry::is_active(),
        "SAFETY: telemetry must be inactive {when}, or Send transmits for real"
    );
}

#[test]
fn sending_a_crash_report_clears_staging_and_transmits_nothing() {
    assert_telemetry_inactive("before the dialog opens");
    let tmp = TempDir::new().unwrap();
    let mut h = mount(crash_modal(&tmp, true));
    assert!(
        crash::read_staged(tmp.path()).is_some(),
        "precondition: a payload is staged"
    );

    h.click("report-send");

    assert_eq!(dialogs(&h), 0, "Send must empty the slot");
    assert!(
        crash::read_staged(tmp.path()).is_none(),
        "a sent report must not be offered again next launch"
    );
    assert_telemetry_inactive("after Send");
}

#[test]
fn dismissing_a_crash_report_from_the_host_still_discards_it() {
    // The exit `views_c` cannot see. Its Dismiss test clicks the panel's own
    // button; this one leaves through the *host* — Escape — which reaches
    // `modals::cancel`, whose `CrashReport` arm calls `crash_report::dismiss`.
    // A host that merely unmounted the panel would leave `last-crash.json` on
    // disk and re-prompt next launch for a report the user already declined.
    assert_telemetry_inactive("before the dialog opens");
    let tmp = TempDir::new().unwrap();
    let mut h = mount(crash_modal(&tmp, true));

    escape(&mut h);

    assert_eq!(dialogs(&h), 0);
    assert!(
        crash::read_staged(tmp.path()).is_none(),
        "Escape must discard the staged payload, not just close the window"
    );
    assert_telemetry_inactive("after Escape");
}

#[test]
fn a_bug_report_never_creates_a_crash_sentinel() {
    // UAT §2.5. Both exits call `clear_staged`, which on a directory with no
    // sentinel is a harmless no-op remove — but "harmless" is exactly the
    // kind of claim that stops being true after a refactor, and a bug report
    // that left a `last-crash.json` behind would make the next launch open a
    // crash prompt for a crash that never happened.
    assert_telemetry_inactive("before the dialog opens");
    let tmp = TempDir::new().unwrap();
    assert!(
        crash::read_staged(tmp.path()).is_none(),
        "precondition: nothing staged before a bug report"
    );

    let mut h = mount(crash_modal(&tmp, false));
    h.click("report-send");

    assert_eq!(dialogs(&h), 0);
    assert!(
        crash::read_staged(tmp.path()).is_none(),
        "Report-a-Bug must not create a last-crash.json"
    );
    assert_telemetry_inactive("after Send");
}

#[test]
fn the_report_header_names_the_kind_of_report_it_is() {
    // The GPUI dialog's TITLE was unreachable — `gpui_component` renders it in
    // chrome AccessKit never saw, which is why `crash_report_window.rs`
    // documents the title as deliberately unseamed and asserts only the body.
    // The host renders the title itself now, so the differential the body
    // carried is available one level up as well.
    let tmp = TempDir::new().unwrap();
    let crashed = mount(crash_modal(&tmp, true));
    let tmp2 = TempDir::new().unwrap();
    let bug = mount(crash_modal(&tmp2, false));

    assert_ne!(header(&crashed), header(&bug));
    assert_eq!(
        header(&crashed),
        dat0_i18n::t("crash.dialog.title"),
        "a prior-run crash announces itself as one"
    );
    assert_eq!(header(&bug), dat0_i18n::t("report.dialog.title"));
}

#[test]
fn a_crash_report_can_be_dismissed_by_a_stray_click() {
    // Deliberate, and the opposite of the export dialog's rule: the scrim
    // dismiss *is* the Dismiss path, which clears staging and sends nothing.
    // The host routes it through `dismiss` rather than merely unmounting, so
    // there is no unsafe outcome for a stray click to pick.
    assert!(scrim_dismissable(&Modal::CrashReport {
        staged: None,
        data_dir: PathBuf::from("/nonexistent"),
    }));
}

// ─────────────────────────────────────────────────────────────────────────────
// About
// ─────────────────────────────────────────────────────────────────────────────

fn about(newer: Option<&str>) -> Modal {
    Modal::About {
        newer: newer.map(str::to_string),
        // The real check is a blocking `ureq` GET handed to `spawn_blocking`,
        // which panics outright with no Tokio runtime under it — and there is
        // no network in CI either.
        check_latest: false,
    }
}

#[test]
fn dismissing_the_newer_release_about_box_downloads_nothing() {
    // The GPUI suite's loudest safety note: never press `enter` on this
    // dialog, because `Dialog::confirm()`'s OK is Download and `on_ok` reaches
    // `platform::open_url` — a real browser, from a test. There is no implicit
    // OK here; Download is a named button nobody presses by accident. What
    // remains worth asserting is that the host's own exits are inert for
    // About: it has no `ModalReply`, so a dismissal must tell nobody and do
    // nothing beyond emptying the slot.
    let mut h = mount(about(Some("0.2.0")));
    assert!(
        h.by_a11y_id("about-download").is_some(),
        "precondition: this is the variant with the dangerous button"
    );

    escape(&mut h);

    assert_eq!(dialogs(&h), 0);
    assert_eq!(
        replies(),
        Vec::<String>::new(),
        "About produces no decision, so a dismissal must invent none"
    );
}

#[test]
fn the_about_header_is_the_same_whether_or_not_an_update_exists() {
    // The nudge belongs in the body, where `views_c` asserts it. A header that
    // changed with the release check would make "About" a status line.
    assert_eq!(title(&about(None)), title(&about(Some("0.2.0"))));
    assert_eq!(title(&about(None)), dat0_i18n::t("about.title"));
    assert_eq!(slug(&about(None)), "about");
}

// ─────────────────────────────────────────────────────────────────────────────
// The updater
// ─────────────────────────────────────────────────────────────────────────────

fn artifact() -> ArtifactEntry {
    ArtifactEntry {
        url: "https://example.invalid/dat0-0.2.0.tar.gz".into(),
        sha256: "00".repeat(32),
        size: 0,
    }
}

fn update(state: UpdateState) -> Modal {
    Modal::Update {
        state,
        is_manual: true,
        reply: reply(),
    }
}

#[test]
fn the_update_header_reports_the_outcome_including_its_detail() {
    // `update_about_window.rs` had one test per outcome, each asserting the
    // alert's text: checking, up-to-date, `Update failed: {msg}` with the
    // underlying message, and `Update available {version}`. The alert *was*
    // the whole dialog; here that string is the modal header, and the two
    // interpolated ones are the reason this is not a lookup table — a failure
    // that dropped its cause, or a prompt that dropped its version, would
    // leave the user with nothing to act on.
    assert_eq!(
        title(&UpdateState::Checking.into_modal()),
        dat0_i18n::t("update.checking")
    );
    assert_eq!(
        title(&UpdateState::UpToDate.into_modal()),
        dat0_i18n::t("update.up_to_date")
    );

    let failed = title(&UpdateState::Failed("network down".into()).into_modal());
    assert!(failed.contains(&dat0_i18n::t("update.failed")), "{failed}");
    assert!(failed.contains("network down"), "{failed}");

    let available = title(
        &UpdateState::Available {
            version: "0.2.0".into(),
            artifact: artifact(),
        }
        .into_modal(),
    );
    assert!(
        available.contains(&dat0_i18n::t("update.available")),
        "{available}"
    );
    assert!(available.contains("0.2.0"), "{available}");
}

#[test]
fn later_answers_the_updater_instead_of_installing() {
    // `update_available_prompt_content_and_later_dismiss` dismissed via
    // `escape` precisely to avoid `enter`, whose `on_ok` spawned the real
    // installer. Both exits are named buttons now, so the guarantee moves up
    // a level: the opener — which owns `perform_install`, a call that
    // downloads, replaces the binary and never returns — must be told
    // `Cancelled` and nothing else.
    let mut h = mount(update(UpdateState::Available {
        version: "0.2.0".into(),
        artifact: artifact(),
    }));
    assert!(
        h.by_a11y_id("update-install").is_some(),
        "precondition: this is the variant that can install"
    );

    h.click("update-later");

    assert_eq!(dialogs(&h), 0);
    assert_eq!(replies(), vec!["Cancelled".to_string()]);
}

#[test]
fn escaping_an_available_update_is_also_a_no() {
    // The same answer through the host's own exit rather than the panel's
    // button, because a flow left waiting on a reply that never comes is
    // indistinguishable from one that is still open.
    let mut h = mount(update(UpdateState::Available {
        version: "0.2.0".into(),
        artifact: artifact(),
    }));

    escape(&mut h);

    assert_eq!(dialogs(&h), 0);
    assert_eq!(replies(), vec!["Cancelled".to_string()]);
}

#[test]
fn installing_hands_the_artifact_to_the_opener_and_never_runs_it_here() {
    // The dialog's job ends at naming the artifact. `perform_install` is
    // blocking, downloads, verifies, replaces the install and execs the new
    // binary — so the panel handing it to the opener rather than calling it is
    // the difference between a modal and a self-updating test run.
    let mut h = mount(update(UpdateState::Available {
        version: "0.2.0".into(),
        artifact: artifact(),
    }));

    h.click("update-install");

    assert_eq!(
        dialogs(&h),
        0,
        "the slot empties; the opener takes it from here"
    );
    let told = replies();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].starts_with("Install("), "{told:?}");
    assert!(
        told[0].contains("dat0-0.2.0.tar.gz"),
        "the opener must receive the artifact it is to fetch: {told:?}"
    );
}

/// Sugar so the header assertions read as one line each.
trait IntoModal {
    fn into_modal(self) -> Modal;
}

impl IntoModal for UpdateState {
    fn into_modal(self) -> Modal {
        Modal::Update {
            state: self,
            is_manual: true,
            reply: reply(),
        }
    }
}
