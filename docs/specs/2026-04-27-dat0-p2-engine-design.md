# dat0 P2 — Engine Layer — Design Spec

**Date:** 2026-04-27
**Phase:** P2 (Engine layer, ~3 weeks per spec §21.6)
**Entry:** P1 Foundation merged to `main` (PR #1, merge `6b96122`)
**Authoritative source:** `docs/specs/2026-04-26-dat0-design.md` §6 + §21.2 P2.
This document is the brainstorm-output spec consumed by `docs/plans/2026-04-27-dat0-p2-engine-plan.md`.

---

## 1. Goal

Stand up the `dat0-engine` crate behind the `QueryEngine` trait surface from §6.1
of the design spec, backed by a `DuckDBEngine` implementation that:

- Registers files in CSV, TSV, JSON, JSONL, NDJSON, and Parquet formats.
- Executes SQL via materialized, paged, and streaming Arrow paths.
- Supports `ATTACH`/`DETACH` for `sqlite:`-prefix DSNs (MotherDuck deferred — see D-007).
- Applies a per-window memory budget at connection init.
- Maintains a forward-only schema-migration runner.
- Cleanly handles N concurrent engine instances in one process (multi-window safe).

Non-goals: no UI integration (P3+), no `.dat0` package format (P9), no MotherDuck
end-to-end (deferred — D-007), no public cancellation-token API on the trait (deferred — D-008).

## 2. Architecture decisions (locked from brainstorm)

### 2.1 Sync→async bridge: `spawn_blocking` + interrupt handle

`duckdb-rs` is synchronous. The `QueryEngine` trait is `async`. Bridge via
`tokio::task::spawn_blocking` per call. Engine struct holds:

```rust
pub struct DuckDBEngine {
    conn: Arc<Mutex<duckdb::Connection>>,
    interrupt: InterruptHandle,  // cheap-clone handle (or Arc-wrapped if not natively Clone; T0 confirms)
    budget: MemoryBudget,
    scratch_path: PathBuf,
    status: Arc<RwLock<EngineStatus>>,
}
```

Where `InterruptHandle` is whatever duckdb-rs 1.4.x exposes via
`Connection::interrupt_handle()` (exact spelling and `Clone`/`Send` guarantees
verified at T0; do not assume `Arc<>` wrapping is needed — most handle types in
this idiom are already cheaply clonable).

Rationale (locked): see brainstorm Q1. The interrupt handle is the cancel
mechanism; `spawn_blocking` is the sync wrapper; pool/dedicated-worker rejected
because DuckDB connections are single-threaded for execution and the `Mutex`
serializes equivalently with less code. Multi-window safety: each engine owns
its own `Connection`, `Mutex`, interrupt handle, scratch DB file — no globals.

**Streaming variant:** `execute_streaming` returns
`Pin<Box<dyn Stream<Item = Result<RecordBatch, EngineError>> + Send>>`. A
`spawn_blocking` worker pulls batches from duckdb-rs's Arrow batch iterator
(T0 confirms exact symbol — likely `Statement::query_arrow` returning an
iterator of `RecordBatch` per the canonical 1.4.x API; the design-spec §6.2
mention of `Connection::stream_arrow` may reflect older naming) and pushes onto
`tokio::sync::mpsc::channel(1)` — channel back-pressure pauses the producer
when the consumer (P3 grid) lags. Default capacity = 1; tune only if P3 bench
shows starvation.

**Arrow type source:** all Arrow types crossing the engine→consumer boundary
come from duckdb-rs's re-exported `duckdb::arrow`. There is **no separate
`arrow = "..."` workspace dependency.** Pinning two Arrow versions in
`Cargo.toml` is wrong — `RecordBatch` from a standalone `arrow` crate would
be a nominally distinct type from `duckdb::arrow::record_batch::RecordBatch`
and would not unify in trait bounds at the P3 grid boundary.

### 2.2 Cancellation: internal `interrupt()` only

P2 ships `Engine::interrupt(&self)` as a public method. Engine internals support
cooperative cancel: callers can spawn a sibling task that calls `interrupt()`
when their cancel signal fires; in-flight `spawn_blocking` thread sees `Err`
from DuckDB and propagates. Trait method signatures stay verbatim per spec §6.1.
Full `CancellationToken` plumbing through trait methods deferred (D-008).

### 2.3 Connection model: one connection per engine

Per spec §6.5. No pool. ATTACH state is per-connection — pool would re-introduce
ATTACH-divergence bugs.

### 2.4 Memory budget: per-engine, set at construction

Caller computes the per-window budget value and passes it into
`DuckDBEngine::new`. Engine sets `PRAGMA memory_limit='${budget}'` and
`PRAGMA threads=${available_cores - 1}` at init. **Engine does not contain any
global window-count logic.** That lives in the eventual app-side window
registry (P3 territory). For P2 tests, callers pass a fixed budget.

### 2.5 Extension distribution: lazy-load with documented offline path; static bundle on a contingency

**Locked path: lazy-load via `INSTALL sqlite_scanner; LOAD sqlite_scanner;`.**
The duckdb-rs published feature set as of this spec authoring does not expose
a `sqlite_scanner` Cargo feature. By analogy with ICU (which the duckdb-rs
README explicitly cites as un-bundleable due to the crates.io 10MB size limit),
sqlite_scanner ships as a runtime-loaded extension.

Concrete locked design:
1. Extension install + load hoisted to `dat0-app` **boot path** — single-shot
   before any window opens — to avoid two-window install races against
   `~/.duckdb/extensions/`.
2. Banner UX during download via P1's `Banner` primitive (first consumer of it).
3. `HTTP_PROXY` / `HTTPS_PROXY` env var documentation in `CONTRIBUTING.md`
   for users behind enterprise proxies.
4. Offline-extensions tarball shipped as release asset (P10) for air-gapped
   users — drop into `~/.duckdb/extensions/<duckdb_version>/<platform>/`.
5. Engine `init()` per instance does **`LOAD sqlite_scanner;` only** (extension
   already installed by boot path); no INSTALL inside engine.

**Core extensions (CSV / JSON / Parquet):**
- CSV is built into DuckDB core — no Cargo feature required.
- JSON and Parquet ship under the duckdb-rs `bundled` Cargo feature (their
  static linking is gated behind that feature). Workspace `dat0-engine`
  enables `features = ["bundled", "json", "parquet"]` on its duckdb dependency.
- All three are autoloaded by DuckDB at `read_csv` / `read_json` / `read_parquet`
  call sites — no extra LOAD required.

**Contingency: static bundle for sqlite_scanner.**
T0 spike re-checks duckdb-rs's current feature set. If a `sqlite_scanner`
Cargo feature has landed since this spec was authored, the engine enables it
and the boot-path install logic becomes a no-op. This contingency is tracked
unconditionally as `D-009`:

> **D-009 — bundle `sqlite_scanner` static when duckdb-rs exposes a feature**
> opens with the spec; closes when the bundle path is implemented (which may
> never happen, in which case D-009 stays open as documented intent).

Rationale: dat0's regulated-industry-analyst persona is disproportionately
behind air-gapped or proxied networks. The boot-path install + offline tarball
escape hatch satisfies the "files at scale, local, no cloud" pillar even
without static bundling, with one acceptable cost (a one-time install banner
on first launch online, or manual extension drop on air-gapped install).

### 2.6 Migration scaffold: minimal real runner

Forward-only, append-only `MIGRATIONS: &[Migration]` slice in
`dat0_engine::migrations`. Engine `init()` calls `apply_migrations(&conn)`
inside a transaction per migration. Records applied versions in
`__dat0_meta_migrations`. Skip migrations on ATTACH'd DBs (sqlite, future
MotherDuck) — only the primary connection is owned by us.

**P2 scope of the migration runner: per-engine scratch DB only.** Each
`DuckDBEngine` instance opens its own scratch DB under a caller-supplied unique
path; no two engines in P2 share the same DB file. The first-launch
"two engines race to apply migration 1" failure mode therefore does not exist
in P2 because there is no shared file to race over.

Workspace DBs (the per-`.dat0`-package shared file that P3+ scratch persistence
and P9 package format introduce) are **out of scope for the P2 migration runner**.
When P3 introduces a workspace DB shared across windows, the migration runner's
multi-engine concurrent-open behavior must be revisited:
either (a) serialize migration application via a process-wide `Mutex` keyed on
DB path, or (b) require migrations to be idempotent (each `up` body uses
`CREATE TABLE IF NOT EXISTS`, `INSERT OR IGNORE`, etc.) and accept that
losers of the race no-op cleanly. Track as a P3 entry-time review item, not
a P2 deferral (P2 doesn't ship a workspace DB at all).

```rust
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub up: fn(&duckdb::Connection) -> Result<()>,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration { version: 1, name: "init", up: m001_init },
];
```

Initial migration `m001_init` creates `__dat0_meta` (`key`/`value` table) and
seeds `dat0_workspace_version = '1'`. Schema serves both as a runner-target and
as the "what version is this DB" probe (reused by P9 .dat0 import/export and
P11 distribution checksum).

Tests: fresh apply, idempotent re-apply, partial-version replay, failed
migration rolls back.

Telemetry: migration runner emits `tracing` event on each apply with
`{from, to, name, duration_ms, outcome}` — wires through P1's Sentry/GlitchTip
instrumentation for free observability.

Future schema PRs are append-only: add a `Migration` to the slice, write `up`,
ship. **Never edit a shipped migration.** Documented in `CONTRIBUTING.md` at P11.

### 2.7 Test fixtures: hybrid (generator + tiny in-repo)

```
tests/fixtures/
  small/              # in repo, ~few KB total
    basic.csv         # happy-path
    edge_cases.csv    # quoted commas, BOM, NULLs, mixed types, CRLF/LF
    simple.json
    simple.jsonl
    simple.ndjson
    simple.parquet
    simple.sqlite
  large/              # gitignored, generated
    generated.csv     # 1 GB
    generated.parquet # 500 MB
    generated.sqlite  # 100 MB

crates/dat0-fixtures/  # workspace bin member
```

`dat0-fixtures` generator: deterministic from `--seed`. Schema = ~10 columns
mixed types (int, float, string, date, bool, NULL). CSV via direct write
(~100 MB/s); Parquet via `arrow::arrow_writer`; SQLite via `rusqlite`.

CI handling:
```yaml
- name: Cache fixtures
  uses: actions/cache@v4
  with:
    path: tests/fixtures/large/
    key: fixtures-${{ hashFiles('crates/dat0-fixtures/**') }}-seed-42
- name: Generate fixtures (if cache miss)
  run: cargo run -p dat0-fixtures --release -- --out tests/fixtures/large --seed 42
```

Cold-cache regen ≤ 1 min; hot ≤ 10 s. `.gitignore` covers `tests/fixtures/large/`.
Tests requiring large fixtures use `#[ignore = "requires generated fixtures"]`
so `cargo test` works without generation; CI runs them via
`cargo test -- --include-ignored` once the cache or fresh generation has
populated `tests/fixtures/large/`. Feature-flag gating rejected to keep the
test surface uniform.

### 2.8 Trait surface — verbatim per design spec §6.1

```rust
trait QueryEngine: Send + Sync {
    async fn init(&self) -> Result<()>;
    async fn close(&self) -> Result<()>;
    fn status(&self) -> EngineStatus;

    async fn register_file(&self, path: &Path, opts: RegisterOpts) -> Result<TableInfo>;
    async fn create_table(&self, name: &str, sql: &str, origin: DerivedOrigin) -> Result<TableInfo>;
    async fn drop_table(&self, name: &str, schema: Option<&str>) -> Result<()>;
    async fn rename_table(&self, old: &str, new: &str, schema: Option<&str>) -> Result<()>;

    async fn execute(&self, sql: &str) -> Result<QueryResult>;
    async fn execute_paged(&self, sql: &str, offset: u64, limit: u64) -> Result<PagedQueryResult>;
    async fn execute_streaming(&self, sql: &str) -> Result<ArrowRecordBatchStream>;

    async fn describe_table(&self, name: &str, schema: Option<&str>) -> Result<Vec<ColumnInfo>>;
    async fn get_tables(&self) -> Result<Vec<TableInfo>>;

    async fn export_table(&self, name: &str, format: ExportFormat) -> Result<Vec<u8>>;

    async fn attach(&self, dsn: &str, alias: &str, opts: AttachOpts) -> Result<()>;
    async fn detach(&self, alias: &str) -> Result<()>;
}
```

`Engine::interrupt(&self)` is added as a public method **outside the trait** in
P2. It moves into the trait when D-008 closes.

### 2.9 Type surface

```rust
pub enum EngineStatus { Initializing, Ready, Closing, Closed, Failed(String) }
// Transition contract:
//   new()              -> Initializing
//   init() success     -> Ready
//   init() failure     -> Failed(reason)
//   close() entry      -> Closing
//   close() complete   -> Closed (even if cleanup steps errored; errors are logged)
//   poisoned mutex     -> Failed(reason)  (transitioned on first observation)
// In-flight query errors do NOT transition status. The engine remains Ready
// until close() is invoked or a panic poisons the connection mutex.

pub struct RegisterOpts {
    pub format: FileFormat,        // detected or explicit (CSV, TSV, JSON, JSONL, NDJSON, Parquet)
    pub delimiter: Option<char>,
    pub quote_char: Option<char>,
    pub escape_char: Option<char>,
    pub has_header: Option<bool>,  // None = auto-detect
    pub type_overrides: HashMap<String, String>, // column_name -> DuckDB type literal
    pub sample_rows: Option<u32>,  // None = DuckDB default; Some(0) is invalid
}
// Defaults: every Option::None defers to DuckDB's read_csv_auto behavior;
// type_overrides empty = no overrides. type_overrides keyed on a column name
// that doesn't exist in the file surfaces as DuckDB's native binder error
// (no silent no-op). P3 import wizard binds UI to these fields.
//
// `encoding` is intentionally absent from P2: DuckDB's read_csv has no
// encoding parameter (assumes UTF-8). Non-UTF-8 input handling deferred —
// see D-010.

pub enum FileFormat { Csv, Tsv, Json, Jsonl, Ndjson, Parquet }

pub struct TableInfo {
    pub name: String,
    pub schema: String,            // "main" by default
    pub columns: Vec<ColumnInfo>,
    pub row_count_estimate: Option<u64>,
    pub origin: TableOrigin,
}

pub enum TableOrigin { File(PathBuf), Derived(DerivedOrigin), Attached { alias: String, source: String } }
pub enum DerivedOrigin { Sql(String), Transform { parent: String, ops: Vec<String> } }

pub struct ColumnInfo { pub name: String, pub data_type: String, pub nullable: bool }

pub struct QueryResult { pub columns: Vec<ColumnInfo>, pub batches: Vec<RecordBatch> }
pub struct PagedQueryResult { pub total_rows: u64, pub offset: u64, pub batches: Vec<RecordBatch> }

pub type ArrowRecordBatchStream =
    Pin<Box<dyn Stream<Item = Result<RecordBatch, EngineError>> + Send>>;

pub enum ExportFormat { Csv, Json, Parquet }

pub struct AttachOpts {
    pub read_only: bool,           // default true for sqlite/MotherDuck
    pub schema_filter: Option<Vec<String>>,
}
```

### 2.10 Error surface

```rust
#[derive(thiserror::Error, Debug)]
pub enum EngineError {
    #[error("DuckDB error: {0}")]
    DuckDb(#[from] duckdb::Error),
    #[error("Arrow error: {0}")]
    Arrow(#[from] duckdb::arrow::error::ArrowError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid path: {0}")]
    InvalidPath(PathBuf),
    #[error("Unsupported file format: {0}")]
    UnsupportedFormat(String),
    #[error("Unknown ATTACH scheme: {0}; supported: sqlite:")]
    UnknownAttachScheme(String),
    #[error("Feature not yet implemented: {feature}")]
    NotImplemented { feature: &'static str },
    #[error("Migration {version} ({name}) failed: {source}")]
    Migration { version: u32, name: String, #[source] source: duckdb::Error },
    #[error("Query interrupted")]
    Interrupted,
    #[error("Engine is closed or closing; new operations rejected")]
    EngineClosed,
    #[error("Engine connection mutex poisoned (prior panic in worker thread)")]
    EnginePoisoned,
}
```

- `Interrupted` surfaces from any `spawn_blocking` query when an external
  `Engine::interrupt()` call has fired. Discriminator unused in P2 (no caller
  hooks `interrupt()` yet) but built so P5 doesn't have to retrofit.
- `EngineClosed` returns when any trait method is called after `close()` has
  begun the shutdown sequence (status is `Closing` or `Closed`). Avoids
  callers seeing opaque `DuckDb(...)` errors after teardown.
- `EnginePoisoned` returns when the connection `Mutex` is poisoned by a
  prior panic in a `spawn_blocking` worker. Engine must transition to
  `EngineStatus::Failed(reason)` on first observation.
- `Migration::source` is `duckdb::Error` (not `Box<dyn Error>`) — every
  migration `up` calls only `duckdb::Connection` methods. Keeps the source
  matchable.

## 3. Multi-window safety contract

Tests must lock these in:

1. **Two `DuckDBEngine` instances in one process.** Each opens its own scratch
   DB tempdir, registers a different file, executes streaming queries
   concurrently. Verify: no cross-talk in results, both engines complete.
2. **`Engine::interrupt` isolation.** Engine A starts a long query (e.g.,
   `SELECT count(*) FROM big_csv` over the 1 GB fixture); engine B starts a
   short query; A is interrupted from a sibling task; B completes unaffected;
   A's awaited future returns `Err(Interrupted)`.
3. **Same-file concurrent `register_file`.** Both engines register the same
   small CSV; both succeed; queries against each engine's view return identical
   results.
4. **Per-engine memory budget independence.** Engine A and B have different
   budgets passed at construction; verify `PRAGMA memory_limit;` returns the
   correct value on each.

## 4. Out of scope (P2 → later phases)

- **MotherDuck ATTACH end-to-end** — D-007. P2 ATTACH parses DSN prefix; `md:`
  returns `EngineError::NotImplemented { feature: "MotherDuck" }`. Token store,
  motherduck extension, integration tests deferred to P5.
- **`CancellationToken` parameter on trait methods** — D-008. P2 ships
  `Engine::interrupt()` only. Trait amendment lands at P5 with SQL Console.
- **Editable settings UI for memory budget override** — already deferred D-001 to P3.
- **Live-applied memory-budget changes mid-session** — spec §6.3 explicitly v1.x.
- **Streaming `export_table` for files larger than RAM** — flag as risk; revisit
  in P4 when file dialog ships.
- **Query progress reporting** — P5 SQL Console concern.
- **Saved queries / history** — P5.
- **Per-query timing chip in status bar** — P5 SQL Console.
- **Catalog UI surfaces** — P3 (engine returns `Vec<TableInfo>`; UI shows it).
- **Import wizard UX** — P3. P2 builds full `RegisterOpts` field set; P3 binds widgets to existing fields.
- **`.dat0` package format** — P9.

## 5. Plan shape (~19–20 tasks)

Confirmed during brainstorm Q5; T11 split per code-review I-5. Hybrid
granularity: combined-verify on trivially mechanical tasks; full two-stage
review on tasks touching duckdb-rs / Arrow APIs or with cross-cutting
integration risk.

```
T0   — duckdb-rs + arrow API research spike (MUST run first; downstream
        tasks consume the output)                              [research, full]
T1   — dat0-engine skeleton: Cargo.toml deps (with bundled+json+parquet
        features) + types + errors + trait + stub impl         [combined-verify]
T2   — connection bootstrap (PRAGMAs, LOAD sqlite_scanner, migration
        runner integration)                                    [full review]
T3   — migrations module + initial migration + unit tests      [full review]
T4   — register_file CSV/TSV                                   [full review]
T5   — register_file JSON/JSONL/NDJSON                         [full review]
T6   — register_file Parquet                                   [full review]
T7   — execute (materialized)                                  [full review]
T8   — execute_paged + execute_streaming + mpsc back-pressure  [full review]
T9   — catalog ops (describe / get_tables / create / drop /
        rename)                                                [combined-verify]
T10  — export_table                                            [combined-verify]
T11a — attach/detach DSN dispatch + EngineError::NotImplemented
        for `md:` prefix                                       [full review]
T11b — sqlite_scanner end-to-end ATTACH path (assumes T14 boot
        installed extension)                                   [full review]
T12  — dat0-fixtures crate (generator; uses DuckDB COPY...TO
        for Parquet + rusqlite for SQLite — no standalone
        arrow dep)                                             [full review]
T13  — CI: fixtures cache + generation step                    [combined-verify]
T14  — extension bootstrap in dat0-app boot path (INSTALL
        sqlite_scanner once; banner UX for first-run download) [full review]
T15  — integration test: multi-window concurrent engines       [full review]
T16  — integration test: interrupt + cancel isolation          [full review]
T17  — integration test: 1 GB CSV / 500 MB Parquet / 100 MB
        SQLite exit criteria                                   [full review]
T18  — P2 retro                                                [combined-verify]
```

Effort: ~3 weeks per spec §21.6, ~1 task/day average matching P1 cadence.
T14 is now a deterministic implementation task (locked path is lazy-load per
§2.5), not a conditional "no-op if static bundle wins" — the locked decision
removes that branch from the plan.

## 6. Exit criteria (verbatim from spec §21.2)

1. `engine.register_file` succeeds for a 1 GB CSV, returns Arrow stream.
2. Same for Parquet, JSON, JSONL, NDJSON.
3. `ATTACH 'sqlite:fixture.db'` exposes fixture's tables.
4. Memory budget pragma applied at connection init; observable via `PRAGMA memory_limit;`.
5. All format integration tests green against fixtures.
6. Migration scaffold runs at least one no-op migration cleanly.
7. Streaming Arrow batches verified zero-copy from engine to consumer
   (no JSON serialization in path).

Plus brainstorm-locked additional gates:

8. Two concurrent engines in one process operate without cross-talk (test #1 above).
9. `Engine::interrupt` isolates per-engine (test #2 above).
10. T0 spike output committed as `docs/internal/duckdb-arrow-api-notes.md`
    (mirrors P1 T0 → `gpui-api-notes.md` pattern).
11. All Arrow types crossing the engine→consumer boundary come from
    duckdb-rs's re-exported `duckdb::arrow`. The `arrow` crate is **not**
    a separate workspace dependency. T0 documents the correct import path
    and the duckdb-rs version pin in `docs/upstream-watch.md`.
12. `dat0-engine` enables duckdb-rs Cargo features `bundled`, `json`, `parquet`
    (CSV is core, no feature required). Pin recorded in
    `docs/upstream-watch.md`.

## 7. Deferrals + commitments to record at plan-write time

### New deferrals to add to `docs/deferrals.md`

**D-007 — MotherDuck ATTACH end-to-end**
- Deferred from: P2
- Target: P5 (SQL Console)
- What P2 ships: generic `attach()` parses DSN prefix; `sqlite:` end-to-end;
  `md:` returns `EngineError::NotImplemented { feature: "MotherDuck" }`.
- What target phase delivers: motherduck extension load, keychain
  `MotherDuckTokenStore` consumer, integration tests against a dev MotherDuck DB,
  per-query timing chip ("local: 38ms / md: 412ms" in status bar).
- Closes (partial): spec §6.5 entirely.
- Reason: motherduck Cargo feature support unverified at P2 entry; keychain
  primitive has no consumer until P5; integration testing requires MotherDuck
  dev DB credential; P2 spec exit only names `sqlite:`.

**D-008 — Cancellation-token wiring through `QueryEngine` trait**
- Deferred from: P2
- Target: P5
- What P2 ships: internal interrupt handle field on `DuckDBEngine`; public
  `Engine::interrupt(&self)` callable from sibling tasks; `EngineError::Interrupted`
  variant present.
- What target phase delivers: trait amendment to add `cancel: CancellationToken`
  parameter on `execute*` methods, automatic interrupt-on-drop semantics,
  structured cancellation propagation through streaming, Cmd+. UX in SQL Console.
- Reason: P2 has zero callers passing tokens — adding the parameter now would
  ship dead-weight ergonomics that can't be evaluated against a real call-site.
  P5 SQL Console is the first surface that needs `Cmd+.` → cancel propagation
  through a streaming query; trait shape is best evaluated against that real
  call-site rather than guessed twice. Engine internals already support
  cancellation today via `Engine::interrupt()` — trait amendment is signature
  ergonomics, not a behavioral change.

**D-009 — bundle `sqlite_scanner` static when duckdb-rs exposes a feature**
- Deferred from: P2 (opens unconditionally with the spec)
- Target: TBD — closes when duckdb-rs adds a `sqlite_scanner` Cargo feature
  for static linking, OR stays open as documented intent.
- What P2 ships: lazy-load via boot-path `INSTALL sqlite_scanner; LOAD
  sqlite_scanner;` (per §2.5).
- What target phase delivers: replaces boot-path INSTALL with a build-time
  link, eliminating the first-run download UX surface.
- Reason: duckdb-rs's published feature surface as of 2026-04-27 does not
  expose this feature. Opens unconditionally so we don't lose the intent if
  upstream adds it later. T0 spike's first action: re-check for the feature.

**D-010 — non-UTF-8 file encoding handling**
- Deferred from: P2
- Target: TBD (likely P3 import wizard or v1.x polish)
- What P2 ships: `RegisterOpts` has no `encoding` field. Files are assumed
  UTF-8 (matches DuckDB's `read_csv` default).
- What target phase delivers: either (a) Rust-side preconversion via
  `encoding_rs` for CSV inputs flagged as non-UTF-8 by chardet/heuristic,
  with a banner "We detected this file is encoded as <X>; do you want to
  convert?", or (b) explicit user override in the import wizard with a
  conversion preview.
- Reason: DuckDB's `read_csv` has no encoding parameter; supporting non-UTF-8
  cleanly requires either preconversion or a separate read path. Neither is
  a P2 concern; the engine's job is to expose what DuckDB supports natively.

### Commitments to write into the plan preamble

1. **`RegisterOpts` ships with full field set in P2** (delimiter, quote, escape,
   encoding, header detection, type overrides, sample rows). P3 import wizard
   binds UI to existing fields — no P3 plumbing churn.
2. **`export_table` returns `Vec<u8>` in P2.** P4 file-dialog UI wraps it.
   Streaming export deferred until file > RAM is a concrete concern.
3. **Tracing instrumentation: lightweight only.** `#[tracing::instrument(skip(self), fields(sql_len = sql.len()))]`
   on trait methods. Never log SQL text (potential PII; P1 Sentry redaction
   already configured to skip). Structured row-count/duration events deferred.
4. **Streaming back-pressure default `mpsc::channel(1)`.** Tune only if P3 1M-row
   scroll bench shows starvation.

### Plan-defects protocol (re-stated)

When implementer reports a plan defect, add as `PD-NNN` per existing protocol;
do not fix inline unless it blocks the current task.

## 8. Risks

- **duckdb-rs API drift at pinned version.** Mitigation: T0 spike isolates risk
  (mirrors P1 T0 pattern). Plan downstream tasks consume T0's notes file.
- **Arrow type-source confusion.** Risk: a contributor adds a standalone
  `arrow = "..."` workspace dependency; resulting `RecordBatch` mismatches
  the engine's `duckdb::arrow::record_batch::RecordBatch` at the P3 grid
  boundary. Mitigation: T1 explicitly uses `duckdb::arrow` re-exports; T0
  notes file documents the import path; CI grep-gate (T13 or T20-equivalent)
  rejects new `arrow = ` entries in `Cargo.toml` files.
- **Arrow batch streaming API symbol drift.** Spec §6.2 references
  `Connection::stream_arrow`; the canonical 1.4.x API is likely
  `Statement::query_arrow` returning an iterator. Mitigation: T0 confirms
  exact symbol and notes any roughness (e.g., issue #418 status); T8
  contingency to vendor a fork if upstream blocks.
- **First-run extension download UX.** Lazy-load means first launch with
  network downloads sqlite_scanner from `extensions.duckdb.org`. Mitigation:
  banner UX during download (T14); offline tarball escape hatch documented
  (P10 release asset, but mention in CONTRIBUTING.md from T14).
- **Multi-window extension-install race.** Two windows opening cold could
  both INSTALL sqlite_scanner simultaneously. Mitigation: hoist INSTALL to
  app boot path single-shot before any window opens (T14); engine `init()`
  does LOAD only.
- **Corporate proxy silently breaks first-run download.** DuckDB's HTTP
  client only reads `HTTP_PROXY`/`HTTPS_PROXY` env vars, not OS-level proxy
  config. Mitigation: document env-var setup in CONTRIBUTING.md (T14);
  surface a clear error (not a stall) when download times out.
- **CI fixture-generation flake.** Mitigation: deterministic seed + cache;
  generator unit-tested separately at T12.
- **Test runtime budget on slow CI runners.** 1 GB CSV scan + 500 MB Parquet
  scan + 100 MB SQLite ATTACH could push CI duration. Mitigation: profile in
  T17; if needed, gate the heaviest exit-criterion tests behind a `--release
  --features=heavy-tests` flag to keep main matrix fast.
- **Migration runner concurrent first-time apply (P3+ concern, not P2).**
  P2 migration runner targets per-engine scratch DBs only — no shared file,
  no race. P3 introduces shared workspace DBs and must address concurrent
  first-time application then. Documented as a P3 entry-time review item
  in §2.6.

## 9. Authoritative cross-references

- Engine spec: `docs/specs/2026-04-26-dat0-design.md` §6 + §21.2 P2 + §21.6
- Type surfaces: §6.1 (trait), §6.2 (streaming), §6.3 (memory budget), §6.4
  (sqlite), §6.5 (MotherDuck — informational only for P2)
- Operational substrate: P0 runbook + P1 retro
- Deferral register: `docs/deferrals.md` (D-007, D-008 added at plan-write time)
- Upstream pin tracker: `docs/upstream-watch.md` (DuckDB + Arrow co-pinning
  added at T0 closure)
- API notes pattern: `docs/internal/gpui-api-notes.md` → mirror as
  `docs/internal/duckdb-arrow-api-notes.md` from T0 output.

---

**Status:** brainstorm-locked. Next: implementation plan via `superpowers:writing-plans`.
