# dat0 — UAT Gap 3: async-flow support in the gpui behavioral harness (design)

**Date:** 2026-06-30
**Branch:** `uat-gap3-async-harness` (off `main` @ `a35a0a5`)
**Status:** design — approved, pre-plan
**Related:** 2026-06-29 UAT-automation research (memory `dat0_uat_automation_research.md`); follows the merged `insta` serialized-state tier (PR #35 `b79bfd0`, PR #36 `a35a0a5`).

## Context

dat0 has a headless gpui behavioral harness (`crates/dat0-app/tests/onboarding_gpui.rs`: `add_window_view` + `VisualTestContext` + `run_until_parked` + `simulate_click` + `debug_bounds`) that drives real production UI code and asserts observable state. The 2026-06-29 research flagged three gaps the harness can't cover. This design addresses **Gap 3** only. **Gap 2** (AccessKit content assertion) is a separate, larger effort that gets its own brainstorm → spec → plan cycle *after* this one (decided: two specs, Gap 3 first). **Gap 1** (visual regression) stays human-UAT (blocked at the gpui level).

## Problem

Interactive flows that touch the DuckDB engine or file import call `tokio::task::spawn_blocking`. The engine is built on ~30 such sites (`crates/dat0-engine/src/duckdb_engine.rs`, `execute/streaming.rs`); the file-drop path sniffs + imports via `spawn_blocking` (`crates/dat0-app/src/file_drop.rs:76`). Under `#[gpui::test]` the foreground executor is **not** a tokio runtime and no `Handle` is entered on the polling thread, so a `cx.spawn`ed future that reaches `spawn_blocking` panics inside the detached task with `"no reactor running"`.

Today the harness works around this in `hero_sample_click_extracts_bundled_csv`: it installs a no-op panic hook, wraps the click in `catch_unwind`, and asserts only the **synchronous** side-effect (the bundled CSV was extracted to disk before the spawn). The async tail — the table actually importing and the tab opening — is explicitly human-UAT-owed.

## Root cause (verified against code)

Production `run_app` (`crates/dat0-app/src/window.rs:1308` + `:1377`) creates `tokio::runtime::Runtime::new()` and holds `runtime.enter()` for the **entire** `Application::run` closure. That single entered guard is why `spawn_blocking` resolves `Handle::current()` everywhere in the running app. Tests never establish this context.

Verified gpui 0.2.2 facts that make the fix sound (not speculative):
- `ForegroundExecutor` polls a spawned future **on the same thread it was spawned from** (runtime-checked, `executor.rs:40-41`). So a tokio runtime entered on the test thread is in scope when the foreground-polled future calls `spawn_blocking`.
- The test executor exposes `allow_parking()` / `forbid_parking()` (default: forbid). `run_until_parked` panics on a would-park task **unless** parking is allowed (`executor.rs:406-423`, `platform/test/dispatcher.rs`). It also exposes `block_test` / `block`.

## Approach

Mirror production inside the harness. A small test-support helper:

1. Builds a multi-thread tokio `Runtime`, holds it alive for the whole test, and `enter()`s it on the test thread → `Handle::current()` resolves when the foreground future calls `spawn_blocking`.
2. Calls `cx.executor().allow_parking()` so `run_until_parked` **waits** for the cross-thread blocking-pool completion instead of panicking on the would-park JoinHandle.
3. Reuses that same runtime for `Session::new`, replacing the throwaway runtime currently created (and dropped) inside `build_empty_session`.

## Components

- **`enter_async_harness(cx) -> AsyncHarness`** — new test-support helper (lifted alongside the existing helpers in `onboarding_gpui.rs`; see Decomposition note). Returns a guard bundling the live `Runtime` + its `EnterGuard`. Dropping it ends the entered scope; it is held to end-of-test.
- **`build_empty_session` refactor** — take the harness runtime by reference rather than building/dropping its own, so session construction and interactive flows share one runtime.
- No new production code (test-support only), **unless the spike forces a seam** (see Risks).

## T0 spike (HARD GATE)

Before building the helper or touching the proof test, prove one engine-backed async op runs to completion under `#[gpui::test]`:

> Open a window over a real session, trigger a single engine round-trip (a query or a small import) from a `cx.spawn`ed flow, `run_until_parked`, and assert the result lands.

**Gate:** if it works → proceed with the approach above. **If it fails** (cross-thread wake doesn't unpark `run_until_parked`, or thread-affinity bites the entered guard), fall back, in order:
1. Explicitly drive the spawned task to completion with `cx.background_executor().block(handle)` rather than relying on `run_until_parked`.
2. A manual harness (not `#[gpui::test]`) that drives the gpui test context **inside** `rt.block_on`, so the whole test runs in tokio context.

The spike decides the mechanism; the approach is not committed until the spike is green. The spike test is kept as the canonical "engine op completes in-harness" example.

## Proof target

Upgrade `hero_sample_click_extracts_bundled_csv`:
- Remove the `std::panic::set_hook` + `catch_unwind` workaround.
- Drive the full async sample import to completion.
- Assert the sample table is actually registered in the session (the tab opens / the table exists), not merely that the CSV reached disk.

This directly retires the documented headless boundary called out in that test's doc comment.

## Risks & mitigations

- **Hung async op hangs the test.** `allow_parking` lets the test wait on real wakes, so a stuck op would hang rather than fail fast. Mitigate: keep the op small; rely on the engine's existing cancel/timeout; consider a wall-clock guard if needed.
- **Guard lifetime.** The entered runtime must outlive every spawned task → the guard is held to end-of-test (returned to the test, dropped last).
- **Determinism.** `allow_parking` reintroduces real cross-thread timing. Keep assertions on **final** state (table exists / tab open), never intermediate frames. This is the same discipline the existing harness already follows.
- **Multi-thread vs current-thread runtime.** Start multi-thread (matches production + gives a real blocking pool). If determinism suffers, the spike can try a current-thread runtime with a dedicated blocking pool.

## Scope (NOT doing)

- No engine changes — the ~30 `spawn_blocking` sites stay exactly as they are.
- No Gap-2 / AccessKit work (separate cycle).
- No production behavior change — test-support only, unless the spike forces a minimal seam (which would be surfaced and justified).

## Testing

- The proof target (`hero_sample_click_...` upgraded) is the primary test.
- The spike test stays as a minimal, documented "engine-backed async op completes in-harness" reference.
- All existing `onboarding_gpui` tests must stay green; the shared helper refactor must not regress them.
- CI-faithful run (`CI=1`) on both platforms before merge, per established practice.

## Decomposition note

The async-harness helpers (and the existing `set_config_dir` / `build_empty_session` / `open_shell_window` / dispatcher helpers currently local to `onboarding_gpui.rs`) are good candidates to lift into a shared `tests/support/` module so Gap-2's kittest tests and future behavioral tests can reuse them. Whether to do that lift **in this effort** or defer it is a planning decision; the proof target only needs the helper reachable.
