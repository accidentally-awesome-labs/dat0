# UAT Slice 5 — MotherDuck UI (design)

> **Date:** 2026-07-05 · **Branch:** `uat-motherduck-slice` off `main` (`02ad054`)
> Fifth slice of the manual-UAT-backlog retirement via the AccessKit + async GPUI
> harness (Slices 1 Settings, 2 Update/About, 3 Charts, 4 Crash/Report merged).
> Covers **P9b MotherDuck UI** (`docs/plans/2026-06-15-dat0-p9b-uat.md`, PR #24
> `9f5d7a2`).

## Problem

P9b shipped three MotherDuck (MD) UI surfaces whose **render / state-display layer**
carries no automated coverage:

1. **Catalog "Cloud" group** — `catalog/tree.rs` routes an `Attached { source }` table
   with `source.starts_with("md:")` into a `cloud` vec; `catalog/panel.rs` renders it as a
   distinct 4th section (i18n `catalog.cloud`, stable ElementId `"Cloud"`).
2. **Test-connection result** — `connections/panel.rs` renders a "Test connection" button
   in the Disconnected + Connected `md_actions` arms (absent in Error), and a transient
   `md_test_result` message below the MD section.
3. **Routing chip** — the SQL-console timing chip appends a routing suffix
   (`local` / `md` / `mixed`) from `last_routing`.

Everything else in the P9b surface is already unit-tested and must **NOT** be duplicated:
the classifier logic (`catalog/tree.rs` inline tests), `connect::test_result_message`
(pure status→string), `ConnectionManager` state incl. `md_test_result` round-trip
(`connections/mod.rs::state_tests`), `classify_routing` (`connections/routing.rs` inline),
the token-never-logged guard (`connections/mod.rs::state_tests`), and the live-account
origin-contract test (`dat0-engine/tests/md_attach.rs`, `#[ignore]`, CI-gated). The genuine
gap is the **state → render wiring**: does seeded MD state actually paint the right content.

## Key reframing (why this is automatable now)

The old "ENVIRONMENT-HEAVY" label on the MD backlog is about the **live token round-trip**
(attach / connect / `spawn_md_test` / a real routed query) — that **stays human** regardless.
But the UI content + **injectable state** layer needs **no `MOTHERDUCK_TOKEN`, no keychain,
no engine** — exactly like Slice 3 injected a fake catalog. The enabling facts:

- `ConnectionManager::set_md_status(ConnectionStatus)` is **public** (`connections/mod.rs:50`),
  as are `set_md_test_result` / `set_md_databases` → all three `md_actions` arms
  (Disconnected / Connected / Error) are seedable directly, no live connection.
- `WorkspaceShell::seed_catalog_for_test(Vec<TableInfo>)` (`window.rs:6618`) + `refresh_catalog`
  already exist (Slice 3) → seed an `md:`-origin `TableInfo`, rebuild the tree, the Cloud
  group populates with no engine.
- The routing chip is **already a11y-seamed** (`sql_console.rs:931`,
  `.a11y_label(AccessRole::Label, chip_text)`); only its `last_routing` needs driving.

## Scope

**Click-free, inject-and-assert.** No clicks on any MD button (Test → live `spawn_md_test`;
Connect → token prompt; both are the live paths). Every surface is driven by seeding state
then `A11ySnapshot::capture`. No network, no keychain, no engine, no side effects → **no
safety spine needed** (unlike Slice 4's `!is_active()` telemetry spine — there is simply no
side-effect path exercised here). The behavioral "click Test → see a real result" journey
**stays human** — that is the honest boundary.

User-confirmed scope (2026-07-05): **all three surfaces**, including the routing chip
(acknowledged marginal — chip already seamed, `classify_routing` already unit-tested; the new
coverage is only the `md`-suffix *render*).

## Approach

Full `WorkspaceShell` mount (Slice-3 pattern) in a new `tests/motherduck_window.rs`. Mount
helpers (`init_components` / `build_empty_session` / `open_shell_window` / async harness) are
**copied** into the new binary, matching precedent (`chart_uat_window.rs`, `a11y_content.rs`
each keep local copies; only `A11ySnapshot` / `DialogHost` live in `tests/support/mod.rs`).
Centralizing the mount helpers is a real improvement but an out-of-scope refactor of three
existing test binaries — not this slice.

### Seams (production edits)

All three seams **chain `.a11y_label` onto an element that already exists** in the tree, so
release markup is unchanged (`a11y-capture` OFF → `.a11y_label` is an identity no-op,
`{ self }`). This is the fully-inert Slice-3 `chain_row` / Slice-4 outcome — **no new wrapper
`div`, no owed human visual glance** (beats Slices 1/3, which shipped inert wrappers).

| Surface | File | Edit |
|---|---|---|
| Cloud group rows | `catalog/panel.rs` `catalog_row` (~:65) | chain `.a11y_label(Label, name)` on the existing row `div().id(...)` |
| MD buttons (all arms) | `connections/panel.rs` `action_button` (:203) | chain `.a11y_label(Label, label)` on the existing button `div().id(...)` — one edit seams Connect / Test / Disconnect / Forget / Retry / Attach / Detach and yields free present/absent teeth |
| Test-result message | `connections/panel.rs` (:140) | chain `.a11y_label(Label, msg)` on the existing result `div().child(...)` |
| Routing chip | *(none)* | already seamed at `sql_console.rs:931` |

`A11yExt::a11y_label` requires `InteractiveElement`; every target is a `div` / `Stateful<Div>`
(satisfied). `cfg`-gate the `use crate::a11y::{A11yExt as _, AccessRole};` imports in each
edited file to avoid an unused-import error under release `-D warnings`.

### Test-only shims (`window.rs`, `#[cfg(feature = "a11y-capture")] pub fn` on `WorkspaceShell`)

- `open_connections_for_test(&mut self)` → `self.connections_panel_visible = true;`
- `connections_mut_for_test(&mut self) -> &mut ConnectionManager` → the test drives state via
  the existing public setters (`set_md_status` / `set_md_test_result` / `set_md_databases`).
  Requires `dat0_app::connections::{ConnectionManager, ConnectionStatus}` to be reachable from
  the integration crate — **T0 confirms/exports**; fallback is a shim taking primitives.
- `seed_routing_chip_for_test(&mut self, ms: u64, routing: Routing, cx)` → reaches the SQL
  console entity and calls `set_last_elapsed(ms, routing, cx)` (`sql_console.rs:340`). Requires
  `Routing` export + the SQL console mounted/visible in the shell — **T0 crux** (heaviest path).
- Cloud group reuses the existing `seed_catalog_for_test` + `refresh_catalog` — no new shim.
  Confirm the catalog panel is default-visible in a freshly-mounted shell, else add
  `open_catalog_for_test`.

## T0 spike (hard-gate — run FIRST, spike EVERY asserted surface)

Slice-3 lesson: *a T0 spike only proves the surface it exercises.* Before any T1, one
throwaway spike must prove each of the three paint paths captures content under the mount:

- **(a) Cloud group** — seed an `md:`-origin `TableInfo` + `refresh_catalog`, catalog panel
  visible → `A11ySnapshot` sees the table name under Cloud.
- **(b) Test button + result** — seed a `ConnectionManager` arm + `md_test_result` → snapshot
  sees the Test button label and the result message.
- **(c) Routing chip** — seed `last_routing = Md` + `last_elapsed_ms` → snapshot sees the chip
  text with the localized MD routing label.

The spike also settles the open cruxes (R1–R4 below) and CHECKs that
`connect::test_result_message` is already unit-tested (don't dup). If the routing-chip mount
proves disproportionately heavy or brittle relative to its marginal coverage, surface it to
the user rather than sink the slice into it.

## Tests (`tests/motherduck_window.rs`, ~5)

1. `cloud_group_renders_md_attached_table` — seed one `md:`-origin table **and** one `File`
   table → refresh → the md table appears under Cloud; the File table does **not** (the
   classifier → render teeth).
2. `test_result_renders_disconnected` — status = Disconnected + `md_test_result = "…ok"` →
   Test button present + result message present.
3. `test_result_renders_connected` — status = Connected + a seeded db name + a result →
   Test + Disconnect buttons + db name present.
4. `error_hides_test_shows_retry` — status = `Error("boom")` → Retry present, **Test button
   absent**, error message shown (the arm differential).
5. `routing_chip_shows_md_suffix` — seed `last_routing = Md` + elapsed → chip text contains the
   localized MD routing label; teeth vs the default `local`.

Tests 2+3 may fold if the spike shows the result render is arm-independent; the plan decides.

## Cost / footprint

- **Zero new dependencies** — Cargo.lock and NOTICE unchanged; **D-015 stays open** (no gpui
  fork; `a11y-capture` remains a test-only, release-off feature).
- **Zero owed human visual glances** — every seam is inert-identical in release.
- Test-only; production seams are release no-ops.

## Risks / open T0 cruxes

- **R1 — routing chip mount.** Reaching the SQL console entity + exporting `Routing` +
  `set_last_elapsed` reachability is the heaviest path for the least new coverage (chip already
  seamed, classifier already unit-tested). Prove viability early in T0; escalate if it balloons.
- **R2 — pub-reachability.** `connections::{ConnectionManager, ConnectionStatus}` may not be
  reachable from the integration crate; if so, either add `pub` or make the shim take primitives.
- **R3 — catalog panel default visibility** in a freshly-mounted shell (else add a shim).
- **R4 — bare-`div` `.a11y_label` trait bound.** Low risk — the error banner precedent
  (`banner.rs:241`) chains `.a11y_label` on a plain div; T0 re-proves for the result-message div.

## Not doing (explicit scope cuts)

- No clicks on any MD button (live paths → human).
- No live connect / attach / real routed query (human UAT).
- No re-test of `test_result_message` / `classify_routing` / classifier / token-guard /
  origin-contract (already unit-tested).
- No centralizing of the duplicated mount helpers (out-of-scope refactor).
