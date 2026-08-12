//! The seven guard surfaces: About, crash report, the onboarding tour,
//! recovery, live-refresh confirm, workspace-in-use and the update prompt.
//!
//! Four of these stand between the user and losing something — a crash report
//! they did not consent to sending, a folder of edits, a workspace opened from
//! two machines at once, a half-finished promotion. The tests here are about
//! those gates: which exit runs which side effect, and which state is *not*
//! allowed to interrupt.

mod support;

use std::fs;
use std::path::PathBuf;

use dioxus::prelude::*;
use serial_test::serial;
use tempfile::TempDir;

use dat0_core::telemetry::crash::{self, StagedCrash};
use dat0_core::update::manifest::ArtifactEntry;
use dat0_core::workspace::lock_manifest::LockManifest;

use dat0_ui::components::about::About;
use dat0_ui::components::crash_report::{self, CrashReport};
use dat0_ui::components::live_refresh::LiveRefreshConfirm;
use dat0_ui::components::onboarding::OnboardingTour;
use dat0_ui::components::recovery::RecoveryPanel;
use dat0_ui::components::update_ui::{UpdatePrompt, UpdateState};
use dat0_ui::components::workspace_in_use::{InUse, WorkspaceInUse};
use support::Harness;

/// A callback log the harness can read, since it sees DOM and not Rust state.
///
/// Rendered as one node per host so an assertion names the exact sequence of
/// calls, not merely that something fired.
#[component]
fn Log(entries: Signal<Vec<String>>) -> Element {
    rsx! { div { "data-a11y-id": "log", {entries.read().join("|")} } }
}

fn log_of(h: &Harness) -> String {
    h.text_of(h.by_a11y_id("log").expect("the host renders a log"))
}

// ── onboarding ──────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct TourHostProps {
    initial_step: usize,
}

#[component]
fn TourHost(props: TourHostProps) -> Element {
    let mut entries = use_signal(Vec::<String>::new);
    let mut tick = use_signal(|| 0_u32);

    rsx! {
        // An unrelated sibling whose signal a test can bump, to force the host
        // to re-render without touching the tour's props.
        button { "data-a11y-id": "tick", onclick: move |_| tick += 1, "{tick}" }
        OnboardingTour {
            initial_step: props.initial_step,
            on_finish: move |_| entries.write().push("finish".to_string()),
        }
        Log { entries }
    }
}

fn tour(initial_step: usize) -> Harness {
    Harness::new(TourHost, TourHostProps { initial_step })
}

fn panel_title(n: usize) -> String {
    dat0_i18n::t(&format!("onboarding.tour.p{n}.title"))
}

#[test]
fn the_tour_steps_forward_and_back_through_all_seven_panels() {
    let mut h = tour(0);
    assert!(h.has_label(&panel_title(1)));
    assert!(
        h.by_a11y_id("tour-back").is_none(),
        "there is nothing behind panel one"
    );

    for n in 2..=7 {
        h.click("tour-next");
        assert!(h.has_label(&panel_title(n)), "forward to panel {n}");
        assert!(
            h.by_a11y_id("tour-back").is_some(),
            "Back appears from panel two on"
        );
    }

    // The primary button changes identity on the last panel, so a click on
    // "Next" cannot silently finish the tour and a click on "Get started"
    // cannot silently advance it.
    assert!(h.by_a11y_id("tour-next").is_none());
    assert!(h.by_a11y_id("tour-get-started").is_some());

    for n in (1..=6).rev() {
        h.click("tour-back");
        assert!(h.has_label(&panel_title(n)), "back to panel {n}");
    }
    assert!(h.by_a11y_id("tour-back").is_none());
}

#[test]
fn the_step_survives_a_parent_re_render() {
    // The GPUI carousel could not hold a step: `open_dialog` stacked, so every
    // Back and Next closed and rebuilt the panel. Here the step is a signal
    // inside one mounted component, and the proof is that something outside it
    // can re-render without resetting it.
    let mut h = tour(0);
    h.click("tour-next");
    h.click("tour-next");
    assert!(h.has_label(&panel_title(3)));

    h.click("tick");
    h.settle();

    assert!(
        h.has_label(&panel_title(3)),
        "a parent re-render must not send the tour back to panel one"
    );
}

#[test]
fn an_initial_step_past_the_end_clamps_to_the_last_panel() {
    let h = tour(99);
    assert!(h.has_label(&panel_title(7)));
}

/// Point the settings store at a scratch directory. `DAT0_CONFIG_DIR` is
/// process-global, hence `#[serial]`.
fn with_config_dir<R>(f: impl FnOnce(&TempDir) -> R) -> R {
    let tmp = TempDir::new().unwrap();
    let previous = std::env::var_os("DAT0_CONFIG_DIR");
    // SAFETY: `#[serial]` keeps every env-touching test off the same clock,
    // and no other thread in this binary reads the variable.
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

fn first_run_done(dir: &TempDir) -> bool {
    dat0_core::settings::store::SettingsStore::with_path(dir.path().join("settings.toml"))
        .load_or_default()
        .unwrap()
        .first_run_done
}

#[test]
#[serial]
fn skipping_the_tour_records_that_it_was_seen() {
    with_config_dir(|dir| {
        let mut h = tour(0);
        assert!(!first_run_done(dir));

        h.click("tour-skip");

        assert_eq!(log_of(&h), "finish");
        assert!(
            first_run_done(dir),
            "skip is an answer; the tour must not come back next launch"
        );
    });
}

#[test]
#[serial]
fn finishing_the_tour_records_that_it_was_seen() {
    with_config_dir(|dir| {
        let mut h = tour(6);
        h.click("tour-get-started");

        assert_eq!(log_of(&h), "finish");
        assert!(first_run_done(dir));
    });
}

#[test]
#[serial]
fn stepping_through_the_tour_does_not_record_it_as_seen() {
    // Only an exit answers the question. A user who closed the laptop on panel
    // three has not dismissed anything.
    with_config_dir(|dir| {
        let mut h = tour(0);
        h.click("tour-next");
        h.click("tour-next");
        assert!(!first_run_done(dir));
        assert_eq!(log_of(&h), "");
    });
}

// ── workspace in use ────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct InUseHostProps {
    kind: InUse,
}

#[component]
fn InUseHost(props: InUseHostProps) -> Element {
    let mut entries = use_signal(Vec::<String>::new);
    rsx! {
        WorkspaceInUse {
            kind: props.kind.clone(),
            on_proceed: move |_| entries.write().push("proceed".to_string()),
            on_cancel: move |_| entries.write().push("cancel".to_string()),
        }
        Log { entries }
    }
}

fn holder(host: &str) -> LockManifest {
    LockManifest {
        pid: 91,
        hostname: host.to_string(),
        started_at: "1000".to_string(),
        dat0_version: "0.1.0".to_string(),
        tombstoned: false,
    }
}

fn in_use(kind: InUse) -> Harness {
    Harness::new(InUseHost, InUseHostProps { kind })
}

#[test]
fn opening_anyway_across_machines_takes_the_proceed_path() {
    let mut h = in_use(InUse::Conflict {
        holder: holder("studio-imac"),
        now_secs: 1000 + 7200,
    });
    assert!(
        h.has_label_contains("studio-imac"),
        "the warning must name the machine that holds it"
    );
    assert!(h.has_label_contains("2 hour(s) ago"));

    h.click("workspace-in-use-proceed");
    assert_eq!(log_of(&h), "proceed");
}

#[test]
fn cancelling_a_cross_machine_conflict_opens_nothing() {
    let mut h = in_use(InUse::Conflict {
        holder: holder("nas"),
        now_secs: 1000,
    });
    h.click("workspace-in-use-cancel");
    assert_eq!(log_of(&h), "cancel");
}

#[test]
fn the_same_machine_gate_offers_focus_rather_than_a_warning() {
    let mut h = in_use(InUse::SameMachine);
    assert!(
        h.by_a11y_id("workspace-in-use-body").is_none(),
        "another window of this process is not a corruption risk; do not imply one"
    );
    assert!(h.query_by_role("button", &dat0_i18n::t("workspace.in_use.focus_existing")));

    h.click("workspace-in-use-proceed");
    assert_eq!(log_of(&h), "proceed");
}

#[test]
fn the_in_use_gate_has_no_exit_other_than_its_two_buttons() {
    // Both outcomes have consequences, so a stray click on the scrim, an
    // Escape or a header ✕ must not resolve to either one.
    // A compile-time assert: flipping the const is the regression, and this
    // catches it at build time rather than on a test run.
    const _: () = assert!(!dat0_ui::components::workspace_in_use::SCRIM_DISMISSABLE);

    let h = in_use(InUse::SameMachine);
    let buttons = h.by_role("button");
    assert_eq!(
        buttons.len(),
        2,
        "exactly Cancel and the proceed verb, nothing else"
    );
}

// ── crash report ────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct ReportHostProps {
    staged: Option<StagedCrash>,
    data_dir: PathBuf,
}

#[component]
fn ReportHost(props: ReportHostProps) -> Element {
    let mut entries = use_signal(Vec::<String>::new);
    rsx! {
        CrashReport {
            staged: props.staged.clone(),
            data_dir: props.data_dir.clone(),
            on_close: move |_| entries.write().push("close".to_string()),
        }
        Log { entries }
    }
}

fn staged() -> StagedCrash {
    StagedCrash {
        message: "index out of bounds".into(),
        backtrace: "frame 0".into(),
        version: "0.1.0".into(),
    }
}

fn report(dir: &TempDir, with_crash: bool) -> Harness {
    if with_crash {
        crash::mark_running(dir.path()).unwrap();
        crash::write_staged(dir.path(), &staged()).unwrap();
    }
    Harness::new(
        ReportHost,
        ReportHostProps {
            staged: with_crash.then(staged),
            data_dir: dir.path().to_path_buf(),
        },
    )
}

#[test]
fn opting_out_discards_a_staged_crash_without_ever_asking() {
    let tmp = TempDir::new().unwrap();
    crash::mark_running(tmp.path()).unwrap();
    crash::write_staged(tmp.path(), &staged()).unwrap();

    assert!(crash_report::on_relaunch(tmp.path(), false).is_none());
    assert!(
        !crash::staged_path(tmp.path()).exists(),
        "an opted-out payload is deleted, not kept for a launch when they might say yes"
    );
}

#[test]
fn opting_in_surfaces_the_staged_crash() {
    let tmp = TempDir::new().unwrap();
    crash::mark_running(tmp.path()).unwrap();
    crash::write_staged(tmp.path(), &staged()).unwrap();

    assert_eq!(crash_report::on_relaunch(tmp.path(), true), Some(staged()));
    assert!(crash::staged_path(tmp.path()).exists());
}

#[test]
fn dismissing_the_report_clears_staging_and_sends_nothing() {
    let tmp = TempDir::new().unwrap();
    let mut h = report(&tmp, true);
    assert!(crash::staged_path(tmp.path()).exists());

    h.click("report-dismiss");

    assert_eq!(log_of(&h), "close");
    assert!(
        !crash::staged_path(tmp.path()).exists(),
        "a declined report must not be re-offered next launch"
    );
}

#[test]
fn sending_the_report_clears_staging_too() {
    let tmp = TempDir::new().unwrap();
    let mut h = report(&tmp, true);

    h.click("report-send");

    assert_eq!(log_of(&h), "close");
    assert!(
        !crash::staged_path(tmp.path()).exists(),
        "a sent report must not be sent again next launch"
    );
}

#[test]
fn the_bug_report_and_the_crash_report_say_different_things() {
    let tmp = TempDir::new().unwrap();
    let bug = report(&tmp, false);
    let bug_body = bug.text_of(bug.by_a11y_id("report-body").unwrap());

    let tmp2 = TempDir::new().unwrap();
    let crashed = report(&tmp2, true);
    let crash_body = crashed.text_of(crashed.by_a11y_id("report-body").unwrap());

    assert_ne!(bug_body, crash_body);
    assert_eq!(bug_body, dat0_i18n::t("report.dialog.body"));
    assert_eq!(crash_body, dat0_i18n::t("crash.dialog.body"));
}

#[test]
fn the_note_field_is_optional_and_editable() {
    let tmp = TempDir::new().unwrap();
    let mut h = report(&tmp, false);
    let note = h.by_a11y_id("report-note").expect("a note field");
    assert_eq!(h.attr(note, "value").as_deref(), Some(""));

    h.dispatch(
        note,
        "input",
        dioxus::html::SerializedFormData::new("it happened on export".to_string(), Vec::new()),
    );

    let note = h.by_a11y_id("report-note").unwrap();
    assert_eq!(
        h.attr(note, "value").as_deref(),
        Some("it happened on export")
    );
}

// ── update ──────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct UpdateHostProps {
    state: UpdateState,
    is_manual: bool,
}

#[component]
fn UpdateHost(props: UpdateHostProps) -> Element {
    let mut entries = use_signal(Vec::<String>::new);
    rsx! {
        UpdatePrompt {
            state: props.state.clone(),
            is_manual: props.is_manual,
            on_install: move |a: ArtifactEntry| entries.write().push(format!("install:{}", a.url)),
            on_close: move |_| entries.write().push("close".to_string()),
        }
        Log { entries }
    }
}

fn artifact() -> ArtifactEntry {
    ArtifactEntry {
        url: "https://example.invalid/dat0-9.9.9.tar.gz".into(),
        sha256: "a".repeat(64),
        size: 42,
    }
}

fn update(state: UpdateState, is_manual: bool) -> Harness {
    Harness::new(UpdateHost, UpdateHostProps { state, is_manual })
}

fn available() -> UpdateState {
    UpdateState::Available {
        version: "9.9.9".into(),
        artifact: artifact(),
    }
}

#[test]
fn a_background_check_stays_silent_unless_it_found_something() {
    for state in [
        UpdateState::Checking,
        UpdateState::UpToDate,
        UpdateState::Failed("dns failure".into()),
    ] {
        let h = update(state.clone(), false);
        assert!(
            h.by_a11y_id("update").is_none(),
            "{state:?} must not interrupt a launch-time check"
        );
    }

    let h = update(available(), false);
    assert!(
        h.by_a11y_id("update").is_some(),
        "a found update is the one thing worth interrupting for"
    );
}

#[test]
fn a_manual_check_reports_every_outcome() {
    let h = update(UpdateState::Checking, true);
    assert!(h.has_label(&dat0_i18n::t("update.checking")));

    let h = update(UpdateState::UpToDate, true);
    assert!(h.has_label(&dat0_i18n::t("update.up_to_date")));

    let h = update(UpdateState::Failed("signature mismatch".into()), true);
    assert!(
        h.has_label_contains("signature mismatch"),
        "a manual check that failed must say why"
    );
}

#[test]
fn the_informational_states_offer_only_dismissal() {
    let mut h = update(UpdateState::UpToDate, true);
    assert!(h.by_a11y_id("update-install").is_none());
    h.click("update-ok");
    assert_eq!(log_of(&h), "close");
}

#[test]
fn later_dismisses_an_available_update_without_installing() {
    let mut h = update(available(), true);
    h.click("update-later");
    assert_eq!(log_of(&h), "close");
}

#[test]
fn installing_hands_over_the_artifact_and_shows_the_download() {
    let mut h = update(available(), false);
    assert!(h.has_label_contains("9.9.9"), "the version must be visible");

    h.click("update-install");

    assert_eq!(
        log_of(&h),
        "install:https://example.invalid/dat0-9.9.9.tar.gz"
    );
    assert!(
        h.has_label(&dat0_i18n::t("update.downloading")),
        "the dialog must show that something is now happening; the install \
         ends in a relaunch, so this is its last visible state"
    );
}

// ── recovery ────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct RecoveryHostProps {
    scratch_root: PathBuf,
    recent_roots: Vec<PathBuf>,
}

#[component]
fn RecoveryHost(props: RecoveryHostProps) -> Element {
    let mut entries = use_signal(Vec::<String>::new);
    rsx! {
        RecoveryPanel {
            scratch_root: props.scratch_root.clone(),
            recent_roots: props.recent_roots.clone(),
            on_open: move |p: PathBuf| {
                entries.write().push(format!("open:{}", name_of(&p)));
            },
            on_resume: move |p: PathBuf| {
                entries.write().push(format!("resume:{}", name_of(&p)));
            },
            on_close: move |_| entries.write().push("close".to_string()),
        }
        Log { entries }
    }
}

fn name_of(p: &std::path::Path) -> String {
    p.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn seed_orphan(scratch: &std::path::Path, name: &str, tables: &[&str]) -> PathBuf {
    let dir = scratch.join(name);
    fs::create_dir_all(&dir).unwrap();
    let tabs: Vec<String> = tables
        .iter()
        .map(|t| format!(r#"{{"table_name":"{t}","source_path":null}}"#))
        .collect();
    fs::write(
        dir.join("session.json"),
        format!(r#"{{"tabs":[{}],"active_tab":0}}"#, tabs.join(",")),
    )
    .unwrap();
    dir
}

fn seed_incomplete(root: &std::path::Path) {
    fs::create_dir_all(root.join(".dat0")).unwrap();
    // A promotion that moved the database but never wrote the manifest.
    fs::write(root.join(".dat0/workspace.duckdb"), b"db").unwrap();
}

struct Fixture {
    _tmp: TempDir,
    scratch: PathBuf,
    recents: Vec<PathBuf>,
}

fn recovery_fixture() -> Fixture {
    let tmp = TempDir::new().unwrap();
    let scratch = tmp.path().join("scratch");
    fs::create_dir_all(&scratch).unwrap();
    seed_orphan(&scratch, "aaa", &["orders", "returns"]);
    seed_orphan(&scratch, "bbb", &["events"]);

    let project = tmp.path().join("q2-report");
    seed_incomplete(&project);

    Fixture {
        _tmp: tmp,
        scratch,
        recents: vec![project],
    }
}

fn recovery(f: &Fixture) -> Harness {
    Harness::new(
        RecoveryHost,
        RecoveryHostProps {
            scratch_root: f.scratch.clone(),
            recent_roots: f.recents.clone(),
        },
    )
}

#[test]
fn the_panel_lists_both_kinds_of_wreckage_under_their_own_headings() {
    let f = recovery_fixture();
    let h = recovery(&f);

    assert!(h.by_a11y_id("recovery-section-orphans").is_some());
    assert!(h.by_a11y_id("recovery-section-incomplete").is_some());

    let text = h.text();
    assert!(
        text.contains("orders, returns"),
        "an orphan is identified by the tables it restores, not by its uuid: {text}"
    );
    assert!(text.contains("events"));
    assert!(text.contains("q2-report"));
    assert!(
        text.contains(&dat0_i18n::t("recovery.row.incomplete_suffix")),
        "an interrupted promotion must say so: {text}"
    );

    // Orphans get Open, interrupted workspaces get Resume — different verbs
    // because they reach different code paths.
    assert!(h.by_a11y_id("recovery-open-0").is_some());
    assert!(h.by_a11y_id("recovery-open-1").is_some());
    assert!(h.by_a11y_id("recovery-resume-2").is_some());
}

#[test]
fn restoring_an_orphan_closes_the_panel_first_then_hands_over_the_directory() {
    let f = recovery_fixture();
    let mut h = recovery(&f);

    h.click("recovery-open-0");

    assert_eq!(
        log_of(&h),
        "close|open:aaa",
        "the panel must be out of the way before the recovered window arrives"
    );
}

#[test]
fn resuming_an_interrupted_workspace_uses_the_workspace_path() {
    let f = recovery_fixture();
    let mut h = recovery(&f);

    h.click("recovery-resume-2");

    assert_eq!(log_of(&h), "close|resume:q2-report");
}

#[test]
fn discarding_rescans_so_the_removed_row_cannot_linger() {
    let f = recovery_fixture();
    let mut h = recovery(&f);
    assert!(f.scratch.join("aaa").exists());

    h.click("recovery-discard-0");

    assert!(!f.scratch.join("aaa").exists(), "the scratch dir is gone");
    let text = h.text();
    assert!(
        !text.contains("orders, returns"),
        "and so is its row: {text}"
    );
    assert!(text.contains("events"), "the other rows stay: {text}");
    assert_eq!(log_of(&h), "", "a discard is not a close");
}

#[test]
fn discarding_an_interrupted_workspace_spares_the_users_folder() {
    let f = recovery_fixture();
    let root = f.recents[0].clone();
    let precious = root.join("sales.csv");
    fs::write(&precious, b"a,b\n").unwrap();
    let mut h = recovery(&f);

    h.click("recovery-discard-2");

    assert!(!root.join(".dat0").exists(), "the half-promotion goes");
    assert!(precious.exists(), "the user's data must not");
    assert!(root.exists());
}

#[test]
fn discarding_the_last_item_closes_the_panel() {
    let tmp = TempDir::new().unwrap();
    let scratch = tmp.path().join("scratch");
    fs::create_dir_all(&scratch).unwrap();
    seed_orphan(&scratch, "only", &["t"]);
    let f = Fixture {
        _tmp: tmp,
        scratch,
        recents: Vec::new(),
    };
    let mut h = recovery(&f);

    h.click("recovery-discard-0");

    assert_eq!(
        log_of(&h),
        "close",
        "an empty recovery list is the same state as nothing to recover"
    );
    assert!(h.by_a11y_id("recovery").is_none());
}

#[test]
fn nothing_to_recover_renders_no_panel_at_all() {
    let tmp = TempDir::new().unwrap();
    let scratch = tmp.path().join("scratch");
    fs::create_dir_all(&scratch).unwrap();
    let f = Fixture {
        _tmp: tmp,
        scratch,
        recents: Vec::new(),
    };

    let h = recovery(&f);
    assert!(h.by_a11y_id("recovery").is_none());
}

// ── about ───────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct AboutHostProps {
    newer: Option<String>,
}

#[component]
fn AboutHost(props: AboutHostProps) -> Element {
    let mut entries = use_signal(Vec::<String>::new);
    rsx! {
        About {
            newer: props.newer.clone(),
            // The real check is a blocking `ureq` GET handed to
            // `spawn_blocking`; there is no Tokio runtime under the harness
            // and no network in CI.
            check_latest: false,
            on_close: move |_| entries.write().push("close".to_string()),
        }
        Log { entries }
    }
}

fn about(newer: Option<&str>) -> Harness {
    Harness::new(
        AboutHost,
        AboutHostProps {
            newer: newer.map(str::to_string),
        },
    )
}

#[test]
fn about_shows_the_build_identity() {
    let h = about(None);
    let body = h.text_of(h.by_a11y_id("about-body").unwrap());
    assert!(body.contains(&dat0_i18n::t("about.version")));
    assert!(body.contains(&dat0_i18n::t("about.license")));
    assert!(body.contains(&dat0_i18n::t("about.acknowledgements")));
}

#[test]
fn an_up_to_date_build_offers_nothing_to_download() {
    let mut h = about(None);
    assert!(h.by_a11y_id("about-download").is_none());
    assert!(
        h.text_of(h.by_a11y_id("about-body").unwrap())
            .contains(&dat0_i18n::t("about.update.current"))
    );

    h.click("about-ok");
    assert_eq!(log_of(&h), "close");
}

#[test]
fn a_newer_release_adds_the_nudge_and_a_download_button() {
    let mut h = about(Some("v9.9.9"));
    let body = h.text_of(h.by_a11y_id("about-body").unwrap());
    assert!(body.contains("v9.9.9"), "{body}");
    assert!(
        body.contains(&dat0_i18n::t("about.update.available")),
        "{body}"
    );

    assert!(
        h.by_a11y_id("about-ok").is_none(),
        "the single-button form is gone"
    );
    assert!(h.by_a11y_id("about-download").is_some());

    // Cancel is the safe exit; Download is not clicked here because it opens
    // the user's browser.
    h.click("about-cancel");
    assert_eq!(log_of(&h), "close");
}

// ── live refresh ────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct RefreshHostProps {
    edits: usize,
    deletes: usize,
}

#[component]
fn RefreshHost(props: RefreshHostProps) -> Element {
    let mut entries = use_signal(Vec::<String>::new);
    rsx! {
        LiveRefreshConfirm {
            dropped_edits: props.edits,
            dropped_deletes: props.deletes,
            on_confirm: move |_| entries.write().push("confirm".to_string()),
            on_cancel: move |_| entries.write().push("cancel".to_string()),
        }
        Log { entries }
    }
}

fn refresh(edits: usize, deletes: usize) -> Harness {
    Harness::new(RefreshHost, RefreshHostProps { edits, deletes })
}

#[test]
fn the_refresh_warning_names_exactly_what_is_lost() {
    let h = refresh(12, 3);
    let body = h.text_of(h.by_a11y_id("live-refresh-body").unwrap());
    assert!(body.contains("12"), "{body}");
    assert!(body.contains('3'), "{body}");
    assert!(
        !body.contains("{edits}") && !body.contains("{deletes}"),
        "an un-interpolated placeholder would ship as literal braces: {body}"
    );
}

#[test]
fn refreshing_anyway_and_cancelling_are_different_answers() {
    let mut h = refresh(1, 0);
    h.click("live-refresh-confirm");
    assert_eq!(log_of(&h), "confirm");

    let mut h = refresh(1, 0);
    h.click("live-refresh-cancel");
    assert_eq!(log_of(&h), "cancel");
}

#[test]
fn the_refresh_confirmation_cannot_be_resolved_by_a_stray_click() {
    // A compile-time assert: flipping the const is the regression, and this
    // catches it at build time rather than on a test run.
    const _: () = assert!(!dat0_ui::components::live_refresh::SCRIM_DISMISSABLE);
}
