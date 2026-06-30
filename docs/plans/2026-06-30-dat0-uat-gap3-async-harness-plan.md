# UAT Gap 3 — Async-flow support in the gpui harness — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the gpui behavioral harness drive interactive async flows that hit `tokio::task::spawn_blocking` (engine ops / file import) to completion under `#[gpui::test]`, instead of panicking `"no reactor running"`.

**Architecture:** Mirror production `run_app` (`window.rs:1308`/`:1377`): a test-support helper builds a multi-thread tokio `Runtime`, the test holds it alive + `enter()`s it on the test thread (gpui polls foreground tasks on the spawning thread, so `Handle::current()` resolves when the future calls `spawn_blocking`), and `cx.executor().allow_parking()` lets `run_until_parked` wait for the cross-thread blocking-pool completion. No engine changes; test-support only.

**Tech Stack:** Rust, gpui 0.2.2 (`test-support`), tokio (multi-thread runtime), DuckDB engine, existing `VisualTestContext` harness.

## Global Constraints

- **No engine changes** — the ~30 `tokio::task::spawn_blocking` sites in `crates/dat0-engine/src/duckdb_engine.rs` (+ `execute/streaming.rs`) stay exactly as they are.
- **Test-support only** — no production behavior change. If the spike forces a minimal production seam, STOP and surface it for approval before adding it.
- **All existing `onboarding_gpui` tests stay green** (currently 8 pass + 1 `#[ignore]`). The new code must not touch or regress them; `build_empty_session` keeps its current signature/behavior for them.
- **CI-faithful** — every verification runs with `CI=1` (so `insta` and any snapshot behave as on CI) on the local box; both-platform CI is the merge gate.
- **DCO sign-off** — every commit uses `git commit -s` and ends with the `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>` trailer.
- **Determinism** — assert only on FINAL state (table exists), never intermediate frames; `allow_parking` reintroduces real cross-thread timing.
- **Branch:** `uat-gap3-async-harness` (already created off `main` @ `a35a0a5`; design doc committed at `6a841d1`).

## Resolved planning decision

The shared-`tests/support/` lift is **deferred to the Gap-2 cycle** (YAGNI: Gap 3 needs only `enter_async_harness` reachable from `onboarding_gpui.rs`; the lift is best done when Gap-2's kittest test is a real second consumer to validate the module shape against). `enter_async_harness` lives in `onboarding_gpui.rs` alongside the existing helpers for now.

## File structure

- **Modify:** `crates/dat0-app/tests/onboarding_gpui.rs` — add the `AsyncHarness` helper + `enter_async_harness`; add the Task-1 spike test; rewrite the Task-2 proof test. This is the only code file touched.
- No new files; no production files; no `Cargo.toml` change (tokio + gpui `test-support` are already dev-deps).

## Reference facts (verified against the pinned tree)

- Production runtime: `crates/dat0-app/src/window.rs:1308` (`Runtime::new()`) + `:1377` (`runtime.enter()` held for all of `Application::run`).
- gpui 0.2.2: `ForegroundExecutor` polls a future on the thread it was spawned from (`executor.rs:40-41`); `cx.executor().allow_parking()`/`forbid_parking()` + `run_until_parked` + `block_test`/`block` exist (`executor.rs:406-423`, `platform/test/dispatcher.rs:196-204`).
- Sample-open flow: `WorkspaceShell::open_sample_kind` (`window.rs:5613`) extracts the bundled CSV synchronously, then `cx.spawn(handle_drop(path, session) → route_drop_outcomes)`. `handle_drop` registers the table in the engine via `spawn_blocking`; `route_drop_outcomes` (`window.rs:5740`) then `GridDataSource::new(...).await` and binds the shell's (private) `view_model`.
- Observable: `Session.engine` is `pub engine: Arc<DuckDBEngine>` (`session/mod.rs:196`); `QueryEngine::get_tables()` (re-exported `dat0_engine::QueryEngine`) returns the registered tables. The bundled sample table is named `iris` (from `iris.csv`).
- Existing helpers in `onboarding_gpui.rs`: `set_config_dir`, `build_empty_session(state_root) -> Arc<Mutex<Session>>` (builds + drops its own throwaway runtime), `open_shell_window(cx, session) -> (Entity<WorkspaceShell>, &mut VisualTestContext)`, `init_components`, dispatcher helpers. `BUDGET: u64 = 128 MiB`.

---

### Task 1: T0 spike — prove the mechanism + land the harness helper (HARD GATE)

**Files:**
- Modify: `crates/dat0-app/tests/onboarding_gpui.rs` (add helper + spike test)

**Interfaces:**
- Produces (consumed by Task 2):
  - `struct AsyncHarness { rt: tokio::runtime::Runtime }` with:
    - `fn enter(&self) -> tokio::runtime::EnterGuard<'_>` — caller binds to a `_guard` held to end-of-test.
    - `fn block_on<F: std::future::Future>(&self, f: F) -> F::Output`.
  - `fn enter_async_harness(cx: &mut gpui::TestAppContext) -> AsyncHarness` — builds a multi-thread `Runtime` (`enable_all`) and calls `cx.executor().allow_parking()`.
  - `fn build_empty_session_in(h: &AsyncHarness, state_root: &std::path::Path) -> Arc<Mutex<Session>>` — like `build_empty_session` but constructs `Session::new` on the harness runtime (so the session's engine shares the entered runtime).

- [ ] **Step 1: Write the helper code**

Add near the top helpers of `onboarding_gpui.rs`:

```rust
/// A tokio runtime kept alive for the whole test so the foreground-polled
/// `cx.spawn` futures can call `tokio::task::spawn_blocking` (the engine and
/// file-import paths are built on it). Mirrors production `run_app`
/// (window.rs:1308/1377), which holds `runtime.enter()` for all of
/// `Application::run`. The caller MUST bind `enter()`'s guard to a `_guard`
/// held to end-of-test, and the harness MUST outlive every spawned task.
struct AsyncHarness {
    rt: tokio::runtime::Runtime,
}

impl AsyncHarness {
    fn enter(&self) -> tokio::runtime::EnterGuard<'_> {
        self.rt.enter()
    }
    fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.rt.block_on(f)
    }
}

/// Build the async harness and switch the gpui test executor into parking mode
/// so `run_until_parked` waits for cross-thread `spawn_blocking` completions
/// instead of panicking on the would-park JoinHandle.
fn enter_async_harness(cx: &mut TestAppContext) -> AsyncHarness {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    cx.executor().allow_parking();
    AsyncHarness { rt }
}

/// Like `build_empty_session`, but constructs the session on the harness
/// runtime so its engine shares the runtime the test has entered.
fn build_empty_session_in(h: &AsyncHarness, state_root: &Path) -> Arc<Mutex<Session>> {
    let sess = h
        .block_on(Session::new(state_root, BUDGET))
        .expect("Session::new");
    Arc::new(Mutex::new(sess))
}
```

- [ ] **Step 2: Write the spike test**

Add at the end of `onboarding_gpui.rs`. It drives ONE real engine-backed async op (a `handle_drop` of a tiny generated CSV) through `cx.spawn` to completion and asserts the table landed — the exact `spawn_blocking` boundary that panics today.

```rust
// ----------------------------------------------------------------------------
// Gap 3 — async-flow support: canonical "engine op completes in-harness" test.
// ----------------------------------------------------------------------------

/// T0 spike (now a permanent regression test): with an entered tokio runtime +
/// `allow_parking`, a `cx.spawn`ed flow that hits `tokio::task::spawn_blocking`
/// (here a real CSV import via the production `handle_drop`) runs to COMPLETION
/// under `#[gpui::test]` — no "no reactor running" panic, and the table is
/// registered in the engine afterwards.
#[gpui::test]
#[serial]
fn engine_backed_async_flow_completes_in_harness(cx: &mut TestAppContext) {
    use dat0_engine::QueryEngine as _;

    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    let harness = enter_async_harness(cx);
    let _guard = harness.enter(); // held to end-of-test

    init_components(cx);
    let session = build_empty_session_in(&harness, state.path());

    // A tiny CSV on disk; importing it exercises the spawn_blocking engine path.
    let csv = state.path().join("spike.csv");
    std::fs::write(&csv, "a,b\n1,2\n3,4\n").unwrap();

    let (_shell, cx) = open_shell_window(cx, Arc::clone(&session));
    cx.run_until_parked();

    // Drive the real import flow from a spawned task, exactly like the UI does.
    let sess = Arc::clone(&session);
    let csv2 = csv.clone();
    cx.cx.spawn(async move |_app| {
        let _ = dat0_app::file_drop::handle_drop(vec![csv2], sess).await;
    })
    .detach();
    cx.run_until_parked();

    // The import's spawn_blocking completed → the engine has the `spike` table.
    let engine = session.lock().engine.clone();
    let tables = harness
        .block_on(async move { engine.get_tables().await })
        .expect("get_tables");
    assert!(
        tables.iter().any(|t| t.name == "spike"),
        "engine-backed async import must complete in-harness (tables: {:?})",
        tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    drop(state);
}
```

> NOTE — exact APIs to confirm while implementing this step (do NOT guess): the `cx.cx.spawn`/`AsyncApp::spawn` closure signature at gpui 0.2.2 (mirror the `onboarding_open_shows_carousel` test's `cx.cx.update(...)` usage and `open_sample_kind`'s `cx.spawn(async move |weak, async_cx| ...)`); `handle_drop`'s exact path (`dat0_app::file_drop::handle_drop`); and `TableInfo`'s field name (`.name`). Adjust to the real signatures — the structure (spawn import → run_until_parked → assert engine table) is fixed.

- [ ] **Step 3: Run the spike — THE GATE**

Run: `CI=1 cargo test -p dat0-app --test onboarding_gpui engine_backed_async_flow_completes_in_harness -- --nocapture`
Expected: **PASS** (the `spike` table is found).

**If it FAILS, STOP and apply fallbacks in order (do not proceed to Task 2 until green):**
1. Don't rely on `run_until_parked` for the cross-thread wake — capture the task and drive it: `let t = cx.cx.spawn(...);` then `harness.block_on(t)` (or `cx.background_executor().block(t)`); keep `run_until_parked` for the gpui side.
2. If thread-affinity still bites, restructure as a manual harness (not `#[gpui::test]`) that drives the gpui `TestAppContext` inside `harness.block_on(async { ... })` so the whole test body runs in tokio context.
Document which path worked in a comment on the test, and record the outcome for the design doc.

- [ ] **Step 4: Verify the existing suite still passes**

Run: `CI=1 cargo test -p dat0-app --test onboarding_gpui`
Expected: PASS — previously-passing tests still green (8 + the new spike = 9 pass, 1 ignored), no panics, no hangs.

- [ ] **Step 5: fmt + clippy**

Run: `cargo fmt -p dat0-app -- --check && cargo clippy -p dat0-app --tests`
Expected: clean (no diff, no warnings).

- [ ] **Step 6: Commit**

```bash
git add crates/dat0-app/tests/onboarding_gpui.rs
git commit -s -m "test(harness): enter tokio runtime + allow_parking so engine async flows complete

Adds enter_async_harness/AsyncHarness + build_empty_session_in and a canonical
#[gpui::test] proving a spawn_blocking-bearing engine import (via production
handle_drop) runs to completion under the gpui test executor — mirroring
run_app's runtime.enter() (window.rs:1308/1377). Closes the prior 'no reactor
running' boundary at the mechanism level (UAT Gap 3 spike).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Upgrade the proof target (`hero_sample_click_extracts_bundled_csv`)

**Files:**
- Modify: `crates/dat0-app/tests/onboarding_gpui.rs` (rewrite the existing test)

**Interfaces:**
- Consumes from Task 1: `enter_async_harness`, `AsyncHarness::{enter, block_on}`, `build_empty_session_in`.

- [ ] **Step 1: Rewrite the test to drive the full async import**

Replace the body of `hero_sample_click_extracts_bundled_csv` (currently the `catch_unwind` + `set_hook` version) with the full-completion version. Keep the same click mechanics (`window_registry::install_state_root`, the `(1700, 40)` sample-card click on the fixed 1920×1080 `TestDisplay`), but use the async harness and assert the engine table instead of the on-disk CSV.

```rust
/// The first hero sample card ("Iris", id `hero-sample-0`) drives the production
/// `open_sample_kind` → `cx.spawn(handle_drop → route_drop_outcomes)` flow to
/// COMPLETION: the bundled CSV is extracted AND imported, leaving the `iris`
/// table registered in the engine. (Pre-Gap-3 this test could only assert the
/// synchronous CSV extraction because the async import's `spawn_blocking`
/// panicked under the gpui test executor; the async harness now drives it home.)
#[gpui::test]
#[serial]
fn hero_sample_click_imports_bundled_csv(cx: &mut TestAppContext) {
    use dat0_engine::QueryEngine as _;
    use gpui::{Modifiers, point, px};

    let cfg = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    set_config_dir(cfg.path());

    // Plain hero (no enriched band / auto-show) keeps the sample column at the
    // deterministic top of the right column.
    let store = SettingsStore::with_path(cfg.path().join("settings.toml"));
    set_first_run_done(&store, true).unwrap();
    // open_sample_kind early-returns unless the state root is installed.
    dat0_app::window_registry::install_state_root(state.path().to_path_buf());

    let harness = enter_async_harness(cx);
    let _guard = harness.enter(); // held to end-of-test

    init_components(cx);
    let session = build_empty_session_in(&harness, state.path());
    let (_shell, cx) = open_shell_window(cx, Arc::clone(&session));
    cx.run_until_parked();

    let iris_csv = state.path().join("samples").join("iris.csv");
    assert!(!iris_csv.exists(), "precondition: sample not yet extracted");

    // Click the first sample card; the spawned import now runs to completion.
    cx.simulate_click(point(px(1700.), px(40.)), Modifiers::none());
    cx.run_until_parked();

    // Sync side-effect still holds: bundled CSV extracted.
    assert!(iris_csv.exists(), "hero-sample-0 must extract the bundled Iris CSV");

    // Async tail completed: the `iris` table is registered in the engine.
    let engine = session.lock().engine.clone();
    let tables = harness
        .block_on(async move { engine.get_tables().await })
        .expect("get_tables");
    assert!(
        tables.iter().any(|t| t.name == "iris"),
        "clicking hero-sample-0 must import Iris to completion (tables: {:?})",
        tables.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    drop(state);
}
```

Remove: the `std::panic::take_hook`/`set_hook`/`catch_unwind` block and its explanatory NOTE comments (the headless boundary they documented no longer exists). Remove the now-unused `iris.exists()`-only assertion. Rename the function `hero_sample_click_extracts_bundled_csv` → `hero_sample_click_imports_bundled_csv`.

> NOTE while implementing: confirm `TableInfo.name`, the sample table name (`iris` from `iris.csv`), and that the `(1700, 40)` coordinate still hits the card (unchanged from the prior test). If the second async op in `route_drop_outcomes` (`GridDataSource::new`) needs an extra `run_until_parked`, add one before the assert — the assert (engine has `iris`) is the fixed contract.

- [ ] **Step 2: Run the upgraded test**

Run: `CI=1 cargo test -p dat0-app --test onboarding_gpui hero_sample_click_imports_bundled_csv -- --nocapture`
Expected: PASS (CSV extracted AND `iris` table present).

- [ ] **Step 3: Teeth check (prove the async assertion is live)**

Temporarily change the asserted table name to a bogus value (`"iris_NOPE"`), run the test, confirm it now FAILS on the engine-table assertion (proving the assert exercises the completed import, not just the sync extraction). Revert.

Run: `CI=1 cargo test -p dat0-app --test onboarding_gpui hero_sample_click_imports_bundled_csv`
Expected: FAIL on the `iris` assertion → revert → PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/dat0-app/tests/onboarding_gpui.rs
git commit -s -m "test(harness): drive hero sample import to completion (retire headless boundary)

Upgrades the hero-sample-0 behavioral test from catch-the-panic + assert
CSV-on-disk to driving the full open_sample_kind -> handle_drop import via the
async harness and asserting the iris table is registered in the engine. Removes
the panic-hook/catch_unwind workaround. (UAT Gap 3 proof target.)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Workspace gate + CI-faithful verification

**Files:** none (verification only).

- [ ] **Step 1: Full onboarding suite, CI-faithful**

Run: `CI=1 cargo test -p dat0-app --test onboarding_gpui`
Expected: all green — the original 8 + 2 new (spike + upgraded proof) = 10 pass, 1 ignored (`carousel_next_back_navigation_is_human_uat`, untouched — that's Gap 2). No panics, no hangs.

- [ ] **Step 2: Touched-crate workspace gate**

Run: `cargo fmt -p dat0-app -- --check` then `cargo clippy -p dat0-app --tests` then `CI=1 cargo test -p dat0-app`
Expected: fmt clean, clippy clean, dat0-app tests green (no regression elsewhere in the crate from the new dev-test code).

- [ ] **Step 3: Push + open PR (same flow as the insta slices)**

```bash
git push -u origin uat-gap3-async-harness
gh pr create --base main --head uat-gap3-async-harness \
  --title "test(harness): async-flow support in the gpui harness (UAT Gap 3)" \
  --body "<summary: mechanism, spike outcome, proof target, scope; per design doc>"
```
Then watch PR checks (both platforms) → on green, merge (squash) → watch the post-merge main run (per project CI lore). Update memory `dat0_uat_automation_research.md` to record Gap 3 closed.

---

## Self-Review

**Spec coverage** (design doc → tasks):
- Async-flow mechanism (entered runtime + `allow_parking`) → Task 1 helper + spike. ✓
- T0 spike as hard gate + ordered fallbacks → Task 1 Step 3. ✓
- Reuse runtime for `Session::new` → `build_empty_session_in` (Task 1). ✓
- Proof target (upgrade `hero_sample_click_…`, remove catch_unwind, assert import completion) → Task 2. ✓
- Constraints (no engine change, test-support only, existing tests green, CI-faithful, DCO) → Global Constraints + Task 3. ✓
- Open decision (tests/support lift) → Resolved: deferred. ✓
- Risks (hang, guard lifetime, determinism) → encoded as `_guard` held to end-of-test, final-state-only asserts, `--nocapture` spike. ✓

**Placeholder scan:** Task code blocks are concrete; the two `NOTE while implementing` callouts name exact APIs to confirm against the real tree (not deferred work) — acceptable because the assertion contract is fixed and only signatures are confirmed at implement-time. No TBD/TODO.

**Type consistency:** `AsyncHarness` / `enter_async_harness` / `build_empty_session_in` signatures match between Task 1 (produces) and Task 2 (consumes). `engine.get_tables()` + `TableInfo.name` used identically in both tests. `BUDGET`, `set_config_dir`, `open_shell_window`, `init_components` reused from existing helpers verbatim.
