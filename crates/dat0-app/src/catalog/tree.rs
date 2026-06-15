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
        for ti in tables {
            let node = CatalogNode {
                name: ti.name.clone(),
                schema: ti.schema.clone(),
                children: vec![],
            };
            match &ti.origin {
                TableOrigin::File(_) => tree.sources.push(node),
                // MotherDuck attaches record `source = "md:"`
                // (duckdb_engine.rs:721-730); SQLite/file attaches record the
                // file path. Cloud ⇔ md: prefix (covers a future `md:dbname`).
                TableOrigin::Attached { source, .. } if source.starts_with("md:") => {
                    tree.cloud.push(node)
                }
                TableOrigin::Attached { .. } => tree.sources.push(node),
                TableOrigin::Derived(d) => match d {
                    dat0_engine::DerivedOrigin::Transform { .. } => tree.derived.push(node),
                    dat0_engine::DerivedOrigin::Sql(s) if !s.is_empty() => tree.derived.push(node),
                    _ => tree.tables.push(node),
                },
            }
        }
        tree
    }

    /// Token-AND filter over node names (case-insensitive). A node survives if
    /// every whitespace token is a substring of its lowercased name.
    pub fn filter(mut self, query: &str) -> Self {
        let toks: Vec<String> = query.split_whitespace().map(|s| s.to_lowercase()).collect();
        if toks.is_empty() {
            return self;
        }
        let keep = |n: &CatalogNode| {
            let lc = n.name.to_lowercase();
            toks.iter().all(|t| lc.contains(t.as_str()))
        };
        self.sources.retain(&keep);
        self.tables.retain(&keep);
        self.derived.retain(&keep);
        self.cloud.retain(&keep);
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
        // File stays under Sources; the md: attach now lands under Cloud.
        assert_eq!(
            tree.sources.len(),
            1,
            "only the file source remains in Sources"
        );
        assert!(tree.sources.iter().any(|n| n.name == "sales"));
        assert!(tree.tables.iter().any(|n| n.name == "orders"));
        assert!(tree.derived.iter().any(|n| n.name == "orders_open"));
        assert!(tree.cloud.iter().any(|n| n.name == "md_events"));
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
        assert_eq!(tree.cloud[0].name, "md_events");
        // file + sqlite attach stay under Sources; md does NOT.
        assert!(tree.sources.iter().any(|n| n.name == "sales"));
        assert!(tree.sources.iter().any(|n| n.name == "local_sqlite"));
        assert!(!tree.sources.iter().any(|n| n.name == "md_events"));
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
        let tree = CatalogTree::build(&tables).filter("ord");
        assert_eq!(tree.cloud.len(), 1);
        assert_eq!(tree.cloud[0].name, "md_orders");
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
