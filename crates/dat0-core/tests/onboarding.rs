use dat0_core::sample_data::{CHINOOK_SQLITE, IRIS_CSV, ensure_bundled_extracted};
use tempfile::tempdir;

#[test]
fn bundled_samples_extract_to_state_root() {
    let dir = tempdir().unwrap();
    let iris = ensure_bundled_extracted(dir.path(), IRIS_CSV, "iris.csv").unwrap();
    let chinook = ensure_bundled_extracted(dir.path(), CHINOOK_SQLITE, "chinook.sqlite").unwrap();
    assert!(iris.exists() && chinook.exists());
    assert_eq!(std::fs::read(&iris).unwrap(), IRIS_CSV);
}

/// Round-trip the bundled `demo.dat0` and verify the three structural invariants
/// required by the P11a onboarding design (D4):
///
///   1. **Chart lineage intact** — at least one `Derived` recipe table exists
///      AND the package carries a saved bar chart (the "Revenue by genre" chart
///      built over Chinook data). A flat-Base recipe means D-025 was triggered
///      (cold CLI export stripped lineage — see escalation note below).
///
///   2. **Pipeline survives** — at least one view has a non-empty
///      `transform_stack` so the PipelineBar renders after opening.
///
///   3. **SQL tab present and not auto-run** — at least one saved query carries
///      pre-filled SQL. Saved queries are *never* auto-run; the user clicks Run
///      explicitly. (SQL console tab buffers are not part of the `.dat0` package
///      format; the pre-filled query must be saved to the query library before
///      exporting so it round-trips via `PackageQuery → SavedQuery`.)
///
/// # D-025 escalation note
///
/// If this test fails because all recipe tables are classified as `Base` (no
/// `Derived` entries), the artifact was cold-CLI exported, which **flattens**
/// derived-table lineage (D-025, still open). Fix: re-export
/// `crates/dat0-core/assets/demo.dat0` through the **in-app** File → Export
/// path, which captures the live engine's `table_origins` map. The cold CLI
/// export path must never be used to regenerate this fixture.
#[tokio::test]
async fn demo_dat0_preserves_chart_lineage() {
    use dat0_core::package;
    use dat0_core::sample_data::DEMO_DAT0;
    use dat0_core::session::Session;
    use dat0_engine::{QueryEngine, chart_spec::ChartType};
    use dat0_format::Reader;

    const BUDGET: u64 = 128 * 1024 * 1024;

    // ------------------------------------------------------------------
    // STEP 1: Write the bundled bytes to a temp path and parse the package.
    // ------------------------------------------------------------------
    let tmp = tempfile::tempdir().unwrap();
    let pkg_path = tmp.path().join("demo.dat0");
    std::fs::write(&pkg_path, DEMO_DAT0).unwrap();

    let parsed = Reader::open(&pkg_path).expect("demo.dat0 must parse as a valid .dat0 package");

    // ------------------------------------------------------------------
    // STEP 2: Pre-unpack structural assertions (directly on ParsedPackage).
    //
    // These check the RECIPE, not the reopened session — if they fail,
    // the export path is wrong (D-025 risk) before we even unpack.
    // ------------------------------------------------------------------

    // 2a. At least one Derived recipe table (lineage preserved by in-app export).
    //
    // If this assertion fires: ALL tables are classified Base.  This is the
    // D-025 fingerprint — re-export via in-app (File → Export .dat0 Package…).
    // A cold CLI `dat0 export` reopens a cold engine whose `table_origins` map
    // is empty, so every table falls back to Base.
    let has_derived = parsed
        .recipe
        .tables
        .iter()
        .any(|t| t.kind == dat0_format::TableKind::Derived);
    assert!(
        has_derived,
        "demo.dat0 recipe must contain at least one Derived table. \
         All-Base means D-025 triggered: the artifact was cold-CLI exported. \
         Re-export via in-app File → Export as .dat0 Package…"
    );

    // 2b. At least one saved bar chart.
    //
    // Adjust the assertion message if the human authored a different chart type.
    let bar_chart = parsed
        .charts
        .charts
        .iter()
        .find(|c| c.spec.chart_type == ChartType::Bar);
    assert!(
        bar_chart.is_some(),
        "demo.dat0 must contain a saved bar chart; found chart types: {:?}. \
         If the authored chart is a different type, update this assertion.",
        parsed
            .charts
            .charts
            .iter()
            .map(|c| c.spec.chart_type)
            .collect::<Vec<_>>()
    );

    // 2c. The bar chart's source table is Derived in the recipe (lineage edge
    //     intact: chart → derived-query → base Chinook table).
    //
    //     `spec.source` is a DuckDB-qualified name like `"main"."revenue_by_genre"`;
    //     strip schema prefix + quotes to get the plain table name.
    //     If this assertion false-negatives due to quoting differences, tighten
    //     the name-strip logic below after authoring the real artifact.
    let chart_source = bar_chart.unwrap().spec.source.as_str();
    let plain_source = chart_source
        .rsplit('.')
        .next()
        .unwrap_or(chart_source)
        .trim_matches('"');
    let chart_source_derived = parsed
        .recipe
        .tables
        .iter()
        .any(|t| t.name == plain_source && t.kind == dat0_format::TableKind::Derived);
    assert!(
        chart_source_derived,
        "bar chart source '{}' (plain: '{}') must map to a Derived recipe table; \
         if this false-negatives on identifier quoting, tighten the name-strip logic above after authoring.",
        chart_source, plain_source
    );

    // 2d. At least one view with a non-empty pipeline (transform_stack).
    let has_pipeline = parsed
        .views
        .views
        .iter()
        .any(|v| !v.transform_stack.is_empty());
    assert!(
        has_pipeline,
        "demo.dat0 must contain at least one view with a non-empty projection pipeline \
         (so the PipelineBar is visible after opening)"
    );

    // 2e. At least one saved query carrying pre-filled SQL.
    //
    // SQL console tab buffers do NOT round-trip through the .dat0 package format
    // (they are stored in session.json sql_tabs, which is not part of the package).
    // The pre-filled "Top customers" query must be saved to the query library
    // ("Save query…") before exporting so it survives as a PackageQuery.
    // Saved queries are structurally never auto-run — the user clicks Run.
    assert!(
        !parsed.queries.queries.is_empty(),
        "demo.dat0 must contain at least one saved query (the pre-filled 'Top customers' SQL). \
         If missing, ensure the human saved the query to the library before exporting."
    );
    assert!(
        parsed
            .queries
            .queries
            .iter()
            .any(|q| !q.sql.trim().is_empty()),
        "at least one saved query must have non-empty SQL"
    );

    // ------------------------------------------------------------------
    // STEP 3: Unpack into a fresh workspace directory.
    //
    // Model: `package_roundtrip.rs` → `dat0_format::Reader::open` +
    //        `package::contents_to_workspace` + `Session::recover_workspace`.
    // ------------------------------------------------------------------
    let ws = tmp.path().join("demo_ws");
    package::contents_to_workspace(&parsed, &ws, BUDGET)
        .await
        .expect("contents_to_workspace must succeed on the real demo.dat0");

    // ------------------------------------------------------------------
    // STEP 4: Reopen and assert session-level invariants survive unpack.
    // ------------------------------------------------------------------
    let recovered = Session::recover_workspace(ws, BUDGET)
        .await
        .expect("recover_workspace must open the unpacked demo.dat0 workspace");

    // Charts survive unpack → write_session_json → recover (P9a-2 round-trip).
    assert!(
        !recovered.charts().is_empty(),
        "recovered session must have at least one saved chart"
    );
    assert!(
        recovered
            .charts()
            .iter()
            .any(|c| c.spec.chart_type == ChartType::Bar),
        "at least one bar chart must survive unpack → recover_workspace"
    );

    // Pipeline (transform_stack) survives for at least one grid tab.
    assert!(
        recovered
            .tabs()
            .iter()
            .any(|t| !t.transform_stack.is_empty()),
        "at least one recovered tab must retain a non-empty pipeline after unpack"
    );

    // Saved queries (the pre-filled SQL) survive.
    assert!(
        !recovered.saved_queries().is_empty(),
        "recovered session must have at least one saved query (the pre-filled SQL tab content)"
    );
    // Structural 'not auto-run' guarantee: saved queries carry SQL text only;
    // no execution result is embedded and no auto-run flag exists.
    // The user must explicitly click Run — consistent with the never-auto-run
    // discipline from P9c-2 and the onboarding design (§6).
    assert!(
        recovered
            .saved_queries()
            .iter()
            .any(|q| !q.sql.trim().is_empty()),
        "saved query must have non-empty SQL content"
    );

    recovered.engine.close().await.unwrap();
}
