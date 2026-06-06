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
    pub sources: Vec<CatalogNode>, // File + Attached
    pub tables: Vec<CatalogNode>,  // local base (Derived::Sql with empty/origin-less) — see note
    pub derived: Vec<CatalogNode>, // Derived::Transform / non-empty Sql
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
        assert_eq!(tree.sources.len(), 2, "file + attached are sources");
        assert!(tree.tables.iter().any(|n| n.name == "orders"));
        assert!(tree.derived.iter().any(|n| n.name == "orders_open"));
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
