//! The five dialog/panel surfaces: export, name prompt, saved queries, query
//! history, connections.
//!
//! Each of these was a GPUI entity whose rules lived partly in the widget and
//! partly in `WorkspaceShell`. What is asserted here is those rules — the
//! gates, the rejections, the supersede guard — not the markup they happen to
//! be wearing.

mod support;

use dioxus::prelude::*;
use uuid::Uuid;

use dat0_core::connections::ConnectionStatus;
use dat0_core::connections::token_store::{MemoryTokenStore, TokenStore as _};
use dat0_core::session::queries::{HistoryEntry, SavedQuery};

use dat0_ui::components::connections::{
    Connections, ConnectionsEvent, ConnectionsPanel, Outcome, route, status_dot_class,
};
use dat0_ui::components::export_dialog::{ExportDialog, ExportRequest};
use dat0_ui::components::name_prompt::NamePrompt;
use dat0_ui::components::query_library::QueryLibrary;
use dat0_ui::components::saved_queries::SavedQueriesPicker;

use support::{Harness, Key, Modifiers};

/// Type into a field.
fn form(value: &str) -> dioxus::html::SerializedFormData {
    dioxus::html::SerializedFormData::new(value.to_string(), Vec::new())
}

/// What a surface reported, read back out of the tree — the harness sees text,
/// not Rust state.
fn captured(h: &Harness) -> String {
    h.text_of(
        h.by_a11y_id("captured")
            .expect("the host renders a readback"),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Export dialog
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct ExportHostProps {
    with_destination: bool,
}

#[component]
fn ExportHost(props: ExportHostProps) -> Element {
    let mut log = use_signal(Vec::<String>::new);
    let destination = props
        .with_destination
        .then(|| std::path::PathBuf::from("/tmp/out"));
    rsx! {
        ExportDialog {
            destination,
            on_browse: move |_| log.write().push("browse".into()),
            on_export: move |r: ExportRequest| {
                log.write()
                    .push(format!("export {:?} {:?} {}", r.scope, r.format, r.path.display()));
            },
            on_cancel: move |_| log.write().push("cancel".into()),
        }
        div { "data-a11y-id": "captured", "{log.read().join(\"|\")}" }
    }
}

fn export(with_destination: bool) -> Harness {
    Harness::new(ExportHost, ExportHostProps { with_destination })
}

#[test]
fn the_export_file_name_carries_the_selected_format() {
    // GPUI derived `export.{ext}` inside `run_export`, where the format was
    // already chosen; here the extension is shown as it is chosen, and the two
    // must not be able to disagree.
    let mut h = export(true);
    assert_eq!(h.text_of(h.by_a11y_id("export-extension").unwrap()), ".csv");

    h.click("export-format-parquet");
    assert_eq!(
        h.text_of(h.by_a11y_id("export-extension").unwrap()),
        ".parquet"
    );

    h.click("export-run");
    assert_eq!(
        captured(&h),
        "export CurrentView Parquet /tmp/out/export.parquet"
    );
}

#[test]
fn export_is_refused_until_a_destination_exists() {
    // The GPUI dialog could always be confirmed because the save panel supplied
    // the directory afterwards. Here the directory arrives first, so pressing
    // Export before it does must produce nothing at all.
    let mut h = export(false);
    let run = h.by_a11y_id("export-run").unwrap();
    assert_eq!(h.attr(run, "disabled").as_deref(), Some("true"));

    h.click("export-run");
    assert_eq!(captured(&h), "", "no destination, no export");
}

#[test]
fn an_empty_file_name_blocks_the_export_and_says_why() {
    let mut h = export(true);
    let field = h.by_a11y_id("export-name").unwrap();
    h.dispatch(field, "input", form("   "));

    assert!(
        h.by_a11y_id("export-name-error").is_some(),
        "the reason has to be on screen; GPUI's silent no-op is the bug"
    );
    let run = h.by_a11y_id("export-run").unwrap();
    assert_eq!(h.attr(run, "disabled").as_deref(), Some("true"));

    h.click("export-run");
    assert_eq!(captured(&h), "");
}

#[test]
fn a_path_separator_in_the_file_name_is_refused() {
    // A save-panel name is a leaf. Accepting `../../etc/x` would write
    // somewhere the destination row does not name.
    let mut h = export(true);
    let field = h.by_a11y_id("export-name").unwrap();
    h.dispatch(field, "input", form("../escape"));

    assert!(h.by_a11y_id("export-name-error").is_some());
    h.click("export-run");
    assert_eq!(captured(&h), "");

    // And recovers the moment the name is legal again.
    h.dispatch(field, "input", form("orders"));
    assert!(h.by_a11y_id("export-name-error").is_none());
    h.click("export-run");
    assert_eq!(captured(&h), "export CurrentView Csv /tmp/out/orders.csv");
}

#[test]
fn the_format_radio_group_wraps_on_arrow_keys() {
    // Radio groups wrap (WAI-ARIA); the list surfaces below clamp. A
    // three-item group that dead-ends is worse than one that cycles.
    let mut h = export(true);
    h.key_at("export-format-group", Key::ArrowLeft, Modifiers::empty());
    assert_eq!(
        h.text_of(h.by_a11y_id("export-extension").unwrap()),
        ".parquet",
        "left from the first format wraps to the last"
    );
    h.key_at("export-format-group", Key::ArrowRight, Modifiers::empty());
    assert_eq!(h.text_of(h.by_a11y_id("export-extension").unwrap()), ".csv");
}

#[test]
fn the_scope_radio_reaches_the_request() {
    let mut h = export(true);
    h.key_at("export-scope-group", Key::ArrowDown, Modifiers::empty());
    h.click("export-run");
    assert_eq!(captured(&h), "export FullTable Csv /tmp/out/export.csv");
}

#[test]
fn browse_and_cancel_are_the_callers_business() {
    let mut h = export(true);
    h.click("export-browse");
    h.click("export-cancel");
    assert_eq!(captured(&h), "browse|cancel");
}

// ─────────────────────────────────────────────────────────────────────────────
// Name prompt
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct PromptHostProps {
    initial: String,
}

#[component]
fn PromptHost(props: PromptHostProps) -> Element {
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        NamePrompt {
            title: "Save query as…".to_string(),
            initial: props.initial.clone(),
            on_confirm: move |v: String| log.write().push(format!("confirm[{v}]")),
            on_cancel: move |_| log.write().push("cancel".into()),
        }
        div { "data-a11y-id": "captured", "{log.read().join(\"|\")}" }
    }
}

fn prompt(initial: &str) -> Harness {
    Harness::new(
        PromptHost,
        PromptHostProps {
            initial: initial.to_string(),
        },
    )
}

#[test]
fn an_empty_prompt_cannot_be_confirmed() {
    // `save_named_query` opens with `if name.trim().is_empty() { return; }`,
    // so GPUI's Save closed the modal and dropped the value. Same rule, but
    // the button now refuses instead of lying.
    let mut h = prompt("");
    let ok = h.by_a11y_id("name-prompt-ok").unwrap();
    assert_eq!(h.attr(ok, "disabled").as_deref(), Some("true"));

    h.click("name-prompt-ok");
    assert_eq!(captured(&h), "");
}

#[test]
fn whitespace_is_rejected_the_same_way_empty_is() {
    let mut h = prompt("");
    let field = h.by_a11y_id("name-prompt-field").unwrap();
    h.dispatch(field, "input", form("    "));

    assert!(h.by_a11y_id("name-prompt-error").is_some());
    h.key(field, Key::Enter, Modifiers::empty());
    assert_eq!(
        captured(&h),
        "",
        "Enter is inert on a value nobody will save"
    );
}

#[test]
fn a_pasted_newline_is_rejected() {
    // The GPUI field was a single-line `InputState` and structurally could not
    // hold one; an `<input>` can, so the constraint became a check.
    let mut h = prompt("");
    let field = h.by_a11y_id("name-prompt-field").unwrap();
    h.dispatch(field, "input", form("q2\nDROP TABLE t"));

    assert!(h.by_a11y_id("name-prompt-error").is_some());
    h.click("name-prompt-ok");
    assert_eq!(captured(&h), "");
}

#[test]
fn enter_submits_the_value_exactly_as_typed() {
    // Untrimmed: `save_named_query` trims for itself, and the token prompt
    // must not have its payload rewritten by a text field.
    let mut h = prompt("");
    let field = h.by_a11y_id("name-prompt-field").unwrap();
    h.dispatch(field, "input", form(" q2 revenue "));
    h.key(field, Key::Enter, Modifiers::empty());

    assert_eq!(captured(&h), "confirm[ q2 revenue ]");
}

#[test]
fn a_seeded_prompt_is_confirmable_immediately() {
    // The chart-save flow seeds a default name; requiring an edit first would
    // make the seed pointless.
    let mut h = prompt("revenue by month");
    h.click("name-prompt-ok");
    assert_eq!(captured(&h), "confirm[revenue by month]");
}

#[test]
fn cancel_reports_a_cancel() {
    let mut h = prompt("x");
    h.click("name-prompt-cancel");
    assert_eq!(captured(&h), "cancel");
}

// ─────────────────────────────────────────────────────────────────────────────
// Saved-query picker
// ─────────────────────────────────────────────────────────────────────────────

fn saved(n: usize) -> Vec<SavedQuery> {
    (0..n)
        .map(|i| SavedQuery {
            // Deterministic ids so an assertion can name one.
            id: Uuid::from_u128(i as u128 + 1),
            name: format!("query {i}"),
            sql: format!("SELECT {i}"),
            saved_at: i as i64,
        })
        .collect()
}

#[derive(Clone, PartialEq, Props)]
struct PickerHostProps {
    queries: Vec<SavedQuery>,
}

#[component]
fn PickerHost(props: PickerHostProps) -> Element {
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        SavedQueriesPicker {
            queries: props.queries.clone(),
            on_pick: move |q: SavedQuery| log.write().push(format!("pick[{}|{}]", q.id, q.sql)),
            on_delete: move |id: Uuid| log.write().push(format!("delete[{id}]")),
        }
        div { "data-a11y-id": "captured", "{log.read().join(\"|\")}" }
    }
}

fn picker(n: usize) -> Harness {
    Harness::new(PickerHost, PickerHostProps { queries: saved(n) })
}

#[test]
fn clicking_a_saved_query_picks_that_one() {
    let mut h = picker(3);
    h.click("saved-row-1");
    assert_eq!(
        captured(&h),
        format!("pick[{}|SELECT 1]", Uuid::from_u128(2))
    );
}

#[test]
fn arrows_move_the_active_row_and_enter_picks_it() {
    // The LISTBOX pattern: one tab stop, arrows inside. Never a focus stop per
    // row — a hundred saved queries must not cost a hundred Tabs.
    let mut h = picker(3);
    h.key_at("sql-saved-list", Key::ArrowDown, Modifiers::empty());
    h.key_at("sql-saved-list", Key::ArrowDown, Modifiers::empty());
    h.key_at("sql-saved-list", Key::Enter, Modifiers::empty());
    assert_eq!(
        captured(&h),
        format!("pick[{}|SELECT 2]", Uuid::from_u128(3))
    );
}

#[test]
fn arrows_clamp_rather_than_wrap() {
    // Only radio groups wrap. Running off the end of a list and reappearing at
    // the top is how a keyboard user loses their place.
    let mut h = picker(2);
    for _ in 0..5 {
        h.key_at("sql-saved-list", Key::ArrowDown, Modifiers::empty());
    }
    h.key_at("sql-saved-list", Key::Enter, Modifiers::empty());
    assert_eq!(
        captured(&h),
        format!("pick[{}|SELECT 1]", Uuid::from_u128(2)),
        "down stops at the last row"
    );
}

#[test]
fn delete_removes_the_active_row_by_id() {
    let mut h = picker(3);
    h.key_at("sql-saved-list", Key::ArrowDown, Modifiers::empty());
    h.key_at("sql-saved-list", Key::Delete, Modifiers::empty());
    assert_eq!(captured(&h), format!("delete[{}]", Uuid::from_u128(2)));
}

#[test]
fn the_row_delete_button_does_not_also_load_the_query() {
    // A DOM click bubbles, so without `stop_propagation` the row's own handler
    // fires too and the query is loaded on its way to being deleted.
    let mut h = picker(3);
    h.click("saved-del-2");
    assert_eq!(captured(&h), format!("delete[{}]", Uuid::from_u128(3)));
}

#[test]
fn an_empty_picker_has_nothing_to_pick() {
    let mut h = picker(0);
    assert!(h.by_a11y_id("saved-empty").is_some());
    h.key_at("sql-saved-list", Key::Enter, Modifiers::empty());
    h.key_at("sql-saved-list", Key::Delete, Modifiers::empty());
    assert_eq!(captured(&h), "", "no row, no event, no panic");
}

// ─────────────────────────────────────────────────────────────────────────────
// Query history
// ─────────────────────────────────────────────────────────────────────────────

fn history() -> Vec<HistoryEntry> {
    // Stored oldest-first, as `queries::push_history` appends.
    vec![
        HistoryEntry {
            sql: "SELECT 1".into(),
            ran_at: 1,
            ok: true,
            elapsed_ms: 3,
        },
        HistoryEntry {
            sql: "SELECT oops".into(),
            ran_at: 2,
            ok: false,
            elapsed_ms: 11,
        },
        HistoryEntry {
            sql: format!("SELECT {}\nFROM t", "x".repeat(200)),
            ran_at: 3,
            ok: true,
            elapsed_ms: 7,
        },
    ]
}

#[component]
fn HistoryHost() -> Element {
    let mut log = use_signal(Vec::<String>::new);
    rsx! {
        QueryLibrary {
            entries: history(),
            on_pick: move |sql: String| log.write().push(format!("pick[{sql}]")),
        }
        div { "data-a11y-id": "captured", "{log.read().join(\"|\")}" }
    }
}

#[test]
fn history_is_newest_first() {
    // Stored oldest-first, shown newest-first — the GPUI overlay built a
    // reversed pick vector for exactly this, and an off-by-one here loads the
    // wrong query.
    let h = Harness::new(HistoryHost, ());
    let first = h.text_of(h.by_a11y_id("hist-row-0").unwrap());
    assert!(first.contains("SELECT xxx"), "got {first:?}");
    assert!(
        h.text_of(h.by_a11y_id("hist-row-2").unwrap())
            .contains("SELECT 1")
    );
}

#[test]
fn a_long_statement_is_previewed_to_one_truncated_line() {
    let h = Harness::new(HistoryHost, ());
    let row = h.text_of(h.by_a11y_id("hist-row-0").unwrap());
    assert!(row.contains('…'), "long previews are elided: {row:?}");
    assert!(
        !row.contains("FROM t"),
        "only the first line is previewed: {row:?}"
    );
}

#[test]
fn a_failed_run_is_marked_as_such() {
    let h = Harness::new(HistoryHost, ());
    let failed = h.text_of(h.by_a11y_id("hist-row-1").unwrap());
    assert!(failed.contains("err"), "got {failed:?}");
    assert!(failed.contains("11 ms"), "got {failed:?}");
}

#[test]
fn picking_a_history_row_yields_the_whole_statement_not_the_preview() {
    // The preview is truncated; loading the truncated text into a tab would
    // produce a syntax error and a very confusing bug report.
    let mut h = Harness::new(HistoryHost, ());
    h.click("hist-row-2");
    assert_eq!(captured(&h), "pick[SELECT 1]");
}

#[test]
fn history_arrows_clamp_and_enter_picks() {
    let mut h = Harness::new(HistoryHost, ());
    for _ in 0..9 {
        h.key_at("sql-history-list", Key::ArrowDown, Modifiers::empty());
    }
    h.key_at("sql-history-list", Key::Enter, Modifiers::empty());
    assert_eq!(captured(&h), "pick[SELECT 1]", "the oldest row is the last");
}

// ─────────────────────────────────────────────────────────────────────────────
// Connections — the supersede rule
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn a_slow_test_result_cannot_overwrite_a_newer_state() {
    // The bug this prevents: press Test, wait, press Disconnect, and the probe
    // lands afterwards with "Connected as md" on a panel the user just
    // disconnected. `ai_test_load_id` guards the AI probe the same way.
    let mut c = Connections::default();
    let store = MemoryTokenStore::default();
    store.set("tok").unwrap();

    let probe = match route(&mut c, ConnectionsEvent::TestMd, &store) {
        Outcome::Test { probe, .. } => probe,
        other => panic!("expected a probe, got {other:?}"),
    };
    assert_eq!(c.md_status(), &ConnectionStatus::Connecting);

    // The user gives up and disconnects while the ATTACH is still in flight.
    route(&mut c, ConnectionsEvent::DisconnectMd, &store);
    assert_eq!(c.md_status(), &ConnectionStatus::Disconnected);

    let applied = c.finish_probe(
        probe,
        ConnectionStatus::Connected,
        vec!["sample_data".into()],
        "Connection OK".into(),
    );
    assert!(!applied, "the stale result must be dropped");
    assert_eq!(c.md_status(), &ConnectionStatus::Disconnected);
    assert_eq!(c.md_test_result(), None);
    assert!(c.md_databases().is_empty());
}

#[test]
fn an_uncontested_test_result_is_applied_in_full() {
    // The guard must not be so eager that the normal path loses its result.
    let mut c = Connections::default();
    let store = MemoryTokenStore::default();
    store.set("tok").unwrap();

    let Outcome::Test { probe, token } = route(&mut c, ConnectionsEvent::TestMd, &store) else {
        panic!("expected a probe");
    };
    assert_eq!(token, "tok");

    assert!(c.finish_probe(
        probe,
        ConnectionStatus::Connected,
        vec!["sample_data".into()],
        "Connection OK".into(),
    ));
    assert_eq!(c.md_status(), &ConnectionStatus::Connected);
    assert_eq!(c.md_test_result(), Some("Connection OK"));
    assert_eq!(c.md_databases(), ["sample_data".to_string()]);
}

#[test]
fn the_older_of_two_overlapping_probes_loses() {
    let mut c = Connections::default();
    let store = MemoryTokenStore::default();
    store.set("tok").unwrap();

    let Outcome::Test { probe: first, .. } = route(&mut c, ConnectionsEvent::TestMd, &store) else {
        panic!("expected a probe");
    };
    let Outcome::Test { probe: second, .. } = route(&mut c, ConnectionsEvent::TestMd, &store)
    else {
        panic!("expected a probe");
    };

    // The second probe answers first — a perfectly ordinary race.
    assert!(c.finish_probe(second, ConnectionStatus::Connected, vec![], "ok".into()));
    assert!(
        !c.finish_probe(
            first,
            ConnectionStatus::Error("auth".into()),
            vec![],
            "failed".into()
        ),
        "the first probe's answer is older than what is on screen"
    );
    assert_eq!(c.md_status(), &ConnectionStatus::Connected);
    assert_eq!(c.md_test_result(), Some("ok"));
}

#[test]
fn leaving_connected_drops_the_cached_database_list() {
    // A disconnect must not leave a stale catalog on screen — the rule
    // `ConnectionManager::set_md_status` has always had.
    let mut c = Connections::default();
    let store = MemoryTokenStore::default();
    store.set("tok").unwrap();
    let Outcome::Test { probe, .. } = route(&mut c, ConnectionsEvent::TestMd, &store) else {
        panic!("expected a probe");
    };
    c.finish_probe(
        probe,
        ConnectionStatus::Connected,
        vec!["a".into()],
        "ok".into(),
    );
    assert_eq!(c.md_databases().len(), 1);

    route(&mut c, ConnectionsEvent::DisconnectMd, &store);
    assert!(c.md_databases().is_empty());
}

#[test]
fn any_action_dismisses_a_previous_test_message() {
    let mut c = Connections::default();
    let store = MemoryTokenStore::default();
    store.set("tok").unwrap();
    let Outcome::Test { probe, .. } = route(&mut c, ConnectionsEvent::TestMd, &store) else {
        panic!("expected a probe");
    };
    c.finish_probe(probe, ConnectionStatus::Connected, vec![], "ok".into());
    assert_eq!(c.md_test_result(), Some("ok"));

    route(&mut c, ConnectionsEvent::DisconnectMd, &store);
    assert_eq!(c.md_test_result(), None);
}

#[test]
fn connecting_without_a_stored_token_asks_for_one() {
    let mut c = Connections::default();
    let store = MemoryTokenStore::default();
    assert_eq!(
        route(&mut c, ConnectionsEvent::ConnectMd, &store),
        Outcome::NeedToken
    );
    // And it does not pretend to be connecting while the prompt is open.
    assert_eq!(c.md_status(), &ConnectionStatus::Disconnected);
}

#[test]
fn forget_clears_the_token_and_disconnects() {
    let mut c = Connections::default();
    let store = MemoryTokenStore::default();
    store.set("tok").unwrap();

    assert_eq!(
        route(&mut c, ConnectionsEvent::ForgetMd, &store),
        Outcome::Disconnected
    );
    assert_eq!(store.get().unwrap(), None);
    assert_eq!(c.md_status(), &ConnectionStatus::Disconnected);
    // The next Connect therefore has to ask again.
    assert_eq!(
        route(&mut c, ConnectionsEvent::ConnectMd, &store),
        Outcome::NeedToken
    );
}

#[test]
fn detaching_removes_the_attachment_and_asks_the_engine_to_follow() {
    let mut c = Connections::default();
    let store = MemoryTokenStore::default();
    c.add_sqlite("data", "/tmp/x.db");
    c.add_sqlite("other", "/tmp/y.db");

    assert_eq!(
        route(&mut c, ConnectionsEvent::Detach("data".into()), &store),
        Outcome::Detach {
            alias: "data".into()
        }
    );
    assert_eq!(c.sqlite().len(), 1);
    assert_eq!(c.sqlite()[0].alias, "other");
}

#[test]
fn attaching_a_file_is_handed_to_the_caller() {
    // The file picker is `rfd`'s, not this surface's.
    let mut c = Connections::default();
    let store = MemoryTokenStore::default();
    assert_eq!(
        route(&mut c, ConnectionsEvent::AttachSqlite, &store),
        Outcome::Browse
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Connections — the panel
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
struct ConnHostProps {
    initial: Connections,
}

#[component]
fn ConnHost(props: ConnHostProps) -> Element {
    let mut log = use_signal(Vec::<String>::new);
    let state = use_signal(|| props.initial.clone());
    rsx! {
        ConnectionsPanel {
            state,
            on_event: move |e: ConnectionsEvent| log.write().push(format!("{e:?}")),
        }
        div { "data-a11y-id": "captured", "{log.read().join(\"|\")}" }
    }
}

fn panel(initial: Connections) -> Harness {
    Harness::new(ConnHost, ConnHostProps { initial })
}

fn with_status(s: ConnectionStatus) -> Connections {
    let mut c = Connections::default();
    c.set_md_status(s);
    c
}

fn dot(h: &Harness) -> String {
    h.attr(h.by_a11y_id("connections-md-dot").unwrap(), "class")
        .unwrap_or_default()
}

#[test]
fn the_status_dot_says_what_the_connection_is_doing() {
    // Green pulses, amber pulses, red is still: a pulsing failure reads as
    // "working on it", which is exactly the wrong thing to say.
    assert_eq!(dot(&panel(Connections::default())), "d0-dot");
    assert_eq!(
        dot(&panel(with_status(ConnectionStatus::Connecting))),
        "d0-dot is-busy"
    );
    assert_eq!(
        dot(&panel(with_status(ConnectionStatus::Connected))),
        "d0-dot is-live"
    );
    assert_eq!(
        dot(&panel(with_status(ConnectionStatus::Error("nope".into())))),
        "d0-dot is-error"
    );
    // And the classifier the panel uses is the one the tests name.
    assert_eq!(
        status_dot_class(&ConnectionStatus::Connected),
        "d0-dot is-live"
    );
}

#[test]
fn the_buttons_offered_depend_on_the_state() {
    let disconnected = panel(Connections::default());
    assert!(disconnected.by_a11y_id("connections-md-connect").is_some());
    assert!(disconnected.by_a11y_id("connections-md-test").is_some());
    assert!(
        disconnected
            .by_a11y_id("connections-md-disconnect")
            .is_none(),
        "nothing to disconnect from"
    );

    let connected = panel(with_status(ConnectionStatus::Connected));
    assert!(connected.by_a11y_id("connections-md-disconnect").is_some());
    assert!(connected.by_a11y_id("connections-md-forget").is_some());
    assert!(connected.by_a11y_id("connections-md-test").is_some());
    assert!(connected.by_a11y_id("connections-md-connect").is_none());

    // Mid-connect there is no honest action: everything on offer would race
    // the in-flight ATTACH.
    let connecting = panel(with_status(ConnectionStatus::Connecting));
    assert!(connecting.by_a11y_id("connections-md-connect").is_none());
    assert!(connecting.by_a11y_id("connections-md-test").is_none());
    assert!(connecting.by_a11y_id("connections-md-disconnect").is_none());
}

#[test]
fn an_error_shows_its_reason_and_offers_a_retry() {
    let h = panel(with_status(ConnectionStatus::Error(
        "MotherDuck token was rejected.".into(),
    )));
    assert!(h.has_label("MotherDuck token was rejected."));
    assert!(h.by_a11y_id("connections-md-retry").is_some());
}

#[test]
fn a_panel_button_emits_its_intent_and_nothing_else() {
    // The panel is a pure function of state: it never reaches the keychain or
    // the engine, which is what makes it drivable with neither.
    let mut h = panel(with_status(ConnectionStatus::Connected));
    h.click("connections-md-test");
    h.click("connections-md-forget");
    assert_eq!(captured(&h), "TestMd|ForgetMd");
}

#[test]
fn attached_files_list_with_a_detach_for_each() {
    let mut c = Connections::default();
    c.add_sqlite("data", "/tmp/x.sqlite");
    let mut h = panel(c);

    assert!(h.by_a11y_id("connections-file-data").is_some());
    h.click("connections-detach-data");
    assert_eq!(captured(&h), "Detach(\"data\")");
}

#[test]
fn the_database_list_only_exists_while_connected() {
    let mut c = Connections::default();
    let store = MemoryTokenStore::default();
    store.set("tok").unwrap();
    let Outcome::Test { probe, .. } = route(&mut c, ConnectionsEvent::TestMd, &store) else {
        panic!("expected a probe");
    };
    c.finish_probe(
        probe,
        ConnectionStatus::Connected,
        vec!["sample_data".into()],
        "Connection OK".into(),
    );

    let h = panel(c.clone());
    assert!(h.by_a11y_id("connections-db-0").is_some());
    assert!(h.has_label("sample_data"));
    assert!(h.by_a11y_id("connections-md-test-result").is_some());

    route(&mut c, ConnectionsEvent::DisconnectMd, &store);
    let h = panel(c);
    assert!(h.by_a11y_id("connections-db-0").is_none());
    assert!(h.by_a11y_id("connections-md-test-result").is_none());
}
