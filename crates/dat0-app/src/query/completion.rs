//! Schema-driven SQL autocomplete (P5b). Pure logic here (snapshot + query
//! extraction + ranking); the `CompletionProvider` adapter is in T2.
use gpui::SharedString;

/// A table and its column names, as seen by autocomplete.
#[derive(Debug, Clone, PartialEq)]
pub struct TableEntry {
    pub name: SharedString,
    pub columns: Vec<SharedString>,
}

/// Cached schema the provider filters against. One per window; refreshed off
/// `engine.get_tables()` (T2).
#[derive(Debug, Clone, Default)]
pub struct SchemaSnapshot {
    pub tables: Vec<TableEntry>,
    pub functions: Vec<SharedString>,
}

/// What kind of identifier a suggestion is (drives the menu icon in T2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestKind {
    Table,
    Column,
    Function,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub label: SharedString,
    pub kind: SuggestKind,
}

/// The identifier context immediately left of the cursor. `qualifier` is the
/// `tbl` in `tbl.col`; `word` is the partial identifier being typed.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionQuery {
    pub qualifier: Option<String>,
    pub word: String,
}

/// Extract the completion context at byte `offset` in `text`. Walks left over
/// `[A-Za-z0-9_]` to get `word`; if the char before `word` is `.`, walks left
/// again to capture the `qualifier`.
pub fn completion_query(text: &str, offset: usize) -> CompletionQuery {
    let bytes = text.as_bytes();
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let off = offset.min(bytes.len());

    let mut ws = off;
    while ws > 0 && is_ident(bytes[ws - 1]) {
        ws -= 1;
    }
    let word = text[ws..off].to_string();

    let mut qualifier = None;
    if ws > 0 && bytes[ws - 1] == b'.' {
        let mut qs = ws - 1;
        while qs > 0 && is_ident(bytes[qs - 1]) {
            qs -= 1;
        }
        let q = &text[qs..ws - 1];
        if !q.is_empty() {
            qualifier = Some(q.to_string());
        }
    }
    CompletionQuery { qualifier, word }
}

/// Rank suggestions for `q` against `snap`. With a `qualifier` that matches a
/// table → that table's columns. Else → tables + all columns + functions whose
/// name has `word` as a case-insensitive prefix. Empty `word` with no qualifier
/// → no suggestions (avoid dumping the whole schema on every keystroke).
pub fn suggestions(snap: &SchemaSnapshot, q: &CompletionQuery) -> Vec<Suggestion> {
    let wl = q.word.to_lowercase();
    let pfx = |name: &str| name.to_lowercase().starts_with(&wl);

    if let Some(qual) = &q.qualifier {
        let ql = qual.to_lowercase();
        if let Some(t) = snap.tables.iter().find(|t| t.name.to_lowercase() == ql) {
            return t
                .columns
                .iter()
                .filter(|c| pfx(c))
                .map(|c| Suggestion {
                    label: c.clone(),
                    kind: SuggestKind::Column,
                })
                .collect();
        }
        return Vec::new();
    }

    if q.word.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for t in &snap.tables {
        if pfx(&t.name) {
            out.push(Suggestion {
                label: t.name.clone(),
                kind: SuggestKind::Table,
            });
        }
    }
    for t in &snap.tables {
        for c in &t.columns {
            if pfx(c) {
                out.push(Suggestion {
                    label: c.clone(),
                    kind: SuggestKind::Column,
                });
            }
        }
    }
    for f in &snap.functions {
        if pfx(f) {
            out.push(Suggestion {
                label: f.clone(),
                kind: SuggestKind::Function,
            });
        }
    }
    out
}

/// Curated DuckDB function names for autocomplete. YAGNI on `duckdb_functions()`
/// introspection — it is huge and noisy; this covers the common surface.
pub fn duckdb_functions() -> Vec<SharedString> {
    [
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
    ]
    .iter()
    .map(|s| SharedString::from(*s))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> SchemaSnapshot {
        SchemaSnapshot {
            tables: vec![
                TableEntry {
                    name: "trips".into(),
                    columns: vec!["fare".into(), "distance".into()],
                },
                TableEntry {
                    name: "zones".into(),
                    columns: vec!["zone_id".into(), "name".into()],
                },
            ],
            functions: duckdb_functions(),
        }
    }

    #[test]
    fn query_bare_word() {
        let q = completion_query("select fa", 9);
        assert_eq!(
            q,
            CompletionQuery {
                qualifier: None,
                word: "fa".into()
            }
        );
    }

    #[test]
    fn query_qualified() {
        let q = completion_query("select trips.fa", 15);
        assert_eq!(
            q,
            CompletionQuery {
                qualifier: Some("trips".into()),
                word: "fa".into()
            }
        );
    }

    #[test]
    fn query_after_dot_empty_word() {
        let q = completion_query("select trips.", 13);
        assert_eq!(
            q,
            CompletionQuery {
                qualifier: Some("trips".into()),
                word: "".into()
            }
        );
    }

    #[test]
    fn query_after_operator_is_empty() {
        let q = completion_query("a = ", 4);
        assert_eq!(
            q,
            CompletionQuery {
                qualifier: None,
                word: "".into()
            }
        );
    }

    #[test]
    fn suggest_tables_and_columns_by_prefix() {
        let s = suggestions(
            &snap(),
            &CompletionQuery {
                qualifier: None,
                word: "z".into(),
            },
        );
        let labels: Vec<_> = s.iter().map(|x| x.label.to_string()).collect();
        assert!(labels.contains(&"zones".to_string())); // table
        assert!(labels.contains(&"zone_id".to_string())); // column of zones
    }

    #[test]
    fn suggest_qualified_columns_only() {
        let s = suggestions(
            &snap(),
            &CompletionQuery {
                qualifier: Some("trips".into()),
                word: "".into(),
            },
        );
        let labels: Vec<_> = s.iter().map(|x| x.label.to_string()).collect();
        assert_eq!(labels, vec!["fare".to_string(), "distance".to_string()]);
        assert!(s.iter().all(|x| x.kind == SuggestKind::Column));
    }

    #[test]
    fn suggest_empty_word_no_qualifier_is_empty() {
        let s = suggestions(
            &snap(),
            &CompletionQuery {
                qualifier: None,
                word: "".into(),
            },
        );
        assert!(s.is_empty());
    }

    #[test]
    fn suggest_unknown_qualifier_is_empty() {
        let s = suggestions(
            &snap(),
            &CompletionQuery {
                qualifier: Some("nope".into()),
                word: "".into(),
            },
        );
        assert!(s.is_empty());
    }

    #[test]
    fn suggest_function_prefix() {
        let s = suggestions(
            &snap(),
            &CompletionQuery {
                qualifier: None,
                word: "coa".into(),
            },
        );
        assert!(
            s.iter()
                .any(|x| x.label == "coalesce" && x.kind == SuggestKind::Function)
        );
    }
}
