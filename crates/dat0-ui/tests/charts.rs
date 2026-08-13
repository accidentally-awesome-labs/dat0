//! The charts pane.
//!
//! The pane is deliberately drivable from props alone — the plot query is the
//! async layer's job (5.9) — so every test here mounts a small driver that
//! plays the part of the shell: it owns the spec, records the config requests
//! the pane emits, and delivers results into [`ChartLoad`] the way a finished
//! query would.

mod support;

use dioxus::prelude::*;
use support::Harness;

use dat0_core::charts::spec::{ChartSpec, ChartType};
use dat0_ui::components::charts::{ChartLoad, ChartRender, ChartRequest, Charts};
use dat0_ui::state::Workspace;

/// Everything the driver needs to stand in for the shell.
#[derive(Clone, PartialEq, Props)]
struct DriverProps {
    spec: ChartSpec,
    columns: Vec<(String, String)>,
    source: Option<String>,
    /// What the pane starts out showing.
    start: ChartRender,
    /// Start with the pane collapsed, to prove the body survives a collapse.
    open: bool,
}

impl Default for DriverProps {
    fn default() -> Self {
        Self {
            spec: ChartSpec {
                chart_type: ChartType::Bar,
                source: "\"orders\"".into(),
                x: None,
                y: None,
                group: None,
                color: None,
                title: String::new(),
            },
            columns: vec![
                ("region".to_string(), "VARCHAR".to_string()),
                ("amount".to_string(), "DOUBLE".to_string()),
                ("qty".to_string(), "BIGINT".to_string()),
            ],
            source: Some("\"orders\"".into()),
            start: ChartRender::Empty,
            open: true,
        }
    }
}

/// The shell's half of the contract, plus probes the tests act and assert on.
#[component]
fn Driver(props: DriverProps) -> Element {
    let mut ws = Workspace::provide();
    let mut spec = use_signal(|| props.spec.clone());
    let mut state = use_signal(|| ChartLoad::ready(props.start.clone()));
    // The last request the pane emitted, and how many it has emitted.
    let mut last = use_signal(|| None::<ChartRequest>);
    let mut requests = use_signal(|| 0usize);
    let mut saves = use_signal(|| 0usize);

    use_hook(move || {
        ws.layout.write().charts_visible = props.open;
    });

    rsx! {
        div {
            // Probes. Text, so a test reads them with `text_of`.
            span { "data-a11y-id": "probe-requests", "{requests}" }
            span { "data-a11y-id": "probe-saves", "{saves}" }
            span {
                "data-a11y-id": "probe-load-id",
                "{last.read().as_ref().map(|r| r.load_id).unwrap_or(0)}"
            }

            // A completion arriving with the id the pane handed out.
            button {
                "data-a11y-id": "deliver-fresh",
                onclick: move |_| {
                    let id = last.read().as_ref().map(|r| r.load_id).unwrap_or(0);
                    state.write().apply(id, ChartRender::Svg("<svg id=\"fresh\"></svg>".into()));
                },
                "fresh"
            }
            // A completion from the load *before* the current one: the slow
            // query that must never win.
            button {
                "data-a11y-id": "deliver-stale",
                onclick: move |_| {
                    let id = last.read().as_ref().map(|r| r.load_id).unwrap_or(0);
                    state.write().apply(
                        id.wrapping_sub(1),
                        ChartRender::Svg("<svg id=\"stale\"></svg>".into()),
                    );
                },
                "stale"
            }
            button {
                "data-a11y-id": "deliver-error",
                onclick: move |_| {
                    let id = last.read().as_ref().map(|r| r.load_id).unwrap_or(0);
                    state.write().apply(id, ChartRender::Error("chart needs an X column".into()));
                },
                "error"
            }

            Charts {
                spec: spec(),
                columns: props.columns.clone(),
                source: props.source.clone(),
                state,
                on_config: move |req: ChartRequest| {
                    // The shell owns the spec: it accepts the proposal, then
                    // the query it kicks off quotes `req.load_id`.
                    spec.set(req.spec.clone());
                    last.set(Some(req));
                    requests += 1;
                },
                on_save: move |_| saves += 1,
            }
        }
    }
}

fn mount(props: DriverProps) -> Harness {
    Harness::new(Driver, props)
}

fn probe(h: &Harness, id: &str) -> String {
    h.text_of(h.by_a11y_id(id).unwrap_or_else(|| panic!("no probe {id}")))
}

/// The SVG a finished query produced is what the pane body shows — no bitmap,
/// no image element, the markup itself.
#[test]
fn a_rendered_chart_is_the_svg_in_the_pane_body() {
    let svg = "<svg data-probe=\"chart\"><rect/></svg>";
    let h = mount(DriverProps {
        start: ChartRender::Svg(svg.to_string()),
        ..Default::default()
    });

    let body = h.by_a11y_id("chart-svg").expect("the chart body renders");
    assert_eq!(
        h.attr(body, "dangerous_inner_html").as_deref(),
        Some(svg),
        "the SVG must be inlined, not linked or blitted"
    );
    // And it is inside the pane the dock mounts, not floating beside it.
    assert!(h.by_a11y_id("pane-body-charts").is_some());
    assert!(h.by_a11y_id("chart-empty").is_none());
}

/// The supersede guard, driven through the component: change the config, then
/// let the *previous* load finish. The port of `chart_load_id`.
#[test]
fn a_slow_load_never_overwrites_a_newer_one() {
    let mut h = mount(DriverProps::default());

    // Load 1: the user cycles the chart type.
    h.click("chart-type");
    assert_eq!(probe(&h, "probe-load-id"), "1");
    h.click("deliver-fresh");
    assert!(h.by_a11y_id("chart-svg").is_some());

    // Load 2: another cycle, then load 1 finally comes back.
    h.click("chart-type");
    assert_eq!(probe(&h, "probe-load-id"), "2");
    h.click("deliver-stale");

    let body = h.by_a11y_id("chart-svg").expect("the earlier chart stays");
    assert_eq!(
        h.attr(body, "dangerous_inner_html").as_deref(),
        Some("<svg id=\"fresh\"></svg>"),
        "a superseded load overwrote a newer one"
    );

    // Load 2's own result is still accepted.
    h.click("deliver-fresh");
    assert!(h.by_a11y_id("chart-svg").is_some());
}

/// Nothing bound, nothing picked: the hint, not a blank rectangle.
#[test]
fn the_empty_state_shows_when_there_is_no_chart() {
    let h = mount(DriverProps::default());

    assert!(h.by_a11y_id("chart-empty").is_some());
    assert!(h.by_a11y_id("chart-svg").is_none());
    assert!(h.has_label(&dat0_i18n::t("chart.panel.empty")));
}

/// A failed spec or query shows its message in place of the chart, and
/// announces it — a chart that silently stops updating is the failure mode
/// this replaces.
#[test]
fn the_error_state_renders_the_message() {
    let mut h = mount(DriverProps {
        start: ChartRender::Svg("<svg/>".into()),
        ..Default::default()
    });

    h.click("chart-type");
    h.click("deliver-error");

    let err = h.by_a11y_id("chart-error").expect("the error renders");
    assert_eq!(h.text_of(err), "chart needs an X column");
    assert_eq!(h.attr(err, "role").as_deref(), Some("alert"));
    assert!(
        h.by_a11y_id("chart-svg").is_none(),
        "the stale chart must not sit under an error"
    );
}

/// Cycling the type asks the shell to re-plot, with the next type in the ring.
#[test]
fn changing_the_chart_type_requests_a_replot() {
    let mut h = mount(DriverProps::default());
    assert_eq!(probe(&h, "probe-requests"), "0");

    h.click("chart-type");

    assert_eq!(probe(&h, "probe-requests"), "1");
    // Bar is ChartType::ALL[0]; the ring's next entry is Line.
    assert_eq!(
        h.attr(h.by_a11y_id("chart-body").unwrap(), "data-chart-type")
            .as_deref(),
        Some(dat0_i18n::t("chart.type.line").as_str())
    );
    assert_eq!(
        h.attr(h.by_a11y_id("pane-head-charts").unwrap(), "aria-expanded")
            .as_deref(),
        Some("true")
    );
}

/// Each axis button advances its own pick, and the optional dims can be
/// cleared while the required ones cannot.
#[test]
fn axis_buttons_cycle_their_pick() {
    // Scatter exposes X, Y and Color; Y is numeric-only, Color takes anything.
    let mut props = DriverProps::default();
    props.spec.chart_type = ChartType::Scatter;
    let mut h = mount(props);

    // X is required: it cycles over every column and never clears.
    h.click("chart-axis-x");
    assert!(h.has_label_contains("region"));
    h.click("chart-axis-x");
    assert!(h.has_label_contains("amount"));

    // Y is numeric-only, so `region` is not an option for it.
    h.click("chart-axis-y");
    assert!(
        h.has_label(&format!("{}: amount", dat0_i18n::t("chart.axis.y"))),
        "Y skipped the text column"
    );

    // Color is optional and takes any column, so its ring is
    // None → region → amount → qty → None: three clicks land on the last
    // option, the fourth clears it. Required axes have no such step.
    for _ in 0..3 {
        h.click("chart-axis-color");
    }
    assert!(h.has_label(&format!("{}: qty", dat0_i18n::t("chart.axis.color"))));
    h.click("chart-axis-color");
    assert!(h.has_label(&format!("{}: —", dat0_i18n::t("chart.axis.color"))));
}

/// Which axes exist is a property of the chart type, not of the toolbar.
#[test]
fn the_toolbar_shows_only_the_axes_the_type_uses() {
    let mut props = DriverProps::default();
    props.spec.chart_type = ChartType::Histogram;
    let h = mount(props);

    // Histogram is single-axis.
    assert!(h.by_a11y_id("chart-axis-x").is_some());
    assert!(h.by_a11y_id("chart-axis-y").is_none());
    assert!(h.by_a11y_id("chart-axis-color").is_none());
}

/// Save is gated on a chart that could actually be saved: a bound source and
/// at least one axis. Disabled rather than a silent no-op, so the affordance
/// tells the truth.
#[test]
fn save_is_disabled_until_there_is_something_to_save() {
    let mut h = mount(DriverProps {
        source: None,
        ..Default::default()
    });
    let save = h.by_a11y_id("chart-save").unwrap();
    assert_eq!(h.attr(save, "disabled").as_deref(), Some("true"));

    // Bound + one axis picked → enabled, and clicking asks the shell to open
    // the name prompt.
    let mut props = DriverProps::default();
    props.spec.x = Some("region".into());
    h = mount(props);
    let save = h.by_a11y_id("chart-save").unwrap();
    assert_ne!(h.attr(save, "disabled").as_deref(), Some("true"));
    h.click("chart-save");
    assert_eq!(probe(&h, "probe-saves"), "1");
}

/// The pane header is the open/close control, and it drives
/// `DockLayout::charts_visible` — the same bit the right column sizes itself
/// from (S5).
#[test]
fn the_header_toggles_the_layout_bit() {
    let mut h = mount(DriverProps {
        open: false,
        ..Default::default()
    });

    let head = h.by_a11y_id("pane-head-charts").unwrap();
    assert_eq!(h.attr(head, "aria-expanded").as_deref(), Some("false"));

    h.click("pane-head-charts");
    let head = h.by_a11y_id("pane-head-charts").unwrap();
    assert_eq!(h.attr(head, "aria-expanded").as_deref(), Some("true"));

    // The body is not unmounted by a collapse, so a chart survives it.
    h.click("pane-head-charts");
    assert!(h.by_a11y_id("chart-body").is_some());
}

/// The header meta is the chart kind (S4), and it follows the type.
#[test]
fn the_pane_header_names_the_source_and_the_kind() {
    let mut h = mount(DriverProps::default());
    let head = h.by_a11y_id("pane-head-charts").unwrap();
    let text = h.text_of(head);
    assert!(text.contains("orders"), "header lost the source: {text}");
    assert!(
        text.contains(&dat0_i18n::t("chart.type.bar")),
        "header lost the kind: {text}"
    );

    h.click("chart-type");
    let head = h.by_a11y_id("pane-head-charts").unwrap();
    assert!(h.text_of(head).contains(&dat0_i18n::t("chart.type.line")));
}
