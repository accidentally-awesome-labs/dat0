//! The charts pane (5.4).
//!
//! # What changed, and why
//!
//! The GPUI build drew a chart with `plotters` into a BGRA buffer, wrapped it
//! in a `gpui::RenderImage` and blitted it through `img()` — which is why
//! `image` and `smallvec` were dependencies, why the bitmap was supersampled
//! 2×, and why the chart was an opaque rectangle no test could look inside.
//!
//! `dat0_core::charts::render::render_svg_with` already produces the same chart
//! as text. A WebView draws SVG natively, so the whole bitmap path is gone: no
//! image crate, no supersample factor to keep in step with the display, and a
//! chart that scales with its pane instead of blurring.
//!
//! # Colour
//!
//! Plotters' stock red/blue/green is a plotting library's palette, not an
//! application's. [`palette`] resolves the drawing colours from the very
//! `ThemeTokens` that `app.css` is generated from — so a chart is the same blue
//! as the focus ring, and switching theme restyles it. Resolved from the token
//! struct rather than by reading CSS back out of the document: the document is
//! a projection of the tokens, and reading a projection to recover its source
//! is how the two drift apart.
//!
//! # Loading
//!
//! This component renders; it does not query. The plot query lives in the async
//! layer (5.9) and reaches the pane through [`ChartLoad`], whose supersede
//! counter is the port of the shell's `chart_load_id`: a slow chart may never
//! overwrite a newer one.

use dioxus::prelude::*;

use dat0_core::charts::data::PlotTable;
use dat0_core::charts::panel::{column_options, visible_axes};
use dat0_core::charts::render::{Palette, render_svg_with};
use dat0_core::charts::spec::{AxisRole, ChartSpec, ChartType};
use dat0_core::theme::tokens::ThemeTokens;

use crate::a11y::AccessRole;
use crate::components::pane::Pane;
use crate::state::Workspace;

/// The chart's logical size, in CSS pixels.
///
/// The same 520×360 the GPUI dock rendered at. It is an *aspect ratio* now
/// rather than a pixel budget — the SVG scales to the pane — but plotters lays
/// captions, axis labels and margins out in absolute units, so the number still
/// decides how crowded a chart looks.
pub const CHART_SIZE: (u32, u32) = (520, 360);

// ── Render state ─────────────────────────────────────────────────────────────

/// What the chart body is showing.
#[derive(Clone, PartialEq, Debug, Default)]
pub enum ChartRender {
    /// No chart yet: nothing bound, or no axis picked.
    #[default]
    Empty,
    /// A rendered chart, as an SVG document.
    Svg(String),
    /// The plot query or the spec failed; the text is shown in place of a chart.
    Error(String),
}

/// The chart body's render state plus the monotonic supersede counter.
///
/// Port of `WorkspaceShell::chart_load_id`. Every config change [`begin`]s a
/// load and gets back the id its completion must quote; a completion whose id
/// is stale is dropped. Without it, cycling chart type three times leaves
/// whichever query happened to finish last on screen — which is not necessarily
/// the one the toolbar is describing.
///
/// [`begin`]: ChartLoad::begin
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ChartLoad {
    render: ChartRender,
    id: u64,
}

impl ChartLoad {
    /// A load already settled on `render`. For restored state and for tests.
    pub fn ready(render: ChartRender) -> Self {
        Self { render, id: 0 }
    }

    pub fn render(&self) -> &ChartRender {
        &self.render
    }

    /// The id the next completion must quote to be accepted.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Begin a load. Returns the id its completion must quote.
    ///
    /// Deliberately leaves the current render in place: a reload that blanked
    /// the pane first would flash empty on every axis click.
    pub fn begin(&mut self) -> u64 {
        self.id = self.id.wrapping_add(1);
        self.id
    }

    /// Apply a completed load. `false` when a newer load has already begun, in
    /// which case nothing changes.
    pub fn apply(&mut self, id: u64, render: ChartRender) -> bool {
        if id != self.id {
            return false;
        }
        self.render = render;
        true
    }
}

/// A request for a new chart: the spec the user just asked for, and the load id
/// its result must quote.
#[derive(Clone, PartialEq, Debug)]
pub struct ChartRequest {
    pub load_id: u64,
    pub spec: ChartSpec,
}

// ── Colour ───────────────────────────────────────────────────────────────────

/// The chart palette for a theme.
///
/// Series order is deliberate, because each single-series chart type takes a
/// fixed index (`Palette::LINE`/`SCATTER`/`BAR`): accent, then the SQL syntax
/// series, which is the app's only other hand-tuned set of mutually legible
/// hues. Line comes out accent-blue, scatter purple, bar green.
pub fn palette(t: &ThemeTokens) -> Palette {
    Palette::from_css(
        &t.surface,
        &t.fg,
        &[
            &t.accent,
            &t.sql_keyword,
            &t.sql_string,
            &t.sql_number,
            &t.sql_fn,
            &t.sql_comment,
        ],
    )
}

/// Render a chart for the pane, in the app's palette. The plot loader (5.9)
/// calls this and hands the string to [`ChartLoad::apply`].
pub fn render_chart(spec: &ChartSpec, data: &PlotTable, tokens: &ThemeTokens) -> String {
    render_svg_with(spec, data, CHART_SIZE, &palette(tokens))
}

// ── Axis binding ─────────────────────────────────────────────────────────────

/// Read the spec field bound to `role`.
fn axis_field(spec: &ChartSpec, role: AxisRole) -> Option<&str> {
    match role {
        AxisRole::X => spec.x.as_deref(),
        AxisRole::Y => spec.y.as_deref(),
        AxisRole::Group => spec.group.as_deref(),
        AxisRole::Color => spec.color.as_deref(),
        // BoxPlot value → y; Heatmap value → color (per query.rs contract).
        AxisRole::Value => match spec.chart_type {
            ChartType::Heatmap => spec.color.as_deref(),
            _ => spec.y.as_deref(),
        },
    }
}

/// Write `val` into the spec field bound to `role`.
fn set_axis_field(spec: &mut ChartSpec, role: AxisRole, val: Option<String>) {
    match role {
        AxisRole::X => spec.x = val,
        AxisRole::Y => spec.y = val,
        AxisRole::Group => spec.group = val,
        AxisRole::Color => spec.color = val,
        AxisRole::Value => match spec.chart_type {
            ChartType::Heatmap => spec.color = val,
            _ => spec.y = val,
        },
    }
}

/// i18n key for an axis role's short label.
fn axis_role_key(role: AxisRole) -> &'static str {
    match role {
        AxisRole::X => "chart.axis.x",
        AxisRole::Y => "chart.axis.y",
        AxisRole::Group => "chart.axis.group",
        AxisRole::Color => "chart.axis.color",
        AxisRole::Value => "chart.axis.value",
    }
}

/// Stable, un-localised handle for an axis role — the `data-a11y-id` suffix.
/// Separate from the i18n key so a test selector never depends on copy.
fn axis_slug(role: AxisRole) -> &'static str {
    match role {
        AxisRole::X => "x",
        AxisRole::Y => "y",
        AxisRole::Group => "group",
        AxisRole::Color => "color",
        AxisRole::Value => "value",
    }
}

/// Whether a role must always carry a column (X + the value axes) vs may be
/// cleared (Group / Color are optional dims that default to COUNT/none).
fn axis_required(role: AxisRole) -> bool {
    matches!(role, AxisRole::X | AxisRole::Y | AxisRole::Value)
}

/// Advance an axis pick through `opts`. `required` axes cycle only over the
/// options (wrapping); optional axes additionally pass through `None` so the
/// user can clear a Group/Color dim. Picks not in `opts` (stale) reset to the
/// first option (or `None` for optional).
fn cycle_axis(current: Option<&str>, opts: &[String], required: bool) -> Option<String> {
    if opts.is_empty() {
        return None;
    }
    let pos = current.and_then(|c| opts.iter().position(|o| o == c));
    match (required, pos) {
        // Required: just wrap over the options.
        (true, Some(i)) => Some(opts[(i + 1) % opts.len()].clone()),
        (true, None) => Some(opts[0].clone()),
        // Optional: order is None → opt0 → … → optN → None → …
        (false, None) => Some(opts[0].clone()),
        (false, Some(i)) if i + 1 < opts.len() => Some(opts[i + 1].clone()),
        (false, Some(_)) => None,
    }
}

/// The next chart type in the cycle.
fn next_type(cur: ChartType) -> ChartType {
    let i = ChartType::ALL.iter().position(|t| *t == cur).unwrap_or(0);
    ChartType::ALL[(i + 1) % ChartType::ALL.len()]
}

/// A bound source for display: the spec carries a quoted identifier
/// (`"orders"`), which is right for SQL and wrong for a header.
fn source_label(source: &str) -> String {
    source.replace('"', "")
}

// ── Component ────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Props)]
pub struct ChartsProps {
    /// The live chart spec. Owned by the shell, because it is also what the
    /// plot query is built from; this component proposes changes to it through
    /// `on_config` and re-reads the result.
    pub spec: ChartSpec,
    /// `(name, duckdb_type)` of the bound source, for the axis pickers.
    #[props(default)]
    pub columns: Vec<(String, String)>,
    /// The bound source (a quoted identifier), or `None` when nothing is bound.
    #[props(default)]
    pub source: Option<String>,
    /// Render state and supersede counter. A `Signal` because the async loader
    /// writes into it from outside this component's render.
    pub state: Signal<ChartLoad>,
    /// The user changed the chart configuration.
    pub on_config: EventHandler<ChartRequest>,
    /// The user asked to save this chart under a name. The shell opens the
    /// name prompt; this component never owns a modal.
    pub on_save: EventHandler<()>,
}

/// The charts pane.
///
/// Open state is `DockLayout::charts_visible` (S5): the right column is a stack
/// of two independently collapsible panes, not a reserved split, so closing
/// this one gives its height back to the inspector — or the whole column back
/// to the grid.
#[component]
pub fn Charts(props: ChartsProps) -> Element {
    let mut ws = Workspace::use_current();
    let open = ws.layout.read().charts_visible;

    let spec = props.spec.clone();
    let kind = dat0_i18n::t(spec.chart_type.label_key());
    let title = match props.source.as_deref() {
        Some(s) => source_label(s),
        None => dat0_i18n::t("chart.panel.title"),
    };

    // Save is gated exactly as the GPUI toolbar gated it: a source must be
    // bound and at least one axis picked, so an empty chart can never be
    // saved. Enforced as `disabled` rather than as a silent no-op, so the
    // affordance reads correctly.
    let can_save = props.source.is_some() && (spec.x.is_some() || spec.y.is_some());

    rsx! {
        Pane {
            id: "charts".to_string(),
            title,
            meta: kind.clone(),
            open,
            on_toggle: move |_| {
                let v = ws.layout.read().charts_visible;
                ws.layout.write().charts_visible = !v;
            },

            div { class: "d0-chart",
                Toolbar {
                    spec: spec.clone(),
                    columns: props.columns.clone(),
                    can_save,
                    state: props.state,
                    on_config: props.on_config,
                    on_save: props.on_save,
                }
                Body { spec: spec.clone(), state: props.state }
            }
        }
    }
}

/// The chart-type cycle, one cycle button per visible axis, and Save.
///
/// Button-cycle rather than a `<select>`, carried over from GPUI: one click
/// advances the value and immediately requests a re-plot, so the data flow is
/// the same as a picker's, and the control reads its own current value.
#[component]
fn Toolbar(
    spec: ChartSpec,
    columns: Vec<(String, String)>,
    can_save: bool,
    mut state: Signal<ChartLoad>,
    on_config: EventHandler<ChartRequest>,
    on_save: EventHandler<()>,
) -> Element {
    let cur_type = spec.chart_type;
    let type_label = format!(
        "{}: {}",
        dat0_i18n::t("chart.panel.title"),
        dat0_i18n::t(cur_type.label_key())
    );

    let axes = visible_axes(cur_type);

    rsx! {
        div { class: "d0-chart-toolbar",

            button {
                class: "d0-btn d0-mono",
                "data-a11y-id": "chart-type",
                role: AccessRole::Button.aria(),
                "aria-label": type_label.clone(),
                onclick: {
                    let spec = spec.clone();
                    move |_| {
                        let mut next = spec.clone();
                        next.chart_type = next_type(next.chart_type);
                        // A new type may expose axes the old picks do not
                        // satisfy; the picks are left alone on purpose —
                        // build_plot_sql then reports "needs a <role> column"
                        // until the user picks one, which is more useful than
                        // silently clearing their work.
                        let load_id = state.write().begin();
                        on_config.call(ChartRequest { load_id, spec: next });
                    }
                },
                "{type_label}"
            }

            for role in axes {
                {
                    let current = axis_field(&spec, role).map(str::to_string);
                    let label = format!(
                        "{}: {}",
                        dat0_i18n::t(axis_role_key(role)),
                        current.clone().unwrap_or_else(|| "—".to_string())
                    );
                    let id = format!("chart-axis-{}", axis_slug(role));
                    let spec = spec.clone();
                    let columns = columns.clone();
                    rsx! {
                        button {
                            key: "{id}",
                            class: "d0-btn d0-mono",
                            "data-a11y-id": "{id}",
                            role: AccessRole::Button.aria(),
                            "aria-label": label.clone(),
                            onclick: move |_| {
                                let opts = column_options(role, &columns);
                                let mut next = spec.clone();
                                let pick = cycle_axis(
                                    axis_field(&next, role),
                                    &opts,
                                    axis_required(role),
                                );
                                set_axis_field(&mut next, role, pick);
                                let load_id = state.write().begin();
                                on_config.call(ChartRequest { load_id, spec: next });
                            },
                            "{label}"
                        }
                    }
                }
            }

            button {
                class: "d0-btn d0-mono",
                "data-a11y-id": "chart-save",
                role: AccessRole::Button.aria(),
                "aria-label": dat0_i18n::t("chart.save"),
                disabled: !can_save,
                onclick: move |_| on_save.call(()),
                {dat0_i18n::t("chart.save")}
            }
        }
    }
}

/// The chart itself, or the state that stands in for it.
#[component]
fn Body(spec: ChartSpec, state: Signal<ChartLoad>) -> Element {
    let load = state.read();
    // The spec the pixels were drawn from, as data attributes. The GPUI build
    // emitted these as invisible AccessKit label nodes because a blitted
    // bitmap was opaque to any assertion; an SVG is not opaque, but the title
    // still appears nowhere else in the DOM, and a data attribute costs a
    // reader nothing whereas four unlabelled notes cost it four announcements.
    let x = spec.x.clone().unwrap_or_default();
    let y = spec.y.clone().unwrap_or_default();

    rsx! {
        div {
            class: "d0-chart-canvas",
            "data-a11y-id": "chart-body",
            "data-chart-type": dat0_i18n::t(spec.chart_type.label_key()),
            "data-chart-x": "{x}",
            "data-chart-y": "{y}",
            "data-chart-title": "{spec.title}",

            match load.render() {
                ChartRender::Svg(svg) => rsx! {
                    div {
                        class: "d0-chart-svg",
                        "data-a11y-id": "chart-svg",
                        role: AccessRole::Label.aria(),
                        "aria-label": dat0_i18n::t(spec.chart_type.label_key()),
                        dangerous_inner_html: "{svg}",
                    }
                },
                ChartRender::Error(msg) => rsx! {
                    div {
                        class: "d0-chart-error d0-mono",
                        "data-a11y-id": "chart-error",
                        role: AccessRole::Alert.aria(),
                        "aria-label": "{msg}",
                        "{msg}"
                    }
                },
                ChartRender::Empty => rsx! {
                    div {
                        class: "d0-chart-empty d0-mono",
                        "data-a11y-id": "chart-empty",
                        role: AccessRole::Label.aria(),
                        "aria-label": dat0_i18n::t("chart.panel.empty"),
                        {dat0_i18n::t("chart.panel.empty")}
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(t: ChartType) -> ChartSpec {
        ChartSpec {
            chart_type: t,
            source: "\"t\"".into(),
            x: None,
            y: None,
            group: None,
            color: None,
            title: String::new(),
        }
    }

    #[test]
    fn required_axis_cycles_over_options_only() {
        let opts = vec!["a".to_string(), "b".to_string()];
        // None → first; a → b; b → wrap to a. Required never returns None.
        assert_eq!(cycle_axis(None, &opts, true), Some("a".into()));
        assert_eq!(cycle_axis(Some("a"), &opts, true), Some("b".into()));
        assert_eq!(cycle_axis(Some("b"), &opts, true), Some("a".into()));
        // Stale pick (not in opts) resets to the first option.
        assert_eq!(cycle_axis(Some("zzz"), &opts, true), Some("a".into()));
        // No options → None even when required (nothing to pick).
        assert_eq!(cycle_axis(None, &[], true), None);
    }

    #[test]
    fn optional_axis_passes_through_none() {
        let opts = vec!["a".to_string(), "b".to_string()];
        // None → a → b → None → a (None is a real step for optional dims).
        assert_eq!(cycle_axis(None, &opts, false), Some("a".into()));
        assert_eq!(cycle_axis(Some("a"), &opts, false), Some("b".into()));
        assert_eq!(cycle_axis(Some("b"), &opts, false), None);
    }

    #[test]
    fn value_axis_maps_to_the_field_each_type_reads() {
        // BoxPlot reads its value from spec.y; Heatmap from spec.color
        // (matches charts/query.rs build_plot_sql).
        let mut bx = spec(ChartType::BoxPlot);
        set_axis_field(&mut bx, AxisRole::Value, Some("amt".into()));
        assert_eq!(bx.y.as_deref(), Some("amt"));
        assert_eq!(bx.color, None);
        assert_eq!(axis_field(&bx, AxisRole::Value), Some("amt"));

        let mut hm = spec(ChartType::Heatmap);
        set_axis_field(&mut hm, AxisRole::Value, Some("cnt".into()));
        assert_eq!(hm.color.as_deref(), Some("cnt"));
        assert_eq!(hm.y, None);
        assert_eq!(axis_field(&hm, AxisRole::Value), Some("cnt"));
    }

    #[test]
    fn required_axes_classification() {
        assert!(axis_required(AxisRole::X));
        assert!(axis_required(AxisRole::Y));
        assert!(axis_required(AxisRole::Value));
        assert!(!axis_required(AxisRole::Group));
        assert!(!axis_required(AxisRole::Color));
    }

    #[test]
    fn every_chart_type_is_reachable_by_cycling() {
        let mut t = ChartType::ALL[0];
        let mut seen = vec![t];
        for _ in 1..ChartType::ALL.len() {
            t = next_type(t);
            seen.push(t);
        }
        assert_eq!(seen.len(), ChartType::ALL.len());
        assert_eq!(next_type(t), ChartType::ALL[0], "the cycle must wrap");
    }

    #[test]
    fn a_stale_completion_never_overwrites_a_newer_one() {
        let mut load = ChartLoad::ready(ChartRender::Svg("first".into()));
        let a = load.begin();
        let b = load.begin();
        assert!(!load.apply(a, ChartRender::Svg("slow".into())));
        assert_eq!(load.render(), &ChartRender::Svg("first".into()));
        assert!(load.apply(b, ChartRender::Svg("fresh".into())));
        assert_eq!(load.render(), &ChartRender::Svg("fresh".into()));
    }

    #[test]
    fn the_palette_comes_from_the_tokens() {
        let t = dat0_core::theme::builtin_or_default("light");
        let p = palette(&t);
        assert_eq!(
            p.background,
            dat0_core::charts::render::Palette::from_css(&t.surface, &t.fg, &[]).background
        );
        assert_eq!(p.series.len(), 6);
        assert_ne!(
            p,
            Palette::legacy(),
            "the chart must not draw in plotters' colours"
        );
    }

    #[test]
    fn a_quoted_source_reads_as_a_plain_name_in_the_header() {
        assert_eq!(source_label("\"orders\""), "orders");
    }
}
