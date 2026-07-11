//! Pure catalog tree model (P6a): a grouping of TableInfo by origin + search.
use dat0_engine::{TableInfo, TableOrigin};

#[derive(Debug, Clone, PartialEq)]
pub struct CatalogNode {
    pub name: String,
    pub schema: String,
    /// For attached-DB source nodes: the tables inside, by name.
    pub children: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CatalogTree {
    pub sources: Vec<CatalogNode>, // File + non-md Attached (SQLite/file)
    pub tables: Vec<CatalogNode>,  // local base (Derived::Sql with empty/origin-less) — see note
    pub derived: Vec<CatalogNode>, // Derived::Transform / non-empty Sql
    pub cloud: Vec<CatalogNode>,   // MotherDuck-attached (source starts "md:")
}

impl CatalogTree {
    pub fn build(tables: &[TableInfo]) -> Self {
        let mut tree = CatalogTree::default();
        // Attached tables group by alias into one PARENT node per attach
        // (catalog-tree slice): parent.name = alias, children = table names.
        // (alias, source, children), first-seen order; sorted below.
        let mut attaches: Vec<(String, String, Vec<String>)> = Vec::new();
        for ti in tables {
            let leaf = CatalogNode {
                name: ti.name.clone(),
                schema: ti.schema.clone(),
                children: vec![],
            };
            match &ti.origin {
                TableOrigin::File(_) => tree.sources.push(leaf),
                TableOrigin::Attached { alias, source } => {
                    match attaches.iter_mut().find(|(a, _, _)| a == alias) {
                        Some((_, _, kids)) => kids.push(ti.name.clone()),
                        None => {
                            attaches.push((alias.clone(), source.clone(), vec![ti.name.clone()]))
                        }
                    }
                }
                TableOrigin::Derived(d) => match d {
                    dat0_engine::DerivedOrigin::Transform { .. } => tree.derived.push(leaf),
                    dat0_engine::DerivedOrigin::Sql(s) if !s.is_empty() => tree.derived.push(leaf),
                    _ => tree.tables.push(leaf),
                },
            }
        }
        for (alias, source, mut kids) in attaches {
            kids.sort();
            let parent = CatalogNode {
                name: alias,
                schema: String::new(),
                children: kids,
            };
            // MotherDuck attaches record `source = "md:…"` (duckdb_engine.rs:721-730);
            // Cloud ⇔ md: prefix, applied to the PARENT (rule unchanged from flat).
            if source.starts_with("md:") {
                tree.cloud.push(parent);
            } else {
                tree.sources.push(parent);
            }
        }
        // Deterministic paint/nav order: every section sorted by node name
        // (parents sort by alias among the leaves).
        for sec in [
            &mut tree.sources,
            &mut tree.cloud,
            &mut tree.tables,
            &mut tree.derived,
        ] {
            sec.sort_by(|a, b| a.name.cmp(&b.name));
        }
        tree
    }

    /// Token-AND filter over node names (case-insensitive). Leaves survive if
    /// every whitespace token is a substring of their lowercased name. Parents
    /// (attach nodes with children): an ALIAS match keeps the parent with ALL
    /// children; otherwise the children are filtered and the parent survives
    /// iff any child matched.
    pub fn filter(mut self, query: &str) -> Self {
        let toks: Vec<String> = query.split_whitespace().map(|s| s.to_lowercase()).collect();
        if toks.is_empty() {
            return self;
        }
        let matches = |name: &str| {
            let lc = name.to_lowercase();
            toks.iter().all(|t| lc.contains(t.as_str()))
        };
        let keep = |n: &mut CatalogNode| {
            if n.children.is_empty() {
                return matches(&n.name);
            }
            if matches(&n.name) {
                return true; // alias match keeps all children
            }
            n.children.retain(|c| matches(c));
            !n.children.is_empty()
        };
        self.sources.retain_mut(keep);
        self.tables.retain_mut(keep);
        self.derived.retain_mut(keep);
        self.cloud.retain_mut(keep);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::{DerivedOrigin, TableInfo, TableOrigin};
    use std::path::PathBuf;

    fn t(name: &str, origin: TableOrigin) -> TableInfo {
        TableInfo {
            name: name.into(),
            schema: "main".into(),
            columns: vec![],
            row_count_estimate: None,
            origin,
        }
    }

    #[test]
    fn groups_by_origin() {
        let tables = vec![
            t("sales", TableOrigin::File(PathBuf::from("/s.csv"))),
            t(
                "orders",
                TableOrigin::Derived(DerivedOrigin::Sql(String::new())),
            ),
            t(
                "orders_open",
                TableOrigin::Derived(DerivedOrigin::Transform {
                    parent: "orders".into(),
                    ops: vec![],
                }),
            ),
            t(
                "md_events",
                TableOrigin::Attached {
                    alias: "md".into(),
                    source: "md:".into(),
                },
            ),
        ];
        let tree = CatalogTree::build(&tables);
        // File stays a flat Sources leaf; the md: attach becomes a Cloud PARENT
        // named by its alias, with the table as a child.
        assert_eq!(tree.sources.len(), 1, "only the file source in Sources");
        assert!(tree.sources.iter().any(|n| n.name == "sales"));
        assert!(tree.tables.iter().any(|n| n.name == "orders"));
        assert!(tree.derived.iter().any(|n| n.name == "orders_open"));
        assert_eq!(tree.cloud.len(), 1);
        assert_eq!(tree.cloud[0].name, "md", "cloud node is the attach ALIAS");
        assert_eq!(tree.cloud[0].children, vec!["md_events".to_string()]);
    }

    #[test]
    fn motherduck_attaches_group_under_cloud_sqlite_stays_sources() {
        let tables = vec![
            t("sales", TableOrigin::File(PathBuf::from("/s.csv"))),
            t(
                "md_events",
                TableOrigin::Attached {
                    alias: "sample_data".into(),
                    source: "md:".into(),
                },
            ),
            t(
                "local_sqlite",
                TableOrigin::Attached {
                    alias: "sq".into(),
                    source: "/tmp/x.db".into(),
                },
            ),
        ];
        let tree = CatalogTree::build(&tables);
        assert_eq!(tree.cloud.len(), 1, "only the md: attach is Cloud");
        assert_eq!(tree.cloud[0].name, "sample_data");
        assert_eq!(tree.cloud[0].children, vec!["md_events".to_string()]);
        // Sources holds the file leaf + the sqlite attach PARENT (sorted by name).
        assert_eq!(tree.sources.len(), 2);
        assert!(
            tree.sources
                .iter()
                .any(|n| n.name == "sales" && n.children.is_empty())
        );
        assert!(
            tree.sources
                .iter()
                .any(|n| n.name == "sq" && n.children == vec!["local_sqlite".to_string()])
        );
    }

    #[test]
    fn cloud_group_respects_token_and_search() {
        let tables = vec![
            t(
                "md_orders",
                TableOrigin::Attached {
                    alias: "db".into(),
                    source: "md:".into(),
                },
            ),
            t(
                "md_events",
                TableOrigin::Attached {
                    alias: "db".into(),
                    source: "md:".into(),
                },
            ),
        ];
        // Child-match: the parent survives with ONLY the matching child.
        let tree = CatalogTree::build(&tables).filter("ord");
        assert_eq!(tree.cloud.len(), 1);
        assert_eq!(tree.cloud[0].name, "db");
        assert_eq!(tree.cloud[0].children, vec!["md_orders".to_string()]);
    }

    #[test]
    fn same_alias_tables_group_under_one_parent_sorted() {
        let tables = vec![
            t(
                "zeta",
                TableOrigin::Attached {
                    alias: "sq".into(),
                    source: "/tmp/x.db".into(),
                },
            ),
            t(
                "alpha",
                TableOrigin::Attached {
                    alias: "sq".into(),
                    source: "/tmp/x.db".into(),
                },
            ),
        ];
        let tree = CatalogTree::build(&tables);
        assert_eq!(tree.sources.len(), 1, "one parent per alias");
        assert_eq!(
            tree.sources[0].children,
            vec!["alpha".to_string(), "zeta".to_string()],
            "children sorted by name"
        );
    }

    #[test]
    fn filter_alias_match_keeps_all_children() {
        let tables = vec![
            t(
                "md_orders",
                TableOrigin::Attached {
                    alias: "warehouse".into(),
                    source: "md:".into(),
                },
            ),
            t(
                "md_events",
                TableOrigin::Attached {
                    alias: "warehouse".into(),
                    source: "md:".into(),
                },
            ),
        ];
        let tree = CatalogTree::build(&tables).filter("ware");
        assert_eq!(tree.cloud.len(), 1);
        assert_eq!(
            tree.cloud[0].children.len(),
            2,
            "alias match keeps ALL children"
        );
    }

    #[test]
    fn filter_no_match_drops_the_parent() {
        let tables = vec![t(
            "md_orders",
            TableOrigin::Attached {
                alias: "db".into(),
                source: "md:".into(),
            },
        )];
        let tree = CatalogTree::build(&tables).filter("zzz");
        assert!(tree.cloud.is_empty());
    }

    #[test]
    fn token_and_search_filters() {
        let tables = vec![
            t(
                "daily_revenue",
                TableOrigin::Derived(DerivedOrigin::Sql(String::new())),
            ),
            t(
                "orders",
                TableOrigin::Derived(DerivedOrigin::Sql(String::new())),
            ),
        ];
        let tree = CatalogTree::build(&tables).filter("dai rev");
        assert_eq!(tree.tables.len(), 1);
        assert_eq!(tree.tables[0].name, "daily_revenue");
    }
}
