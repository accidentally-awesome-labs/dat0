//! Fixed system prompts for the P9c-2 features. Scoped narrowly (SQL-only /
//! explanation-only) so schema names — which are user-controlled — cannot
//! redirect the model into arbitrary behaviour, and so returned SQL is never
//! executed automatically (the app routes NL→SQL to a new editor tab).

/// System prompt for NL→SQL. Output is dropped verbatim into a SQL editor, so
/// it must be a runnable DuckDB statement and nothing else.
pub fn nl_to_sql_system() -> &'static str {
    "You translate a natural-language request into a single DuckDB SQL query. \
     Use only the tables and columns in the provided schema. Output ONLY the SQL \
     statement — no prose, no Markdown fences, no explanation. If the request \
     cannot be answered from the schema, output a SQL comment explaining why."
}

/// System prompt for Explain. Output is plain prose shown in a side panel.
pub fn explain_system() -> &'static str {
    "You explain a DuckDB SQL query in clear, concise plain language: what it \
     returns, the tables and columns it reads, and any filters, joins, grouping, \
     or ordering. Use the provided schema for context. Do not rewrite or execute \
     the query; only explain it."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nl_to_sql_is_sql_scoped() {
        let s = nl_to_sql_system().to_lowercase();
        assert!(s.contains("sql"));
        assert!(s.contains("duckdb"));
        // Must instruct SQL-only output (no prose), so the preview strip shows
        // a clean statement to Insert.
        assert!(s.contains("only") || s.contains("do not"));
    }

    #[test]
    fn explain_is_explanation_scoped() {
        let s = explain_system().to_lowercase();
        assert!(s.contains("explain"));
        assert!(s.contains("sql"));
    }
}
