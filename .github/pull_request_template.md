<!--
The checklist below is the per-slice invariant list from
docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md §8, plus the perf budget.
These are not suggestions — every one of them exists because something broke.

Tick a box only after you have SEEN the command pass. If an item genuinely does
not apply to this PR, strike it through and say why in one line; do not delete
it silently.
-->

## What this changes

<!-- One paragraph. What behaviour is different after this merges? -->

## Why

<!-- The problem, with file:line evidence where it is a defect. -->

## Per-slice invariants

- [ ] **Nav / a11y suite green** — `keyboard_nav`, `input_nav`,
      `sql_console_nav`, `sql_console_transient_nav`, `cell_editor_nav`,
      `catalog_nav`, `ai_nav`, `recents_nav`, `a11y_content`, `a11y_spike`.
      These assert labels and focus sequences, not colours, so token swaps are
      invisible to them. A slice that changes an a11y **label string** updates
      the assertion in the SAME PR. The `data-a11y-id` handles ship in release,
      so there is no capture feature to enable.
      `cargo test -p dat0-ui`
- [ ] **Escape ladder — all 5 rungs intact**, and the SQL console's key
      registration is exercised by both production and the harness. Keyboard
      behaviour is driven through the harness's key events, never by calling an
      action directly (transient-bars lesson).
- [ ] **Theme switching green**, and every surface reads its tokens through
      `var(--d0-…)` rather than a baked colour.
      `cargo test -p dat0-ui --test theme_live_switch --test style_lint`
- [ ] **Appearance defended** — the SSR scene catalogue covers any new surface
      or state, and its snapshots are reviewed rather than blind-accepted.
      A geometry change re-runs the real-window probe on a machine with a
      display; it is not a CI job (no display on hosted runners, see D-032).
      `cargo test -p dat0-ui --test visual_snapshot`
      `cargo run -p dat0-ui --features visual --example visual_probe`
- [ ] **Grid perf**: the `GridDataSource` paging path is unchanged, or the
      change is justified here and re-baselined.
- [ ] **crash-e2e + session round-trip green**; any session-schema change is
      additive-only.
- [ ] **Perf budget** — `cargo xtask perf --check` passes on the release host.
      Attach the JSON, or apply the `run-perf` label to run the blocking
      `perf-gate` job on this PR.

## Standard gate

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Ratchets and register

- [ ] Any ratcheted quantity this PR moves (`window_module_ratchet`'s ceilings,
      `design_contract`'s track counts, the `style_lint` `ALLOW` table) is
      adjusted **in this PR**, by the measured delta — those tests fail
      stale-under as well as over.
- [ ] `docs/deferrals.md` entries this PR opens or closes are edited here.
- [ ] New invariant introduced → a ratchet test defends it.
