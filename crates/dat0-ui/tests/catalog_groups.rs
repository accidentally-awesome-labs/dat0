//! SH2, on the sidebar: the catalog is three FIXED sections — FILES,
//! CONNECTIONS, PACKAGES — each of which paints its heading whether or not it
//! has content, and paints exactly one muted row when it does not.
//!
//! ## Why "fixed" is still the property under test
//! A section that disappeared when empty would make the sidebar's shape depend
//! on session state: a user with no attachments would never learn that
//! connections live in the sidebar at all, and the first attach would move
//! every row below it. The heading-always-present assertions are what stop a
//! later slice from "tidying up" the empty sections away.
//!
//! ## Why a bare heading is also a failure
//! An empty section with nothing under it reads as a rendering bug — the
//! reviewer cannot tell "no connections" from "the connections query threw".
//! So each empty section is asserted to carry its `catalog.empty.*` row too.
//!
//! ## What changed from the GPUI suite
//! The GPUI panel painted `FILES (0)` — a heading with a count — and every
//! assertion there was a `count_label` over that composed string. The sidebar
//! paints the heading and puts a parent's child count in the row's own meta
//! slot, so membership is asserted where it is actually visible: inside the
//! section's subtree.

mod support;

use std::collections::HashSet;
use std::path::PathBuf;

use dioxus::prelude::*;
use support::Harness;

use dat0_core::catalog::nav::{CONNECTIONS, FILES, PACKAGES, RowKind, visible_rows};
use dat0_core::catalog::{CATALOG_EMPTY_KEYS, CatalogTree, PackageNode};
use dat0_ui::components::sidebar::{Sidebar, sections};
use dat0_ui::state::Workspace;

#[derive(Clone, PartialEq, Props)]
struct HostProps {
    tree: CatalogTree,
}

/// Mounts the sidebar over a catalog, with the collapsed-alias set the widget
/// reports into. Copied per binary, matching the GPUI suite's own precedent.
#[component]
fn Host(props: HostProps) -> Element {
    let _ws = Workspace::provide();
    let collapsed = use_signal(HashSet::<String>::new);
    let rows = sections(&props.tree, &collapsed.read());
    rsx! {
        Sidebar {
            files: rows.files,
            connections: rows.connections,
            packages: rows.packages,
            session_line: "session · 1 window · 0 tabs".to_string(),
            ai_line: "ai none".to_string(),
            egress_line: "egress 0 B".to_string(),
            on_open: move |_: (&'static str, usize)| {},
            on_toggle: move |_: String| {},
        }
    }
}

fn mount(tree: CatalogTree) -> Harness {
    Harness::new(Host, HostProps { tree })
}

fn file_tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::File(PathBuf::from("/data/local.csv")),
    }
}

fn md_tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::Attached {
            alias: "sample_data".into(),
            source: "md:sample_data".into(),
        },
    }
}

/// The visible text of one section's subtree.
fn section_text(h: &Harness, name: &str) -> String {
    let key = h
        .by_a11y_id(&format!("section-{name}"))
        .unwrap_or_else(|| panic!("the {name} section must render"));
    h.text_of(key)
}

#[test]
fn all_three_sections_render_on_a_fresh_session() {
    let h = mount(CatalogTree::default());
    for name in ["files", "connections", "packages"] {
        assert!(
            h.by_a11y_id(&format!("section-{name}")).is_some(),
            "the {name} section must render on an empty catalog"
        );
        let heading = dat0_i18n::t(&format!("catalog.group.{name}"));
        assert_eq!(
            h.count_label(&heading),
            1,
            "{heading} must name exactly one node on an empty session"
        );
    }
}

#[test]
fn every_empty_section_paints_its_empty_state_row() {
    let h = mount(CatalogTree::default());
    for key in CATALOG_EMPTY_KEYS {
        let text = dat0_i18n::t(key);
        assert_ne!(&text, key, "{key} has no entry in en.json");
        let name = key.rsplit('.').next().expect("a trailing section name");
        let row = h
            .by_a11y_id(&format!("empty-{name}"))
            .unwrap_or_else(|| panic!("an empty section must paint its `{key}` row"));
        assert_eq!(
            h.text_of(row),
            text,
            "an empty section must say so, never leave a bare heading"
        );
    }
}

#[test]
fn a_registered_file_lands_under_files() {
    let h = mount(CatalogTree::build(&[file_tbl("local_sales")]));

    assert!(
        section_text(&h, "files").contains("local_sales"),
        "a file-origin table is local"
    );
    assert!(
        !section_text(&h, "connections").contains("local_sales"),
        "and must not also appear under connections"
    );
    // FILES is no longer empty, so its placeholder must be gone — otherwise the
    // "empty" row would be decoration rather than a statement.
    assert!(
        h.by_a11y_id("empty-files").is_none(),
        "a populated section must not also show its empty-state row"
    );
    assert!(
        h.by_a11y_id("empty-connections").is_some(),
        "the sections that are still empty keep theirs"
    );
}

#[test]
fn an_attached_table_lands_under_connections_beneath_its_alias() {
    let h = mount(CatalogTree::build(&[
        md_tbl("md_events"),
        file_tbl("sales"),
    ]));

    let connections = section_text(&h, "connections");
    assert!(
        connections.contains("sample_data"),
        "the attach alias is the parent row"
    );
    assert!(
        connections.contains("md_events"),
        "and the attached table is its child"
    );
    assert!(
        section_text(&h, "files").contains("sales"),
        "the file-origin table stays local"
    );
    assert!(
        !section_text(&h, "files").contains("md_events"),
        "an attached table must never be counted as local"
    );
}

#[test]
fn an_attach_parent_carries_its_child_count() {
    // The GPUI panel spelled this into the row's name (`sample_data (1)`); the
    // sidebar puts it in the meta slot, but the count must still be visible —
    // it is how a user sees an attach is non-empty while it is collapsed.
    let h = mount(CatalogTree::build(&[
        md_tbl("md_events"),
        md_tbl("md_users"),
    ]));
    let row = h.by_a11y_id("row-connections-0").expect("the parent row");
    let text = h.text_of(row);
    assert!(text.contains("sample_data"), "got {text:?}");
    assert!(
        text.contains('2'),
        "the child count must be visible: {text:?}"
    );
}

#[test]
fn sections_paint_in_files_connections_packages_order() {
    let tree = CatalogTree::build(&[file_tbl("sales"), md_tbl("md_events")]).with_packages(vec![
        PackageNode {
            name: "run.dat0".into(),
            path: PathBuf::from("/tmp/run.dat0"),
        },
    ]);

    // The model's order — the same `visible_rows` Vec the sidebar slices, so a
    // package landing in the wrong section fails here first.
    let rows = visible_rows(&tree, &HashSet::new());
    let order: Vec<&str> = rows.iter().map(|r| r.section).collect();
    assert_eq!(order, vec![FILES, CONNECTIONS, CONNECTIONS, PACKAGES]);
    assert!(matches!(
        &rows[3].kind,
        RowKind::Package { name, path }
            if name == "run.dat0" && path == std::path::Path::new("/tmp/run.dat0")
    ));

    // And the painted order matches it.
    let h = mount(tree);
    let text = h.text();
    let files = text.find("FILES").expect("FILES heading");
    let connections = text.find("CONNECTIONS").expect("CONNECTIONS heading");
    let packages = text.find("PACKAGES").expect("PACKAGES heading");
    assert!(
        files < connections && connections < packages,
        "sections paint FILES -> CONNECTIONS -> PACKAGES; got {text:?}"
    );
    assert!(
        section_text(&h, "packages").contains("run.dat0"),
        "the package row lands in its own section"
    );
}

#[test]
fn the_sidebar_names_itself_exactly_once() {
    // The GPUI twin asserted the dock was still the Catalog after seeding,
    // because every other assertion read through a capture that an empty panel
    // would also satisfy. Same guard, new surface: the landmark is present and
    // mounted once.
    let h = mount(CatalogTree::build(&[file_tbl("sales")]));
    assert!(h.by_a11y_id("sidebar").is_some());
    assert_eq!(h.count_label(&dat0_i18n::t("catalog.title")), 1);
}
