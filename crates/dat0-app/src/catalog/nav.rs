//! Pure keyboard-nav model for the Catalog panel (catalog-tree slice).
//!
//! `visible_rows` flattens the [`CatalogTree`] + a collapsed-alias set into the
//! exact row list the panel paints, in paint order — the SINGLE source of truth
//! for both the render iteration and the keyboard active-index (ring, arrows,
//! Enter and painted rows derive from one Vec, so they cannot drift). `tree_nav`
//! maps (rows, active, key) to a [`NavAction`] — the pure-fn → enum → thin-match
//! idiom (`resolve_relaunch_action` precedent), unit-testable without GPUI.

use std::collections::HashSet;

use crate::catalog::{CatalogNode, CatalogTree};

/// One visible catalog row. `section` is the panel's stable section id
/// ("Sources" / "Cloud" / "Tables" / "Derived").
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogRow {
    pub section: &'static str,
    pub kind: RowKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowKind {
    /// An attached-DB parent node (name = attach alias).
    Parent {
        alias: String,
        expanded: bool,
        n_children: usize,
    },
    /// A table row. `depth` 0 = top-level leaf, 1 = child of the preceding
    /// Parent (children always immediately follow their parent, so `tree_nav`
    /// finds the parent by scanning backward for the nearest Parent row).
    Leaf { name: String, depth: u8 },
}

/// Every VISIBLE catalog row in paint order. Children of collapsed parents are
/// absent. Section headers are NOT rows (non-interactive; nav skips them).
pub fn visible_rows(tree: &CatalogTree, collapsed: &HashSet<String>) -> Vec<CatalogRow> {
    let sections: [(&'static str, &Vec<CatalogNode>); 4] = [
        ("Sources", &tree.sources),
        ("Cloud", &tree.cloud),
        ("Tables", &tree.tables),
        ("Derived", &tree.derived),
    ];
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
            RowKind::Leaf { .. } => NavAction::None,
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
        },
        _ => NavAction::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// sources: [parent "sq" → (a, b)], cloud: [parent "md" → (c)],
    /// tables: [leaf "t"].
    fn tree() -> CatalogTree {
        CatalogTree {
            sources: vec![CatalogNode {
                name: "sq".into(),
                schema: String::new(),
                children: vec!["a".into(), "b".into()],
            }],
            cloud: vec![CatalogNode {
                name: "md".into(),
                schema: String::new(),
                children: vec!["c".into()],
            }],
            tables: vec![CatalogNode {
                name: "t".into(),
                schema: "main".into(),
                children: vec![],
            }],
            derived: vec![],
        }
    }

    fn names(rows: &[CatalogRow]) -> Vec<String> {
        rows.iter()
            .map(|r| match &r.kind {
                RowKind::Parent { alias, .. } => format!("P:{alias}"),
                RowKind::Leaf { name, depth } => format!("L{depth}:{name}"),
            })
            .collect()
    }

    #[test]
    fn expanded_tree_flattens_in_paint_order() {
        let rows = visible_rows(&tree(), &HashSet::new());
        assert_eq!(
            names(&rows),
            vec!["P:sq", "L1:a", "L1:b", "P:md", "L1:c", "L0:t"]
        );
    }

    #[test]
    fn collapse_hides_exactly_that_parents_children() {
        let collapsed: HashSet<String> = ["sq".to_string()].into();
        let rows = visible_rows(&tree(), &collapsed);
        assert_eq!(names(&rows), vec!["P:sq", "P:md", "L1:c", "L0:t"]);
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
        let rows = visible_rows(&tree(), &HashSet::new()); // 6 rows
        assert_eq!(tree_nav(&rows, 0, "down"), NavAction::Move(1));
        assert_eq!(tree_nav(&rows, 5, "down"), NavAction::None);
        assert_eq!(tree_nav(&rows, 1, "up"), NavAction::Move(0));
        assert_eq!(tree_nav(&rows, 0, "up"), NavAction::None);
    }

    #[test]
    fn right_expands_or_steps_into_first_child() {
        let rows = visible_rows(&tree(), &HashSet::new());
        // expanded parent → first child (the next row)
        assert_eq!(tree_nav(&rows, 0, "right"), NavAction::Move(1));
        // leaf → None
        assert_eq!(tree_nav(&rows, 5, "right"), NavAction::None);
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
        // child of the SECOND parent (row 4 = L1:c) → row 3, not row 0
        assert_eq!(tree_nav(&rows, 4, "left"), NavAction::Move(3));
        // top-level leaf → None
        assert_eq!(tree_nav(&rows, 5, "left"), NavAction::None);
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
        assert_eq!(tree_nav(&rows, 5, "space"), NavAction::Open("t".into()));
    }

    #[test]
    fn out_of_range_and_unknown_keys_are_none() {
        let rows = visible_rows(&tree(), &HashSet::new());
        assert_eq!(tree_nav(&rows, 99, "down"), NavAction::None);
        assert_eq!(tree_nav(&[], 0, "down"), NavAction::None);
        assert_eq!(tree_nav(&rows, 0, "escape"), NavAction::None);
    }
}
