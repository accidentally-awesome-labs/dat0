//! The schema the SQL editor completes against.
//!
//! This is *data*, not a ranking engine. Under GPUI, dat0 owned the whole
//! completion pipeline — extract the identifier under the cursor, filter, rank,
//! wrap the result in `lsp_types::CompletionItem`s for `gpui-component`'s
//! `CompletionProvider`. That existed because the widget library demanded a
//! provider, not because dat0 had an opinion about ranking.
//!
//! CodeMirror's `@codemirror/lang-sql` already does schema-qualified completion
//! properly — it understands aliases, `FROM` clauses and qualified names — so
//! the editor is handed a `table -> columns` map plus a function list and does
//! its own ranking. That deletes `completion_query`, `suggestions`, `rank`, and
//! the `lsp-types` and `ropey` dependencies.
//!
//! What survives is the snapshot: one per window, refreshed off
//! `engine.get_tables()`, shared by every console tab.

use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::Mutex;

/// A table and its columns, as autocomplete sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableEntry {
    pub name: String,
    pub columns: Vec<String>,
}

/// The cached schema the editor completes against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemaSnapshot {
    pub tables: Vec<TableEntry>,
    pub functions: Vec<String>,
}

impl SchemaSnapshot {
    /// The `{table: [columns]}` shape `@codemirror/lang-sql` takes.
    pub fn schema_map(&self) -> BTreeMap<String, Vec<String>> {
        self.tables
            .iter()
            .map(|t| (t.name.clone(), t.columns.clone()))
            .collect()
    }
}

/// Shared, mutable per-window schema cache. One refresh updates every tab.
///
/// `Arc<Mutex<_>>` rather than `Rc<RefCell<_>>`: the refresh runs on a tokio
/// task off the UI thread, which an `Rc` cannot cross. `parking_lot`, so a
/// reader is a `lock()` and not a `lock().unwrap()` — there is no poisoning to
/// handle and nothing useful to do about it if there were.
pub type SharedSnapshot = Arc<Mutex<SchemaSnapshot>>;

/// An empty snapshot pre-seeded with the function catalogue.
pub fn new_shared_snapshot() -> SharedSnapshot {
    Arc::new(Mutex::new(SchemaSnapshot {
        tables: Vec::new(),
        functions: DUCKDB_FUNCTIONS.iter().map(|s| s.to_string()).collect(),
    }))
}

/// Curated DuckDB function names.
///
/// Deliberately not `duckdb_functions()` introspection: that returns thousands
/// of rows including every internal and every overload, which buries the twenty
/// a person actually types.
pub const DUCKDB_FUNCTIONS: &[&str] = &[
    "count",
    "sum",
    "avg",
    "min",
    "max",
    "coalesce",
    "cast",
    "try_cast",
    "nullif",
    "greatest",
    "least",
    "abs",
    "round",
    "floor",
    "ceil",
    "length",
    "lower",
    "upper",
    "trim",
    "substring",
    "replace",
    "concat",
    "date_trunc",
    "strftime",
    "strptime",
    "epoch",
    "now",
    "current_date",
    "regexp_matches",
    "regexp_replace",
    "row_number",
    "rank",
    "dense_rank",
    "lag",
    "lead",
    "first",
    "last",
    "list",
    "unnest",
    "string_agg",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_snapshot_knows_the_functions_but_no_tables() {
        let s = new_shared_snapshot();
        let g = s.lock();
        assert!(g.tables.is_empty());
        assert_eq!(g.functions.len(), DUCKDB_FUNCTIONS.len());
        assert!(g.functions.iter().any(|f| f == "date_trunc"));
    }

    #[test]
    fn the_schema_map_is_what_lang_sql_expects() {
        let s = SchemaSnapshot {
            tables: vec![
                TableEntry {
                    name: "orders".into(),
                    columns: vec!["id".into(), "total".into()],
                },
                TableEntry {
                    name: "customers".into(),
                    columns: vec!["id".into()],
                },
            ],
            functions: Vec::new(),
        };
        let m = s.schema_map();
        assert_eq!(m.len(), 2);
        assert_eq!(m["orders"], vec!["id".to_string(), "total".to_string()]);
    }

    #[test]
    fn the_catalogue_has_no_duplicates() {
        // A duplicate shows twice in the popup, which reads as a bug in the
        // editor rather than in a list.
        let mut sorted = DUCKDB_FUNCTIONS.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), before);
    }

    #[test]
    fn a_snapshot_refresh_is_visible_to_every_holder() {
        // The whole point of sharing it: one refresh, every tab completes
        // against the new schema.
        let a = new_shared_snapshot();
        let b = Arc::clone(&a);
        a.lock().tables.push(TableEntry {
            name: "t".into(),
            columns: vec!["c".into()],
        });
        assert_eq!(b.lock().tables.len(), 1);
    }
}
