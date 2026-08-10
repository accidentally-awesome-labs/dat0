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

- [ ] **Nav / a11y suite green** under `--features a11y-capture` — `keyboard_nav`,
      `input_nav`, `sql_console_nav`, `sql_console_transient_nav`,
      `cell_editor_nav`, `catalog_nav`, `ai_nav`, `recents_nav`, `a11y_content`,
      `a11y_spike`. These assert labels and focus sequences, not colours, so
      token swaps are invisible to them. A slice that changes an a11y **label
      string** updates the assertion in the SAME PR.
      `cargo test -p dat0-app --features a11y-capture`
- [ ] **Escape ladder — all 5 rungs intact**, and `register_sql_console_keys` is
      called by both production and the harness. Keyboard behaviour is driven
      with `simulate_keystrokes`, never `dispatch_action` (transient-bars lesson).
- [ ] **Theme switching + contrast gates green**, and every new panel entity
      holds a theme subscription.
      `cargo test -p dat0-app --test theme_contrast_gate`
- [ ] **Grid perf**: `Table` + `GridTableDelegate` unchanged, or the change is
      justified here and re-baselined.
- [ ] **crash-e2e + session round-trip green**; any session-schema change is
      additive-only.
- [ ] **Perf budget** — `cargo xtask perf --check` passes on the release host.
      Attach the JSON, or apply the `run-perf` label to run the blocking
      `perf-gate` job on this PR.

## Standard gate

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo test -p dat0-app --features a11y-capture`

## Ratchets and register

- [ ] Any ratcheted quantity this PR moves (`MAX_LINES`, `MAX_SHELL_FIELDS`,
      `DOCK_MOUNT_CALLS`, the `style_lint` `ALLOW` table) is adjusted **in this
      PR**, by the measured delta — those tests fail stale-under as well as over.
- [ ] `docs/deferrals.md` entries this PR opens or closes are edited here.
- [ ] New invariant introduced → a ratchet test defends it.
