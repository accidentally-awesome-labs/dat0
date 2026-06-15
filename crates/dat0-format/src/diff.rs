//! P8 T5: a pure-JSON `DiffEngine` over two `.dat0` recipes + saved-query sets.
//!
//! The diff is computed entirely at the metadata level — NO engine, NO parquet
//! read. Row counts are read straight from [`RecipeTable::row_count`]; schema
//! deltas from the [`ColumnFingerprint`] lists; lineage from table presence and
//! the [`Derivation`] enum; query deltas from the saved-query SQL; chart deltas
//! from the saved-chart [`ChartSpec`]. This keeps `dat0 diff` instant and
//! dependency-light (it never opens DuckDB).
//!
//! Matching is by NAME on both axes (tables by `name`, columns by `name`, saved
//! queries by `name`, saved charts by `name`). Five orthogonal dimensions are
//! reported; an empty diff (all five empty) means the two packages are
//! recipe-equivalent.
//!
//! Intentional non-comparisons: `source_ref` and the `sources` block are NOT
//! diffed. A base table that lost its File-origin source on unpack (re-exporting
//! without a `source_ref`) must NOT register as a difference — only `derivation`
//! participates in the lineage dimension.

use serde::Serialize;

use crate::ParsedPackage;
use crate::model::{Charts, Derivation, Queries, Recipe, RecipeTable};

/// What changed about a single column within a same-named table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ColumnChange {
    /// Present in `b`, absent in `a`. `r#type` is the new column's type.
    Added { r#type: String },
    /// Present in `a`, absent in `b`.
    Removed { r#type: String },
    /// Same column name, different DuckDB type literal.
    Retyped { from: String, to: String },
}

/// A schema-dimension delta: a column added/removed/retyped in `table`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaDelta {
    pub table: String,
    pub column: String,
    pub change: ColumnChange,
}

/// What changed about a table at the lineage level (its presence / derivation).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineageChange {
    /// Table present in `b` only (a new node).
    Added,
    /// Table present in `a` only (a removed node).
    Removed,
    /// Table in both, but its `derivation` differs (different sql/parents/ops,
    /// or gained/lost a derivation entirely).
    DerivationChanged {
        from: Option<Derivation>,
        to: Option<Derivation>,
    },
}

/// A lineage-dimension delta for a single table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LineageDelta {
    pub table: String,
    pub change: LineageChange,
}

/// What changed about a saved query (matched by `name`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueryChange {
    /// Query name present in `b` only.
    Added,
    /// Query name present in `a` only.
    Removed,
    /// Same name, different SQL text.
    SqlChanged { from: String, to: String },
}

/// A query-dimension delta for a single saved query (keyed by `name`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueryDelta {
    pub name: String,
    pub change: QueryChange,
}

/// What changed about a saved chart (matched by `name`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChartChange {
    /// Chart name present in `b` only.
    Added,
    /// Chart name present in `a` only.
    Removed,
    /// Same name, different spec. Carries one-line summaries of each side.
    SpecChanged { from: String, to: String },
}

/// A chart-dimension delta for a single saved chart (keyed by `name`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChartDelta {
    pub name: String,
    pub change: ChartChange,
}

/// One-line spec summary used by `ChartChange::SpecChanged` (compact, readable).
fn chart_summary(spec: &dat0_engine::chart_spec::ChartSpec) -> String {
    let ty = format!("{:?}", spec.chart_type).to_lowercase();
    let mut s = ty;
    if let Some(x) = &spec.x {
        s.push_str(&format!(" x={x}"));
    }
    if let Some(y) = &spec.y {
        s.push_str(&format!(" y={y}"));
    }
    s
}

/// The full structured diff between two packages: five orthogonal dimensions.
/// [`PackageDiff::is_empty`] is the "are these recipe-equivalent?" predicate.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct PackageDiff {
    pub schema: Vec<SchemaDelta>,
    pub lineage: Vec<LineageDelta>,
    pub queries: Vec<QueryDelta>,
    pub charts: Vec<ChartDelta>,
    /// `(table_name, old_row_count, new_row_count)` for same-named tables whose
    /// row count differs.
    pub row_count_deltas: Vec<(String, u64, u64)>,
}

impl PackageDiff {
    /// `true` iff all five dimensions are empty (the packages are recipe-equal).
    pub fn is_empty(&self) -> bool {
        self.schema.is_empty()
            && self.lineage.is_empty()
            && self.queries.is_empty()
            && self.row_count_deltas.is_empty()
            && self.charts.is_empty()
    }

    /// A human-readable text summary (one section per non-empty dimension).
    /// Returns a fixed "No differences." line when empty.
    pub fn render_text(&self) -> String {
        if self.is_empty() {
            return "No differences.\n".to_string();
        }
        let mut out = String::new();

        if !self.lineage.is_empty() {
            out.push_str("Lineage:\n");
            for d in &self.lineage {
                match &d.change {
                    LineageChange::Added => {
                        out.push_str(&format!("  + table {}\n", d.table));
                    }
                    LineageChange::Removed => {
                        out.push_str(&format!("  - table {}\n", d.table));
                    }
                    LineageChange::DerivationChanged { .. } => {
                        out.push_str(&format!("  ~ table {} derivation changed\n", d.table));
                    }
                }
            }
        }

        if !self.schema.is_empty() {
            out.push_str("Schema:\n");
            for d in &self.schema {
                match &d.change {
                    ColumnChange::Added { r#type } => {
                        out.push_str(&format!("  + {}.{} ({})\n", d.table, d.column, r#type));
                    }
                    ColumnChange::Removed { r#type } => {
                        out.push_str(&format!("  - {}.{} ({})\n", d.table, d.column, r#type));
                    }
                    ColumnChange::Retyped { from, to } => {
                        out.push_str(&format!(
                            "  ~ {}.{} {} -> {}\n",
                            d.table, d.column, from, to
                        ));
                    }
                }
            }
        }

        if !self.row_count_deltas.is_empty() {
            out.push_str("Row counts:\n");
            for (table, old, new) in &self.row_count_deltas {
                out.push_str(&format!("  ~ {table}: {old} -> {new}\n"));
            }
        }

        if !self.queries.is_empty() {
            out.push_str("Queries:\n");
            for d in &self.queries {
                match &d.change {
                    QueryChange::Added => out.push_str(&format!("  + {}\n", d.name)),
                    QueryChange::Removed => out.push_str(&format!("  - {}\n", d.name)),
                    QueryChange::SqlChanged { .. } => {
                        out.push_str(&format!("  ~ {} sql changed\n", d.name))
                    }
                }
            }
        }

        if !self.charts.is_empty() {
            out.push_str("Charts:\n");
            for d in &self.charts {
                match &d.change {
                    ChartChange::Added => out.push_str(&format!("  + {}\n", d.name)),
                    ChartChange::Removed => out.push_str(&format!("  - {}\n", d.name)),
                    ChartChange::SpecChanged { .. } => {
                        out.push_str(&format!("  ~ {} spec changed\n", d.name))
                    }
                }
            }
        }

        out
    }

    /// A structured JSON rendering (the five dimensions as a flat object).
    pub fn render_json(&self) -> serde_json::Value {
        // `PackageDiff` derives `Serialize`; serialize directly. (Falls back to
        // an empty object only on the practically-impossible serde failure.)
        serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}))
    }
}

/// Compute the diff between two recipes + their saved-query and saved-chart sets.
/// Pure — no engine, no I/O. Tables/columns/queries/charts are matched by name
/// (see module docs).
pub fn compute(
    a_recipe: &Recipe,
    a_queries: &Queries,
    a_charts: &Charts,
    b_recipe: &Recipe,
    b_queries: &Queries,
    b_charts: &Charts,
) -> PackageDiff {
    let mut diff = PackageDiff::default();

    diff_tables(a_recipe, b_recipe, &mut diff);
    diff_queries(a_queries, b_queries, &mut diff);
    diff_charts(a_charts, b_charts, &mut diff);

    diff
}

/// `ParsedPackage`-level wrapper: diff two opened packages by their recipe +
/// saved queries + saved charts (ignores sources/views by design).
pub fn diff(a: &ParsedPackage, b: &ParsedPackage) -> PackageDiff {
    compute(
        &a.recipe, &a.queries, &a.charts, &b.recipe, &b.queries, &b.charts,
    )
}

/// Look up a table by name in a recipe (linear; recipes are small).
fn find_table<'a>(recipe: &'a Recipe, name: &str) -> Option<&'a RecipeTable> {
    recipe.tables.iter().find(|t| t.name == name)
}

/// Diff the table dimension: presence (lineage add/remove), derivation change
/// (lineage), per-column schema deltas, and row-count deltas.
fn diff_tables(a: &Recipe, b: &Recipe, out: &mut PackageDiff) {
    // Tables in `a`: removed (absent in b) OR matched (compare in-place).
    for ta in &a.tables {
        match find_table(b, &ta.name) {
            None => out.lineage.push(LineageDelta {
                table: ta.name.clone(),
                change: LineageChange::Removed,
            }),
            Some(tb) => diff_matched_table(ta, tb, out),
        }
    }
    // Tables in `b` not in `a`: added.
    for tb in &b.tables {
        if find_table(a, &tb.name).is_none() {
            out.lineage.push(LineageDelta {
                table: tb.name.clone(),
                change: LineageChange::Added,
            });
        }
    }
}

/// Compare two same-named tables: derivation (lineage), columns (schema), and
/// row count.
fn diff_matched_table(ta: &RecipeTable, tb: &RecipeTable, out: &mut PackageDiff) {
    // Lineage: only `derivation` participates (NOT source_ref — see module docs).
    if ta.derivation != tb.derivation {
        out.lineage.push(LineageDelta {
            table: tb.name.clone(),
            change: LineageChange::DerivationChanged {
                from: ta.derivation.clone(),
                to: tb.derivation.clone(),
            },
        });
    }

    // Schema: columns matched by name.
    for ca in &ta.schema {
        match tb.schema.iter().find(|c| c.name == ca.name) {
            None => out.schema.push(SchemaDelta {
                table: tb.name.clone(),
                column: ca.name.clone(),
                change: ColumnChange::Removed {
                    r#type: ca.r#type.clone(),
                },
            }),
            Some(cb) if cb.r#type != ca.r#type => out.schema.push(SchemaDelta {
                table: tb.name.clone(),
                column: ca.name.clone(),
                change: ColumnChange::Retyped {
                    from: ca.r#type.clone(),
                    to: cb.r#type.clone(),
                },
            }),
            Some(_) => {} // unchanged
        }
    }
    for cb in &tb.schema {
        if !ta.schema.iter().any(|c| c.name == cb.name) {
            out.schema.push(SchemaDelta {
                table: tb.name.clone(),
                column: cb.name.clone(),
                change: ColumnChange::Added {
                    r#type: cb.r#type.clone(),
                },
            });
        }
    }

    // Row count.
    if ta.row_count != tb.row_count {
        out.row_count_deltas
            .push((tb.name.clone(), ta.row_count, tb.row_count));
    }
}

/// Diff the saved-query dimension: matched by name, compare SQL.
fn diff_queries(a: &Queries, b: &Queries, out: &mut PackageDiff) {
    for qa in &a.queries {
        match b.queries.iter().find(|q| q.name == qa.name) {
            None => out.queries.push(QueryDelta {
                name: qa.name.clone(),
                change: QueryChange::Removed,
            }),
            Some(qb) if qb.sql != qa.sql => out.queries.push(QueryDelta {
                name: qa.name.clone(),
                change: QueryChange::SqlChanged {
                    from: qa.sql.clone(),
                    to: qb.sql.clone(),
                },
            }),
            Some(_) => {} // unchanged
        }
    }
    for qb in &b.queries {
        if !a.queries.iter().any(|q| q.name == qb.name) {
            out.queries.push(QueryDelta {
                name: qb.name.clone(),
                change: QueryChange::Added,
            });
        }
    }
}

/// Diff the saved-chart dimension: matched by name, compare spec summaries.
fn diff_charts(a: &Charts, b: &Charts, out: &mut PackageDiff) {
    for ca in &a.charts {
        match b.charts.iter().find(|c| c.name == ca.name) {
            None => out.charts.push(ChartDelta {
                name: ca.name.clone(),
                change: ChartChange::Removed,
            }),
            Some(cb) if cb.spec != ca.spec => out.charts.push(ChartDelta {
                name: ca.name.clone(),
                change: ChartChange::SpecChanged {
                    from: chart_summary(&ca.spec),
                    to: chart_summary(&cb.spec),
                },
            }),
            Some(_) => {} // unchanged
        }
    }
    for cb in &b.charts {
        if !a.charts.iter().any(|c| c.name == cb.name) {
            out.charts.push(ChartDelta {
                name: cb.name.clone(),
                change: ChartChange::Added,
            });
        }
    }
}
