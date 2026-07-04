# dat0 UAT automation — Slice 3: Charts save / persist / lineage (P9a-2)

**Date:** 2026-07-03
**Branch:** `uat-charts` off `main` `5196c76`
**Slice of:** the manual-UAT backlog automation effort (follows Slice 1 Settings window PR #39 `78f6ff9`, Slice 2 Update+About dialogs PR #40 `5196c76`).
**Feature under test:** P9a-2 "Save chart → persist + lineage" (shipped PR #23 `4aeb1a0`, session schema v9).

## Goal

Close the rendered-UI + behavioral UAT gap for the Charts *save / persist / reopen* flow using the existing AccessKit + async headless harness (`a11y-capture` feature). Pixel/plotters-image correctness stays human (Gap 1). Test-only footprint: zero release-code behavior change, zero new dependencies, NOTICE/lock unchanged, D-015 stays open.

## Triage — what is already covered (do NOT re-test beyond the light belt)

The genuine gap is smaller than "~10 charts tests" implies. Already covered by existing tests:

- **Lineage chart-node *structure*** — unit-tested in `crates/dat0-app/src/inspector/lineage.rs:252-267` (node `kind`, `edge`, `descendants`, openability).
- **Chart persist *round-trip*** — `crates/dat0-app/tests/package_roundtrip.rs:88-193` asserts `SavedChart` → `PackageChart` → unpack → recover survives (name / chart_type / y).
- **Session.json wire format** — snapshot-gated by `crates/dat0-app/tests/snapshots/session_migration__session_json_wire_format.snap` (fixture currently carries `"charts": []` — an *empty* array only).
- **Reopen *routing*** — wired at `crates/dat0-app/src/inspector/panel.rs:285-287`: a `NodeKind::Chart` row's `on_click` routes to `ws.open_saved_chart(name, …)`.
- **Save-toast a11y** — `render_banner` already emits `.a11y_label(AccessRole::Alert, title)` (`crates/dat0-app/src/error_ux/banner.rs:241`). The "Chart saved" banner is capturable with **no new seam**.

## Genuine residual gap (this slice)

The rendered-UI + save/reopen behavioral layer:

1. **Chart panel content** — `charts/panel.rs::render_chart_body` is a11y-dark; headless cannot assert the panel shows chart type / axis / title.
2. **Save → toast + persist** — the `SaveChart` confirm (`window.rs::save_named_chart:4277`) untested at the app level.
3. **Lineage panel *render* + click-reopen behavioral** — that a saved chart appears as a rendered 📊 node and clicking it actually reopens the panel.
4. **Reopen restores spec** — `open_saved_chart` → `show_chart_with_spec` sets the persisted spec verbatim (not blanked), asserted through rendered content.

## Design

### 1. Harness

Full `WorkspaceShell` mount. Template = `crates/dat0-app/tests/onboarding_gpui.rs:136-160`:

- `cx.add_window_view(move |window, cx| { let shell = cx.new(|c| WorkspaceShell::new(session, c)); … })` → `(Entity<WorkspaceShell>, &mut VisualTestContext)`.
- Capture via `support::A11ySnapshot::capture(cx)` (the Slice-1/2 support crate).
- App-level free-fn / method calls via `TestAppContext::update` — **not** nested inside `VisualTestContext::update` (double-borrow; precedent `onboarding_gpui.rs:196-199`).
- New integration file `crates/dat0-app/tests/chart_uat_window.rs`, compiled only under the `a11y-capture` feature (self-dev-dependency, `crates/dat0-app/Cargo.toml:91,120-124`).
- Settle before capture: `cx.executor().advance_clock(…)` / `run_until_parked()` so render-tick paths fire (Slice-1/2 pattern).

### 2. Seams (the only production-file change)

Add `.a11y_label(AccessRole::Label, …)` content locators in `charts/panel.rs::render_chart_body` for **chart type**, **X axis**, **Y axis**, and **title**. Pattern mirrors `inspector/panel.rs` lineage rows (`.a11y_label(AccessRole::Label, …)`). These are inert single-purpose annotations: release no-op semantically, but a real layout-tree change → **human visual glance owed** on the Charts dock (same class as the Settings wrapper divs and the Slice-2 dialog bodies).

Toast (Alert) and lineage 📊 node (Label) are **already seamed** → untouched.

### 3. Test inventory (~10)

| # | Test | Kind | Anchor / note |
|---|------|------|----------------|
| 1 | Panel renders bound-spec content (type + X + Y) | render | via new seams |
| 2 | Empty-state renders `chart.panel.empty` | render | `charts/panel.rs:98-101` |
| 3 | `save_named_chart` → "Chart saved" Alert captured **and** `session.charts()` has 1 entry with the bound name + spec | behavioral | toast `banner.rs:241`; persist `window.rs:4294-4297` |
| 4 | Save with empty/whitespace name → no-op (no chart, no toast) | behavioral | guard `window.rs:4278` |
| 5 | Save with no bound source → no-op | behavioral | guard `window.rs:4284` |
| 6 | Lineage panel renders 📊 chart node + name label | render | seed session with a `SavedChart` whose `spec.source` is the inspected table (the derived `ChartNode.source_table` roots the edge); `inspector/panel.rs:266` |
| 7 | Click chart node → panel becomes visible + reopen fires | behavioral | route `inspector/panel.rs:286` → `open_saved_chart` |
| 8 | Reopen restores spec content (type/x/y not blanked) | behavioral | `open_saved_chart:4312` → `show_chart_with_spec`; asserted via new seams |
| 9 | **insta**: populated-`charts` session.json wire snapshot | snapshot | gate the non-empty chart field shape |
| 10 | Session-level chart round-trip re-assert (belt) | round-trip | complements `package_roundtrip.rs` at the session (not package) layer |

Tests may split/merge during planning; ~10 is the target.

### 4. insta belt (the "full" addition)

A snapshot gating a **non-empty** `charts` array's wire shape (the existing session-format snapshot only proves `[]`). Add a new snapshot test (own name, e.g. `chart_saved__session_json_wire_format`) seeding one `SavedChart`, then `insta::assert_json_snapshot!` on the serialized session.

⚠️ **Determinism requirement:** `SavedChart` in production is built with `Uuid::now_v7()` + `now_unix_millis()` (`window.rs:4289-4292`) — both time-based, non-deterministic. The snapshot fixture MUST construct the `SavedChart` with a **hardcoded `Uuid`** and a **fixed `saved_at`**, never the live constructors, or the snapshot will churn every run.

### 5. Risks — all hard-gated by the T0 spike

- **MainThreadDispatcher**: `toggle_chart_panel` requires a dispatcher and silently drops the bind otherwise (`window.rs:3526`). Bypass by binding `chart_panel` directly and setting `spec` (no dispatcher). `show_chart_with_spec` sets the spec verbatim *before* its off-thread `describe_table` (axis-opts only), so content assertions hold without a dispatcher. T0 confirms.
- **`maybe_prompt_save_workspace`** at end of `save_named_chart` (`window.rs:4303`) may open a dialog / read `crate::platform::config_dir()`. If so, apply the `DAT0_CONFIG_DIR` + `#[serial]` seam (Slice-1 hermeticity trap; copy `set_config_dir` from `tests/a11y_content.rs:64-68`, `serial_test` dev-dep) and/or dismiss the dialog via `simulate_keystrokes("escape")`. T0 confirms whether needed.
- **Seam = layout change** → human visual glance owed on the Charts dock (tracked, one-line `#[cfg]`-gate available if a gap shows, per Slice-2 precedent).

### 6. Non-goals

- Pixel / plotters-image correctness (Gap 1 — human).
- PNG/SVG export byte correctness (P9a-1 already benched/tested).
- Re-testing lineage *structure* or full package round-trip beyond the light session-level belt (already unit-tested).
- OS integration, live external round-trips.

## Build process

SDD (subagent-driven development):

- **T0 spike hard-gate first** — prove the full-shell mount + save flow + capture end-to-end (mirrors Slice-1/2 T0 which caught the dialog-layer paint gap). Resolve the two risks above before any real test lands.
- T1..Tn per the inventory; TDD per task; two-stage (spec + quality) review per task; final whole-branch opus review = Ready-to-merge gate.
- **Anti-loop exec**: implementer subagents run ONLY the focused test (`cargo test -p dat0-app --test chart_uat_window --features a11y-capture`) synchronously; the CONTROLLER runs the `cargo test --workspace` + `clippy --workspace --all-targets -D warnings` + fmt gate.
- DCO: planning commits predate signed work → `git rebase --signoff main` before push (recurring pattern). All impl commits `-s` inline.
- Watch the **post-merge main run** (macOS grid-scroll bench is push-to-main-only → can redden main silently).

## Owed after merge

One human visual glance at the Charts dock (new content seams) — joins the standing manual-UAT backlog.
