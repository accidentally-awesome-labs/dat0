//! Everything every scene renders from, built once per run.
//!
//! One DuckDB session, one temp directory, one set of derived values. The
//! scenes then take slices of it as props — nothing a scene shows is fetched
//! during its render, because effects and async do not run under SSR and a
//! scene that waited for a resource would snapshot as a spinner.
//!
//! # Determinism is a construction rule
//!
//! Every value here is either a fixed literal or derived deterministically from
//! one. Uuids are [`uuid::Uuid::from_u128`] constants, never `now_v7`; the two
//! components that render a filesystem path they were handed
//! (`components/empty_state.rs`, `components/export_dialog.rs`) are given fixed
//! literals that need not exist, because both only display them.
//!
//! The single exception is [`RecoveryPanel`](crate::components::recovery), which
//! reads real directories during render and can therefore surface this run's
//! `TempDir`. [`Fixtures::scrub`] exists for exactly that, and for nothing else.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use tempfile::TempDir;

use dat0_core::catalog::CatalogTree;
use dat0_core::charts::data::PlotTable;
use dat0_core::charts::spec::{ChartSpec, ChartType};
use dat0_core::grid::data_source::GridDataSource;
use dat0_core::session::queries::{HistoryEntry, SavedQuery};
use dat0_engine::transform::ProjectionColumn;
use dat0_engine::{
    ColumnInfo, DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts, TableInfo, TableOrigin,
};

/// Twelve rows, four columns. Small enough to snapshot, wide enough that the
/// grid's column loop, the profiler's numeric branch and a grouped bar chart
/// all have something real to work on.
const SALES_CSV: &str = "\
id,region,revenue,active
1,north,1200.5,true
2,south,830.25,false
3,east,1544.0,true
4,west,610.75,true
5,north,990.0,false
6,south,1310.5,true
7,east,720.25,false
8,west,1425.75,true
9,north,505.0,true
10,south,1180.5,false
11,east,860.25,true
12,west,1035.0,true
";

/// The same header, no data rows.
///
/// The grid's zero-row render is a different shape from its populated one — the
/// header row with nothing under it — and it has no other fixture. It is also a
/// state the app could not reach until `run_page` learned to carry a schema
/// through an empty result; `tests/grid_empty_table.rs` in `dat0-core` is the
/// behavioural half of the same fix.
const EMPTY_CSV: &str = "id,region,revenue,active\n";

/// Everything every scene needs, built once.
///
/// Owns its `TempDir`, so it must outlive every render that borrows from it.
pub struct Fixtures {
    /// Dropped last; every path below lives under it.
    _tmp: TempDir,
    /// The `TempDir`'s canonical path, for [`Self::scrub`].
    root: PathBuf,

    /// A 12-row, 4-column table over a real CSV, page 0 resident.
    pub source: Arc<GridDataSource>,
    /// `source`'s visible columns, in display order.
    pub columns: Vec<ProjectionColumn>,
    /// The same four columns with zero rows behind them.
    pub empty_source: Arc<GridDataSource>,
    /// Files, connections (one live, one not) and packages.
    pub catalog: CatalogTree,
    /// A bar chart's plot-ready rows, and the spec they came from.
    pub plot: PlotTable,
    pub spec: ChartSpec,
    /// `(name, duckdb type)` for the chart's axis pickers.
    pub chart_columns: Vec<(String, String)>,
    /// `SUMMARIZE` over the sales table.
    pub profile: dat0_engine::TableProfile,
    /// A scratch root holding two orphan sessions.
    pub recovery_root: PathBuf,
    /// A recent workspace root with a half-written `.dat0/`.
    pub incomplete_root: PathBuf,
    /// Where a `SettingsPanel` scene writes. Never pre-populated: the panel
    /// renders defaults, which is the state worth pinning.
    pub settings_path: PathBuf,
    pub queries: Vec<SavedQuery>,
    pub history: Vec<HistoryEntry>,
}

impl Fixtures {
    /// Open one DuckDB session, register the fixtures, build the derived data.
    ///
    /// Also pins `DAT0_CONFIG_DIR` at a scratch directory with `first_run_done`
    /// already set. `Shell` reads the real `settings.toml` during render
    /// (`components/shell.rs`, the `first_run_done` hook) and auto-opens the
    /// tour when it is unset, so without this the `shell/*` scenes would render
    /// whatever the developer's own profile says — the exact non-determinism
    /// `tests/onboarding.rs` and six sibling suites already guard against the
    /// same way.
    ///
    /// The variable is **not** restored when the `Fixtures` drops: the pointed-at
    /// directory goes with the `TempDir`, so anything in the same process that
    /// read it afterwards would see a path that no longer exists. Every consumer
    /// today is a binary that owns its whole process — one `#[serial]` test, one
    /// probe, one page generator. A second scene-mounting test in the same
    /// binary would need to hold the `Fixtures` for its own lifetime too.
    pub async fn build() -> Result<Self> {
        let tmp = TempDir::new().context("scene fixture tempdir")?;
        let root = tmp.path().to_path_buf();

        let config = root.join("config");
        std::fs::create_dir_all(&config).context("create the scene config dir")?;
        // SAFETY: the snapshot test is `#[serial]` and the probe is one process
        // with one scene loop, so nothing races this process-global write.
        unsafe { std::env::set_var("DAT0_CONFIG_DIR", &config) };
        dat0_core::settings::set_first_run_done(
            &dat0_core::settings::store::SettingsStore::with_path(config.join("settings.toml")),
            true,
        )
        .context("seed first_run_done")?;

        let sales_csv = root.join("sales.csv");
        std::fs::write(&sales_csv, SALES_CSV).context("write sales.csv")?;
        let empty_csv = root.join("empty.csv");
        std::fs::write(&empty_csv, EMPTY_CSV).context("write empty.csv")?;

        let engine = DuckDBEngine::new(
            root.join("scratch.duckdb"),
            MemoryBudget {
                bytes: 128 * 1024 * 1024,
            },
        )
        .context("open the scene engine")?;
        engine.init().await.context("init the scene engine")?;
        let engine = Arc::new(engine);

        engine
            .register_file_as_table(&sales_csv, RegisterOpts::default())
            .await
            .context("register sales.csv")?;
        engine
            .register_file_as_table(&empty_csv, RegisterOpts::default())
            .await
            .context("register empty.csv")?;

        let source = grid_source(&engine, "sales").await?;
        let empty_source = grid_source(&engine, "empty").await?;
        let columns = source
            .visible_column_names()
            .into_iter()
            .map(|n| ProjectionColumn {
                source: n.clone(),
                display: n,
            })
            .collect();

        let profile = engine
            .profile_table("sales", None)
            .await
            .context("profile sales")?;
        let chart_columns = engine
            .describe_table("sales", None)
            .await
            .context("describe sales")?
            .into_iter()
            .map(|c| (c.name, c.data_type))
            .collect();

        // `source` is already quoted by contract (`charts::query`'s module doc).
        let spec = ChartSpec {
            chart_type: ChartType::Bar,
            source: "\"sales\"".into(),
            x: Some("region".into()),
            y: Some("revenue".into()),
            group: None,
            color: None,
            title: "revenue by region".into(),
        };
        let sql = dat0_core::charts::query::build_plot_sql(&spec)
            .map_err(anyhow::Error::msg)
            .context("build the plot sql")?;
        let plot = PlotTable::from_query_result(
            &QueryEngine::execute(engine.as_ref(), &sql)
                .await
                .context("run the plot query")?,
        );

        let recovery_root = root.join("scratch");
        orphan(&recovery_root, "win-a", &["sales", "orders"])?;
        orphan(&recovery_root, "win-b", &["taxi_trips"])?;

        // `detect_incomplete` flags a `.dat0/` that exists but lacks
        // `manifest.json` or `workspace.duckdb` — an interrupted Save Workspace.
        let incomplete_root = root.join("quarterly-review");
        std::fs::create_dir_all(incomplete_root.join(".dat0"))
            .context("create the incomplete workspace")?;

        let settings_path = config.join("settings.toml");

        Ok(Self {
            _tmp: tmp,
            root,
            source,
            columns,
            empty_source,
            catalog: catalog(),
            plot,
            spec,
            chart_columns,
            profile,
            recovery_root,
            incomplete_root,
            settings_path,
            queries: queries(),
            history: history(),
        })
    }

    /// Replace the three things a scene can render that change between runs.
    ///
    /// 1. **This run's fixture root.** Only
    ///    [`RecoveryPanel`](crate::components::recovery) reaches the disk during
    ///    render (`components/recovery.rs`, a `use_signal` initialiser calling
    ///    `collect_rows`), and its rows carry the path.
    /// 2. **Uuids minted during render.** `SqlConsole`'s tab ids come from
    ///    `Tabs::new`, which calls `Uuid::now_v7` — inside `Shell`, with no prop
    ///    between it and a scene. Every uuid a *fixture* supplies is a fixed
    ///    `Uuid::from_u128` constant, so anything left matching the shape was
    ///    minted by the component and is not visual information.
    /// 3. **The build identity.** `modal/about` renders `BuildInfo::current()`,
    ///    whose `git_sha` is baked in by `dat0-core/build.rs` from `DAT0_GIT_SHA`
    ///    — so the About snapshot would fail on *every commit*, which is a gate
    ///    that cries wolf until someone deletes it. The exact current values are
    ///    substituted rather than a pattern matched, so a real change to the
    ///    surrounding markup still shows up.
    ///
    /// Snagged by the suite itself: the About baseline was recorded at one
    /// commit and failed at the next.
    ///
    /// Snapshots call this; the probe and the page generator do not.
    pub fn scrub(&self, html: String) -> String {
        let build = dat0_core::about::build_info::BuildInfo::current();
        let html = html.replace(&self.root.to_string_lossy().into_owned(), "<FIXTURE>");
        let html = html.replace(build.git_sha, "<SHA>");
        let html = match build.built {
            Some(at) => html.replace(at, "<BUILT-AT>"),
            None => html,
        };
        scrub_uuids(html)
    }
}

/// Replace every clock-minted (version 7) uuid with `<UUID7>`.
///
/// Version 7 specifically, because that is the one a component mints from the
/// clock — `Tabs::new` calls `Uuid::now_v7`, inside `Shell`, with no prop
/// between it and a scene. Every uuid a *fixture* supplies is
/// `Uuid::from_u128`, whose version nibble is 0, so those survive and a
/// saved-query id stays pinned.
///
/// Hand-rolled rather than a `regex` dependency: the shape is fixed, and adding
/// a crate to the shipped dependency graph for one substitution in a dev-only
/// module is the wrong trade.
fn scrub_uuids(html: String) -> String {
    const LEN: usize = 36;

    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + LEN <= bytes.len() && is_uuid_v7(&bytes[i..i + LEN]) {
            out.push_str("<UUID7>");
            i += LEN;
        } else {
            // Walk by character: a uuid is pure ASCII, so a match can never
            // begin inside a multi-byte character, and a non-match copies the
            // character the string already had.
            let ch = html[i..].chars().next().expect("a char boundary");
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn is_uuid_v7(w: &[u8]) -> bool {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    /// Index of the version nibble: `8 hex + '-' + 4 hex + '-'`.
    const VERSION: usize = 14;

    let mut at = 0;
    for (n, len) in GROUPS.iter().enumerate() {
        if n > 0 {
            if w[at] != b'-' {
                return false;
            }
            at += 1;
        }
        if !w[at..at + len].iter().all(u8::is_ascii_hexdigit) {
            return false;
        }
        at += len;
    }
    w[VERSION] == b'7'
}

/// One `GridDataSource` with page 0 already resident.
///
/// The residency is not an optimisation: `cell_display_for_source` is
/// synchronous and returns the placeholder for a missing page, so a scene built
/// without it would snapshot a grid of `…`.
async fn grid_source(engine: &Arc<DuckDBEngine>, table: &str) -> Result<Arc<GridDataSource>> {
    let ds = GridDataSource::new(Arc::clone(engine), table.to_string())
        .await
        .with_context(|| format!("open a grid source over {table}"))?;
    if ds.row_count > 0 {
        ds.page_for(0)
            .await
            .with_context(|| format!("page 0 of {table}"))?;
    }
    Ok(Arc::new(ds))
}

/// An orphan scratch directory: a subdir holding a `session.json`.
fn orphan(scratch_root: &Path, name: &str, tables: &[&str]) -> Result<()> {
    let dir = scratch_root.join(name);
    std::fs::create_dir_all(&dir).context("create an orphan scratch dir")?;
    let tabs: Vec<serde_json::Value> = tables
        .iter()
        .map(|t| serde_json::json!({ "table_name": t, "source_path": null }))
        .collect();
    std::fs::write(
        dir.join("session.json"),
        serde_json::to_vec_pretty(&serde_json::json!({ "tabs": tabs }))?,
    )
    .context("write an orphan session.json")?;
    Ok(())
}

/// Files, two connections and a package — one of every row shape the sidebar
/// paints, so a collapsed section has something to hide.
fn catalog() -> CatalogTree {
    let mut tree = CatalogTree::build(&[
        tbl("sales", TableOrigin::File(PathBuf::from("/data/sales.csv"))),
        tbl(
            "trips",
            TableOrigin::File(PathBuf::from("/data/trips.parquet")),
        ),
        tbl(
            "md_events",
            TableOrigin::Attached {
                alias: "sample_data".into(),
                source: "md:sample_data".into(),
            },
        ),
        tbl(
            "customers",
            TableOrigin::Attached {
                alias: "sq".into(),
                source: "/data/crm.db".into(),
            },
        ),
        tbl(
            "invoices",
            TableOrigin::Attached {
                alias: "sq".into(),
                source: "/data/crm.db".into(),
            },
        ),
    ]);
    // Packages are not derived from the engine's table list
    // (`CatalogTree::build` is deliberately blind to them), so the one row that
    // exercises the PACKAGES section is set directly.
    tree.packages = vec![dat0_core::catalog::tree::PackageNode {
        name: "q3-review.dat0".into(),
        path: PathBuf::from("/data/q3-review.dat0"),
    }];
    tree
}

fn tbl(name: &str, origin: TableOrigin) -> TableInfo {
    TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![ColumnInfo {
            name: "id".into(),
            data_type: "BIGINT".into(),
            nullable: true,
        }],
        row_count_estimate: Some(12),
        origin,
    }
}

/// Fixed uuids and fixed timestamps: a saved-query list whose ids changed per
/// run would make every snapshot a diff.
fn queries() -> Vec<SavedQuery> {
    [
        (
            "revenue by region",
            "SELECT region, SUM(revenue) FROM sales GROUP BY region",
        ),
        ("active only", "SELECT * FROM sales WHERE active"),
        (
            "top ten",
            "SELECT * FROM sales ORDER BY revenue DESC LIMIT 10",
        ),
        ("row count", "SELECT COUNT(*) FROM sales"),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (name, sql))| SavedQuery {
        id: uuid::Uuid::from_u128(i as u128 + 1),
        name: name.into(),
        sql: sql.into(),
        saved_at: 1_700_000_000 + i as i64,
    })
    .collect()
}

/// Five runs, one of them failed — the library's two row states in one list.
fn history() -> Vec<HistoryEntry> {
    [
        ("SELECT 1", true, 3_u64),
        ("SELECT * FROM sales", true, 12),
        ("SELECT * FROM saless", false, 1),
        (
            "SELECT region, SUM(revenue)\nFROM sales\nGROUP BY region",
            true,
            27,
        ),
        ("SUMMARIZE sales", true, 44),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (sql, ok, elapsed_ms))| HistoryEntry {
        sql: sql.into(),
        ran_at: 1_700_000_000 + i as i64,
        ok,
        elapsed_ms,
    })
    .collect()
}
