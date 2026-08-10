//! The catalog sidebar (S1).
//!
//! One always-present column with three independently collapsible sections,
//! replacing the activity rail and its three-way mode switch. The workbench
//! needs files, connections and packages visible at once: you pick a table,
//! then a connection, then seal a package, and mode-hopping between those was
//! the single most-repeated interaction in the old shell.
//!
//! The rows are the catalog, flattened by [`dat0_core::catalog::nav`] — the
//! same `visible_rows` list the GPUI panel painted and the same `tree_nav`
//! transition table its arrow keys ran. Paint order, the keyboard cursor and
//! the activation target therefore derive from ONE Vec and cannot drift.

use std::collections::HashSet;

use dioxus::prelude::*;

use dat0_core::catalog::CatalogTree;
use dat0_core::catalog::nav::{self, CatalogRow, NavAction, RowKind};

use crate::a11y::{AccessRole, format_swatch};
use crate::state::{SECTION_CONNECTIONS, SECTION_FILES, SECTION_PACKAGES, Workspace};

/// One row in a section.
#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    /// Left-hand text.
    pub label: String,
    /// Right-aligned, ellipsising meta: `1.2 B rows`, `12.4 GB`, `sealed`.
    pub meta: String,
    /// Format swatch class, when the row names a file.
    pub swatch: Option<&'static str>,
    /// Connection liveness: `Some(true)` pulses green, `Some(false)` is grey,
    /// `None` draws no dot.
    pub live: Option<bool>,
    /// Whether this row is the current selection.
    pub active: bool,
    /// What this row is in [`nav`] terms.
    ///
    /// Carried rather than re-derived: the keyboard model is `nav::tree_nav`,
    /// and a row that cannot say whether it is an attach parent, one of its
    /// children or a package cannot be arrowed over correctly.
    pub kind: RowKind,
}

impl Row {
    /// A file row, swatched by extension.
    pub fn file(name: impl Into<String>, meta: impl Into<String>) -> Self {
        let label: String = name.into();
        let swatch = format_swatch(std::path::Path::new(&label));
        Self {
            kind: RowKind::Leaf {
                name: label.clone(),
                depth: 0,
            },
            label,
            meta: meta.into(),
            swatch: Some(swatch),
            live: None,
            active: false,
        }
    }

    /// A connection row, with a liveness dot.
    pub fn connection(name: impl Into<String>, meta: impl Into<String>, live: bool) -> Self {
        let label: String = name.into();
        Self {
            kind: RowKind::Leaf {
                name: label.clone(),
                depth: 0,
            },
            label,
            meta: meta.into(),
            swatch: None,
            live: Some(live),
            active: false,
        }
    }

    /// The display projection of one flattened catalog row.
    pub fn from_catalog(row: &CatalogRow) -> Self {
        let (label, meta, swatch) = match &row.kind {
            // The count is the meta rather than part of the name: the GPUI
            // panel painted `sq (2)`, which a screen reader then read as the
            // alias — the number belongs in the right-aligned slot every
            // other row uses for its size.
            RowKind::Parent {
                alias, n_children, ..
            } => (alias.clone(), n_children.to_string(), None),
            RowKind::Leaf { name, .. } => {
                let swatch =
                    (row.section == nav::FILES).then(|| format_swatch(std::path::Path::new(name)));
                (name.clone(), String::new(), swatch)
            }
            RowKind::Package { name, path } => {
                (name.clone(), String::new(), Some(format_swatch(path)))
            }
        };
        Self {
            label,
            meta,
            swatch,
            live: None,
            active: false,
            kind: row.kind.clone(),
        }
    }
}

/// The three sections' rows, in paint order.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Sections {
    pub files: Vec<Row>,
    pub connections: Vec<Row>,
    pub packages: Vec<Row>,
}

/// Split the catalog into the three sections the sidebar paints.
///
/// The split preserves [`nav::visible_rows`] order within each section, and the
/// concatenation `files ++ connections ++ packages` **is** `visible_rows` — the
/// sidebar rebuilds exactly that list for the keyboard, so what is painted and
/// what is arrowed over are the same rows by construction.
pub fn sections(tree: &CatalogTree, collapsed: &HashSet<String>) -> Sections {
    let mut out = Sections::default();
    for row in nav::visible_rows(tree, collapsed) {
        let display = Row::from_catalog(&row);
        match row.section {
            nav::CONNECTIONS => out.connections.push(display),
            nav::PACKAGES => out.packages.push(display),
            _ => out.files.push(display),
        }
    }
    out
}

/// The sidebar's three sections, filled by the shell.
#[derive(Clone, PartialEq, Props)]
pub struct SidebarProps {
    pub files: Vec<Row>,
    pub connections: Vec<Row>,
    pub packages: Vec<Row>,
    /// Footer line 1: `session · N windows · M tabs`.
    pub session_line: String,
    /// Footer line 2: the AI provider, or `ai none`.
    pub ai_line: String,
    /// Footer line 3: egress, always shown so zero is visible.
    pub egress_line: String,
    /// A row was clicked or activated, by section name and index.
    pub on_open: EventHandler<(&'static str, usize)>,
    /// An attach parent's collapse was toggled, by alias. The collapsed set
    /// belongs to whoever persists it, not to the widget.
    pub on_toggle: EventHandler<String>,
}

#[component]
pub fn Sidebar(props: SidebarProps) -> Element {
    let ws = Workspace::use_current();
    let mut cursor = use_signal(|| 0usize);
    let (on_open, on_toggle) = (props.on_open, props.on_toggle);

    // Rows the user can actually reach: a collapsed SECTION paints none, so
    // arrowing into one would move a cursor nobody can see.
    let reachable: Vec<(&'static str, usize, CatalogRow)> = [
        (SECTION_FILES, nav::FILES, &props.files),
        (SECTION_CONNECTIONS, nav::CONNECTIONS, &props.connections),
        (SECTION_PACKAGES, nav::PACKAGES, &props.packages),
    ]
    .into_iter()
    .filter(|(name, _, _)| !ws.section_collapsed(name))
    .flat_map(|(name, section, rows)| {
        rows.iter().enumerate().map(move |(i, row)| {
            (
                name,
                i,
                CatalogRow {
                    section,
                    kind: row.kind.clone(),
                },
            )
        })
    })
    .collect();

    // Rows come and go as sections and parents collapse; a cursor past the end
    // would silently stop responding to arrows.
    let at = (*cursor.read()).min(reachable.len().saturating_sub(1));
    let cursor_in = |section: &'static str| {
        reachable
            .get(at)
            .filter(|(name, _, _)| *name == section)
            .map(|(_, i, _)| *i)
    };

    let nav_rows: Vec<CatalogRow> = reachable.iter().map(|(_, _, r)| r.clone()).collect();
    let addressed: Vec<(&'static str, usize)> =
        reachable.iter().map(|(name, i, _)| (*name, *i)).collect();

    // A click activates AND moves the keyboard cursor. The GPUI rail kept the
    // two in step for the same reason: a cursor the mouse never moves means
    // the next arrow key jumps back to wherever the keyboard last was.
    let clicked = addressed.clone();
    let activate = use_callback(move |target: (&'static str, usize)| {
        if let Some(k) = clicked.iter().position(|a| *a == target) {
            cursor.set(k);
        }
        on_open.call(target);
    });

    rsx! {
        div {
            class: "d0-sidebar",
            "data-a11y-id": "sidebar",
            role: AccessRole::Navigation.aria(),
            "aria-label": dat0_i18n::t("catalog.title"),

            div { class: "d0-sidebar-head",
                span { class: "d0-label", "catalog" }
                span { class: "d0-mono is-local", "local" }
            }

            div {
                class: "d0-sidebar-body",
                "data-a11y-id": "catalog-tree",
                role: "tree",
                tabindex: "0",
                onkeydown: move |e: KeyboardEvent| {
                    let Some(key) = nav_key(&e.key()) else { return };
                    // Only for keys the tree owns: everything else keeps
                    // bubbling to the shell's cascade.
                    e.stop_propagation();
                    e.prevent_default();
                    match nav::tree_nav(&nav_rows, at, key) {
                        NavAction::Move(i) => cursor.set(i),
                        NavAction::Toggle(alias) => on_toggle.call(alias),
                        NavAction::Open(_) => {
                            if let Some(target) = addressed.get(at) {
                                on_open.call(*target);
                            }
                        }
                        // A package is not a table and must never reach
                        // `open_table_tab`; `nav` keeps it out of `NavAction`
                        // for exactly that reason, so it activates here.
                        NavAction::None => {
                            if nav::package_activation(&nav_rows, at, key).is_some() {
                                if let Some(target) = addressed.get(at) {
                                    on_open.call(*target);
                                }
                            }
                        }
                    }
                },

                Section {
                    name: SECTION_FILES,
                    rows: props.files.clone(),
                    empty: dat0_i18n::t("catalog.empty.files"),
                    cursor: cursor_in(SECTION_FILES),
                    on_open: activate,
                }
                Section {
                    name: SECTION_CONNECTIONS,
                    rows: props.connections.clone(),
                    empty: dat0_i18n::t("catalog.empty.connections"),
                    cursor: cursor_in(SECTION_CONNECTIONS),
                    on_open: activate,
                }
                Section {
                    name: SECTION_PACKAGES,
                    rows: props.packages.clone(),
                    empty: dat0_i18n::t("catalog.empty.packages"),
                    cursor: cursor_in(SECTION_PACKAGES),
                    on_open: activate,
                }
            }

            div { class: "d0-sidebar-foot",
                span { "{props.session_line}" }
                span { "{props.ai_line}" }
                span { class: "is-ok", "{props.egress_line}" }
            }

        }
    }
}

/// The nav grammar a DOM key press means, if any.
///
/// `nav::tree_nav` speaks the keymap table's vocabulary (`"down"`, `"enter"`);
/// the DOM speaks UI Events'. One mapping, here, so the model stays free of
/// renderer types.
fn nav_key(key: &Key) -> Option<&'static str> {
    Some(match key {
        Key::ArrowDown => "down",
        Key::ArrowUp => "up",
        Key::ArrowLeft => "left",
        Key::ArrowRight => "right",
        Key::Enter => "enter",
        Key::Character(c) if c == " " => "space",
        _ => return None,
    })
}

#[component]
fn Section(
    name: &'static str,
    rows: Vec<Row>,
    empty: String,
    cursor: Option<usize>,
    on_open: EventHandler<(&'static str, usize)>,
) -> Element {
    let mut ws = Workspace::use_current();
    let collapsed = ws.section_collapsed(name);
    // Composed from the section id rather than written out three times; the
    // keys are listed in `catalog::CATALOG_GROUP_KEYS` so the i18n checker can
    // still resolve them.
    let heading = dat0_i18n::t(&format!("catalog.group.{name}"));

    rsx! {
        div { "data-a11y-id": "section-{name}", role: "group", "aria-label": "{heading}",
            button {
                class: "d0-section-label d0-label",
                "data-a11y-id": "section-toggle-{name}",
                "aria-expanded": if collapsed { "false" } else { "true" },
                onclick: move |_| ws.toggle_section(name),
                span { class: if collapsed { "d0-chevron is-collapsed" } else { "d0-chevron" }, "▾" }
                "{heading}"
            }
            if !collapsed {
                if rows.is_empty() {
                    // Kept rather than hidden: a section that vanishes when
                    // empty makes the shell's shape jump around on every
                    // attach, and hides where the thing you want would appear.
                    div { class: "d0-row is-empty", "data-a11y-id": "empty-{name}", "{empty}" }
                } else {
                    for (i, row) in rows.iter().enumerate() {
                        button {
                            key: "{name}-{i}",
                            class: row_class(row, cursor == Some(i)),
                            "data-a11y-id": "row-{name}-{i}",
                            role: "treeitem",
                            "aria-selected": if cursor == Some(i) { "true" } else { "false" },
                            "aria-expanded": match row.kind {
                                RowKind::Parent { expanded, .. } => Some(if expanded { "true" } else { "false" }),
                                _ => None,
                            },
                            onclick: move |_| on_open.call((name, i)),
                            span { class: "d0-row-name",
                                if let Some(sw) = row.swatch {
                                    span { class: "d0-swatch {sw}" }
                                }
                                if let Some(live) = row.live {
                                    span { class: if live { "d0-dot is-live" } else { "d0-dot" } }
                                }
                                "{row.label}"
                            }
                            span { class: "d0-row-meta", "{row.meta}" }
                        }
                    }
                }
            }
        }
    }
}

/// A row's classes: selection, the keyboard cursor, and the one indent level
/// an attach parent's children take.
fn row_class(row: &Row, at_cursor: bool) -> String {
    let mut class = String::from("d0-row");
    if matches!(row.kind, RowKind::Leaf { depth: 1, .. }) {
        class.push_str(" is-child");
    }
    if row.active {
        class.push_str(" is-active");
    }
    if at_cursor {
        class.push_str(" is-cursor");
    }
    class
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_core::catalog::{CatalogNode, PackageNode};
    use std::path::PathBuf;

    #[test]
    fn a_file_row_takes_its_swatch_from_the_extension() {
        assert_eq!(Row::file("sales.csv", "1.2 B rows").swatch, Some("sw-csv"));
        assert_eq!(
            Row::file("events.parquet", "12 GB").swatch,
            Some("sw-parquet")
        );
        assert_eq!(Row::file("q2.dat0", "sealed").swatch, Some("sw-dat0"));
    }

    #[test]
    fn a_connection_row_carries_liveness_and_no_swatch() {
        let r = Row::connection("pg · crm", "connected", true);
        assert_eq!(r.live, Some(true));
        assert_eq!(r.swatch, None);
    }

    #[test]
    fn the_three_sections_concatenate_back_into_visible_rows() {
        // The property the keyboard depends on: what each section paints, in
        // order, IS the flat nav list.
        let tree = CatalogTree {
            files: vec![CatalogNode {
                name: "sales".into(),
                schema: "main".into(),
                children: vec![],
            }],
            connections: vec![CatalogNode {
                name: "sq".into(),
                schema: String::new(),
                children: vec!["alpha".into(), "zeta".into()],
            }],
            packages: vec![PackageNode {
                name: "q2.dat0".into(),
                path: PathBuf::from("/tmp/q2.dat0"),
            }],
        };
        let s = sections(&tree, &HashSet::new());
        let flat: Vec<String> = s
            .files
            .iter()
            .chain(&s.connections)
            .chain(&s.packages)
            .map(|r| r.label.clone())
            .collect();
        let expected: Vec<String> = nav::visible_rows(&tree, &HashSet::new())
            .iter()
            .map(|r| Row::from_catalog(r).label)
            .collect();
        assert_eq!(flat, expected);
        assert_eq!(flat, vec!["sales", "sq", "alpha", "zeta", "q2.dat0"]);
    }

    #[test]
    fn an_attach_parent_shows_its_child_count_as_meta() {
        let tree = CatalogTree {
            files: vec![],
            connections: vec![CatalogNode {
                name: "sq".into(),
                schema: String::new(),
                children: vec!["alpha".into(), "zeta".into()],
            }],
            packages: vec![],
        };
        let s = sections(&tree, &HashSet::new());
        assert_eq!(s.connections[0].label, "sq");
        assert_eq!(s.connections[0].meta, "2");
    }
}
