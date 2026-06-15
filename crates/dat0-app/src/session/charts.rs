//! Persisted saved charts (P9a-2): named `ChartSpec`s that survive workspace
//! save/reopen and attach to lineage. Pure data + helpers; persistence lives in
//! `SessionState` (session/mod.rs), mirroring `session/queries.rs`.
use crate::charts::spec::{ChartSpec, ChartType};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A user-named saved chart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedChart {
    pub id: Uuid,
    pub name: String,
    pub spec: ChartSpec,
    pub saved_at: i64,
}

/// Insert `c`, replacing any existing entry with the same (case-insensitive)
/// name; otherwise append. Returns true if an existing entry was replaced.
/// Mirrors `queries::upsert_saved`.
pub fn upsert_chart(list: &mut Vec<SavedChart>, c: SavedChart) -> bool {
    let nl = c.name.to_lowercase();
    if let Some(slot) = list.iter_mut().find(|s| s.name.to_lowercase() == nl) {
        *slot = c;
        true
    } else {
        list.push(c);
        false
    }
}

/// Seed name shown pre-filled in the Save prompt. `"<Type>: <y> by <x>"` when
/// both axes are set, else `"<Type> of <bare source>"`. Pure (no i18n) — this is
/// editable seed text, not a translated label.
pub fn default_chart_name(spec: &ChartSpec) -> String {
    let ty = match spec.chart_type {
        ChartType::Bar => "Bar",
        ChartType::Line => "Line",
        ChartType::Area => "Area",
        ChartType::Scatter => "Scatter",
        ChartType::Histogram => "Histogram",
        ChartType::BoxPlot => "Box plot",
        ChartType::Heatmap => "Heatmap",
    };
    match (spec.y.as_deref(), spec.x.as_deref()) {
        (Some(y), Some(x)) => format!("{ty}: {y} by {x}"),
        _ => format!("{ty} of {}", bare_source(&spec.source)),
    }
}

/// Reduce a quoted/qualified source (`"main"."orders"`) to a bare display name
/// (`orders`). Pure string work, local to name-seeding.
fn bare_source(source: &str) -> String {
    source
        .rsplit('.')
        .next()
        .unwrap_or(source)
        .trim_matches('"')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charts::spec::ChartSpec;

    fn spec(t: ChartType, x: Option<&str>, y: Option<&str>) -> ChartSpec {
        ChartSpec {
            chart_type: t,
            source: "\"main\".\"orders\"".into(),
            x: x.map(Into::into),
            y: y.map(Into::into),
            group: None,
            color: None,
            title: String::new(),
        }
    }

    fn chart(name: &str) -> SavedChart {
        SavedChart {
            id: Uuid::now_v7(),
            name: name.into(),
            spec: spec(ChartType::Bar, Some("region"), Some("amount")),
            saved_at: 0,
        }
    }

    #[test]
    fn upsert_replaces_same_name_case_insensitive() {
        let mut list = Vec::new();
        assert!(!upsert_chart(&mut list, chart("Sales")));
        assert!(upsert_chart(&mut list, chart("sales")));
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn default_name_uses_axes_then_falls_back_to_source() {
        assert_eq!(
            default_chart_name(&spec(ChartType::Bar, Some("region"), Some("amount"))),
            "Bar: amount by region"
        );
        assert_eq!(
            default_chart_name(&spec(ChartType::Histogram, Some("amount"), None)),
            "Histogram of orders"
        );
    }
}
