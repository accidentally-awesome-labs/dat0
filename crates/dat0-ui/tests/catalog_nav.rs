//! Catalog keyboard navigation, on the sidebar.
//!
//! The GPUI suite drove a focus-stopped panel inside the left dock; the surface
//! is now a section of the always-present sidebar. What is under test is
//! unchanged, because the model is unchanged: the rows come from
//! [`dat0_core::catalog::nav::visible_rows`] and each key press runs through
//! `nav::tree_nav`, so paint order, the keyboard cursor and the activation
//! target derive from ONE Vec and cannot drift apart.
//!
//! Every keystroke goes through the component's own `onkeydown` — never a
//! direct call to `tree_nav`. Calling the model would prove the model works
//! (which its unit tests already do) while a dead key path shipped.
//!
//! Two guarantees are stronger here than in the GPUI suite:
//!
//! * Enter-on-a-leaf is covered. The GPUI version was deleted because
//!   `open_table_tab` needs an ambient tokio reactor the test had none of; a
//!   Dioxus `EventHandler` needs no runtime, so the arm is asserted directly.
//! * Collapse-to-disk is asserted as the round trip the widget actually feeds:
//!   the alias it emits is what `SessionUiState::catalog_collapsed` persists.
//!   The widget deliberately does not own the collapsed set — see `on_toggle`.

mod support;

use std::collections::HashSet;
use std::path::PathBuf;

use dioxus::prelude::*;
use support::{Harness, Key, Modifiers};

use dat0_core::catalog::{CatalogTree, PackageNode};
use dat0_core::session::SessionUiState;
use dat0_ui::components::sidebar::{Sidebar, sections};
use dat0_ui::state::Workspace;

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    tree: CatalogTree,
}

/// Mounts the sidebar and records what it emitted. The collapsed-alias set
/// lives here, where a session would keep it, because the widget must not own
/// state something else has to persist.
#[component]
fn Host(props: HostProps) -> Element {
    let _ws = Workspace::provide();
    let mut collapsed = use_signal(HashSet::<String>::new);
    let mut opened = use_signal(Vec::<String>::new);
    let rows = sections(&props.tree, &collapsed.read());
    let collapsed_readback = {
        let mut names: Vec<String> = collapsed.read().iter().cloned().collect();
        names.sort();
        names.join(",")
    };
    rsx! {
        Sidebar {
            files: rows.files,
            connections: rows.connections,
            packages: rows.packages,
            session_line: "session · 1 window · 0 tabs".to_string(),
            ai_line: "ai none".to_string(),
            egress_line: "egress 0 B".to_string(),
            on_open: move |(section, i): (&'static str, usize)| {
                opened.write().push(format!("{section}:{i}"));
            },
            on_toggle: move |alias: String| {
                let mut set = collapsed.write();
                if !set.remove(&alias) {
                    set.insert(alias);
                }
            },
        }
        div { "data-a11y-id": "opened", "{opened.read().join(\" \")}" }
        div { "data-a11y-id": "collapsed", "{collapsed_readback}" }
    }
}

fn file_tbl(name: &str) -> dat0_engine::TableInfo {
    tbl(
        name,
        dat0_engine::TableOrigin::File(PathBuf::from("/data/local.csv")),
    )
}

fn md_tbl(name: &str) -> dat0_engine::TableInfo {
    tbl(
        name,
        dat0_engine::TableOrigin::Attached {
            alias: "sample_data".into(),
            source: "md:sample_data".into(),
        },
    )
}

fn sqlite_tbl(name: &str) -> dat0_engine::TableInfo {
    tbl(
        name,
        dat0_engine::TableOrigin::Attached {
            alias: "sq".into(),
            source: "/tmp/x.db".into(),
        },
    )
}

fn tbl(name: &str, origin: dat0_engine::TableOrigin) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin,
    }
}

/// The depth-2 tree every test below walks. Visible rows, paint order:
///
/// ```text
/// flat 0  files/0        L0:local_sales   (FILES)
/// flat 1  connections/0  P :sample_data   (md attach; "sample_data" < "sq")
/// flat 2  connections/1  L1:md_events
/// flat 3  connections/2  P :sq            (sqlite attach)
/// flat 4  connections/3  L1:alpha
/// flat 5  connections/4  L1:zeta
/// ```
fn seeded() -> Harness {
    let tree = CatalogTree::build(&[
        sqlite_tbl("alpha"),
        sqlite_tbl("zeta"),
        md_tbl("md_events"),
        file_tbl("local_sales"),
    ]);
    let mut h = Harness::new(Host, HostProps { tree });
    h.settle();
    h
}

fn press(h: &mut Harness, key: Key) {
    h.key_at("catalog-tree", key, Modifiers::empty());
}

fn down(h: &mut Harness, n: usize) {
    for _ in 0..n {
        press(h, Key::ArrowDown);
    }
}

/// The `data-a11y-id` of the row the keyboard cursor is on.
fn cursor_row(h: &Harness) -> Option<String> {
    h.dom()
        .walk()
        .into_iter()
        .find(|k| h.attr(*k, "aria-selected").as_deref() == Some("true"))
        .and_then(|k| h.attr(k, "data-a11y-id"))
}

fn readback(h: &Harness, id: &str) -> String {
    let key = h.by_a11y_id(id).unwrap_or_else(|| panic!("no {id}"));
    h.text_of(key)
}

fn has_row(h: &Harness, label: &str) -> bool {
    h.text_of(h.by_a11y_id("catalog-tree").expect("the tree"))
        .contains(label)
}

#[test]
fn the_catalog_tree_is_a_tab_stop() {
    // R6 on the new surface: a keyboard user reaches the catalog by Tab, not by
    // knowing a chord. The container is the stop — the rows are a roving
    // cursor inside it, exactly as the GPUI panel's `focus_stop` was.
    let mut h = seeded();
    for _ in 0..8 {
        if h.press_tab().is_some() && h.focused_id().as_deref() == Some("catalog-tree") {
            return;
        }
    }
    panic!(
        "the catalog tree was never reached by Tab; stops = {:?}",
        h.tab_order().len()
    );
}

#[test]
fn down_moves_the_active_row() {
    // R1: the tree's own key handler receives arrows and moves the cursor.
    let mut h = seeded();
    assert_eq!(cursor_row(&h).as_deref(), Some("row-files-0"));
    press(&mut h, Key::ArrowDown);
    assert_eq!(
        cursor_row(&h).as_deref(),
        Some("row-connections-0"),
        "Down moves onto the next VISIBLE row, across the section boundary"
    );
}

#[test]
fn arrows_walk_visible_rows_and_clamp() {
    let mut h = seeded();
    press(&mut h, Key::ArrowUp);
    assert_eq!(
        cursor_row(&h).as_deref(),
        Some("row-files-0"),
        "Up on the first row clamps rather than wrapping"
    );

    down(&mut h, 7); // 6 rows: 5 moves, then 2 that must do nothing
    assert_eq!(
        cursor_row(&h).as_deref(),
        Some("row-connections-4"),
        "Down clamps on the last visible row"
    );
}

#[test]
fn left_jumps_to_the_parent_then_collapses_it() {
    let mut h = seeded();
    down(&mut h, 5); // flat 5 = "zeta", a child of "sq"
    assert_eq!(cursor_row(&h).as_deref(), Some("row-connections-4"));

    press(&mut h, Key::ArrowLeft);
    assert_eq!(
        cursor_row(&h).as_deref(),
        Some("row-connections-2"),
        "Left on a child moves to ITS parent, not to any earlier row"
    );
    assert!(has_row(&h, "alpha"), "children render while expanded");
    assert!(has_row(&h, "zeta"));

    // Left on the expanded parent collapses it: the children leave the tree
    // entirely — absence is the assertion, because a hidden-but-present row is
    // still a screen-reader stop.
    press(&mut h, Key::ArrowLeft);
    assert!(!has_row(&h, "alpha") && !has_row(&h, "zeta"));
    assert!(
        has_row(&h, "md_events"),
        "the OTHER attach's children are untouched"
    );
    assert_eq!(readback(&h, "collapsed"), "sq");

    // Right re-expands, and the children come back.
    press(&mut h, Key::ArrowRight);
    assert!(has_row(&h, "alpha"));
    assert_eq!(readback(&h, "collapsed"), "");
}

#[test]
fn enter_on_a_parent_toggles_its_collapse() {
    let mut h = seeded();
    down(&mut h, 1); // flat 1 = the "sample_data" attach parent
    press(&mut h, Key::Enter);
    assert_eq!(readback(&h, "collapsed"), "sample_data");
    assert!(
        !has_row(&h, "md_events"),
        "its child vanishes on Enter-collapse"
    );
    assert_eq!(
        readback(&h, "opened"),
        "",
        "a parent is toggled, never opened as a table"
    );
}

#[test]
fn enter_on_a_leaf_asks_the_owner_to_open_it() {
    let mut h = seeded();
    press(&mut h, Key::Enter); // flat 0 = the "local_sales" leaf
    assert_eq!(readback(&h, "opened"), "files:0");
    assert_eq!(
        readback(&h, "collapsed"),
        "",
        "opening a leaf must not collapse anything"
    );
}

#[test]
fn space_opens_a_leaf_the_same_way_enter_does() {
    let mut h = seeded();
    press(&mut h, Key::Character(" ".into()));
    assert_eq!(readback(&h, "opened"), "files:0");
}

#[test]
fn a_package_activates_as_a_package_and_never_as_a_table() {
    // `nav` keeps packages out of `NavAction` precisely so a `.dat0` name can
    // never reach `open_table_tab`; the sidebar has to preserve that, and the
    // section it reports with is what makes the caller's dispatch unambiguous.
    let tree = CatalogTree::build(&[file_tbl("local_sales")]).with_packages(vec![PackageNode {
        name: "q2.dat0".into(),
        path: PathBuf::from("/tmp/q2.dat0"),
    }]);
    let mut h = Harness::new(Host, HostProps { tree });
    h.settle();

    down(&mut h, 1); // flat 1 = the package row
    assert_eq!(cursor_row(&h).as_deref(), Some("row-packages-0"));
    press(&mut h, Key::Enter);
    assert_eq!(
        readback(&h, "opened"),
        "packages:0",
        "the package is reported as a package, not as a table row"
    );
}

#[test]
fn a_click_moves_the_keyboard_cursor_too() {
    // The rail's rule, kept: mouse and keyboard may not drift out of sync, or
    // the next arrow key jumps back to wherever the keyboard last was.
    let mut h = seeded();
    h.click("row-connections-4"); // "zeta"
    assert_eq!(cursor_row(&h).as_deref(), Some("row-connections-4"));
    assert_eq!(readback(&h, "opened"), "connections:4");

    press(&mut h, Key::ArrowUp);
    assert_eq!(
        cursor_row(&h).as_deref(),
        Some("row-connections-3"),
        "Up steps from the row the mouse used, not from the old cursor"
    );
}

#[test]
fn a_collapsed_alias_survives_a_session_round_trip() {
    // The widget emits the alias; the session stores it. This pins the pair, so
    // a rename on either side fails here rather than silently forgetting every
    // user's collapsed attaches on the next launch.
    let mut h = seeded();
    down(&mut h, 1);
    press(&mut h, Key::Enter);
    let emitted = readback(&h, "collapsed");
    assert_eq!(emitted, "sample_data");

    let ui = SessionUiState {
        catalog_collapsed: vec![emitted.clone()],
    };
    let raw = serde_json::to_string(&ui).expect("serialize");
    assert!(
        raw.contains(r#""catalog_collapsed""#) && raw.contains(r#""sample_data""#),
        "the collapsed alias must reach the session document; got {raw}"
    );
    let back: SessionUiState = serde_json::from_str(&raw).expect("deserialize");
    assert_eq!(back.catalog_collapsed, vec![emitted]);
}

#[test]
fn a_collapsed_section_takes_its_rows_out_of_the_cursor_ring() {
    // A cursor that can land on a row nobody can see is a cursor that stops
    // responding: the user presses Down and nothing moves.
    let mut h = seeded();
    h.click("section-toggle-connections");
    assert!(!has_row(&h, "md_events"), "the section collapsed");

    down(&mut h, 3);
    assert_eq!(
        cursor_row(&h).as_deref(),
        Some("row-files-0"),
        "the only reachable row is the one still painted"
    );
}
