//! The grid's edit verbs (step 5.3b): a typed cell, copy, cut, paste, fill
//! down, set NULL, set a value, delete rows, and delete a column.
//!
//! None of them reached anything before (PD-023): the cell editor's commit
//! went nowhere and the context menu offered every verb disabled. These tests
//! mount the real `Shell` over a real session, open a CSV, and drive each verb
//! the way a person does — the keyboard, the context menu, the palette — then
//! read the cells back, and for a copy the clipboard.

mod support;

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use dioxus::prelude::*;
use serial_test::serial;

use dat0_core::actions::builtin::{ids, register_all};
use dat0_core::actions::registry::ActionRegistry;
use dat0_ui::components::shell::Shell;
use dat0_ui::components::use_window_bus;
use dat0_ui::launch::Boot;
use dat0_ui::router::{Surface, route};
use dat0_ui::session_boot;
use dat0_ui::state::Workspace;
use dat0_ui::theme::Theme;
use support::{Harness, primary};

/// Process-global state root and config dir, leaked so a session never reads a
/// deleted directory mid-test. The shape `grid_views.rs` uses.
static STATE_ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("state");
    let cfg = tmp.path().join("cfg");
    std::fs::create_dir_all(&root).expect("mkdir state");
    std::fs::create_dir_all(&cfg).expect("mkdir cfg");
    // SAFETY: every test in this binary is `#[serial]`, so no other thread
    // races this process-global write.
    unsafe { std::env::set_var("DAT0_CONFIG_DIR", &cfg) };
    dat0_core::settings::set_first_run_done(
        &dat0_core::settings::store::SettingsStore::with_path(cfg.join("settings.toml")),
        true,
    )
    .expect("seed first_run_done");
    dat0_core::globals::install_state_root(root.clone());
    std::mem::forget(tmp);
    root
});

const COMMANDS: &[&str] = &[
    ids::VIEW_UNDO,
    ids::VIEW_SET_VALUE,
    ids::VIEW_DELETE_ROWS,
    ids::VIEW_DELETE_COLUMN,
];

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    cli_paths: Vec<PathBuf>,
}

/// `App`'s wiring, minus what needs a desktop window.
#[component]
fn Host(props: HostProps) -> Element {
    Theme::provide(None);
    let ws = Workspace::provide();
    let surface = use_context_provider(|| Signal::new(Option::<Surface>::None));
    let boot = use_hook(|| {
        let reg = ActionRegistry::new();
        register_all(&reg).expect("built-in actions register");
        Boot::new(reg, Vec::new())
    });
    use_context_provider(|| boot.registry.clone());
    session_boot::use_session(ws, props.cli_paths.clone());
    let events = use_window_bus(boot, ws, surface);
    let mut read_only = ws.read_only;
    rsx! {
        button {
            "data-a11y-id": "read-only",
            onclick: move |_| read_only.set(true),
        }
        for id in COMMANDS.iter().copied() {
            button {
                key: "{id}",
                "data-a11y-id": "do-{id}",
                onclick: {
                    let events = events.clone();
                    move |_| assert!(route(ws, &events, surface, id), "{id} was not routed")
                },
            }
        }
        Shell {}
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

/// Settle, then sleep, until `done` or two minutes.
fn pump(h: &mut Harness, done: impl Fn(&Harness) -> bool) -> bool {
    for _ in 0..4800 {
        h.settle();
        if done(h) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

fn text(h: &Harness, id: &str) -> String {
    h.by_a11y_id(id).map(|k| h.text_of(k)).unwrap_or_default()
}

/// Column `col`, top to bottom, as far as the rows go.
fn column(h: &Harness, col: usize) -> Vec<String> {
    (0..10)
        .map(|r| text(h, &format!("cell-{r}-{col}")))
        .take_while(|v| !v.is_empty())
        .collect()
}

/// A window over a three-row CSV, with its rows painted. `day` is a DATE
/// column, which the grid used to paint as `(Date32)`.
fn window(dir: &str) -> Harness {
    let dir = STATE_ROOT.join(dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    let csv = dir.join("stock.csv");
    std::fs::write(
        &csv,
        "name,qty,day\nb,2,2024-01-02\na,3,2024-02-03\nc,1,2024-03-04\n",
    )
    .expect("write csv");
    let mut h = Harness::new(
        Host,
        HostProps {
            cli_paths: vec![csv],
        },
    );
    assert!(
        pump(&mut h, |h| column(h, 0) == ["b", "a", "c"]),
        "the CSV never painted: {:?}",
        column(&h, 0)
    );
    h
}

fn mouse(mods: Modifiers) -> dioxus::html::SerializedMouseData {
    use dioxus::html::geometry::{ClientPoint, Coordinates, ElementPoint, PagePoint, ScreenPoint};
    use dioxus::html::input_data::MouseButton;
    let at = Coordinates::new(
        ScreenPoint::new(0.0, 0.0),
        ClientPoint::new(0.0, 0.0),
        ElementPoint::new(0.0, 0.0),
        PagePoint::new(0.0, 0.0),
    );
    dioxus::html::SerializedMouseData::new(
        Some(MouseButton::Primary),
        MouseButton::Primary.into(),
        at,
        mods,
    )
}

/// Press on a cell and let go, with `mods` held: Shift extends the selection.
fn press(h: &mut Harness, row: usize, col: usize, mods: Modifiers) {
    let cell = h
        .by_a11y_id(&format!("cell-{row}-{col}"))
        .unwrap_or_else(|| panic!("no cell {row},{col}"));
    h.dispatch(cell, "mousedown", mouse(mods));
    let vp = h.by_a11y_id("grid-viewport").expect("the grid");
    h.dispatch(vp, "mouseup", mouse(Modifiers::empty()));
}

/// Select `(r0, c0)` through `(r1, c1)`.
fn select(h: &mut Harness, (r0, c0): (usize, usize), (r1, c1): (usize, usize)) {
    press(h, r0, c0, Modifiers::empty());
    if (r1, c1) != (r0, c0) {
        press(h, r1, c1, Modifiers::SHIFT);
    }
}

/// A keystroke at the grid.
fn key(h: &mut Harness, k: Key, mods: Modifiers) {
    h.key_at("grid-viewport", k, mods);
}

fn chord(h: &mut Harness, c: &str) {
    key(h, Key::Character(c.into()), primary());
}

fn perform(h: &mut Harness, id: &str) {
    h.click(&format!("do-{id}"));
}

fn typed(value: &str) -> dioxus::html::SerializedFormData {
    dioxus::html::SerializedFormData::new(value.to_string(), Vec::new())
}

fn banner_says(h: &Harness, key: &str) -> bool {
    h.has_label_contains(&dat0_i18n::t(key))
}

#[test]
#[serial]
fn every_type_paints_its_value_and_a_typed_cell_is_written_and_undone() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("typed");
    assert_eq!(
        column(&h, 2),
        ["2024-01-02", "2024-02-03", "2024-03-04"],
        "a date paints as a date, not as its type's name"
    );

    // Enter opens the editor on a's quantity; Enter commits and walks down.
    select(&mut h, (1, 1), (1, 1));
    key(&mut h, Key::Enter, Modifiers::empty());
    let editor = h.by_a11y_id("cell-editor").expect("Enter opens the editor");
    h.dispatch(editor, "input", typed("30"));
    h.key(editor, Key::Enter, Modifiers::empty());
    assert!(
        pump(&mut h, |h| column(h, 1) == ["2", "30", "1"]),
        "the typed value lands in its cell: {:?}",
        column(&h, 1)
    );
    assert_eq!(
        h.by_a11y_id("cell-2-1").and_then(|c| h.attr(c, "class")),
        Some("d0-cell is-right is-selected is-active".to_string()),
        "the cursor walked down, and the rebind kept it there"
    );

    perform(&mut h, ids::VIEW_UNDO);
    assert!(
        pump(&mut h, |h| column(h, 1) == ["2", "3", "1"]),
        "Undo takes the edit back: {:?}",
        column(&h, 1)
    );
}

#[test]
#[serial]
fn copy_puts_the_selection_on_the_clipboard_and_paste_writes_it_back() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("copy");

    select(&mut h, (0, 0), (1, 1));
    chord(&mut h, "c");
    assert!(
        pump(&mut h, |_| dat0_ui::clipboard::text().as_deref()
            == Some("b\t2\r\na\t3")),
        "Copy writes the block as TSV: {:?}",
        dat0_ui::clipboard::text()
    );

    // Pasted with its corner on c: c's row takes b's, and the row the block
    // would need past the end is skipped, and said so.
    select(&mut h, (2, 0), (2, 0));
    chord(&mut h, "v");
    assert!(
        pump(&mut h, |h| column(h, 0) == ["b", "a", "b"]
            && column(h, 1) == ["2", "3", "2"]),
        "Paste writes the block at the cursor: {:?} {:?}",
        column(&h, 0),
        column(&h, 1)
    );
    assert!(
        banner_says(&h, "grid.paste.skipped"),
        "the two cells past the last row are reported, not dropped silently"
    );
}

#[test]
#[serial]
fn cut_copies_then_clears_and_delete_sets_null() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("cut");

    select(&mut h, (0, 0), (0, 0));
    chord(&mut h, "x");
    assert!(
        pump(&mut h, |h| text(h, "cell-0-0") == "NULL"),
        "Cut clears the cell: {:?}",
        column(&h, 0)
    );
    assert_eq!(dat0_ui::clipboard::text().as_deref(), Some("b"));

    select(&mut h, (1, 1), (2, 1));
    key(&mut h, Key::Delete, Modifiers::empty());
    assert!(
        pump(&mut h, |h| column(h, 1) == ["2", "NULL", "NULL"]),
        "Delete sets the selection to NULL: {:?}",
        column(&h, 1)
    );
    for cell in ["cell-1-1", "cell-2-1"] {
        assert!(
            h.by_a11y_id(cell)
                .and_then(|c| h.attr(c, "class"))
                .is_some_and(|c| c.contains("is-selected")),
            "{cell}: an edit keeps the range it acted on selected"
        );
    }

    // A NULL copies out as an empty cell and pastes back as NULL, even into
    // a number column that could not hold the empty string.
    chord(&mut h, "c");
    assert!(pump(&mut h, |_| dat0_ui::clipboard::text().as_deref()
        == Some("\r\n")));
    select(&mut h, (0, 1), (0, 1));
    chord(&mut h, "v");
    assert!(
        pump(&mut h, |h| column(h, 1) == ["NULL", "NULL", "NULL"]),
        "{:?}",
        column(&h, 1)
    );
    assert!(
        !banner_says(&h, "grid.paste.skipped"),
        "nothing was skipped"
    );
}

#[test]
#[serial]
fn fill_down_copies_each_columns_top_value() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("fill");

    select(&mut h, (0, 1), (2, 2));
    chord(&mut h, "d");
    assert!(
        pump(&mut h, |h| column(h, 1) == ["2", "2", "2"]
            && column(h, 2) == ["2024-01-02", "2024-01-02", "2024-01-02"]),
        "each column fills from its top cell, as its own type: {:?} {:?}",
        column(&h, 1),
        column(&h, 2)
    );
    assert_eq!(
        column(&h, 0),
        ["b", "a", "c"],
        "unselected columns keep theirs"
    );
}

#[test]
#[serial]
fn set_value_asks_then_writes_each_cell_that_can_hold_it() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("set");

    select(&mut h, (0, 0), (1, 1));
    perform(&mut h, ids::VIEW_SET_VALUE);
    let field = h.by_a11y_id("name-prompt-field").expect("the value prompt");
    h.dispatch(field, "input", typed("9"));
    h.click("name-prompt-ok");
    assert!(
        pump(&mut h, |h| column(h, 0) == ["9", "9", "c"]
            && column(h, 1) == ["9", "9", "1"]),
        "both columns take 9: {:?} {:?}",
        column(&h, 0),
        column(&h, 1)
    );

    // A number column cannot hold "x"; the name column can.
    perform(&mut h, ids::VIEW_SET_VALUE);
    let field = h.by_a11y_id("name-prompt-field").expect("the value prompt");
    h.dispatch(field, "input", typed("x"));
    h.click("name-prompt-ok");
    assert!(
        pump(&mut h, |h| column(h, 0) == ["x", "x", "c"]),
        "{:?}",
        column(&h, 0)
    );
    assert_eq!(column(&h, 1), ["9", "9", "1"], "qty keeps its numbers");
    assert!(banner_says(&h, "grid.set_value.skipped"));
}

#[test]
#[serial]
fn delete_rows_and_delete_column_change_the_view_and_undo_restores_them() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("delete");

    // b and c's rows, through the context menu.
    press(&mut h, 0, 0, Modifiers::empty());
    press(&mut h, 2, 0, primary());
    perform(&mut h, ids::VIEW_DELETE_ROWS);
    assert!(
        pump(&mut h, |h| column(h, 0) == ["a"]),
        "{:?}",
        column(&h, 0)
    );
    assert!(
        h.by_a11y_id("cell-0-0")
            .and_then(|c| h.attr(c, "class"))
            .is_some_and(|c| c.contains("is-selected")),
        "the cursor stays, clamped to the rows that are left, rather than \
         the selection vanishing"
    );
    perform(&mut h, ids::VIEW_UNDO);
    assert!(pump(&mut h, |h| column(h, 0) == ["b", "a", "c"]));

    // Hiding qty moves day into its place.
    select(&mut h, (0, 1), (0, 1));
    perform(&mut h, ids::VIEW_DELETE_COLUMN);
    assert!(
        pump(&mut h, |h| h
            .by_a11y_id("col-1")
            .and_then(|c| h.attr(c, "aria-label"))
            == Some("day".to_string())),
        "qty is hidden"
    );
    assert!(h.by_a11y_id("col-2").is_none(), "two columns left");

    // The last shown columns cannot all go.
    select(&mut h, (0, 0), (0, 1));
    perform(&mut h, ids::VIEW_DELETE_COLUMN);
    assert!(banner_says(&h, "grid.delete_column.last"));
    assert!(h.by_a11y_id("col-1").is_some(), "both still shown");

    perform(&mut h, ids::VIEW_UNDO);
    assert!(
        pump(&mut h, |h| column(h, 1) == ["2", "3", "1"]),
        "Undo shows qty again: {:?}",
        column(&h, 1)
    );
}

#[test]
#[serial]
fn a_read_only_workspace_refuses_every_edit_but_copy() {
    let rt = runtime();
    let _guard = rt.enter();
    let mut h = window("read-only");
    h.click("read-only");

    select(&mut h, (0, 1), (1, 1));
    key(&mut h, Key::Delete, Modifiers::empty());
    assert!(banner_says(&h, "view.read_only"), "the refusal is said");
    chord(&mut h, "c");
    assert!(
        pump(&mut h, |_| dat0_ui::clipboard::text().as_deref()
            == Some("2\r\n3")),
        "copy still works: {:?}",
        dat0_ui::clipboard::text()
    );
    std::thread::sleep(Duration::from_millis(200));
    h.settle();
    assert_eq!(column(&h, 1), ["2", "3", "1"], "and nothing was written");
}
