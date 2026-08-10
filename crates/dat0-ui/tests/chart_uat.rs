//! Charts UAT: the spec the pane is describing, and the chart it is actually
//! showing.
//!
//! Port of `dat0-app/tests/chart_uat_window.rs`. That suite existed because a
//! GPUI chart was **opaque**: `plotters` drew into a BGRA buffer, the buffer
//! became a `RenderImage`, and `img()` blitted it. No assertion could look
//! inside, so the panel emitted four invisible AccessKit label nodes carrying
//! the type, the axes and the title, and the UAT asserted on those seams — a
//! proxy for the picture, never the picture.
//!
//! The bitmap is gone. `render_chart` produces SVG, the pane drops it into the
//! DOM, and the markup is readable, so this file asserts both halves: the spec
//! seams (now data attributes, one announcement instead of four) **and the SVG
//! itself** — that it is the string the renderer produced, drawn in the app's
//! palette rather than plotters' stock red/blue/green.
//!
//! `tests/charts.rs` already covers the pane's own behaviour — empty state,
//! error state, supersede, axis cycling, save gating on an unbound source, pane
//! header. Nothing here repeats it.
//!
//! ## Two guarantees from the original that have no home yet
//!
//! `save_chart_shows_toast_and_persists` and `saved_chart_appears_as_lineage_node`
//! are **not** ported, because the code they would test does not exist in the
//! Dioxus build — the shell's `on_save` is `move |_| {}` and nothing maps a
//! `SavedChart` onto a `lineage::ChartNode` (GPUI did it in
//! `window/catalog_inspector.rs`). Writing them against `dat0-core`'s
//! `upsert_chart` would be a green test for a feature the user cannot reach.
//! Both halves of each are covered where they *are* real: persistence by
//! `dat0-core/tests/package_roundtrip.rs`, attachment by
//! `inspector::lineage`'s own tests, and the click routing by
//! `dat0-ui/tests/inspector.rs`. What is missing is the wiring between them.

mod support;

use dioxus::prelude::*;
use support::Harness;

use dat0_core::charts::data::{PlotColumn, PlotTable};
use dat0_core::charts::spec::{ChartSpec, ChartType};
use dat0_core::session::charts::SavedChart;
use dat0_core::theme::tokens::{ThemeTokens, builtin};
use dat0_ui::components::charts::{ChartLoad, ChartRender, ChartRequest, Charts, render_chart};
use dat0_ui::state::Workspace;

#[derive(Clone, PartialEq, Props)]
struct DriverProps {
    spec: ChartSpec,
    columns: Vec<(String, String)>,
    source: Option<String>,
    /// What the pane starts out showing — a finished load, delivered the way
    /// the async layer would deliver it.
    start: ChartRender,
}

impl Default for DriverProps {
    fn default() -> Self {
        Self {
            spec: bar_spec(),
            columns: vec![
                ("region".to_string(), "VARCHAR".to_string()),
                ("amt".to_string(), "DOUBLE".to_string()),
            ],
            source: Some("\"sales\"".into()),
            start: ChartRender::Empty,
        }
    }
}

/// The shell's half of the contract: it owns the spec and the load state.
#[component]
fn Driver(props: DriverProps) -> Element {
    let mut ws = Workspace::provide();
    let mut spec = use_signal(|| props.spec.clone());
    let state = use_signal(|| ChartLoad::ready(props.start.clone()));
    let mut saves = use_signal(|| 0usize);

    use_hook(move || {
        ws.layout.write().charts_visible = true;
    });

    rsx! {
        div {
            span { "data-a11y-id": "probe-saves", "{saves}" }
            Charts {
                spec: spec(),
                columns: props.columns.clone(),
                source: props.source.clone(),
                state,
                on_config: move |req: ChartRequest| spec.set(req.spec),
                on_save: move |_| saves += 1,
            }
        }
    }
}

fn mount(props: DriverProps) -> Harness {
    Harness::new(Driver, props)
}

fn bar_spec() -> ChartSpec {
    ChartSpec {
        chart_type: ChartType::Bar,
        source: "\"sales\"".into(),
        x: None,
        y: None,
        group: None,
        color: None,
        title: String::new(),
    }
}

/// The seams the GPUI panel emitted as invisible label nodes, now one data
/// attribute each on the chart body.
fn seam(h: &Harness, name: &str) -> String {
    let body = h.by_a11y_id("chart-body").expect("the chart body renders");
    h.attr(body, name)
        .unwrap_or_else(|| panic!("the body carries no {name}"))
}

/// `region → amt`, three rows over two regions. Column order is the positional
/// contract `charts/query.rs` builds and `render.rs` reads: key, then value.
fn sales() -> PlotTable {
    PlotTable {
        columns: vec![
            PlotColumn {
                name: "region".into(),
                num: None,
                text: Some(vec!["West".into(), "East".into(), "West".into()]),
            },
            PlotColumn {
                name: "amt".into(),
                num: Some(vec![10.0, 20.0, 5.0]),
                text: None,
            },
        ],
        rows: 3,
    }
}

fn tokens(id: &str) -> ThemeTokens {
    builtin(id).expect("builtin theme")
}

/// A bound chart describes itself completely: type, both axes and the title.
///
/// The direct port of `spike_bound_chart_renders_spec_content`. The title is
/// the one that matters most — it appears nowhere else in the DOM, so without
/// this seam a chart captioned with the wrong table is invisible to every test
/// and to a screen reader alike.
#[test]
fn a_bound_chart_reports_its_whole_spec() {
    let mut spec = bar_spec();
    spec.x = Some("region".into());
    spec.y = Some("amt".into());
    spec.title = "Sales by region".into();

    let h = mount(DriverProps {
        spec,
        ..Default::default()
    });

    assert_eq!(seam(&h, "data-chart-type"), dat0_i18n::t("chart.type.bar"));
    assert_eq!(seam(&h, "data-chart-x"), "region");
    assert_eq!(seam(&h, "data-chart-y"), "amt");
    assert_eq!(seam(&h, "data-chart-title"), "Sales by region");

    // And the toolbar reads back the same picks, so the control and the body
    // cannot disagree about what is plotted.
    assert!(h.has_label(&format!("{}: region", dat0_i18n::t("chart.axis.x"))));
    assert!(h.has_label(&format!("{}: amt", dat0_i18n::t("chart.axis.y"))));
}

/// Each type reports its own identity, not the default one.
///
/// Port of `chart_panel_renders_scatter_axes`. The failure it guards is a seam
/// wired to a constant: every assertion about "the chart is a Bar" passes
/// forever, including for the scatter.
#[test]
fn each_chart_type_reports_its_own_identity() {
    for (ty, key) in [
        (ChartType::Bar, "chart.type.bar"),
        (ChartType::Scatter, "chart.type.scatter"),
        (ChartType::Line, "chart.type.line"),
        (ChartType::Histogram, "chart.type.histogram"),
    ] {
        let mut spec = bar_spec();
        spec.chart_type = ty;
        spec.x = Some("region".into());
        spec.y = Some("amt".into());

        let h = mount(DriverProps {
            spec,
            ..Default::default()
        });
        assert_eq!(seam(&h, "data-chart-type"), dat0_i18n::t(key), "{ty:?}");
        assert_eq!(seam(&h, "data-chart-x"), "region", "{ty:?}");
    }
}

/// What the pane shows is the SVG the renderer drew — the markup itself.
///
/// This is the assertion the GPUI suite could not make. The chart went through
/// `plotters → BGRA → RenderImage → img()`, so the strongest available claim
/// was "a label node beside the picture says Bar". Here the picture *is* the
/// assertion: the pane's inner HTML is byte-for-byte what `render_chart`
/// produced, it is an `<svg>` document, it changes when the data changes, and
/// there is no `<img>` anywhere in the tree.
#[test]
fn the_pane_shows_the_svg_the_renderer_drew_and_no_bitmap() {
    let mut spec = bar_spec();
    spec.x = Some("region".into());
    spec.y = Some("amt".into());
    spec.title = "Sales by region".into();

    let svg = render_chart(&spec, &sales(), &tokens("light"));
    assert!(
        svg.starts_with("<svg"),
        "the renderer produced no SVG document"
    );

    let h = mount(DriverProps {
        spec: spec.clone(),
        start: ChartRender::Svg(svg.clone()),
        ..Default::default()
    });

    let body = h.by_a11y_id("chart-svg").expect("the chart renders");
    assert_eq!(
        h.attr(body, "dangerous_inner_html").as_deref(),
        Some(svg.as_str()),
        "the pane is showing something other than the chart it was handed"
    );

    // The drawing is real, in two senses. The caption is in the markup…
    assert!(svg.contains("Sales by region"), "the caption was not drawn");
    // …and the plot is a function of the data rather than a fixed picture.
    // Asserted by re-rendering with different values rather than by looking
    // for a category label: `render::bar` plots against an indexed x-axis and
    // deliberately does not draw the category names, so an assertion on
    // "West" would be testing a renderer that never promised it.
    let mut other = sales();
    other.columns[1].num = Some(vec![1.0, 2.0, 3.0]);
    assert_ne!(
        render_chart(&spec, &other, &tokens("light")),
        svg,
        "the chart is identical for different data — nothing is being plotted"
    );

    // No raster path survives anywhere: not in the pane, not in the tree.
    let html = h.html();
    assert!(
        !html.contains("<img"),
        "an <img> element is back in the chart pane"
    );
    assert!(!html.contains("data:image"), "a data-URI bitmap is back");
}

/// The chart is painted in the app's palette, and follows a theme switch.
///
/// `plotters`' stock red/blue/green is a plotting library's palette, not an
/// application's — a chart in it reads as a screenshot from another program
/// pasted into the pane. The palette is resolved from the same `ThemeTokens`
/// `app.css` is generated from, so light and dark must produce genuinely
/// different markup.
#[test]
fn the_chart_is_drawn_in_the_active_theme_not_in_plotters_defaults() {
    let mut spec = bar_spec();
    spec.x = Some("region".into());
    spec.y = Some("amt".into());

    let light_tokens = tokens("light");
    let dark_tokens = tokens("dark");
    let light = render_chart(&spec, &sales(), &light_tokens);
    let dark = render_chart(&spec, &sales(), &dark_tokens);

    assert_ne!(light, dark, "the chart ignores the theme");

    let has = |svg: &str, hex: &str| svg.to_ascii_lowercase().contains(&hex.to_ascii_lowercase());
    assert!(
        has(&light, &light_tokens.surface),
        "the light chart is not drawn on the light surface"
    );
    assert!(
        has(&dark, &dark_tokens.surface),
        "the dark chart is not drawn on the dark surface"
    );
    for svg in [&light, &dark] {
        assert!(
            !has(svg, "#ff0000"),
            "plotters' stock red survived a themed render"
        );
    }

    // And the pane shows whichever one it is handed, unchanged.
    let h = mount(DriverProps {
        spec,
        start: ChartRender::Svg(dark.clone()),
        ..Default::default()
    });
    assert_eq!(
        h.attr(h.by_a11y_id("chart-svg").unwrap(), "dangerous_inner_html")
            .as_deref(),
        Some(dark.as_str())
    );
}

/// Save is refused for a chart that could not be reopened.
///
/// Ports both no-op guards the GPUI shell enforced *after* the click
/// (`save_named_chart` returned early on an unbound source, and on a
/// whitespace-only name), for the half that moved onto the affordance: the
/// button is disabled rather than silently doing nothing, so the control tells
/// the truth about what will happen. The empty-name half now lives in the name
/// prompt, which disables its own Save.
#[test]
fn a_chart_with_nothing_to_save_cannot_be_saved() {
    // No source bound: nothing to re-query on reopen.
    let unbound = mount(DriverProps {
        source: None,
        ..Default::default()
    });
    assert_eq!(
        unbound
            .attr(unbound.by_a11y_id("chart-save").unwrap(), "disabled")
            .as_deref(),
        Some("true")
    );

    // Source bound but no axis picked: a saved spec that plots nothing.
    let axisless = mount(DriverProps::default());
    assert_eq!(
        axisless
            .attr(axisless.by_a11y_id("chart-save").unwrap(), "disabled")
            .as_deref(),
        Some("true"),
        "a bound source with no axis is still not a chart"
    );

    // Both satisfied: enabled, and the click reaches the shell.
    let mut spec = bar_spec();
    spec.x = Some("region".into());
    let mut ready = mount(DriverProps {
        spec,
        ..Default::default()
    });
    assert_ne!(
        ready
            .attr(ready.by_a11y_id("chart-save").unwrap(), "disabled")
            .as_deref(),
        Some("true")
    );
    ready.click("chart-save");
    assert_eq!(
        ready.text_of(ready.by_a11y_id("probe-saves").unwrap()),
        "1",
        "the enabled Save must ask the shell for a name"
    );
}

/// A persisted chart reopens with its spec intact, not blanked.
///
/// The half of `click_lineage_chart_reopens_panel_with_restored_spec` that
/// still has an implementation: given a `SavedChart`'s spec, the pane must show
/// every field of it — the type, both axes and the title. The other half (the
/// lineage click that produces this spec) is asserted in `tests/inspector.rs`,
/// which drives a `NodeKind::Chart` row and checks the shell is asked to reopen
/// it by name. What joins them is the wiring noted in this file's header.
#[test]
fn a_saved_chart_reopens_with_its_spec_intact() {
    let saved = SavedChart {
        // Fixed, never `now_v7`/`now_unix_millis`: a reopen test that depends
        // on the clock is a reopen test that fails on someone else's machine.
        id: uuid::Uuid::from_u128(1),
        name: "Region totals".into(),
        spec: ChartSpec {
            chart_type: ChartType::Scatter,
            source: "\"sales\"".into(),
            x: Some("region".into()),
            y: Some("amt".into()),
            group: None,
            color: None,
            title: "Region totals".into(),
        },
        saved_at: 1_700_000_000_000,
    };

    let h = mount(DriverProps {
        spec: saved.spec.clone(),
        source: Some(saved.spec.source.clone()),
        ..Default::default()
    });

    assert_eq!(
        seam(&h, "data-chart-type"),
        dat0_i18n::t("chart.type.scatter"),
        "the restored type was replaced by the default"
    );
    assert_eq!(seam(&h, "data-chart-x"), "region");
    assert_eq!(seam(&h, "data-chart-y"), "amt");
    assert_eq!(seam(&h, "data-chart-title"), "Region totals");

    // The pane header names the source it was restored onto (S4), so a chart
    // reopened against the wrong table is visible rather than plausible.
    let head = h.by_a11y_id("pane-head-charts").expect("the pane header");
    let text = h.text_of(head);
    assert!(text.contains("sales"), "the header lost the source: {text}");
    assert!(
        text.contains(&dat0_i18n::t("chart.type.scatter")),
        "the header lost the kind: {text}"
    );
}
