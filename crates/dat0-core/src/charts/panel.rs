//! Chart panel state and the axis rules that constrain it.
//!
//! Which axis roles a chart type exposes, and which columns are legal in each,
//! are properties of the chart spec — not of the widget that renders the
//! pickers. `ChartPanel` is the live state the shell binds to a source.

use crate::charts::data::PlotTable;
use crate::charts::spec::{AxisRole, ChartSpec, ChartType, is_numeric};

/// Which axis roles to show for a type (delegates to ChartType::axes).
pub fn visible_axes(t: ChartType) -> Vec<AxisRole> {
    t.axes().to_vec()
}

/// Column names valid for a given axis role, filtered by data type.
/// `cols` is `(name, duckdb_type)`. Y/Value require numeric; X/Group/Color accept any.
pub fn column_options(role: AxisRole, cols: &[(String, String)]) -> Vec<String> {
    let numeric_only = matches!(role, AxisRole::Y | AxisRole::Value);
    cols.iter()
        .filter(|(_, ty)| !numeric_only || is_numeric(ty))
        .map(|(n, _)| n.clone())
        .collect()
}

/// Live panel state held on the WorkspaceShell.
pub struct ChartPanel {
    pub source: Option<String>,         // engine table/view name (quoted)
    pub columns: Vec<(String, String)>, // (name, duckdb_type) of the source
    pub spec: ChartSpec,
    /// Last rendered plot data (set after the plot query returns).
    pub data: Option<PlotTable>,
    /// Last error to show in place of the chart.
    pub error: Option<String>,
}

impl ChartPanel {
    pub fn new() -> Self {
        ChartPanel {
            source: None,
            columns: Vec::new(),
            spec: ChartSpec {
                chart_type: ChartType::Bar,
                source: String::new(),
                x: None,
                y: None,
                group: None,
                color: None,
                title: String::new(),
            },
            data: None,
            error: None,
        }
    }

    /// Bind to a new source: set name + columns and infer a default type.
    pub fn bind(&mut self, source: String, columns: Vec<(String, String)>) {
        let types: Vec<&str> = columns.iter().map(|(_, t)| t.as_str()).collect();
        self.spec.chart_type = crate::charts::spec::default_type(&types);
        // Clear stale axis picks — a new source has a different schema, so any
        // previously chosen column names are invalid until the user re-selects them.
        self.spec.x = None;
        self.spec.y = None;
        self.spec.group = None;
        self.spec.color = None;
        self.spec.source = source.clone();
        self.source = Some(source);
        self.columns = columns;
        self.data = None;
        self.error = None;
    }
}

impl Default for ChartPanel {
    fn default() -> Self {
        Self::new()
    }
}
