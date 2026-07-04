# UAT Slice 4 — Crash / Report-a-Bug dialogs (design)

> **Date:** 2026-07-04 · **Branch:** `uat-crash-report-slice` off `main` (`9842759`)
> Fourth slice of the manual-UAT-backlog retirement via the AccessKit + async GPUI
> harness (Slices 1 Settings, 2 Update/About, 3 Charts merged). Covers **P10c
> crash-reporting UI** (`docs/plans/2026-06-24-dat0-p10c-uat.md` §1–5).

## Problem

P10c shipped the crash/bug-report modal (`view/crash_report.rs::open_report`) and the
relaunch prompt wiring, but the **UI content + behavioral layer** and the **relaunch-gate
composition** carry no automated coverage. Everything else is already unit-tested and must
NOT be duplicated: sentinel fs lifecycle (`tests/crash_staging.rs`), `CrashGuard` arm/drop
(`tests/crash_guard.rs`), `should_prompt` + kind→key selection (`report_logic.rs` inline),
submit-is-no-op-when-inactive (`tests/telemetry_submit.rs`), `before_send` redaction
(`tests/telemetry.rs`), CLI parse (`tests/cli_telemetry_test.rs`).

Two genuine gaps:
1. **The modal** — does the crash/bug dialog render the right content, and do Send/Dismiss
   honor their side-effect contract (both `clear_staged`; Send additionally submits)?
2. **The relaunch gate** at `window.rs:1852` — the glue that reads opt-in, detects a prior
   crash, reads the staged payload, and branches to show-dialog vs silently-discard. It lived
   inside a `dispatcher().dispatch` closure with **zero coverage**, including the
   privacy-critical opt-out → discard path (UAT §3).

## Key enabling fact

`capture()` (`telemetry/mod.rs:85`) early-returns when `is_active()==false`, and telemetry is
inactive unless a test calls `Telemetry::init(true)`. So a test that never initializes
telemetry can safely press **Send** — no network, no 5s `sentry::flush` block. Unlike Slice 2
(Send → real browser `open_url`) the side effect self-disables, which unlocks the
*behavioral* half (Send → `clear_staged` → close) that Slice 2 had to leave to a human.

## Approach

- **Part A — relaunch-gate pure seam.** Extract the `window.rs:1852` gate, behavior-preserving,
  into `report_logic::resolve_relaunch_action(dir, opt_in) -> RelaunchAction`
  (`ShowCrash | DiscardMarkerOnly | DiscardOptOut | Nothing`). The window closure collapses to
  a thin match; both discard arms still `clear_staged`. Unit-tested with `tempfile` (no GPUI,
  no config seam) — the opt-out-never-shows guarantee gains its first coverage.
- **Part B — modal tests** (`tests/crash_report_window.rs`, `a11y-capture`). Mount the shared
  `DialogHost`; invoke `open_report(app, kind, tempdir)` directly. Send = `enter`
  (`on_ok`), Dismiss = `escape` (`on_cancel`); settle with `advance_clock(1s)`. Every test
  asserts `!telemetry::is_active()` at entry (the safety spine). Hermetic via the injected
  `data_dir` tempdir — no `#[serial]`, no `DAT0_CONFIG_DIR`.
- **Part C — body seam + shared host.** The dialog body (`.child(String)`) is
  AccessKit-invisible; add a `#[cfg(feature = "a11y-capture")]` seam that `cfg`-*selects* the
  child (labeled `div` in test, plain `.child(body)` in release) → **release element tree is
  byte-identical; no owed human visual glance** (improves on Slice-3's inert wrapper). Hoist
  `DialogHost`/`open_dialog_host`/`dialog_open` from `tests/update_about_window.rs` into
  `tests/support/mod.rs` for reuse.

## T0 spike findings (settled the seam scope)

Probed under the real `DialogHost`: **body captured = true, title = false, note placeholder =
false.** gpui-component renders the Dialog title in its own chrome (no a11y node) and does not
surface the `Input` placeholder. Verdict: **seam the body only** — it carries the full
semantic content *and* the crash-vs-bug differential ("dat0 quit unexpectedly…" vs "Describe
what happened…"). A title seam would be redundant; a note-field seam would assert a low-value
presence whose submit path is unobservable (submit is a no-op). Both omitted on evidence.

## Coverage (9 tests)

- 4 modal (`crash_report_window.rs`): crash body captured + Send→clear_staged (the T0 gate);
  bug body distinct from crash; Dismiss→discard-without-send; Report-a-Bug creates no sentinel
  (§2.5).
- 4 relaunch-seam (`report_logic.rs`): opt-out-never-shows (§3), opt-in+staged→ShowCrash,
  opt-in+marker-only→DiscardMarkerOnly, opt-in+clean→Nothing.

## Stays human-UAT (unchanged)

Real GlitchTip upload + redaction-on-the-wire (§1.5/1.6/2.4/4), the actual panic trigger
(process crash), signed-build/OS-integration path, GlitchTip API-path confirmation. No new
owed visual glance — the body seam `cfg`-selects to identical release markup.
