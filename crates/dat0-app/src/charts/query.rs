//! Pure: ChartSpec -> push-down plot SQL. Caps result size per type so the chart
//! stays fast + meaningful on million-row sources. `source` is already quoted;
//! column names are quoted here.

use crate::charts::spec::{ChartSpec, ChartType};

const CATEGORY_CAP: u32 = 100;
const LINE_CAP: u32 = 5000;
const SCATTER_SAMPLE: u32 = 2000;
const HIST_SAMPLE: u32 = 5000;
const HEATMAP_CELL_CAP: u32 = 2500;

fn q(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

fn need<'a>(v: &'a Option<String>, role: &str) -> Result<&'a str, String> {
    v.as_deref().ok_or_else(|| format!("chart needs a {role} column"))
}

/// Build the engine SQL that returns plot-ready rows for `spec`.
pub fn build_plot_sql(spec: &ChartSpec) -> Result<String, String> {
    let src = &spec.source;
    // CONTRACT: each type returns columns in a FIXED order/role that render.rs
    // reads POSITIONALLY (not by user column name). Category dims are CAST to
    // VARCHAR so data.rs reliably types them as text; value dims are numeric.
    //   Bar/BoxPlot: [0]=category:text, [1]=value:num
    //   Line/Area/Scatter: [0]=x:num, [1]=y:num
    //   Histogram: [0]=values:num
    //   Heatmap: [0]=x:text, [1]=y:text, [2]=value:num
    Ok(match spec.chart_type {
        ChartType::Bar => {
            let x = q(need(&spec.x, "x")?);
            let agg = match &spec.y {
                Some(y) => format!("SUM({})", q(y)),
                None => "COUNT(*)".to_string(),
            };
            format!(
                "SELECT CAST({x} AS VARCHAR) AS k, {agg} AS v FROM {src} WHERE {x} IS NOT NULL \
                 GROUP BY {x} ORDER BY v DESC LIMIT {CATEGORY_CAP}"
            )
        }
        ChartType::Line | ChartType::Area => {
            let x = q(need(&spec.x, "x")?);
            let y = q(need(&spec.y, "y")?);
            format!(
                "SELECT {x} AS x, {y} AS y FROM {src} WHERE {x} IS NOT NULL AND {y} IS NOT NULL \
                 ORDER BY {x} LIMIT {LINE_CAP}"
            )
        }
        ChartType::Scatter => {
            let x = q(need(&spec.x, "x")?);
            let y = q(need(&spec.y, "y")?);
            format!(
                "SELECT {x} AS x, {y} AS y FROM {src} \
                 WHERE {x} IS NOT NULL AND {y} IS NOT NULL USING SAMPLE {SCATTER_SAMPLE} ROWS"
            )
        }
        ChartType::Histogram => {
            let x = q(need(&spec.x, "x")?);
            format!(
                "SELECT {x} AS v FROM {src} WHERE {x} IS NOT NULL USING SAMPLE {HIST_SAMPLE} ROWS"
            )
        }
        ChartType::BoxPlot => {
            let cat = q(need(&spec.x, "category")?);
            let val = q(need(&spec.y, "value")?);
            format!(
                "SELECT CAST({cat} AS VARCHAR) AS k, {val} AS v FROM {src} \
                 WHERE {val} IS NOT NULL AND {cat} IS NOT NULL ORDER BY {cat}"
            )
        }
        ChartType::Heatmap => {
            let x = q(need(&spec.x, "x")?);
            let y = q(need(&spec.y, "y")?);
            let val = match &spec.color {
                Some(v) => format!("SUM({})", q(v)),
                None => "COUNT(*)".to_string(),
            };
            format!(
                "SELECT CAST({x} AS VARCHAR) AS x, CAST({y} AS VARCHAR) AS y, {val} AS v FROM {src} \
                 WHERE {x} IS NOT NULL AND {y} IS NOT NULL GROUP BY {x}, {y} ORDER BY v DESC LIMIT {HEATMAP_CELL_CAP}"
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charts::spec::{ChartSpec, ChartType};

    fn spec(t: ChartType, x: Option<&str>, y: Option<&str>) -> ChartSpec {
        ChartSpec {
            chart_type: t,
            source: "\"main\".\"orders\"".into(),
            x: x.map(str::to_string),
            y: y.map(str::to_string),
            group: None,
            color: None,
            title: String::new(),
        }
    }

    #[test]
    fn bar_groups_and_aggs() {
        let sql = build_plot_sql(&spec(ChartType::Bar, Some("region"), Some("sales"))).unwrap();
        assert!(sql.contains("GROUP BY"), "{sql}");
        assert!(sql.contains("SUM(\"sales\")"), "{sql}");
        assert!(sql.contains("FROM \"main\".\"orders\""), "{sql}");
        assert!(sql.to_uppercase().contains("LIMIT"), "category cap: {sql}");
    }

    #[test]
    fn bar_without_y_counts() {
        let sql = build_plot_sql(&spec(ChartType::Bar, Some("region"), None)).unwrap();
        assert!(sql.contains("COUNT(*)"), "{sql}");
    }

    #[test]
    fn scatter_samples() {
        let sql = build_plot_sql(&spec(ChartType::Scatter, Some("a"), Some("b"))).unwrap();
        assert!(sql.to_uppercase().contains("USING SAMPLE"), "{sql}");
    }

    #[test]
    fn histogram_one_column() {
        let sql = build_plot_sql(&spec(ChartType::Histogram, Some("amount"), None)).unwrap();
        assert!(sql.contains("\"amount\""), "{sql}");
        assert!(sql.to_uppercase().contains("USING SAMPLE"), "{sql}");
    }

    #[test]
    fn line_orders_by_x() {
        let sql = build_plot_sql(&spec(ChartType::Line, Some("day"), Some("v"))).unwrap();
        assert!(sql.to_uppercase().contains("ORDER BY"), "{sql}");
    }

    #[test]
    fn boxplot_selects_cat_and_value() {
        // BoxPlot: x = category, value carried in `y`.
        let sql = build_plot_sql(&spec(ChartType::BoxPlot, Some("region"), Some("amount"))).unwrap();
        assert!(sql.contains("\"region\""), "{sql}");
        assert!(sql.contains("\"amount\""), "{sql}");
    }

    #[test]
    fn heatmap_groups_two_dims() {
        // Heatmap: x, y from x/y; value carried in `color`.
        let mut s = spec(ChartType::Heatmap, Some("xc"), Some("yc"));
        s.color = Some("v".into());
        let sql = build_plot_sql(&s).unwrap();
        assert!(sql.contains("GROUP BY"), "{sql}");
        assert!(sql.contains("SUM(\"v\")"), "{sql}");
        assert!(sql.to_uppercase().contains("LIMIT"), "{sql}");
    }

    #[test]
    fn boxplot_guards_null_category() {
        let sql = build_plot_sql(&spec(ChartType::BoxPlot, Some("region"), Some("amount"))).unwrap();
        let up = sql.to_uppercase();
        assert!(up.contains("\"REGION\" IS NOT NULL"), "{sql}");
        assert!(up.contains("\"AMOUNT\" IS NOT NULL"), "{sql}");
    }

    #[test]
    fn missing_required_axis_errors() {
        let err = build_plot_sql(&spec(ChartType::Bar, None, Some("sales"))).unwrap_err();
        assert!(err.to_lowercase().contains("x"), "{err}");
    }
}
