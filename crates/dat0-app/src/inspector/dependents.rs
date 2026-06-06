//! Reverse-lineage dependents for the Inspector (P6a T11). Forward lineage = P6b.
use dat0_engine::{DerivedOrigin, TableInfo, TableOrigin};

/// Tables whose origin is a Transform with `parent == table`. Best-effort:
/// `Derived::Sql` references are NOT matched in P6a (P6b formalizes lineage).
pub fn dependents_of(table: &str, tables: &[TableInfo]) -> Vec<String> {
    tables
        .iter()
        .filter_map(|t| match &t.origin {
            TableOrigin::Derived(DerivedOrigin::Transform { parent, .. }) if parent == table => {
                Some(t.name.clone())
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::{DerivedOrigin, TableInfo, TableOrigin};

    fn t(name: &str, o: TableOrigin) -> TableInfo {
        TableInfo {
            name: name.into(),
            schema: "main".into(),
            columns: vec![],
            row_count_estimate: None,
            origin: o,
        }
    }

    #[test]
    fn lists_transform_children() {
        let tables = vec![
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
                "rev",
                TableOrigin::Derived(DerivedOrigin::Transform {
                    parent: "orders".into(),
                    ops: vec![],
                }),
            ),
            t(
                "other",
                TableOrigin::Derived(DerivedOrigin::Transform {
                    parent: "customers".into(),
                    ops: vec![],
                }),
            ),
        ];
        let deps = dependents_of("orders", &tables);
        assert_eq!(deps, vec!["orders_open".to_string(), "rev".to_string()]);
    }
}
