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
    Chart(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Table,
    File,
    External,
    Chart,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    FileImport,
    Transform(usize), // op count (label rendered as "transform (N ops)")
    SqlRef,
    Chart,
}

/// A saved chart to inject into the graph as a descendant of its (bare) source
/// table. `source_table` is already reduced to a bare name by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartNode {
    pub name: String,
    pub source_table: String,
}

/// One rendered row in the chain (an ancestor or a descendant of the target).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainStep {
    pub label: String, // display text (table name / file basename / external name)
    pub kind: NodeKind,
    pub edge: EdgeKind,            // edge connecting this node toward the target
    pub depth: u32,                // 1-based distance from the target
    pub open_name: Option<String>, // table to open on click; None for File leaves
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineageChain {
    pub ancestors: Vec<ChainStep>,   // ordered root → … → immediate parent
    pub descendants: Vec<ChainStep>, // ordered immediate child → … → leaf
}

pub struct LineageGraph {
    parents: HashMap<NodeKey, Vec<(NodeKey, EdgeKind)>>,
    children: HashMap<NodeKey, Vec<(NodeKey, EdgeKind)>>,
    kind: HashMap<NodeKey, NodeKind>,
}

impl LineageGraph {
    pub fn build(
        tables: &[TableInfo],
        sql_parents: &HashMap<String, Vec<String>>,
        charts: &[ChartNode],
    ) -> Self {
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

        // Inject saved charts (P9a-2) as descendants of their source table — but
        // only when that source is a known node. A chart whose source is absent
        // (dropped table, attached-DB collision) is silently skipped.
        for c in charts {
            if known.contains(c.source_table.as_str()) {
                let cn = NodeKey::Chart(c.name.clone());
                g.kind.insert(cn.clone(), NodeKind::Chart);
                g.add_edge(NodeKey::Table(c.source_table.clone()), cn, EdgeKind::Chart);
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

    /// Full transitive closure around `target`: all ancestors (up to roots) and
    /// all descendants (down to leaves). Cycle-guarded; deterministic ordering.
    pub fn closure(&self, target: &str) -> LineageChain {
        let start = NodeKey::Table(target.to_string());

        // Ancestors: BFS up the `parents` map. Collect (node, edge, depth).
        let mut ancestors = self.walk(&start, &self.parents);
        // Roots first → depth descending, then by label for determinism.
        ancestors.sort_by(|a, b| b.depth.cmp(&a.depth).then(a.label.cmp(&b.label)));

        // Descendants: BFS down the `children` map. Immediate first → depth asc.
        let mut descendants = self.walk(&start, &self.children);
        descendants.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.label.cmp(&b.label)));

        LineageChain {
            ancestors,
            descendants,
        }
    }

    fn walk(
        &self,
        start: &NodeKey,
        adj: &HashMap<NodeKey, Vec<(NodeKey, EdgeKind)>>,
    ) -> Vec<ChainStep> {
        let mut out = Vec::new();
        let mut visited: HashSet<NodeKey> = HashSet::from([start.clone()]);
        let mut frontier: Vec<(NodeKey, u32)> = vec![(start.clone(), 0)];
        while let Some((node, depth)) = frontier.pop() {
            for (next, edge) in adj.get(&node).into_iter().flatten() {
                if !visited.insert(next.clone()) {
                    continue; // cycle / diamond guard
                }
                out.push(ChainStep {
                    label: label_for(next),
                    kind: self.kind.get(next).copied().unwrap_or(NodeKind::Table),
                    edge: edge.clone(),
                    depth: depth + 1,
                    open_name: open_name_for(next),
                });
                frontier.push((next.clone(), depth + 1));
            }
        }
        out
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
fn label_for(key: &NodeKey) -> String {
    match key {
        NodeKey::Table(n) | NodeKey::External(n) | NodeKey::Chart(n) => n.clone(),
        NodeKey::File(p) => p.rsplit(['/', '\\']).next().unwrap_or(p).to_string(),
    }
}

/// `Some(name)` to open on click; File leaves are not openable. Charts ARE
/// openable — the panel routes a chart click to `open_saved_chart` by kind.
fn open_name_for(key: &NodeKey) -> Option<String> {
    match key {
        NodeKey::Table(n) | NodeKey::External(n) | NodeKey::Chart(n) => Some(n.clone()),
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

    fn sales_chain() -> (Vec<TableInfo>, HashMap<String, Vec<String>>) {
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
        ];
        let mut sp = HashMap::new();
        sp.insert("revenue".to_string(), vec!["sales_open".to_string()]);
        (tables, sp)
    }

    #[test]
    fn chart_attaches_as_descendant_of_source() {
        let tables = vec![tbl(
            "sales",
            TableOrigin::File(PathBuf::from("/d/sales.csv")),
        )];
        let charts = vec![ChartNode {
            name: "Bar of sales".into(),
            source_table: "sales".into(),
        }];
        let g = LineageGraph::build(&tables, &HashMap::new(), &charts);
        let c = g.closure("sales");
        assert_eq!(
            c.descendants
                .iter()
                .map(|s| s.label.clone())
                .collect::<Vec<_>>(),
            vec!["Bar of sales".to_string()]
        );
        assert_eq!(c.descendants[0].kind, NodeKind::Chart);
        assert_eq!(c.descendants[0].open_name, Some("Bar of sales".to_string()));
        assert_eq!(c.descendants[0].edge, EdgeKind::Chart);
    }

    #[test]
    fn chart_with_absent_source_is_skipped() {
        // A chart whose source table is not a known node must not crash build and
        // must produce no descendant edge.
        let tables = vec![tbl(
            "sales",
            TableOrigin::File(PathBuf::from("/d/sales.csv")),
        )];
        let charts = vec![ChartNode {
            name: "Orphan".into(),
            source_table: "gone".into(),
        }];
        let g = LineageGraph::build(&tables, &HashMap::new(), &charts);
        let c = g.closure("sales");
        assert!(c.descendants.is_empty());
    }

    #[test]
    fn closure_walks_full_ancestry_and_descendants() {
        let (tables, sp) = sales_chain();
        let g = LineageGraph::build(&tables, &sp, &[]);
        let c = g.closure("sales_open");

        // ancestors: root file first → … (depth descending)
        assert_eq!(
            c.ancestors
                .iter()
                .map(|s| s.label.clone())
                .collect::<Vec<_>>(),
            vec!["sales.csv".to_string(), "sales".to_string()]
        );
        assert_eq!(c.ancestors[0].kind, NodeKind::File);
        assert_eq!(c.ancestors[0].open_name, None);

        // descendants: immediate child first (depth ascending)
        assert_eq!(
            c.descendants
                .iter()
                .map(|s| s.label.clone())
                .collect::<Vec<_>>(),
            vec!["revenue".to_string()]
        );
        assert_eq!(c.descendants[0].edge, EdgeKind::SqlRef);
        assert_eq!(c.descendants[0].open_name, Some("revenue".to_string()));
    }

    #[test]
    fn closure_is_cycle_safe() {
        // Two tables that (pathologically) reference each other via Sql.
        let tables = vec![
            tbl("a", TableOrigin::Derived(DerivedOrigin::Sql("…".into()))),
            tbl("b", TableOrigin::Derived(DerivedOrigin::Sql("…".into()))),
        ];
        let mut sp = HashMap::new();
        sp.insert("a".to_string(), vec!["b".to_string()]);
        sp.insert("b".to_string(), vec!["a".to_string()]);
        let g = LineageGraph::build(&tables, &sp, &[]);
        // Must terminate (visited-set guard), not loop forever.
        let c = g.closure("a");
        assert!(c.ancestors.len() <= 1 && c.descendants.len() <= 1);
    }

    #[test]
    fn closure_of_leaf_table_is_empty_both_ways() {
        let tables = vec![tbl(
            "lonely",
            TableOrigin::Derived(DerivedOrigin::Sql("…".into())),
        )];
        let g = LineageGraph::build(&tables, &HashMap::new(), &[]);
        let c = g.closure("lonely");
        assert!(c.ancestors.is_empty() && c.descendants.is_empty());
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

        let g = LineageGraph::build(&tables, &sql_parents, &[]);
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
