//! The column inspector (5.5).
//!
//! Ported from the GPUI panel, so what this suite defends is the *behaviour*
//! that panel had, not its markup:
//!
//! * the three overview states — no target, loading, profiled;
//! * the flat lineage list and its depth clamp, which is the only thing keeping
//!   a deep chain inside a 320px dock;
//! * the projection-aware cards and the Hidden section's gate;
//! * the inline mini-chart, now inline SVG rather than a raster;
//! * the load-supersede rule, which is what stops a slow profile from painting
//!   table A's columns under table B's name;
//! * the pane's open state living in `DockLayout`, not in the panel.

mod support;

use dioxus::prelude::*;

use dat0_core::inspector::lineage::{ChainStep, EdgeKind, LineageChain, NodeKind};
use dat0_core::inspector::projection::ProjectionContext;
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{ColumnProfile, NumericStats, TableProfile};
use dat0_ui::components::inspector::{Inspector, InspectorState};
use dat0_ui::state::Workspace;
use support::Harness;

// ── fixtures ─────────────────────────────────────────────────────────────────

fn col(name: &str, ty: &str, distinct: u64) -> ColumnProfile {
    ColumnProfile {
        name: name.into(),
        ty: ty.into(),
        null_pct: 0.0,
        approx_distinct: distinct,
        count: 3,
        numeric: None,
        length: None,
    }
}

/// `id BIGINT`, `name VARCHAR`, `price DOUBLE` — the shape the card assertions
/// use. `price` carries numeric stats so its stat line is non-empty.
fn profile() -> TableProfile {
    let mut price = col("price", "DOUBLE", 900);
    price.null_pct = 0.3;
    price.numeric = Some(NumericStats {
        min: 0.0,
        max: 9942.0,
        avg: 41.2,
        std: 63.1,
        q25: 10.0,
        median: 28.0,
        q75: 55.0,
    });
    TableProfile {
        rows: 1000,
        columns: vec![col("id", "BIGINT", 1000), col("name", "VARCHAR", 4), price],
    }
}

/// A second, distinguishable profile: one column, so a card count separates it
/// from [`profile`] without relying on names.
fn other_profile() -> TableProfile {
    TableProfile {
        rows: 7,
        columns: vec![col("only", "VARCHAR", 2)],
    }
}

fn step(label: &str, depth: u32, kind: NodeKind, open: bool) -> ChainStep {
    ChainStep {
        label: label.into(),
        kind,
        edge: EdgeKind::SqlRef,
        depth,
        open_name: open.then(|| label.to_string()),
    }
}

// ── host ─────────────────────────────────────────────────────────────────────

/// Everything a scenario seeds into the panel, plus the drive buttons the
/// supersede test needs.
#[derive(Clone, Props, Default)]
struct HostProps {
    target: Option<String>,
    profile: Option<TableProfile>,
    lineage: LineageChain,
    /// `(source column, top-N pairs)` — the lazily-fetched chart data.
    topn: Vec<(String, Vec<(String, u64)>)>,
    projection: Option<ProjectionContext>,
    focus_column: Option<String>,
}

impl PartialEq for HostProps {
    fn eq(&self, other: &Self) -> bool {
        // `ProjectionContext` has no `PartialEq`; compare its two fields.
        let projection = match (&self.projection, &other.projection) {
            (None, None) => true,
            (Some(a), Some(b)) => a.visible == b.visible && a.base_sources == b.base_sources,
            _ => false,
        };
        self.target == other.target
            && self.profile == other.profile
            && self.lineage == other.lineage
            && self.topn == other.topn
            && projection
            && self.focus_column == other.focus_column
    }
}

/// Owns the workspace, the inspector state, and readback nodes — the harness
/// sees text, not Rust state.
#[component]
fn Host(props: HostProps) -> Element {
    let ws = Workspace::provide();
    let state = InspectorState::use_new();
    let mut reloads = use_signal(|| 0usize);
    let mut opened = use_signal(String::new);
    // Load ids handed out by `drive-begin`, in order, so a test can land them
    // out of order — which is the whole point of the supersede rule.
    let mut ids = use_signal(Vec::<u64>::new);
    let mut last_put = use_signal(String::new);

    {
        let seed = props.clone();
        use_hook(move || {
            if let Some(t) = seed.target.clone() {
                state.set_target(t);
            }
            state.set_lineage(seed.lineage.clone());
            if let Some(p) = seed.profile.clone() {
                let id = state.begin_load();
                state.put_profile(id, p);
                for (c, data) in seed.topn.clone() {
                    state.put_topn(id, &c, data);
                }
            }
        });
    }

    let visible = ws.layout.read().inspector_visible;

    rsx! {
        Inspector {
            state,
            projection: props.projection.clone(),
            focus_column: props.focus_column.clone(),
            on_reload: move |_| {
                let n = reloads();
                reloads.set(n + 1);
            },
            on_open: move |(kind, name): (NodeKind, String)| opened.set(format!("{kind:?}:{name}")),
        }

        div { "data-a11y-id": "rb-visible", "{visible}" }
        div { "data-a11y-id": "rb-reloads", "{reloads}" }
        div { "data-a11y-id": "rb-opened", "{opened}" }
        div { "data-a11y-id": "rb-put", "{last_put}" }

        button {
            "data-a11y-id": "drive-begin",
            onclick: move |_| {
                let id = state.begin_load();
                ids.write().push(id);
            },
            "begin"
        }
        button {
            "data-a11y-id": "drive-land-first",
            onclick: move |_| {
                let id = ids.read()[0];
                last_put.set(state.put_profile(id, profile()).to_string());
            },
            "land first"
        }
        button {
            "data-a11y-id": "drive-land-second",
            onclick: move |_| {
                let id = ids.read()[1];
                last_put.set(state.put_profile(id, other_profile()).to_string());
            },
            "land second"
        }
    }
}

fn mount(props: HostProps) -> Harness {
    Harness::new(Host, props)
}

/// A panel showing `profile()` for table `orders`.
fn profiled() -> HostProps {
    HostProps {
        target: Some("orders".into()),
        profile: Some(profile()),
        ..Default::default()
    }
}

// ── overview and the empty state ─────────────────────────────────────────────

#[test]
fn with_no_target_the_panel_says_so_and_shows_nothing_else() {
    let h = mount(HostProps::default());

    assert_eq!(
        h.text_of(h.by_a11y_id("inspector-overview").unwrap()),
        dat0_i18n::t("inspector.empty")
    );
    // No target means no lineage to draw and no columns to card: the original
    // gates both on `target_table` / `cached()`, and a panel that renders an
    // empty lineage frame implies a table that is not there.
    assert!(h.by_a11y_id("inspector-lineage").is_none());
    assert!(h.by_a11y_id("inspector-cards").is_none());
}

#[test]
fn a_target_without_a_profile_yet_reads_as_loading() {
    let h = mount(HostProps {
        target: Some("orders".into()),
        ..Default::default()
    });

    assert_eq!(
        h.text_of(h.by_a11y_id("inspector-overview").unwrap()),
        format!("orders — {}", dat0_i18n::t("inspector.loading"))
    );
    // The lineage frame *is* drawn — there is a table, it just has no stats.
    assert!(h.by_a11y_id("inspector-lineage").is_some());
    assert!(h.by_a11y_id("inspector-cards").is_none());
}

#[test]
fn a_profiled_table_reports_its_row_and_column_counts() {
    let h = mount(profiled());

    assert_eq!(
        h.text_of(h.by_a11y_id("inspector-overview").unwrap()),
        "orders — 1000 rows · 3 cols"
    );
}

// ── the pane ─────────────────────────────────────────────────────────────────

#[test]
fn the_pane_header_meta_names_the_focus_column_and_its_type() {
    let h = mount(HostProps {
        focus_column: Some("price".into()),
        ..profiled()
    });

    let head = h.by_a11y_id("pane-head-inspector").unwrap();
    assert!(
        h.text_of(head).contains("price · DOUBLE"),
        "header was {:?}",
        h.text_of(head)
    );
}

#[test]
fn the_pane_open_state_lives_in_the_dock_layout() {
    // Not local pane state: the console, the charts pane and this one all
    // persist through `DockLayout`, and a panel that owned its own flag would
    // reopen closed on every relaunch.
    let mut h = mount(profiled());

    assert_eq!(h.text_of(h.by_a11y_id("rb-visible").unwrap()), "false");
    let head = h.by_a11y_id("pane-head-inspector").unwrap();
    assert_eq!(h.attr(head, "aria-expanded").as_deref(), Some("false"));

    h.click("pane-head-inspector");

    assert_eq!(h.text_of(h.by_a11y_id("rb-visible").unwrap()), "true");
    let head = h.by_a11y_id("pane-head-inspector").unwrap();
    assert_eq!(h.attr(head, "aria-expanded").as_deref(), Some("true"));
}

// ── lineage ──────────────────────────────────────────────────────────────────

#[test]
fn lineage_rows_indent_by_depth_and_stop_at_six() {
    let h = mount(HostProps {
        target: Some("orders".into()),
        lineage: LineageChain {
            ancestors: vec![step("raw.csv", 1, NodeKind::File, false)],
            descendants: vec![
                step("d3", 3, NodeKind::Table, true),
                step("d6", 6, NodeKind::Table, true),
                // Past the clamp: a 9-deep chain must still fit the dock.
                step("d9", 9, NodeKind::Table, true),
            ],
        },
        ..Default::default()
    });

    let indent = |id: &str| h.attr(h.by_a11y_id(id).unwrap(), "style").unwrap();
    assert_eq!(indent("lineage-1-raw.csv"), "padding-left: 12px");
    assert_eq!(indent("lineage-3-d3"), "padding-left: 36px");
    assert_eq!(indent("lineage-6-d6"), "padding-left: 72px");
    assert_eq!(
        indent("lineage-9-d9"),
        "padding-left: 72px",
        "depth 9 must clamp to depth 6's indent"
    );
}

#[test]
fn the_lineage_headers_follow_what_the_chain_actually_holds() {
    // No ancestors -> no "Sources" heading; no descendants -> the em-dash
    // placeholder instead of "Used by". Both gates are in the original.
    let bare = mount(HostProps {
        target: Some("orders".into()),
        ..Default::default()
    });
    assert!(bare.by_a11y_id("inspector-lineage-sources").is_none());
    assert_eq!(
        bare.text_of(bare.by_a11y_id("inspector-lineage-usedby").unwrap()),
        dat0_i18n::t("inspector.lineage.none")
    );

    let full = mount(HostProps {
        target: Some("orders".into()),
        lineage: LineageChain {
            ancestors: vec![step("raw.csv", 1, NodeKind::File, false)],
            descendants: vec![step("summary", 1, NodeKind::Table, true)],
        },
        ..Default::default()
    });
    assert!(full.by_a11y_id("inspector-lineage-sources").is_some());
    assert_eq!(
        full.text_of(full.by_a11y_id("inspector-lineage-usedby").unwrap()),
        dat0_i18n::t("inspector.lineage.usedby")
    );
}

#[test]
fn only_an_openable_node_reroots_the_inspector() {
    let mut h = mount(HostProps {
        target: Some("orders".into()),
        lineage: LineageChain {
            // A file leaf has no `open_name`: there is no tab to open for it.
            ancestors: vec![step("raw.csv", 1, NodeKind::File, false)],
            descendants: vec![step("revenue", 1, NodeKind::Chart, true)],
        },
        ..Default::default()
    });

    h.click("lineage-1-raw.csv");
    assert_eq!(h.text_of(h.by_a11y_id("rb-opened").unwrap()), "");

    h.click("lineage-1-revenue");
    // Routed by kind: the shell reopens a chart, not a table tab.
    assert_eq!(
        h.text_of(h.by_a11y_id("rb-opened").unwrap()),
        "Chart:revenue"
    );
}

// ── cards ────────────────────────────────────────────────────────────────────

#[test]
fn every_profiled_column_gets_a_card_with_its_stat_lines() {
    let h = mount(profiled());

    assert!(h.has_label("price · DOUBLE"));
    assert!(h.has_label("distinct ≈900 (approx)"));
    assert!(h.has_label("null 0.3%"));
    assert!(h.has_label_contains("min 0 · max 9942"));

    // A column with neither numeric nor length stats gets no stat line at all
    // — the original skips it rather than rendering a blank row.
    assert!(h.by_a11y_id("inspector-stats-price").is_some());
    assert!(h.by_a11y_id("inspector-stats-name").is_none());
    assert!(h.by_a11y_id("inspector-distinct-name").is_some());
}

#[test]
fn a_renamed_column_shows_its_label_and_what_it_was() {
    let h = mount(HostProps {
        projection: Some(ProjectionContext {
            visible: vec![ProjectionColumn {
                source: "price".into(),
                display: "Unit price".into(),
            }],
            base_sources: vec!["id".into(), "name".into(), "price".into()],
        }),
        ..profiled()
    });

    assert!(h.has_label_contains(&format!(
        "Unit price · DOUBLE  ·  {} price",
        dat0_i18n::t("inspector.col.was")
    )));
}

#[test]
fn columns_the_grid_hides_go_behind_the_hidden_toggle() {
    let mut h = mount(HostProps {
        projection: Some(ProjectionContext {
            visible: vec![ProjectionColumn {
                source: "price".into(),
                display: "price".into(),
            }],
            base_sources: vec!["id".into(), "name".into(), "price".into()],
        }),
        ..profiled()
    });

    assert!(h.by_a11y_id("inspector-card-price").is_some());
    // Collapsed by default, so a wide table does not open as a wall of cards.
    assert!(h.by_a11y_id("inspector-card-id").is_none());
    let toggle = h.by_a11y_id("inspector-hidden-toggle").unwrap();
    assert_eq!(h.attr(toggle, "aria-expanded").as_deref(), Some("false"));
    assert!(h.has_label(&format!("{} (2)", dat0_i18n::t("inspector.hidden"))));

    h.click("inspector-hidden-toggle");

    assert!(h.by_a11y_id("inspector-card-id").is_some());
    assert!(h.by_a11y_id("inspector-card-name").is_some());
    let toggle = h.by_a11y_id("inspector-hidden-toggle").unwrap();
    assert_eq!(h.attr(toggle, "aria-expanded").as_deref(), Some("true"));
}

#[test]
fn with_no_projection_there_is_no_hidden_section_at_all() {
    // `project_cards(None)` puts every column in `visible`, so the section has
    // nothing to hold and must not render an empty "Hidden (0)" control.
    let h = mount(profiled());
    assert!(h.by_a11y_id("inspector-hidden").is_none());
    assert!(h.by_a11y_id("inspector-card-id").is_some());
}

// ── inline charts ────────────────────────────────────────────────────────────

#[test]
fn a_columns_top_n_data_renders_as_inline_svg() {
    let h = mount(HostProps {
        topn: vec![(
            "name".into(),
            vec![("alpha".into(), 3), ("bravo".into(), 1)],
        )],
        ..profiled()
    });

    let chart = h
        .by_a11y_id("inspector-chart-name")
        .expect("the profiled column with top-N data draws a chart");
    let svg = h.attr(chart, "dangerous_inner_html").unwrap();

    // Inline SVG, not a raster: the bars take their fill from a CSS class, so
    // a theme switch re-colours them without re-rendering anything.
    assert!(svg.starts_with("<svg"), "{svg}");
    assert_eq!(svg.matches("<rect").count(), 2, "one bar per item: {svg}");
    assert!(svg.contains("d0-mini-bar-topn"), "{svg}");
    assert!(svg.contains("alpha") && svg.contains("bravo"), "{svg}");
    assert_eq!(h.attr(chart, "role").as_deref(), Some("img"));

    // Only the column that has data gets one.
    assert!(h.by_a11y_id("inspector-chart-price").is_none());
    assert!(h.by_a11y_id("inspector-chart-id").is_none());
}

// ── mode toggle ──────────────────────────────────────────────────────────────

#[test]
fn the_mode_toggle_flips_the_label_and_asks_for_a_reprofile() {
    let mut h = mount(profiled());

    assert!(h.has_label(&dat0_i18n::t("inspector.mode.whole")));
    assert_eq!(h.text_of(h.by_a11y_id("rb-reloads").unwrap()), "0");

    h.click("inspector-mode-toggle");

    assert!(h.has_label(&dat0_i18n::t("inspector.mode.view")));
    // The (table, epoch) cache is not keyed by mode, so a toggle must always
    // re-profile — otherwise Current-view shows Whole-table's numbers.
    assert_eq!(h.text_of(h.by_a11y_id("rb-reloads").unwrap()), "1");

    h.click("inspector-mode-toggle");
    assert!(h.has_label(&dat0_i18n::t("inspector.mode.whole")));
    assert_eq!(h.text_of(h.by_a11y_id("rb-reloads").unwrap()), "2");
}

#[test]
fn switching_mode_drops_the_stale_inline_charts() {
    // Extras query the *base* table, so a Whole-table bar beside a
    // Current-view (filtered) profile is simply the wrong number.
    let mut h = mount(HostProps {
        topn: vec![("name".into(), vec![("alpha".into(), 3)])],
        ..profiled()
    });
    assert!(h.by_a11y_id("inspector-chart-name").is_some());

    h.click("inspector-mode-toggle");

    assert!(h.by_a11y_id("inspector-chart-name").is_none());
}

// ── supersede ────────────────────────────────────────────────────────────────

#[test]
fn a_superseded_profile_never_replaces_a_newer_one() {
    // The race the guard exists for: a slow load starts, the user moves on, a
    // faster load lands, and then the slow one finally returns. Without the
    // monotonic load id the panel ends up showing the stale table's columns
    // under the current table's name.
    let mut h = mount(HostProps {
        target: Some("orders".into()),
        ..Default::default()
    });

    h.click("drive-begin"); // slow load  -> id 1
    h.click("drive-begin"); // fast load  -> id 2

    h.click("drive-land-second"); // the newer load lands first
    assert_eq!(h.text_of(h.by_a11y_id("rb-put").unwrap()), "true");
    assert_eq!(
        h.text_of(h.by_a11y_id("inspector-overview").unwrap()),
        "orders — 7 rows · 1 cols"
    );

    h.click("drive-land-first"); // the superseded load returns

    assert_eq!(
        h.text_of(h.by_a11y_id("rb-put").unwrap()),
        "false",
        "the stale write must be refused, not merely ignored by the view"
    );
    assert_eq!(
        h.text_of(h.by_a11y_id("inspector-overview").unwrap()),
        "orders — 7 rows · 1 cols",
        "the newer profile is still the one on screen"
    );
    assert!(h.by_a11y_id("inspector-card-only").is_some());
    assert!(h.by_a11y_id("inspector-card-price").is_none());
}

#[test]
fn the_newest_load_still_wins_when_it_lands_last() {
    // The other order, so the test above cannot pass by refusing every write.
    let mut h = mount(HostProps {
        target: Some("orders".into()),
        ..Default::default()
    });

    h.click("drive-begin");
    h.click("drive-land-first");
    assert_eq!(h.text_of(h.by_a11y_id("rb-put").unwrap()), "true");
    assert_eq!(
        h.text_of(h.by_a11y_id("inspector-overview").unwrap()),
        "orders — 1000 rows · 3 cols"
    );
}
