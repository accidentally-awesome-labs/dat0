//! P8 T5: pure-JSON `DiffEngine` unit tests.
//!
//! The diff compares two recipes + saved-query sets at the JSON level only —
//! NO engine, NO parquet read (row counts come from `RecipeTable.row_count`).
//! These cases drive `diff::compute` directly so they needn't build full zips.

use dat0_format::diff::*;
use dat0_format::*;

/// Build a `Base` `RecipeTable` named `name` with `rows` rows and the given
/// column names (all typed `BIGINT`). `kind` is `Base`, `derivation`/`source_ref`
/// are `None`, `id`/`data` are filled trivially.
fn rt(name: &str, rows: u64, cols: &[&str]) -> RecipeTable {
    RecipeTable {
        id: format!("t_{name}"),
        name: name.to_string(),
        kind: TableKind::Base,
        schema: cols
            .iter()
            .map(|c| ColumnFingerprint {
                name: c.to_string(),
                r#type: "BIGINT".to_string(),
            })
            .collect(),
        row_count: rows,
        data: format!("data/{name}.parquet"),
        source_ref: None,
        derivation: None,
    }
}

/// Build a saved query with a fixed UUID (queries are matched by name, not id).
fn pq(name: &str, sql: &str) -> PackageQuery {
    PackageQuery {
        id: uuid::Uuid::nil(),
        name: name.to_string(),
        sql: sql.to_string(),
        saved_at: 0,
    }
}

fn no_queries() -> Queries {
    Queries { queries: vec![] }
}

#[test]
fn identical_recipes_diff_empty() {
    let a = Recipe {
        tables: vec![rt("sales", 42, &["id"])],
    };
    let d = compute(&a, &no_queries(), &a.clone(), &no_queries());
    assert!(d.is_empty(), "identical recipes must produce an empty diff");
}

#[test]
fn row_count_delta_detected() {
    let a = Recipe {
        tables: vec![rt("sales", 42, &["id"])],
    };
    let b = Recipe {
        tables: vec![rt("sales", 50, &["id"])],
    };
    let d = compute(&a, &no_queries(), &b, &no_queries());
    assert!(!d.is_empty());
    assert_eq!(d.row_count_deltas.len(), 1);
    assert_eq!(d.row_count_deltas[0], ("sales".into(), 42, 50));
    // ONLY a row-count delta — nothing else changed.
    assert!(d.schema.is_empty());
    assert!(d.lineage.is_empty());
    assert!(d.queries.is_empty());
}

#[test]
fn schema_column_added_listed() {
    let a = Recipe {
        tables: vec![rt("sales", 42, &["id"])],
    };
    let b = Recipe {
        tables: vec![rt("sales", 42, &["id", "amount"])],
    };
    let d = compute(&a, &no_queries(), &b, &no_queries());
    assert!(!d.is_empty());
    assert_eq!(d.schema.len(), 1, "exactly one column-added schema delta");
    let delta = &d.schema[0];
    assert_eq!(delta.table, "sales");
    assert_eq!(delta.column, "amount");
    assert!(
        matches!(delta.change, ColumnChange::Added { .. }),
        "added column must be reported as Added, got {:?}",
        delta.change
    );
    // A pure schema addition is not a row-count or lineage change.
    assert!(d.row_count_deltas.is_empty());
    assert!(d.lineage.is_empty());
}

#[test]
fn dropped_table_is_lineage_delta() {
    let a = Recipe {
        tables: vec![rt("sales", 42, &["id"]), rt("monthly", 12, &["m"])],
    };
    let b = Recipe {
        tables: vec![rt("sales", 42, &["id"])],
    };
    let d = compute(&a, &no_queries(), &b, &no_queries());
    assert!(!d.is_empty());
    assert_eq!(
        d.lineage.len(),
        1,
        "exactly one lineage (table removed) delta"
    );
    let delta = &d.lineage[0];
    assert_eq!(delta.table, "monthly");
    assert!(
        matches!(delta.change, LineageChange::Removed),
        "dropped table must be Removed, got {:?}",
        delta.change
    );
}

#[test]
fn changed_query_sql_is_query_delta() {
    let a = Recipe { tables: vec![] };
    let qa = Queries {
        queries: vec![pq("top_sales", "SELECT * FROM sales LIMIT 10")],
    };
    let qb = Queries {
        queries: vec![pq("top_sales", "SELECT * FROM sales LIMIT 25")],
    };
    let d = compute(&a, &qa, &a.clone(), &qb);
    assert!(!d.is_empty());
    assert_eq!(d.queries.len(), 1, "exactly one query delta");
    let delta = &d.queries[0];
    assert_eq!(delta.name, "top_sales");
    assert!(
        matches!(delta.change, QueryChange::SqlChanged { .. }),
        "same-name changed-sql must be SqlChanged, got {:?}",
        delta.change
    );
}

#[test]
fn changed_derivation_is_lineage_delta() {
    // A table present on both sides whose derivation changed (Sql -> None) is a
    // changed-derivation lineage delta, NOT an add/remove.
    let mut a_tbl = rt("monthly", 12, &["m"]);
    a_tbl.kind = TableKind::Derived;
    a_tbl.derivation = Some(Derivation::Sql {
        sql: "SELECT * FROM sales".into(),
        parents: vec!["sales".into()],
    });
    let a = Recipe {
        tables: vec![a_tbl],
    };
    // Same table name, but now base (derivation dropped).
    let b = Recipe {
        tables: vec![rt("monthly", 12, &["m"])],
    };
    let d = compute(&a, &no_queries(), &b, &no_queries());
    assert!(!d.is_empty());
    assert_eq!(d.lineage.len(), 1);
    assert_eq!(d.lineage[0].table, "monthly");
    assert!(
        matches!(d.lineage[0].change, LineageChange::DerivationChanged { .. }),
        "changed derivation must be DerivationChanged, got {:?}",
        d.lineage[0].change
    );
    // Schema/row_count unchanged → no other deltas.
    assert!(d.schema.is_empty());
    assert!(d.row_count_deltas.is_empty());
}

#[test]
fn render_json_and_text_round_trip_shape() {
    // A non-empty diff renders to a JSON object with the four dimension keys and
    // a non-empty text summary (sanity that the renderers don't panic / are wired).
    let a = Recipe {
        tables: vec![rt("sales", 42, &["id"])],
    };
    let b = Recipe {
        tables: vec![rt("sales", 50, &["id", "amount"])],
    };
    let d = compute(&a, &no_queries(), &b, &no_queries());
    let json = d.render_json();
    assert!(json.get("schema").is_some());
    assert!(json.get("lineage").is_some());
    assert!(json.get("queries").is_some());
    assert!(json.get("row_count_deltas").is_some());
    let text = d.render_text();
    assert!(!text.is_empty());
    assert!(text.contains("sales"));
}
