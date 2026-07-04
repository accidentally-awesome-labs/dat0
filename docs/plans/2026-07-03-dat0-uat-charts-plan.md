# Charts save/persist/lineage UAT slice — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automate the rendered-UI + behavioral UAT for the Charts save/persist/reopen flow (P9a-2) using the existing AccessKit + async headless harness.

**Architecture:** A new `a11y-capture`-gated integration test file mounts a full `WorkspaceShell` (the `onboarding_gpui.rs` template), drives save/reopen/lineage through `#[cfg(feature="a11y-capture")] pub *_for_test` shims + real a11y clicks, and asserts rendered content via `.a11y_label` seams captured by `A11ySnapshot`. The only production change is adding content seams to `charts/panel.rs::render_chart_body`; a populated-chart `insta` snapshot gates the session-JSON wire shape.

**Tech Stack:** Rust, gpui 0.2.2 + gpui-component, accesskit/kittest (test-only), insta, serial_test, tokio, DuckDB.

## Global Constraints

- Branch `uat-charts` off `main` `5196c76`.
- **Zero new dependencies.** NOTICE.md + Cargo.lock must be unchanged. D-015 stays open (no gpui fork; test-only capture).
- All test-crate machinery compiles **only** under the `a11y-capture` feature (self-dev-dependency, `crates/dat0-app/Cargo.toml:91,120-124`). Release builds must be byte-unaffected in behavior.
- `*_for_test` shims MUST be `#[cfg(feature = "a11y-capture")] pub fn` and MUST be placed **before** any `#[cfg(test)] mod tests` in the file (clippy `items-after-test-module` under `-D warnings` — caught by the controller gate, not focused tests).
- Anti-loop exec: implementer runs ONLY the focused test `cargo test -p dat0-app --test chart_uat_window --features a11y-capture` synchronously. The **controller** runs the workspace gate: `cargo test --workspace`, `cargo clippy --workspace --all-targets --all-features -D warnings`, `cargo fmt --check`.
- DCO: planning commits predate signed work → `git rebase --signoff main` before push. All impl commits `-s` inline, trailer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- Owed after merge: one human visual glance at the Charts dock (new inert content seams).

## File Structure

- **Create** `crates/dat0-app/tests/chart_uat_window.rs` — the whole slice's tests + file-local harness helpers (copied, per prior-slice convention: `set_config_dir`, `build_empty_session`, `open_shell_window`, `enter_async_harness`). Uses the shared `mod support;` (`A11ySnapshot`).
- **Create** `crates/dat0-app/tests/chart_wire_snapshot.rs` — the `insta` populated-chart session-JSON snapshot + session round-trip belt (kept separate: pure-serde, no gpui/`a11y-capture`, no `#[serial]`; mirrors `session_migration.rs`).
- **Modify** `crates/dat0-app/src/charts/panel.rs` — add `.a11y_label` content seams to `render_chart_body` + a `chart_type_label` helper.
- **Modify** `crates/dat0-app/src/window.rs` — add `#[cfg(feature="a11y-capture")] pub *_for_test` shims on `WorkspaceShell`.
- **Create** `crates/dat0-app/tests/snapshots/chart_wire_snapshot__*.snap` — the accepted insta snapshot (committed).

The two test files share nothing; `chart_wire_snapshot.rs` needs no window/harness.

---

## Task 0: Spike — prove mount + seams + shims end-to-end (HARD GATE)

Establishes the whole mechanism (mirrors Slice-1/2 T0 which caught the dialog-layer paint gap). Nothing else starts until this is green. Resolves the two design risks: dispatcher bypass and config_dir hermeticity.

**Files:**
- Modify: `crates/dat0-app/src/charts/panel.rs` (seams + helper)
- Modify: `crates/dat0-app/src/window.rs` (shims)
- Create: `crates/dat0-app/tests/chart_uat_window.rs`
- Test: `crates/dat0-app/tests/chart_uat_window.rs::spike_bound_chart_renders_spec_content`

**Interfaces:**
- Produces (panel.rs): `render_chart_body(panel, image, logical)` unchanged signature, now emitting `.a11y_label(AccessRole::Label, …)` for type/x/y/title; `fn chart_type_label(t: ChartType) -> gpui::SharedString`.
- Produces (window.rs shims, all `#[cfg(feature="a11y-capture")] pub` on `WorkspaceShell`):
  - `fn chart_bind_for_test(&mut self, source: String, cols: Vec<(String, String)>)` — `self.chart_panel.bind(source, cols); self.chart_panel_visible = true;`
  - `fn chart_set_axes_for_test(&mut self, chart_type: ChartType, x: Option<String>, y: Option<String>, title: String)` — sets `self.chart_panel.spec.{chart_type,x,y,title}`.
  - `fn chart_visible_for_test(&self) -> bool` — returns `self.chart_panel_visible`.
  - `fn chart_spec_for_test(&self) -> crate::charts::spec::ChartSpec` — returns `self.chart_panel.spec.clone()`.
  - `fn save_named_chart_for_test(&mut self, name: String, cx: &mut Context<Self>)` — calls `self.save_named_chart(name, cx)`.
  - `fn seed_catalog_for_test(&mut self, tables: Vec<dat0_engine::TableInfo>)` — `self.catalog_tables = tables;`
  - `fn seed_lineage_target_for_test(&mut self, name: String, cx: &mut Context<Self>)` — `self.inspector.set_target(name); self.recompute_lineage(); self.inspector_panel_visible = true; cx.notify();` (deliberately skips `load_inspector_profile`'s off-thread SUMMARIZE — isolates lineage render).
  - `fn open_saved_chart_for_test(&mut self, name: String, window: &mut gpui::Window, cx: &mut Context<Self>)` — calls `self.open_saved_chart(name, window, cx)`.

Confirm the exact `dat0_engine::TableInfo` construction shape during this task (`grep -n "pub struct TableInfo" crates/dat0-engine/src/*.rs`); use its real fields in `seed_catalog_for_test` callers (Task 3).

- [ ] **Step 1: Add the `chart_type_label` helper + seams to `render_chart_body`**

In `crates/dat0-app/src/charts/panel.rs`, add the trait import at the top (after existing `use` lines):

```rust
use crate::a11y::{A11yExt as _, AccessRole};
use gpui::SharedString;
```

Add the helper above `render_chart_body`:

```rust
/// Localised display name for a chart type (used by the content seam so the
/// headless UAT can assert the *rendered* type). Keys already exist in en.json.
pub(crate) fn chart_type_label(t: ChartType) -> SharedString {
    let key = match t {
        ChartType::Bar => "chart.type.bar",
        ChartType::Line => "chart.type.line",
        ChartType::Area => "chart.type.area",
        ChartType::Scatter => "chart.type.scatter",
        ChartType::Histogram => "chart.type.histogram",
        ChartType::BoxPlot => "chart.type.boxplot",
        ChartType::Heatmap => "chart.type.heatmap",
    };
    dat0_i18n::t(key).into()
}
```

Confirm the `ChartType` variant names against `crates/dat0-engine/src/chart_spec.rs:10` (`ChartType::ALL`) and fix any mismatch before compiling.

Replace the final line of `render_chart_body` (`div().flex().flex_col().gap_2().child(body)`) with a version that prepends an inert seam row:

```rust
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
```

- [ ] **Step 2: Add the shims to `window.rs`**

In `crates/dat0-app/src/window.rs`, immediately before the file's `#[cfg(test)] mod tests` block (or at the end of the `impl WorkspaceShell` block if none), add all shims listed under **Interfaces** above. Example (the rest follow the same pattern):

```rust
#[cfg(feature = "a11y-capture")]
impl WorkspaceShell {
    pub fn chart_bind_for_test(&mut self, source: String, cols: Vec<(String, String)>) {
        self.chart_panel.bind(source, cols);
        self.chart_panel_visible = true;
    }
    pub fn chart_set_axes_for_test(
        &mut self,
        chart_type: crate::charts::spec::ChartType,
        x: Option<String>,
        y: Option<String>,
        title: String,
    ) {
        self.chart_panel.spec.chart_type = chart_type;
        self.chart_panel.spec.x = x;
        self.chart_panel.spec.y = y;
        self.chart_panel.spec.title = title;
    }
    pub fn chart_visible_for_test(&self) -> bool {
        self.chart_panel_visible
    }
    pub fn chart_spec_for_test(&self) -> crate::charts::spec::ChartSpec {
        self.chart_panel.spec.clone()
    }
    pub fn save_named_chart_for_test(&mut self, name: String, cx: &mut Context<Self>) {
        self.save_named_chart(name, cx);
    }
    pub fn seed_catalog_for_test(&mut self, tables: Vec<dat0_engine::TableInfo>) {
        self.catalog_tables = tables;
    }
    pub fn seed_lineage_target_for_test(&mut self, name: String, cx: &mut Context<Self>) {
        self.inspector.set_target(name);
        self.recompute_lineage();
        self.inspector_panel_visible = true;
        cx.notify();
    }
    pub fn open_saved_chart_for_test(
        &mut self,
        name: String,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.open_saved_chart(name, window, cx);
    }
}
```

- [ ] **Step 3: Write the spike test (harness helpers + one smoke assertion)**

Create `crates/dat0-app/tests/chart_uat_window.rs`. Copy the harness helpers verbatim from `crates/dat0-app/tests/onboarding_gpui.rs`: the `mod support;` line, `set_config_dir` (`:60-64`), `build_empty_session` (`:77-86`), `open_shell_window` (`:141-157`), and `enter_async_harness`/`AsyncHarness` (`:94-124`). Then:

```rust
#![cfg(feature = "a11y-capture")]

use gpui::TestAppContext;
use serial_test::serial;
use std::time::Duration;
use support::A11ySnapshot;

use dat0_app::charts::spec::ChartType;

// ... (copied helpers: set_config_dir, build_empty_session, open_shell_window,
//      enter_async_harness, AsyncHarness) ...

#[gpui::test]
#[serial]
fn spike_bound_chart_renders_spec_content(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.chart_bind_for_test(
                "\"sales\"".into(),
                vec![("region".into(), "VARCHAR".into()), ("amt".into(), "DOUBLE".into())],
            );
            ws.chart_set_axes_for_test(
                ChartType::Bar,
                Some("region".into()),
                Some("amt".into()),
                "Sales by region".into(),
            );
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label_contains("Bar"), "chart type seam rendered");
    assert!(snap.has_label("region"), "x-axis seam rendered");
    assert!(snap.has_label("amt"), "y-axis seam rendered");
    assert!(snap.has_label("Sales by region"), "title seam rendered");
}
```

- [ ] **Step 4: Run the spike; verify it PASSES**

Run: `cargo test -p dat0-app --test chart_uat_window --features a11y-capture spike_bound_chart_renders_spec_content -- --nocapture`
Expected: PASS. If 0 nodes captured, the paint path is wrong — verify `render_chart_body` is reached (`window.rs:6551`, gated on `chart_panel_visible` at `:6544`) and that `advance_clock` + `run_until_parked` ran before capture. **This is the gate: do not proceed until green.**

- [ ] **Step 5: Controller workspace gate + commit**

Controller runs: `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -D warnings`, `cargo test --workspace`, and `git diff --stat NOTICE.md Cargo.lock` (must be empty).

```bash
git add crates/dat0-app/src/charts/panel.rs crates/dat0-app/src/window.rs crates/dat0-app/tests/chart_uat_window.rs
git commit -s -m "test(harness): chart UAT spike — panel content seams + shims (T0)"
```

---

## Task 1: Panel content render tests

**Files:**
- Test: `crates/dat0-app/tests/chart_uat_window.rs` (add tests)

**Interfaces:**
- Consumes: `chart_bind_for_test`, `chart_set_axes_for_test` (T0); `A11ySnapshot::{has_label, has_label_contains}` (`tests/support/mod.rs:104,148`).

- [ ] **Step 1: Write the empty-state test**

```rust
#[gpui::test]
#[serial]
fn chart_panel_empty_state_renders_hint(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    // Visible panel, but no columns/axes bound → empty hint renders.
    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.chart_bind_for_test("\"t\"".into(), vec![]);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.has_label_contains("Select columns"),
        "empty-state hint rendered (chart.panel.empty)"
    );
}
```

- [ ] **Step 2: Write the per-type axis-content test** (a second type, distinct from the spike's Bar)

```rust
#[gpui::test]
#[serial]
fn chart_panel_renders_scatter_axes(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| {
            ws.chart_bind_for_test(
                "\"m\"".into(),
                vec![("x".into(), "DOUBLE".into()), ("y".into(), "DOUBLE".into())],
            );
            ws.chart_set_axes_for_test(ChartType::Scatter, Some("x".into()), Some("y".into()), String::new());
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label_contains("Scatter"), "scatter type seam rendered");
    assert!(snap.has_label("x") && snap.has_label("y"), "axis seams rendered");
}
```

- [ ] **Step 3: Run focused tests; verify PASS**

Run: `cargo test -p dat0-app --test chart_uat_window --features a11y-capture chart_panel_ -- --nocapture`
Expected: both PASS.

- [ ] **Step 4: Controller gate + commit**

```bash
git add crates/dat0-app/tests/chart_uat_window.rs
git commit -s -m "test(harness): chart panel content + empty-state render tests (T1)"
```

---

## Task 2: Save → toast + persist + guards

**Files:**
- Test: `crates/dat0-app/tests/chart_uat_window.rs` (add tests)

**Interfaces:**
- Consumes: `chart_bind_for_test`, `chart_set_axes_for_test`, `save_named_chart_for_test` (T0); `A11ySnapshot::query_by_role(AccessRole::Alert, …)` (`support/mod.rs:117`); `error_ux::banner::drain_pending()` (`banner.rs:143`) to clear the process-global `PENDING` queue between serial tests.
- Persist assertions read `session.lock().charts()` (`session/mod.rs:617`).

Note the toast title is `dat0_i18n::t("chart.save.done.title")` = "Chart saved" (`en.json:213`); it is painted via the already-seamed `render_banner` (`banner.rs:241`, `AccessRole::Alert`). The banner queue is a process-global static, so these tests are `#[serial]` and drain `PENDING` at entry.

- [ ] **Step 1: Write the save→toast+persist test**

```rust
#[gpui::test]
#[serial]
fn save_chart_shows_toast_and_persists(cx: &mut TestAppContext) {
    let _ = dat0_app::error_ux::banner::drain_pending(); // clear cross-test PENDING
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let harness = enter_async_harness(cx); // save_named_chart → refresh_catalog may spawn
    let _g = harness.enter();
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session.clone());

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            ws.chart_bind_for_test(
                "\"sales\"".into(),
                vec![("region".into(), "VARCHAR".into()), ("amt".into(), "DOUBLE".into())],
            );
            ws.chart_set_axes_for_test(ChartType::Bar, Some("region".into()), Some("amt".into()), String::new());
            ws.save_named_chart_for_test("Q1 sales".into(), cx);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    // Persisted into the session.
    {
        let s = session.lock();
        assert_eq!(s.charts().len(), 1, "one saved chart");
        assert_eq!(s.charts()[0].name, "Q1 sales");
        assert_eq!(s.charts()[0].spec.chart_type, ChartType::Bar);
        assert_eq!(s.charts()[0].spec.y.as_deref(), Some("amt"));
    }
    // Toast rendered.
    let snap = A11ySnapshot::capture(vcx);
    assert!(
        snap.query_by_role(support::AccessRole::Alert, "Chart saved"),
        "Chart saved alert rendered"
    );
}
```

Confirm the `AccessRole` re-export path used by `query_by_role` in `tests/support/mod.rs` and match it (either `support::AccessRole` or `dat0_app::a11y::AccessRole`).

- [ ] **Step 2: Write the two guard tests (empty name, no source → no-op)**

```rust
#[gpui::test]
#[serial]
fn save_chart_empty_name_is_noop(cx: &mut TestAppContext) {
    let _ = dat0_app::error_ux::banner::drain_pending();
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session.clone());
    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            ws.chart_bind_for_test("\"sales\"".into(), vec![("amt".into(), "DOUBLE".into())]);
            ws.save_named_chart_for_test("   ".into(), cx); // whitespace → guard at window.rs:4278
        });
    });
    vcx.run_until_parked();
    assert_eq!(session.lock().charts().len(), 0, "whitespace name saves nothing");
}

#[gpui::test]
#[serial]
fn save_chart_without_source_is_noop(cx: &mut TestAppContext) {
    let _ = dat0_app::error_ux::banner::drain_pending();
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let harness = enter_async_harness(cx);
    let _g = harness.enter();
    let session = build_empty_session(&tmp.path().join("state"));
    let (shell, vcx) = open_shell_window(cx, session.clone());
    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            // No bind → chart_panel.source is None → guard at window.rs:4284.
            ws.save_named_chart_for_test("orphan".into(), cx);
        });
    });
    vcx.run_until_parked();
    assert_eq!(session.lock().charts().len(), 0, "no source saves nothing");
}
```

- [ ] **Step 3: Run focused tests; verify PASS**

Run: `cargo test -p dat0-app --test chart_uat_window --features a11y-capture save_chart_ -- --nocapture`
Expected: all three PASS. If `save_chart_shows_toast_and_persists` panics at `tokio::spawn`, the async harness guard isn't entered — verify `_g` is bound to end of test. If persistence fails because `maybe_prompt_save_workspace` wrote elsewhere, that path is already hermetic via `set_config_dir` (session store is the injected `Arc<Mutex<Session>>`); no extra seam needed — but confirm during this task and, if a config-dir read surfaces, the `set_config_dir` + `#[serial]` seam is already in place.

- [ ] **Step 4: Controller gate + commit**

```bash
git add crates/dat0-app/tests/chart_uat_window.rs
git commit -s -m "test(harness): save-chart toast + persist + guards (T2)"
```

---

## Task 3: Lineage render + click-reopen + spec-restore

**Files:**
- Test: `crates/dat0-app/tests/chart_uat_window.rs` (add tests)

**Interfaces:**
- Consumes: `seed_catalog_for_test`, `seed_lineage_target_for_test`, `chart_visible_for_test`, `chart_spec_for_test` (T0); `A11ySnapshot::{has_label_contains, click}` (`support/mod.rs:148,175`); `session.set_charts(...)` (`session/mod.rs:622`); `dat0_engine::TableInfo` (fields confirmed in T0).
- Chart lineage nodes render at `inspector/panel.rs:266` (📊 + name `.a11y_label(Label, …)`); the `on_click` routes to `open_saved_chart` at `inspector/panel.rs:286`.

Build a `SavedChart` with a **fixed** id/`saved_at` (not `Uuid::now_v7`/`now_unix_millis`) via a small local helper so tests are deterministic:

```rust
fn seeded_chart(name: &str, source: &str, chart_type: ChartType, x: &str, y: &str) -> dat0_app::session::charts::SavedChart {
    dat0_app::session::charts::SavedChart {
        id: uuid::Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001),
        name: name.into(),
        spec: dat0_engine::chart_spec::ChartSpec {
            chart_type,
            source: source.into(),
            x: Some(x.into()),
            y: Some(y.into()),
            group: None,
            color: None,
            title: String::new(),
        },
        saved_at: 1_700_000_000_000,
    }
}
```

Confirm `ChartSpec`'s module path re-export used elsewhere (`dat0_engine::chart_spec::ChartSpec` re-exported as `charts::spec::ChartSpec`; use whichever the test crate resolves).

A minimal `TableInfo` builder for the injected catalog (fields per `dat0-engine/src/types.rs:117`; template = `inspector/lineage.rs:216`):

```rust
fn tbl(name: &str) -> dat0_engine::TableInfo {
    dat0_engine::TableInfo {
        name: name.into(),
        schema: "main".into(),
        columns: vec![],
        row_count_estimate: None,
        origin: dat0_engine::TableOrigin::File(std::path::PathBuf::from("/data/sales.csv")),
    }
}
```

The lineage closure matches on the bare `name` only, so `origin`/`columns` are irrelevant to chart-descendant attachment.

- [ ] **Step 1: Write the lineage-render test**

```rust
#[gpui::test]
#[serial]
fn saved_chart_appears_as_lineage_node(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let session = build_empty_session(&tmp.path().join("state"));
    // Seed the session with a chart rooted on table "sales".
    session.lock().set_charts(vec![seeded_chart("Region totals", "\"sales\"", ChartType::Bar, "region", "amt")]).unwrap();
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            // Inject a catalog containing "sales" so closure("sales") exists, then
            // target it → recompute_lineage attaches the chart as a descendant.
            ws.seed_catalog_for_test(vec![tbl("sales")]);
            ws.seed_lineage_target_for_test("sales".into(), cx);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label_contains("Region totals"), "chart node rendered in lineage");
}
```

- [ ] **Step 2: Write the click-reopen + spec-restore test**

```rust
#[gpui::test]
#[serial]
fn click_lineage_chart_reopens_panel_with_restored_spec(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    set_config_dir(&tmp.path().join("cfg"));
    let harness = enter_async_harness(cx); // open_saved_chart → show_chart_with_spec tokio::spawn
    let _g = harness.enter();
    let session = build_empty_session(&tmp.path().join("state"));
    session.lock().set_charts(vec![seeded_chart("Region totals", "\"sales\"", ChartType::Scatter, "region", "amt")]).unwrap();
    let (shell, vcx) = open_shell_window(cx, session);

    vcx.cx.update(|app| {
        shell.update(app, |ws, cx| {
            ws.seed_catalog_for_test(vec![tbl("sales")]);
            ws.seed_lineage_target_for_test("sales".into(), cx);
        });
    });
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    // Real click on the rendered chart node → routes to open_saved_chart.
    let snap = A11ySnapshot::capture(vcx);
    snap.click(vcx, "Region totals");
    vcx.executor().advance_clock(Duration::from_secs(1));
    vcx.run_until_parked();

    // Panel reopened with the persisted spec (verbatim, not blanked).
    let (visible, spec) = vcx.cx.update(|app| {
        shell.update(app, |ws, _cx| (ws.chart_visible_for_test(), ws.chart_spec_for_test()))
    });
    assert!(visible, "chart panel reopened");
    assert_eq!(spec.chart_type, ChartType::Scatter, "restored type");
    assert_eq!(spec.x.as_deref(), Some("region"));
    assert_eq!(spec.y.as_deref(), Some("amt"));

    // And the restored spec is visible in the rendered panel via the seams.
    let snap2 = A11ySnapshot::capture(vcx);
    assert!(snap2.has_label_contains("Scatter"), "restored type rendered");
}
```

If `A11ySnapshot::click` panics on a dynamic/duplicate label, fall back to clicking by static id via `vcx.debug_bounds(id).center()` + `simulate_click` (Slice-1 pattern) — but the chart node label ("Region totals") is unique, so `click` by label should resolve.

- [ ] **Step 3: Run focused tests; verify PASS**

Run: `cargo test -p dat0-app --test chart_uat_window --features a11y-capture lineage -- --nocapture` and `... click_lineage`
Expected: both PASS. If the lineage node does not render, verify `closure("sales")` includes the injected `TableInfo` (name must match the bare target) and `inspector_panel_visible` is true.

- [ ] **Step 4: Controller gate + commit**

```bash
git add crates/dat0-app/tests/chart_uat_window.rs
git commit -s -m "test(harness): lineage chart-node render + click-reopen restores spec (T3)"
```

---

## Task 4: insta populated-chart wire snapshot + session round-trip belt

**Files:**
- Create: `crates/dat0-app/tests/chart_wire_snapshot.rs`
- Create (accepted): `crates/dat0-app/tests/snapshots/chart_wire_snapshot__populated_chart_session_json.snap`

**Interfaces:**
- Consumes: `dat0_app::session::{SessionState, SESSION_SCHEMA_VERSION}` (`session/mod.rs:134,16`), `SavedChart`/`ChartSpec`, `insta::assert_json_snapshot!`, `serde_json`.
- Mirrors the serde-only pattern of `session_migration.rs` (no gpui, no `a11y-capture`, no `#[serial]`).

- [ ] **Step 1: Write the populated-chart snapshot test (deterministic fixture)**

```rust
//! Gate the *non-empty* `charts` array wire shape of a session (the existing
//! session-format snapshot only proves `[]`). Fixture uses a hardcoded UUID +
//! fixed `saved_at` so the snapshot is deterministic (production builds these
//! from `Uuid::now_v7()` + `now_unix_millis()`).

use dat0_app::session::SessionState;
use dat0_app::session::charts::SavedChart;
use dat0_engine::chart_spec::{ChartSpec, ChartType};

fn state_with_chart() -> SessionState {
    let mut s = SessionState::default();
    s.charts = vec![SavedChart {
        id: uuid::Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001),
        name: "Monthly totals".into(),
        spec: ChartSpec {
            chart_type: ChartType::Bar,
            source: "\"sales\"".into(),
            x: Some("month".into()),
            y: Some("total".into()),
            group: None,
            color: None,
            title: "Monthly totals".into(),
        },
        saved_at: 1_700_000_000_000,
    }];
    s
}

#[test]
fn populated_chart_session_json_wire_format() {
    let json = serde_json::to_value(state_with_chart()).unwrap();
    insta::assert_json_snapshot!("populated_chart_session_json", json);
}
```

- [ ] **Step 2: Run the test to generate the snapshot, then accept it**

Run: `cargo test -p dat0-app --test chart_wire_snapshot populated_chart_session_json_wire_format`
Expected: FAIL first run (no stored snapshot) with a pending `.snap.new`. Review the emitted JSON — assert by eye the `charts[0]` object carries `id`, `name`, `spec.{chart_type,source,x,y,group,color,title}`, `saved_at` with the fixed values. Accept:

```bash
cargo insta accept   # or: mv the .snap.new to .snap after review
```

If `cargo insta` is unavailable, rename the generated `crates/dat0-app/tests/snapshots/chart_wire_snapshot__populated_chart_session_json.snap.new` to drop `.new`.

- [ ] **Step 3: Write the session-level round-trip belt test**

```rust
#[test]
fn saved_chart_survives_session_json_round_trip() {
    let json = serde_json::to_string_pretty(&state_with_chart()).unwrap();
    let back: SessionState = serde_json::from_str(&json).unwrap();
    assert_eq!(back.charts.len(), 1);
    assert_eq!(back.charts[0].name, "Monthly totals");
    assert_eq!(back.charts[0].spec.chart_type, ChartType::Bar);
    assert_eq!(back.charts[0].spec.x.as_deref(), Some("month"));
    assert_eq!(back.charts[0].spec.y.as_deref(), Some("total"));
    assert_eq!(back.charts[0].saved_at, 1_700_000_000_000);
}
```

- [ ] **Step 4: Run both; verify PASS**

Run: `cargo test -p dat0-app --test chart_wire_snapshot`
Expected: both PASS (snapshot now stored + accepted).

- [ ] **Step 5: Controller gate + commit**

```bash
git add crates/dat0-app/tests/chart_wire_snapshot.rs crates/dat0-app/tests/snapshots/chart_wire_snapshot__populated_chart_session_json.snap
git commit -s -m "test(wire): populated-chart session.json insta snapshot + round-trip belt (T4)"
```

---

## Final integration gate (controller, before PR)

- [ ] Full workspace gate green: `cargo fmt --check` && `cargo clippy --workspace --all-targets --all-features -D warnings` && `cargo test --workspace`.
- [ ] `git diff --stat NOTICE.md Cargo.lock` is empty (zero new deps).
- [ ] Release build behavior unaffected: `cargo build -p dat0-app` (no `a11y-capture`) compiles; seams are inert.
- [ ] Whole-branch opus review (spec + quality) = Ready-to-merge, no Critical/Important.
- [ ] `git rebase --signoff main` (sign the two planning commits), push, open PR.
- [ ] Poll `gh pr checks` (not `gh run watch`); both platforms + clippy green.
- [ ] After merge: watch the **post-merge main run** — the push-to-main-only macOS grid-scroll bench can redden main silently; confirm the artifact + green.
- [ ] Record owed human visual glance (Charts dock content seams) in the manual-UAT backlog.

## Self-Review — spec coverage

| Spec item | Task |
|---|---|
| Harness: full-shell mount, `a11y-capture`-gated, zero deps | T0 (helpers + gate) |
| Seam: `charts/panel.rs` content labels (type/x/y/title) | T0 Step 1 |
| Test 1 panel content / Test 2 empty-state | T1 |
| Test 3 save→toast+persist | T2 Step 1 |
| Test 4/5 save guards | T2 Step 2 |
| Test 6 lineage 📊 render | T3 Step 1 |
| Test 7 click-reopen | T3 Step 2 |
| Test 8 spec-restore | T3 Step 2 |
| Test 9 insta populated-chart snapshot (deterministic) | T4 Step 1-2 |
| Test 10 session round-trip belt | T4 Step 3 |
| Risk: dispatcher bypass | T0/T2/T3 (async harness entered; graceful warn+drop) |
| Risk: config_dir hermeticity | `set_config_dir` in every windowed test |
| Build/DCO/bench-watch | Final integration gate |
