# duckdb-rs + Arrow API Notes

Verified for **dat0 P2** on **2026-04-27** (P2.T0 spike — read-only inspection
of `docs.rs/duckdb/1.4.4`, the duckdb-rs GitHub repo at the v1.4.4 tag, and
the DuckDB engine docs at `duckdb.org/docs/current`).

This document is the canonical reference for the duckdb-rs API surface used by
the dat0 engine layer. Subsequent P2 tasks (T1 dependency wiring, T2 engine
construction, T3-T5 file registration, T6-T9 execution paths, T10-T11 ATTACH,
T12 fixture generation) MUST defer to this file when plan snippets contradict
the actual API.

---

## Pinned versions

| Component | Version / Tag | Date pinned | SHA (40-char) | Source |
|---|---|---|---|---|
| `duckdb` (crates.io) | `=1.4.4` | 2026-01-27 (release) | `46d2e094ae741a4e7a500ae4389abf2cfd7e1458` | crates.io publish from `duckdb/duckdb-rs` tag `v1.4.4` |
| Underlying DuckDB native | `1.4.4` | (bundled via `libduckdb-sys`) | tracks duckdb-rs `v1.4.4` | bundled C++ library shipped through the `bundled` feature |

Pin form in `Cargo.toml` (T1 will write):

```toml
[dependencies]
duckdb = { version = "=1.4.4", features = ["bundled", "json", "parquet"] }
```

### Why 1.4.x and not 1.10500.x (CalVer)

Two release lines exist as of 2026-04-27:

- **`1.4.x`** — semver maintenance line, currently at `1.4.4`. Tracks DuckDB
  native `1.4.4`. This is the line P2 pins.
- **`1.10500.x`** — calendar-versioned line where the second semver component
  encodes the underlying DuckDB native version. `1.10500.0` (2025-03-11)
  tracks DuckDB **1.5.0** and ships breaking changes (Rust 2024 edition
  required, `Decimal` rework, Arrow 57 upgrade).

The spec (§2.5) and plan default to `1.4.x`. CalVer is rejected for P2 because
(a) DuckDB 1.5.0 SQL/PRAGMA surface drift has not been audited against P2's
ATTACH / `read_csv` / `read_json` / migration-runner SQL, and (b) the
maintenance line's upstream DuckDB version is the one tested against the
bundled extensions surface we depend on. Re-evaluate at P5/P6 entry.

---

## Feature flags enabled in `dat0-engine` Cargo.toml

T1 enables exactly these on the `duckdb` crate:

- **`bundled`** — statically links DuckDB via `libduckdb-sys/bundled`. Required
  for `json` and `parquet`. **Build-time impact:** triggers a C++ compile of
  the DuckDB amalgamation; first build is multi-minute on a cold target dir.
  No external `cmake` / `clang` requirement beyond the platform toolchains
  P1's CI matrix already provides (Apple Silicon `macos-14`, Ubuntu `x86_64`
  + `aarch64`).
- **`json`** — built-in JSON read functions (`read_json_auto`, `read_ndjson`,
  etc.). Requires `bundled`. Static-linked into the bundled engine.
- **`parquet`** — built-in Parquet read/write functions (`read_parquet`,
  `COPY ... TO 'x.parquet' (FORMAT parquet)`). Requires `bundled`. Static-linked.

The full feature graph (verbatim from `crates/duckdb/Cargo.toml` at v1.4.4):

```toml
default = []
bundled = ["libduckdb-sys/bundled"]
json = ["libduckdb-sys/json", "bundled"]
parquet = ["libduckdb-sys/parquet", "bundled"]
vscalar = ["vtab-arrow"]
vscalar-arrow = []
vtab = []
vtab-loadable = ["loadable-extension"]
vtab-excel = ["vtab", "calamine"]
vtab-arrow = ["vtab", "num"]
appender-arrow = ["vtab-arrow"]
vtab-full = ["vtab-excel", "vtab-arrow", "appender-arrow"]
extensions-full = ["json", "parquet", "vtab-full"]
buildtime_bindgen = ["libduckdb-sys/buildtime_bindgen"]
modern-full = ["chrono", "serde_json", "url", "r2d2", "uuid", "polars"]
polars = ["dep:polars", "dep:polars-arrow"]
loadable-extension = ["vtab", "duckdb-loadable-macros",
                      "libduckdb-sys/loadable-extension"]
```

Features dat0-engine deliberately does **not** enable: `polars` (we never cross
the polars boundary; Arrow `RecordBatch` is the canonical interchange type),
`modern-full` (drags in chrono / r2d2 / uuid we don't need), `extensions-full`
(adds `vtab-full` we don't need; bundling json + parquet directly is leaner),
`vtab` family (no virtual-table consumers in P2), `loadable-extension` (we
*consume* extensions, we don't *publish* them).

---

## Connection API

All paths verified against `docs.rs/duckdb/1.4.4/duckdb/struct.Connection.html`.
`Connection` lives at the crate root: `duckdb::Connection`.

### Opening

```rust
pub fn open<P: AsRef<Path>>(path: P) -> Result<Self>
pub fn open_in_memory() -> Result<Self>
pub fn open_with_flags<P: AsRef<Path>>(path: P, config: Config) -> Result<Self>
pub fn open_in_memory_with_flags(config: Config) -> Result<Self>
```

`Config` is `duckdb::Config`. P2 uses `open` for the on-disk scratch DB (per
spec §2.3 / §2.4 — one connection per engine, scratch path injected at
construction). T2 picks `open` over `open_with_flags` unless a future
PRAGMA-via-Config requirement surfaces.

### Statement-free execution

```rust
pub fn execute<P: Params>(&self, sql: &str, params: P) -> Result<usize>
pub fn execute_batch(&self, sql: &str) -> Result<()>
pub fn query_row<T, P, F>(&self, sql: &str, params: P, f: F) -> Result<T>
    where P: Params, F: FnOnce(&Row<'_>) -> Result<T>
```

- `execute` returns row count for INSERT/UPDATE/DELETE. T6 (`execute()` on the
  trait) wraps this for non-SELECT SQL.
- `execute_batch` accepts multi-statement SQL (semicolon-separated). Used by the
  migration runner (spec §6.6) and the boot-path `INSTALL sqlite_scanner;
  LOAD sqlite_scanner;` pair.
- `query_row` for single-row results (e.g. `SELECT count(*) FROM ...`) — used
  by registration validators.

### Prepared statements

```rust
pub fn prepare(&self, sql: &str) -> Result<Statement<'_>>
```

Returns `Statement<'conn>` borrowing `&Connection`. The streaming Arrow path
(below) is reached through `Statement::query_arrow`.

### Transactions

```rust
pub fn transaction(&mut self) -> Result<Transaction<'_>>
pub fn unchecked_transaction(&self) -> Result<Transaction<'_>>
```

`transaction()` requires `&mut self`; `unchecked_transaction()` is the
`&self` variant (suitable for the `Arc<Mutex<Connection>>` access pattern,
where holding the mutex guarantees non-aliased access without the borrow
checker proving it). Migration-runner code in T2/T15 uses
`unchecked_transaction()` while holding the mutex.

### Interrupt handle (cancellation)

```rust
pub fn interrupt_handle(&self) -> Arc<InterruptHandle>
```

**Plan-spec correction:** spec §2.1 hedges "(or Arc-wrapped if not natively
Clone; T0 confirms)". T0 confirms: the method returns
**`Arc<InterruptHandle>`** — already wrapped. The engine struct should hold
`Arc<InterruptHandle>` (not `InterruptHandle` directly):

```rust
pub struct DuckDBEngine {
    conn: Arc<Mutex<duckdb::Connection>>,
    interrupt: Arc<duckdb::InterruptHandle>,  // confirmed shape
    // ...
}
```

The spec's intent (cheap-clone, share with sibling cancel tasks) is satisfied:
`Arc::clone` is a refcount bump.

`InterruptHandle` itself is `#[derive(Copy)]` and implements `Send + Sync`.
The `Arc<>` wrap makes shared ownership across tasks the canonical pattern;
holding only the inner `InterruptHandle` by `Copy` is also legal but means
you can't outlive the `Connection`'s drop without UB unless you keep an `Arc`
strong ref alive somewhere.

`InterruptHandle::interrupt(&self)` causes any in-flight query on the
originating connection to fail with `Error::DuckDBFailure`. Idempotent / safe
to call after the connection is dropped (no-op).

---

## Arrow streaming API

### Canonical pull-based path: `Statement::query_arrow`

Verified at `docs.rs/duckdb/1.4.4/duckdb/struct.Statement.html` and
`crates/duckdb/src/arrow_batch.rs` at v1.4.4.

```rust
impl Statement<'_> {
    pub fn query_arrow<P: Params>(&mut self, params: P) -> Result<Arrow<'_>>
}
```

The returned `Arrow<'stmt>` struct is a **pull-based, lazy iterator**. Lifetime
is bound to the `Statement`, which itself borrows the `Connection`. Iterator impl:

```rust
// crates/duckdb/src/arrow_batch.rs (v1.4.4)
impl<'stmt> Iterator for Arrow<'stmt> {
    type Item = RecordBatch;       // <-- bare RecordBatch, NOT Result<...>
    fn next(&mut self) -> Option<Self::Item> {
        Some(RecordBatch::from(&self.stmt?.step()?))
    }
}

impl Arrow<'_> {
    pub fn get_schema(&self) -> SchemaRef;
}
```

**Spec-resolution: streaming symbol.** Spec §2.1 hedged: "design-spec §6.2
mention of `Connection::stream_arrow` may reflect older naming". T0 confirms:

- There is **no** `Connection::stream_arrow` method. The streaming entry
  point is on `Statement`, not `Connection`.
- The canonical pull-based iterator for the engine's
  `execute_streaming` path is **`Statement::query_arrow(params)`**.
- A separate `Statement::stream_arrow(params, schema: SchemaRef)` exists but
  takes an explicit `SchemaRef` argument and yields `ArrowStream<'_>` (also
  `Iterator<Item = RecordBatch>`). It exists to support CALL-statement and
  prepared-statement variants that can't infer schema upfront. **Engine code
  uses `query_arrow` unless a specific reason to provide an upfront schema
  surfaces.** (Issue duckdb/duckdb-rs#418 documents the API-design oddness;
  upstream open as of 2026-04-27.)

### Item-type implication: errors disappear silently

The Arrow iterator yields `RecordBatch` **bare** — not `Result<RecordBatch,
duckdb::Error>`. The internal `next()` implementation uses `?` to short-circuit
to `None` if either `self.stmt` becomes invalid or `step()` returns `None`.
**Mid-stream errors are observationally indistinguishable from
end-of-stream.**

Engine-side implications (T7 `execute_streaming`):

1. The `spawn_blocking` worker that pulls from `query_arrow().unwrap()` **must
   inspect the underlying `Statement` post-iteration** to surface late errors.
   The plan's `tokio::sync::mpsc::channel(1)` carrying `Result<RecordBatch,
   EngineError>` is correct; the engine just needs to be aware that the source
   iterator can't itself report the mid-stream error case.
2. Cancellation via `interrupt_handle.interrupt()` causes `step()` to return
   `None` — to the consumer this looks like normal end-of-stream. T7 must
   check the engine's status (set by the cancelling sibling task) before
   reporting "completed" vs "interrupted".
3. Pre-flight prepare errors *do* surface — they come back from
   `prepare(...)` / `query_arrow(...)` directly as `Result`. Only mid-stream
   errors collapse.

This is a **known rough edge** — see `docs/upstream-watch.md` "Why we watch
closely" column for `duckdb-rs`. Track upstream issue #418 for resolution.

### Back-pressure

`Arrow<'stmt>` is genuinely pull-based: each `.next()` triggers one DuckDB
fetch. The plan's design (worker thread pumps batches into
`mpsc::channel(1)`) achieves back-pressure by blocking the worker on `send()`
when the consumer lags. The producer side calls a synchronous `mpsc::Sender`
clone (or an async one inside a blocking-tolerant adapter) — choose at T7.

### Auto-trait status

`Arrow<'stmt>` and `ArrowStream<'stmt>` do **not** implement `Send` or `Sync`
(documented `!Send` / `!Sync` impls on the docs.rs page). This is fine for
the engine's design — the iterator stays inside the `spawn_blocking` worker;
only the resulting `Result<RecordBatch, _>` items cross the `mpsc::Sender`
into the async runtime, and `RecordBatch` itself is `Send + Sync`.

---

## Arrow type paths

All Arrow types crossing the engine→consumer boundary come from the duckdb-rs
re-exports. **Never** add a standalone `arrow = "..."` workspace dep — the
re-exported `arrow` crate version is whatever duckdb-rs 1.4.4 pins internally,
and a duplicate dep would produce nominally-distinct types that don't unify
at the P3 grid trait boundary.

Verified paths (docs.rs/duckdb/1.4.4/duckdb/arrow):

```rust
duckdb::arrow                              // module (root re-export of `arrow` crate)
duckdb::arrow::record_batch::RecordBatch   // primary streaming unit
duckdb::arrow::datatypes::Schema           // schema struct
duckdb::arrow::datatypes::SchemaRef        // = Arc<Schema>
duckdb::arrow::datatypes::Field
duckdb::arrow::datatypes::DataType
duckdb::arrow::error::ArrowError           // arrow-side error type
duckdb::arrow::array::*                    // Array trait + per-type Array structs
```

Workspace import convention (T2 sets the canonical aliases):

```rust
use duckdb::arrow::record_batch::RecordBatch;
use duckdb::arrow::datatypes::{Schema, SchemaRef, Field, DataType};
use duckdb::arrow::error::ArrowError;
```

---

## Extension features

### `sqlite_scanner` static support: NOT AVAILABLE at v1.4.4

Confirmed by exhaustive feature-flag enumeration (see "Feature flags" section
above). The published feature surface contains:

- `bundled`, `json`, `parquet` — core static-link paths
- `vscalar`, `vtab*` family — virtual table machinery
- `loadable-extension` — for *publishing* extensions, not consuming them
- `extensions-full` — convenience alias = `json + parquet + vtab-full`. Does
  **NOT** include sqlite_scanner.
- `modern-full` — convenience alias for chrono + polars + r2d2 + serde_json +
  url + uuid integration features. Does **NOT** include sqlite_scanner.

**No `sqlite_scanner` (or `sqlite-scanner`) feature exists.** The duckdb-rs
README explicitly documents this pattern via the analogous ICU case:

> "When using the `bundled` feature, the ICU extension is not included due
> to crates.io's 10MB package size limit."

The same constraint applies to sqlite_scanner. **Lazy-load via boot-path
`INSTALL sqlite_scanner; LOAD sqlite_scanner;` is the locked path** (per
spec §2.5). **D-009 stays open** as a contingency for the day duckdb-rs
adds a feature.

T0 spike's first action when D-009's target trigger fires: re-enumerate
the feature graph at the then-current duckdb-rs version against this list.

### `motherduck` static support: NOT AVAILABLE at v1.4.4 (informational)

Confirmed: no `motherduck` feature in the v1.4.4 feature graph. Same
distribution model as sqlite_scanner — runtime `INSTALL motherduck; LOAD
motherduck;` is the only path. P2 doesn't ship MotherDuck end-to-end (D-007),
so this is recorded for future P5 work.

When P5 wires MotherDuck:
- The boot-path INSTALL/LOAD pattern from sqlite_scanner is the template.
- The motherduck extension reads the access token from env var `motherduck_token`
  (lowercase) or via the `ATTACH 'md:?token=...'` DSN form. Spec §6.5 noted
  the keychain-resolved token threading; that's a P5 concern.

---

## Parquet writer (COPY)

T12 (fixture generation) writes Parquet via DuckDB SQL — no Rust-side Arrow
writer involvement. Confirmed at `duckdb.org/docs/current/sql/statements/copy`:

### Canonical syntax (DuckDB 1.4.x, supported)

```sql
COPY (SELECT * FROM source_table) TO 'output.parquet' (FORMAT parquet);
```

Or with options:

```sql
COPY (FROM generate_series(100000)) TO 'fixture.parquet'
  (FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 100000);
```

### Relevant options

| Option | Default | Notes |
|---|---|---|
| `COMPRESSION` | `snappy` | Accepts `uncompressed`, `snappy`, `gzip`, `zstd`, `brotli`, `lz4`, `lz4_raw`. T12 uses default unless fixture size demands smaller. |
| `COMPRESSION_LEVEL` | `3` | zstd-only; range 1–22. |
| `ROW_GROUP_SIZE` | `122880` | Target rows per row-group. T12 fixtures don't need to tune this. |
| `ROW_GROUP_SIZE_BYTES` | (unset) | Alternative byte-budget form (e.g., `'2MB'`). |
| `ROW_GROUPS_PER_FILE` | (unset) | Triggers multi-file output. |
| `PARQUET_VERSION` | `V1` | `V1` or `V2`. P2 has no V2 requirement. |
| `FIELD_IDS` | (unset) | `'auto'` infers from schema. |
| `PARTITION_BY` | (unset) | Hive-style partitioning when set. |

### Quirks

- The `FORMAT parquet` token is lowercase per the docs; DuckDB accepts both
  cases but match the docs exactly to avoid drift surprises.
- The `(FORMAT parquet)` parenthesized option syntax is the modern form; an
  older `WITH (FORMAT 'parquet')` form exists but is not recommended.
- COPY always overwrites the destination silently — T12 must clear the
  fixtures dir before regenerating.
- `parquet` is a *built-in* DuckDB feature once `bundled` + the `parquet`
  Cargo feature are enabled; no `INSTALL parquet` is required.

---

## Gotchas

1. **`interrupt_handle()` returns `Arc<InterruptHandle>`, not bare
   `InterruptHandle`.** Spec §2.1 hedged on this — the answer is "yes,
   already Arc-wrapped". Engine struct field type is `Arc<duckdb::InterruptHandle>`.
   `Arc::clone` is the cheap-clone idiom; cloning is a refcount bump.

2. **`Arrow<'_>::Item = RecordBatch`, not `Result<RecordBatch, _>`.** Source
   iterator collapses mid-stream errors to end-of-stream silently. Engine
   `execute_streaming` worker must inspect `Statement` post-iteration to
   surface late errors. Cancellation looks like normal completion to the
   consumer — engine status flag is the source of truth for "interrupted vs
   completed". (See "Item-type implication" above.)

3. **No `Connection::stream_arrow`. The streaming symbol is
   `Statement::query_arrow`.** Plan §"Risks & Caveats" anticipated this — it's
   the *expected* T0 outcome and is not a plan-defect (PD entry not warranted).
   The companion `Statement::stream_arrow(params, schema)` exists but is
   unnecessarily verbose for our path. Use `query_arrow`.

4. **`Arrow<'_>` and `ArrowStream<'_>` are `!Send`.** They live inside the
   `spawn_blocking` worker. Only `RecordBatch` (which IS `Send + Sync`)
   crosses the `mpsc` boundary into async-land. Don't try to hold the iterator
   across `await` points.

5. **No standalone `arrow = "..."` dep.** All Arrow types come from
   `duckdb::arrow::*`. Adding a top-level `arrow` workspace dep would create
   a duplicate type universe that doesn't unify with duckdb-rs's
   `RecordBatch` at the P3 grid boundary. Spec §2.1 is correct here.

6. **`bundled` feature triggers a multi-minute first-build.** The
   `libduckdb-sys/bundled` static-link path compiles the DuckDB amalgamation
   from C++. CI build time is dominated by this on cold targets; cache
   `target/` and `~/.cargo/registry` in CI. (Spec §"Risks" already notes this.)

7. **`sqlite_scanner` and `motherduck` Cargo features do not exist.**
   The feature flag enumeration above is exhaustive. Document this in
   `dat0-engine/src/lib.rs` doc-comment so future maintainers don't grep for
   them and assume they're misspelled.

8. **`unchecked_transaction()` exists for the `&self`-with-mutex pattern.**
   `transaction()` requires `&mut self`. With `Arc<Mutex<Connection>>`, the
   mutex guard provides exclusive access but the borrow checker doesn't
   project that to `&mut Connection` — use `unchecked_transaction()`. Naming
   is unfortunate but the operation is sound when paired with the mutex.

9. **DuckDB native version comes from `bundled`, not a separate pin.**
   Pinning `duckdb = "=1.4.4"` pins the native engine to `1.4.4` via the
   `libduckdb-sys` transitive dep. There is nothing else to pin for the
   engine binary itself.

10. **CalVer line (`1.10500.x`) requires Rust edition 2024 + Arrow 57.**
    P2's workspace is on edition 2024 already (per gpui-component v0.5.1
    requirement, see P1.T0 notes), so the edition itself isn't a blocker —
    but the underlying DuckDB jump to 1.5.0 plus Arrow 57 is too much SQL +
    type drift to take on at P2 entry. Stay on 1.4.x. Re-evaluate per the
    upstream-watch monthly cadence.

11. **`stream_arrow()` got a bug-fix in 1.4.4 (CALL statement support via
    automatic `duckdb_fetch_chunk()` fallback).** Not directly relevant to
    P2 (we use `query_arrow`), but worth noting that 1.4.4 is a Strictly
    Better choice than any earlier 1.4.x patch for the streaming surface.

---

## Provenance

URLs fetched on **2026-04-27** (P2.T0 verification date):

- <https://crates.io/crates/duckdb> — version listing (initial fetch returned
  no payload via WebFetch; cross-referenced via the GitHub Releases page below)
- <https://github.com/duckdb/duckdb-rs/releases> — release notes for v1.4.4
  (2026-01-27) and v1.10500.0 (2025-03-11), commit SHA `46d2e094...`
- <https://github.com/duckdb/duckdb-rs/releases/tag/v1.4.4> — exact tag SHA
  + release-note quotes
- <https://github.com/duckdb/duckdb-rs/blob/v1.4.4/crates/duckdb/Cargo.toml>
  — verbatim feature graph
- <https://github.com/duckdb/duckdb-rs/blob/v1.4.4/README.md> — bundled-vs-ICU
  10MB-limit rationale; building notes
- <https://github.com/duckdb/duckdb-rs/blob/v1.4.4/crates/duckdb/src/arrow_batch.rs>
  — `impl Iterator for Arrow<'stmt>` source confirming `Item = RecordBatch`
- <https://github.com/duckdb/duckdb-rs/issues/418> — `stream_arrow` API
  oddness, open as of fetch date
- <https://docs.rs/duckdb/1.4.4/duckdb/struct.Connection.html> — public
  method list and signatures
- <https://docs.rs/duckdb/1.4.4/duckdb/struct.Statement.html> —
  `query_arrow` / `stream_arrow` signatures and lifetime bounds
- <https://docs.rs/duckdb/1.4.4/duckdb/struct.InterruptHandle.html> —
  `Send + Sync + Copy` confirmation
- <https://docs.rs/duckdb/1.4.4/duckdb/struct.Arrow.html> — Iterator
  impl, `Item = RecordBatch`, `!Send`
- <https://docs.rs/duckdb/1.4.4/duckdb/struct.ArrowStream.html> —
  `stream_arrow` companion type
- <https://docs.rs/duckdb/1.4.4/duckdb/arrow/index.html> — re-export module
  layout
- <https://docs.rs/crate/duckdb/1.4.4/features> — feature enumeration
- <https://duckdb.org/docs/current/sql/statements/copy.html> — COPY syntax
  + Parquet options table
- <https://duckdb.org/docs/current/data/parquet/overview.html> — COPY...TO
  Parquet examples

When bumping the duckdb pin (per `docs/upstream-watch.md` cadence):

1. Refetch every URL above against the new version.
2. Diff the feature-graph block against the new `crates/duckdb/Cargo.toml`.
3. Re-verify the Arrow Iterator `Item` type — if it ever flips to
   `Result<RecordBatch, _>` upstream, gotcha #2 collapses and the engine's
   `execute_streaming` wrapper simplifies.
4. Re-check for `sqlite_scanner` / `motherduck` features (D-009 close trigger).
5. Update the verified-pins row in `docs/upstream-watch.md`.
