//! Pure lineage graph for the Inspector (P6b). Built from the catalog's table
//! origins plus a precomputed `sql_parents` map (Sql-table → referenced base
//! tables, resolved off-thread by the engine in `WorkspaceShell::refresh_catalog`).
//! No engine/GPUI dependency here, so it is fully unit-testable.
use dat0_engine::{DerivedOrigin, TableInfo, TableOrigin};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NodeKey {
    Table(String),
    File(String),
    External(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Table,
    File,
    External,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    FileImport,
    Transform(usize), // op count (label rendered as "transform (N ops)")
    SqlRef,
}

/// One rendered row in the chain (an ancestor or a descendant of the target).
// Consumed by `closure()` in the next P6b task; not yet referenced here.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainStep {
    pub label: String, // display text (table name / file basename / external name)
    pub kind: NodeKind,
    pub edge: EdgeKind,            // edge connecting this node toward the target
    pub depth: u32,                // 1-based distance from the target
    pub open_name: Option<String>, // table to open on click; None for File leaves
}

// Consumed by `closure()` in the next P6b task; not yet referenced here.
#[allow(dead_code)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineageChain {
    pub ancestors: Vec<ChainStep>,   // ordered root → … → immediate parent
    pub descendants: Vec<ChainStep>, // ordered immediate child → … → leaf
}

pub struct LineageGraph {
    // `parents` is unused until `closure()` walks it in the next P6b task.
    #[allow(dead_code)]
    parents: HashMap<NodeKey, Vec<(NodeKey, EdgeKind)>>,
    children: HashMap<NodeKey, Vec<(NodeKey, EdgeKind)>>,
    kind: HashMap<NodeKey, NodeKind>,
}

impl LineageGraph {
    pub fn build(tables: &[TableInfo], sql_parents: &HashMap<String, Vec<String>>) -> Self {
        let known: HashSet<&str> = tables.iter().map(|t| t.name.as_str()).collect();
        let mut g = LineageGraph {
            parents: HashMap::new(),
            children: HashMap::new(),
            kind: HashMap::new(),
        };

        for t in tables {
            let node = NodeKey::Table(t.name.clone());
            g.kind.entry(node.clone()).or_insert(NodeKind::Table);
            match &t.origin {
                TableOrigin::File(path) => {
                    let f = NodeKey::File(path.to_string_lossy().into_owned());
                    g.kind.insert(f.clone(), NodeKind::File);
                    g.add_edge(f, node, EdgeKind::FileImport);
                }
                TableOrigin::Derived(DerivedOrigin::Transform { parent, ops }) => {
                    if known.contains(parent.as_str()) {
                        g.add_edge(
                            NodeKey::Table(parent.clone()),
                            node,
                            EdgeKind::Transform(ops.len()),
                        );
                    }
                }
                TableOrigin::Derived(DerivedOrigin::Sql(_)) => {
                    if let Some(parents) = sql_parents.get(&t.name) {
                        for p in parents {
                            if known.contains(p.as_str()) {
                                g.add_edge(
                                    NodeKey::Table(p.clone()),
                                    node.clone(),
                                    EdgeKind::SqlRef,
                                );
                            }
                        }
                    }
                }
                TableOrigin::Attached { .. } => {
                    g.kind.insert(node, NodeKind::External);
                }
            }
        }
        g
    }

    fn add_edge(&mut self, from: NodeKey, to: NodeKey, edge: EdgeKind) {
        self.children
            .entry(from.clone())
            .or_default()
            .push((to.clone(), edge.clone()));
        self.parents.entry(to).or_default().push((from, edge));
    }

    #[cfg(test)]
    fn has_edge(&self, from: &NodeKey, to: &NodeKey) -> bool {
        self.children
            .get(from)
            .is_some_and(|v| v.iter().any(|(n, _)| n == to))
    }

    #[cfg(test)]
    fn kind_of(&self, n: &NodeKey) -> Option<NodeKind> {
        self.kind.get(n).copied()
    }
}

/// Display label for a node key given its kind.
// Used by `closure()` in the next P6b task; not yet referenced here.
#[allow(dead_code)]
fn label_for(key: &NodeKey) -> String {
    match key {
        NodeKey::Table(n) | NodeKey::External(n) => n.clone(),
        NodeKey::File(p) => p.rsplit(['/', '\\']).next().unwrap_or(p).to_string(),
    }
}

/// `Some(name)` to open on click; File leaves are not openable.
// Used by `closure()` in the next P6b task; not yet referenced here.
#[allow(dead_code)]
fn open_name_for(key: &NodeKey) -> Option<String> {
    match key {
        NodeKey::Table(n) | NodeKey::External(n) => Some(n.clone()),
        NodeKey::File(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::{DerivedOrigin, TableInfo, TableOrigin};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn tbl(name: &str, origin: TableOrigin) -> TableInfo {
        TableInfo {
            name: name.into(),
            schema: "main".into(),
            columns: vec![],
            row_count_estimate: None,
            origin,
        }
    }

    #[test]
    fn build_indexes_all_edge_kinds() {
        let tables = vec![
            tbl("sales", TableOrigin::File(PathBuf::from("/data/sales.csv"))),
            tbl(
                "sales_open",
                TableOrigin::Derived(DerivedOrigin::Transform {
                    parent: "sales".into(),
                    ops: vec![],
                }),
            ),
            tbl(
                "revenue",
                TableOrigin::Derived(DerivedOrigin::Sql("…".into())),
            ),
            tbl(
                "ext",
                TableOrigin::Attached {
                    alias: "md".into(),
                    source: "md:db".into(),
                },
            ),
        ];
        let mut sql_parents = HashMap::new();
        sql_parents.insert("revenue".to_string(), vec!["sales_open".to_string()]);

        let g = LineageGraph::build(&tables, &sql_parents);
        // sales_open's parent is the File node for sales (via sales -> File edge chain)
        assert!(g.has_edge(
            &NodeKey::Table("sales".into()),
            &NodeKey::Table("sales_open".into())
        ));
        assert!(g.has_edge(
            &NodeKey::File("/data/sales.csv".into()),
            &NodeKey::Table("sales".into())
        ));
        assert!(g.has_edge(
            &NodeKey::Table("sales_open".into()),
            &NodeKey::Table("revenue".into())
        ));
        assert_eq!(
            g.kind_of(&NodeKey::Table("ext".into())),
            Some(NodeKind::External)
        );
        assert_eq!(
            g.kind_of(&NodeKey::File("/data/sales.csv".into())),
            Some(NodeKind::File)
        );
    }
}
