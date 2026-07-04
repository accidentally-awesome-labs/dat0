# UAT Slice 4 — Crash / Report-a-Bug dialogs (plan)

> Companion to `2026-07-04-dat0-uat-crash-report-design.md`. Task breakdown + verification.

## Files

| File | Change |
|------|--------|
| `crates/dat0-app/src/telemetry/report_logic.rs` | add `RelaunchAction` + `resolve_relaunch_action` + 4 unit tests (Part A) |
| `crates/dat0-app/src/window.rs` (~1852) | collapse the inline relaunch gate to a match on the seam (Part A) |
| `crates/dat0-app/src/view/crash_report.rs` | `#[cfg(feature="a11y-capture")]` body seam, `cfg`-selected (Part C) |
| `crates/dat0-app/tests/support/mod.rs` | receive hoisted `DialogHost` / `open_dialog_host` / `dialog_open` (Part C) |
| `crates/dat0-app/tests/update_about_window.rs` | use the hoisted host; drop the local copy (Part C) |
| `crates/dat0-app/tests/crash_report_window.rs` | **new** — 4 modal tests (Part B) |

## Tasks (executed in order)

- **T0 — spike hard-gate.** Prove EVERY asserted surface (Slice-3 lesson): crash mount → body
  captured under `DialogHost`; `enter` → `on_ok` → seeded `last-crash.json` removed; bug body
  captured (second paint path); probe title/note capturability. RESULT: body ✓, title ✗, note
  ✗ → seam body only. GREEN before any further task.
- **T1 — Part A.** `resolve_relaunch_action` seam + window rewire (behavior-preserving) + 4
  unit tests. `cargo test -p dat0-app --lib report_logic` → 6/6.
- **T2 — Part C.** Body seam (cfg-select) + `DialogHost` hoist to `support/mod.rs` + rewire
  `update_about_window.rs`. Regression: `cargo test -p dat0-app --test update_about_window` →
  9/9.
- **T3 — Part B.** 4 modal tests. `cargo test -p dat0-app --test crash_report_window` → 4/4.

## Verification (all GREEN on `uat-crash-report-slice`)

1. **T0 gate** — body captured, `enter`→clear_staged, bug body distinct. ✓
2. **Focused modal**: `cargo test -p dat0-app --test crash_report_window` → 4 passed. ✓
3. **Seam units**: `cargo test -p dat0-app --lib report_logic` → 6 passed. ✓
4. **Regression**: `cargo test -p dat0-app --test update_about_window` → 9 passed (post-hoist). ✓
5. **Full gate**: `cargo test --workspace` → **859 passed, 0 failed**; `cargo clippy
   --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean. ✓
6. **Release footprint**: `cargo build -p dat0-app --release` builds (seam compiles out,
   feature off); `Cargo.lock` + `NOTICE` unchanged (0 lines) → zero new deps, D-015 stays
   open. ✓

## Anti-loop / execution notes

Controller ran the workspace + clippy gate directly (implementers, when used, run only the
focused test synchronously — the stalling lesson from Slices 1–3). DCO `-s`. Watch the
post-merge main run: the push-to-main-only macOS grid-scroll bench can redden main silently.

## Stays human-UAT

Real GlitchTip upload + redaction-on-the-wire, the actual panic trigger, signed-build/OS
path, GlitchTip API-path confirmation (`docs/plans/2026-06-24-dat0-p10c-uat.md` §1.5/1.6/2.4/
3/4). No new owed visual glance (body seam → identical release markup).
