//! What feeds the charts pane (step 5.5b).
//!
//! The pane was built with nothing feeding it (PD-023): the shell's chart kept
//! an empty source, so the pane showed its empty state whatever was open, its
//! axis pickers had no columns to offer, and a plot query that failed was
//! dropped without a word. [`ChartHost`] binds the chart to the active tab's
//! table while the pane is open, keeps each table's chart for when its tab
//! comes back, plots it, and shows why when a chart cannot be drawn.
//!
//! The chart is drawn from what the tab's grid reads: the table with its
//! filters and edits, which dat0 lays over it as a view. (The GPUI build drew
//! the bare table, so an edited value stayed as it was in the chart.) The
//! chart still names the table, not the view: a view is a TEMP view that does
//! not outlive the window, and a chart is saved and packaged under its
//! table's name.

use std::collections::HashMap;
use std::path::PathBuf;

use dioxus::prelude::*;

use dat0_core::charts::data::PlotTable;
use dat0_core::charts::query::build_plot_sql;
use dat0_core::charts::render::Palette;
use dat0_core::charts::spec::{ChartSpec, ChartType, default_type};
use dat0_core::error_ux::Banner;
use dat0_engine::{QueryEngine, quote_ident};
use dat0_i18n::t;

use super::{ChartFormat, ChartLoad, ChartRender, ChartRequest, palette, render_chart};
use crate::components::grid::views::Views;
use crate::state::Workspace;
use crate::theme::Theme;

/// Pixel size every chart export is rendered at. Fixed rather than taken from
/// the pane: an export is a document, and a file whose resolution depended on
/// how wide the user had dragged a pane would be irreproducible.
pub const EXPORT_SIZE: (u32, u32) = (1600, 900);

/// A plotted chart: the spec it was drawn from and its rows, or why there are
/// none. `None` while there is nothing to plot.
pub type Plotted = Option<(ChartSpec, Result<PlotTable, String>)>;

/// What the chart is bound to.
#[derive(Clone, PartialEq, Debug)]
enum Binding {
    /// The pane is shut, or the window has no session yet: keep what there is.
    Idle,
    /// No tab is open: there is nothing to chart.
    Nothing,
    /// The active tab's table and its `(name, type)` columns, or why they
    /// could not be read.
    Table(String, Result<Vec<(String, String)>, String>),
}

/// The charts pane's state and what feeds it.
#[derive(Clone, Copy)]
pub struct ChartHost {
    /// What the pane describes and what is plotted.
    pub spec: Signal<ChartSpec>,
    /// The bound table's `(name, type)` columns, for the axis pickers.
    pub columns: Signal<Vec<(String, String)>>,
    /// The chart body's render state.
    pub state: Signal<ChartLoad>,
    /// The chart as last plotted, which is what an export writes.
    pub plotted: Resource<Plotted>,
    /// Bumped each time a chart is saved: what lists the saved charts reads
    /// it, since the session holding them is not a signal.
    pub saved: Signal<u64>,
    /// The table the chart is bound to.
    pub(super) bound_to: Signal<Option<String>>,
    /// Each table's chart, kept for when its tab comes back.
    kept: Signal<HashMap<String, ChartSpec>>,
    theme: Theme,
}

impl ChartHost {
    /// This window's chart. A hook: call it once, from the shell's body.
    pub fn use_new(ws: Workspace, views: Views, theme: Theme) -> Self {
        let mut spec = use_signal(unbound);
        let mut columns = use_signal(Vec::<(String, String)>::new);
        let mut state = use_signal(ChartLoad::default);
        let saved = use_signal(|| 0u64);
        let mut bound_to = use_signal(|| Option::<String>::None);
        let mut kept = use_signal(HashMap::<String, ChartSpec>::new);
        let open = use_memo(move || ws.layout.read().charts_visible);
        let table = use_memo(move || ws.active_tab().map(|t| t.table));

        // Every read inside the future, so each restarts it (see the shell's
        // `source`). A console run or a Live Refresh rebinds a tab over rows,
        // and perhaps columns, that are not the ones charted.
        let binding = use_resource(move || async move {
            if !open() {
                return Binding::Idle;
            }
            let Some(table) = table() else {
                return Binding::Nothing;
            };
            let reads = reads(views, &table);
            let Some(engine) = ws.session.read().ready().map(|s| s.lock().engine.clone()) else {
                return Binding::Idle;
            };
            let described = engine
                .describe_table(&reads, None)
                .await
                .map(|cs| chartable(cs.into_iter().map(|c| (c.name, c.data_type))))
                .map_err(|e| format!("{e:#}"));
            Binding::Table(table, described)
        });

        use_effect(move || {
            let Some(binding) = binding.read().clone() else {
                return;
            };
            let (table, found) = match binding {
                Binding::Idle => return,
                Binding::Nothing => (None, Vec::new()),
                Binding::Table(table, found) => (Some(table), found.unwrap_or_default()),
            };
            let was = bound_to.peek().clone();
            let current = spec.peek().clone();
            if let Some(was) = was.as_ref().filter(|w| Some(*w) != table.as_ref()) {
                kept.write().insert(was.clone(), current.clone());
            }
            let next = match &table {
                Some(t) if was.as_ref() == Some(t) => chart_for(t, &found, Some(&current)),
                Some(t) => chart_for(t, &found, kept.peek().get(t)),
                None => unbound(),
            };
            if next != current {
                spec.set(next);
            }
            if *columns.peek() != found {
                columns.set(found);
            }
            if was != table {
                bound_to.set(table);
            }
        });

        // Plotted again whenever the tab is rebound: a filter, an edit or a
        // console run changes what it reads.
        let plotted = use_resource(move || async move {
            let spec = spec();
            if !open() || !picked(&spec) {
                return None;
            }
            let reads = reads(views, &bound_to()?);
            let engine = ws.session.read().ready().map(|s| s.lock().engine.clone())?;
            let from = ChartSpec {
                source: quote_ident(&reads),
                ..spec.clone()
            };
            let rows = plot(engine.as_ref(), &from).await;
            Some((spec, rows))
        });

        use_effect(move || {
            let tokens = theme.tokens();
            let render = match binding.read().as_ref() {
                Some(Binding::Table(_, Err(why))) => ChartRender::Error(why.clone()),
                _ => render_of(plotted.read().as_ref().and_then(Option::as_ref), &tokens),
            };
            // Through the supersede counter, not a bare write: a slow chart
            // must never overwrite a newer one.
            let id = state.write().begin();
            state.write().apply(id, render);
        });

        Self {
            spec,
            columns,
            state,
            plotted,
            saved,
            bound_to,
            kept,
            theme,
        }
    }

    /// The bound source, a quoted identifier, or `None` when nothing is bound.
    pub fn source(&self) -> Option<String> {
        let spec = self.spec.read();
        (!spec.source.is_empty()).then(|| spec.source.clone())
    }

    /// What the pane's header calls the bound table: the title of its tab.
    pub fn label(&self, ws: &Workspace) -> Option<String> {
        let bound = self.bound_to.read().clone()?;
        let tabs = ws.tabs.read();
        let tab = tabs.iter().find(|t| t.table == bound)?;
        Some(tab.title().to_string())
    }

    /// The user changed the chart's type or an axis.
    pub fn configure(&self, request: ChartRequest) {
        let mut spec = self.spec;
        spec.set(request.spec);
    }

    /// Show `spec` as `table`'s chart, over the table's tab.
    pub(super) fn show(
        &self,
        ws: Workspace,
        table: String,
        path: Option<PathBuf>,
        spec: ChartSpec,
    ) {
        let (mut current, mut kept) = (self.spec, self.kept);
        if self.bound_to.peek().as_deref() == Some(table.as_str()) {
            // Bound already, so the binding will not run again: show it now.
            current.set(chart_for(&table, &self.columns.peek(), Some(&spec)));
        } else {
            kept.write().insert(table.clone(), spec);
        }
        ws.show_tab(table, path);
        let mut layout = ws.layout;
        layout.write().charts_visible = true;
    }
}

/// What `table`'s tab reads: the view its filters and edits are laid over it
/// as, or the table itself.
pub(crate) fn reads(views: Views, table: &str) -> String {
    views
        .bound
        .read()
        .get(table)
        .map(|(reads, _)| reads.clone())
        .unwrap_or_else(|| table.to_string())
}

/// The chart bound to nothing: the pane's empty state.
fn unbound() -> ChartSpec {
    ChartSpec {
        chart_type: ChartType::Bar,
        source: String::new(),
        x: None,
        y: None,
        group: None,
        color: None,
        title: String::new(),
    }
}

/// The chart to show for `table`, whose columns are `columns`: the chart it
/// had, less any pick whose column has gone, or a new one of the type its
/// columns suggest with nothing picked.
fn chart_for(table: &str, columns: &[(String, String)], had: Option<&ChartSpec>) -> ChartSpec {
    let source = quote_ident(table);
    let Some(had) = had else {
        let types: Vec<&str> = columns.iter().map(|(_, ty)| ty.as_str()).collect();
        return ChartSpec {
            chart_type: default_type(&types),
            source,
            ..unbound()
        };
    };
    let keep = |pick: &Option<String>| {
        pick.clone()
            .filter(|p| columns.iter().any(|(name, _)| name == p))
    };
    ChartSpec {
        source,
        x: keep(&had.x),
        y: keep(&had.y),
        group: keep(&had.group),
        color: keep(&had.color),
        ..had.clone()
    }
}

/// The columns a chart may use: all but the row id an editable table carries
/// for its edits, which is no one's data.
fn chartable(columns: impl Iterator<Item = (String, String)>) -> Vec<(String, String)> {
    columns
        .filter(|(name, _)| name != dat0_engine::ROWID_COL)
        .collect()
}

/// Whether any axis is picked. A chart with none is waiting for the user, not
/// failing, so it is not plotted.
fn picked(spec: &ChartSpec) -> bool {
    spec.x.is_some() || spec.y.is_some() || spec.group.is_some() || spec.color.is_some()
}

/// Plot `spec` in `engine`: its rows, or why there are none.
pub async fn plot(engine: &impl QueryEngine, spec: &ChartSpec) -> Result<PlotTable, String> {
    let sql = build_plot_sql(spec)?;
    let result = engine.execute(&sql).await.map_err(|e| format!("{e:#}"))?;
    Ok(PlotTable::from_query_result(&result))
}

/// What the chart body shows for a plot.
pub fn render_of(
    plotted: Option<&(ChartSpec, Result<PlotTable, String>)>,
    tokens: &dat0_core::theme::tokens::ThemeTokens,
) -> ChartRender {
    match plotted {
        None => ChartRender::Empty,
        Some((spec, Ok(rows))) => ChartRender::Svg(render_chart(spec, rows, tokens)),
        Some((_, Err(why))) => ChartRender::Error(why.clone()),
    }
}

/// Export Chart as PNG or SVG: ask where, then write the chart as plotted.
pub fn export(ws: Workspace, host: ChartHost, format: ChartFormat) {
    let Some((spec, Ok(rows))) = host.plotted.peek().clone().flatten() else {
        ws.push_banner(Banner::warning(t("chart.export.nothing")));
        return;
    };
    if !crate::launch::has_desktop() {
        tracing::debug!("export chart: no window system, nothing to show");
        return;
    }
    let palette = palette(&host.theme.tokens());
    let ext = match format {
        ChartFormat::Png => "png",
        ChartFormat::Svg => "svg",
    };
    let stem = if spec.title.is_empty() {
        super::source_label(&spec.source)
    } else {
        spec.title.clone()
    };
    spawn(async move {
        let Some(path) = crate::files::pick_save_path(&format!("{stem}.{ext}")).await else {
            return;
        };
        write(ws, spec, rows, palette, format, path).await;
    });
}

/// Write the chart `spec` drew from `rows` to `path`, in `palette`, off the
/// window's thread: rasterising a PNG takes long enough to drop frames.
pub async fn write(
    ws: Workspace,
    spec: ChartSpec,
    rows: PlotTable,
    palette: Palette,
    format: ChartFormat,
    path: PathBuf,
) {
    use dat0_core::charts::export::{export_png_with, export_svg_with};

    let to = path.clone();
    let written = tokio::task::spawn_blocking(move || match format {
        ChartFormat::Png => {
            export_png_with(&spec, &rows, EXPORT_SIZE, &palette, &to).map_err(|e| e.to_string())
        }
        ChartFormat::Svg => {
            export_svg_with(&spec, &rows, EXPORT_SIZE, &palette, &to).map_err(|e| e.to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    match written {
        Ok(()) => ws.push_banner(Banner {
            body: path.display().to_string(),
            ..Banner::info(t("chart.export.done"))
        }),
        Err(why) => ws.push_banner(Banner::warning_with_body(t("chart.export.failed"), why)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, t)| (n.to_string(), t.to_string()))
            .collect()
    }

    #[test]
    fn a_new_table_gets_the_type_its_columns_suggest_and_no_picks() {
        let c = chart_for(
            "sales",
            &cols(&[("region", "VARCHAR"), ("amt", "BIGINT")]),
            None,
        );
        assert_eq!(c.source, "\"sales\"");
        assert_eq!(c.chart_type, ChartType::Bar);
        assert!(!picked(&c));

        let c = chart_for("xy", &cols(&[("x", "DOUBLE"), ("y", "DOUBLE")]), None);
        assert_eq!(c.chart_type, ChartType::Scatter);
    }

    #[test]
    fn a_table_keeps_its_chart_less_the_columns_that_went() {
        let had = ChartSpec {
            chart_type: ChartType::Heatmap,
            x: Some("region".into()),
            y: Some("gone".into()),
            color: Some("amt".into()),
            title: "Sales".into(),
            ..chart_for("sales", &[], None)
        };
        let c = chart_for(
            "sales",
            &cols(&[("region", "VARCHAR"), ("amt", "BIGINT")]),
            Some(&had),
        );
        assert_eq!(c.chart_type, ChartType::Heatmap);
        assert_eq!(c.x.as_deref(), Some("region"));
        assert_eq!(c.y, None, "its column went");
        assert_eq!(c.color.as_deref(), Some("amt"));
        assert_eq!(c.title, "Sales");
    }

    #[test]
    fn the_row_id_is_not_a_column_to_chart() {
        let found = chartable(
            cols(&[
                (dat0_engine::ROWID_COL, "BIGINT"),
                ("region", "VARCHAR"),
                ("amt", "BIGINT"),
            ])
            .into_iter(),
        );
        assert_eq!(found, cols(&[("region", "VARCHAR"), ("amt", "BIGINT")]));
        assert_eq!(chart_for("sales", &found, None).chart_type, ChartType::Bar);
    }

    #[test]
    fn a_table_name_is_quoted_as_an_identifier() {
        assert_eq!(chart_for("a\"b", &[], None).source, "\"a\"\"b\"");
    }

    #[test]
    fn a_plot_that_fails_says_why() {
        let tokens = dat0_core::theme::builtin_or_default("light");
        let spec = chart_for("t", &[], None);
        let failed = (
            spec.clone(),
            Err("Catalog Error: t does not exist".to_string()),
        );
        assert_eq!(
            render_of(Some(&failed), &tokens),
            ChartRender::Error("Catalog Error: t does not exist".into())
        );
        assert_eq!(render_of(None, &tokens), ChartRender::Empty);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn plotting_a_table_that_is_not_there_is_an_error_not_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let budget = dat0_engine::MemoryBudget {
            bytes: 64 * 1024 * 1024,
        };
        let engine = dat0_engine::DuckDBEngine::new(dir.path().join("s.duckdb"), budget).unwrap();
        engine.init().await.unwrap();
        let spec = ChartSpec {
            x: Some("x".into()),
            ..chart_for("missing", &[], None)
        };
        let why = plot(&engine, &spec).await.err().expect("an error");
        assert!(why.contains("missing"), "{why}");

        engine
            .execute("CREATE TABLE t AS SELECT * FROM (VALUES ('a'), ('a'), ('b')) v(x)")
            .await
            .unwrap();
        let spec = ChartSpec {
            x: Some("x".into()),
            ..chart_for("t", &[], None)
        };
        let rows = plot(&engine, &spec).await.unwrap();
        assert_eq!(rows.rows, 2, "a bar per value");

        let spec = ChartSpec {
            chart_type: ChartType::Line,
            ..spec
        };
        let why = plot(&engine, &spec).await.err().expect("an error");
        assert!(why.contains("y column"), "{why}");
    }
}
