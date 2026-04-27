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

### 2.1 Sync→async bridge: `spawn_blocking` + `InterruptHandle`

`duckdb-rs` is synchronous. The `QueryEngine` trait is `async`. Bridge via
`tokio::task::spawn_blocking` per call. Engine struct holds:

```rust
pub struct DuckDBEngine {
    conn: Arc<Mutex<duckdb::Connection>>,
    interrupt: Arc<duckdb::InterruptHandle>,  // cheap clone, callable cross-thread
    budget: MemoryBudget,
    scratch_path: PathBuf,
    status: Arc<RwLock<EngineStatus>>,
}
```

Rationale (locked): see brainstorm Q1. `InterruptHandle` is the cancel mechanism;
`spawn_blocking` is the sync wrapper; pool/dedicated-worker rejected because
DuckDB connections are single-threaded for execution and the `Mutex` serializes
equivalently with less code. Multi-window safety: each engine owns its own
`Connection`, `Mutex`, `InterruptHandle`, scratch DB file — no globals.

**Streaming variant:** `execute_streaming` returns
`Pin<Box<dyn Stream<Item = Result<RecordBatch, EngineError>> + Send>>`. A
`spawn_blocking` worker pulls batches from `Connection::stream_arrow` and pushes
onto `tokio::sync::mpsc::channel(1)` — channel back-pressure pauses the producer
when the consumer (P3 grid) lags. Default capacity = 1; tune only if P3 bench shows starvation.

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

### 2.5 Extension distribution: static bundle (with documented fallback)

Static-link `sqlite_scanner` into `dat0-engine` via duckdb-rs Cargo feature
(verified at T0 spike). CSV / JSON / Parquet are core extensions, autoloaded.

If T0 finds duckdb-rs at the pinned version does not expose a `sqlite_scanner`
feature for static linking, fall back to:
1. Pre-fetch at `dat0-app` boot path (NOT at first ATTACH — avoid surprise pause).
2. Banner UX during download via P1 banner primitive.
3. `HTTP_PROXY` / `HTTPS_PROXY` env var documentation in `CONTRIBUTING.md`.
4. Offline-extensions tarball as release asset for air-gapped users.
5. Track outcome as new deferral `D-009 — bundle sqlite_scanner static when duckdb-rs supports it` (added to register only if fallback path is triggered at T0).

Rationale: dat0's regulated-industry-analyst persona is disproportionately
behind air-gapped or proxied networks; lazy-download contradicts the "files at
scale, local, no cloud" pillar.

**Multi-window safety:** Extension `INSTALL` (when fallback path is active)
hoisted to `dat0-app` boot — single-shot before any window opens — to avoid
two-window install races against `~/.duckdb/extensions/`. With static bundle
the issue does not arise.

### 2.6 Migration scaffold: minimal real runner

Forward-only, append-only `MIGRATIONS: &[Migration]` slice in
`dat0_engine::migrations`. Engine `init()` calls `apply_migrations(&conn)`
inside a transaction per migration. Records applied versions in
`__dat0_meta_migrations`. Skip migrations on ATTACH'd DBs (sqlite, future
MotherDuck) — only the primary connection is owned by us.

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

pub struct RegisterOpts {
    pub format: FileFormat,        // detected or explicit (CSV, TSV, JSON, JSONL, NDJSON, Parquet)
    pub delimiter: Option<char>,
    pub quote_char: Option<char>,
    pub escape_char: Option<char>,
    pub encoding: Option<String>,  // "utf-8", "utf-16", etc.
    pub has_header: Option<bool>,  // None = auto-detect
    pub type_overrides: HashMap<String, String>, // column_name -> DuckDB type literal
    pub sample_rows: u32,          // type inference sample size
}
// Defaults match DuckDB's read_csv_auto behavior. P3 import wizard binds UI to these fields.

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
    Arrow(#[from] arrow::error::ArrowError),
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
    Migration { version: u32, name: String, #[source] source: Box<dyn std::error::Error + Send + Sync> },
    #[error("Query interrupted")]
    Interrupted,
}
```

`EngineError::Interrupted` surfaces from any `spawn_blocking` query when an
external `Engine::interrupt()` call has fired. Discriminator unused in P2 (no
caller hooks `interrupt()` yet) but built so P5 doesn't have to retrofit.

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

## 5. Plan shape (~17–19 tasks)

Confirmed during brainstorm Q5. Hybrid granularity: combined-verify on trivially
mechanical tasks; full two-stage review on tasks touching duckdb-rs / Arrow APIs
or with cross-cutting integration risk.

```
T0  — duckdb-rs + arrow API research spike (MUST run first; downstream
       tasks consume the output)                              [research, full]
T1  — dat0-engine skeleton: Cargo.toml deps + types + errors +
       trait + stub impl                                      [combined-verify]
T2  — connection bootstrap (PRAGMAs, extension load, migration
       runner integration)                                    [full review]
T3  — migrations module + initial migration + unit tests      [full review]
T4  — register_file CSV/TSV                                   [full review]
T5  — register_file JSON/JSONL/NDJSON                         [full review]
T6  — register_file Parquet                                   [full review]
T7  — execute (materialized)                                  [full review]
T8  — execute_paged + execute_streaming + mpsc back-pressure  [full review]
T9  — catalog ops (describe / get_tables / create / drop /
       rename)                                                [combined-verify]
T10 — export_table                                            [combined-verify]
T11 — attach/detach + sqlite_scanner ATTACH                   [full review]
T12 — dat0-fixtures crate (generator)                         [full review]
T13 — CI: fixtures cache + generation step                    [combined-verify]
T14 — extension bootstrap in dat0-app boot path (or no-op if
       static bundle wins at T0)                              [combined-verify]
T15 — integration test: multi-window concurrent engines       [full review]
T16 — integration test: interrupt + cancel isolation          [full review]
T17 — integration test: 1 GB CSV / 500 MB Parquet / 100 MB
       SQLite exit criteria                                   [full review]
T18 — P2 retro                                                [doc]
```

Effort: ~3 weeks per spec §21.6, ~1 task/day average matching P1 cadence.

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
11. Workspace `arrow` and `duckdb` versions co-pinned at `Cargo.toml` workspace level.

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
- What P2 ships: internal `Arc<InterruptHandle>`; public `Engine::interrupt(&self)`
  callable from sibling tasks; `EngineError::Interrupted` variant present.
- What target phase delivers: trait amendment to add `cancel: CancellationToken`
  parameter on `execute*` methods, automatic interrupt-on-drop semantics,
  structured cancellation propagation through streaming, Cmd+. UX in SQL Console.
- Reason: trait shape change is best made once with P5's call-site requirements
  visible; no caller in P2 needs cancellation.

(If T0 spike forces extension fallback path: add a third `D-009 — bundle
sqlite_scanner static when duckdb-rs supports it`.)

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
- **Arrow version skew with downstream consumers.** Mitigation: pin both at
  workspace level; T0 verifies; P3 grid uses same pinned Arrow.
- **`Connection::stream_arrow` known roughness** (spec §6.2 mentions
  duckdb-rs issue #418). Mitigation: T8 includes contingency to vendor a fork
  if upstream blocks; flag at T0 if state has changed since spec authoring.
- **Static-extension support gaps.** Mitigation: §2.5 fallback path documented;
  worst case is `D-007`-equivalent deferral, not a P2 blocker.
- **CI fixture-generation flake.** Mitigation: deterministic seed + cache;
  generator unit-tested separately at T12.
- **Multi-window race in extension install** (fallback path only). Mitigation:
  hoist install to `dat0-app` boot path before any window opens (T14).
- **Test runtime budget on slow CI runners.** 1 GB CSV scan + 500 MB Parquet
  scan + 100 MB SQLite ATTACH could push CI duration. Mitigation: profile in
  T17; if needed, gate the heaviest exit-criterion tests behind a `--release
  --features=heavy-tests` flag to keep main matrix fast.

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
