//! Pure keyboard-nav model for the Catalog panel (catalog-tree slice).
//!
//! `visible_rows` flattens the [`CatalogTree`] + a collapsed-alias set into the
//! exact row list the panel paints, in paint order — the SINGLE source of truth
//! for both the render iteration and the keyboard active-index (ring, arrows,
//! Enter and painted rows derive from one Vec, so they cannot drift). `tree_nav`
//! maps (rows, active, key) to a [`NavAction`] — the pure-fn → enum → thin-match
//! idiom (`resolve_relaunch_action` precedent), unit-testable without GPUI.

use std::collections::HashSet;
use std::path::PathBuf;

use crate::catalog::{CatalogNode, CatalogTree, PackageNode};

/// Stable section ids, in paint order. These are ids, not labels: the visible
/// text is `catalog.group.*` from the string table, and the two must be free to
/// diverge (the label is uppercase and carries a count).
pub const FILES: &str = "Files";
pub const CONNECTIONS: &str = "Connections";
pub const PACKAGES: &str = "Packages";

/// One visible catalog row. `section` is the panel's stable section id — one of
/// [`FILES`], [`CONNECTIONS`], [`PACKAGES`].
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogRow {
    pub section: &'static str,
    pub kind: RowKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    /// An attachment parent node (name = attach alias).
    Parent {
        alias: String,
        expanded: bool,
        n_children: usize,
    },
    /// A table row. `depth` 0 = top-level leaf, 1 = child of the preceding
    /// Parent (children always immediately follow their parent, so `tree_nav`
    /// finds the parent by scanning backward for the nearest Parent row).
    Leaf { name: String, depth: u8 },
    /// A `.dat0` package row. Carries the path because activating it opens a
    /// file, not a catalog entry — see [`package_activation`].
    Package { name: String, path: PathBuf },
}

/// Every VISIBLE catalog row in paint order. Children of collapsed parents are
/// absent. Section headers are NOT rows (non-interactive; nav skips them), and
/// neither are empty-group placeholder rows (SH2) — a row that exists only to
/// say "nothing here" must not be a stop the user has to arrow past.
pub fn visible_rows(tree: &CatalogTree, collapsed: &HashSet<String>) -> Vec<CatalogRow> {
    let sections: [(&'static str, &Vec<CatalogNode>); 2] =
        [(FILES, &tree.files), (CONNECTIONS, &tree.connections)];
    let mut rows = Vec::new();
    for (section, nodes) in sections {
        for node in nodes {
            if node.children.is_empty() {
                rows.push(CatalogRow {
                    section,
                    kind: RowKind::Leaf {
                        name: node.name.clone(),
                        depth: 0,
                    },
                });
            } else {
                let expanded = !collapsed.contains(&node.name);
                rows.push(CatalogRow {
                    section,
                    kind: RowKind::Parent {
                        alias: node.name.clone(),
                        expanded,
                        n_children: node.children.len(),
                    },
                });
                if expanded {
                    for c in &node.children {
                        rows.push(CatalogRow {
                            section,
                            kind: RowKind::Leaf {
                                name: c.clone(),
                                depth: 1,
                            },
                        });
                    }
                }
            }
        }
    }
    for PackageNode { name, path } in &tree.packages {
        rows.push(CatalogRow {
            section: PACKAGES,
            kind: RowKind::Package {
                name: name.clone(),
                path: path.clone(),
            },
        });
    }
    rows
}

/// What a key press does at (rows, active). ARIA-tree-core transition table —
/// see the design doc for the full matrix.
#[derive(Debug, Clone, PartialEq)]
pub enum NavAction {
    Move(usize),
    /// Flip this attach alias in the collapsed set.
    Toggle(String),
    /// Open this table into the main grid.
    Open(String),
    None,
}

pub fn tree_nav(rows: &[CatalogRow], active: usize, key: &str) -> NavAction {
    let Some(row) = rows.get(active) else {
        return NavAction::None;
    };
    match key {
        "down" => {
            if active + 1 < rows.len() {
                NavAction::Move(active + 1)
            } else {
                NavAction::None
            }
        }
        "up" => {
            if active > 0 {
                NavAction::Move(active - 1)
            } else {
                NavAction::None
            }
        }
        "right" => match &row.kind {
            RowKind::Parent {
                alias,
                expanded: false,
                ..
            } => NavAction::Toggle(alias.clone()),
            // Expanded parent: its first child is by construction the next row.
            RowKind::Parent { expanded: true, .. } => NavAction::Move(active + 1),
            RowKind::Leaf { .. } | RowKind::Package { .. } => NavAction::None,
        },
        "left" => match &row.kind {
            RowKind::Parent {
                alias,
                expanded: true,
                ..
            } => NavAction::Toggle(alias.clone()),
            RowKind::Leaf { depth: 1, .. } => {
                // Children always directly follow their parent, so the nearest
                // Parent row above is THE parent.
                match rows[..active]
                    .iter()
                    .rposition(|r| matches!(r.kind, RowKind::Parent { .. }))
                {
                    Some(p) => NavAction::Move(p),
                    None => NavAction::None,
                }
            }
            _ => NavAction::None,
        },
        "enter" | "space" => match &row.kind {
            RowKind::Parent { alias, .. } => NavAction::Toggle(alias.clone()),
            RowKind::Leaf { name, .. } => NavAction::Open(name.clone()),
            // Packages resolve through `package_activation`, not here — see its
            // doc for why they are not a `NavAction`.
            RowKind::Package { .. } => NavAction::None,
        },
        _ => NavAction::None,
    }
}

/// The package a keyboard activation at `active` should open, if any.
///
/// Package activation deliberately does NOT go through [`NavAction`].
/// `NavAction` is consumed by `WorkspaceShell::catalog_nav_key`, whose arms are
/// all shell-local transitions (`open_table_tab`, `toggle_catalog_parent`);
/// opening a package is an `App`-level flow that spawns a NEW read-only window
/// (`window::open_package_at`) and mutates nothing on this shell. Routing it
/// through a shell method would put a window-spawn behind a name that promises
/// a catalog-tree state change.
///
/// `catalog::panel::render_catalog`'s activate listener consults this first and
/// falls through to `catalog_nav_key` when it returns `None`, so mouse and
/// keyboard still resolve the row through the SAME [`visible_rows`] list — the
/// property that keeps the ring, the paint order and the activation target from
/// drifting apart.
pub fn package_activation(rows: &[CatalogRow], active: usize, key: &str) -> Option<PathBuf> {
    if !matches!(key, "enter" | "space") {
        return None;
    }
    match &rows.get(active)?.kind {
        RowKind::Package { path, .. } => Some(path.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// files: [parent "sq" → (a, b)], connections: [parent "md" → (c)],
    /// plus a top-level file leaf "t" and one package.
    fn tree() -> CatalogTree {
        CatalogTree {
            files: vec![
                CatalogNode {
                    name: "sq".into(),
                    schema: String::new(),
                    children: vec!["a".into(), "b".into()],
                },
                CatalogNode {
                    name: "t".into(),
                    schema: "main".into(),
                    children: vec![],
                },
            ],
            connections: vec![CatalogNode {
                name: "md".into(),
                schema: String::new(),
                children: vec!["c".into()],
            }],
            packages: vec![PackageNode {
                name: "run.dat0".into(),
                path: PathBuf::from("/tmp/run.dat0"),
            }],
        }
    }

    fn names(rows: &[CatalogRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match &r.kind {
                RowKind::Parent { alias, .. } => format!("P:{alias}"),
                RowKind::Leaf { name, depth } => format!("L{depth}:{name}"),
                RowKind::Package { name, .. } => format!("K:{name}"),
            })
            .collect()
    }

    #[test]
    fn expanded_tree_flattens_in_paint_order() {
        let rows = visible_rows(&tree(), &HashSet::new());
        assert_eq!(
            names(&rows),
            vec!["P:sq", "L1:a", "L1:b", "L0:t", "P:md", "L1:c", "K:run.dat0"]
        );
        // Groups paint FILES → CONNECTIONS → PACKAGES, and the row list carries
        // the section id so the renderer can slice it without re-deriving.
        assert_eq!(rows[0].section, FILES);
        assert_eq!(rows[4].section, CONNECTIONS);
        assert_eq!(rows[6].section, PACKAGES);
    }

    #[test]
    fn collapse_hides_exactly_that_parents_children() {
        let collapsed: HashSet<String> = ["sq".to_string()].into();
        let rows = visible_rows(&tree(), &collapsed);
        assert_eq!(
            names(&rows),
            vec!["P:sq", "L0:t", "P:md", "L1:c", "K:run.dat0"]
        );
        assert!(matches!(
            rows[0].kind,
            RowKind::Parent {
                expanded: false,
                ..
            }
        ));
    }

    #[test]
    fn up_down_move_and_clamp() {
        let rows = visible_rows(&tree(), &HashSet::new()); // 7 rows
        assert_eq!(tree_nav(&rows, 0, "down"), NavAction::Move(1));
        assert_eq!(tree_nav(&rows, 6, "down"), NavAction::None);
        assert_eq!(tree_nav(&rows, 1, "up"), NavAction::Move(0));
        assert_eq!(tree_nav(&rows, 0, "up"), NavAction::None);
    }

    #[test]
    fn right_expands_or_steps_into_first_child() {
        let rows = visible_rows(&tree(), &HashSet::new());
        // expanded parent → first child (the next row)
        assert_eq!(tree_nav(&rows, 0, "right"), NavAction::Move(1));
        // leaf → None
        assert_eq!(tree_nav(&rows, 3, "right"), NavAction::None);
        // package → None (it has no children to step into)
        assert_eq!(tree_nav(&rows, 6, "right"), NavAction::None);
        // collapsed parent → expand
        let collapsed: HashSet<String> = ["sq".to_string()].into();
        let rows = visible_rows(&tree(), &collapsed);
        assert_eq!(tree_nav(&rows, 0, "right"), NavAction::Toggle("sq".into()));
    }

    #[test]
    fn left_collapses_or_jumps_to_parent() {
        let rows = visible_rows(&tree(), &HashSet::new());
        // expanded parent → collapse
        assert_eq!(tree_nav(&rows, 0, "left"), NavAction::Toggle("sq".into()));
        // child (row 2 = L1:b) → its parent (row 0)
        assert_eq!(tree_nav(&rows, 2, "left"), NavAction::Move(0));
        // child of the SECOND parent (row 5 = L1:c) → row 4, not row 0
        assert_eq!(tree_nav(&rows, 5, "left"), NavAction::Move(4));
        // top-level leaf → None
        assert_eq!(tree_nav(&rows, 3, "left"), NavAction::None);
        // package → None
        assert_eq!(tree_nav(&rows, 6, "left"), NavAction::None);
    }

    #[test]
    fn left_on_collapsed_parent_is_none() {
        let collapsed: HashSet<String> = ["sq".to_string()].into();
        let rows = visible_rows(&tree(), &collapsed);
        // row 0 is the collapsed "sq" parent; ARIA: a collapsed parent has no
        // parent to jump to.
        assert_eq!(tree_nav(&rows, 0, "left"), NavAction::None);
    }

    #[test]
    fn enter_toggles_parents_and_opens_leaves() {
        let rows = visible_rows(&tree(), &HashSet::new());
        assert_eq!(tree_nav(&rows, 0, "enter"), NavAction::Toggle("sq".into()));
        assert_eq!(tree_nav(&rows, 1, "enter"), NavAction::Open("a".into()));
        assert_eq!(tree_nav(&rows, 3, "space"), NavAction::Open("t".into()));
    }

    #[test]
    fn package_rows_activate_out_of_band_not_through_nav_action() {
        let rows = visible_rows(&tree(), &HashSet::new());
        // A package must NEVER reach `open_table_tab` — "run.dat0" is not a
        // table name and the grid would fail to bind it.
        assert_eq!(tree_nav(&rows, 6, "enter"), NavAction::None);
        assert_eq!(
            package_activation(&rows, 6, "enter"),
            Some(PathBuf::from("/tmp/run.dat0"))
        );
        assert_eq!(package_activation(&rows, 6, "space"), rows_path());
        // Non-activation keys and non-package rows yield nothing.
        assert_eq!(package_activation(&rows, 6, "down"), None);
        assert_eq!(package_activation(&rows, 1, "enter"), None);
        assert_eq!(package_activation(&rows, 99, "enter"), None);
    }

    fn rows_path() -> Option<PathBuf> {
        Some(PathBuf::from("/tmp/run.dat0"))
    }

    #[test]
    fn out_of_range_and_unknown_keys_are_none() {
        let rows = visible_rows(&tree(), &HashSet::new());
        assert_eq!(tree_nav(&rows, 99, "down"), NavAction::None);
        assert_eq!(tree_nav(&[], 0, "down"), NavAction::None);
        assert_eq!(tree_nav(&rows, 0, "escape"), NavAction::None);
    }
}
