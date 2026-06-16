//! Schema-only outbound payload model (R17). `AiRequest` structurally cannot
//! carry row values, query results, or file paths — only `sample_rows`, which
//! callers populate ONLY when the include-sample-rows toggle is on.

#[derive(Debug, Clone, Default)]
pub struct SchemaContext {
    pub tables: Vec<TableSchema>,
}

#[derive(Debug, Clone)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnSchema>,
}

#[derive(Debug, Clone)]
pub struct ColumnSchema {
    pub name: String,
    pub ty: String,
}

#[derive(Debug, Clone)]
pub struct SampleRows {
    /// Stringified cells. Present only when include-sample-rows is enabled.
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct AiRequest {
    pub model: String,
    pub system: Option<String>,
    pub schema: SchemaContext,
    pub prompt: String,
    pub sample_rows: Option<SampleRows>,
    pub max_tokens: u32,
}

impl SchemaContext {
    /// Human-readable schema block: `table(col TYPE, col TYPE)` per line.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for t in &self.tables {
            let cols: Vec<String> = t
                .columns
                .iter()
                .map(|c| format!("{} {}", c.name, c.ty))
                .collect();
            out.push_str(&format!("{}({})\n", t.name, cols.join(", ")));
        }
        out
    }
}

impl AiRequest {
    /// Minimal request used by Test-connection: no schema, no rows, tiny budget.
    pub fn ping(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            system: Some("Reply with the single word OK.".into()),
            schema: SchemaContext::default(),
            prompt: "ping".into(),
            sample_rows: None,
            max_tokens: 16,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_renders_names_and_types_only() {
        let ctx = SchemaContext {
            tables: vec![TableSchema {
                name: "users".into(),
                columns: vec![
                    ColumnSchema {
                        name: "id".into(),
                        ty: "INTEGER".into(),
                    },
                    ColumnSchema {
                        name: "email".into(),
                        ty: "VARCHAR".into(),
                    },
                ],
            }],
        };
        let text = ctx.render();
        assert!(text.contains("users"));
        assert!(text.contains("email"));
        assert!(text.contains("VARCHAR"));
        // Lock the exact format — wire.rs (T2) embeds this string verbatim in
        // the outbound payload, so a format regression must fail here.
        assert_eq!(text.trim(), "users(id INTEGER, email VARCHAR)");
    }

    #[test]
    fn default_request_has_no_sample_rows() {
        let req = AiRequest::ping("test-model");
        assert!(req.sample_rows.is_none());
        assert!(req.schema.tables.is_empty());
        assert_eq!(req.max_tokens, 16);
    }
}
