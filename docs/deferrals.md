# Deferrals & Plan-Defect Register

> **Purpose.** Single source of truth for scope splits and plan defects discovered
> during phase execution. Planning and brainstorming agents reference this document
> on entry to a phase to know what's already promised, what was punted forward, and
> what known plan-level bugs need addressing.

**Scope.** This register tracks two things:

1. **Deferrals (D-NNN)** — work that was originally scoped to one phase but was
   formally split off and re-scheduled to a later phase. Each entry records
   *what*, *from where*, *to where*, and *why*.
2. **Plan defects (PD-NNN)** — bugs in a phase's authored plan that surfaced
   during execution (e.g., wrong API signature in a snippet, stale dependency
   pin, broken filter directive). Recorded so they're fixed deliberately rather
   than accumulating as silent tech debt.

**Out of scope.** Pre-1.0 scope decisions made at spec authoring time (e.g.,
"Mac App Store deferred to v1.x", "Hosted tier deferred to post-PMF") live in
the spec's roadmap sections (§21.4 v1.x, §21.5 v2). They are not deferrals in
the sense tracked here — they were never in v1 scope.

**Status vocabulary**

| Status | Meaning |
|---|---|
| `open` | Not yet addressed. Will land in the target phase. |
| `in-progress` | Active work in the target phase. |
| `closed` | Shipped. Cross-reference the closing commit/PR/phase retro. |
| `cancelled` | Decided not to do. Cross-reference the decision rationale. |

**Update protocol.** Every phase plan must scan this register on entry and add
or close entries during execution. Every phase retro re-scans and updates
status. Treat the register as read-write within the worktree of the phase
that's modifying it; merge conflicts are signals worth investigating.

---

## At-a-glance — Deferrals

| ID    | Title                                              | Status | From | Target |
|-------|----------------------------------------------------|--------|------|--------|
| D-001 | Editable Settings widgets (author identity + theme dropdown) | closed | P1 | P3 |
| D-002 | Theme live-switch through running window           | closed | P1   | P3     |
| D-003 | Sparkle Objective-C `SUUpdater` bridge             | closed | P1   | P10a-2 |
| D-004 | AppImageUpdate subprocess invocation               | closed | P1   | P10a-2 |
| D-005 | Linux Secret Service "setup banner" UX             | open | P1   | TBD    |
| D-006 | macOS x86_64 (Intel) CI matrix coverage            | open | P1   | TBD    |
| D-007 | MotherDuck ATTACH end-to-end                       | closed | P2   | P5c    |
| D-008 | Cancellation-token wiring through `QueryEngine` trait (→ token-free in P5a) | closed | P2 | P5a |
| D-009 | Bundle `sqlite_scanner` static when duckdb-rs exposes a feature | open | P2 | TBD |
| D-010 | Non-UTF-8 file encoding handling                   | open | P2   | TBD    |
| D-011 | Remove `__debug_query_scalar` test-only helper     | closed | P2   | P3a    |
| D-012 | Engine catalog `TableInfo` synthesis (origin + schema) | closed | P2   | P3a    |
| D-013 | Self-hosted macOS CI runner (cut hosted macos-14 10× billing) | closed | P2 | superseded by MX3 → D-032 |
| D-014 | Memory Budget Settings section | closed | P3b | P3c / P9c |
| D-015 | AccessKit / screen-reader selection-tree exposure | closed | P4b | closed by the GPUI→Dioxus migration |
| D-018 | Workspace lineage DAG — node-edge graph with auto-layout (left→right topological), pan/zoom, whole-workspace view | open | P6b | — |
| D-019 | Workspace concurrency/sync-drive: cross-machine lock, sync-drive detection, rich in-use modal, Settings → Workspace, force-unlock | closed | P7a | P7b |
| D-020 | Live-data refresh: file-watcher on Tab.source_path → re-import on change (re-CTAS + replay transforms, debounced) + finish recovery_panel Sheet UI | closed | P7a | P7c |
| D-021 | Banner action buttons: `error_ux::render_banner` renders title+body only; `.with_primary` action ids stored but not displayed | closed | P7a | P7c |
| D-022 | Live-view import mode — `read_csv` VIEW that auto-reflects source-file changes with no re-import | open | P7c | — |
| D-023 | Cross-table refresh cascade — re-materialize the P6b dependency closure in topological order on a base-table refresh (P8b `ReplayEngine` provides the machinery; in-app wiring remains) | open | P7c | — |
| D-024 | Per-table / global auto-refresh toggle (+ multi-table simultaneous watching) | open | P7c | — |
| D-025 | Derived-table provenance not persisted across workspace reopen (cold CLI export flattens derived → base) | open | P8 | — |
| D-026 | Python (non-Rust) `.dat0` reader — format is reader-ready (Parquet + tagged JSON) | open | P8 | — |
| D-027 | In-app Inspect polish (read-only badge, scratch GC, multi-source GUI replay, Unpack button) | open | P8 | — |
| D-028 | Privileged `/Applications` auto-update (SMJobBless/SMAppService helper for authenticated install) | open | P10a-2 | v1.x |
| D-029 | Settings panel persist-on-render → change-gate (per-frame fsync); + P10b cleanup (orphan `SettingsView`/dead `render` trait, 2 hardcoded input placeholders, orphan `settings.update.auto_check` key) + correct the "i18n-check fails on missing keys" claim (it is warn-only) | closed | P10b | P10c |
| D-031 | Display-type letter-spacing (v4's −0.03em/−0.035em tracking) unavailable on gpui 0.2.2 — no `Styled` setter and no `TextStyle` field | closed | UI1 | closed by the GPUI→Dioxus migration |
| D-032 | Promote `perf-gate` from label-triggered to every-PR (needs dedicated macOS hardware) | open | MX3 | — |
| D-036 | Two `block_on(Session::…)` sites remain on the GPUI main thread — `workspace_ops::spawn_workspace_window` and `package_ops::open_package_at` | open | EN4 | — |
| D-037 | `docs/a11y.md` (and 8 more docs) still describe the GPUI build — dead crate `dat0-app`, dead test `theme_contrast_gate`, dead feature `a11y-capture`, dead paths `src/window/render.rs` | closed | GPUI→Dioxus migration | closed by the doc-accuracy pass after PR #82 |
| D-038 | `Coverage (report only)` exhausted the runner's disk — fixed by dropping debug info from the instrumented build (21 GB vs 64 GB); awaiting first green CI run | in-progress | GPUI→Dioxus migration | — |

## At-a-glance — Plan defects

| ID     | Title                                                              | Status | Severity |
|--------|--------------------------------------------------------------------|--------|----------|
| PD-001 | tracing EnvFilter directive `dat0=debug` doesn't match dat0 crates | open   | low      |
| PD-002 | Settings store atomic-write missing `fsync` before rename          | closed | low      |
| PD-003 | cargo-about NOTICE output not deterministic across host platforms  | closed | low      |
| PD-004 | Linux Secret Service backend not reachable from CI keychain tests  | open   | low      |
| PD-011 | P3b plan §3.7 ambiguity rule references sniff outputs that don't exist: no candidate-delimiter scores, no encoding column, no per-column confidence in `sniff_csv` | open | low |
| PD-012 | `NYC_TAXI_SHA256 = "FILL_AT_T8"` — release asset not yet uploaded, so the fetch path always fails the checksum check at runtime | open | low |
| PD-013 | P4a T0 plan-snippet drifts: `dat0-fixtures` assumed to be a lib crate; `dat0-engine` assumed to have a `benches/` dir + criterion dev-dep; plan snippet pinned `criterion = "0.5"` instead of using workspace inheritance | closed | low |
| PD-014 | P4a design §3 used `#[serde(untagged)]` on `FilterValue` + `Scalar`, causing Str/Date/Timestamp collisions and FilterValue::None/Scalar::Null collision; reworked to tagged wire shapes | closed | low |
| PD-015 | P4a plan T2–T15 snippets still use the pre-PD-014 tuple form `FilterValue::Scalar(...)` + `FilterValue::List(vec![...])` (~57 occurrences); implementers must read variants as struct form `FilterValue::Scalar { value }` + `FilterValue::List { values }` | closed | low |
| PD-016 | P4a UI-click → ViewChange wirings unowned by plan T13: funnel-click → open popover, popover `Outcome::Apply` → `vm.apply`, sort-zone-click → `vm.set_sort`. T7/T9/T10b/T12 implementers each commented "T13 wires" but plan T13 Steps 1-4 cover only keybind undo/redo + supersede-cancel test. Closed by P4b T0: `on_sort_zone_click`/`on_funnel_click`/`route_outcome` → `spawn_view_change`; `click_wiring.rs` (7 tests). | closed | medium |
| PD-017 | P4b T3 plan premise wrong: it assumed `register_file` "finalizes a CTAS import", but `register_file` emits `CREATE OR REPLACE VIEW … AS SELECT * FROM read_csv/json/parquet(…)` — a VIEW, which cannot be `ALTER TABLE`-d, so the eager `__dat0_rowid` surrogate only lands via `create_table` (base tables). The app imports files exclusively via `register_file` (`file_drop.rs:133`), so imported grids are VIEWs with no `__dat0_rowid`, and the P4b edit/delete overlay (`WHERE __dat0_rowid NOT IN …`, `CASE WHEN __dat0_rowid = …`) references a non-existent column → edit/delete fail on real imports. T3 engine work is correct + complete; resolution is app-side (materialize imports to base tables, or back-fill via `ensure_rowid` on first bind). **Closed via Path A:** new `QueryEngine::register_file_as_table` materializes imports into rowid-bearing base tables (reusing all P3b sniffing); `file_drop.rs` now calls it. | closed | high |
| PD-018 | Pre-existing (P3a-era) grid-render gap surfaced during P4b T7: `GridTableDelegate::render_td` (`grid/mod.rs`) renders the em-dash placeholder for EVERY cell — it never calls `render_cell` or `page_for`, and `page_for` (the only method that populates the page LRU from DuckDB) has ZERO production callers (no `load_more`/visible-range/prefetch). So in the running app the grid shows `—` and the cache is empty. P4b's cache-only reads (`cell_display`/`row_key`/`column_arrow_type`) resolve nothing on screen, so copy reads empty strings and paste/cut/edit skip every cell. P4b edit/select/clipboard LOGIC is correct + fully test-green (engine round-trips), but the headline T14 manual Excel/Sheets UAT is BLOCKED until the paged-render cache is wired (render_td → real values via the page LRU + prefetch visible page on bind). Out of every P4b plan task's scope. **Closed (Path A):** `render_td` now does a synchronous LRU lookup → real `render_cell` value; the LRU is populated off-thread by `WorkspaceShell::prefetch_visible_rows` (page-0 prefetch on grid bind + the gpui-component `TableDelegate::visible_rows_changed` scroll hook), notifying the main thread via the `MainThreadDispatcher`. Also wired the right-click context menu (`ContextMenuExt`), a per-cell focus ring, and the forward-incompat recover banner. | closed | high |
| PD-019 | P4c T13 wired header single-click → select-column but could NOT wire row-gutter click → select-row: the gpui-component `TableDelegate` trait (rev `0f0ab35`) has no `render_row_header`/gutter seam, and `TableState::render_table_row` owns the row layout internally. Two alternatives were rejected: (a) subscribing to `TableEvent::SelectRow` makes every row-body click select a whole row, clobbering the single-cell click selection wired in T5; (b) a fake first column holding row numbers corrupts the `col_ix` passed to `render_td` and breaks column addressing. `WorkspaceShell::select_row_at` IS implemented + reachable programmatically; the click wiring is unwired. | open | low |
| PD-020 | P4c T14 wired inline-editor `Enter` → commit + move-DOWN + focus-on-mount, but `Tab` → commit + move-RIGHT could NOT be wired: gpui-component `Input` (rev `0f0ab35`) consumes Tab internally for focus tab-stops and surfaces no `InputEvent::PressTab` variant (`InputEvent` is `{ Change, PressEnter, Focus, Blur }`). **Closed by Phase 6 of the GPUI→Dioxus migration (2026-08-10):** the limitation was the toolkit's, and a plain `<input>` surfaces Tab like any other key, so `dat0-ui`'s cell editor commits and steps one column right on Tab, one left on Shift-Tab, clamped at the row's ends. | closed | low |
| PD-021 | P4c (T11 review): `error_ux::push` enqueues success/error banners into the global `PENDING` queue, but NOTHING drains it in the runtime render tree — only `#[cfg(test)]` code calls `drain_pending`. So export completion/failure feedback (`window.rs::run_export`) AND the pre-existing P4b paste-reject banner (`grid/edit_ops.rs`) are invisible to the user at runtime. **Closed by P6a T1:** `WorkspaceShell::render` now calls `error_ux::banner::merge_pending` into a per-window `banners` field and renders a host strip atop the shell. | closed | medium |
| PD-022 | P6a (T12 review): the Inspector profile is refreshed on forward data/schema mutations (cell edit, paste, cut, delete, rename, reorder, transform-apply via `route_change`), but NOT on `undo`/`redo` or SQL-console grid-bind — those rebind via `apply_view_change`, which has no inspector hook. So undoing an edit (or rebinding a grid from the SQL console) leaves the inspector profile stale until the next forward mutation. Single well-scoped fix: hook `apply_view_change` (or an `on_rebind_complete` seam) to invalidate/re-profile the inspected table. Not a regression — the inspector did not refresh at all before P6a. | closed | low |

## At-a-glance — Closed plan defects

| ID     | Title                                                              | Status | Severity |
|--------|--------------------------------------------------------------------|--------|----------|
| PD-005 | P2 plan T3 snippet uses `&*conn` triggering clippy explicit-auto-deref | closed | trivial |
| PD-006 | P2 plan T12 fixtures snippet uses bare `rng.gen::<…>()` — reserved keyword in Rust 2024 | closed | low      |
| PD-007 | P2 plan T14 snippet calls non-existent `error_ux::banner::push_banner` with mismatched `Banner` shape | closed | low      |
| PD-008 | P3a plan T2 snippets use wrong import paths for `fs4::FileExt` and `interprocess::local_socket::traits::Stream` | closed | trivial  |
| PD-009 | P3a plan T6 test snippet calls non-existent `engine.catalog().get_tables()` — no `catalog()` method on `DuckDBEngine` | closed | trivial |
| PD-010 | P3a plan T12 UDS → GPUI cross-thread window-open bridge is unsafe: `AsyncApp::update` borrows a `RefCell`-backed app cell, not safe to call from a tokio background thread | closed | low |

---

## Deferrals

### D-001 — Editable Settings widgets (author identity + theme dropdown)

- **Status:** closed
- **Deferred from:** P1 (T7 schema, T16 settings UI)
- **Target phase:** P3
- **Reason:** Depends on form-input primitives (text input, select dropdown) from
  gpui-component that aren't a P1 concern.
- **What P1 ships:** TOML schema, atomic-write store, file watcher, sections
  registry, read-only display of current settings.
- **What target phase delivers:** Editable inputs for author-name + author-email
  on the Profile section; selectable theme dropdown on the Theme section. Both
  bound to the existing `SettingsStore`.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Closes:** spec §21.2 P1 exit — "Settings panel opens; changes persist across launches" (full editability)
- **Last touched:** 2026-05-25
- **Closed by:** T11 (`crates/dat0-app/src/settings_ui/sections/{profile,theme}.rs`
  + `crates/dat0-app/src/settings/store.rs`); P3b plan T11. The plan-verbatim
  KV facade (`SettingsStore::open_in_memory`, `set`, `get_string`) lands on
  top of the P1 TOML store — `author.name`, `author.email`, `theme.id`
  round-trip via the same atomic-write path P1 already uses, and the on-change
  closures (`ProfileSection::on_name_change` / `on_email_change`,
  `ThemeSection::on_theme_change`) carry the live wiring shape. The visible
  view stayed stubbed because the gpui-component `Input` + `Select` mount
  rides on the T13 follow-up that opens the real settings window
  (see T0 spike §3 + §3.6 in `docs/internal/gpui-component-api-notes.md`);
  SettingsStore round-trip is live + tested (7 tests in
  `crates/dat0-app/tests/settings_ui.rs`). T12 reads `theme.id` for the
  `Theme::switch` fan-out via the same facade.

### D-002 — Theme live-switch through running window

- **Status:** closed
- **Deferred from:** P1 (T10)
- **Target phase:** P3
- **Reason:** Live theme application requires the running UI surfaces to consume
  the active theme through a reactive channel. P1 has too few theme-driven
  surfaces (one window with an empty view) for live-apply to be observable.
- **What P1 ships:** Zed-JSON theme schema, three built-in themes
  (`dark`/`light`/`high-contrast`), `Theme::load_builtin`, selection persistence.
- **What target phase delivers:** Theme observable channel; running window
  re-renders on theme change without restart.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Closes:** spec §21.2 P1 exit — "Theme switch (default ↔ alternate) works without restart"
- **Last touched:** 2026-05-25
- **Closed by:** T12 (`crates/dat0-app/src/theme/mod.rs` Theme::install + Theme::switch + observe_global audit); P3b plan T12. cx.set_global + cx.observe_global propagation; Theme dropdown change in Settings updates the global; subscribed views re-render in the same tick. Cross-window propagation automatic via app-scoped global.

### D-003 — Sparkle Objective-C `SUUpdater` bridge

- **Status:** closed
- **Deferred from:** P1 (T15)
- **Target phase:** P10a-2
- **Reason:** Cross-language Objective-C bridge needs notarized .app bundle +
  release pipeline before it's testable end-to-end. P1 doesn't sign or notarize.
- **What P1 ships:** HTTP GET smoke against the appcast URL (satisfies the
  exit criterion "makes a network call"); EdDSA public key bundled at build
  time via `include_str!`; `objc`/`cocoa` deps deliberately deferred.
- **What target phase delivers:** Real `SUUpdater` instantiation, appcast
  parsing, signature verification, in-app update prompt UI, restart flow.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Closes:** spec §21.2 P10 exit — "Sparkle 'Check for Updates' finds a test update + applies it"
- **Note (2026-06-21):** P10a ships the signed pipeline + a minimal update
  *nudge* (About box → open Releases); the in-app Sparkle bridge moves to the
  P10a-2 updater slice, gated on the Sparkle↔GPUI run-loop spike.
- **Note (2026-06-22):** Closed — superseded by P10a-2's unified Rust updater
  (minisign-signed `latest.json` + cross-platform self-swap); the Sparkle
  `SUUpdater` scaffolding was retired in T5. Merged in P10a-2.
- **Last touched:** 2026-06-22

### D-004 — AppImageUpdate subprocess invocation

- **Status:** closed
- **Deferred from:** P1 (T15)
- **Target phase:** P10a-2
- **Reason:** Real subprocess invocation of `appimageupdatetool` requires a
  packaged AppImage; P1 builds plain Linux binaries.
- **What P1 ships:** `AppImageUpdater` stub conforming to the `Updater` trait;
  logs a placeholder message.
- **What target phase delivers:** Subprocess wiring, progress reporting,
  restart flow.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Note (2026-06-21):** P10a ships the signed pipeline + a minimal update
  *nudge* (About box → open Releases); the AppImageUpdate subprocess invocation
  moves to the P10a-2 updater slice, gated on the Sparkle↔GPUI run-loop spike.
- **Note (2026-06-22):** Closed — superseded by P10a-2's unified Rust updater
  (minisign-signed `latest.json` + cross-platform self-swap); the AppImageUpdate
  subprocess scaffolding was retired in T5. Merged in P10a-2.
- **Last touched:** 2026-06-22

### D-005 — Linux Secret Service "setup banner" UX

- **Status:** open
- **Deferred from:** P1 (T12)
- **Target phase:** TBD (likely P10 hardening)
- **Reason:** Banner UI ships when a feature first instantiates `Banner`. P1
  primitives exist (T17) but no feature consumes them yet.
- **What P1 ships:** Documented error path when Secret Service isn't running on
  Linux; integration tests gate cleanly via `#[cfg]` selection.
- **What target phase delivers:** Visible Banner shown on app start when keychain
  init fails, with link to user-runnable docs for `gnome-keyring-daemon` /
  `kwalletmanager` setup.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Last touched:** 2026-04-28

### D-006 — macOS x86_64 (Intel) CI matrix coverage

- **Status:** open
- **Deferred from:** P1 (T20)
- **Target phase:** TBD — gated on external CI capacity (self-hosted runner,
  paid runner pool, or GitHub queue recovery for `macos-13` images)
- **Reason:** GitHub-hosted Intel-Mac (`macos-13`) runners are heavily
  oversubscribed since Apple's transition to Apple Silicon. PR #1's
  `macos-13` job sat queued for 50+ minutes without ever starting; this
  is reportedly typical for the image. Holding P1's PR merge on its
  scheduling is not productive.
- **What P1 ships:** `macos-14` (Apple Silicon arm64) build + test in
  CI; `linux-x86_64` and `linux-arm64` matrix coverage. Local development
  still builds for any installed target. The Cargo workspace is
  cross-architecture-clean; `macos-13` would be a re-confirmation, not
  a new validation surface.
- **What target phase delivers:** Restore the `macos-13` matrix entry
  when one of: a self-hosted Intel-Mac runner is provisioned, GitHub's
  hosted queue stabilizes for the image, or the team migrates the matrix
  to a paid runner provider. Suggested phase: P10 hardening, since that's
  also when notarization and signing infrastructure lands.
- **Originating doc:** PR #1 first-run timeout; `.github/workflows/ci.yml`
  matrix definition.
- **Closes (partial):** spec §21.2 P1 exit — "Cold-launches on macOS arm64,
  macOS x86_64, Linux x86_64, Linux aarch64" — Apple Silicon + both Linux
  triples covered; macOS Intel coverage deferred.
- **Note (2026-06-21):** P10a's universal macOS build adds the
  `x86_64-apple-darwin` build leg (lipo'd into the DMG); the full Intel *test*
  matrix (running CI on actual Intel hardware) remains open.
- **Last touched:** 2026-06-21

### D-007 — MotherDuck ATTACH end-to-end

- **Status:** closed
- **Deferred from:** P2
- **Target phase:** P5c (credential-gated MotherDuck slice)
- **P5 split (2026-06-02):** P5 (SQL Console) was split three ways —
  **P5a** = editor + run + cancel + multi-tab (D-008); **P5b** = completion +
  command palette; **P5c** = the credential-gated MotherDuck slice. D-007
  retargets from P5 → **P5c** because the `md:` ATTACH path needs a provisioned
  MotherDuck dev credential and a `MotherDuckTokenStore` consumer of the P1
  keychain primitive — neither is in P5a/P5b scope.
- **Reason:** The `motherduck` Cargo feature does not exist in duckdb-rs as of
  the P2.T0 spike (verified 2026-04-27 against duckdb-rs `v1.4.4` —
  `crates/duckdb/Cargo.toml` feature graph contains no `motherduck` entry).
  The keychain primitive shipped in P1 has no consumer until a UI surface
  needs the token. Integration testing requires a MotherDuck dev DB credential
  not yet provisioned. The P2 spec exit names only `sqlite:` for end-to-end
  ATTACH coverage.
- **What P2 ships:** generic `attach()` method that parses the DSN prefix;
  `sqlite:` end-to-end (extension lazy-loaded via boot-path
  `INSTALL sqlite_scanner; LOAD sqlite_scanner;`); `md:` returns
  `EngineError::NotImplemented { feature: "MotherDuck" }`.
- **What target phase delivers:** motherduck extension load (boot-path,
  same template as sqlite_scanner); a `MotherDuckTokenStore` consumer of the
  P1 keychain primitive; integration tests against a MotherDuck dev DB;
  per-query timing chip ("local: 38ms / md: 412ms") in the P5 SQL Console
  status bar.
- **Originating doc:** `docs/specs/2026-04-27-dat0-p2-engine-design.md` §7
- **Closes:** spec §6.5 entirely (partial closure — `sqlite:` lands in P2;
  `md:` lands in P5c).
- **Closed by:** P5c — PR #12, squash `6d406e6` (merged 2026-06-06).
  P5c delivered: runtime `INSTALL/LOAD motherduck` on duckdb-rs 1.4.4
  (S1 spike confirmed it works), the `attach()` md arm (`LOAD motherduck` on
  live conn + `SET motherduck_token` + `ATTACH 'md:'`),
  `EngineError::{MotherDuckAuth, ExtensionLoad}`, redacted token in
  `AttachOpts`, keychain token store (`TokenStore`), `ConnectionManager`,
  Connections panel + token prompt UI, session-v7 `PersistedAttachment`
  persistence + boot auto-reconnect, routing classifier, routing-tagged timing
  chip (`· local` / `· md` / `· mixed`), and CI-required `MOTHERDUCK_TOKEN`
  integration test job. Catalog enumeration (database names via
  `duckdb_databases()`) is implemented. One trim applied: the general
  "Attach SQLite…" panel add-flow is a **no-op stub** (trim-valve ②) — no
  reusable native file-picker exists in this codebase; files attach only via
  drag-and-drop. Engine `attach()`/`detach()` + the Detach button are fully
  implemented; only the panel "pick a file" UI entry point is deferred.
- **CI-discovered design correction (the design assumed an alias that does not
  exist):** MotherDuck rejects `ATTACH 'md:' AS <alias>` ("Database aliases are
  not yet supported by MotherDuck in workspace mode"), so the design's single
  `md` alias model was invalid. Corrected to **workspace mode** — `ATTACH 'md:'`
  attaches the account's databases under their real names (identified by
  `duckdb_databases().type = 'motherduck'`); the routing classifier keys on
  those real names. Also: workspace-mode `DETACH` **persists** to the account's
  saved workspace, so dat0's Disconnect is a **soft disconnect** (UI/state only,
  no `DETACH`) to avoid mutating the user's cloud workspace; Connect is
  idempotent. The integration test runs green against a live account on both
  macOS + linux in CI. (macOS PR CI skips the advisory grid bench — its release
  recompile of DuckDB exhausted runner disk.)
- **Last touched:** 2026-06-06 (closed by P5c, PR #12 `6d406e6`).

### D-008 — Cancellation-token wiring through `QueryEngine` trait

- **Status:** closed — functionally delivered token-free by P5a (2026-06-02).
- **Deferred from:** P2
- **Target phase:** P5a (SQL Console — editor + run + cancel + multi-tab)
- **What P5a delivers (2026-06-02):** Query cancellation for the SQL Console,
  wired **token-free**. The original "add `cancel: CancellationToken` param on
  every `execute*` trait method" path (see "What target phase delivers" below)
  was **rejected against the real call-site**: each window owns a single
  `Mutex`'d DuckDB connection, and at most one query runs against it at a time,
  so the engine's connection-wide `Engine::interrupt()` (backed by
  `Arc<duckdb::InterruptHandle>`, already shipped in P2) is exact — there is no
  ambiguity about *which* query a token would cancel. P5a therefore ships a
  `QueryCancel` RAII drop-guard that calls `engine.interrupt()` on drop (so
  `Cmd+.`, the Cancel button, or dropping the in-flight task all interrupt the
  live query) with **no `QueryEngine` trait amendment** — the trait shape is
  unchanged. P5a also ships, alongside the cancel path, the SQL highlight
  (runtime-registered `tree-sitter-sequel`), multi-tab persistence (session v5),
  and run-statement-under-cursor — but cancellation is the D-008 outcome proper.
  Keybinds: `Cmd+Enter` run / `Cmd+.` cancel / `Cmd+Shift+C` toggle console.
- **Closed by:** P5a, token-free, as described above. The deferral's remaining
  ambition — the `cancel: CancellationToken` trait amendment under "What target
  phase delivers" below — is **superseded**, not owed. Production-v1 slice EN2
  (lane-scoped cancellation) replaces it with a better-shaped mechanism: a
  `QueryToken` + `QueryLane` pair held on `DuckDBEngine` itself, with
  `begin_query` / `end_query` / `interrupt_scoped` / `interrupt_lane`. That puts
  the scoping where the single per-connection `duckdb::InterruptHandle` actually
  lives, instead of threading a token parameter through every `execute*`
  signature. Anything still wanted from the original token design is tracked as
  EN2 work, so D-008 carries no open remainder.
- **Reason:** P2 has zero callers passing tokens — adding a
  `cancel: CancellationToken` parameter on every `execute*` trait method now
  would ship dead-weight ergonomics that can't be evaluated against a real
  call-site. P5 SQL Console is the first surface that needs `Cmd+.` → cancel
  propagation through a streaming query; trait shape is best evaluated against
  that real call-site rather than guessed twice. Engine internals already
  support cancellation today via `Engine::interrupt()` (backed by
  `Arc<duckdb::InterruptHandle>`, verified at P2.T0) — the trait amendment is
  signature ergonomics, not a behavioral change.
- **What P2 ships:** internal `Arc<duckdb::InterruptHandle>` field on
  `DuckDBEngine`; public `Engine::interrupt(&self)` method callable from
  sibling tasks; `EngineError::Interrupted` variant present in the error enum.
- **What target phase delivers:** trait amendment to add
  `cancel: CancellationToken` parameter on `execute` / `execute_paged` /
  `execute_streaming`; automatic interrupt-on-drop semantics for cancellation
  tokens; structured cancellation propagation through the streaming `mpsc`
  channel; Cmd+. UX wiring in the P5 SQL Console.
- **Originating doc:** `docs/specs/2026-04-27-dat0-p2-engine-design.md` §7
  + §2.2.
- **Last touched:** 2026-08-08 (status → closed; P5a delivered the token-free
  cancel path, and EN2's lane-scoped cancellation supersedes the trait
  amendment).

### D-009 — Bundle `sqlite_scanner` static when duckdb-rs exposes a feature

- **Status:** open
- **Deferred from:** P2 (opens unconditionally with the spec; duckdb-rs's
  feature surface determines whether it ever closes)
- **Target phase:** TBD — closes when duckdb-rs adds a `sqlite_scanner`
  Cargo feature for static linking, OR stays open as documented intent if
  upstream never does. The constraint is real: the duckdb-rs README states
  the ICU extension isn't bundled "due to crates.io's 10MB package size
  limit," and `sqlite_scanner` is in the same distribution category.
- **Reason:** Opens unconditionally so we don't lose the intent if upstream
  adds the feature later. The "why" is the crates.io 10MB package size limit
  cited in the duckdb-rs README — the same constraint that keeps the ICU
  extension out of the bundled distribution applies to `sqlite_scanner`. As
  of P2.T0 verification (2026-04-27, duckdb-rs `v1.4.4`), the published
  feature surface contains no `sqlite_scanner` entry — confirmed by
  enumerating `crates/duckdb/Cargo.toml`.
- **What P2 ships:** lazy-load via the `dat0-app` boot path —
  `INSTALL sqlite_scanner; LOAD sqlite_scanner;` executed once before any
  window opens (per spec §2.5). First-run UX uses the P1 `Banner` primitive
  to show extension-download status. Engine `init()` per instance does
  `LOAD sqlite_scanner;` only (extension already installed by boot path).
- **What target phase delivers:** replaces boot-path `INSTALL` with a
  build-time static link, eliminating the first-run extension-download UX
  surface entirely. Boot path simplifies to `LOAD sqlite_scanner;` against
  the statically-linked extension.
- **Re-check trigger for closing:** duckdb-rs release notes mentioning a new
  feature flag for sqlite_scanner, OR the feature-graph row in
  `docs/internal/duckdb-arrow-api-notes.md` § "Extension features" gaining
  a positive entry on monthly upstream-watch refresh.
- **Originating doc:** `docs/specs/2026-04-27-dat0-p2-engine-design.md` §7
  + §2.5; `docs/internal/duckdb-arrow-api-notes.md` § "Extension features".
- **Note (P2 retro):** P2.T0 re-verified 2026-04-27: duckdb-rs 1.4.4 has no
  `sqlite_scanner` Cargo feature. Lazy-load remains the locked path.
- **Last touched:** 2026-04-28

### D-010 — Non-UTF-8 file encoding handling

- **Status:** open
- **Deferred from:** P2
- **Target phase:** TBD (likely P3 import wizard or v1.x polish)
- **Reason:** DuckDB's `read_csv` has no encoding parameter; supporting
  non-UTF-8 cleanly requires either Rust-side preconversion or a separate
  read path. Neither is a P2 concern — the engine's job is to expose what
  DuckDB supports natively. UI-side detection / conversion belongs with the
  P3 import wizard surface where the user can confirm the encoding choice.
- **What P2 ships:** `RegisterOpts` has no `encoding` field. CSV / TSV / JSON
  / NDJSON files are assumed UTF-8 (matches DuckDB's `read_csv` default).
  Non-UTF-8 input fails at parse time with a DuckDB error surfaced through
  `EngineError`.
- **What target phase delivers:** either (a) Rust-side preconversion via
  `encoding_rs` for CSV inputs flagged as non-UTF-8 by `chardet` /
  heuristic, with a banner ("We detected this file is encoded as <X>; do
  you want to convert?"), or (b) explicit user override in the import wizard
  with a conversion preview. P3 picks one or escalates to v1.x.
- **Originating doc:** `docs/specs/2026-04-27-dat0-p2-engine-design.md` §7.
- **Last touched:** 2026-04-28

### D-011 — Remove `__debug_query_scalar` test-only helper

- **Status:** CLOSED — 2026-05-16 (P3a T5)
- **Deferred from:** P2 (T2 fix-up; commit `f7ed3a7`)
- **Target phase:** P3a
- **Reason:** During P2.T2, bootstrap tests needed scalar-query access before
  T7's `execute()` shipped. The test-only helper `__debug_query_scalar` filled
  the gap and was marked `#[deprecated(note = "test-only; will be replaced by
  execute() in T7")]` in T2's review fix-up. T7 then shipped `execute()`, but
  ripping the helper out of 8 test files was deferred to avoid scope creep
  inside P2 (the deprecation attribute + per-file `#![allow(deprecated)]`
  prevent the helper from leaking into production callers in the meantime).
- **What P2 shipped:** `pub async fn __debug_query_scalar` on `DuckDBEngine` with
  `#[doc(hidden)]` and `#[deprecated]` attributes; file-level
  `#![allow(deprecated)]` in 8 test files (`crates/dat0-engine/tests/`:
  `bootstrap.rs`, `migrations.rs`, `register_csv.rs`, `register_json.rs`,
  `register_parquet.rs`, `attach_sqlite.rs`, `multi_window.rs`,
  `exit_criteria.rs`).
- **What P3a T5 delivered:** Removed `__debug_query_scalar` impl; test callers
  rewritten to use `execute_paged`; CI grep gate added in T20. All 8 test files
  had their `#![allow(deprecated)]` attributes removed; each file received a
  local `async fn scalar(engine, sql) -> String` helper using `engine.execute()`
  + Arrow `StringArray` downcast. Behavior identical — same assertions now
  exercised via the public API.
- **Originating doc:** `docs/plans/2026-04-27-dat0-p2-engine-plan.md` (T2
  fix-up commit `f7ed3a7`); P2 retro `docs/plans/2026-04-27-dat0-p2-retro.md`
  § "Recommendations for P3" #3.
- **Last touched:** 2026-05-16

### D-012 — Engine catalog `TableInfo` synthesis (origin + schema)

- **Status:** CLOSED — 2026-05-16 (P3a T6+T7)
- **Deferred from:** P2 (T9 catalog ops)
- **Target phase:** P3a (Scratch mode + DataGrid)
- **Reason:** DuckDB doesn't persist `TableOrigin` metadata across reconnects,
  and the engine had no per-table origin registry. P2's `get_tables`
  synthesized `TableOrigin::Derived(DerivedOrigin::Sql(""))` as a placeholder
  for every table — a false positive that future code could misinterpret.
  Similarly, `create_table` returned `TableInfo { schema: "main", ... }`
  hardcoded regardless of where the table actually landed.
- **What P2 shipped:** `TableInfo.origin` and `TableInfo.schema` populated with
  best-effort placeholders. `register_file` produced the correct
  `TableOrigin::File(PathBuf)`. All other paths returned `Derived(Sql(""))`
  or hardcoded `"main"`.
- **What P3a T6+T7 delivered:** per-engine in-memory origin registry
  (`DuckDBEngine.table_origins: Arc<RwLock<HashMap<String, TableOrigin>>>`)
  populated on `register_file` (→ `File(path)`) and `create_table`
  (→ `Derived(Sql(sql))`). `get_tables` (T7) joins `information_schema`
  rows against the map; untracked tables fall through to the existing
  `Derived(Sql(""))` placeholder. `create_table` resolves `schema` via
  `information_schema.tables` rather than hardcoding `"main"`.
- **P4 remainder (attach):** `attach` does not enumerate the tables inside the
  attached database, so `TableOrigin::Attached { alias, source }` entries are
  not recorded per table. Per-table attach origin tracking is deferred to P4.
  See TODO comment in `duckdb_engine.rs` `attach` impl.
- **Closed by:** P3a T6 commit `324d89f`; T7 commit on branch `p3a-hot-path`
  (2026-05-16). Tests: `register_file_origin_is_file` + `create_table_returns_real_schema`
  in `crates/dat0-engine/tests/catalog_origin.rs` both pass.
- **Originating doc:** `docs/plans/2026-04-27-dat0-p2-engine-plan.md` T9 +
  T9 review notes; `docs/plans/2026-04-27-dat0-p2-retro.md` § "Reviewer-
  flagged minor follow-ups".
- **P5c note (2026-06-05):** The Connections panel's catalog enumeration is
  **database-names-only** (`duckdb_databases()`). Per-table `TableOrigin::Attached`
  origins for attached databases remain deferred — the engine `attach()` still
  does not enumerate attached tables or record them in the origin registry.
- **P6a delivery (2026-06-06) — the attach remainder is now DONE.** P6a T4
  implemented the deferred per-table attach enumeration: `catalog::list_attached_tables`
  enumerates an attached catalog's tables/views (`duckdb_tables()`/`duckdb_views()`
  filtered by `database_name`), and `attach()` writes a `TableOrigin::Attached {
  alias, source }` per object into `table_origins` after a successful ATTACH (both
  the sqlite/file branch and the MotherDuck workspace branch, keyed by real db
  name); `detach()` prunes by alias. Enumeration is best-effort (a hiccup never
  fails an already-successful ATTACH — see `refactor(p6a): attach origin-enumeration
  best-effort`). This also fixed a latent bug: `get_tables` used an unqualified
  `DESCRIBE "schema"."table"` that errored for any attached table, so `get_tables`
  failed whenever a database was attached — now qualified 3-part
  (`"db"."schema"."table"`). Commits `ff9926f` + `0d0b169`; test
  `attach_records_per_table_attached_origin` in `tests/catalog_origin.rs`. The
  P4/P5c attach remainder of D-012 is closed.
- **Last touched:** 2026-06-06

### D-013 — Self-hosted macOS CI runner (cut hosted macos-14 10× billing)

- **Status:** closed — superseded (2026-08-08, MX3)
- **Deferred from:** P2 (PR #3 — `ci(p2)` split + self-hosted Linux routing)
- **Target phase:** TBD — gated on hardware purchase (dedicated Mac mini).
- **Reason:** PR #3 routed the `linux-x86_64` matrix entry to a runnerkit
  self-hosted runner, dropping that job's billing to 0×. The `macos-14`
  matrix entry remains on GitHub-hosted runners at the 10× billing
  multiplier, which is the dominant remaining contributor to the org-level
  Actions cap. Self-hosting macOS requires Apple hardware (EULA: macOS
  guests may only run on Apple-branded hosts), so the cost reduction is
  blocked on owning or renting a Mac. Running CI on the active dev Mac
  was rejected because background CI fights interactive dev work for
  CPU/RAM and creates dependency on the machine staying awake.
- **What P2 / PR #3 ships:** Linux x86_64 build + test on runnerkit
  self-hosted (`[self-hosted, Linux, runnerkit]`); heavy exit_criteria
  suite on the same runner via `heavy.yml` (schedule + `run-heavy`
  label). `macos-arm64` stays on `macos-14` hosted. Per-PR Actions
  minute usage roughly halved.
- **What target phase delivers:** A dedicated Apple Silicon Mac mini
  running Tart-managed ephemeral macOS VMs as GitHub Actions runners
  (label set `[self-hosted, macos, runnerkit]` or similar). Route the
  `ci.yml` `macos-14` matrix entry to those labels. Validate Metal
  Toolchain, Xcode CLT, rustup, and the keychain/Secret Service test
  paths inside the VM image. Expected outcome: macOS CI minutes → 0×;
  cargo cache + Xcode cache survive between runs (faster feedback).
- **Originating doc:** PR #3 conversation; `.github/workflows/ci.yml`
  matrix definition (`macos-14` entry).
- **Closes (partial):** the org Actions cap remediation chain started
  by the SQLite fixture + heavy-test split. Linux side complete;
  macOS side remains.
- **Note (2026-06-21):** still open; → P10c.
- **Note (2026-06-25):** P10c left this deferred — no Mac mini available; the
  GlitchTip/crash-reporting half of P10 shipped, but the perf-gate runner stays
  gated on hardware.
- **Note (2026-08-08, MX3) — SUPERSEDED, closing.** The premise was that
  enforcing a perf budget required dedicated Apple hardware. MX2 removed it:
  `cargo xtask perf` measures a real 1440×900 window on ANY host and gates on
  two independent things — the absolute budgets in
  `docs/internal/perf-baselines.json`, and a per-host recorded baseline keyed by
  `os-arch-$DAT0_PERF_HOST` so a virtualized runner is never compared against a
  dev box. The budget is therefore enforced today, on the release host, as a
  numbered step in `docs/release-runbook.md`. Dedicated hardware would only
  change WHICH runner executes it, which is a cost question, not a correctness
  one. The cost half is re-filed as D-032; the billing rationale above is
  preserved there rather than kept open under a title that now overstates what
  is blocked.
- **Last touched:** 2026-08-08

### D-014 — Memory Budget Settings section

- **Status:** closed
- **Deferred from:** P3b (T11 scope decision)
- **Target phase:** P3c (if split) or P9c (settings polish)
- **Reason:** P3b T11 scope locks to D-001 wording (Profile + Theme widgets).
  Memory Budget requires engine plumbing to re-apply `memory_limit` PRAGMA on
  change, not in P3b ad-hoc scope. Settings store already persists the value;
  the missing piece is the engine-side reapply path + a UI surface that does
  not silently mislead users into thinking a budget change took effect when
  it actually only applies on next window open.
- **What P3b ships:** Profile + Theme editable sections (D-001 closed by T11).
  Memory Budget remains read-only display or absent from the Settings panel
  surface area depending on which sections the registry exposes.
- **What target phase delivers:** Slider/number input for `memory_limit`;
  engine reapplies on change (`PRAGMA memory_limit = 'NMB'` against the live
  connection per window), or — if reapply-on-live-connection turns out to
  carry mid-query risk — a footnote "applies next window" tied to the
  control with a Restart hint.
- **Closure note (2026-06-23):** Closed by P10b — editable Memory Budget
  section (`memory_budget_mb`) + read-once `memory_budget_bytes` helper at all
  window-open sites; applies to new windows (live-reapply on a running
  connection remains a v1.x note).
- **Originating doc:** `docs/specs/2026-05-25-dat0-p3b-ux-polish-design.md` §7.
- **Last touched:** 2026-06-23.

---

### D-015 — AccessKit / screen-reader selection-tree exposure

- **Status:** closed — 2026-08-10, by Phase 7 of the GPUI→Dioxus migration.
  The deferral was a statement about GPUI: AccessKit was entirely absent from
  the pinned 0.2.2, so the only accessibility dat0 could produce was a
  test-only `TreeUpdate` that `kittest` read and no screen reader ever saw.
  A WebView has an accessibility tree by construction. `dat0-ui` emits real
  ARIA in release builds — `role` from `a11y::AccessRole`, names from
  `aria-label`, tab participation from `a11y::TabStop` — and the platform
  adapter is the one the OS already ships for WebKit and WebKit2GTK. The
  `a11y-capture` cargo feature, the hand-built `TreeUpdate`, `kittest`,
  `accesskit` and `accesskit_consumer` are all gone from the tree.
- **Was:** open
- **Deferred from:** P4b (T0 probe finding; design decision 5)
- **Target phase:** P10b
- **Reason:** The P4b T0 probe (`docs/internal/dat0-p4b-t0-probe.md` §2) verified
  that AccessKit is **entirely absent** from the pinned GPUI 0.2.2 and
  gpui-component 0.5.1 — no `accesskit` dependency, no `AccessibilityNode`, no
  adapter. A screen-reader-navigable selection tree is therefore not exposable
  on these pins without forking GPUI.
- **What P4b ships:** the **operability-only** a11y baseline (design decision 5)
  — full keyboard navigation of every selection variant (arrows, shift-extend,
  cmd-jump, select-all/row/column, escape) as pure input handling via
  `grid::keymap` + `SelectionModel`, with a visible focus ring. No AccessKit
  dependency.
- **What target phase delivers:** a screen-reader selection/cell semantics tree,
  contingent on a GPUI version that ships an AccessKit adapter (or a dat0-side
  fork). Revisit when the GPUI pin advances.
- **Originating doc:** `docs/internal/dat0-p4b-t0-probe.md` §2.
- **P4c re-scan (2026-06-01):** confirmed still open + still targeted at P10; no
  AccessKit adapter on the pinned GPUI/gpui-component. P4c T13 added header-click
  → select-column (operability), not a screen-reader semantics tree.
- **P5c note (2026-06-05):** The Connections panel (attachment list, connect/disconnect
  controls) and token-prompt modal are additional AccessKit / screen-reader surfaces
  to cover alongside the selection tree (still deferred).
- **Note (2026-06-21):** still open; → P10b.
- **Re-checked P10b (2026-06-23):** `accesskit` still absent from `Cargo.lock`, gpui pinned `=0.2.2`. Stays open; v1 a11y = operability + AA contrast (see `docs/a11y.md`); revisit when the GPUI pin ships an AccessKit adapter.
- **UAT Gap 2 note (2026-07-01):** `accesskit`/`accesskit_consumer` (0.21) now appear in `Cargo.lock`, but **only** via the **test-only** `a11y-capture` cargo feature (enabled through a self-dev-dependency; OFF in `cargo build --release`, where the `.a11y*` helpers compile to identity no-ops). This is the UAT-automation content-assertion harness (`crates/dat0-app/src/a11y/`, `tests/support/`, `tests/a11y_content.rs`): dat0's own render code emits an AccessKit `TreeUpdate` that `kittest` reads in tests. **D-015 STAYS OPEN** — there is still no OS platform adapter, no always-on production emission, and no gpui integration, so the running app gains no screen-reader support. The upside: the `.a11y(id, role, label)` / `.a11y_label(role, text)` render-site annotations added across onboarding/hero/grid/inspector/SQL/error surfaces are a reusable down-payment — if the gpui pin ever ships an AccessKit adapter, those semantic annotations can feed a real production tree.
- **Last touched:** 2026-07-01.

---

### D-018 — Workspace lineage DAG

- **Status:** open
- **Deferred from:** P6b (lineage-half scope split; user pick — option A)
- **Target phase:** —
- **What it is:** a whole-workspace lineage **DAG** — a node-edge graph with
  auto-layout (left→right topological ordering), pan/zoom — that renders the
  entire workspace's table/file/external lineage at once, versus the per-table
  Inspector lineage chain shipped in P6b.
- **Reason:** the P6 lineage work was split into two halves; the user picked
  **option A** (the Inspector lineage chain only) for P6b and dropped the
  workspace-DAG visualization, leaving it as a standalone deferral.
- **Substrate already shipped in P6b:** the Inspector lineage chain, the
  `json_serialize_sql`-based SQL-edge resolution (`engine::referenced_tables`),
  the pure `LineageGraph` (build + transitive closure) in
  `inspector/lineage.rs`, and click-to-open re-rooting. D-018 is purely the
  graph-rendering surface built on top of that existing substrate.
- **Originating doc:** `docs/plans/2026-06-06-dat0-p6b-design.md` (P6b design —
  lineage split).
- **Last touched:** 2026-06-06.

---

### D-019 — Workspace concurrency + sync-drive safety

- **Status:** closed — resolved by P7b (2026-06-11).
- **Resolved by:** P7b (`docs/plans/2026-06-11-dat0-p7b-design.md` + `…-p7b-plan.md`).
  Delivered: the cross-machine `.dat0/lock.json` holder record (acquire/tombstone
  state machine), the sync-drive heuristic + **Settings → Networked Workspaces**
  global toggle, the blocking `WorkspaceInUseModal` (gpui-component `Dialog`), and
  same-machine focus-existing (real per-window activation).
  **Design change from the original sketch:** P7b uses **NO heartbeat / no TTL**
  (design D1) — sync-drive lag makes TTL staleness a corruption risk — replacing
  the "heartbeat/lease" idea in (a) with a tombstone-on-clean-release model.
  **Force-unlock (e) was NOT implemented** and remains a v1.x item: dat0 warns on
  a live foreign holder and lets the user "Open anyway", but never auto-resolves;
  the rich-modal "Force unlock" button (c) is likewise v1.x.
- **Note:** D-021 (banner action buttons) remains **open** and is unrelated — the
  P7b modal uses a gpui-component `Dialog`, not the banner host.
- **Last touched:** 2026-06-11.
- **Deferred from:** P7a (workspace core; design non-goals)
- **Target phase:** P7b
- **What it is (historical):** The full cross-machine concurrency and sync-drive safety story:
  (a) A **heartbeat/lease `lock.json`** alongside the `fs4` flock — `fs4` exclusive locks
  are process-scoped and self-heal on exit, but don't cross machines (e.g. NFS, a
  sync-drive folder). A `lock.json` with a heartbeat timestamp and holder identity
  (hostname, pid, display-name) lets any opener detect a stale vs. live cross-machine
  lock and show the full holder context to the user.
  (b) **Sync-drive heuristic** — detect whether the workspace folder sits under a
  Dropbox / iCloud Drive / OneDrive / Google Drive path and warn the user that
  concurrent writes from sync are unsafe while dat0 has the workspace open.  A
  "Treat as networked storage" Settings override lets users suppress the warning for
  intentional shared drives.
  (c) A **rich `WorkspaceInUseModal`** that replaces P7a's minimal "in use" warning
  banner with a sheet showing the holder's identity, whether it appears to be on
  the same machine, and a "Force unlock (data may be lost)" button.
  (d) A **Settings → Workspace section** for the sync-drive override and any future
  workspace-level preferences.
  (e) A **force-unlock path** (v1.x hardening) for recovery after a crashed holder
  on a remote/sync mount leaves a stale cross-machine lock.
- **Why deferred:** P7a's `fs4` flock is sufficient for the primary single-machine
  use-case (workspaces on a local SSD). Cross-machine and sync-drive scenarios require
  additional ambient context (hostname, sync-daemon path inspection) and a richer UI
  surface that is disproportionate to the P7a scope. The flock already provides stale-lock
  self-healing for the common crash case (lock released on process exit).
- **Originating doc:** `docs/plans/2026-06-10-dat0-p7a-design.md` §"Non-goals / deferred to P7b".

### D-020 — Live-data refresh (file-watcher on Tab.source_path)

- **Status:** closed — resolved by P7c (2026-06-12).
- **Resolved by:** P7c — a per-window `SourceWatcher` on the active table's
  source file (debounced single-path `notify` watcher, re-targeted on table
  switch); on a detected change a "<file> changed on disk — [Refresh]" banner
  appears, and Refresh re-imports the file via `register_file_as_table` and
  replays the tab's transforms. The replay is **structural-only**:
  `dat0_engine::transform::split_replayable` keeps the column-keyed ops
  (filters / sorts / reorder / rename / hide) and drops the rowid-keyed ops
  (cell edits + row deletions, which reference `__dat0_rowid` surrogates a
  re-CTAS regenerates); if any are present the user gets a confirm dialog first.
  Replay is force-rebound onto the fresh base via
  `ViewModel::reset_to_replayed`. **Schema drift** (a surviving filter/sort
  references a now-missing column) lands the tab on the bare re-imported base
  with a warning banner (`livedata.replay.schema_drift`) — never silent
  corruption. The `recovery_panel` Sheet UI is finished too (orphaned sessions +
  interrupted workspaces, Open / Resume / Discard).
- **Originating doc:** `docs/plans/2026-06-10-dat0-p7a-design.md` §"Non-goals /
  deferred to P7c"; closed by `docs/plans/2026-06-12-dat0-p7c-plan.md` (T1–T9).
- **User-facing doc:** `docs/live-data-recovery.md`.
- **Follow-ups opened by P7c:** D-022 (live-view import mode), D-023
  (cross-table refresh cascade), D-024 (auto-refresh toggle + multi-table
  watching).
- **Last touched:** 2026-06-12.
- **What it is (historical):** A `notify`-crate file-watcher on each `Tab.source_path` (the
  original source file for imported tables) that re-imports the table when the
  source file changes: re-runs CTAS (same sniffing path as the original import),
  then replays the tab's transform stack on the new base. The re-import is
  debounced (e.g. 500 ms quiet window) to coalesce rapid editor saves.  If the
  tab has user edits (cell overlays from `compile_view_sql`), the user is warned
  that re-import will discard those base-table edits.  Also covers finishing the
  `recovery_panel` Sheet UI (the current recovery panel is a logic stub with a
  placeholder render).
- **Why deferred:** dat0 tables are MATERIALIZED at import time (CTAS, not views
  over the source file — see PD-017 resolution) so a file change does NOT
  automatically reflect in the open tab.  Wiring the watcher requires a persistent
  mapping from table name → original source path (already present in
  `TableOrigin::File(path)`) and a re-import pipeline that preserves the transform
  stack — non-trivial scope that belongs in a focused live-data phase once the
  workspace persistence substrate (P7a) is solid.
- **Originating doc:** `docs/plans/2026-06-10-dat0-p7a-design.md` §"Non-goals / deferred to P7c".
- **Last touched:** 2026-06-10.

### D-021 — Banner action buttons not rendered

- **Status:** closed — resolved by P7c (2026-06-12).
- **Resolved by:** P7c (T3) — `error_ux::render_banner` now renders a banner's
  `primary` / `secondary` actions as `gpui_component::button::Button`s (primary
  styled prominent, secondary ghost). Clicking a button dispatches the stored
  `action_id` through the global `actions::registry::ActionRegistry`. This is
  what makes the live-data "Refresh" banner and the recovery "Review" banner
  clickable; the P7a workspace "Save Workspace" prompt is now a real button too.
- **Originating doc:** P7a T12; `crates/dat0-app/src/error_ux/banner.rs`
  (`render_banner`, `Banner::with_primary` / `with_secondary`); closed by
  `docs/plans/2026-06-12-dat0-p7c-plan.md` T3.
- **Last touched:** 2026-06-12.
- **What it is (historical):** `error_ux::render_banner` (the per-banner chip painted by
  `WorkspaceShell::render`) renders the banner's `title` and `body` text only.
  The `Banner::with_primary(label, action_id)` method stores the action label and
  id on the `Banner` struct, but `render_banner` does not yet display a button
  for it.  As a result, P7a's workspace prompt (`workspace.prompt.title` /
  `workspace.prompt.save`) appears as a text-only nudge — the "Save Workspace"
  call-to-action label is stored but not rendered as a clickable button.
- **Why deferred:** The banner chip layout was introduced in P6a (PD-021 closure)
  as a minimal title+body strip. Adding a styled action button requires a
  `gpui-component` button primitive inside the chip and a dispatch path through
  `MainThreadDispatcher` to fire the stored `action_id`.  The workspace prompt is
  still useful as a text nudge; the full button UX is a polish item.
- **Originating doc:** P7a T12; `crates/dat0-app/src/error_ux/banner.rs`
  (`render_banner`, `Banner::with_primary`).
- **Last touched:** 2026-06-10.

### D-022 — Live-view import mode (auto-reflecting source files)

- **Status:** open
- **Deferred from:** P7c (design decision D1)
- **Target phase:** —
- **What it is:** An opt-in import mode that registers a CSV/Parquet source as a
  `read_csv` / `read_parquet` **VIEW** over the file on disk (instead of the
  materialized base table P7c re-imports), so the open tab auto-reflects external
  file changes with **no re-import and no manual Refresh**. Effectively a live
  query against the file rather than a snapshot.
- **Why deferred:** P7c (design D1) deliberately chose the **re-import** model —
  re-CTAS into a rowid-bearing base table + replay the transform stack — because
  dat0's edit/delete overlays, the `__dat0_rowid` surrogate, and profiling all
  assume a materialized base (see PD-017). A live `read_csv` VIEW has no
  `__dat0_rowid`, so edits/deletes don't apply and every scroll re-reads the file;
  it's a fundamentally different data model that trades editability + stable
  profiling for zero-latency reflection. Worth offering as an explicit per-table
  mode, but out of P7c's "make the existing materialized tables refresh safely"
  scope.
- **Originating doc:** `docs/plans/2026-06-12-dat0-p7c-design.md` (decision D1).
- **Last touched:** 2026-06-12.

### D-023 — Cross-table refresh cascade

- **Status:** open
- **Deferred from:** P7c (design decision D5)
- **Target phase:** —
- **What it is:** When a base table is refreshed (re-imported from a changed
  source), automatically re-materialize the tables **derived from it** — the P6b
  lineage dependency closure (`inspector::lineage::LineageGraph` /
  `engine::referenced_tables`) — in **topological order**, so downstream derived
  tables and saved-as-table results pick up the new source data in one pass.
- **Why deferred:** P7c refreshes only the **single watched table** the user is
  looking at. Cascading through the dependency closure means re-running each
  derived table's defining SQL in dependency order, handling partial-failure
  rollback, and deciding the UX for a multi-table refresh (which tabs update,
  what the user sees mid-cascade). The lineage substrate exists (P6b) but the
  cascade execution + UX is its own slice.
- **P8b note (2026-06-13) — the cascade *machinery* now exists.** P8b's
  `dat0_format::replay::ReplayEngine` already re-executes a recipe's derived
  tables in **topological order** against a (possibly refreshed) set of sources
  — exactly the "re-run each derivation in dependency order" engine this cascade
  needs. The remaining follow-up is purely the **in-app wiring**: on a watched
  base-table refresh, drive the P6b lineage closure through the same
  topological-re-exec algorithm (live engine, in place of `ReplayEngine`'s
  throwaway engine), plus the multi-table refresh UX. The hard part (correct
  topological re-execution of a derivation graph) is solved; track the in-app
  wiring as the remaining work here if still wanted.
- **Originating doc:** `docs/plans/2026-06-12-dat0-p7c-design.md` (decision D5).
- **Last touched:** 2026-06-13 (P8b — ReplayEngine provides the cascade machinery).

### D-024 — Per-table / global auto-refresh toggle (+ multi-table watching)

- **Status:** open
- **Deferred from:** P7c (design decision D6)
- **Target phase:** —
- **What it is:** A setting — per-table and/or a global default — to **auto-apply**
  a detected source-file change (skip the "<file> changed on disk — [Refresh]"
  prompt and re-import immediately on the debounced change), for users who want a
  source file to flow straight into the open tab. Confirm-on-discard would still
  fire when rowid-keyed edits/deletions are present.
- **Also covers — multi-table simultaneous watching:** P7c's `SourceWatcher`
  tracks only the **active** `view_model`'s source path and re-targets the watch
  whenever the user switches tables, so only the foreground table is watched at a
  time. Watching every open table's source concurrently (so a background table's
  refresh banner appears the moment its file changes, without first switching to
  it) is part of this deferral — it needs a multi-path watcher (or per-tab
  watchers) plus a per-table change-pending indicator.
- **Why deferred:** P7c (design D6) chose the **prompt-on-change, single active
  watcher** model as the safe default — the user always sees what will be
  discarded before a refresh runs, and one watcher keeps the `notify` wiring
  simple. Auto-apply + concurrent multi-table watching are an opt-in convenience
  layer on top.
- **Originating doc:** `docs/plans/2026-06-12-dat0-p7c-design.md` (decision D6);
  `crates/dat0-app/src/window.rs` (`SourceWatcher` retarget-on-table-switch).
- **Last touched:** 2026-06-12.

### D-025 — Derived-table provenance not persisted across workspace reopen

- **Status:** open
- **Deferred from:** P8 (T5 finding; empirically validated)
- **Target phase:** —
- **What it is:** Derived-table provenance — the SQL/transform that produces a
  table and the link to its parents — lives **only in memory** while a workspace
  is open. The engine's `table_origins` is an in-memory `HashMap` on
  `DuckDBEngine`; `TableOrigin` is **not** `Serialize`; and
  `Session::recover_workspace` opens a **fresh** engine with an **empty** origin
  map. Nothing on disk records which tables are derived.
- **Consequence:** `dat0 export <cold-workspace-dir>` (the CLI export path)
  reopens the workspace via `recover_workspace` before exporting, so the live
  origin map is gone and `get_tables()` classifies **every** table as `Base`.
  The exported package is therefore **data-only** — the derived recipe /
  replayable lineage is lost (`inspect` shows no lineage edges; `replay` has no
  derivations to re-run). The **in-app "Export Package"** (live session, via
  `session_to_contents` before any reopen) and the headless
  `Writer`-from-live-`session_to_contents` path DO preserve lineage — they
  export straight from the running session's populated origin map.
- **Why it's a deferral, not a bug:** within P8's scope the in-app/live export
  path is correct and lossless, and the cold-CLI round-trip is genuinely
  *self-consistent* (a table classifies identically — as `Base` — in both the
  exported and re-exported packages, so the round-trip diff is empty). Closing
  the gap requires persisting origins across reopen, a larger storage change.
- **Fix:** persist `table_origins` (e.g. into `session.json` or a `.dat0/`
  sidecar) and restore it on `recover_workspace`. `TableOrigin` would need a
  `Serialize`/`Deserialize` derive (or a portable on-disk shape).
- **Discovered + validated in:** P8 T5 — see the `FINDING (T5)` doc comment on
  `export_unpack_reexport_diff_is_empty` in
  `crates/dat0-app/tests/cli_roundtrip.rs`, and the live-session-vs-cold-export
  contrast in `crates/dat0-app/tests/package_e2e.rs`.
- **Originating doc:** `docs/plans/2026-06-13-dat0-p8-plan.md` (T5);
  `crates/dat0-app/src/package/mod.rs` (`classify`),
  `crates/dat0-engine/src/duckdb_engine.rs` (`table_origins`).
- **User-facing doc:** `docs/dat0-packages.md` § "Known limitation".
- **Last touched:** 2026-06-13.

### D-026 — Python (non-Rust) `.dat0` reader

- **Status:** open
- **Deferred from:** P8 (explicit non-goal)
- **Target phase:** —
- **What it is:** A non-Rust reader for `.dat0` packages — e.g. a Python library
  that opens a package, reads its Parquet data, and routes its metadata. The
  format is already **reader-ready** for this: data is plain Apache Parquet, and
  the metadata is self-describing tagged JSON where every polymorphic node
  carries an explicit `kind`/`tag` discriminator (`manifest.kind == "package"`,
  `derivation.kind ∈ {sql, transform}`, each `Transformation` op tagged on
  `kind`) — so a non-Rust reader can route purely on those discriminators with
  no Rust-side type knowledge. See [`docs/dat0-format-v1.md`](dat0-format-v1.md)
  §11.
- **Why deferred:** out of scope for P8, which delivers the format + the Rust
  reader/writer/replay/diff + the CLI. A second-language reader is future work
  once there's demand (a Python notebook integration, a data-pipeline consumer,
  etc.).
- **Originating doc:** `docs/dat0-format-v1.md` §11; P8 non-goals.
- **Last touched:** 2026-06-13.

### D-027 — In-app Inspect polish (P8 follow-ups)

- **Status:** open
- **Deferred from:** P8 (T9 — non-blocking GUI follow-ups)
- **Target phase:** —
- **What it is:** A cluster of non-blocking polish items on the read-only Inspect
  window shipped in P8 T9:
  - **No "read-only" badge** in the Inspect window chrome — the window refuses
    edits, but nothing in the title/header visibly signals that it's a read-only
    package view (the user discovers it only when an edit is refused).
  - **Inspect scratch dir not GC'd on close** — the Inspect window materializes
    the package under `<state>/inspect/<uuid>/` and keeps it for the window's
    lifetime, but the directory is **not** pruned when the window closes
    (candidate: prune on launch like the orphan-scratch scan does for sessions).
  - **GUI Replay binds the first source only** — the in-app Replay flow binds the
    picked file to the package's **first** source; multi-source replay stays on
    the `dat0 replay --source logical=path` CLI (which accepts repeated
    `--source`).
  - **No "Unpack to edit" button** in the read-only shell header — unpacking is
    reachable via **File → Unpack**, but there's no in-context shortcut in the
    Inspect window itself.
- **Why deferred:** P8 T9 delivered the functional read-only Inspect shell
  (browse + refuse edits); these are cosmetic/convenience refinements that don't
  block the verb. Each is small and independent.
- **Originating doc:** P8 T9; `crates/dat0-app/src/package/inspect.rs`.
- **User-facing doc:** `docs/dat0-packages.md` § "In the app".
- **Last touched:** 2026-06-13.

---

### D-028 — Privileged `/Applications` auto-update (SMJobBless / SMAppService helper)

- **Status:** open
- **Deferred from:** P10a-2 (design non-goal — user-writable-only scope)
- **Target phase:** v1.x
- **What it is:** When dat0 is installed in a location the running user cannot
  write to (e.g. `/Applications` owned by root), the P10a-2 updater's
  `is_writable` check returns false and the updater falls back to the P10a nudge
  (opens the GitHub Releases URL). A privileged updater helper would allow
  one-click in-app installs even into system-owned locations:
  - **macOS:** an `SMJobBless`- or `SMAppService`-registered privileged helper
    tool that requests user authentication (Touch ID / password prompt via
    Security framework), then performs the `.app` swap at the privileged path.
  - **Linux:** equivalent approach (e.g. a `pkexec`/PolicyKit wrapper or an
    `systemd` activation unit that can write to `/opt` or `/usr/local`).
- **Why deferred:** The P10a-2 updater targets the common user-level install
  case (dat0 dropped into `~/Applications` or equivalent, or installed via a
  user-writable path). Privileged helpers add significant OS-integration surface
  (code-signing requirements, entitlements, SMJobBless / SMAppService
  registration, PolicyKit `.policy` files) that is disproportionate to the P10a-2
  scope and requires additional testing on both platforms. The nudge fallback
  is a safe and complete substitute for v0 users.
- **Originating doc:** `docs/plans/2026-06-22-dat0-p10a-2-design.md` (non-goals:
  privileged `/Applications` update); `docs/plans/2026-06-22-dat0-p10a-2-uat.md`
  §3 (not-writable fallback UAT scenario).
- **Note (2026-06-22):** opened at P10a-2 merge. The not-writable fallback
  (`is_writable` → nudge) is exercised in the UAT checklist §3. Full SMJobBless /
  PolicyKit wiring is v1.x scope.
- **Last touched:** 2026-06-22

---

### D-029 — Settings panel persist-on-render → change-gate (+ P10b cleanup trio)

- **Status:** closed
- **Deferred from:** P10b (plan-sanctioned model; final whole-branch review flagged the cost)
- **Target phase:** P10c
- **What it is:** `SettingsPanel::render` (`crates/dat0-app/src/settings_ui/panel.rs`)
  calls `persist_inputs(cx)` on **every render tick**, unconditionally. Each call
  routes Profile name/email (and, on the memory_budget section, the budget value)
  through `store.set`/`set_memory_budget_mb` → `SettingsStore::save`, which does an
  atomic write with `f.sync_all()` + parent-dir fsync (PD-002). So a Profile render
  performs 2 fsync-ing atomic `settings.toml` writes per frame (3 on memory_budget),
  **even when nothing changed** (`set` does not diff). On a hot render loop (hover,
  cursor blink, any external `cx.notify()`) this is double-fsync-per-frame to the
  config dir, which can stall the UI thread on a slow/synced drive and races the
  `SettingsWatcher`. The plan explicitly sanctioned this ("cheap; values are short")
  and correctness is fine (last-write-wins, idempotent) — this is a perf smell, not
  a bug.
- **Fix when picked up:** change-gate the persistence — cache the last-persisted
  `String`s on the panel and only call the setter when the input value actually
  changed, or move persistence to the input `on_change`/blur hook instead of render.
- **Bundled P10b cleanups (all zero-risk subtractions, do together in P10c):**
  - Delete the orphaned `SettingsView` struct + its `Render` impl + the now-dead
    `SettingsSection::render()` trait method and its 9 placeholder impls
    (`settings_ui/mod.rs` + `sections/*`). `SettingsPanel` superseded them in P10b
    T4; `SettingsView` is instantiated nowhere and `section.render()` is never
    called. Both `pub`, so they compile without a dead-code warning today.
  - i18n the two hardcoded input placeholders `.placeholder("Name")`/`("Email")`
    in `panel.rs` (add `settings.profile.*_placeholder` keys).
  - Drop the orphan i18n key `settings.update.auto_check` (referenced only in a
    comment in `sections/updates.rs`; now duplicates `settings.updates.toggle`'s
    value) + the stale comment.
- **Process note (i18n-check):** the planning docs claim `scripts/i18n-check.sh`
  "fails on a missing (referenced-but-absent) key." It does NOT — it is a warn-only
  un-i18n'd-literal heuristic that `exit 0`s and does not resolve referenced keys
  against `en.json`; `dat0_i18n::t` returns the key string itself on a miss (no
  panic). P10b ships zero missing keys (final review verified every `settings_ui`
  `t("…")` resolves), but the next phase should not over-trust this gate. A real
  key-resolution check is a candidate follow-up.
- **Originating doc:** `docs/plans/2026-06-23-dat0-p10b-plan.md` (T4/T7 persist-on-render;
  Global Constraints i18n-check claim). Final whole-branch review 2026-06-23 (verdict:
  merge OK with this filed as a tracked follow-up).
- **Closed in:** P10c (branch `p10c-crash-reporting`). Change-gated persist (T11) +
  cleanup trio — orphan `SettingsView` + dead `render` trait (T12) + i18n-check
  key-resolution + orphan `settings.update.auto_check` key removed (T9). Also
  corrected the "i18n-check fails on missing keys" claim: it is warn-only (exit 0;
  does not resolve referenced keys against `en.json`). Squash SHA not yet assigned
  (pre-merge at time of writing).
- **Last touched:** 2026-06-25

### D-031 — Display-type letter-spacing unavailable on gpui 0.2.2

- **Status:** closed — 2026-08-10, by Phase 7 of the GPUI→Dioxus migration.
  The deferral was gated on a GPUI feature that never arrived: no `Styled`
  setter, no `TextStyle` field, nothing to set. CSS has had `letter-spacing`
  since CSS1, so the tracking the v4 shell specifies is now simply written
  where the rest of the type is, in `crates/dat0-ui/assets/app.css`.
- **Was:** open
- **Severity:** low
- **Deferred from:** UI1 (2026-08-08)
- **Target phase:** gated on upstream — see `docs/upstream-watch.md`
- **Affected files:** `crates/dat0-app/src/theme/tokens.rs` (`TextRole`)
- **Reason:** the v4 shell specifies negative tracking on every display size —
  `-0.035em` on h1 and the waitlist h2, `-0.03em` on the compare h2 and the
  pane h2s, `-0.05em` on the wordmark
  (`dat0-site/.planning/sketches/009-redesign-landing-v4/DESIGN-SPEC.md` §4).
  There is no API for it on the pinned toolchain, and this was verified against
  the vendored source rather than assumed:
  - `gpui::Styled` (`gpui-0.2.2/src/styled.rs`) exposes `font_weight` (`:404`),
    `text_size` (`:424`), `font_family` (`:616`) and `line_height` (`:644`).
    There is no tracking/letter-spacing setter.
  - `gpui::TextStyle` (`gpui-0.2.2/src/style.rs:354-398`) has sixteen fields —
    `color`, `font_family`, `font_features`, `font_fallbacks`, `font_size`,
    `line_height`, `font_weight`, `font_style`, `background_color`,
    `underline`, `strikethrough`, `white_space`, `text_overflow`, `text_align`,
    `line_clamp` — and none of them is letter-spacing.
  - A case-insensitive scan of `gpui-0.2.2/src/` for `letter_spacing` /
    `letterspacing` / `tracking` returns only unrelated hits (a macOS mouse
    tracking area, Wayland serial tracking, and three doc comments).
  So the gap is in the renderer, not in dat0's token layer: `TextRole` could
  carry a tracking value today, but nothing downstream could apply it.
- **What UI1 shipped instead:** the rest of the v4 ladder lands in full — sizes
  11 / 12.5 / 13 / 15 / 20 / 28 px, weights NORMAL/SEMIBOLD, and the tightening
  leading (1.4 → 1.03). Display type is therefore correct in every dimension
  except optical tightness, which reads as very slightly loose at 20 px and
  28 px and is invisible below that.
- **What target phase delivers:** once gpui exposes the setter, add
  `TextRole::tracking()` beside `size()`/`weight()`/`line_height_factor()`,
  apply it in `TypoStyled::text_role`, and extend
  `text_role_ladder_exact_values` with the fourth column. No call site changes —
  that is the whole reason the ladder is centralised.
- **Why not now:** the only workarounds are worse than the defect. Per-character
  positioning would mean abandoning `text_role` for a custom element on every
  heading; forking gpui would break the exact-pin policy in root `Cargo.toml`
  and `docs/upstream-watch.md`.
- **Originating doc:** UI1 (production-v1 plan, UI workstream).
- **Last touched:** 2026-08-08

### D-032 — Promote `perf-gate` from label-triggered to every-PR (needs dedicated macOS hardware)

- **Status:** open
- **Severity:** low
- **Deferred from:** MX3 (2026-08-08), splitting D-013
- **Target phase:** gated on hardware purchase (dedicated Apple Silicon Mac mini)
- **Reason:** MX2/MX3 made the perf budget enforceable anywhere, which closed
  D-013's correctness half. What is left is purely a question of *where* the
  gate runs, and it is a cost question:
  - `ci.yml`'s three-scenario perf step is `continue-on-error: true` on
    `macos-14` hosted runners. It has to be. A virtualized GPU cannot defend a
    frame-rate claim, and a gate that reddens `main` on hardware noise gets
    disabled within a week — which is strictly worse than an advisory one.
  - The blocking six-scenario `perf-gate` job therefore runs only behind the
    `run-perf` PR label, the same shape `heavy.yml` uses.
  - Hosted `macos-14` also bills at the 10× multiplier, which was D-013's
    original driver and is still the dominant contributor to the org Actions
    cap. Self-hosting macOS needs Apple hardware (the EULA permits macOS guests
    only on Apple-branded hosts).
- **What target phase delivers:** a Mac mini running Tart-managed ephemeral
  macOS VMs as Actions runners. Then `perf-gate`'s `runs-on:` changes and its
  label condition is dropped — **that is the entire diff**, which is the point
  of having built the harness host-independently. Validate Metal Toolchain,
  Xcode CLT, rustup and the Secret Service test paths in the VM image, and
  record the new host's baseline with `cargo xtask perf --update-baseline` in
  its own commit.
- **Why not now:** no Mac mini available. Running CI on the active dev Mac was
  rejected in P2 and the reason still holds — background CI fights interactive
  work for CPU/RAM, and on a *perf* gate specifically that contention is not
  merely slow, it corrupts the measurement.
- **Originating doc:** MX3; supersedes the runner half of D-013.
- **Last touched:** 2026-08-08

### D-030 — Mid-stream Arrow errors are invisible on uncounted paths

- **Status:** open
- **Deferred from:** EN3 (production v1)
- **Target phase:** blocked upstream — see `docs/upstream-watch.md` (duckdb-rs
  `Result`-yielding Arrow iterator)
- **What it is:** `duckdb::Arrow::next` is
  `Some(RecordBatch::from(&self.stmt?.step()?))`
  (`duckdb-1.4.4/src/arrow_batch.rs:27-33`). `step()` returns an `Option`, not a
  `Result`, and the iterator's `Item` is a bare `RecordBatch`. A DuckDB error
  raised *after* the statement bound successfully therefore terminates the batch
  loop in exactly the same observable way as end-of-stream, and duckdb-rs 1.4.4
  exposes no statement-error accessor to disambiguate them. The consumer sees a
  short result and a successful `Ok(_)`.

  Every code path that drains that iterator is affected:
  `execute::run_materialized` (`execute/mod.rs`), `execute::paged::run_page`
  (`execute/paged.rs`), and `execute::streaming::spawn_streaming`
  (`execute/streaming.rs`). Prepare/bind-time errors are unaffected — those
  return a real `Err` and are translated normally.
- **What EN3 delivered instead of a fix:** reconciliation on the ONE path that
  can afford a detector. `execute::paged::run_paged` already computes
  `SELECT COUNT(*)`, so after the batch loop it compares the rows that arrived
  against `min(limit, total - offset)` and returns
  `EngineError::EngineFailed("result stream ended early: expected N rows, got M")`
  on a mismatch. That covers every SQL-console result page and every
  `execute_paged` caller. It deliberately does NOT cover `execute_page` (EN1's
  count-free grid path), `execute`, or `execute_streaming`: none of them has a
  count to compare against, and fabricating one would reintroduce the O(N)-per-page
  scan EN1 removed.
- **Why no probe was invented:** the obvious candidates do not exist at this pin.
  There is no `Statement::error()`, the `Arrow` iterator borrows the statement so
  it cannot be re-interrogated after the loop, and re-running the query to
  compare is both a second full scan and racy against concurrent DDL. A
  heuristic here would report truncation that did not happen, which is worse
  than the current silence.
- **Fix when picked up:** replace the drain loops with the upstream
  `Result`-yielding iterator once it lands, and delete the reconcile block in
  `run_paged` (its cost is one comparison, but its existence is only justified by
  this gap). Until then the three drain sites carry a `D-030` reference comment
  so the limitation is traceable from the code.
- **Originating doc:** EN3, `docs/plans/2026-08-08-dat0-production-v1-plan.md`
- **Last touched:** 2026-08-08

### D-033 — MotherDuck traffic is unmetered by the egress counter

- **Status:** open
- **Deferred from:** SH1 (production v1)
- **Target phase:** blocked upstream — needs a byte-accounting hook in the
  DuckDB MotherDuck extension, or a dat0-supplied transport for it
- **What it is:** `crate::telemetry::egress::total_sent()` counts
  application-layer request bytes dat0 itself puts on the wire, and the status
  bar renders that number under the marketing page's `0 bytes left this
  machine` claim. Four of the five outbound seams are fully counted. The fifth
  is not: `connections::connect::run_connect` hands a token to DuckDB's
  MotherDuck extension via `ATTACH 'md:'`, and from that point the extension
  owns its own socket to the service. Every query dat0 routes at an `md:`
  database leaves this machine over a connection dat0 never sees and cannot
  size.
- **What SH1 delivered instead of a fix:** honesty about the gap, in the
  product rather than only in this file. The attach seam records the credential
  handoff it does originate and calls
  `telemetry::egress::note_unmetered_channel()`. Once that latches,
  `has_unmetered_channel()` is true forever (the bytes already left; a
  disconnect does not un-send them) and the sidebar footer renders
  `egress <n> +` — the `+` marks the figure as a FLOOR. A bare number after an
  attach would read as a complete accounting it cannot be, which is the failure
  mode this deferral exists to prevent.
- **Why no estimate was invented:** any number dat0 could synthesise for MD
  query traffic (SQL text length, result-set size) would be off by orders of
  magnitude in both directions and would look more authoritative than the
  em-dash it replaced. A measured floor marked as a floor is the only honest
  shape available at this pin.
- **Fix when picked up:** if the extension gains a byte-counter pragma or dat0
  proxies MD over its own transport, record real totals at that seam and delete
  the `UNMETERED` latch plus the footer's `+` branch.
- **Originating doc:** SH1, `docs/plans/2026-08-08-dat0-production-v1-plan.md`
- **Last touched:** 2026-08-08

### D-034 — Sidebar footer reports no tab count

- **Status:** open
- **Severity:** low
- **Deferred from:** SH2 (production v1)
- **What it is:** the plan specified the footer's first row as "window/tab
  counts from `window_registry::WindowRegistry`". The registry tracks windows
  only (`WindowRegistry::windows: Vec<WindowHandle>`); tabs live in
  `session::Session.tabs`, behind the shell's `Arc<Mutex<Session>>`.
  `catalog::panel::render_catalog` — the seam the footer renders from —
  receives `(tree, collapsed, active, focus_handle, cx)` and has no route to
  the session. So the row reports `N windows · M workspaces`, both of which the
  registry genuinely has (`len()` and the `workspace_path.is_some()` count),
  and no tab count.
- **Why not just widen the signature:** `render_catalog`'s caller is
  `WorkspaceShell::render_catalog_body` in `window/dock.rs`, which does have
  `&self`. Threading a tab count through is a two-line change — but a tab count
  is per-window state and the other two numbers on that row are process-wide,
  so mixing them would make one row answer two different questions. If the row
  should carry tabs, it should be a fourth row scoped to this window, which is
  a design call for AX2's visual pass rather than something to guess here.
- **Fix when picked up:** either add `tabs: usize` to
  `view::sidebar_footer::SidebarFooterModel` and pass it from
  `render_catalog_body`, or drop the idea and record that windows+workspaces is
  the intended census.
- **Originating doc:** SH2, `docs/plans/2026-08-08-dat0-production-v1-plan.md`
- **Last touched:** 2026-08-08

### D-036 — Two `block_on(Session::…)` sites remain on the GPUI main thread

- **Status:** open
- **Severity:** low
- **Deferred from:** EN4 (production v1)
- **What it is:** EN4 removed the two `block_on(Session::new(...))` calls the
  plan named — `window/boot.rs`'s `spawn_window` (Cmd-N / UDS second instance)
  and `run_app`'s cold start — and with them the SAFETY note admitting the
  first would become a nested-runtime abort the day gpui dispatched an action
  from inside a tokio task. Two structurally identical calls survive:
  `window/workspace_ops.rs`'s `spawn_workspace_window`
  (`rt.block_on(Session::recover_workspace(...))`) and
  `window/package_ops.rs`'s `open_package_at` (`rt.block_on` around an unpack
  plus `Session::from_parts`). Both carry the same latent hazard and both
  freeze every open window for the length of a DuckDB open.
- **Why not in EN4:** neither is a `Session::new`, and neither reduces to
  "open the window, post the session in". `spawn_workspace_window` must hold a
  `WorkspaceLock` + `LockManifestGuard` that are acquired BEFORE the window and
  released if the recover fails, so a `Booting` window would have to own the
  guards and hand them back on failure. `open_package_at` unpacks a `.dat0`
  into a scratch dir and builds the session `from_parts` with the parsed tabs,
  saved queries and charts already installed — a `Booting` shell would need a
  way to receive that payload, not just an `Arc<Mutex<Session>>`. Both are also
  behind a user gesture that already paused for a native file picker, so the
  perceived freeze is smaller than Cmd-N's was.
- **Fix when picked up:** `SessionSlot` and `boot::open_shell_window` are the
  infrastructure this needs and both exist now. Widen
  `WorkspaceShell::adopt_session` (or add a sibling) to take the extra payload,
  give `SessionSlot::Booting` an optional guard slot, and route both callers
  through `spawn_session_boot`'s dispatcher hop.
- **Originating doc:** EN4, `docs/plans/2026-08-08-dat0-production-v1-plan.md`
- **Last touched:** 2026-08-08

### D-037 — Nine docs still describe the GPUI build

- **Status:** closed
- **Severity:** low
- **Deferred from:** the GPUI→Dioxus migration
- **What it was:** the migration swept the contributor-facing docs (README,
  CONTRIBUTING, SECURITY, the CI configs, the PR template) and
  `docs/release-prerequisites.md`, whose `dat0-app` references were pure path
  renames into `dat0-core`. Nine deeper documents were left alone, `docs/a11y.md`
  chief among them.
- **Why it was not swept with the others:** those files do not merely *mention*
  the old crate, they document its architecture. `a11y.md` gave commands for a
  test that no longer exists (`theme_contrast_gate`), a feature that no longer
  exists (`a11y-capture`), and files that no longer exist
  (`src/window/render.rs`). A path rename would have made every one of those
  read as current while staying wrong, which is worse than visibly stale.
- **Closed by:** the doc-accuracy pass following PR #82.
  - `docs/a11y.md` **rewritten**, not renamed. It was a point-in-time audit of a
    toolkit that no longer exists. Its hand-maintained contrast table and
    keyboard tally are now owned by gates that cannot drift, so the document
    points at them and states the measured minima instead of copying rows: light
    4.74:1, dark 4.75:1, high-contrast 9.18:1, measured with `contrast_ratio`
    over the committed builtins. Every `path:line` citation in it was checked
    against the tree, and four the research pass got wrong were corrected.
  - The screen-reader section changed substantively rather than cosmetically.
    Its old rationale — GPUI exposes no accessibility tree, so A5 is out of
    scope — is void: a WebView exposes ARIA natively and dat0's roles and labels
    ship in release. The section now records that, and is careful to claim only
    the mechanism: no screen-reader UAT has been performed, and it says so.
  - `privacy.md`, `privacy-review-process.md`, `security-runbook.md`,
    `release-runbook.md`, `ci-mac-vm-runner.md`, `README.md` were genuine path
    renames — each target resolved 1:1 into `dat0-core` and was verified to
    exist before the edit.
  - `docs/ci.md` needed more than renames: it described a `[sources]` allow-list
    and a dependabot ignore-list that no longer match the files. Both were
    corrected against the real configs.
  - `docs/upstream-watch.md` needed nothing. Its gpui mentions are explicitly
    marked historical, which is the correct treatment and the reason it was
    wrongly counted in the original nine.
  - One live config defect fell out of the sweep: `deny.toml` still allow-listed
    the `gpui-component` git URL. The lockfile carries zero git sources now, so
    `allow-git` is empty and `unknown-git = "deny"` has nothing to excuse.
    Verified `sources ok` with cargo-deny 0.20.2 on Linux.
  - `docs/deferrals.md` keeps its GPUI references. This is a historical register;
    rewriting the record would be falsifying it.
- **Originating doc:** this migration's PR
- **Last touched:** 2026-08-13

---

### D-038 — `Coverage (report only)` has never produced a report

- **Status:** in-progress — fixed locally, awaiting the first green CI run
- **Severity:** low
- **Deferred from:** the GPUI→Dioxus migration
- **What it was:** the coverage job arrived with this migration — `main`'s
  `ci.yml` has no such job — and had not once succeeded. It runs
  `cargo llvm-cov nextest --workspace`, and the hosted runner died partway: the
  job marked failed while `cargo llvm-cov` was still `in_progress`, no logs
  uploaded, giving up around 27 minutes against a 90 minute timeout.
- **The cause, from the only evidence that survived.** No logs reached the
  artifact store, but GitHub keeps check annotations separately, and they said:
  `System.IO.IOException: No space left on device` — while writing the runner's
  *own* diagnostic log. Disk, not memory, and so complete that the runner could
  not report it. That is why the job looked like an unexplained hang.
- **Why it could never have fit.** The workspace builds 190 integration-test
  binaries, each linking a `libduckdb-sys` rlib that is 1.6 GB by itself. The
  ordinary debug build is 64 GB of target directory here, and `build-and-test`
  already calls that its low-water mark for disk. Instrumenting the same
  workload on top of it was never going to work; this was not a few GB short.
- **The fix: `CARGO_PROFILE_DEV_DEBUG: 0`.** Debug info was what did not fit,
  and coverage does not need it — llvm-cov takes line numbers from the coverage
  map `-C instrument-coverage` emits, not from DWARF. Measured on the whole
  workspace: **21 GB** instrumented without debug info, against 64 GB for the
  ordinary debug build, and the report is undiminished — 19 789 line records
  over 194 files, 8 005 functions, 1 709 tests, **84.9% lines**. The figure is
  recorded in `docs/ci.md`, which is where the job's comment always said the
  first measurement would live.
- **`continue-on-error` removed.** It was added when the job could not run at
  all; a job that completes should be allowed to speak.
- **What is left:** confirm the same figure on a hosted runner. The 21 GB was
  measured on macOS, and Linux binaries are not identical — the margin is
  large but it is not yet observed on the machine that matters.
- **Originating doc:** this migration's PR
- **Last touched:** 2026-08-13

---

## Plan defects

### PD-001 — tracing EnvFilter directive `dat0=debug` doesn't match dat0 crates

- **Status:** open
- **Severity:** low
- **Affected files:** `crates/dat0-app/src/boot.rs:init_logging`
- **Symptom:** `EnvFilter::new("info,dat0=debug")` matches by *module path*.
  All dat0 crates compile to module paths with underscores: `dat0_app::*`,
  `dat0_engine::*`, `dat0_format::*`, `dat0_i18n::*`, `dat0_keychain::*`. The
  directive `dat0=debug` matches `dat0::*` (which doesn't exist) and not any
  of the actual crates. Net effect: dev runs only show `INFO` events; nothing
  emits at `DEBUG` despite the apparent intent.
- **Discovered:** P1 T3 code review
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` Step 3.3
  + plan inheritance into T21 boot rewrite
- **Suggested fix:** Replace with explicit per-crate directives —
  `info,dat0_app=debug,dat0_engine=debug,dat0_format=debug,dat0_i18n=debug,dat0_keychain=debug`
  — or use a shared helper that enumerates dat0 crate prefixes.
- **Last touched:** 2026-04-28

### PD-002 — Settings store atomic-write missing `fsync` before rename

- **Status:** closed
- **Severity:** low (durability, not correctness)
- **Affected files:**
  - `crates/dat0-app/src/settings/store.rs:save` — **closed by T12** (settings.toml path)
  - `crates/dat0-app/src/session/mod.rs:persist` — **closed by T8** (session.json path)
- **Symptom:** Comment claimed "Atomic write: write to .tmp, fsync, rename." but
  the code used `std::fs::write` (which only writes + closes, no fsync). On
  macOS the rename is atomic at the directory-entry level, but the new file's
  data blocks may not be durable before the rename completes if the kernel
  hasn't flushed the page cache. On power loss the user could see a
  zero-length `settings.toml`.
- **Discovered:** P1 T8 implementer report
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` Step 8.3
- **Session.json side closed by:** commit `e295cd5` (T8 post-review, P4a) — `session/mod.rs::persist` now uses `OpenOptions` + `write_all` + `sync_all` + `rename` + parent-dir fsync.
- **Closed by:** P4b T12 — `settings/store.rs::save` now uses the same durable
  write pattern as the session-side twin: `OpenOptions` + `BufWriter` +
  `write_all` + `sync_all` (file) + `rename` + `sync_all` (parent dir). Three
  durability regression tests added to `crates/dat0-app/tests/settings_store.rs`:
  `save_no_tmp_file_left_after_successful_save`, `save_overwrite_yields_complete_valid_toml`,
  `save_durability_round_trip_all_fields`. Both sides now fully closed.
- **Last touched:** 2026-05-31

---

### PD-003 — cargo-about NOTICE output not deterministic across host platforms

- **Status:** closed
- **Severity:** low (was a warn-only CI gate, not a blocker)
- **Affected files:** `about.toml`, `.github/workflows/notice.yml`, `NOTICE.md`
- **Symptom:** `cargo about generate` produces different output on macOS vs Linux
  hosts even with `targets = [all-4-targets]` in `about.toml`. Specifically,
  dual-licensed crates (e.g., `ureq` with `Apache-2.0 OR MIT`) get assigned
  different licenses depending on host platform's tiebreak — moving them
  between license sections in the rendered NOTICE. The `notice` CI job, which
  diffs the committed NOTICE against a fresh CI-generated one, fires on this
  one-line ordering noise.
- **Discovered:** P1 CI first run on PR #1 (2026-04-26) — committed NOTICE was
  generated on macOS arm64; Linux x86_64 runner regenerated and saw `ureq`
  in a different license section.
- **Mitigation in P1:** `notice` job set to `continue-on-error: true`. Failure
  reported as `::warning::` instead of `::error::`. Drift is still surfaced
  but doesn't block PRs.
- **Suggested fix:** Either (a) pin tie-broken licenses explicitly in
  `about.toml` per crate (e.g., `[crates.ureq] accepted = ["MIT"]`), or
  (b) restrict `targets` to a single canonical platform for the gate, or
  (c) compare a normalized form of the output (sort lines per section).
  Option (a) is the most-honest if the dual-license selection is genuinely a
  policy choice; option (b) is the simplest if NOTICE is intended as a
  generic union.
- **Closed by:** QA2 — suggested fix (b). The non-determinism was never in
  cargo-about; it was in comparing output generated on one host against output
  generated on another. `.github/workflows/notice.yml` now pins the job to
  `ubuntu-latest` (which `docs/release-runbook.md` already mandates as the
  regeneration host), installs `cargo-about` from a prebuilt binary via
  `taiki-e/install-action` instead of `cargo install` (one less resolution to
  drift), and `continue-on-error` is removed — the job is a hard gate and the
  drift message is now `::error::`, naming Linux as the canonical host so a
  contributor does not "fix" it by regenerating on macOS.
  `about.toml` still lists all four targets, so the NOTICE CONTENT is still
  the union across platforms; only the tiebreak host is fixed.
- **Last touched:** 2026-08-08

---

### PD-004 — Linux Secret Service backend not reachable from CI keychain tests

- **Status:** open
- **Severity:** low (tests skipped on Linux CI; macOS coverage still authoritative for keychain primitive)
- **Affected files:** `crates/dat0-keychain/tests/round_trip.rs`,
  `crates/dat0-app/tests/p1_exit_smoke.rs`, `.github/workflows/ci.yml`
- **Symptom:** Keychain round-trip tests on Linux runners panic with
  `SS error: result not returned from SS API`. The CI job sets up
  `dbus-launch` + `gnome-keyring-daemon --unlock --start --components=secrets`
  in one step and propagates `DBUS_SESSION_BUS_ADDRESS` via `$GITHUB_ENV`,
  but the daemon process does not survive cleanly across step boundaries
  on GitHub-hosted Ubuntu images. By the time `cargo test` runs, the
  Secret Service bus is unreachable.
- **Discovered:** P1 CI first run on PR #1 (2026-04-26) — both
  `ubuntu-latest` and `ubuntu-22.04-arm` failed.
- **Mitigation in P1:** keychain tests gated `#[cfg_attr(target_os = "linux", ignore = "...")]`
  so they're skipped on Linux runners. macOS keychain coverage (T12) is
  authoritative for the round-trip contract; Linux-side correctness is
  established only by compilation under `#[cfg(target_os = "linux")]`.
- **Suggested fix paths:**
  - **(a) Run tests under `dbus-run-session`:**
    ```yaml
    - name: Test (Linux)
      if: runner.os == 'Linux'
      run: |
        dbus-run-session -- bash -c '
          gnome-keyring-daemon --unlock --start --components=secrets <<<"" >/dev/null
          cargo test --workspace --target ${{ matrix.target.triple }}
        '
    ```
    Self-contained per-step session bus; daemon and tests live in the same
    invocation. Most reliable.
  - **(b) Use the OS-keyring crate's "mock" feature** for unit tests; keep
    integration tests macOS-only. Cleaner separation but less coverage.
  - **(c) Self-hosted Linux runner with persistent gnome-keyring** —
    overkill for what's tested.
- **Diagnostic instrument (QA4, 2026-08-08):**
  `.github/workflows/pd004-diagnose.yml` — `workflow_dispatch` only, not a gate.
  It runs `cargo test -p dat0-keychain --test round_trip -- --include-ignored`
  on hosted `ubuntu-latest` under **two** arms and uploads both logs:
  - **Arm A** — exactly the setup `ci.yml:191-198` already performs
    (`dbus-launch` + `gnome-keyring-daemon --unlock --start --components=secrets`,
    propagated across steps via `$GITHUB_ENV`). This arm exists because the
    obvious remedy has been sitting in `ci.yml` since P1 and the symptom above
    was never re-checked against it. **No duplicate of that command was added.**
  - **Arm B** — suggested fix (a) above, `dbus-run-session` wrapping daemon and
    test in one invocation.

  **This deferral stays open until the captured error is pasted here.** Run the
  workflow, then replace this sentence with the failing arm's exact output. The
  deliverable of QA4 was the instrument and the diagnosis, not a fix: the
  `#[cfg_attr(target_os = "linux", ignore)]` at
  `crates/dat0-keychain/tests/round_trip.rs:3` is deliberately **left in place**.
  Remove it only if the captured cause is then fixed AND the real `ci.yml` job
  is made to run the test — a green scratch workflow proves nothing about the
  gate.
- **Last touched:** 2026-08-08

---

### PD-005 — P2 plan T3 snippet uses `&*conn` triggering clippy explicit-auto-deref

- **Status:** closed
- **Severity:** trivial (style-only; auto-deref handles it)
- **Affected files:** `crates/dat0-engine/src/duckdb_engine.rs::apply_migrations_real`
- **Symptom:** Plan §"Task 3, Step 3.4" provides a verbatim snippet that calls
  `crate::migrations::apply_migrations(&*conn, …)` where `conn` is a
  `MutexGuard<duckdb::Connection>`. Under `clippy -D warnings` (Rust 1.95+
  toolchain pinned in `rust-toolchain.toml`), this fires
  `clippy::explicit_auto_deref` because `&conn` would coerce identically.
- **Discovered:** P2 T3 implementation (2026-04-27) — `cargo clippy
  --workspace --all-targets -- -D warnings` failed compile on first attempt.
- **Fix applied:** Replaced `&*conn` with `&conn` in the wired-in
  `apply_migrations_real` body. Behavior identical; clippy clean.
- **Originating doc:** `docs/plans/2026-04-27-dat0-p2-engine-plan.md` Step 3.4
  code block.
- **Closed by:** P2 T3 commit `4418cbd` on branch `p2-engine`.
- **Last touched:** 2026-04-28

---

### PD-006 — P2 plan T12 fixtures snippet uses bare `rng.gen::<…>()` — reserved keyword in Rust 2024

- **Status:** closed
- **Severity:** low (mechanical fix; both raw-identifier escape and rand 0.9 upgrade are clean alternatives)
- **Affected files:** `crates/dat0-fixtures/src/main.rs`
- **Symptom:** Plan §"Task 12, Step 12.3" provides verbatim code with
  `rng.gen::<u32>()`, `rng.gen::<f64>()`, and friends. The workspace is on
  edition 2024 (`rust-version = "1.85"` in root `Cargo.toml`), where `gen` is a
  reserved keyword for the eventual `gen` block syntax. rand 0.8 still names
  its `Rng` trait method `gen`, so bare `rng.gen::<T>()` calls fail to parse
  with `error: expected identifier, found reserved keyword 'gen'`.
- **Discovered:** P2 T12 implementation (2026-04-27) — first
  `cargo build -p dat0-fixtures` produced 7 parse errors; rustc itself emits a
  `help: escape gen to use it as an identifier` hint suggesting `rng.r#gen::<T>()`.
- **Fix applied:** Replaced bare `rng.gen::<T>()` calls with the raw-identifier
  form `rng.r#gen::<T>()`. `rng.gen_bool(...)`, `rng.gen_range(...)` are
  unaffected (suffixed identifiers, not the `gen` keyword). Determinism
  preserved — `r#gen` is purely lexical and resolves to the identical method.
- **Alternative considered:** Upgrade to rand 0.9, which renames `Rng::gen` →
  `Rng::random`. Rejected as a larger change for T12; staying on rand 0.8
  matches the plan's pinned version and the existing transitive resolution in
  `Cargo.lock`. If a future task adopts rand 0.9 workspace-wide, T12 can drop
  the `r#gen` escapes.
- **Originating doc:** `docs/plans/2026-04-27-dat0-p2-engine-plan.md` Step 12.3
  code block (lines 3503-3601).
- **Closed by:** P2 T12 commit `0cc61bd` on branch `p2-engine`.
- **Last touched:** 2026-04-28

---

### PD-007 — P2 plan T14 snippet calls non-existent `error_ux::banner::push_banner` with mismatched `Banner` shape

- **Status:** closed
- **Severity:** low (P1 Banner is intentionally a pure-data type; the plan
  predicted this in its inline note and asked the implementer to adapt)
- **Affected files:** `crates/dat0-app/src/error_ux/banner.rs`,
  `crates/dat0-app/src/boot.rs`
- **Symptom:** Plan §"Task 14, Step 14.2" supplies verbatim code calling
  `crate::error_ux::banner::push_banner(crate::error_ux::banner::Banner { severity: …::Severity::Warning, title_key, body_key, link })`.
  None of those identifiers match P1's actual API:
  - No `push_banner` function exists — P1 ships `Banner` as a pure-data
    state struct with no in-process registry or service layer.
  - The severity enum is `BannerSeverity` (not `Severity`).
  - `Banner`'s fields are `{ message: String, severity, dismissible, action_label }`
    — there are no `title_key`, `body_key`, or `link` fields.
  - `Banner` does not exist as a render-attached primitive yet either; the
    intent in P1 was that real GPUI rendering wires it up at use sites in P7.
- **Discovered:** P2 T14 implementation (2026-04-27) on first attempt to
  compile boot.rs against the verbatim plan snippet. The plan itself
  flags the risk (line 3799) and instructs sub-agents to consult the
  actual file and adapt — so this is more "expected drift" than a defect,
  but logging per protocol since the verbatim code does not compile.
- **Fix applied:**
  1. Added a minimal in-process pending-banner queue to
     `error_ux::banner` (`push(Banner)` + `drain_pending() -> Vec<Banner>`)
     so boot-time call sites can stash banners before any window exists.
     The render layer (P7+) drains on first window open.
  2. Boot calls `banner::push(Banner::warning(format!("{title}: {body}")))`
     using the existing P1 constructor. The `link` field has no slot yet;
     the link target is documented in the i18n body for now and can be
     promoted to a structured field when `Banner` grows an action surface.
  3. i18n keys `boot.sqlite_scanner_install_failed.title` /
     `boot.sqlite_scanner_install_failed.body` added to
     `crates/dat0-i18n/src/strings/en.json` per plan.
- **Alternative considered:** Defer all banner UX to P7 and only
  `tracing::error!` at boot. Rejected — the queue-based solution is ~30
  lines, preserves the user-facing surface the plan promised, and is
  exactly the API shape P7 will need anyway.
- **Originating doc:** `docs/plans/2026-04-27-dat0-p2-engine-plan.md`
  Step 14.2 code block (lines 3775-3796).
- **Closed by:** P2 T14 commit `9ea964b` on branch `p2-engine`.
- **Last touched:** 2026-04-28

---

### PD-008 — P3a plan T2 snippets use wrong import paths for `fs4::FileExt` and `interprocess` sync `Stream`

- **Status:** closed
- **Severity:** trivial (compile-time failure, mechanical fix)
- **Affected files:** `crates/dat0-app/src/app_lock.rs`
- **Symptom 1 — fs4:** Plan T2 Step 3 snippet uses `use fs4::FileExt;`. In
  fs4 0.9.1 the sync `FileExt` trait is not re-exported at the crate root;
  it lives at `fs4::fs_std::FileExt` (gated behind the `"sync"` feature,
  which the workspace Cargo.toml already enables). The old path `fs4::FileExt`
  resolves to `std::os::unix::prelude::FileExt` — a different trait — causing
  `try_lock_exclusive` not to be found.
- **Symptom 2 — interprocess sync connect:** Plan T2 Step 4 snippet imports
  `interprocess::local_socket::traits::Stream` and calls `.connect(name)` on
  it. In interprocess 2.4.2 `traits::Stream` is a trait (not a type), so
  `Stream::connect` is not accessible via the trait path. The concrete enum
  type `interprocess::local_socket::Stream` carries `connect(name)` and is
  the correct call site. The plan's sync example also omits the `prelude::*`
  glob that brings `ToFsName` into scope; the real code requires both.
- **Fix applied:** In `try_acquire`, changed `use fs4::FileExt;` →
  `use fs4::fs_std::FileExt;`. In `forward_open_window`, replaced the
  `traits::Stream` import + method path with
  `use interprocess::local_socket::{GenericFilePath, Stream, prelude::*};`
  and `Stream::connect(name)`. Both changes are minimal and preserve the
  intended semantics exactly.
- **Originating doc:** `docs/plans/2026-05-14-dat0-p3a-plan.md` T2 Steps 3–4
  code blocks.
- **Closed by:** P3a T2 commit on branch `p3a-hot-path`.
- **Last touched:** 2026-05-16

---

### PD-009 — P3a plan T6 test snippet calls non-existent `engine.catalog().get_tables()`

- **Status:** closed
- **Severity:** trivial (compile-time failure, mechanical fix)
- **Affected files:** `crates/dat0-engine/tests/catalog_origin.rs`
- **Symptom:** Plan T6 Step 3 test snippet calls `engine.catalog().get_tables()`
  to retrieve the table list. `DuckDBEngine` has no `catalog()` method; the
  correct call is `engine.get_tables().await` via the `QueryEngine` trait,
  which is the same pattern used in `tests/catalog.rs` and `tests/exit_criteria.rs`.
- **Fix applied:** Changed `engine.catalog().get_tables().await` →
  `engine.get_tables().await` in the new test. The semantic intent (list all
  engine-visible tables and find the just-registered one) is unchanged.
- **Originating doc:** `docs/plans/2026-05-14-dat0-p3a-plan.md` T6 Step 3
  code block (lines 979–1005).
- **Closed by:** P3a T6 commit on branch `p3a-hot-path`.
- **Last touched:** 2026-05-16

---

### PD-010 — P3a plan T12 UDS → GPUI cross-thread window-open bridge is unsafe

- **Status:** closed
- **Severity:** low (single-instance enforcement and Cmd-N multi-window are fully
  functional; only UDS-triggered window-open from a second launch is affected)
- **Affected files:** `crates/dat0-app/src/window.rs` (`run_app` UDS handler)
- **Symptom:** The P3a T12 plan instructs the UDS `serve` handler to call
  `async_cx.update(|cx| cx.open_window(...))` from inside a tokio background
  task. `AsyncApp::update` internally calls `app.borrow_mut()` on a
  `Rc<RefCell<AppState>>`. `RefCell` is not `Send`/`Sync` — calling it from a
  non-main thread while the Cocoa event loop may hold the borrow causes a
  `RefCell` panic (or UB on older rustc). GPUI's foreground executor (backed by
  the macOS Cocoa run loop) is the only safe caller of `AsyncApp::update`.
- **Root cause:** GPUI v0.2.2 was designed for a single-threaded event model
  where all window mutations happen on the platform main thread. There is no
  thread-safe "wake the main thread and dispatch a closure" API analogous to
  `dispatch_async` in GCD (which Zed uses internally but does not expose
  publicly in the published `gpui` crate).
- **What P3a T12 ships:** UDS `serve` handler logs received `OpenWindowMessage`
  via `tracing::info!`. Single-instance enforcement (second launch forwards via
  UDS + exits) is fully functional. Cmd-N (`menu_macos::NewWindow` action)
  spawns a new window synchronously on the main thread via `cx.on_action`.
- **What target phase delivers:** One of:
  - **(a) GCD dispatch bridge** — capture a `dispatch_async`-able block in
    `Application::new()` before the event loop starts; invoke it from the tokio
    handler to schedule a main-thread closure via libdispatch. Requires
    `block2` + `dispatch` crates and an FFI shim.
  - **(b) gpui foreground executor channel** — poll `ForegroundExecutor::spawn`
    from a futures-compatible channel (e.g. `futures::channel::mpsc`) that the
    tokio handler sends into via `try_send`. The gpui spawn'd future loops on
    the receiver and calls `cx.open_window` on the main thread.
    The `futures::channel::mpsc` bridge requires capturing the `Sender<Box<dyn FnOnce(&mut App) + Send>>` before `Application::run` begins so the UDS handler closure (running on a tokio task) can post into it; the receiver lives inside a gpui foreground-executor loop registered during app init.
  - **(c) Upstream contribution** — add a `dispatch_to_main_thread` API to
    `gpui` and upstream it.
  - Option (b) is the lowest-friction path: gpui already uses
    `futures::channel::oneshot` internally; `try_send` is sync and safe from
    tokio context.
- **Originating doc:** `docs/plans/2026-05-14-dat0-p3a-plan.md` T12 Step 2
  ("Pattern C (preferred)" + "Practical fallback").
- **Target phase:** T17 / P3b polish — depends on the UDS round-trip
  integration test (T16) that validates single-instance enforcement end-to-end.
- **Closed by:** T1 (`crates/dat0-app/src/main_bridge.rs` + UDS handler rewire + `tests/main_thread_dispatcher.rs`); P3b plan T1. The futures-mpsc dispatcher (option (b)) is captured before `Application::run`; the UDS handler posts visual-spawn closures through it. P3a partial exit #5 now PASS.
- **Last touched:** 2026-05-25

---

### PD-011 — P3b plan §3.7 ambiguity rule references sniff outputs that don't exist

- **Status:** open
- **Severity:** low (the import wizard still ships; only the trigger heuristic
  changes shape)
- **Affected files:** `docs/plans/2026-05-25-dat0-p3b-plan.md` T9 task description
  (the three-clause ambiguity rule); `docs/specs/2026-05-25-dat0-p3b-ux-polish-design.md`
  §3.7 (mirrors the same rule)
- **Symptom:** P3b spec §3.7 and the plan's T9 description specify a three-clause
  ambiguity rule for triggering the import wizard:
  (a) "more than one candidate delimiter scored within 5% of the top",
  (b) "sniff returns a non-UTF-8 encoding marker",
  (c) "type-inference flags any column with confidence below the sniff's
  reported threshold."
  None of these three clauses match the actual `sniff_csv` output shape
  documented at <https://duckdb.org/docs/stable/data/csv/auto_detection#sniff_csv-function>:
  - (a) `sniff_csv` returns only the winner delimiter; there is no candidate
    list and no scores.
  - (b) There is no encoding column in the output. The CSV reader auto-detects
    UTF-8 vs Latin-1 internally but does not surface that choice.
  - (c) The `Columns` STRUCT array has `(name, type)` pairs only — no
    per-column confidence value.
  Cited verification: P3b.T0 spike doc
  `docs/internal/duckdb-arrow-api-notes.md` §SniffCsv (2026-05-25 entry).
- **Root cause:** Plan drafted from spec memory of "sniff returns rich
  metadata" assumption. The real DuckDB sniff is dialect-focused, not
  uncertainty-focused. duckdb-rs 1.4.4 has no typed Rust wrapper for
  `sniff_csv` either; it is SQL-only, so the plan also can't lean on a Rust
  return-type signature.
- **What P3b T9 must do instead:** Replace the three-clause rule with a
  spec-compatible substitute grounded in actual sniff output. Recommended
  shape (see `duckdb-arrow-api-notes.md` §SniffCsv #5):
  1. **Delimiter check via dual-sniff:** run `sniff_csv` twice with two
     `sample_size` values (e.g., 4096 and 65536); if the inferred `Delimiter`
     differs between runs, treat the file as ambiguous.
  2. **Encoding heuristic:** read the first 8 KB of the file with
     `std::str::from_utf8`; on decode error, treat as ambiguous and default
     the wizard to Latin-1.
  3. **(Optional) Type stability:** compare `Columns[*].type` between the
     same two sample sizes; if any column type differs, treat as ambiguous.
- **What target phase delivers:** Plan + spec §3.7 amended to drop (a), (c)
  as written and substitute the dual-sniff + UTF-8-heuristic rule. T9 plan
  step list updates to call the new shape. Spec §3.7 wording remains
  conceptually accurate ("ambiguous → drawer, confident → bypass") so the
  amendment is mechanical.
- **Originating doc:** `docs/specs/2026-05-25-dat0-p3b-ux-polish-design.md`
  §3.7 + `docs/plans/2026-05-25-dat0-p3b-plan.md` T9 (the spike-source
  references at the bottom of each).
- **Target phase:** P3b T9 (no separate target phase — this defect is
  surfaced by T0 and consumed by T9 before T9 begins implementation).
- **Last touched:** 2026-05-25

---

### PD-012 — `NYC_TAXI_SHA256` placeholder until release asset upload

- **Status:** open
- **Severity:** low (the fetch path is wired + fully tested via mockito; only
  the production NYC-taxi sample is unavailable until the asset upload)
- **Affected files:** `crates/dat0-app/src/sample_data.rs`
  (`NYC_TAXI_SHA256` constant)
- **Symptom:** `NYC_TAXI_SHA256` is currently `"FILL_AT_T8"`. If the
  empty-state hero (T7) dispatches the NYC taxi sample, `fetch_remote`
  downloads the body from `NYC_TAXI_URL` and then fails the SHA-256
  compare ("checksum mismatch: expected fill_at_t8 got <real>"). The
  failure path is correct UX — the T8 banner shows
  "Sample data download failed: Couldn't download from <url>: checksum
  mismatch…" with a Retry button — but the user can never successfully
  pull the asset until this placeholder is replaced with the real hash.
- **Discovered:** P3b T8 implementation (2026-05-25). The plan explicitly
  scoped this as maintainer work: T8 wires + tests the fetch path; the
  asset upload + hash compute is owned outside the implementer's
  worktree.
- **What T8 ships:** Async `fetch_remote(url, expected_sha, …)` with
  rustls-only reqwest, SHA-256 verify, atomic-write cache. mockito tests
  cover happy-path / checksum-mismatch / 404 / cache-hit. Banner
  `fetch_failed_banner` + `sample_data.retry_taxi` action descriptor for
  the offline UX. `NYC_TAXI_URL` points at the
  `accidentally-awesome-labs/dat0` GitHub Release tag `sample-data-v1`
  with asset name `nyc_taxi.parquet`.
- **What target phase delivers:** Maintainer uploads `nyc_taxi.parquet`
  to the `sample-data-v1` release, computes the SHA-256
  (`shasum -a 256 nyc_taxi.parquet`), replaces `"FILL_AT_T8"` in
  `sample_data.rs` with the real lowercase hex, and lands a one-line
  commit. No code paths change. The fetch path then succeeds end-to-end
  on first hero click.
- **Target phase:** P3b T13 retro, or P3b follow-up commit if the asset
  upload lands before retro.
- **Originating doc:** `docs/plans/2026-05-25-dat0-p3b-plan.md` T8 ("External
  work (defer to maintainer)").
- **Last touched:** 2026-05-25

---

### PD-013 — P4a T0 plan-snippet drifts: fixtures lib + engine bench setup + criterion workspace inheritance

- **Status:** closed
- **Severity:** low (build-time failures; mechanical fixes applied in T0)
- **Affected files:**
  - `docs/plans/2026-05-27-dat0-p4a-plan.md` T0 Steps 3, 5 code snippets
  - `crates/dat0-fixtures/Cargo.toml` + `src/lib.rs`
  - `crates/dat0-engine/Cargo.toml` + `benches/`
  - `Cargo.toml` (workspace) — criterion feature set
- **Symptoms (three related drifts):**

  **Drift 1 — `dat0-fixtures` is binary-only.**
  Plan T0 Step 3 instructs "Append to `crates/dat0-fixtures/src/lib.rs`: `pub mod filter;`"
  as if `dat0-fixtures` already had a lib target. At T0 execution, the crate was
  binary-only: `[[bin]] name = "dat0-fixtures" path = "src/main.rs"` with no `[lib]`
  section and no `src/lib.rs`. The plan step fails at the first `pub mod filter;`
  reference from the bench.

  **Drift 2 — `dat0-engine` has no `benches/` dir or criterion dev-dep.**
  Plan T0 Step 5 hint "matches what `grid_scroll` bench uses" is misleading —
  `grid_scroll` lives in `crates/dat0-app/benches/`, not engine. At T0 execution,
  `crates/dat0-engine/Cargo.toml` had no `[[bench]]` entry, no `benches/` directory,
  and no `criterion` in `[dev-dependencies]`.

  **Drift 3 — Plan Step 5 pins `criterion = "0.5"` verbatim.**
  The plan snippet provides `criterion = { version = "0.5", default-features = false,
  features = ["cargo_bench_support"] }` — a version-pinned form that diverges from
  the project's workspace-inheritance convention and omits the `async_tokio` feature
  required by `b.to_async(&rt)` in the bench body.

- **Discovered:** T0 execution (2026-05-27) — pre-T0 gap analysis documented
  in the task prompt; fixes applied in place per the "Plan-snippet drift
  recurrence" rule in `docs/plans/2026-05-27-dat0-p4a-plan.md` §"Phase context".
- **Fix applied:**
  1. Added `[lib] path = "src/lib.rs"` to `crates/dat0-fixtures/Cargo.toml`
     (bin + lib coexist; `main.rs` unchanged). Created `src/lib.rs` containing
     only `pub mod filter;`. Created `src/filter.rs` with the fixture generator.
  2. Created `crates/dat0-engine/benches/` directory. Added to
     `crates/dat0-engine/Cargo.toml`: `criterion = { workspace = true }`,
     `dat0-fixtures = { path = "../dat0-fixtures" }`,
     `tempfile = { workspace = true }` under `[dev-dependencies]`, and
     `[[bench]] name = "view_regen" harness = false`.
  3. Added `async_tokio` feature to the workspace `criterion` entry
     (`Cargo.toml` line 82) so `b.to_async(&rt)` compiles. The `html_reports`
     feature already present is retained; the addition is purely additive.
- **Originating doc:** `docs/plans/2026-05-27-dat0-p4a-plan.md` T0 Steps 3
  and 5; §"Phase context" "Plan-snippet drift recurrence" rule.
- **Closed by:** commits `74694e2` (T0 spike) + `2f1ab44` (T0 post-review), applied in-place during T0 execution. Session-side verified: bench compiles clean, `cargo test --workspace` green.
- **Last touched:** 2026-05-29

---

### PD-014 — P4a design §3 `#[serde(untagged)]` collision on `FilterValue` + `Scalar`

- **Status:** closed
- **Severity:** low (T1 implementation catch; design.md + wire shapes corrected before any consumer exists)
- **Affected files:**
  - `docs/plans/2026-05-27-dat0-p4a-design.md` §3 (Transformation enum snippet) + §8.2 (session.json v2 example)
  - `crates/dat0-engine/src/transform.rs`
  - `crates/dat0-engine/tests/transformation_serde.rs`
  - Downstream: `session.json` v2 wire shape (no live consumers yet — T8 will write v2; PD note so T8 author knows the shape)
- **Symptom:** Design.md §3 originally specified `#[serde(untagged)]` on both
  `FilterValue` and `Scalar`. Under this scheme:
  1. `Scalar::Str("2026-01-01")`, `Scalar::Date("2026-01-01")`, and
     `Scalar::Timestamp("2026-01-01 00:00:00")` all serialize as bare JSON
     strings — deserialization cannot determine which variant was intended.
  2. `FilterValue::None` (nullary placeholder) and
     `FilterValue::Scalar(Scalar::Null)` both serialize as JSON `null` —
     indistinguishable on the wire, causing the wrong variant to be
     reconstructed after a round-trip.
  The original manual `Serialize`/`Deserialize` impls in the first T1 commit
  (`86be0b7`, now unreachable on the branch) papered over this by using
  sentinel keys (`__date`, `__ts`, `__none`) rather than real serde attributes.
  That approach was non-standard and hard to consume from other languages.
- **Fix:** Reworked to fully serde-derived tagged forms (controller decision
  2026-05-29 after user input):
  - `FilterValue`: `#[serde(tag = "kind", rename_all = "snake_case")]`
    (internally tagged). Variants reshaped: `Scalar(Scalar)` →
    `Scalar { value: Scalar }` and `List(Vec<Scalar>)` →
    `List { values: Vec<Scalar> }`.
  - `Scalar`: `#[serde(tag = "type", content = "value", rename_all = "snake_case")]`
    (adjacent tagged). Variant shapes unchanged.
  Wire: `FilterValue::None` → `{ "kind": "none" }`;
  `FilterValue::Scalar { value: Scalar::Null }` →
  `{ "kind": "scalar", "value": { "type": "null" } }`. Distinct on the wire.
  `Scalar::Str("2026-01-01")` → `{ "type": "str", "value": "2026-01-01" }`;
  `Scalar::Date("2026-01-01")` → `{ "type": "date", "value": "2026-01-01" }`. Distinct.
- **Design doc amended:** design.md §3 snippet + §8.2 session.json v2 example
  updated in place to match the new wire shapes (2026-05-29).
- **Regression tests:** Two new tests in
  `crates/dat0-engine/tests/transformation_serde.rs` guard the formerly-colliding
  pairs: `filter_str_value_vs_date_value_are_distinct` (test 15) and
  `filter_none_vs_scalar_null_are_distinct` (test 16). Total test count: 16.
- **Wire format is self-describing:** Cross-language consumers (e.g. future
  Python .dat0 reader for P8) can route on discriminator fields without
  Rust-side knowledge.
- **Discovered:** T1 implementation (2026-05-29). The sentinel-key workaround
  in the first T1 commit (`86be0b7`) was the signal; controller chose tagged
  forms after user review.
- **Originating doc:** `docs/plans/2026-05-27-dat0-p4a-design.md` §3 +
  `docs/plans/2026-05-27-dat0-p4a-plan.md` T1 task description.
- **Closed by:** commit `48a95d0` (T1 — tagged wire format + 16 serde round-trip tests, including collision-guard tests 15 + 16); design.md §3 + §8.2 amended in-place to match wire shapes.
- **Last touched:** 2026-05-29

### PD-015 — P4a plan T2–T15 snippets use pre-PD-014 tuple form for `FilterValue::Scalar` + `FilterValue::List`

- **Status:** closed
- **Severity:** low (mechanical adaptation; ~57 occurrences are all in plan code blocks, not in committed code)
- **Affected files:**
  - `docs/plans/2026-05-27-dat0-p4a-plan.md` — T1 example (Step 1), T2 (Step 4 render impl, Steps 7+ golden tests), T5/T6/T8/T10/T11/T13/T14 wherever `FilterValue` is constructed in a snippet
- **Symptom:** PD-014 reshaped `FilterValue::Scalar(Scalar)` → `FilterValue::Scalar { value: Scalar }`
  and `FilterValue::List(Vec<Scalar>)` → `FilterValue::List { values: Vec<Scalar> }`. The plan
  document was authored before PD-014 and still uses the tuple form throughout. A T2+
  implementer pasting plan snippets verbatim hits compile errors immediately.
- **Fix (mechanical, per snippet):**
  - `FilterValue::Scalar(Scalar::Null)` → `FilterValue::Scalar { value: Scalar::Null }`
  - `FilterValue::Scalar(s)` (binding pattern) → `FilterValue::Scalar { value: s }`
  - `FilterValue::Scalar(_)` (ignore pattern) → `FilterValue::Scalar { .. }`
  - `FilterValue::List(items)` (binding) → `FilterValue::List { values: items }`
  - `FilterValue::List(vec![…])` (construction) → `FilterValue::List { values: vec![…] }`
  - `FilterValue::List(_)` (ignore) → `FilterValue::List { .. }`
  - `FilterValue::Range { lo, hi, inclusive }` is unchanged (was already struct-form).
  - `FilterValue::None` is unchanged (unit variant).
- **Discovered:** T1 quality review (2026-05-29).
- **Originating doc:** `docs/plans/2026-05-27-dat0-p4a-plan.md` — pre-PD-014 wire form.
- **Closed by:** All committed code in T1 (`48a95d0`) through T14 (`2af2c02`) uses the struct form. The plan doc was annotated in `45419b1` (PD-015 doc commit) to warn implementers; every T2+ task adapted snippets in-place and none committed the tuple form. No tuple-form code exists in the worktree.
- **Last touched:** 2026-05-29

### PD-016 — P4a UI-click → ViewChange wirings unowned by plan T13

- **Status:** closed
- **Closed by:** P4b T0 (`p4b-edit` branch). `WorkspaceShell::on_sort_zone_click` + `on_funnel_click` + the shared `route_outcome` decision drive the funnel + sort header clicks through `spawn_view_change` → `apply_view_change`; the grid delegate's click closures upgrade the `WeakEntity<WorkspaceShell>` and dispatch into them. Verified by `crates/dat0-app/tests/click_wiring.rs` (7 tests). The funnel popover's `Outcome` is routed via a stored subscription (`emit_outcome_cx` → `cx.emit`), and the right-click context-menu trigger was mounted at the PD-018 closure. T0 also fixed a latent bug the plan snippet would have shipped (funnel-editing a column that already had a filter stacked a second `AND` predicate instead of replacing — closed with `ViewModel::set_filter` column-aware upsert).
- **Severity:** medium (P4a was functionally incomplete on the UI-click path: funnel + sort-zone clicks logged but didn't trigger ViewChanges; only the keybind undo/redo path was wired end-to-end — now resolved)
- **Affected files:**
  - `crates/dat0-app/src/grid/mod.rs` — sort-zone + funnel-zone click handlers still log debug + leave wiring comments
  - `crates/dat0-app/src/view/filter_popover_entity.rs` — `Outcome::Apply` / `Outcome::Clear` emit but have no upper-layer consumer wired to `vm.apply` / `vm.replace_at_cursor` + `spawn_view_change`
  - Possibly: a missing "popover-mount-on-funnel-click" hook in `WorkspaceShell` or wherever the popover is instantiated
- **Symptom:** T7 (action registry stubs), T9 (four-zone column header), T10b (filter popover entity), T12 (sort header state machine) each shipped with placeholder dispatch closures and inline `// T13 wires …` comments. Plan §T13 Steps 1-4 (`spawn_view_change` helper, `WorkspaceShell::apply_view_change`, `view_actions::dispatch_undo/redo` real wiring, supersede-cancel test) did NOT include the click-handler wirings those earlier tasks deferred. T13 (`516c31c`) matches plan §T13 exactly; the gap is in the plan, not the implementation.
- **Net effect:** keybind undo/redo works end-to-end via `dispatch_undo` / `dispatch_redo` → `spawn_view_change` → engine round-trip → `apply_view_change` rebind. But:
  - Clicking the funnel zone does NOT open the filter popover.
  - The filter popover, if mounted manually, would emit `Outcome::Apply` to nobody.
  - Clicking the sort zone (plain or shift-click) logs the column/modifier but does NOT call `vm.set_sort`.
- **Fix paths (to close before P4a merges or accept as in-progress):**
  - **Path A (in-phase completion):** wire all three click paths in `grid/mod.rs` + `filter_popover_entity.rs` to call `spawn_view_change` via the focused-workspace lookup. ~50-100 lines of glue. Closes the P4a integration story.
  - **Path B (defer to T14 E2E):** T14's `view_restore_e2e.rs` can test the apply→undo→redo→clear→crash→reload flow via direct `ViewModel` API + engine round-trips, sidestepping the UI click layer. P4a still ships, but the UI is only half-interactive (keybinds work, mouse doesn't).
  - **Path C (defer to P4b/c):** P4b ships row-edit + clipboard which also need a click-handler integration layer; the wirings could fold in there.
- **Decision (T15 retro):** Path B accepted. T14 E2E (`view_restore_e2e.rs`) validates the full apply → undo → redo → clear → crash → restore story via direct ViewModel API + real engine round-trips. UI-click wirings defer to P4b.
- **Next phase:** P4b T0 must wire: (1) `grid/mod.rs` funnel-zone click → `filter_popover_entity.rs` mount + present via `WorkspaceShell`; (2) `filter_popover_entity.rs` `Outcome::Apply` → `vm.apply` + `spawn_view_change`; (3) `grid/mod.rs` sort-zone click (plain + shift) → `vm.set_sort` + `spawn_view_change`; (4) a click-path integration test covering the full UI-click → ViewChange → rebind loop.
- **Discovered:** T13 implementation review (2026-05-29). Documented by the controller after the T13 implementer correctly noted plan §T13 Steps 1-4 don't cover these wirings.
- **Originating doc:** `docs/plans/2026-05-27-dat0-p4a-plan.md` §T13.
- **Last touched:** 2026-05-31 (closed by P4b T0 — see `click_wiring.rs`)

---

### PD-017 — Imported files are VIEWs, so the `__dat0_rowid` surrogate never reaches them

- **Status:** closed
- **Closed by:** Path A (materialize at import). The engine grew a new
  `QueryEngine::register_file_as_table` (in `crates/dat0-engine/src/duckdb_engine.rs` +
  `src/trait_def.rs`) that REUSES the exact `CREATE OR REPLACE VIEW … AS SELECT *
  FROM read_csv/json/parquet(…)` SQL `register_file` builds (so all P3b
  delimiter/type sniffing is preserved), materializes it into a base table via
  `CREATE TABLE <name> AS SELECT * FROM <tmp_view>; DROP VIEW <tmp_view>`, then
  injects `__dat0_rowid` via `ensure_rowid_blocking` — all under one connection
  lock so the table is never observable without its surrogate. The app's sole
  import path (`crates/dat0-app/src/file_drop.rs`) now calls
  `register_file_as_table` instead of `register_file`. `register_file` (view)
  remains available for read-only callers (the grid `mod.rs`/`data_source.rs`
  unit tests still use it). Session restore needed NO change: restore re-opens
  the persistent `scratch.duckdb` rather than re-running the import, so the
  materialized base table (with its surrogate) survives recovery as-is. Tests:
  `crates/dat0-engine/tests/import_materialize.rs` (base-table + gap-free
  surrogate + sniffing preserved + ALTER-TABLE-able + no leftover view),
  `crates/dat0-app/src/file_drop.rs::csv_drop_yields_rowid_bearing_base_table`,
  and `crates/dat0-app/tests/scratch_lifecycle.rs` (recovered import is still a
  rowid-bearing base table). Closed by this commit (the `P4b: close PD-017 …
  (Path A)` commit on branch `p4b-edit`).
- **Severity:** high (the P4b edit/selection/clipboard headline feature does not work on real file imports until resolved; engine + test paths are correct, but the running app's grids are views without the surrogate)
- **Affected files:**
  - `crates/dat0-engine/src/duckdb_engine.rs` — `ensure_rowid` is correctly wired into `create_table` (the only CTAS→base-table path); `register_file` carries a hand-off comment explaining why it is NOT wired there.
  - `crates/dat0-engine/src/register/{csv,json,parquet}.rs` — all emit `CREATE OR REPLACE VIEW … AS SELECT * FROM read_*(…)`.
  - `crates/dat0-app/src/file_drop.rs:133` — the app's sole import path; calls `engine.register_file(...)` (a VIEW). No `create_table` or `ensure_rowid` call exists anywhere in the app crate.
  - Consumers of the surrogate: `crates/dat0-engine/src/render.rs` (overlay references `__dat0_rowid`); `crates/dat0-app/src/grid/{mod,data_source}.rs` (T5 hidden-key plumbing).
- **Symptom:** P4b plan T3 Step 3 says "Call `ensure_rowid` at the end of the existing table-create path … where `register_file` finalizes a CTAS import." That premise is false — `register_file` finalizes a VIEW, not a CTAS. A VIEW cannot be `ALTER TABLE … ADD COLUMN`-ed, so calling `ensure_rowid` there would error on every import. The eager surrogate therefore only lands for base tables built via `create_table`, which the app never calls for imports.
- **Net effect:** the engine surrogate + `ensure_rowid` migration + all T2/T3 tests are correct (tests use `create_table` base tables). But an imported CSV/JSON/Parquet becomes a DuckDB VIEW with no `__dat0_rowid`; when P4b edit/delete renders its overlay against that view's name, the query references a column that does not exist → runtime SQL error. The headline edit/selection/clipboard flow cannot work end-to-end on real imports until closed. T14 manual UAT (Excel/Sheets round-trip via in-app edits) depends on this.
- **Fix paths:**
  - **Path A (materialize on import):** change the app import path to `create_table` (CTAS materializing `read_*(…)` into a base table) instead of `register_file` (view), so `ensure_rowid` runs eagerly. Trade-off: loads the file into a DuckDB base table rather than a lazy view over the source file — memory/behavior change for large files. The design's "surrogate injected at import" language implies materialization was intended.
  - **Path B (back-fill on first bind):** keep views for browsing; when the grid first binds a view for editing, materialize-or-back-fill that table via `ensure_rowid`. More surgical but adds a view→table promotion step.
- **Decision:** RESOLVED — Path A (materialize at import), user-approved. Chose A1 (materialize the sniffing view) over A2 (direct CTAS) because A1 reuses 100% of the existing `register/*.rs` view-builder SQL with zero changes to those builders (smallest blast radius); the view is a transient intermediate dropped within the same lock. Session restore needed no change (recovery re-opens the persistent scratch.duckdb, not a re-import).
- **Discovered:** P4b T3 implementation (2026-05-30), confirmed independently by the T3 spec review (quoted the `register/*.rs` VIEW DDL).
- **Originating doc:** `docs/plans/2026-05-30-dat0-p4b-plan.md` §T3 Step 3.
- **Last touched:** 2026-05-30 (closed via Path A — see the `P4b: close PD-017 … (Path A)` commit on `p4b-edit`)
### PD-018 — Grid renders placeholders; paged-render cache never populated (blocks P4b UAT)

- **Status:** closed
- **Severity:** high (P4b's edit/select/clipboard logic is correct + fully test-green, but the running app cannot demonstrate it: the grid shows em-dashes and copy/paste/edit have no real cell values to act on; the headline T14 Excel/Sheets UAT gate is blocked)
- **Closed by:** the `P4b: close PD-018 — wire paged-render cache + mount deferred triggers` commit on `p4b-edit` (Path A, user-approved scope addition). What was wired:
  - **render_td → real values:** `GridTableDelegate::render_td` now calls the new `GridDataSource::cell_render(row, col)` — a synchronous LRU lookup mirroring `cell_display`/`row_key` (never triggers a DuckDB fetch). Cached cells paint their real value with numeric right-alignment + dimmed NULL styling; rows whose page isn't loaded yet keep the `"—"` placeholder for THAT cell only (virtualized-table pattern — values appear as pages load). The byte-identical-render parity tests (`delegate_columns_match_schema`, etc.) still pass.
  - **Cache population (prefetch-on-bind + scroll-paging, both complete):** `WorkspaceShell::prefetch_visible_rows(start, end, cx)` runs `page_for` OFF the GPUI thread (tokio task), then posts a `cx.notify()` back via the `MainThreadDispatcher` — the canonical `spawn_view_change` discipline, NEVER `cx.update` from the tokio task. It is called (a) on grid bind when the `TableState` is first promoted (`render`), seeding the first PAGE_ROWS window, and (b) from the gpui-component `TableDelegate::visible_rows_changed(range, …)` scroll hook the delegate now implements, which delegates into the shell via its weak handle. `page_for` is idempotent (cache hit on resident pages), so re-entrant calls are nearly free. Scroll-paging is COMPLETE (not partial): the `visible_rows_changed` hook fetches both the start-of-range and end-of-range page so a range straddling a page boundary loads both.
  - **Right-click context menu (T9):** mounted via `gpui_component::menu::ContextMenuExt::context_menu` on a `div` wrapping the `Table` in `WorkspaceShell::render` (the `Table` is `RenderOnce`, not `ParentElement + Styled`, so the menu hangs off the wrapping div). The builder is the existing `crate::grid::context_menu::build_menu(ws_weak, selection)`.
  - **Focus ring (per-cell, replaces the T11 badge):** `render_td` reads the live selection through the delegate's weak `WorkspaceShell` handle and draws a 2-px blue border on the cell at `selection.active()`, plus a lighter tint on selected cells. The previous bottom-left floating-badge placeholder in `render` was removed.
  - **Forward-incompat banner routing (T13 review Important 2):** `Session::recover` now routes a `SessionLoadError` where `is_forward_incompat()` is true to a dedicated `error_ux::Banner` (pushed to the pending queue) AND propagates the error — so the caller does NOT fall back to default state + eagerly persist the older schema over the user's newer file. Malformed-JSON / other errors keep their prior generic-propagation handling.
  - **Tests:** `tests/edit_lifecycle.rs::prefetch_populates_cache_so_cell_display_resolves_real_values` asserts `cell_display`/`cell_render` are `None` before a prefetch and resolve the REAL values after `page_for(0)`; `session::tests::forward_incompat_banner_describes_both_shapes` + `recover_forward_incompat_pushes_banner_and_errors` cover the banner routing. Full `dat0-app` + `dat0-engine` suite green; clippy `-D warnings` clean; fmt clean.
- **Affected files:**
  - `crates/dat0-app/src/grid/mod.rs` — `GridTableDelegate::render_td` returns the `"—"` placeholder for every cell (both `match` arms + fallback). Its own doc comment says `"—"` is the expected T4 (P3a) render and "the follow-up task replaces this with a synchronous cache lookup." That follow-up never happened.
  - `crates/dat0-app/src/grid/data_source.rs` — `page_for` (the only method that loads a page from DuckDB into the LRU) has ZERO production callers; it is exercised only by its own unit tests. There is no `load_more` / visible-range / prefetch wiring on the delegate.
- **Symptom:** With `render_td` painting placeholders and the page LRU never populated by the display path, the P4b cache-only synchronous reads added for clipboard/edit — `cell_display`, `row_key`, `column_arrow_type` — return `None`/empty for essentially every on-screen cell. Copy serializes empty strings; paste/cut/cell-edit resolve `row_key(row) == None` and skip every cell (paste raises the reject banner having applied nothing).
- **Net effect:** P4b is logic-complete and test-green (every transform/codec/migration is validated via real engine round-trips against base-table harnesses), but **not demonstrable in the running app** until the grid renders real values. The T14 manual Excel/Sheets round-trip + keyboard-sweep UAT cannot pass.
- **Not a P4b defect:** this is pre-existing debt from the P3a minimum-viable grid delegate (render + paging cache deliberately stubbed). It is outside the scope of every P4b plan task and the plan's "Files NOT touched" list; the P4b plan implicitly assumed the grid already rendered real data.
- **Fix paths (controller to decide with the user before T14):**
  - **Path A (wire it, scope addition):** make `render_td` do a synchronous LRU lookup → `render_cell(real value)`; populate the LRU for visible rows (prefetch page 0 on bind, and `load_more`/visible-range on scroll). Unblocks UAT and makes the whole app usable. Larger than a typical P4b task; ideally its own reviewed task.
  - **Path B (defer):** ship P4b as logic-complete + test-green, mark T14 UAT as blocked-by-PD-018, and schedule the grid-render-cache wiring as a dedicated follow-up (P4c or a hotfix phase).
- **Discovered:** P4b T7 implementation + spec review (2026-05-30); the T7 implementer flagged it and the spec reviewer confirmed it with the quoted `render_td` body and the zero-caller `page_for`.
- **Originating doc:** P3a grid delegate task (render_td placeholder); surfaced against `docs/plans/2026-05-30-dat0-p4b-plan.md` §T14 (UAT gate).
- **Last touched:** 2026-05-31 (closed via Path A — render-cache wiring + 3 deferred triggers; see the `P4b: close PD-018 …` commit on `p4b-edit`)
- **Follow-up (not blocking):** the gpui-component `Table` also exposes a built-in `TableDelegate::context_menu(row_ix, …)` hook (row-aware) that we did NOT use — we mounted the decoupled `ContextMenuExt` on the body to reuse the existing `build_menu`. If a future polish pass wants the right-clicked ROW reflected in the menu (e.g. "Delete this row" vs. the current selection-based delete), switching to the delegate hook is the path. The P10 AccessKit / screen-reader exposure D-NNN from the T0 probe is T14's job, intentionally NOT filed here.

---

### PD-019 — Row-gutter click → select-row unwired (no `render_row_header` seam in gpui-component)

- **Status:** open
- **Severity:** low (column-select fully wired; select-row reachable programmatically + via keyboard)
- **Affected files:** `crates/dat0-app/src/window.rs` (`WorkspaceShell::select_row_at`)
- **Symptom:** P4c T13 wired header single-click → `select_column_at` (column-select) but could NOT
  provide a matching row-gutter click → `select_row_at`. The gpui-component `TableDelegate` trait
  (rev `0f0ab35`) has no `render_row_header` / row-gutter seam — `TableState::render_table_row` owns
  the row-number layout internally and does not call through to the delegate. There is therefore no
  hook point in the existing trait surface for attaching a click handler.
- **Two alternatives rejected:**
  - **(a) Subscribe to `TableEvent::SelectRow`:** this event fires on ANY row-body click, which would
    make every single-cell click select a whole row — clobbering the per-cell `SelectionModel`
    click wiring from T5 (P4b). Not usable.
  - **(b) Fake first column holding row numbers:** mounting a synthetic column at index 0 to act as
    a visual row-number gutter would corrupt the `col_ix` argument passed to `render_td` and
    `render_th` for all real data columns (+1 off everywhere), breaking column addressing end-to-end.
    Not usable.
- **Current state:** `WorkspaceShell::select_row_at` is fully implemented and correct; it is
  reachable programmatically (e.g., from tests and keyboard bindings) but has no UI click wiring.
  Column-select (`select_column_at`, wired in T13) is fully functional.
- **Two clean future paths:**
  - **(1) Upstream gpui-component PR:** add a `render_row_header(&self, row_ix, cx) ->
    impl IntoElement` hook to `TableDelegate` and call it from `render_table_row`. dat0 then
    implements the hook to return a labelled div with `on_click → select_row_at`.
  - **(2) Absolute-positioned per-row overlay:** paint a zero-width `div` at x=0 with `position:
    absolute`, height matching one row, its own `on_click`, iterated over the visible row range.
    Fragile (must track row height + scroll offset), but requires no upstream change.
- **Discovered:** P4c T13 implementation review (2026-06-01).
- **Originating doc:** `docs/plans/2026-05-31-dat0-p4c-plan.md` T13.
- **Last touched:** 2026-06-01.

---

### PD-020 — Inline-editor `Tab` → move-RIGHT advance unwired (gpui-component `Input` exposes no Tab event)

- **Status:** CLOSED — 2026-08-10 (Phase 6, GPUI→Dioxus migration). The deferral was a
  `gpui-component` limitation, not a dat0 one: a plain `<input>` surfaces Tab like any other key,
  so `crates/dat0-ui/src/components/grid/cell_editor.rs` commits and moves the active cell one
  column right on Tab, one left on Shift-Tab, clamped at the row's ends. Covered by
  `crates/dat0-ui/tests/grid_edit.rs::tab_commits_and_steps_right_closing_pd_020` /
  `shift_tab_steps_left` and `crates/dat0-ui/tests/cell_editor_nav.rs::tab_walks_the_row_and_stops_at_its_ends`.
- **Severity:** low (the `Enter` → move-DOWN advance + focus-on-mount are shipped; `Tab` still
  works as the input's own tab-stop, it just doesn't commit-and-advance one cell right).
- **Affected files:** `crates/dat0-app/src/grid/cell_editor.rs` (`CellEditor`),
  `crates/dat0-app/src/grid/edit_ops.rs` (`commit_cell_edit_and_advance` only handles
  `EditorAdvance::Down`).
- **Symptom:** P4c T14 added a real `FocusHandle` + focus-on-mount and wired `Enter` → commit + move
  the active cell DOWN one row + re-open the editor (spreadsheet semantics) by emitting
  `CellEditorEvent::CommitAndMove(value, EditorAdvance::Down)` on `InputEvent::PressEnter`. The
  matching `Tab` → commit + move RIGHT could NOT be wired: the gpui-component `Input` (rev `0f0ab35`)
  consumes Tab internally for tab-stop focus navigation and surfaces it to subscribers only as
  focus changes, NOT as an `InputEvent` variant — `InputEvent` is exactly `{ Change, PressEnter,
  Focus, Blur }` (see `crates/ui/src/input/state.rs`). There is therefore no hook point on the
  existing event surface for a Tab-advance.
- **One alternative rejected:**
  - **Wrapper-level `on_key_down` intercept:** attaching a key handler to the `CellEditor` container
    (tracking its own `FocusHandle`) to catch Tab would require the wrapper to HOLD focus — but then
    keystrokes route to the wrapper, not the inner `InputState`, so typing into the field would land
    nowhere (focus contention). Shipping that would break the very focus-on-mount this task added.
    Not usable.
- **Current state:** `Enter` → down + focus-on-mount are fully wired and shipped; `EditorAdvance` is
  an enum (currently single-variant `Down`) so adding `Right` later is a one-line extension once an
  upstream Tab seam exists. `Esc` cancels (P4b). `Tab` falls through to the input's own behaviour.
- **One clean future path:**
  - **Upstream gpui-component PR:** add a `PressTab { shift: bool }` variant to `InputEvent` (emitted
    alongside the existing tab-stop handling) so dat0 can subscribe and emit
    `CommitAndMove(value, EditorAdvance::Right)` exactly like the `PressEnter` → `Down` path.
- **Discovered:** P4c T14 implementation (2026-06-01).
- **Originating doc:** `docs/plans/2026-05-31-dat0-p4c-plan.md` T14 (Step 3).
- **Last touched:** 2026-08-10.

---

### PD-021 — Banner host unmounted: `error_ux::PENDING` is never drained at runtime

- **Status:** CLOSED — 2026-06-06 (P6a T1)
- **Severity:** medium (no data loss; the operation succeeds — but the user gets
  zero on-screen confirmation/error feedback for exports and paste-rejects)
- **Affected files:**
  - `crates/dat0-app/src/error_ux/banner.rs` (`PENDING` queue, `push`,
    `drain_pending` — `drain_pending` has ZERO non-test callers)
  - `crates/dat0-app/src/window.rs` (`WorkspaceShell::run_export` pushes
    `Banner::info`/`Banner::error` on export done/fail; `WorkspaceShell::render`
    never drains)
  - `crates/dat0-app/src/grid/edit_ops.rs` (paste-reject pushes `Banner::error`)
  - `crates/dat0-app/src/boot.rs`, `import_progress.rs`, `session/mod.rs` (other
    producers that enqueue but are never surfaced)
- **Symptom:** `error_ux::push(banner)` appends a `Banner` to a global
  `static PENDING: Lazy<Mutex<Vec<Banner>>>`. The only code that calls
  `drain_pending()` is `#[cfg(test)]` (the `file_drop.rs` / `session/mod.rs` test
  modules, which drain to assert producers fired). No production render path
  drains the queue and paints the banners. Net effect at runtime:
  - **Export feedback is invisible.** T11's `run_export` pushes
    `Banner::info("export.done.title", <dest path>)` on success and
    `Banner::error("export.failed.title", …)` on failure (`window.rs:1065-1076`),
    but neither ever appears on screen — the COPY runs, the file lands on disk,
    and the user sees nothing.
  - **The pre-existing P4b paste-reject banner is also invisible** — the
    coerce-or-skip reject count pushed from `grid/edit_ops.rs` never shows.
  - Boot-time and forward-incompat banners (`boot.rs`, `session/mod.rs`) share
    the same dead-letter fate (the original P2 PD-007 design always intended a
    P7 render layer to drain on first window open; that drain was never wired).
- **Discovered:** P4c T11 code-quality review (2026-06-01) — surfaced while
  reviewing the export completion path; the success/error banners were pushed but
  could not be observed in a smoke launch.
- **Suggested fix:** Mount a banner host in the `WorkspaceShell` render tree: on
  each render (or via a short GPUI timer / an explicit notify after producers
  run), call `error_ux::drain_pending()` into a `Vec<Banner>` field on
  `WorkspaceShell`, render a stack of dismissible banner chips (the
  `error_ux::Banner` already carries `kind`/`title`/`body`/`action`), and clear on
  dismiss. This is the single drain site the P2 design (PD-007) deferred to "the
  render layer (P7+)"; P4c export UX is the first feature that makes its absence
  user-visible.
- **Originating doc:** `docs/plans/2026-05-31-dat0-p4c-plan.md` T11;
  `docs/deferrals.md` PD-007 (the queue's original "render layer drains later"
  intent).
- **Closed by:** P6a T1 commit `13fdc6e` (`feat(p6a): mount banner host draining
  error_ux PENDING (closes PD-021)`). `error_ux::banner::merge_pending(&mut
  self.banners)` runs at the top of `WorkspaceShell::render`, draining the global
  `PENDING` into a per-window `banners: Vec<Banner>` field; a host strip
  (`render_banner` per banner, kind-accented left border) is mounted as the first
  child of the shell root, before the tab strip. Test
  `merge_pending_moves_global_into_live_vec` in `error_ux/banner.rs`.
- **Last touched:** 2026-06-06.

### PD-022 — Inspector profile not refreshed on undo/redo or SQL-console grid-bind

- **Status:** closed
- **Severity:** low (transient visual staleness only; the next forward mutation,
  table reselection, or mode toggle re-profiles correctly — no wrong data is
  persisted, and the inspector did not auto-refresh at all before P6a)
- **Affected files:**
  - `crates/dat0-app/src/actions/view_actions.rs` (`dispatch_undo` / `dispatch_redo`
    apply their `ViewChange` via `spawn_view_change` → `apply_view_change`)
  - `crates/dat0-app/src/window.rs` (`apply_view_change` has no inspector hook;
    the SQL-console `Bound(ds)` rebind in `MainGrid` mode also lands here)
  - `crates/dat0-app/src/grid/edit_ops.rs` (forward mutations DO refresh, via
    `on_table_mutated_structural` / `route_change` — the asymmetry is the gap)
- **Symptom:** P6a T12 wired the hybrid write path so that forward data/schema
  mutations (cell edit, paste, cut, delete, fill, set-null/value, column
  rename/reorder/delete, transform-apply) invalidate + re-profile the inspected
  table. Undo/redo and SQL-console grid-binds rebind the grid through
  `apply_view_change`, which T12 did not hook, so the inspector profile (and its
  inline charts/dependents) can show pre-undo state until the next forward event.
- **Discovered:** P6a T12 code-quality + spec review (2026-06-06).
- **Suggested fix:** Add a single inspector-refresh seam at the rebind convergence
  point — either call `on_table_mutated_structural(target, cx)` from
  `apply_view_change` (gated to data/schema-affecting changes to avoid a redundant
  re-SUMMARIZE on pure display sort/filter), or introduce an `on_rebind_complete`
  callback that both the undo/redo and SQL-bind paths already funnel through.
- **Originating doc:** `docs/plans/2026-06-06-dat0-p6a-plan.md` T12.
- **Closed by:** P6b — `docs/plans/2026-06-06-dat0-p6b-plan.md`, Task 7
  (2026-06-06). Fix: `apply_view_change` now, when an inspector target is set,
  calls `recompute_lineage()` (rebuilds the lineage chain from `catalog_tables`
  + `sql_parents` for the current target) and `on_table_mutated_structural(target,
  cx)` (bumps the profile epoch + re-profiles via SUMMARIZE + notifies) — so
  undo/redo and SQL-console grid-binds refresh the Inspector profile, inline
  charts, and lineage chain at the rebind convergence point, matching the
  forward-mutation behavior wired in P6a T12. The target `String` is cloned
  before the calls so the `&mut self` methods don't conflict with the borrow of
  `self.inspector`. Verified by compile + green suite (`dat0-engine` + `dat0-app`
  pass, clippy/fmt clean); behavioral confirmation is owed in the P6b UAT
  (undo an edit → Inspector profile + chain update at the rebind point).
- **Last touched:** 2026-06-06.

---

## How to add an entry

1. Grab the next ID in sequence (`D-NNN` for deferrals, `PD-NNN` for plan defects).
2. Add a row to the at-a-glance table at top.
3. Add a full entry under the appropriate section, following the field schema
   above. Keep sections in numeric order.
4. Cross-link the originating phase plan and (if applicable) the spec exit
   criterion the deferral closes.
5. Update `Last touched:` to today's date in YYYY-MM-DD form.

## How to close an entry

1. Set `Status: closed`.
2. Append a `**Closed by:**` line citing the commit SHA, PR number, and phase
   retro that documented completion.
3. Move the at-a-glance row to a "Closed" sub-table at the bottom of each
   section once the count of closed entries grows beyond ~5; until then keep
   them inline so closure is visible at a glance.
