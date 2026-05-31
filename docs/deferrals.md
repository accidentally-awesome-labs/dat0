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
| D-003 | Sparkle Objective-C `SUUpdater` bridge             | open | P1   | P10    |
| D-004 | AppImageUpdate subprocess invocation               | open | P1   | P10    |
| D-005 | Linux Secret Service "setup banner" UX             | open | P1   | TBD    |
| D-006 | macOS x86_64 (Intel) CI matrix coverage            | open | P1   | TBD    |
| D-007 | MotherDuck ATTACH end-to-end                       | open | P2   | P5     |
| D-008 | Cancellation-token wiring through `QueryEngine` trait | open | P2 | P5     |
| D-009 | Bundle `sqlite_scanner` static when duckdb-rs exposes a feature | open | P2 | TBD |
| D-010 | Non-UTF-8 file encoding handling                   | open | P2   | TBD    |
| D-011 | Remove `__debug_query_scalar` test-only helper     | closed | P2   | P3a    |
| D-012 | Engine catalog `TableInfo` synthesis (origin + schema) | closed | P2   | P3a    |
| D-013 | Self-hosted macOS CI runner (cut hosted macos-14 10× billing) | open | P2 | TBD |
| D-014 | Memory Budget Settings section | open | P3b | P3c / P9c |

## At-a-glance — Plan defects

| ID     | Title                                                              | Status | Severity |
|--------|--------------------------------------------------------------------|--------|----------|
| PD-001 | tracing EnvFilter directive `dat0=debug` doesn't match dat0 crates | open   | low      |
| PD-002 | Settings store atomic-write missing `fsync` before rename          | open   | low      |
| PD-003 | cargo-about NOTICE output not deterministic across host platforms  | open   | low      |
| PD-004 | Linux Secret Service backend not reachable from CI keychain tests  | open   | low      |
| PD-011 | P3b plan §3.7 ambiguity rule references sniff outputs that don't exist: no candidate-delimiter scores, no encoding column, no per-column confidence in `sniff_csv` | open | low |
| PD-012 | `NYC_TAXI_SHA256 = "FILL_AT_T8"` — release asset not yet uploaded, so the fetch path always fails the checksum check at runtime | open | low |
| PD-013 | P4a T0 plan-snippet drifts: `dat0-fixtures` assumed to be a lib crate; `dat0-engine` assumed to have a `benches/` dir + criterion dev-dep; plan snippet pinned `criterion = "0.5"` instead of using workspace inheritance | closed | low |
| PD-014 | P4a design §3 used `#[serde(untagged)]` on `FilterValue` + `Scalar`, causing Str/Date/Timestamp collisions and FilterValue::None/Scalar::Null collision; reworked to tagged wire shapes | closed | low |
| PD-015 | P4a plan T2–T15 snippets still use the pre-PD-014 tuple form `FilterValue::Scalar(...)` + `FilterValue::List(vec![...])` (~57 occurrences); implementers must read variants as struct form `FilterValue::Scalar { value }` + `FilterValue::List { values }` | closed | low |
| PD-016 | P4a UI-click → ViewChange wirings unowned by plan T13: funnel-click → open popover, popover `Outcome::Apply` → `vm.apply`, sort-zone-click → `vm.set_sort`. T7/T9/T10b/T12 implementers each commented "T13 wires" but plan T13 Steps 1-4 cover only keybind undo/redo + supersede-cancel test. T14 E2E either pulls in the missing glue or tests via direct VM API; P4b/c may inherit if not closed at T15. | open | medium |
| PD-017 | P4b T3 plan premise wrong: it assumed `register_file` "finalizes a CTAS import", but `register_file` emits `CREATE OR REPLACE VIEW … AS SELECT * FROM read_csv/json/parquet(…)` — a VIEW, which cannot be `ALTER TABLE`-d, so the eager `__dat0_rowid` surrogate only lands via `create_table` (base tables). The app imports files exclusively via `register_file` (`file_drop.rs:133`), so imported grids are VIEWs with no `__dat0_rowid`, and the P4b edit/delete overlay (`WHERE __dat0_rowid NOT IN …`, `CASE WHEN __dat0_rowid = …`) references a non-existent column → edit/delete fail on real imports. T3 engine work is correct + complete; resolution is app-side (materialize imports to base tables, or back-fill via `ensure_rowid` on first bind). **Closed via Path A:** new `QueryEngine::register_file_as_table` materializes imports into rowid-bearing base tables (reusing all P3b sniffing); `file_drop.rs` now calls it. | closed | high |
| PD-018 | Pre-existing (P3a-era) grid-render gap surfaced during P4b T7: `GridTableDelegate::render_td` (`grid/mod.rs`) renders the em-dash placeholder for EVERY cell — it never calls `render_cell` or `page_for`, and `page_for` (the only method that populates the page LRU from DuckDB) has ZERO production callers (no `load_more`/visible-range/prefetch). So in the running app the grid shows `—` and the cache is empty. P4b's cache-only reads (`cell_display`/`row_key`/`column_arrow_type`) resolve nothing on screen, so copy reads empty strings and paste/cut/edit skip every cell. P4b edit/select/clipboard LOGIC is correct + fully test-green (engine round-trips), but the headline T14 manual Excel/Sheets UAT is BLOCKED until the paged-render cache is wired (render_td → real values via the page LRU + prefetch visible page on bind). Out of every P4b plan task's scope. | open | high |

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

- **Status:** open
- **Deferred from:** P1 (T15)
- **Target phase:** P10
- **Reason:** Cross-language Objective-C bridge needs notarized .app bundle +
  release pipeline before it's testable end-to-end. P1 doesn't sign or notarize.
- **What P1 ships:** HTTP GET smoke against the appcast URL (satisfies the
  exit criterion "makes a network call"); EdDSA public key bundled at build
  time via `include_str!`; `objc`/`cocoa` deps deliberately deferred.
- **What target phase delivers:** Real `SUUpdater` instantiation, appcast
  parsing, signature verification, in-app update prompt UI, restart flow.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Closes:** spec §21.2 P10 exit — "Sparkle 'Check for Updates' finds a test update + applies it"
- **Last touched:** 2026-04-28

### D-004 — AppImageUpdate subprocess invocation

- **Status:** open
- **Deferred from:** P1 (T15)
- **Target phase:** P10
- **Reason:** Real subprocess invocation of `appimageupdatetool` requires a
  packaged AppImage; P1 builds plain Linux binaries.
- **What P1 ships:** `AppImageUpdater` stub conforming to the `Updater` trait;
  logs a placeholder message.
- **What target phase delivers:** Subprocess wiring, progress reporting,
  restart flow.
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` §"Risks & Caveats"
- **Last touched:** 2026-04-28

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
- **Last touched:** 2026-04-28

### D-007 — MotherDuck ATTACH end-to-end

- **Status:** open
- **Deferred from:** P2
- **Target phase:** P5 (SQL Console)
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
  `md:` lands in P5).
- **Last touched:** 2026-04-28

### D-008 — Cancellation-token wiring through `QueryEngine` trait

- **Status:** open
- **Deferred from:** P2
- **Target phase:** P5 (SQL Console)
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
- **Last touched:** 2026-04-28

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
- **Last touched:** 2026-05-16

### D-013 — Self-hosted macOS CI runner (cut hosted macos-14 10× billing)

- **Status:** open
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
- **Last touched:** 2026-05-14

### D-014 — Memory Budget Settings section

- **Status:** open
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
- **Originating doc:** `docs/specs/2026-05-25-dat0-p3b-ux-polish-design.md` §7.
- **Last touched:** 2026-05-25.

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

- **Status:** open (partial — session.json side closed; settings.toml side still open)
- **Severity:** low (durability, not correctness)
- **Affected files:**
  - `crates/dat0-app/src/settings/store.rs:save` — **still open** (settings.toml path)
  - `crates/dat0-app/src/session/store.rs:save` — **closed by T8** (session.json path)
- **Symptom:** Comment claims "Atomic write: write to .tmp, fsync, rename." but
  the code uses `std::fs::write` (which only writes + closes, no fsync). On
  macOS the rename is atomic at the directory-entry level, but the new file's
  data blocks may not be durable before the rename completes if the kernel
  hasn't flushed the page cache. On power loss the user could see a
  zero-length `settings.toml`.
- **Discovered:** P1 T8 implementer report
- **Originating doc:** `docs/plans/2026-04-26-dat0-p1-foundation-plan.md` Step 8.3
- **Suggested fix (for remaining settings.toml path):**
  ```rust
  let tmp = self.path.with_extension("toml.tmp");
  let mut f = std::fs::OpenOptions::new()
      .write(true).create(true).truncate(true).open(&tmp)?;
  use std::io::Write;
  f.write_all(serialized.as_bytes())?;
  f.sync_all()?;
  drop(f);
  std::fs::rename(&tmp, &self.path)?;
  // Optional: fsync the parent directory for durable rename:
  // std::fs::File::open(self.path.parent().unwrap_or(Path::new("/")))?.sync_all()?;
  ```
- **Acceptable when:** dat0 begins storing recoverable state in settings (e.g.,
  workspace pointers a user would lose if the file zeroed). Until then the cost
  of a corrupt settings file is "user re-enters preferences," which is low.
- **Session.json side closed by:** commit `e295cd5` (T8 post-review, P4a) — `session/store.rs` now uses `OpenOptions` + `write_all` + `sync_all` + `rename` + optional parent-dir fsync. Only `settings/store.rs` remains.
- **Last touched:** 2026-05-29

---

### PD-003 — cargo-about NOTICE output not deterministic across host platforms

- **Status:** open
- **Severity:** low (warn-only CI gate, not a blocker)
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
- **Last touched:** 2026-04-28

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
- **Last touched:** 2026-04-28

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

- **Status:** open
- **Severity:** medium (P4a is functionally incomplete on the UI-click path: funnel + sort-zone clicks log but don't actually trigger ViewChanges; only the keybind undo/redo path is wired end-to-end)
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
- **Last touched:** 2026-05-29

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

- **Status:** open
- **Severity:** high (P4b's edit/select/clipboard logic is correct + fully test-green, but the running app cannot demonstrate it: the grid shows em-dashes and copy/paste/edit have no real cell values to act on; the headline T14 Excel/Sheets UAT gate is blocked)
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
- **Last touched:** 2026-05-30

---


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
