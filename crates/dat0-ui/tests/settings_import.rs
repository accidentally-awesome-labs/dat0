//! Settings, the import strip, the import wizard and the AI panel.
//!
//! Four surfaces, one binary, because they share a spine: each is a form over
//! something persistent — `settings.toml`, the keychain, the active import —
//! and what is worth testing about all four is whether the thing on screen and
//! the thing on disk agree.
//!
//! What is deliberately *not* here: geometry. The harness has no layout, so a
//! test that a bar is 4px tall would be testing the string "4px".

// `AiDeps` declares `probe`/`keys` as `Arc<dyn ..>`, and these scripted
// fixtures hold `RefCell`, so the Arc is not Sync. The harness is
// single-threaded and the type is the production API's, not a choice made
// here - satisfying the lint would mean changing `AiDeps`.
#![allow(clippy::arc_with_non_send_sync)]

mod support;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use dioxus::prelude::*;
use futures::channel::oneshot;

use dat0_core::ai::key_store::{KeyStore, MemoryKeyStore};
use dat0_core::ai::settings::AiSettings;
use dat0_core::ai::transport::TestOutcome;
use dat0_core::ai::{AiRequest, Provider};
use dat0_core::file_drop::DropOutcome;
use dat0_core::import_wizard::SniffSummary;
use dat0_core::settings::Settings;
use dat0_core::settings::store::SettingsStore;
use dat0_engine::transform::ROWID_COL;

use dat0_ui::components::ai::{
    AiController, AiDeps, AiFuture, AiPanel, AiPanelEvent, AiProbe, StreamKind, StreamPhase,
};
use dat0_ui::components::import_progress::{ImportProgress, ImportState};
use dat0_ui::components::import_wizard::{
    ColumnDraft, ImportWizard, Issue, Step, TYPES, WizardModel,
};
use dat0_ui::components::settings_ui::{
    Bus, LOG_LEVELS, SECTIONS, SettingsPanel, SettingsProps, Store,
};

use support::Harness;

/// A form event carrying `value`. `checked()` parses the same string, so
/// `"true"` / `"false"` also drives a checkbox.
fn form(value: &str) -> dioxus::html::SerializedFormData {
    dioxus::html::SerializedFormData::new(value.to_string(), Vec::new())
}

/// The text of a readback node.
fn read(h: &Harness, id: &str) -> String {
    h.text_of(h.by_a11y_id(id).unwrap_or_else(|| panic!("no {id}")))
}

// ─────────────────────────────────────────────────────────────────────────────
// settings
// ─────────────────────────────────────────────────────────────────────────────

/// A settings panel over a throwaway store, plus the store to assert on.
fn settings() -> (Harness, Arc<SettingsStore>) {
    let store = Arc::new(SettingsStore::open_in_memory());
    let h = Harness::new(
        SettingsPanel,
        SettingsProps {
            store: Store(Arc::clone(&store)),
            events: Bus(None),
        },
    );
    (h, store)
}

#[test]
fn every_section_is_reachable_from_the_sidebar() {
    let (mut h, _store) = settings();
    for s in SECTIONS {
        h.click(s.id);
        assert!(
            h.by_a11y_id(&format!("settings-body-{}", s.id)).is_some(),
            "section {} did not render",
            s.id
        );
    }
}

#[test]
fn the_telemetry_toggle_persists_and_reflects_the_store() {
    let (mut h, store) = settings();
    h.click("telemetry");

    assert_eq!(
        h.attr(h.by_a11y_id("tg-telemetry").unwrap(), "aria-checked")
            .as_deref(),
        Some("false"),
        "crash submission is off by default"
    );

    h.click("tg-telemetry");
    assert!(
        store
            .load_or_default()
            .unwrap()
            .telemetry
            .crash_submission_enabled,
        "the click must reach settings.toml, not just the pixel"
    );
    assert_eq!(
        h.attr(h.by_a11y_id("tg-telemetry").unwrap(), "aria-checked")
            .as_deref(),
        Some("true"),
        "and the control must re-read what it wrote"
    );

    h.click("tg-telemetry");
    assert!(
        !store
            .load_or_default()
            .unwrap()
            .telemetry
            .crash_submission_enabled
    );
}

#[test]
fn the_networked_workspace_override_persists() {
    let (mut h, store) = settings();
    h.click("workspace");
    h.click("tg-workspace");
    assert!(
        store
            .load_or_default()
            .unwrap()
            .workspace
            .treat_all_as_networked
    );
}

#[test]
fn the_update_check_opt_out_persists() {
    let (mut h, store) = settings();
    h.click("updates");
    // Ships on, so the interesting direction is off.
    assert!(store.load_or_default().unwrap().update_auto_check);
    h.click("tg-updates");
    assert!(!store.load_or_default().unwrap().update_auto_check);
}

#[test]
fn typing_a_name_persists_it_through_the_store() {
    let (mut h, store) = settings();
    let field = h.by_a11y_id("settings-name-input").expect("name field");
    h.dispatch(field, "input", form("Ada Lovelace"));
    assert_eq!(
        store.get_string("author.name").as_deref(),
        Some("Ada Lovelace")
    );
}

#[test]
fn a_half_typed_memory_budget_is_never_persisted() {
    // Clearing the field must not be read as "zero megabytes"; the engine
    // would take that literally at the next window open.
    let (mut h, store) = settings();
    h.click("memory_budget");
    let field = h.by_a11y_id("settings-budget-input").expect("budget field");

    h.dispatch(field, "input", form(""));
    assert_eq!(
        store.load_or_default().unwrap().memory_budget_mb,
        1024,
        "an unparseable value leaves the persisted budget alone"
    );

    let field = h.by_a11y_id("settings-budget-input").unwrap();
    h.dispatch(field, "input", form("2048"));
    assert_eq!(store.load_or_default().unwrap().memory_budget_mb, 2048);
}

#[test]
fn cycling_the_theme_persists_the_next_one() {
    let (mut h, store) = settings();
    h.click("theme");
    let before = store.get_string("theme.id");
    h.click("settings-theme-cycle");
    let after = store.get_string("theme.id").expect("a theme was written");
    assert_ne!(before.as_deref(), Some(after.as_str()));
    assert!(
        dat0_core::theme::BUILTIN_IDS.contains(&after.as_str()),
        "cycled to {after}, which is not a builtin"
    );
}

#[test]
fn resetting_asks_first_and_cancelling_changes_nothing() {
    let (mut h, store) = settings();
    // Something to lose.
    let mut s = Settings {
        update_auto_check: false,
        ..Settings::default()
    };
    s.profile.author_name = "Ada".into();
    store.save(&s).unwrap();

    h.click("advanced");
    assert!(
        h.by_a11y_id("adv-reset-confirm").is_none(),
        "the confirmation must not be showing before it is asked for"
    );

    h.click("adv-reset");
    assert!(h.by_a11y_id("adv-reset-confirm").is_some());
    h.click("adv-reset-cancel");
    assert!(h.by_a11y_id("adv-reset-confirm").is_none());
    assert_eq!(
        store.load_or_default().unwrap().profile.author_name,
        "Ada",
        "cancel must not reset anything"
    );

    h.click("adv-reset");
    h.click("adv-reset-ok");
    assert_eq!(store.load_or_default().unwrap(), Settings::default());
}

#[test]
fn the_log_level_cycles_through_the_persisted_setting() {
    let (mut h, store) = settings();
    h.click("advanced");
    let before = store.load_or_default().unwrap().log_level;
    h.click("adv-log-level");
    let after = store.load_or_default().unwrap().log_level;
    assert_ne!(before, after);
    assert!(LOG_LEVELS.contains(&after.as_str()), "cycled to {after}");
}

// ─────────────────────────────────────────────────────────────────────────────
// import progress
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct ProgressHostProps {
    state: ImportState,
}

#[component]
fn ProgressHost(props: ProgressHostProps) -> Element {
    let mut cancelled = use_signal(|| false);
    let mut dismissed = use_signal(|| false);
    rsx! {
        ImportProgress {
            state: props.state.clone(),
            on_cancel: move |_| cancelled.set(true),
            on_dismiss: move |_| dismissed.set(true),
        }
        div { "data-a11y-id": "t-cancelled", "{cancelled}" }
        div { "data-a11y-id": "t-dismissed", "{dismissed}" }
    }
}

fn progress(state: ImportState) -> Harness {
    Harness::new(ProgressHost, ProgressHostProps { state })
}

#[test]
fn an_idle_import_shows_nothing_at_all() {
    // A strip that is always mounted eats a row of the workbench forever.
    let h = progress(ImportState::Idle);
    assert!(h.by_a11y_id("import-progress").is_none());
}

#[test]
fn a_running_import_reports_progress_and_offers_cancel() {
    let h = progress(ImportState::Running {
        file: PathBuf::from("/tmp/sales.csv"),
        done: 25,
        total: 100,
    });
    let bar = h.by_a11y_id("import-bar").expect("a bar");
    assert_eq!(h.attr(bar, "aria-valuenow").as_deref(), Some("25"));
    assert!(h.by_a11y_id("import-cancel").is_some());
    assert!(
        h.by_a11y_id("import-dismiss").is_none(),
        "a live import is stopped by cancelling it, not by hiding it"
    );
}

#[test]
fn an_unknown_size_leaves_the_bar_indeterminate() {
    // `aria-valuenow` absent is the ARIA definition of indeterminate; a 0
    // would be announced as "no progress", which is a different claim.
    let h = progress(ImportState::Running {
        file: PathBuf::from("/tmp/stream.csv"),
        done: 4096,
        total: 0,
    });
    let bar = h.by_a11y_id("import-bar").unwrap();
    assert_eq!(h.attr(bar, "aria-valuenow"), None);
}

#[test]
fn a_failed_import_is_an_alert_that_names_the_error() {
    let h = progress(ImportState::from_outcome(&DropOutcome::EngineError {
        path: PathBuf::from("/tmp/broken.csv"),
        error: "Conversion Error: could not convert 'x' to BIGINT".into(),
    }));
    let strip = h.by_a11y_id("import-progress").expect("the strip");
    assert_eq!(
        h.attr(strip, "role").as_deref(),
        Some("alert"),
        "a failure must interrupt; a note is read after everything else"
    );
    assert!(h.text_of(strip).contains("could not convert"));
    assert!(
        h.by_a11y_id("import-dismiss").is_some(),
        "a terminal state must be dismissible or it is permanent"
    );
}

#[test]
fn an_unsupported_extension_says_which_one() {
    let h = progress(ImportState::from_outcome(&DropOutcome::Unsupported {
        path: PathBuf::from("/tmp/notes.docx"),
        extension: Some("docx".into()),
    }));
    let strip = h.by_a11y_id("import-progress").unwrap();
    assert!(h.text_of(strip).contains("docx"));
    assert_eq!(h.attr(strip, "role").as_deref(), Some("alert"));
}

#[test]
fn a_registered_import_names_the_table_it_landed_in() {
    let h = progress(ImportState::from_outcome(&DropOutcome::Registered {
        table_name: "sales".into(),
        source_path: PathBuf::from("/tmp/sales.csv"),
    }));
    let strip = h.by_a11y_id("import-progress").unwrap();
    assert!(h.text_of(strip).contains("sales"));
    assert_eq!(
        h.attr(strip, "role").as_deref(),
        Some("note"),
        "success is not an alert"
    );
}

#[test]
fn a_wizard_bound_drop_leaves_the_strip_silent() {
    // The wizard is about to take over the screen; two things describing the
    // same file at once is noise.
    assert_eq!(
        ImportState::from_outcome(&DropOutcome::OpenWizard {
            path: PathBuf::from("/tmp/ambiguous.csv"),
            sniff: sniff(true),
        }),
        ImportState::Idle
    );
}

#[test]
fn cancel_flips_the_flag_the_import_task_is_watching() {
    // The button and `ids::IMPORT_CANCEL` share one entry point; this proves
    // the button reaches the same flag the background task polls.
    dat0_core::import_progress::clear_active();
    let active = dat0_core::import_progress::ImportProgress::new(1024);
    dat0_core::import_progress::set_active(active.clone());

    let mut h = progress(ImportState::Running {
        file: PathBuf::from("/tmp/sales.csv"),
        done: 0,
        total: 1024,
    });
    assert!(!active.cancel.load(Ordering::SeqCst));

    h.click("import-cancel");

    assert!(
        active.cancel.load(Ordering::SeqCst),
        "the in-flight import was never told to stop"
    );
    assert!(
        dat0_core::import_progress::active().is_none(),
        "the active slot must be cleared, or the next import cannot claim it"
    );
    assert_eq!(
        read(&h, "t-cancelled"),
        "true",
        "the host is told too, so it can clear its own state"
    );
    dat0_core::import_progress::clear_active();
}

// ─────────────────────────────────────────────────────────────────────────────
// import wizard
// ─────────────────────────────────────────────────────────────────────────────

fn sniff(encoding_supported: bool) -> SniffSummary {
    SniffSummary {
        top_delimiter: ',',
        top_score: 0.55,
        next_score: 0.53,
        encoding_supported,
        any_low_confidence_column: false,
    }
}

/// The one part of the wizard that talks to DuckDB. It runs a prepared
/// `read_csv(?, delim := ?, …)`, and named table-function arguments taking
/// bound parameters is exactly the sort of thing that is fine until it isn't.
#[test]
fn describe_csv_reads_the_columns_under_the_chosen_dialect() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("semi.csv");
    std::fs::write(&path, "id;name\n1;alpha\n2;beta\n").unwrap();

    let cols = dat0_ui::components::import_wizard::describe_csv(&path, ";", "\"", true)
        .expect("DESCRIBE over a semicolon CSV");
    let names: Vec<&str> = cols.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["id", "name"]);
    assert_eq!(cols[0].1, "BIGINT", "the id column sniffs as an integer");

    // The wrong delimiter yields one column holding the whole line, which is
    // precisely the confusion the wizard exists to let the user fix.
    let wrong = dat0_ui::components::import_wizard::describe_csv(&path, ",", "\"", true)
        .expect("DESCRIBE with the wrong delimiter still parses");
    assert_eq!(wrong.len(), 1);
}

fn wizard_model() -> WizardModel {
    WizardModel::from_sniff(
        std::path::Path::new("/tmp/ambiguous.csv"),
        &sniff(true),
        vec![
            ("id".into(), "BIGINT".into()),
            ("name".into(), "VARCHAR".into()),
            ("score".into(), "DOUBLE".into()),
        ],
    )
}

#[derive(Clone, PartialEq, Props)]
struct WizardHostProps {
    model: WizardModel,
}

#[component]
fn WizardHost(props: WizardHostProps) -> Element {
    let model = use_signal(|| props.model.clone());
    let mut imported = use_signal(String::new);
    rsx! {
        ImportWizard {
            model,
            on_import: move |m: WizardModel| imported.set(m.read_csv_sql().unwrap_or_default()),
            on_cancel: move |_| {},
        }
        div { "data-a11y-id": "t-step", "{model.read().step.id()}" }
        div { "data-a11y-id": "t-imported", "{imported}" }
    }
}

fn wizard(model: WizardModel) -> Harness {
    Harness::new(WizardHost, WizardHostProps { model })
}

#[test]
fn the_wizard_opens_on_the_dialect_step() {
    let h = wizard(wizard_model());
    assert_eq!(read(&h, "t-step"), "dialect");
    assert!(h.by_a11y_id("wizard-import").is_none(), "import is last");
}

#[test]
fn an_empty_delimiter_blocks_the_step_and_says_why() {
    let mut h = wizard(wizard_model());
    let field = h.by_a11y_id("wizard-delimiter").unwrap();
    h.dispatch(field, "input", form(""));

    let next = h.by_a11y_id("wizard-next").unwrap();
    assert_eq!(h.attr(next, "aria-disabled").as_deref(), Some("true"));
    assert!(h.by_a11y_id("wizard-issues").is_some());

    h.click("wizard-next");
    assert_eq!(
        read(&h, "t-step"),
        "dialect",
        "a blocked step must not advance"
    );
}

#[test]
fn a_multi_character_delimiter_is_rejected() {
    let mut m = wizard_model();
    m.delimiter = "||".into();
    assert_eq!(m.issues(Step::Dialect), vec![Issue::DelimiterNotSingleChar]);
    assert!(!m.can_advance());
}

#[test]
fn the_delimiter_and_the_quote_may_not_be_the_same_character() {
    let mut m = wizard_model();
    m.delimiter = "\"".into();
    assert!(
        m.issues(Step::Dialect)
            .contains(&Issue::DelimiterEqualsQuote)
    );
}

#[test]
fn a_file_that_is_not_utf8_cannot_be_advanced_past_the_dialect_step() {
    // DuckDB's CSV reader errors hard on non-UTF-8. Letting the user fill in
    // three steps first buys them a guaranteed failure at the end.
    let m = WizardModel::from_sniff(
        std::path::Path::new("/tmp/latin1.csv"),
        &sniff(false),
        vec![("a".into(), "VARCHAR".into())],
    );
    assert!(
        m.issues(Step::Dialect)
            .contains(&Issue::EncodingUnsupported)
    );
    assert!(!m.can_advance());
}

#[test]
fn a_clean_dialect_advances_to_the_columns_step() {
    let mut h = wizard(wizard_model());
    h.click("wizard-next");
    assert_eq!(read(&h, "t-step"), "columns");
    assert!(h.by_a11y_id("wizard-columns").is_some());
}

#[test]
fn deselecting_every_column_blocks_the_step() {
    let mut h = wizard(wizard_model());
    h.click("wizard-next");
    for i in 0..3 {
        let cb = h.by_a11y_id(&format!("wizard-include-{i}")).unwrap();
        h.dispatch(cb, "change", form("false"));
    }
    assert_eq!(
        h.attr(h.by_a11y_id("wizard-next").unwrap(), "aria-disabled")
            .as_deref(),
        Some("true")
    );
    h.click("wizard-next");
    assert_eq!(read(&h, "t-step"), "columns");
}

#[test]
fn an_excluded_column_is_not_validated() {
    // You cannot be wrong about a column you are not importing.
    let mut m = wizard_model();
    m.columns[2].name = String::new();
    assert!(!m.issues(Step::Columns).is_empty());
    m.columns[2].include = false;
    assert!(m.issues(Step::Columns).is_empty());
}

#[test]
fn two_columns_may_not_share_a_name_even_in_a_different_case() {
    // DuckDB identifiers are case-insensitive: `ID` and `id` are one column,
    // and the second silently wins.
    let mut m = wizard_model();
    m.columns[1].name = "ID".into();
    let issues = m.issues(Step::Columns);
    assert!(
        issues.iter().any(|i| matches!(
            i,
            Issue::DuplicateColumnName { row, name } if *row == 1 && name == "ID"
        )),
        "{issues:?}"
    );
}

#[test]
fn a_column_may_not_take_the_engines_surrogate_name() {
    let mut m = wizard_model();
    m.columns[0].name = ROWID_COL.to_uppercase();
    assert!(
        m.issues(Step::Columns)
            .contains(&Issue::ReservedColumnName { row: 0 })
    );
}

#[test]
fn a_blank_name_is_reported_against_its_own_row() {
    let mut m = wizard_model();
    m.columns[1].name = "   ".into();
    assert!(
        m.issues(Step::Columns)
            .contains(&Issue::EmptyColumnName { row: 1 })
    );
}

#[test]
fn a_type_outside_the_vocabulary_blocks_the_step() {
    let mut m = wizard_model();
    m.columns[0].ty = "STRUCT(a INT)".into();
    let issues = m.issues(Step::Columns);
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, Issue::UnknownType { row, .. } if *row == 0)),
        "{issues:?}"
    );
    // …and every type the form offers does not.
    for t in TYPES {
        let mut ok = wizard_model();
        ok.columns[0].ty = t.to_string();
        assert!(ok.issues(Step::Columns).is_empty(), "{t} was rejected");
    }
}

#[test]
fn editing_a_column_name_through_the_form_reaches_the_model() {
    let mut h = wizard(wizard_model());
    h.click("wizard-next");
    let field = h.by_a11y_id("wizard-name-1").unwrap();
    h.dispatch(field, "input", form("full_name"));
    h.click("wizard-next");
    assert_eq!(read(&h, "t-step"), "confirm");
    let sql = h.text_of(h.by_a11y_id("wizard-sql").expect("the preview"));
    assert!(sql.contains("\"name\" AS \"full_name\""), "{sql}");
}

#[test]
fn import_is_only_offered_on_the_last_step_and_only_when_every_step_is_clean() {
    let mut m = wizard_model();
    assert!(!m.can_import(), "not from the dialect step");
    m.step = Step::Confirm;
    assert!(m.can_import());

    // A late edit that breaks step 1 must disarm Import on step 3 — the gate
    // is every step, not the one you are looking at.
    m.delimiter = ";;".into();
    assert!(!m.can_import());
    assert!(
        m.issues(Step::Confirm).is_empty(),
        "confirm owns no rules of its own"
    );
    assert!(!m.all_issues().is_empty());
}

#[test]
fn confirming_hands_back_runnable_sql() {
    let mut h = wizard(wizard_model());
    h.click("wizard-next");
    h.click("wizard-next");
    assert_eq!(read(&h, "t-step"), "confirm");
    h.click("wizard-import");

    let sql = read(&h, "t-imported");
    assert!(sql.starts_with("SELECT "), "{sql}");
    assert!(sql.contains("read_csv('/tmp/ambiguous.csv'"), "{sql}");
    assert!(sql.contains("delim = ','"), "{sql}");
    assert!(sql.contains("header = true"), "{sql}");
    assert!(sql.contains("'id': 'BIGINT'"), "{sql}");
}

#[test]
fn a_disarmed_import_button_does_not_import_when_clicked_anyway() {
    // The button is `aria-disabled`, not `disabled`, so it stays focusable —
    // which means it also stays clickable, and the guard must be re-checked.
    let mut m = wizard_model();
    m.step = Step::Confirm;
    m.columns[0].name = String::new();
    let mut h = wizard(m);
    h.click("wizard-import");
    assert_eq!(
        read(&h, "t-imported"),
        "",
        "an invalid wizard must not produce SQL"
    );
}

#[test]
fn going_back_is_always_possible_from_a_later_step() {
    let mut h = wizard(wizard_model());
    h.click("wizard-next");
    assert_eq!(read(&h, "t-step"), "columns");
    // Break the step, then leave it: a wizard you cannot retreat from is a
    // trap.
    let field = h.by_a11y_id("wizard-name-0").unwrap();
    h.dispatch(field, "input", form(""));
    h.click("wizard-back");
    assert_eq!(read(&h, "t-step"), "dialect");
}

#[test]
fn back_does_nothing_on_the_first_step() {
    let mut m = wizard_model();
    assert!(!m.can_go_back());
    assert!(!m.go_back());
    assert_eq!(m.step, Step::Dialect);
}

#[test]
fn a_new_column_draft_defaults_to_included_and_named_after_the_file() {
    let c = ColumnDraft::new("total_amount", "DOUBLE");
    assert!(c.include);
    assert_eq!(c.name, "total_amount");
    assert_eq!(c.source, "total_amount");
    assert_eq!(c.ty, "DOUBLE");
}

// ─────────────────────────────────────────────────────────────────────────────
// AI panel
// ─────────────────────────────────────────────────────────────────────────────

/// A probe the test drives by hand.
///
/// Each call takes the next queued channel; the test decides when — and
/// whether — it resolves. That is what makes "a slow answer must never
/// overwrite a newer one" a test rather than a race.
#[derive(Default)]
struct ScriptedProbe {
    tests: RefCell<VecDeque<oneshot::Receiver<(bool, String)>>>,
    #[allow(clippy::type_complexity)]
    streams: RefCell<VecDeque<oneshot::Receiver<(Vec<String>, Option<String>)>>>,
}

impl ScriptedProbe {
    fn queue_test(&self) -> oneshot::Sender<(bool, String)> {
        let (tx, rx) = oneshot::channel();
        self.tests.borrow_mut().push_back(rx);
        tx
    }

    #[allow(clippy::type_complexity)]
    fn queue_stream(&self) -> oneshot::Sender<(Vec<String>, Option<String>)> {
        let (tx, rx) = oneshot::channel();
        self.streams.borrow_mut().push_back(rx);
        tx
    }
}

impl AiProbe for ScriptedProbe {
    fn test_connection(
        &self,
        _provider: Provider,
        _key: String,
        _cfg: AiSettings,
    ) -> AiFuture<TestOutcome> {
        let rx = self.tests.borrow_mut().pop_front();
        Box::pin(async move {
            match rx {
                Some(rx) => match rx.await {
                    Ok((ok, message)) => TestOutcome { ok, message },
                    Err(_) => TestOutcome {
                        ok: false,
                        message: "dropped".into(),
                    },
                },
                None => TestOutcome {
                    ok: false,
                    message: "unscripted".into(),
                },
            }
        })
    }

    fn stream(
        &self,
        _provider: Provider,
        _key: String,
        _cfg: AiSettings,
        _req: AiRequest,
        mut on_delta: Box<dyn FnMut(String)>,
    ) -> AiFuture<Result<String, String>> {
        let rx = self.streams.borrow_mut().pop_front();
        Box::pin(async move {
            let Some(rx) = rx else {
                return Err("unscripted".into());
            };
            match rx.await {
                Ok((deltas, err)) => {
                    let mut full = String::new();
                    for d in deltas {
                        full.push_str(&d);
                        on_delta(d);
                    }
                    match err {
                        Some(e) => Err(e),
                        None => Ok(full),
                    }
                }
                Err(_) => Err("dropped".into()),
            }
        })
    }
}

#[derive(Clone, PartialEq, Props)]
struct AiHostProps {
    deps: AiDeps,
}

/// The panel, plus a readback of the state a DOM query cannot see.
#[component]
fn AiHost(props: AiHostProps) -> Element {
    let ctl = AiController::use_new(props.deps.clone());
    let stream_ctl = ctl.clone();
    let key_ctl = ctl.clone();
    let phase = ctl.state.stream.read().phase;
    let test_id = (ctl.state.test_load_id)();
    let stream_id = (ctl.state.stream_load_id)();
    let entry = ctl.state.entry.read().is_some();
    let ready = ctl.ready();
    rsx! {
        AiPanel { controller: ctl.clone() }
        button {
            "data-a11y-id": "t-stream",
            onclick: move |_| {
                stream_ctl.spawn_stream(StreamKind::NlToSql, "count the rows".into(), &[]);
            },
            "stream"
        }
        button {
            "data-a11y-id": "t-set-key",
            onclick: move |_| {
                key_ctl.handle(AiPanelEvent::SetKey("sk-live-hunter2".into()));
            },
            "set key"
        }
        div { "data-a11y-id": "t-test-id", "{test_id}" }
        div { "data-a11y-id": "t-stream-id", "{stream_id}" }
        div { "data-a11y-id": "t-phase", "{phase.id()}" }
        div { "data-a11y-id": "t-entry", "{entry}" }
        div { "data-a11y-id": "t-ready", "{ready}" }
    }
}

/// A configured panel: OpenRouter selected, a key in the store, a model set.
fn ai() -> (Harness, Arc<ScriptedProbe>, Arc<SettingsStore>) {
    let store = Arc::new(SettingsStore::open_in_memory());
    let mut s = Settings::default();
    s.ai.enabled = true;
    s.ai.provider = Some("openrouter".into());
    s.ai.model = "vendor/model".into();
    // Already acked, so a toggle does not push a banner mid-assertion.
    s.ai.privacy_ack = true;
    store.save(&s).unwrap();

    let keys = Arc::new(MemoryKeyStore::default());
    keys.set(Provider::OpenRouter, "sk-or-seeded").unwrap();

    let probe = Arc::new(ScriptedProbe::default());
    let deps = AiDeps {
        store: Arc::clone(&store),
        keys,
        probe: Arc::clone(&probe) as Arc<dyn AiProbe>,
    };
    (Harness::new(AiHost, AiHostProps { deps }), probe, store)
}

#[test]
fn the_panel_hydrates_from_settings_and_the_keychain() {
    let (h, _probe, _store) = ai();
    assert!(h.has_label_contains("OpenRouter"));
    assert_eq!(read(&h, "ai-key-state"), dat0_i18n::t("ai.key.set"));
    assert!(read(&h, "ai-model-state").contains("vendor/model"));
    assert_eq!(read(&h, "t-ready"), "true");
}

#[test]
fn the_egress_figure_is_always_on_screen() {
    // "0 bytes left this machine" is a claim; a hidden counter cannot make it.
    let (h, _probe, _store) = ai();
    let line = read(&h, "ai-egress");
    assert!(line.starts_with(&dat0_i18n::t("ai.egress")), "{line}");
}

#[test]
fn a_stale_test_result_never_overwrites_a_newer_configuration() {
    let (mut h, probe, _store) = ai();
    let gate = probe.queue_test();

    h.click("ai-test-connection");
    let issued: u64 = read(&h, "t-test-id").parse().unwrap();
    assert!(h.by_a11y_id("ai-test-result").is_none(), "still in flight");

    // The user changes provider while the probe is out.
    h.click("ai-provider-cycle");
    let now: u64 = read(&h, "t-test-id").parse().unwrap();
    assert_ne!(issued, now, "a config change must invalidate the probe");

    // The old probe finally answers — about a provider that is no longer set.
    gate.send((true, "Connected".into())).unwrap();
    h.settle();

    assert!(
        h.by_a11y_id("ai-test-result").is_none(),
        "a result about the previous configuration was shown as if it were current"
    );
}

#[test]
fn a_result_that_is_still_current_is_shown() {
    let (mut h, probe, _store) = ai();
    let gate = probe.queue_test();
    h.click("ai-test-connection");
    gate.send((true, "Connected".into())).unwrap();
    h.settle();
    assert_eq!(read(&h, "ai-test-result"), "✓ Connected");
}

#[test]
fn a_failed_probe_reports_the_reason() {
    let (mut h, probe, _store) = ai();
    let gate = probe.queue_test();
    h.click("ai-test-connection");
    gate.send((false, "401 unauthorized".into())).unwrap();
    h.settle();
    assert_eq!(read(&h, "ai-test-result"), "✗ 401 unauthorized");
}

#[test]
fn every_configuration_action_clears_a_stale_message() {
    let (mut h, probe, _store) = ai();
    let gate = probe.queue_test();
    h.click("ai-test-connection");
    gate.send((true, "Connected".into())).unwrap();
    h.settle();
    assert!(h.by_a11y_id("ai-test-result").is_some());

    h.click("ai-toggle-sample-rows");
    assert!(
        h.by_a11y_id("ai-test-result").is_none(),
        "the message describes a configuration that no longer exists"
    );
}

#[test]
fn testing_without_a_key_says_so_instead_of_calling_out() {
    let store = Arc::new(SettingsStore::open_in_memory());
    let mut s = Settings::default();
    s.ai.provider = Some("anthropic".into());
    s.ai.privacy_ack = true;
    store.save(&s).unwrap();
    let probe = Arc::new(ScriptedProbe::default());
    let deps = AiDeps {
        store,
        keys: Arc::new(MemoryKeyStore::default()),
        probe: Arc::clone(&probe) as Arc<dyn AiProbe>,
    };
    let mut h = Harness::new(AiHost, AiHostProps { deps });

    h.click("ai-test-connection");
    assert_eq!(
        read(&h, "ai-test-result"),
        format!("✗ {}", dat0_i18n::t("ai.test.no_key"))
    );
    assert!(
        probe.tests.borrow().is_empty() && probe.streams.borrow().is_empty(),
        "nothing was queued, so nothing should have been asked for"
    );
}

#[test]
fn testing_without_a_provider_says_so() {
    let store = Arc::new(SettingsStore::open_in_memory());
    let mut s = Settings::default();
    s.ai.privacy_ack = true;
    store.save(&s).unwrap();
    let deps = AiDeps {
        store,
        keys: Arc::new(MemoryKeyStore::default()),
        probe: Arc::new(ScriptedProbe::default()),
    };
    let mut h = Harness::new(AiHost, AiHostProps { deps });
    h.click("ai-test-connection");
    assert_eq!(
        read(&h, "ai-test-result"),
        format!("✗ {}", dat0_i18n::t("ai.test.no_provider"))
    );
}

#[test]
fn the_api_key_is_never_echoed_back_into_the_panel() {
    let (mut h, _probe, store) = ai();
    h.click("t-set-key");
    assert_eq!(read(&h, "ai-key-state"), dat0_i18n::t("ai.key.set"));
    assert!(
        !h.html().contains("hunter2"),
        "the key reached the rendered DOM"
    );
    let persisted = format!("{:?}", store.load_or_default().unwrap());
    assert!(
        !persisted.contains("hunter2"),
        "the key reached settings.toml"
    );
}

#[test]
fn asking_to_set_a_key_raises_the_entry_outbox_rather_than_writing_one() {
    let (mut h, _probe, _store) = ai();
    assert_eq!(read(&h, "t-entry"), "false");
    h.click("ai-key-set");
    assert_eq!(
        read(&h, "t-entry"),
        "true",
        "the host must be asked to open its prompt"
    );
}

#[test]
fn forgetting_a_key_leaves_the_button_where_the_users_focus_is() {
    // Upstream unmounted this button on click and then had to move focus by
    // hand. It stays mounted and merely `aria-disabled`, so focus is never
    // left on an element that stopped existing.
    let (mut h, _probe, _store) = ai();
    h.click("ai-key-forget");
    assert_eq!(read(&h, "ai-key-state"), dat0_i18n::t("ai.key.unset"));
    let forget = h
        .by_a11y_id("ai-key-forget")
        .expect("the button must survive its own click");
    assert_eq!(h.attr(forget, "aria-disabled").as_deref(), Some("true"));
    assert_eq!(read(&h, "t-ready"), "false");
}

#[test]
fn a_stream_moves_from_streaming_to_done_and_accumulates_its_deltas() {
    let (mut h, probe, _store) = ai();
    let gate = probe.queue_stream();

    h.click("t-stream");
    assert_eq!(read(&h, "t-phase"), StreamPhase::Streaming.id());

    gate.send((
        vec!["SELECT ".into(), "count(*) ".into(), "FROM t".into()],
        None,
    ))
    .unwrap();
    h.settle();

    assert_eq!(read(&h, "t-phase"), StreamPhase::Done.id());
    assert_eq!(read(&h, "ai-stream-text"), "SELECT count(*) FROM t");
    assert!(h.by_a11y_id("ai-stream-error").is_none());
}

#[test]
fn a_stream_that_fails_ends_in_the_failed_phase_with_the_reason() {
    let (mut h, probe, _store) = ai();
    let gate = probe.queue_stream();
    h.click("t-stream");
    gate.send((vec!["SEL".into()], Some("stream closed".into())))
        .unwrap();
    h.settle();

    assert_eq!(read(&h, "t-phase"), StreamPhase::Failed.id());
    assert_eq!(read(&h, "ai-stream-error"), "stream closed");
    assert_eq!(
        read(&h, "ai-stream-text"),
        "SEL",
        "the partial answer stays; deleting it hides what went wrong"
    );
}

#[test]
fn deltas_from_a_superseded_stream_are_dropped() {
    let (mut h, probe, _store) = ai();
    let first = probe.queue_stream();
    let second = probe.queue_stream();

    h.click("t-stream");
    let issued: u64 = read(&h, "t-stream-id").parse().unwrap();
    h.click("t-stream");
    let now: u64 = read(&h, "t-stream-id").parse().unwrap();
    assert_ne!(issued, now, "a second stream must supersede the first");

    // The abandoned request answers late, in full.
    first
        .send((vec!["STALE".into()], Some("stale failure".into())))
        .unwrap();
    h.settle();
    assert_eq!(read(&h, "t-phase"), StreamPhase::Streaming.id());
    assert_eq!(read(&h, "ai-stream-text"), "");

    second.send((vec!["FRESH".into()], None)).unwrap();
    h.settle();
    assert_eq!(read(&h, "ai-stream-text"), "FRESH");
    assert_eq!(read(&h, "t-phase"), StreamPhase::Done.id());
}

#[test]
fn a_stream_needs_a_model_before_it_will_build_a_request() {
    let store = Arc::new(SettingsStore::open_in_memory());
    let mut s = Settings::default();
    s.ai.provider = Some("openrouter".into());
    s.ai.model = String::new();
    s.ai.privacy_ack = true;
    store.save(&s).unwrap();
    let keys = Arc::new(MemoryKeyStore::default());
    keys.set(Provider::OpenRouter, "sk-or-seeded").unwrap();
    let probe = Arc::new(ScriptedProbe::default());
    let deps = AiDeps {
        store,
        keys,
        probe: Arc::clone(&probe) as Arc<dyn AiProbe>,
    };
    let mut h = Harness::new(AiHost, AiHostProps { deps });

    h.click("t-stream");
    assert_eq!(read(&h, "t-phase"), StreamPhase::Idle.id());
}
