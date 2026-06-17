//! Build the schema-only [`SchemaContext`](crate::ai::request::SchemaContext)
//! sent to the model (R17). Source = the shell's cached `catalog_tables`; this
//! maps NAMES + TYPES only (no values — `TableInfo`/`ColumnInfo` carry none),
//! drops the internal `__dat0_rowid` surrogate, and caps the payload.

use crate::ai::request::{ColumnSchema, SchemaContext, TableSchema};

/// Internal surrogate row-id column, never shown to the model (mirrors the
/// Inspector's projection filter).
const SURROGATE_COL: &str = "__dat0_rowid";

#[derive(Debug, Clone, Copy)]
pub struct SchemaCaps {
    pub max_tables: usize,
    pub max_cols_per_table: usize,
}

impl Default for SchemaCaps {
    fn default() -> Self {
        Self { max_tables: 40, max_cols_per_table: 60 }
    }
}

/// Map catalog tables → schema-only context, capped. Returns a human note when
/// the table count was truncated so the model knows context is partial.
///
/// The note is returned separately (not inside [`SchemaContext`]) so callers can
/// append it to the prompt text. Keeping the note out of `SchemaContext` preserves
/// the R17 guarantee: `SchemaContext` contains names + types only.
pub fn build_schema_context(
    tables: &[dat0_engine::TableInfo],
    caps: SchemaCaps,
) -> (SchemaContext, Option<String>) {
    let mapped: Vec<TableSchema> = tables
        .iter()
        .take(caps.max_tables)
        .map(|t| TableSchema {
            name: t.name.clone(),
            columns: t
                .columns
                .iter()
                .filter(|c| c.name != SURROGATE_COL)
                .take(caps.max_cols_per_table)
                .map(|c| ColumnSchema {
                    name: c.name.clone(),
                    ty: c.data_type.clone(),
                })
                .collect(),
        })
        .collect();
    let note = tables
        .len()
        .checked_sub(caps.max_tables)
        .filter(|&n| n > 0)
        .map(|n| format!("… {n} more table(s) omitted from the schema"));
    (SchemaContext { tables: mapped }, note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat0_engine::types::{ColumnInfo, TableInfo, TableOrigin};

    fn col(name: &str, ty: &str) -> ColumnInfo {
        ColumnInfo { name: name.into(), data_type: ty.into(), nullable: true }
    }
    fn tbl(name: &str, cols: Vec<ColumnInfo>) -> TableInfo {
        TableInfo {
            name: name.into(),
            schema: "main".into(),
            columns: cols,
            row_count_estimate: None,
            origin: TableOrigin::Derived(dat0_engine::types::DerivedOrigin::Sql(String::new())),
        }
    }

    #[test]
    fn maps_names_and_types_drops_surrogate() {
        let tables = vec![tbl(
            "users",
            vec![col("__dat0_rowid", "BIGINT"), col("email", "VARCHAR")],
        )];
        let (ctx, note) = build_schema_context(&tables, SchemaCaps::default());
        assert!(note.is_none());
        assert_eq!(ctx.tables.len(), 1);
        let cols: Vec<&str> = ctx.tables[0].columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(cols, vec!["email"]); // surrogate dropped
        assert_eq!(ctx.tables[0].columns[0].ty, "VARCHAR");
    }

    #[test]
    fn truncates_and_notes() {
        let tables: Vec<TableInfo> =
            (0..50).map(|i| tbl(&format!("t{i}"), vec![col("c", "INTEGER")])).collect();
        let caps = SchemaCaps { max_tables: 40, max_cols_per_table: 60 };
        let (ctx, note) = build_schema_context(&tables, caps);
        assert_eq!(ctx.tables.len(), 40);
        let note = note.expect("truncation note");
        assert!(note.contains("10"), "note should mention 10 omitted tables: {note}");
    }

    #[test]
    fn caps_columns_per_table() {
        let cols: Vec<ColumnInfo> = (0..100).map(|i| col(&format!("c{i}"), "INTEGER")).collect();
        let (ctx, _) = build_schema_context(&[tbl("wide", cols)], SchemaCaps::default());
        assert_eq!(ctx.tables[0].columns.len(), 60);
    }

    // R17: a built schema context, rendered through both wire bodies, carries
    // names+types only — never row values.
    #[test]
    fn r17_schema_context_carries_no_row_values() {
        use crate::ai::request::AiRequest;
        use crate::ai::wire::{AnthropicWire, OpenAiCompatWire, Wire};
        let tables = vec![tbl("orders", vec![col("amount", "DECIMAL")])];
        let (schema, _) = build_schema_context(&tables, SchemaCaps::default());
        let req = AiRequest {
            model: "m".into(),
            system: Some("emit sql".into()),
            schema,
            prompt: "total revenue".into(),
            sample_rows: None,
            max_tokens: 64,
        };
        for body in [
            AnthropicWire.build_body("m", &req),
            OpenAiCompatWire.build_body("m", &req),
        ] {
            let s = body.to_string();
            assert!(s.contains("orders") && s.contains("amount") && s.contains("total revenue"));
            assert!(!s.contains("SECRET_ROW_VALUE"), "row data must never appear: {s}");
        }
    }
}
