//! Chart panel: state + pure axis helpers + GPUI render. The pure parts
//! (`visible_axes`, `column_options`) carry the tests; the GPUI render is UAT-gated.

use crate::a11y::{A11yExt as _, AccessRole};
use crate::charts::data::PlotTable;
use crate::charts::spec::{AxisRole, ChartSpec, ChartType, is_numeric};
use gpui::{
    ImageSource, IntoElement, ParentElement, RenderImage, SharedString, Styled, div, img, px,
};
use std::sync::Arc;

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

/// Localised display name for a chart type (used by the content seam so the
/// headless UAT can assert the *rendered* type). Keys already exist in en.json.
pub(crate) fn chart_type_label(t: ChartType) -> SharedString {
    dat0_i18n::t(t.label_key()).into()
}

/// Render the chart image area from a prepared RenderImage (or a hint/error).
/// Toolbar widgets (Selects/Buttons) are composed in window.rs where the
/// Entity<SelectState> + listeners live; this renders the body.
pub fn render_chart_body(
    panel: &ChartPanel,
    image: Option<Arc<RenderImage>>,
    logical: (f32, f32),
) -> impl IntoElement {
    let body = if let Some(err) = &panel.error {
        div()
            .p_4()
            .text_color(gpui::rgb(0xcc4444))
            .child(err.clone())
            .into_any_element()
    } else if let Some(ri) = image {
        img(ImageSource::Render(ri))
            .w(px(logical.0))
            .h(px(logical.1))
            .into_any_element()
    } else {
        div()
            .p_4()
            .text_color(gpui::rgb(0x888888))
            .child(dat0_i18n::t("chart.panel.empty"))
            .into_any_element()
    };
    // Content seams (release no-op; emit AccessKit Label nodes only under the
    // `a11y-capture` feature) so the headless UAT can assert the *rendered*
    // spec — type, axis picks, title — without inspecting pixels (Gap 1 stays
    // human). Inert single-purpose divs → a real layout node, hence a human
    // visual glance is owed on the Charts dock (mirrors the Settings wrappers).
    let s = &panel.spec;
    let seams = div()
        .flex()
        .gap_1()
        .child(div().a11y_label(AccessRole::Label, chart_type_label(s.chart_type)))
        .child(div().a11y_label(
            AccessRole::Label,
            SharedString::from(s.x.clone().unwrap_or_default()),
        ))
        .child(div().a11y_label(
            AccessRole::Label,
            SharedString::from(s.y.clone().unwrap_or_default()),
        ))
        .child(div().a11y_label(AccessRole::Label, SharedString::from(s.title.clone())));
    div().flex().flex_col().gap_2().child(seams).child(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charts::spec::{AxisRole, ChartType};

    #[test]
    fn panel_shows_only_relevant_axes() {
        assert_eq!(visible_axes(ChartType::Histogram), vec![AxisRole::X]);
        assert_eq!(
            visible_axes(ChartType::Heatmap),
            vec![AxisRole::X, AxisRole::Y, AxisRole::Value]
        );
    }

    #[test]
    fn numeric_only_axes_filter_columns() {
        let cols = vec![
            ("region".to_string(), "VARCHAR".to_string()),
            ("sales".to_string(), "BIGINT".to_string()),
        ];
        // Y wants numeric only
        let opts = column_options(AxisRole::Y, &cols);
        assert_eq!(opts, vec!["sales".to_string()]);
        // X accepts any
        let optsx = column_options(AxisRole::X, &cols);
        assert_eq!(optsx.len(), 2);
    }

    #[test]
    fn bind_clears_stale_axis_picks() {
        let mut p = ChartPanel::new();
        p.spec.x = Some("region".into());
        p.spec.y = Some("sales".into());
        p.spec.group = Some("g".into());
        p.spec.color = Some("c".into());
        p.bind("\"b\"".into(), vec![("other".into(), "DOUBLE".into())]);
        assert!(p.spec.x.is_none());
        assert!(p.spec.y.is_none());
        assert!(p.spec.group.is_none());
        assert!(p.spec.color.is_none());
        // sanity: rebinding still infers a type + sets source
        assert_eq!(p.spec.source, "\"b\"");
    }
}
