//! Pure chart specification: type, source, axis selections. No GPUI, no app.
//! Serde-ready so P9a-2 (persistence) reuses `ChartSpec` verbatim. Relocated
//! here from `dat0-app` so the `.dat0` package format (`dat0-format`) can share
//! the same serde definition — mirroring `dat0_engine::transform::Transformation`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartType {
    Bar,
    Line,
    Area,
    Scatter,
    Histogram,
    BoxPlot,
    Heatmap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisRole {
    X,
    Y,
    Group,
    Color,
    Value,
}

impl ChartType {
    pub const ALL: [ChartType; 7] = [
        ChartType::Bar,
        ChartType::Line,
        ChartType::Area,
        ChartType::Scatter,
        ChartType::Histogram,
        ChartType::BoxPlot,
        ChartType::Heatmap,
    ];

    /// Which axis pickers to show for this type (drives the panel UI).
    pub fn axes(self) -> &'static [AxisRole] {
        use AxisRole::*;
        match self {
            ChartType::Bar => &[X, Y],
            ChartType::Line => &[X, Y, Group],
            ChartType::Area => &[X, Y],
            ChartType::Scatter => &[X, Y, Color],
            ChartType::Histogram => &[X],
            ChartType::BoxPlot => &[X, Value],
            ChartType::Heatmap => &[X, Y, Value],
        }
    }

    /// i18n key for the type's display name (e.g. "chart.type.bar").
    pub fn label_key(self) -> &'static str {
        match self {
            ChartType::Bar => "chart.type.bar",
            ChartType::Line => "chart.type.line",
            ChartType::Area => "chart.type.area",
            ChartType::Scatter => "chart.type.scatter",
            ChartType::Histogram => "chart.type.histogram",
            ChartType::BoxPlot => "chart.type.boxplot",
            ChartType::Heatmap => "chart.type.heatmap",
        }
    }
}

/// A fully-specified chart. `source` is the engine table/view name (already quoted,
/// e.g. `"main"."orders"`). Column fields are bare column names (unquoted).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartSpec {
    pub chart_type: ChartType,
    pub source: String,
    pub x: Option<String>,
    pub y: Option<String>,
    pub group: Option<String>,
    pub color: Option<String>,
    #[serde(default)]
    pub title: String,
}

/// True for DuckDB numeric type literals (as returned by DESCRIBE / ColumnInfo.data_type).
/// Trims any parameter suffix (e.g. `DECIMAL(9,2)` -> `DECIMAL`) then matches an explicit
/// set — explicit rather than prefix-based so temporal types like `INTERVAL` (which would
/// match a bare `starts_with("INT")`) are correctly excluded.
pub fn is_numeric(data_type: &str) -> bool {
    let base = data_type
        .split('(')
        .next()
        .unwrap_or(data_type)
        .trim()
        .to_ascii_uppercase();
    matches!(
        base.as_str(),
        "TINYINT"
            | "SMALLINT"
            | "INTEGER"
            | "BIGINT"
            | "HUGEINT"
            | "UHUGEINT"
            | "UTINYINT"
            | "USMALLINT"
            | "UINTEGER"
            | "UBIGINT"
            | "FLOAT"
            | "DOUBLE"
            | "DECIMAL"
            | "NUMERIC"
            | "REAL"
            | "INT"
            | "INT1"
            | "INT2"
            | "INT4"
            | "INT8"
            | "INT16"
            | "INT32"
            | "INT64"
            | "INT128"
            | "FLOAT4"
            | "FLOAT8"
    )
}

/// Pick a sensible default chart type from the column data-types of the source.
pub fn default_type(data_types: &[&str]) -> ChartType {
    let numeric = data_types.iter().filter(|t| is_numeric(t)).count();
    let has_text = data_types.iter().any(|t| !is_numeric(t));
    match (numeric, has_text) {
        (n, _) if n >= 2 => ChartType::Scatter,
        (1, false) => ChartType::Histogram,
        _ => ChartType::Bar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_type_detection() {
        assert!(is_numeric("BIGINT"));
        assert!(is_numeric("DOUBLE"));
        assert!(is_numeric("DECIMAL(9,2)"));
        assert!(!is_numeric("VARCHAR"));
        assert!(!is_numeric("DATE"));
        // Guard against the INTERVAL/INT prefix-collision bug (INTERVAL starts_with "INT"):
        assert!(!is_numeric("INTERVAL"));
        assert!(!is_numeric("TIMESTAMP"));
        assert!(is_numeric("INT"));
        assert!(is_numeric("NUMERIC"));
        assert!(is_numeric("HUGEINT"));
    }

    #[test]
    fn visible_axes_per_type() {
        assert_eq!(ChartType::Histogram.axes(), &[AxisRole::X]);
        assert_eq!(ChartType::Bar.axes(), &[AxisRole::X, AxisRole::Y]);
        assert_eq!(
            ChartType::Scatter.axes(),
            &[AxisRole::X, AxisRole::Y, AxisRole::Color]
        );
        assert_eq!(
            ChartType::Heatmap.axes(),
            &[AxisRole::X, AxisRole::Y, AxisRole::Value]
        );
        assert_eq!(ChartType::BoxPlot.axes(), &[AxisRole::X, AxisRole::Value]);
    }

    #[test]
    fn default_type_infers_from_schema() {
        // two numerics -> scatter
        assert_eq!(default_type(&["DOUBLE", "BIGINT"]), ChartType::Scatter);
        // one numeric only -> histogram
        assert_eq!(default_type(&["DOUBLE"]), ChartType::Histogram);
        // text + numeric -> bar
        assert_eq!(default_type(&["VARCHAR", "BIGINT"]), ChartType::Bar);
        // nothing numeric -> bar (count)
        assert_eq!(default_type(&["VARCHAR", "DATE"]), ChartType::Bar);
    }

    #[test]
    fn all_types_have_a_label_and_parse_round_trips() {
        for t in ChartType::ALL {
            assert!(!t.label_key().is_empty());
        }
        assert_eq!(ChartType::ALL.len(), 7);
    }
}
