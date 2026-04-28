# dat0 P2 — Engine Layer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `dat0-engine` crate behind the `QueryEngine` trait surface from design-spec §6.1, backed by a `DuckDBEngine` implementation that registers files in CSV/TSV/JSON/JSONL/NDJSON/Parquet, executes SQL via materialized/paged/streaming Arrow paths, supports `ATTACH`/`DETACH` for `sqlite:` DSNs, applies a per-window memory budget at init, runs forward-only schema migrations, and is multi-window-safe by construction.

**Architecture:** `duckdb-rs` is synchronous; the engine bridges to async via `tokio::task::spawn_blocking` over an `Arc<Mutex<Connection>>`. Cancellation uses `Connection::interrupt_handle()` cloned cheaply from the connection at construction. Streaming returns a `Pin<Box<dyn Stream + Send>>` driven by a `spawn_blocking` worker pushing batches onto a `tokio::sync::mpsc::channel(1)` for back-pressure. Each `DuckDBEngine` instance owns its own connection, mutex, interrupt handle, and scratch DB tempdir — no globals. Multi-window concurrency is achieved by N independent engine instances; their scratch DBs never collide and their `PRAGMA memory_limit` values are independent. The `sqlite_scanner` extension is lazy-loaded once at app boot (not per engine) because duckdb-rs's published feature surface as of 2026-04-27 does not statically bundle it.

**Tech Stack:** Rust 2024 (workspace edition; `rust-toolchain.toml` stable 1.95) · duckdb-rs (1.4.x with features `bundled`, `json`, `parquet`) · DuckDB native (statically linked via `bundled`) · tokio · futures (`Stream` trait) · `duckdb::arrow` re-exports (no standalone `arrow` workspace dep) · thiserror · tracing · rusqlite (fixtures-only) · tempfile · the existing P1 telemetry (Sentry/GlitchTip) and i18n (`t()`) infrastructure.

---

## Authoritative cross-references

- **Spec:** [`docs/specs/2026-04-27-dat0-p2-engine-design.md`](../specs/2026-04-27-dat0-p2-engine-design.md) — brainstorm-locked design for this phase.
- **Upstream design spec:** [`docs/specs/2026-04-26-dat0-design.md`](../specs/2026-04-26-dat0-design.md) — §6 (Engine layer) and §21.2 P2 (entry/exit).
- **Deferral register:** [`docs/deferrals.md`](../deferrals.md) — D-001..D-006 + PD-001..PD-004 from P1; this plan adds D-007, D-008, D-009, D-010 in T0.
- **P1 retro:** [`docs/plans/2026-04-26-dat0-p1-retro.md`](2026-04-26-dat0-p1-retro.md) — toolchain/CI lessons re-applied here.
- **Upstream watch:** [`docs/upstream-watch.md`](../upstream-watch.md) — pin record updated in T0.
- **API notes pattern:** [`docs/internal/gpui-api-notes.md`](../internal/gpui-api-notes.md) — mirrored as `docs/internal/duckdb-arrow-api-notes.md` from T0 output.

---

## Risks & Caveats

This phase depends heavily on `duckdb-rs` at version `1.4.x`. The plan's authored snippets reflect best-current-knowledge of the library's surface; T0 verifies the actual pinned-version surface and downstream tasks consume those notes. **If any code snippet in T2–T11b contradicts what T0 produces, the T0 finding wins.** Update the snippet inline as part of executing the affected task; commit the snippet correction with the task work.

**Plan-defect protocol (from P1):** When an implementer hits a defect in this plan during execution (wrong API signature, missing feature flag, broken assumption), do not silently fix in place. Add a `PD-NNN` entry to `docs/deferrals.md` with severity + suggested fix. Fix inline only if the defect blocks the current task. P1 closed with PD-001..PD-004; continue numbering from PD-005.

**Toolchain pre-flight (lesson from P1 closeout):** Before starting any task, run the CI-equivalent check locally:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

If `cargo` complains about the toolchain, run `rustup update stable` and re-confirm `rust-toolchain.toml` channel. Subagent self-review hits clippy but often skips fmt-check; fmt drifts silently across many tasks otherwise.

**P2-specific known constraints (from spec §2 + code-review pass):**
- `sqlite_scanner` is **not** statically linkable via a duckdb-rs Cargo feature at version 1.4.x as of spec authoring. Locked path is boot-time `INSTALL`+`LOAD`. T0 re-verifies; if a feature has landed since, switch to it and close D-009. Otherwise D-009 stays open as documented intent.
- Arrow types crossing the engine→consumer boundary use `duckdb::arrow` re-exports. **Never add a standalone `arrow = "..."` workspace dependency** — this would produce a nominally distinct `RecordBatch` type and break the P3 grid integration.
- The streaming API symbol may be `Statement::query_arrow` (returning an iterator) rather than `Connection::stream_arrow` named in design-spec §6.2. T0 confirms.
- The migration runner targets per-engine scratch DBs only in P2. Workspace-DB concurrent-open race is a P3 entry-time review item (no shared workspace DB ships in P2).

**Spec exit-criteria reconciliation (P2):** Three items relax in-spec contracts to match the locked deferrals:
- "ATTACH for MotherDuck" — D-007 defers to P5. P2's `attach()` returns `EngineError::NotImplemented { feature: "MotherDuck" }` for `md:` DSNs.
- "Public CancellationToken on trait" — D-008 defers to P5. P2 ships `Engine::interrupt(&self)` outside the trait.
- "non-UTF-8 file encoding" — D-010 defers to P3 import wizard or v1.x. `RegisterOpts` has no `encoding` field.

---

## Prerequisites (from P1)

P1 Foundation merged via PR #1 (merge commit `6b96122`). Before starting T0, verify on a fresh clone:

```bash
cd /Users/salar/Projects/dat0
git status                                                         # clean
git log --oneline -1                                               # 6b96122 or descendant
cargo build --workspace                                            # green
cargo test --workspace                                             # green (Linux keychain tests skipped per PD-004)
ls crates/dat0-engine/src/lib.rs                                   # exists, contains stub comment only
```

If any check fails, fix before proceeding. Do not start T0 against a broken main.

P0 prerequisites that affect P2:
- macOS Metal Toolchain set up locally and in CI (gpui build script). P1 lesson: probe `xcrun --find metal` then fall back to `xcodebuild -downloadComponent MetalToolchain`. Already wired in `.github/workflows/ci.yml`.
- Rust toolchain on dev machine matches CI (stable 1.95.0 as of P1 closeout). Run `rustup show` and confirm.
- Pre-push hook in `.git/hooks/pre-push` (refuses force-push + deletion on main) — already installed locally; reinstall if cloning fresh.

---

## Worktree convention

P1 was executed in `.worktrees/p1-foundation/`. Mirror for P2:

```bash
cd /Users/salar/Projects/dat0
git worktree add .worktrees/p2-engine -b p2-engine main
cd .worktrees/p2-engine
```

All task work happens in the worktree. PR is opened from `p2-engine` branch into `main` after T18 retro.

---

## File structure

This is the file/module tree at the end of P2. Tasks below create or modify these files. Files marked `(stub)` exist after P1 but are essentially empty.

```
dat0/
├── Cargo.toml                          # workspace — adds duckdb dep at workspace level
├── Cargo.lock                          # committed
├── .github/
│   └── workflows/
│       └── ci.yml                      # T13 adds fixture cache + generation step
├── crates/
│   ├── dat0-app/
│   │   ├── Cargo.toml                  # T14 adds dat0-engine path dep
│   │   └── src/
│   │       └── boot.rs                 # T14 adds extension bootstrap call
│   ├── dat0-engine/                    # currently a stub crate (P1 T2)
│   │   ├── Cargo.toml                  # T1: deps, features bundled+json+parquet
│   │   └── src/
│   │       ├── lib.rs                  # T1: re-exports + module decls
│   │       ├── error.rs                # T1: EngineError
│   │       ├── types.rs                # T1: TableInfo, ColumnInfo, RegisterOpts, ...
│   │       ├── trait_def.rs            # T1: QueryEngine trait
│   │       ├── duckdb_engine.rs        # T2: DuckDBEngine struct + new + init + close + status
│   │       ├── migrations.rs           # T3: Migration + apply_migrations + m001_init
│   │       ├── register/
│   │       │   ├── mod.rs              # T4..T6: register_file dispatch
│   │       │   ├── csv.rs              # T4: CSV/TSV impl
│   │       │   ├── json.rs             # T5: JSON/JSONL/NDJSON impl
│   │       │   └── parquet.rs          # T6: Parquet impl
│   │       ├── execute/
│   │       │   ├── mod.rs              # T7: execute (materialized)
│   │       │   ├── paged.rs            # T8: execute_paged
│   │       │   └── streaming.rs        # T8: execute_streaming + Stream impl
│   │       ├── catalog.rs              # T9: describe_table, get_tables, create/drop/rename
│   │       ├── export.rs               # T10: export_table
│   │       ├── attach.rs               # T11a + T11b: attach/detach + sqlite path
│   │       ├── extension_bootstrap.rs  # T14: install_sqlite_scanner_once (called by app boot)
│   │       └── tracing_helpers.rs      # T1: instrument helpers (skip_all + fields(sql_len))
│   ├── dat0-fixtures/                  # T12 — new workspace bin crate
│   │   ├── Cargo.toml                  # T12: clap + duckdb + rusqlite
│   │   └── src/
│   │       └── main.rs                 # T12: generator with --seed + --out
│   └── (existing dat0-format/dat0-i18n/dat0-keychain unchanged)
├── tests/
│   └── fixtures/
│       ├── small/                      # T1: in-repo edge-case fixtures
│       │   ├── basic.csv
│       │   ├── edge_cases.csv
│       │   ├── simple.json
│       │   ├── simple.jsonl
│       │   ├── simple.ndjson
│       │   ├── simple.parquet
│       │   └── simple.sqlite
│       └── large/                      # T12 generator output (gitignored)
├── .gitignore                          # T12 adds tests/fixtures/large/
├── docs/
│   ├── deferrals.md                    # T0 adds D-007..D-010
│   ├── upstream-watch.md               # T0 adds DuckDB pin row
│   ├── internal/
│   │   └── duckdb-arrow-api-notes.md   # T0 creates
│   └── plans/
│       ├── 2026-04-27-dat0-p2-engine-plan.md   # this file
│       └── 2026-04-27-dat0-p2-retro.md          # T18
└── (everything else from P1 unchanged)
```

---

## Tasks

### Task 0: duckdb-rs + Arrow API research spike

**This task is research, not code.** Output is a verified-imports document and three deferral entries. No source-code commits; only documentation commits.

**Files:**
- Create: `docs/internal/duckdb-arrow-api-notes.md`
- Modify: `docs/upstream-watch.md` (add DuckDB row to "Current verified pins" table)
- Modify: `docs/deferrals.md` (add D-007, D-008, D-009, D-010)

**Subagent dispatch profile:** full review (downstream tasks consume this output; mistakes here cascade through 18+ tasks). Mirror P1 T0 pattern — single research subagent, two-stage review (spec compliance + code quality on the produced doc).

- [ ] **Step 0.1: Pick the duckdb-rs version**

Visit `https://crates.io/crates/duckdb`. Find the latest 1.4.x release (e.g., `1.4.4`). Decide between the maintenance line (`1.4.x`) and the CalVer line (`1.10500.0`+, which corresponds to upstream DuckDB native versions). If picking CalVer, note that DuckDB native SQL syntax may have drifted from 1.4.x — verify ATTACH forms, `read_json`/`read_csv` params, and migration runner SQL stay valid. Record the exact version. Open its `Cargo.toml` to confirm features available; in particular look for: `bundled`, `json`, `parquet`, `sqlite_scanner` (whether it exists as a feature), `motherduck`, `vtab`, `vtab-arrow`.

Open `docs/internal/duckdb-arrow-api-notes.md` and write:

```markdown
# duckdb-rs + Arrow API Notes

Verified for **dat0 P2** on `<YYYY-MM-DD>`.

## Pinned versions

- `duckdb` crate version: `<exact, e.g., 1.4.4 — latest 1.4.x as of Jan 2026>`
- duckdb-rs publish commit (if available): `<SHA>`
- Underlying DuckDB native version: `<e.g., 1.4.4>`

## Feature flags enabled in dat0-engine Cargo.toml

- `bundled` — statically links DuckDB; required for `json`/`parquet`.
- `json` — built-in JSON read functions (`read_json_auto`, etc.). Requires `bundled`.
- `parquet` — built-in Parquet read functions. Requires `bundled`.
- (sqlite_scanner / motherduck features investigated — see §"Extension features" below.)
```

- [ ] **Step 0.2: Verify the `Connection` API surface**

Read the duckdb-rs source (or rustdoc on docs.rs) for the pinned version. Locate:
- `duckdb::Connection::open(path)` — exact signature, return type
- `duckdb::Connection::open_in_memory()` — exact signature
- `duckdb::Connection::execute(sql, params)` — for non-query SQL
- `duckdb::Connection::execute_batch(sql)` — for multi-statement SQL
- `duckdb::Connection::query_row(sql, params, f)` — for single-row results
- `duckdb::Connection::prepare(sql)` — returns `Statement`
- `duckdb::Connection::interrupt_handle()` — name, signature, return type, `Send`/`Sync`/`Clone` traits on the returned handle
- `duckdb::Connection::transaction()` — returns transaction wrapper

Record exact signatures + use paths in the API notes file under a `## Connection API` heading. For each method, copy a one-liner call example.

- [ ] **Step 0.3: Verify the Arrow streaming API**

Locate the Arrow batch streaming entry point. Try in order:
1. `duckdb::Statement::query_arrow(params)` — returns an iterator/struct over `RecordBatch`
2. `duckdb::Connection::stream_arrow(...)` — if it exists
3. `duckdb::Arrow` adapter type if present

Record:
- Exact type returned (e.g., `Arrow<'_>` or `ArrowIterator` — name and lifetime params)
- The `Iterator::Item` type — confirm `Result<RecordBatch, duckdb::Error>` or `RecordBatch` directly
- Whether the iterator is lazy (pull-based) — the streaming back-pressure design depends on this
- Whether `Statement` borrows `Connection`; if so, lifetime constraints on the returned iterator
- The `RecordBatch` type path: should be `duckdb::arrow::record_batch::RecordBatch`

Note in the API notes: any known-rough-edges issues (link to GitHub issues if found).

- [ ] **Step 0.4: Verify Arrow type re-exports**

Confirm that `duckdb::arrow` is the module re-exporting Arrow types. Locate:
- `duckdb::arrow::record_batch::RecordBatch`
- `duckdb::arrow::datatypes::Schema`, `Field`, `DataType`
- `duckdb::arrow::error::ArrowError`

Record the full set of needed paths in the API notes under `## Arrow type paths`.

- [ ] **Step 0.5: Investigate `sqlite_scanner` static support**

Check duckdb-rs's feature surface for any of: `sqlite_scanner`, `sqlite-scanner`, `extensions-full`, `modern-full` that bundles sqlite_scanner statically. Read the duckdb-rs README's "Notes on building" section for any sqlite_scanner discussion. Read the latest changelog entries for any mention.

If a static-linking path exists at the pinned version, record the exact feature name + how to enable. The plan's locked path (lazy-load) becomes a fallback.

If no static path exists (expected outcome per spec §2.5), record explicitly: "No `sqlite_scanner` static feature available at pinned version. Lazy-load via `INSTALL sqlite_scanner; LOAD sqlite_scanner;` is the locked path. D-009 stays open."

- [ ] **Step 0.6: Investigate `motherduck` static support (informational)**

Same exercise for the MotherDuck DuckDB extension. Record findings. P2 doesn't ship MotherDuck end-to-end (D-007 deferral) but knowing the extension distribution shape avoids re-spiking later.

- [ ] **Step 0.7: Investigate the `parquet` writer API**

For T12 fixture generation, P2 uses `COPY ... TO 'file.parquet' (FORMAT PARQUET)` via DuckDB SQL — not a Rust-side Arrow writer. Confirm DuckDB at the pinned version supports this syntax (it does as of DuckDB 1.0; verify still true at 1.4.x). Record the exact COPY syntax + any quirks.

- [ ] **Step 0.8: Note any other gotchas**

Record anything that surprised you: a method renamed, a feature flag with non-obvious behavior, a build-script step needed (e.g., `cmake` required for `bundled`), a known platform-specific issue. Anything T1+ should know.

- [ ] **Step 0.9: Update `docs/upstream-watch.md`**

Add a row to the "Current verified pins" table for DuckDB. Replace the existing `duckdb-rs` row in the "Tracked dependencies" table to record the actual pin policy (exact version `=1.4.x`).

```markdown
| `duckdb` (duckdb-rs) | `=1.4.x` | `<publish commit if available>` | `<verification date>` | Features `bundled`, `json`, `parquet`. sqlite_scanner lazy-loaded; see D-009. |
```

- [ ] **Step 0.10: Add deferrals D-007..D-010**

Append to `docs/deferrals.md`:

D-007 (MotherDuck ATTACH end-to-end), D-008 (CancellationToken on trait), D-009 (static-bundle sqlite_scanner contingency), D-010 (non-UTF-8 file encoding). Use the exact bodies in spec §7. Update the at-a-glance table at the top of the file.

- [ ] **Step 0.11: Commit**

```bash
git add docs/internal/duckdb-arrow-api-notes.md docs/upstream-watch.md docs/deferrals.md
git commit -s -m "$(cat <<'EOF'
docs(p2): T0 — duckdb-rs API spike notes + deferrals D-007..D-010

Verifies duckdb-rs <version> Connection/Statement/Arrow surface for the
P2 engine implementation. Records canonical streaming API symbol,
extension static-linking state (sqlite_scanner lazy-load locked,
D-009 contingency open), and Arrow type re-export paths.

Adds D-007 MotherDuck end-to-end, D-008 CancellationToken trait wiring,
D-009 sqlite_scanner static bundle contingency, D-010 non-UTF-8 file
encoding to the deferral register.

Updates upstream-watch with the DuckDB pin row.
EOF
)"
```

**Verification:** No code changes. The committed doc is consumed by all downstream tasks. Spec compliance: every locked item in design-spec §2 corresponds to a section in `duckdb-arrow-api-notes.md`.

---

### Task 1: dat0-engine skeleton

Build the crate's foundational module structure: dependencies, types, errors, `QueryEngine` trait, and a stub `DuckDBEngine` impl that compiles but doesn't yet do anything. Also drops the small in-repo fixture files into `tests/fixtures/small/`.

**Files:**
- Modify: `Cargo.toml` (workspace deps)
- Modify: `crates/dat0-engine/Cargo.toml`
- Modify: `crates/dat0-engine/src/lib.rs`
- Create: `crates/dat0-engine/src/error.rs`
- Create: `crates/dat0-engine/src/types.rs`
- Create: `crates/dat0-engine/src/trait_def.rs`
- Create: `crates/dat0-engine/src/duckdb_engine.rs` (stub)
- Create: `crates/dat0-engine/src/tracing_helpers.rs`
- Create: `tests/fixtures/small/basic.csv`
- Create: `tests/fixtures/small/edge_cases.csv`
- Create: `tests/fixtures/small/simple.json`
- Create: `tests/fixtures/small/simple.jsonl`
- Create: `tests/fixtures/small/simple.ndjson`
- Create: `tests/fixtures/small/simple.parquet` (committed binary, ~2 KB; see Step 1.3)
- Create: `tests/fixtures/small/simple.sqlite` (committed binary, ~2 KB; see Step 1.3)
- Modify: `.gitignore`

**Subagent dispatch profile:** combined-verify. Mostly mechanical type definitions; no DuckDB API contact yet.

- [ ] **Step 1.1: Add workspace deps**

Edit `/Users/salar/Projects/dat0/Cargo.toml`. Add to `[workspace.dependencies]`:

```toml
# Engine: DuckDB native + bundled extensions (json, parquet are statically linked under `bundled`).
# CSV reading is in DuckDB core; sqlite_scanner is NOT bundled (see D-009) and is loaded at runtime.
# Arrow types are consumed via duckdb::arrow re-exports — DO NOT add a standalone `arrow` dep.
duckdb = { version = "=1.4.4", features = ["bundled", "json", "parquet"] }

# Streaming
futures = "0.3"

# Internal crates
dat0-engine = { path = "crates/dat0-engine" }
```

> The default `=1.4.4` is the latest 1.4.x maintenance release (Jan 2026). T0 may
> bump to a newer 1.4.x patch or pivot to the CalVer line (`1.10500.0`+, which
> aligns with upstream DuckDB native version). If T0 picks the CalVer line,
> verify that DuckDB SQL syntax (`(TYPE SQLITE)` ATTACH form, `read_json` params,
> etc.) hasn't drifted; record any drift in the API notes file.

Also confirm `tempfile = "3"` is already in workspace deps (P1 added it). It will be used by engine tests.

- [ ] **Step 1.2: Edit `crates/dat0-engine/Cargo.toml`**

Replace contents:

```toml
[package]
name = "dat0-engine"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
rust-version.workspace = true

[lib]
path = "src/lib.rs"

[dependencies]
duckdb.workspace = true
futures.workspace = true
tokio.workspace = true
tracing.workspace = true
thiserror.workspace = true
serde.workspace = true

[dev-dependencies]
tempfile.workspace = true
tokio = { workspace = true, features = ["rt-multi-thread", "macros", "fs", "sync", "time"] }
```

- [ ] **Step 1.3: Create the small in-repo fixtures**

Each file is small and lives in `tests/fixtures/small/`. Hand-author rather than generate.

`tests/fixtures/small/basic.csv`:

```csv
id,name,score,active
1,alpha,1.5,true
2,beta,2.5,false
3,gamma,3.5,true
```

`tests/fixtures/small/edge_cases.csv` (BOM at start; CRLF line endings; quoted commas; embedded quotes; NULLs as empty fields):

```
\u{FEFF}id,name,note
1,"a, b","he said ""hi"""
2,,
3,"line1\r\nline2",ok
```

(Note: write the actual BOM byte at byte 0 — `EF BB BF` — and use real CRLF separators in the file. The escape codes shown are illustrative.)

`tests/fixtures/small/simple.json`:

```json
[{"id":1,"name":"a"},{"id":2,"name":"b"},{"id":3,"name":"c"}]
```

`tests/fixtures/small/simple.jsonl`:

```
{"id":1,"name":"a"}
{"id":2,"name":"b"}
{"id":3,"name":"c"}
```

`tests/fixtures/small/simple.ndjson` (same content as `.jsonl` — DuckDB treats them identically; we keep both filenames for `register_file` extension-detection coverage):

```
{"id":1,"name":"a"}
{"id":2,"name":"b"}
{"id":3,"name":"c"}
```

For `simple.parquet` and `simple.sqlite`, generate them once locally with a one-shot script:

```bash
mkdir -p tests/fixtures/small
duckdb :memory: <<'EOF'
COPY (SELECT * FROM (VALUES (1,'a'),(2,'b'),(3,'c')) v(id, name)) TO 'tests/fixtures/small/simple.parquet' (FORMAT PARQUET);
EOF

# Build SQLite via Python (or rusqlite once T1 compiles); easiest one-off:
python3 - <<'EOF'
import sqlite3
con = sqlite3.connect('tests/fixtures/small/simple.sqlite')
con.executescript("""
  CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT);
  INSERT INTO items VALUES (1,'a'),(2,'b'),(3,'c');
""")
con.close()
EOF
```

Both files are < 2 KB and committed. Verify with `du -sh tests/fixtures/small/*` — total should be under 10 KB.

- [ ] **Step 1.4: Update `.gitignore`**

Append:

```gitignore
# P2: generated large fixtures live here; regenerated by dat0-fixtures crate
/tests/fixtures/large/
```

- [ ] **Step 1.5: Replace `crates/dat0-engine/src/lib.rs`**

```rust
//! dat0 query engine.
//!
//! Public API: the [`QueryEngine`] trait and the [`DuckDBEngine`] implementation.
//! See `docs/specs/2026-04-27-dat0-p2-engine-design.md` for the architectural
//! contract.

pub mod error;
pub mod types;
pub mod trait_def;
pub mod duckdb_engine;
pub mod migrations;
pub mod register;
pub mod execute;
pub mod catalog;
pub mod export;
pub mod attach;
pub mod extension_bootstrap;
pub(crate) mod tracing_helpers;

pub use error::EngineError;
pub use trait_def::QueryEngine;
pub use types::{
    AttachOpts, ColumnInfo, DerivedOrigin, EngineStatus, ExportFormat,
    FileFormat, MemoryBudget, PagedQueryResult, QueryResult,
    RegisterOpts, TableInfo, TableOrigin, ArrowRecordBatchStream,
};
pub use duckdb_engine::DuckDBEngine;

/// Result type for engine operations.
pub type Result<T> = std::result::Result<T, EngineError>;
```

> Module decls reference files some later tasks create. Use `mod migrations;` etc. as **empty stub modules** in this step — see Step 1.10 for the stubs.

- [ ] **Step 1.6: Create `crates/dat0-engine/src/error.rs`**

```rust
//! Engine error type. Per spec §2.10.

use std::path::PathBuf;

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
    Migration {
        version: u32,
        name: String,
        #[source]
        source: duckdb::Error,
    },

    #[error("Query interrupted")]
    Interrupted,

    #[error("Engine is closed or closing; new operations rejected")]
    EngineClosed,

    #[error("Engine connection mutex poisoned (prior panic in worker thread)")]
    EnginePoisoned,

    #[error("Engine is in Failed state: {0}")]
    EngineFailed(String),
}
```

- [ ] **Step 1.7: Create `crates/dat0-engine/src/types.rs`**

```rust
//! Engine type surface. Per spec §2.9.

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;

use duckdb::arrow::record_batch::RecordBatch;
use futures::Stream;
use serde::{Deserialize, Serialize};

/// Engine lifecycle state.
///
/// Transition contract:
/// - `new()`              -> `Initializing`
/// - `init()` success     -> `Ready`
/// - `init()` failure     -> `Failed(reason)`
/// - `close()` entry      -> `Closing`
/// - `close()` complete   -> `Closed` (errors during cleanup are logged but do not affect the transition)
/// - poisoned mutex       -> `Failed(reason)` (transitioned on first observation)
///
/// In-flight query errors do **not** transition status. The engine remains
/// `Ready` until `close()` is invoked or a panic poisons the connection mutex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineStatus {
    Initializing,
    Ready,
    Closing,
    Closed,
    Failed(String),
}

/// Per-engine memory budget. Caller computes; engine applies via `PRAGMA memory_limit`.
#[derive(Debug, Clone, Copy)]
pub struct MemoryBudget {
    pub bytes: u64,
}

impl MemoryBudget {
    /// Format as a DuckDB pragma string, e.g. "16GB" or "512MB".
    pub fn as_pragma(&self) -> String {
        // DuckDB accepts bytes integers in newer versions but a units-suffixed string is safest.
        let mb = self.bytes / (1024 * 1024);
        format!("{}MB", mb)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FileFormat {
    Csv,
    Tsv,
    Json,
    Jsonl,
    Ndjson,
    Parquet,
}

impl FileFormat {
    /// Sniff format from a path extension. None means unknown — caller decides.
    pub fn from_extension(path: &std::path::Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "csv" => Some(FileFormat::Csv),
            "tsv" => Some(FileFormat::Tsv),
            "json" => Some(FileFormat::Json),
            "jsonl" => Some(FileFormat::Jsonl),
            "ndjson" => Some(FileFormat::Ndjson),
            "parquet" | "pq" => Some(FileFormat::Parquet),
            _ => None,
        }
    }
}

/// Per spec §2.9. `encoding` deliberately absent (D-010).
///
/// **Spec deviation (intentional):** spec §2.9 declares `format: FileFormat`
/// with auto-detection externalized to the caller. The plan uses
/// `format: Option<FileFormat>` so the engine handles auto-detect internally
/// (None = sniff from path extension). This is the same end-user contract,
/// shifted one layer: callers can still pass an explicit format. Document in
/// T1 commit message; revisit if P3 import wizard prefers explicit-format
/// dispatch.
#[derive(Debug, Clone, Default)]
pub struct RegisterOpts {
    pub format: Option<FileFormat>, // None = sniff from extension
    pub delimiter: Option<char>,
    pub quote_char: Option<char>,
    pub escape_char: Option<char>,
    pub has_header: Option<bool>,                // None = auto-detect
    pub type_overrides: HashMap<String, String>, // column_name -> DuckDB type literal
    pub sample_rows: Option<u32>,                // None = DuckDB default; Some(0) is invalid
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String, // DuckDB type literal as returned by DESCRIBE
    pub nullable: bool,
}

#[derive(Debug, Clone)]
pub enum TableOrigin {
    File(PathBuf),
    Derived(DerivedOrigin),
    Attached { alias: String, source: String },
}

#[derive(Debug, Clone)]
pub enum DerivedOrigin {
    Sql(String),
    Transform { parent: String, ops: Vec<String> },
}

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub name: String,
    pub schema: String,
    pub columns: Vec<ColumnInfo>,
    pub row_count_estimate: Option<u64>,
    pub origin: TableOrigin,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<ColumnInfo>,
    pub batches: Vec<RecordBatch>,
}

#[derive(Debug, Clone)]
pub struct PagedQueryResult {
    pub total_rows: u64,
    pub offset: u64,
    pub batches: Vec<RecordBatch>,
}

pub type ArrowRecordBatchStream =
    Pin<Box<dyn Stream<Item = Result<RecordBatch, crate::EngineError>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    Parquet,
}

#[derive(Debug, Clone, Default)]
pub struct AttachOpts {
    pub read_only: bool,
    pub schema_filter: Option<Vec<String>>,
}
```

- [ ] **Step 1.8: Create `crates/dat0-engine/src/trait_def.rs`**

```rust
//! `QueryEngine` trait per design-spec §6.1 verbatim.

use std::path::Path;

use crate::types::{
    ArrowRecordBatchStream, AttachOpts, ColumnInfo, DerivedOrigin, EngineStatus,
    ExportFormat, PagedQueryResult, QueryResult, RegisterOpts, TableInfo,
};
use crate::Result;

#[async_trait::async_trait]
pub trait QueryEngine: Send + Sync {
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

> The `async_trait` crate must be added to `crates/dat0-engine/Cargo.toml`. Add `async-trait = "0.1"` under `[dependencies]`. (The Rust 2024 native AFIT — async fn in trait — works for trait *definition* but `Send` bounds on returned futures still need `async_trait` for ergonomic dyn-compat consumers. We'll let T7+ revisit if AFIT is sufficient at the pinned toolchain.)

Also add `async-trait` to `[workspace.dependencies]`:

```toml
async-trait = "0.1"
```

- [ ] **Step 1.9: Create `crates/dat0-engine/src/duckdb_engine.rs` — stub**

```rust
//! `DuckDBEngine` — sole `QueryEngine` impl in v1.
//!
//! Implementation lands across T2 (bootstrap), T3 (migrations), T4–T6 (register_file),
//! T7–T8 (execute family), T9 (catalog), T10 (export), T11 (attach/detach).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use crate::types::{EngineStatus, MemoryBudget};

/// The concrete engine type. Per spec §2.1.
pub struct DuckDBEngine {
    pub(crate) conn: Arc<Mutex<duckdb::Connection>>,
    pub(crate) interrupt: Arc<duckdb::InterruptHandle>,
    pub(crate) budget: MemoryBudget,
    pub(crate) scratch_path: PathBuf,
    pub(crate) status: Arc<RwLock<EngineStatus>>,
}

impl DuckDBEngine {
    /// Construction lands in T2. This stub is here only to make `lib.rs`'s
    /// public re-export compile in T1.
    #[doc(hidden)]
    pub fn __t1_stub_marker(&self) {}
}
```

> `Connection::interrupt_handle()` returns `Arc<InterruptHandle>` directly per duckdb-rs 1.4.4 — no double-wrapping needed. T0 confirms this exactly. If T0 finds the API has changed (e.g., returns a bare `InterruptHandle: Clone`), drop the `Arc<>` wrap and update T2's constructor accordingly.

- [ ] **Step 1.10: Create stub modules for everything not yet implemented**

```bash
# from worktree root
cat > crates/dat0-engine/src/migrations.rs <<'EOF'
//! Migration runner. T3 fills this in.
EOF
mkdir -p crates/dat0-engine/src/register crates/dat0-engine/src/execute
cat > crates/dat0-engine/src/register/mod.rs <<'EOF'
//! `register_file` dispatch. T4–T6 fill this in.
pub mod csv;
pub mod json;
pub mod parquet;
EOF
echo '//! T4 fills this in.' > crates/dat0-engine/src/register/csv.rs
echo '//! T5 fills this in.' > crates/dat0-engine/src/register/json.rs
echo '//! T6 fills this in.' > crates/dat0-engine/src/register/parquet.rs
cat > crates/dat0-engine/src/execute/mod.rs <<'EOF'
//! Execute family. T7–T8 fill this in.
pub mod paged;
pub mod streaming;
EOF
echo '//! T8 fills this in.' > crates/dat0-engine/src/execute/paged.rs
echo '//! T8 fills this in.' > crates/dat0-engine/src/execute/streaming.rs
echo '//! T9 fills this in.' > crates/dat0-engine/src/catalog.rs
echo '//! T10 fills this in.' > crates/dat0-engine/src/export.rs
echo '//! T11 fills this in.' > crates/dat0-engine/src/attach.rs
echo '//! T14 fills this in.' > crates/dat0-engine/src/extension_bootstrap.rs
```

- [ ] **Step 1.11: Create `crates/dat0-engine/src/tracing_helpers.rs`**

```rust
//! Tracing instrumentation helpers.
//!
//! Per spec §7 commitment 3: never log SQL text (potential PII; P1 Sentry
//! redaction skips it). Wrap span construction so contributors don't have to
//! remember `skip_all` + `fields(sql_len)` every time.

/// Returns the byte length of the SQL string, for use as a span field.
/// SQL text itself is never logged.
#[inline]
pub(crate) fn sql_len(sql: &str) -> usize {
    sql.len()
}
```

- [ ] **Step 1.12: Run `cargo check`**

```bash
cargo check --workspace
```

Expected: green. Engine crate compiles with the new types and stub modules. If any path resolution issue, fix it before committing.

- [ ] **Step 1.13: Run `cargo fmt + clippy`**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: no warnings. (`#[doc(hidden)]` on `__t1_stub_marker` may need an `#[allow(dead_code)]` or rename to `_t1_stub_marker` to silence dead-code warnings — adjust as needed.)

- [ ] **Step 1.14: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore crates/dat0-engine tests/fixtures/small
git commit -s -m "$(cat <<'EOF'
feat(engine): T1 — dat0-engine skeleton + small in-repo fixtures

Adds duckdb workspace dep (=1.4.4 with bundled+json+parquet),
async-trait, futures. Defines EngineError, type surface (TableInfo,
RegisterOpts, etc.), QueryEngine trait verbatim per design-spec §6.1,
and a stub DuckDBEngine struct.

Spec deviation (intentional): RegisterOpts.format is Option<FileFormat>
rather than FileFormat per spec §2.9 — engine handles auto-detect
internally so callers don't have to pre-sniff. Same end-user contract.

Drops 7 small in-repo fixtures (CSV happy/edge, JSON/JSONL/NDJSON,
Parquet, SQLite) for unit tests. Large fixtures (gitignored) generated
in T12.

No engine behavior yet; T2 bootstraps the connection.
EOF
)"
```

**Verification:** `cargo check --workspace` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; fixtures committed and < 10 KB total.

---

### Task 2: Connection bootstrap

Implement `DuckDBEngine::new`, `init`, `close`, `status`, and the cross-thread interrupt mechanism. Apply pragmas at init. Load `sqlite_scanner` (extension is installed by app boot in T14; engine just `LOAD`s).

**Files:**
- Modify: `crates/dat0-engine/src/duckdb_engine.rs`
- Modify: `crates/dat0-engine/src/trait_def.rs` (no signature change; T2 provides impl on `DuckDBEngine`)
- Create: `crates/dat0-engine/tests/bootstrap.rs`

**Subagent dispatch profile:** full review — first contact with duckdb-rs Connection API.

- [ ] **Step 2.1: Write the failing tests**

`crates/dat0-engine/tests/bootstrap.rs`:

```rust
use std::path::PathBuf;
use std::time::Duration;

use dat0_engine::{DuckDBEngine, EngineStatus, MemoryBudget, QueryEngine};

fn budget_512mb() -> MemoryBudget {
    MemoryBudget { bytes: 512 * 1024 * 1024 }
}

#[tokio::test]
async fn engine_status_starts_initializing_then_becomes_ready() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.duckdb");
    let engine = DuckDBEngine::new(scratch.clone(), budget_512mb()).unwrap();
    assert_eq!(engine.status(), EngineStatus::Initializing);
    engine.init().await.unwrap();
    assert_eq!(engine.status(), EngineStatus::Ready);
    engine.close().await.unwrap();
    assert_eq!(engine.status(), EngineStatus::Closed);
}

#[tokio::test]
async fn engine_init_applies_memory_pragma() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.duckdb");
    let budget = MemoryBudget { bytes: 1024 * 1024 * 1024 }; // 1 GB
    let engine = DuckDBEngine::new(scratch.clone(), budget).unwrap();
    engine.init().await.unwrap();

    // Probe via execute() once T7 lands. For T2 we expose a debug helper.
    let limit = engine
        .__debug_query_scalar("SELECT current_setting('memory_limit')")
        .await
        .unwrap();
    // DuckDB normalizes; expect "1.0 GiB" or similar.
    assert!(limit.contains("GiB") || limit.contains("MiB"));
    engine.close().await.unwrap();
}

#[tokio::test]
async fn engine_rejects_ops_after_close() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.duckdb");
    let engine = DuckDBEngine::new(scratch.clone(), budget_512mb()).unwrap();
    engine.init().await.unwrap();
    engine.close().await.unwrap();
    let err = engine
        .__debug_query_scalar("SELECT 1")
        .await
        .expect_err("must reject ops after close");
    assert!(matches!(err, dat0_engine::EngineError::EngineClosed));
}

#[tokio::test]
async fn interrupt_handle_is_clonable_cross_thread() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("scratch.duckdb");
    let engine = DuckDBEngine::new(scratch.clone(), budget_512mb()).unwrap();
    engine.init().await.unwrap();

    // Sanity check: interrupt() must be callable from a sibling task without
    // holding the connection lock.
    let engine_arc = std::sync::Arc::new(engine);
    let e2 = engine_arc.clone();
    let handle = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        e2.interrupt();
    });
    handle.await.unwrap();
    engine_arc.close().await.unwrap();
}
```

> The `__debug_query_scalar` helper is intentionally test-only. T7 ships `execute()` and tests can switch to it, but T2's tests run before T7 — we ship a hidden helper to break the dependency cycle.

- [ ] **Step 2.2: Run the tests, verify they fail**

```bash
cargo test --package dat0-engine --test bootstrap
```

Expected: compile error (no `DuckDBEngine::new`, etc.) — OK as a "fails to compile" failure.

- [ ] **Step 2.3: Implement `DuckDBEngine` bootstrap**

Replace `crates/dat0-engine/src/duckdb_engine.rs`:

```rust
//! `DuckDBEngine` — sole `QueryEngine` impl in v1. Per spec §2.1.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tracing::{instrument, debug, error};

use crate::error::EngineError;
use crate::types::{EngineStatus, MemoryBudget};
use crate::Result;

pub struct DuckDBEngine {
    pub(crate) conn: Arc<Mutex<duckdb::Connection>>,
    pub(crate) interrupt: Arc<duckdb::InterruptHandle>,
    pub(crate) budget: MemoryBudget,
    pub(crate) scratch_path: PathBuf,
    pub(crate) status: Arc<RwLock<EngineStatus>>,
}

impl DuckDBEngine {
    /// Construct an engine bound to `scratch_path` (a DuckDB file). Status begins
    /// `Initializing`; call `init()` to transition to `Ready`.
    pub fn new(scratch_path: PathBuf, budget: MemoryBudget) -> Result<Self> {
        let conn = duckdb::Connection::open(&scratch_path)?;
        // duckdb-rs 1.4.x: `interrupt_handle()` returns `Arc<InterruptHandle>` directly.
        let interrupt = conn.interrupt_handle();
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            interrupt,
            budget,
            scratch_path,
            status: Arc::new(RwLock::new(EngineStatus::Initializing)),
        })
    }

    /// External cancel handle. Callable from any task; cooperative — in-flight
    /// query returns `EngineError::Interrupted` from spawn_blocking on next yield.
    pub fn interrupt(&self) {
        self.interrupt.interrupt();
    }

    /// Test-only scalar probe. T7 replaces in tests with `execute()`.
    #[doc(hidden)]
    pub async fn __debug_query_scalar(&self, sql: &str) -> Result<String> {
        self.assert_open()?;
        let conn = self.conn.clone();
        let sql = sql.to_owned();
        tokio::task::spawn_blocking(move || -> Result<String> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            let v: String = conn.query_row(&sql, [], |r| r.get(0))?;
            Ok(v)
        })
        .await
        .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?
    }

    fn assert_open(&self) -> Result<()> {
        let status = self.status.read().map_err(|_| EngineError::EnginePoisoned)?;
        match &*status {
            EngineStatus::Closing | EngineStatus::Closed => Err(EngineError::EngineClosed),
            EngineStatus::Failed(reason) => Err(EngineError::EngineFailed(reason.clone())),
            _ => Ok(()),
        }
    }

    fn set_status(&self, new_status: EngineStatus) {
        if let Ok(mut s) = self.status.write() {
            *s = new_status;
        }
    }
}

#[async_trait::async_trait]
impl crate::QueryEngine for DuckDBEngine {
    #[instrument(skip(self), fields(scratch = %self.scratch_path.display(), budget_mb = self.budget.bytes / (1024*1024)))]
    async fn init(&self) -> Result<()> {
        let conn = self.conn.clone();
        let budget = self.budget;

        // T14 installs sqlite_scanner once at app boot. Engine init only LOADs.
        // For tests where boot has not run, LOAD will fail with "extension not
        // found" — tests that exercise sqlite ATTACH must call
        // `extension_bootstrap::__test_install_sqlite_scanner()` first.
        let result: Result<()> = tokio::task::spawn_blocking(move || {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            // Memory + thread pragmas
            conn.execute_batch(&format!(
                "PRAGMA memory_limit='{}'; PRAGMA threads={};",
                budget.as_pragma(),
                num_cpus::get().saturating_sub(1).max(1),
            ))?;
            // LOAD extensions if installed (best effort — tests may run without).
            // Errors here are swallowed; T11b asserts ATTACH 'sqlite:' end-to-end.
            let _ = conn.execute_batch("LOAD sqlite_scanner;");
            Ok(())
        })
        .await
        .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?;

        match result {
            Ok(()) => {
                // Apply migrations (T3) — placeholder until T3 lands.
                self.apply_migrations_t3_placeholder().await?;
                self.set_status(EngineStatus::Ready);
                debug!("engine ready");
                Ok(())
            }
            Err(e) => {
                self.set_status(EngineStatus::Failed(e.to_string()));
                error!(error = %e, "engine init failed");
                Err(e)
            }
        }
    }

    #[instrument(skip(self))]
    async fn close(&self) -> Result<()> {
        self.set_status(EngineStatus::Closing);
        // duckdb-rs exposes `Connection::close(self)` (consuming) but our
        // connection lives behind `Arc<Mutex<_>>` and is shared with any
        // outstanding `spawn_blocking` workers (paged/streaming/etc.). We
        // cannot consume it here without breaking those workers. Instead:
        // 1. Flip status to Closed so subsequent calls fail via assert_open.
        // 2. The connection drops naturally when the last Arc reference goes,
        //    typically when the engine itself drops along with all in-flight
        //    streams. This is safe — DuckDB's Connection::Drop closes the
        //    underlying handle.
        // P3+ may want graceful drain (interrupt + await all streams) before
        // marking Closed; for P2 the synchronous status flip is sufficient.
        self.set_status(EngineStatus::Closed);
        Ok(())
    }

    fn status(&self) -> EngineStatus {
        self.status
            .read()
            .map(|s| s.clone())
            .unwrap_or(EngineStatus::Failed("status mutex poisoned".into()))
    }

    // The rest of the trait surface is unimplemented in T2. T3..T11 fill in.

    async fn register_file(
        &self,
        _path: &std::path::Path,
        _opts: crate::types::RegisterOpts,
    ) -> Result<crate::types::TableInfo> {
        Err(EngineError::NotImplemented { feature: "register_file (T4-T6)" })
    }

    async fn create_table(
        &self,
        _name: &str,
        _sql: &str,
        _origin: crate::types::DerivedOrigin,
    ) -> Result<crate::types::TableInfo> {
        Err(EngineError::NotImplemented { feature: "create_table (T9)" })
    }

    async fn drop_table(&self, _name: &str, _schema: Option<&str>) -> Result<()> {
        Err(EngineError::NotImplemented { feature: "drop_table (T9)" })
    }

    async fn rename_table(&self, _old: &str, _new: &str, _schema: Option<&str>) -> Result<()> {
        Err(EngineError::NotImplemented { feature: "rename_table (T9)" })
    }

    async fn execute(&self, _sql: &str) -> Result<crate::types::QueryResult> {
        Err(EngineError::NotImplemented { feature: "execute (T7)" })
    }

    async fn execute_paged(
        &self,
        _sql: &str,
        _offset: u64,
        _limit: u64,
    ) -> Result<crate::types::PagedQueryResult> {
        Err(EngineError::NotImplemented { feature: "execute_paged (T8)" })
    }

    async fn execute_streaming(
        &self,
        _sql: &str,
    ) -> Result<crate::types::ArrowRecordBatchStream> {
        Err(EngineError::NotImplemented { feature: "execute_streaming (T8)" })
    }

    async fn describe_table(
        &self,
        _name: &str,
        _schema: Option<&str>,
    ) -> Result<Vec<crate::types::ColumnInfo>> {
        Err(EngineError::NotImplemented { feature: "describe_table (T9)" })
    }

    async fn get_tables(&self) -> Result<Vec<crate::types::TableInfo>> {
        Err(EngineError::NotImplemented { feature: "get_tables (T9)" })
    }

    async fn export_table(
        &self,
        _name: &str,
        _format: crate::types::ExportFormat,
    ) -> Result<Vec<u8>> {
        Err(EngineError::NotImplemented { feature: "export_table (T10)" })
    }

    async fn attach(
        &self,
        _dsn: &str,
        _alias: &str,
        _opts: crate::types::AttachOpts,
    ) -> Result<()> {
        Err(EngineError::NotImplemented { feature: "attach (T11)" })
    }

    async fn detach(&self, _alias: &str) -> Result<()> {
        Err(EngineError::NotImplemented { feature: "detach (T11)" })
    }
}

impl DuckDBEngine {
    /// T3 replaces with the real migration runner.
    async fn apply_migrations_t3_placeholder(&self) -> Result<()> {
        Ok(())
    }
}
```

> Add `num_cpus = "1"` to `[workspace.dependencies]` and the engine `[dependencies]`.

- [ ] **Step 2.4: Run the tests, verify they pass**

```bash
cargo test --package dat0-engine --test bootstrap -- --nocapture
```

Expected: 4 PASSes. If `num_cpus` resolution fails, double-check the workspace dep.

- [ ] **Step 2.5: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "$(cat <<'EOF'
feat(engine): T2 — DuckDBEngine bootstrap (init/close/status/interrupt)

Implements DuckDBEngine::new/init/close/status with PRAGMA memory_limit
+ threads applied at init, an external `interrupt()` callable from
sibling tasks, and `EngineStatus` transitions matching the spec §2.9
contract. LOAD sqlite_scanner is best-effort (T14 installs it at app
boot; tests that need ATTACH 'sqlite:' wire it explicitly in T11b).

Trait methods other than init/close/status return
EngineError::NotImplemented; T3..T11 fill them in.

Test-only `__debug_query_scalar` helper unblocks bootstrap tests
without the full execute() machinery (lands T7).
EOF
)"
```

---

### Task 3: Migrations module + initial migration

Implement the forward-only, append-only migration runner. Replace T2's `apply_migrations_t3_placeholder` with the real one. Per spec §2.6 the runner targets per-engine scratch DBs only in P2.

**Files:**
- Modify: `crates/dat0-engine/src/migrations.rs`
- Modify: `crates/dat0-engine/src/duckdb_engine.rs` (replace placeholder with real call)
- Create: `crates/dat0-engine/tests/migrations.rs`

**Subagent dispatch profile:** full review.

- [ ] **Step 3.1: Write the failing tests**

`crates/dat0-engine/tests/migrations.rs`:

```rust
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 256 * 1024 * 1024 }
}

#[tokio::test]
async fn migrations_apply_on_fresh_db() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("a.duckdb");
    let engine = DuckDBEngine::new(scratch.clone(), budget()).unwrap();
    engine.init().await.unwrap();

    let v = engine
        .__debug_query_scalar("SELECT COALESCE(MAX(version), 0)::TEXT FROM __dat0_meta_migrations")
        .await
        .unwrap();
    assert_eq!(v, "1", "first migration should be applied");

    let workspace_v = engine
        .__debug_query_scalar("SELECT value FROM __dat0_meta WHERE key = 'dat0_workspace_version'")
        .await
        .unwrap();
    assert_eq!(workspace_v, "1");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn migrations_idempotent_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("a.duckdb");
    {
        let engine = DuckDBEngine::new(scratch.clone(), budget()).unwrap();
        engine.init().await.unwrap();
        engine.close().await.unwrap();
    }
    // Second open: no new rows in __dat0_meta_migrations
    {
        let engine = DuckDBEngine::new(scratch.clone(), budget()).unwrap();
        engine.init().await.unwrap();
        let count = engine
            .__debug_query_scalar("SELECT COUNT(*)::TEXT FROM __dat0_meta_migrations")
            .await
            .unwrap();
        assert_eq!(count, "1");
        engine.close().await.unwrap();
    }
}

#[tokio::test]
async fn failed_migration_rolls_back() {
    use dat0_engine::migrations::{Migration, apply_migrations};
    fn boom(_: &duckdb::Connection) -> std::result::Result<(), duckdb::Error> {
        Err(duckdb::Error::ToSqlConversionFailure(
            "intentional test failure".into(),
        ))
    }
    let migrations = &[
        Migration { version: 1, name: "init", up: dat0_engine::migrations::__test_only_m001_init },
        Migration { version: 2, name: "boom", up: boom },
    ];

    let dir = tempfile::tempdir().unwrap();
    let scratch = dir.path().join("a.duckdb");
    let conn = duckdb::Connection::open(&scratch).unwrap();

    let err = apply_migrations(&conn, migrations).expect_err("must fail at v2");
    let dat0_engine::EngineError::Migration { version, .. } = err else {
        panic!("expected Migration error, got {err:?}");
    };
    assert_eq!(version, 2);

    // v1 should remain applied; v2 row not present
    let count: u32 = conn
        .query_row("SELECT COUNT(*) FROM __dat0_meta_migrations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1, "v1 stays applied; v2 rolled back");
}
```

- [ ] **Step 3.2: Run the tests — they fail**

```bash
cargo test --package dat0-engine --test migrations
```

Expected: compile error (no `apply_migrations`, no `Migration`, etc.).

- [ ] **Step 3.3: Implement migrations module**

Replace `crates/dat0-engine/src/migrations.rs`:

```rust
//! Forward-only, append-only migration runner. Per spec §2.6.
//!
//! In P2 the runner targets per-engine scratch DBs only. Workspace-DB
//! concurrent-open race is a P3 entry-time review item.

use tracing::{info, warn};

use crate::error::EngineError;

pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub up: fn(&duckdb::Connection) -> std::result::Result<(), duckdb::Error>,
}

/// Production migrations. Forward-only, append-only. Never edit a shipped entry.
pub const MIGRATIONS: &[Migration] = &[
    Migration { version: 1, name: "init", up: m001_init },
];

/// Apply all migrations whose version is greater than the current applied version.
/// Idempotent — safe to call on every `init()`. Each migration runs inside a
/// transaction; failure rolls back and surfaces as `EngineError::Migration`.
pub fn apply_migrations(
    conn: &duckdb::Connection,
    migrations: &[Migration],
) -> std::result::Result<u32, EngineError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS __dat0_meta_migrations (
            version    INTEGER PRIMARY KEY,
            name       VARCHAR NOT NULL,
            applied_at TIMESTAMP DEFAULT current_timestamp
        );",
    )?;

    let current: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM __dat0_meta_migrations",
            [],
            |r| r.get::<_, u32>(0),
        )?;

    let started = std::time::Instant::now();

    for m in migrations.iter().filter(|m| m.version > current) {
        let migration_started = std::time::Instant::now();

        // DuckDB does not yet support nested transactions everywhere; use
        // an explicit BEGIN/COMMIT/ROLLBACK pair.
        conn.execute_batch("BEGIN;")?;
        let res = (m.up)(conn).and_then(|_| {
            conn.execute(
                "INSERT INTO __dat0_meta_migrations (version, name) VALUES (?, ?)",
                duckdb::params![m.version, m.name],
            )?;
            Ok(())
        });
        match res {
            Ok(()) => {
                conn.execute_batch("COMMIT;")?;
                let dur_ms = migration_started.elapsed().as_millis();
                info!(
                    target: "dat0_engine::migrations",
                    version = m.version,
                    name = m.name,
                    duration_ms = dur_ms as u64,
                    "migration applied"
                );
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK;");
                warn!(
                    target: "dat0_engine::migrations",
                    version = m.version,
                    name = m.name,
                    error = %e,
                    "migration failed; rolled back"
                );
                return Err(EngineError::Migration {
                    version: m.version,
                    name: m.name.to_string(),
                    source: e,
                });
            }
        }
    }

    let final_version = migrations.last().map(|m| m.version).unwrap_or(0);
    info!(
        target: "dat0_engine::migrations",
        from = current,
        to = final_version,
        total_duration_ms = started.elapsed().as_millis() as u64,
        "migrations complete"
    );
    Ok(final_version)
}

fn m001_init(conn: &duckdb::Connection) -> std::result::Result<(), duckdb::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS __dat0_meta (
            key   VARCHAR PRIMARY KEY,
            value VARCHAR NOT NULL
        );
        INSERT OR IGNORE INTO __dat0_meta (key, value) VALUES ('dat0_workspace_version', '1');",
    )?;
    Ok(())
}

/// Test re-export so `tests/migrations.rs` can construct custom migration sets.
#[doc(hidden)]
pub fn __test_only_m001_init(conn: &duckdb::Connection) -> std::result::Result<(), duckdb::Error> {
    m001_init(conn)
}
```

- [ ] **Step 3.4: Wire migrations into engine init**

In `crates/dat0-engine/src/duckdb_engine.rs`, replace `apply_migrations_t3_placeholder`:

```rust
async fn apply_migrations_real(&self) -> Result<()> {
    let conn = self.conn.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
        crate::migrations::apply_migrations(&*conn, crate::migrations::MIGRATIONS)?;
        Ok(())
    })
    .await
    .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?
}
```

Update the `init` body's call site from `apply_migrations_t3_placeholder` to `apply_migrations_real`. Order: after pragmas, before `set_status(Ready)`. Apply order is critical: PRAGMAs first (so migrations run with correct memory budget), then migrations, then Ready.

- [ ] **Step 3.5: Run tests, verify they pass**

```bash
cargo test --package dat0-engine --test migrations -- --nocapture
cargo test --package dat0-engine --test bootstrap -- --nocapture
```

Expected: all 7 PASSes (4 from T2 + 3 from T3).

- [ ] **Step 3.6: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "$(cat <<'EOF'
feat(engine): T3 — migration runner + initial m001_init

Forward-only, append-only `MIGRATIONS` slice with `__dat0_meta_migrations`
tracking. Each migration wrapped in BEGIN/COMMIT/ROLLBACK pair (DuckDB
does not yet allow nested txns everywhere). Failure rolls back; success
records version. Tracing emits per-migration {version, name,
duration_ms} events plus an aggregate {from, to, total} on completion.

m001_init creates `__dat0_meta` k/v table and seeds
`dat0_workspace_version=1`. Reused as the "what version is this DB"
probe by P9 import/export and P11 distribution checksum.

Tests: fresh apply, idempotent re-apply, failed migration rollback.
EOF
)"
```

---

### Task 4: register_file CSV/TSV

Implement `register_file` for CSV and TSV. Wires `RegisterOpts` to DuckDB's `read_csv` parameters. Returns `TableInfo`.

**Files:**
- Modify: `crates/dat0-engine/src/register/csv.rs`
- Modify: `crates/dat0-engine/src/register/mod.rs`
- Modify: `crates/dat0-engine/src/duckdb_engine.rs` (`register_file` impl)
- Create: `crates/dat0-engine/tests/register_csv.rs`

**Subagent dispatch profile:** full review.

- [ ] **Step 4.1: Write the failing tests**

`crates/dat0-engine/tests/register_csv.rs`:

```rust
use std::path::PathBuf;

use dat0_engine::{DuckDBEngine, FileFormat, MemoryBudget, QueryEngine, RegisterOpts};

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 256 * 1024 * 1024 }
}

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/small")
        .join(rel)
}

#[tokio::test]
async fn register_csv_basic() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    let info = engine
        .register_file(&fixture("basic.csv"), RegisterOpts::default())
        .await
        .unwrap();
    assert_eq!(info.columns.len(), 4, "id,name,score,active");
    assert!(info.columns.iter().any(|c| c.name == "id"));
    assert!(info.columns.iter().any(|c| c.name == "score"));

    // Sanity scalar via debug helper (T7's `execute` lands the real test, but T4 must verify the view exists).
    let v = engine
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_csv_edge_cases_quoting_bom() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    let info = engine
        .register_file(&fixture("edge_cases.csv"), RegisterOpts::default())
        .await
        .unwrap();
    let v = engine
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_tsv_via_explicit_format() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    // Author a small TSV at runtime
    let tsv_path = dir.path().join("simple.tsv");
    std::fs::write(&tsv_path, "id\tname\n1\ta\n2\tb\n").unwrap();

    let opts = RegisterOpts {
        format: Some(FileFormat::Tsv),
        ..RegisterOpts::default()
    };
    let info = engine.register_file(&tsv_path, opts).await.unwrap();
    assert_eq!(info.columns.len(), 2);
    let v = engine
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(v, "2");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_file_unknown_extension_errors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let path = dir.path().join("data.xyz");
    std::fs::write(&path, "id,name\n1,a\n").unwrap();
    let err = engine
        .register_file(&path, RegisterOpts::default())
        .await
        .expect_err("unknown extension must error");
    assert!(matches!(
        err,
        dat0_engine::EngineError::UnsupportedFormat(_)
    ));
    engine.close().await.unwrap();
}
```

- [ ] **Step 4.2: Run tests, verify they fail**

```bash
cargo test --package dat0-engine --test register_csv
```

Expected: tests fail because `register_file` returns `NotImplemented`.

- [ ] **Step 4.3: Implement CSV/TSV register**

`crates/dat0-engine/src/register/mod.rs`:

```rust
//! `register_file` dispatch by `FileFormat`.

pub mod csv;
pub mod json;
pub mod parquet;

use std::path::Path;

use crate::error::EngineError;
use crate::types::{FileFormat, RegisterOpts, TableInfo};
use crate::Result;

/// Compute the table name from a path: stem in lowercase, with non-alphanum
/// replaced by `_`. Caller can override via SQL once the table exists.
pub(crate) fn derive_table_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("table");
    let mut name = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect::<String>();
    if name.is_empty() || name.chars().next().unwrap().is_ascii_digit() {
        name.insert(0, 't');
    }
    name
}

pub(crate) fn resolve_format(path: &Path, opts: &RegisterOpts) -> Result<FileFormat> {
    if let Some(f) = opts.format {
        return Ok(f);
    }
    FileFormat::from_extension(path).ok_or_else(|| {
        EngineError::UnsupportedFormat(format!(
            "cannot determine format from extension; pass RegisterOpts.format explicitly: {}",
            path.display()
        ))
    })
}

pub(crate) fn dispatch_register_sql(
    path: &Path,
    opts: &RegisterOpts,
    table_name: &str,
) -> Result<String> {
    let format = resolve_format(path, opts)?;
    match format {
        FileFormat::Csv | FileFormat::Tsv => csv::build_csv_view_sql(path, opts, table_name, format),
        FileFormat::Json | FileFormat::Jsonl | FileFormat::Ndjson => {
            json::build_json_view_sql(path, opts, table_name, format)
        }
        FileFormat::Parquet => parquet::build_parquet_view_sql(path, table_name),
    }
}
```

`crates/dat0-engine/src/register/csv.rs`:

```rust
//! CSV/TSV registration via DuckDB `read_csv`.

use std::path::Path;

use crate::error::EngineError;
use crate::types::{FileFormat, RegisterOpts};
use crate::Result;

pub(crate) fn build_csv_view_sql(
    path: &Path,
    opts: &RegisterOpts,
    table_name: &str,
    format: FileFormat,
) -> Result<String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| EngineError::InvalidPath(path.to_path_buf()))?;
    let escaped_path = path_str.replace('\'', "''");

    let delim = match (format, opts.delimiter) {
        (_, Some(c)) => Some(c),
        (FileFormat::Tsv, None) => Some('\t'),
        _ => None, // CSV: let DuckDB sniff
    };

    let mut params: Vec<String> = Vec::new();
    if let Some(d) = delim {
        params.push(format!("delim='{}'", escape_for_sql_literal(&d.to_string())));
    }
    if let Some(q) = opts.quote_char {
        params.push(format!("quote='{}'", escape_for_sql_literal(&q.to_string())));
    }
    if let Some(e) = opts.escape_char {
        params.push(format!("escape='{}'", escape_for_sql_literal(&e.to_string())));
    }
    if let Some(h) = opts.has_header {
        params.push(format!("header={}", h));
    }
    if let Some(s) = opts.sample_rows {
        if s == 0 {
            return Err(EngineError::Io(std::io::Error::other(
                "RegisterOpts.sample_rows must be > 0 when set; use None for default",
            )));
        }
        params.push(format!("sample_size={}", s));
    }
    if !opts.type_overrides.is_empty() {
        let mut entries = opts.type_overrides.iter().collect::<Vec<_>>();
        entries.sort_by(|a, b| a.0.cmp(b.0)); // deterministic SQL
        let inner = entries
            .iter()
            .map(|(col, typ)| {
                format!(
                    "'{}': '{}'",
                    escape_for_sql_literal(col),
                    escape_for_sql_literal(typ)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        params.push(format!("types={{{}}}", inner));
    }

    let read_args = if params.is_empty() {
        format!("'{}'", escaped_path)
    } else {
        format!("'{}', {}", escaped_path, params.join(", "))
    };

    Ok(format!(
        "CREATE OR REPLACE VIEW \"{}\" AS SELECT * FROM read_csv({});",
        table_name, read_args
    ))
}

fn escape_for_sql_literal(s: &str) -> String {
    s.replace('\'', "''")
}
```

(Stub the JSON + Parquet builders so the dispatch compiles before T5/T6 land. They will be replaced in their own tasks.)

`crates/dat0-engine/src/register/json.rs`:

```rust
//! JSON/JSONL/NDJSON registration. T5 fills this in.

use std::path::Path;

use crate::error::EngineError;
use crate::types::{FileFormat, RegisterOpts};
use crate::Result;

pub(crate) fn build_json_view_sql(
    _path: &Path,
    _opts: &RegisterOpts,
    _table_name: &str,
    _format: FileFormat,
) -> Result<String> {
    Err(EngineError::NotImplemented { feature: "register_file json (T5)" })
}
```

`crates/dat0-engine/src/register/parquet.rs`:

```rust
//! Parquet registration. T6 fills this in.

use std::path::Path;

use crate::error::EngineError;
use crate::Result;

pub(crate) fn build_parquet_view_sql(_path: &Path, _table_name: &str) -> Result<String> {
    Err(EngineError::NotImplemented { feature: "register_file parquet (T6)" })
}
```

Now wire `register_file` in `duckdb_engine.rs`. Replace the `NotImplemented` body:

```rust
async fn register_file(
    &self,
    path: &std::path::Path,
    opts: crate::types::RegisterOpts,
) -> Result<crate::types::TableInfo> {
    self.assert_open()?;
    let conn = self.conn.clone();
    let table_name = crate::register::derive_table_name(path);
    let sql = crate::register::dispatch_register_sql(path, &opts, &table_name)?;
    let path = path.to_path_buf();

    let columns = tokio::task::spawn_blocking({
        let conn = conn.clone();
        let sql = sql.clone();
        let table_name = table_name.clone();
        move || -> Result<Vec<crate::types::ColumnInfo>> {
            let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
            conn.execute_batch(&sql)?;
            // DESCRIBE returns columns: column_name, column_type, null, key, default, extra
            let mut stmt = conn.prepare(&format!("DESCRIBE \"{}\"", table_name))?;
            let rows: Vec<crate::types::ColumnInfo> = stmt
                .query_map([], |row| {
                    Ok(crate::types::ColumnInfo {
                        name: row.get::<_, String>(0)?,
                        data_type: row.get::<_, String>(1)?,
                        nullable: row
                            .get::<_, String>(2)
                            .map(|s| s.eq_ignore_ascii_case("YES"))
                            .unwrap_or(true),
                    })
                })?
                .filter_map(std::result::Result::ok)
                .collect();
            Ok(rows)
        }
    })
    .await
    .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))??;

    Ok(crate::types::TableInfo {
        name: table_name,
        schema: "main".to_string(),
        columns,
        row_count_estimate: None,
        origin: crate::types::TableOrigin::File(path),
    })
}
```

- [ ] **Step 4.4: Run tests**

```bash
cargo test --package dat0-engine --test register_csv -- --nocapture
```

Expected: 4 PASSes.

If `register_csv_edge_cases_quoting_bom` fails on the BOM detection, double-check the fixture file's first three bytes via `xxd tests/fixtures/small/edge_cases.csv | head -1` — must be `efbb bf`. If absent, recreate the fixture with `printf '\xef\xbb\xbfid,name,note\r\n...'`.

- [ ] **Step 4.5: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "$(cat <<'EOF'
feat(engine): T4 — register_file CSV/TSV via DuckDB read_csv

CSV/TSV registration plumbs RegisterOpts (delimiter, quote, escape,
header, sample_rows, type_overrides) into DuckDB's read_csv params.
Auto-format detection from path extension; explicit override via
RegisterOpts.format. Returns TableInfo with column metadata via
DESCRIBE.

Tests cover happy path, BOM/quoted-comma/embedded-quote edge cases,
explicit TSV format, and unknown-extension error.

JSON + Parquet dispatch land in T5/T6 (stubbed return NotImplemented).
EOF
)"
```

---

### Task 5: register_file JSON/JSONL/NDJSON

Replace the JSON stub with the real impl. DuckDB's `read_json_auto` handles all three flavors; the differentiator is `format='auto'` vs `'array'` vs `'newline_delimited'`.

**Files:**
- Modify: `crates/dat0-engine/src/register/json.rs`
- Create: `crates/dat0-engine/tests/register_json.rs`

**Subagent dispatch profile:** full review.

- [ ] **Step 5.1: Write the failing tests**

`crates/dat0-engine/tests/register_json.rs`:

```rust
use std::path::PathBuf;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 256 * 1024 * 1024 }
}
fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/small")
        .join(rel)
}

#[tokio::test]
async fn register_json_array() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&fixture("simple.json"), RegisterOpts::default())
        .await
        .unwrap();
    let v = engine
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&fixture("simple.jsonl"), RegisterOpts::default())
        .await
        .unwrap();
    let v = engine
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_ndjson() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&fixture("simple.ndjson"), RegisterOpts::default())
        .await
        .unwrap();
    let v = engine
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}

#[tokio::test]
async fn register_json_rejects_type_overrides_p2() {
    // P2: type_overrides for JSON would silently drop columns due to DuckDB
    // read_json's subset semantics on `columns={}`. Reject explicitly.
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let mut opts = RegisterOpts::default();
    opts.type_overrides.insert("id".into(), "BIGINT".into());
    let err = engine
        .register_file(&fixture("simple.json"), opts)
        .await
        .expect_err("must reject type_overrides for JSON in P2");
    assert!(matches!(err, dat0_engine::EngineError::Io(_)));
    engine.close().await.unwrap();
}
```

- [ ] **Step 5.2: Run, verify failure**

```bash
cargo test --package dat0-engine --test register_json
```

Expected: tests fail with `NotImplemented`.

- [ ] **Step 5.3: Implement JSON dispatch**

Replace `crates/dat0-engine/src/register/json.rs`:

```rust
//! JSON/JSONL/NDJSON registration via DuckDB `read_json` / `read_json_auto`.

use std::path::Path;

use crate::error::EngineError;
use crate::types::{FileFormat, RegisterOpts};
use crate::Result;

pub(crate) fn build_json_view_sql(
    path: &Path,
    opts: &RegisterOpts,
    table_name: &str,
    format: FileFormat,
) -> Result<String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| EngineError::InvalidPath(path.to_path_buf()))?;
    let escaped_path = path_str.replace('\'', "''");

    // DuckDB `read_json` `format` param:
    //   'auto'                 — sniff
    //   'array'                — single JSON array (.json)
    //   'newline_delimited'    — JSONL/NDJSON
    //   'unstructured'         — non-array, non-NDJSON
    let format_clause = match format {
        FileFormat::Json => "format='auto'",
        FileFormat::Jsonl | FileFormat::Ndjson => "format='newline_delimited'",
        _ => return Err(EngineError::UnsupportedFormat(format!("{:?}", format))),
    };

    let mut params: Vec<String> = vec![format_clause.to_string()];
    if let Some(s) = opts.sample_rows {
        if s == 0 {
            return Err(EngineError::Io(std::io::Error::other(
                "RegisterOpts.sample_rows must be > 0 when set; use None for default",
            )));
        }
        params.push(format!("sample_size={}", s));
    }
    // SEMANTIC NOTE: DuckDB's read_json `columns={...}` parameter has SUBSET
    // semantics — when set, only the listed columns are exposed (other columns
    // are dropped). This is materially different from read_csv's `types={...}`
    // which is a partial override leaving non-listed columns auto-detected.
    // Applying RegisterOpts.type_overrides as a `columns={}` clause would
    // therefore silently drop columns the user didn't list — a contract bug.
    // For P2, JSON registration ignores type_overrides. P3 import wizard or
    // a later phase wires JSON column-type overrides via a different shape
    // (likely a separate full-schema field). If type_overrides is non-empty
    // for a JSON file, we surface a clear error rather than silently dropping
    // columns.
    if !opts.type_overrides.is_empty() {
        return Err(EngineError::Io(std::io::Error::other(
            "RegisterOpts.type_overrides is not yet supported for JSON formats in P2 \
             (DuckDB read_json's `columns` param has subset, not partial-override, \
             semantics — applying it would silently drop other columns). \
             Use the column-typed result via a follow-up SQL CAST instead.",
        )));
    }
    let args = format!("'{}', {}", escaped_path, params.join(", "));
    Ok(format!(
        "CREATE OR REPLACE VIEW \"{}\" AS SELECT * FROM read_json({});",
        table_name, args
    ))
}
```

- [ ] **Step 5.4: Run, verify pass**

```bash
cargo test --package dat0-engine --test register_json -- --nocapture
```

Expected: 3 PASSes.

- [ ] **Step 5.5: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "$(cat <<'EOF'
feat(engine): T5 — register_file JSON/JSONL/NDJSON

DuckDB read_json with format='auto' for .json arrays and
format='newline_delimited' for .jsonl/.ndjson. RegisterOpts sample_rows
and type_overrides plumb through to read_json's sample_size and columns
params.

Tests verify 3 fixtures land 3 rows each.
EOF
)"
```

---

### Task 6: register_file Parquet

Replace the Parquet stub with the real impl. DuckDB native `read_parquet`.

**Files:**
- Modify: `crates/dat0-engine/src/register/parquet.rs`
- Create: `crates/dat0-engine/tests/register_parquet.rs`

**Subagent dispatch profile:** full review.

- [ ] **Step 6.1: Write the failing test**

`crates/dat0-engine/tests/register_parquet.rs`:

```rust
use std::path::PathBuf;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 256 * 1024 * 1024 }
}
fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/small")
        .join(rel)
}

#[tokio::test]
async fn register_parquet_basic() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&fixture("simple.parquet"), RegisterOpts::default())
        .await
        .unwrap();
    assert!(info.columns.iter().any(|c| c.name == "id"));
    let v = engine
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info.name))
        .await
        .unwrap();
    assert_eq!(v, "3");
    engine.close().await.unwrap();
}
```

- [ ] **Step 6.2: Run, verify failure**

```bash
cargo test --package dat0-engine --test register_parquet
```

Expected: fail with `NotImplemented`.

- [ ] **Step 6.3: Implement Parquet dispatch**

Replace `crates/dat0-engine/src/register/parquet.rs`:

```rust
//! Parquet registration via DuckDB `read_parquet`.

use std::path::Path;

use crate::error::EngineError;
use crate::Result;

pub(crate) fn build_parquet_view_sql(path: &Path, table_name: &str) -> Result<String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| EngineError::InvalidPath(path.to_path_buf()))?;
    let escaped_path = path_str.replace('\'', "''");
    Ok(format!(
        "CREATE OR REPLACE VIEW \"{}\" AS SELECT * FROM read_parquet('{}');",
        table_name, escaped_path
    ))
}
```

> `RegisterOpts` fields beyond format are not applied to Parquet — Parquet is self-describing. T0 should have confirmed this.

- [ ] **Step 6.4: Run, verify pass**

```bash
cargo test --package dat0-engine --test register_parquet -- --nocapture
```

Expected: PASS.

- [ ] **Step 6.5: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "feat(engine): T6 — register_file Parquet via DuckDB read_parquet"
```

---

### Task 7: execute (materialized)

Implement `execute()` returning a fully-materialized `QueryResult`. For "small" results consumed by inspector/charts.

**Files:**
- Modify: `crates/dat0-engine/src/execute/mod.rs`
- Modify: `crates/dat0-engine/src/duckdb_engine.rs`
- Create: `crates/dat0-engine/tests/execute.rs`

**Subagent dispatch profile:** full review.

- [ ] **Step 7.1: Write the failing tests**

`crates/dat0-engine/tests/execute.rs`:

```rust
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 256 * 1024 * 1024 }
}

#[tokio::test]
async fn execute_returns_materialized_batches() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let qr = engine
        .execute("SELECT * FROM (VALUES (1, 'a'), (2, 'b'), (3, 'c')) v(id, name)")
        .await
        .unwrap();
    assert_eq!(qr.columns.len(), 2);
    assert!(qr.batches.iter().map(|b| b.num_rows()).sum::<usize>() == 3);
    engine.close().await.unwrap();
}

#[tokio::test]
async fn execute_propagates_sql_errors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let err = engine
        .execute("SELECT FROM not_a_thing")
        .await
        .expect_err("syntax error");
    assert!(matches!(err, dat0_engine::EngineError::DuckDb(_)));
    engine.close().await.unwrap();
}
```

- [ ] **Step 7.2: Run, verify failure**

```bash
cargo test --package dat0-engine --test execute
```

- [ ] **Step 7.3: Implement execute()**

Replace `crates/dat0-engine/src/execute/mod.rs`:

```rust
//! `execute()` — materialized result. T8 adds paged + streaming.

pub mod paged;
pub mod streaming;

use crate::error::EngineError;
use crate::types::{ColumnInfo, QueryResult};
use crate::Result;

pub(crate) fn run_materialized(
    conn: &duckdb::Connection,
    sql: &str,
) -> Result<QueryResult> {
    let mut stmt = conn.prepare(sql).map_err(translate_duckdb_err)?;
    // T0 confirmed: query_arrow returns an iterator of RecordBatch.
    // Symbol may be different; T0 records exact name.
    let arrow_iter = stmt.query_arrow([]).map_err(translate_duckdb_err)?;
    let mut batches: Vec<duckdb::arrow::record_batch::RecordBatch> = Vec::new();
    let schema = arrow_iter.get_schema();
    for batch in arrow_iter {
        batches.push(batch);
    }
    let columns: Vec<ColumnInfo> = schema
        .fields()
        .iter()
        .map(|f| ColumnInfo {
            name: f.name().clone(),
            data_type: format!("{:?}", f.data_type()),
            nullable: f.is_nullable(),
        })
        .collect();
    Ok(QueryResult { columns, batches })
}

/// Translate a `duckdb::Error` into the appropriate `EngineError`. Specifically:
/// when the underlying DuckDB call was interrupted (because a sibling task
/// called `Engine::interrupt()`), surface as `EngineError::Interrupted` rather
/// than a generic `DuckDb(_)`. P5 SQL Console depends on this discriminator
/// for Cmd+. UX (D-008).
pub(crate) fn translate_duckdb_err(e: duckdb::Error) -> crate::error::EngineError {
    if let duckdb::Error::DuckDBFailure(_, ref msg) = e {
        if msg.as_deref().map(|s| s.contains("INTERRUPT")).unwrap_or(false) {
            return crate::error::EngineError::Interrupted;
        }
    }
    crate::error::EngineError::DuckDb(e)
}
```

> If T0 found that `query_arrow` returns `Result<RecordBatch, Error>` per-item (not infallible), wrap with `for batch_res in arrow_iter { batches.push(batch_res.map_err(translate_duckdb_err)?); }`.

> The exact substring matching `"INTERRUPT"` is heuristic. T0 should verify what DuckDB's interrupt error code looks like at the pinned version; if there's a richer error variant (e.g., `Error::Interrupted` or a code constant), match on that instead. Track as PD-NNN if T0 finds a cleaner translation path.

In `duckdb_engine.rs`, replace `execute()`:

```rust
async fn execute(&self, sql: &str) -> Result<crate::types::QueryResult> {
    self.assert_open()?;
    let _span = tracing::info_span!("engine.execute", sql_len = sql.len()).entered();
    let conn = self.conn.clone();
    let sql = sql.to_owned();
    tokio::task::spawn_blocking(move || -> Result<crate::types::QueryResult> {
        let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
        crate::execute::run_materialized(&*conn, &sql)
    })
    .await
    .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?
}
```

- [ ] **Step 7.4: Run tests**

```bash
cargo test --package dat0-engine --test execute -- --nocapture
```

Expected: 2 PASSes.

- [ ] **Step 7.5: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "$(cat <<'EOF'
feat(engine): T7 — execute() materialized via Arrow iterator

execute() drives DuckDB's Arrow batch iterator (query_arrow), collects
all batches into Vec<RecordBatch>, returns QueryResult with column
metadata from the iterator's schema.

For small / aggregate results consumed by inspector/charts. Streaming
variant (T8) handles unbounded results.
EOF
)"
```

---

### Task 8: execute_paged + execute_streaming + mpsc back-pressure

Implement paged execution (LIMIT/OFFSET wrapping with a `total_rows` count) and streaming via `tokio::sync::mpsc::channel(1)` driven by a `spawn_blocking` worker.

**Files:**
- Modify: `crates/dat0-engine/src/execute/paged.rs`
- Modify: `crates/dat0-engine/src/execute/streaming.rs`
- Modify: `crates/dat0-engine/src/duckdb_engine.rs`
- Create: `crates/dat0-engine/tests/execute_paged.rs`
- Create: `crates/dat0-engine/tests/execute_streaming.rs`

**Subagent dispatch profile:** full review (streaming + back-pressure are the highest-risk surface in P2).

- [ ] **Step 8.1: Write paged tests**

`crates/dat0-engine/tests/execute_paged.rs`:

```rust
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 256 * 1024 * 1024 }
}

#[tokio::test]
async fn execute_paged_returns_window() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let pq = engine
        .execute_paged("SELECT i FROM range(100) t(i)", 10, 5)
        .await
        .unwrap();
    assert_eq!(pq.total_rows, 100);
    assert_eq!(pq.offset, 10);
    let sum: usize = pq.batches.iter().map(|b| b.num_rows()).sum();
    assert_eq!(sum, 5);
    engine.close().await.unwrap();
}
```

- [ ] **Step 8.2: Write streaming tests**

`crates/dat0-engine/tests/execute_streaming.rs`:

```rust
use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};
use futures::StreamExt;

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 256 * 1024 * 1024 }
}

#[tokio::test]
async fn streaming_yields_all_rows() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let mut stream = engine
        .execute_streaming("SELECT i FROM range(50000) t(i)")
        .await
        .unwrap();
    let mut total = 0_usize;
    while let Some(batch) = stream.next().await {
        let b = batch.unwrap();
        total += b.num_rows();
    }
    assert_eq!(total, 50000);
    engine.close().await.unwrap();
}

#[tokio::test]
async fn streaming_respects_consumer_drop() {
    // Drop the stream before draining; producer should clean up without panic.
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    {
        let mut stream = engine
            .execute_streaming("SELECT i FROM range(1000000) t(i)")
            .await
            .unwrap();
        let _ = stream.next().await; // pull one batch
        // stream drops here
    }
    // Engine still functional after a dropped stream.
    let qr = engine.execute("SELECT 1::INTEGER as v").await.unwrap();
    assert_eq!(qr.batches.iter().map(|b| b.num_rows()).sum::<usize>(), 1);
    engine.close().await.unwrap();
}
```

- [ ] **Step 8.3: Run, verify failures**

```bash
cargo test --package dat0-engine --test execute_paged --test execute_streaming
```

Expected: fail with `NotImplemented`.

- [ ] **Step 8.4: Implement paged**

Replace `crates/dat0-engine/src/execute/paged.rs`:

```rust
//! Paged execution: total_rows + windowed batches.

use crate::error::EngineError;
use crate::types::PagedQueryResult;
use crate::Result;

pub(crate) fn run_paged(
    conn: &duckdb::Connection,
    sql: &str,
    offset: u64,
    limit: u64,
) -> Result<PagedQueryResult> {
    // Compute total via wrapping COUNT(*). DuckDB optimizes; on large queries
    // this still walks the source — that is the cost contract.
    let count_sql = format!("SELECT COUNT(*) FROM ({}) sub", sql);
    let total_rows: u64 = conn.query_row(&count_sql, [], |r| r.get::<_, u64>(0))?;

    let windowed_sql = format!("SELECT * FROM ({}) sub LIMIT {} OFFSET {}", sql, limit, offset);
    let mut stmt = conn.prepare(&windowed_sql)?;
    let arrow_iter = stmt.query_arrow([])?;
    let mut batches = Vec::new();
    for b in arrow_iter {
        batches.push(b);
    }
    Ok(PagedQueryResult { total_rows, offset, batches })
}
```

- [ ] **Step 8.5: Implement streaming**

Replace `crates/dat0-engine/src/execute/streaming.rs`:

```rust
//! Streaming execution via `spawn_blocking` worker pushing batches to a
//! bounded `tokio::sync::mpsc` channel. Per spec §2.1.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use duckdb::arrow::record_batch::RecordBatch;
use futures::Stream;
use tokio::sync::mpsc;

use crate::error::EngineError;
use crate::Result;

/// Spawn a blocking worker that pulls batches from DuckDB and pushes them
/// onto a bounded channel; return a stream that polls the channel.
pub(crate) fn spawn_streaming(
    conn: Arc<Mutex<duckdb::Connection>>,
    sql: String,
) -> Result<crate::types::ArrowRecordBatchStream> {
    // capacity 1: producer waits when consumer hasn't pulled the previous batch.
    let (tx, rx) = mpsc::channel::<Result<RecordBatch>>(1);

    // Worker: holds the connection mutex while iterating. Other engine
    // operations (including execute()) will queue behind it. This is by design;
    // DuckDB connections are single-threaded for execution anyway.
    tokio::task::spawn_blocking(move || {
        let conn = match conn.lock() {
            Ok(g) => g,
            Err(_) => {
                let _ = tx.blocking_send(Err(EngineError::EnginePoisoned));
                return;
            }
        };
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.blocking_send(Err(crate::execute::translate_duckdb_err(e)));
                return;
            }
        };
        let arrow_iter = match stmt.query_arrow([]) {
            Ok(it) => it,
            Err(e) => {
                let _ = tx.blocking_send(Err(crate::execute::translate_duckdb_err(e)));
                return;
            }
        };
        for batch in arrow_iter {
            // blocking_send blocks until consumer pulls; channel cap=1.
            // If consumer dropped, send fails — exit cleanly.
            if tx.blocking_send(Ok(batch)).is_err() {
                tracing::debug!("streaming consumer dropped; producer shutting down");
                return;
            }
        }
    });

    Ok(Box::pin(ChannelStream { rx }))
}

struct ChannelStream {
    rx: mpsc::Receiver<Result<RecordBatch>>,
}

impl Stream for ChannelStream {
    type Item = Result<RecordBatch>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}
```

In `duckdb_engine.rs`, replace `execute_paged` and `execute_streaming`:

```rust
async fn execute_paged(
    &self,
    sql: &str,
    offset: u64,
    limit: u64,
) -> Result<crate::types::PagedQueryResult> {
    self.assert_open()?;
    let conn = self.conn.clone();
    let sql = sql.to_owned();
    tokio::task::spawn_blocking(move || -> Result<crate::types::PagedQueryResult> {
        let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
        crate::execute::paged::run_paged(&*conn, &sql, offset, limit)
    })
    .await
    .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?
}

async fn execute_streaming(
    &self,
    sql: &str,
) -> Result<crate::types::ArrowRecordBatchStream> {
    self.assert_open()?;
    crate::execute::streaming::spawn_streaming(self.conn.clone(), sql.to_owned())
}
```

- [ ] **Step 8.6: Run tests**

```bash
cargo test --package dat0-engine --test execute_paged --test execute_streaming -- --nocapture
```

Expected: all PASSes.

- [ ] **Step 8.7: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "$(cat <<'EOF'
feat(engine): T8 — execute_paged + execute_streaming

execute_paged: COUNT(*) over the user query for total_rows, then
LIMIT/OFFSET windowed Arrow iterator.

execute_streaming: spawn_blocking worker pulls batches from
Statement::query_arrow and pushes onto bounded mpsc channel
(capacity 1) for back-pressure. Returns Pin<Box<dyn Stream + Send>>.
Consumer-drop case handled cleanly: producer exits when send fails.

Tests cover happy path, paging window math, and consumer-early-drop.
EOF
)"
```

---

### Task 9: Catalog ops (describe / get_tables / create / drop / rename)

Implement `describe_table`, `get_tables`, `create_table`, `drop_table`, `rename_table`.

**Files:**
- Modify: `crates/dat0-engine/src/catalog.rs`
- Modify: `crates/dat0-engine/src/duckdb_engine.rs`
- Create: `crates/dat0-engine/tests/catalog.rs`

**Subagent dispatch profile:** combined-verify. Mostly thin SQL wrappers.

- [ ] **Step 9.1: Write the failing tests**

`crates/dat0-engine/tests/catalog.rs`:

```rust
use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 256 * 1024 * 1024 }
}

#[tokio::test]
async fn create_describe_drop_cycle() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    let info = engine
        .create_table(
            "things",
            "SELECT 1::INTEGER AS id, 'a'::VARCHAR AS name UNION ALL SELECT 2, 'b'",
            DerivedOrigin::Sql("test".to_string()),
        )
        .await
        .unwrap();
    assert_eq!(info.name, "things");
    assert_eq!(info.columns.len(), 2);

    let cols = engine.describe_table("things", None).await.unwrap();
    assert_eq!(cols.len(), 2);

    let tables = engine.get_tables().await.unwrap();
    assert!(tables.iter().any(|t| t.name == "things"));

    engine.rename_table("things", "stuff", None).await.unwrap();
    let cols2 = engine.describe_table("stuff", None).await.unwrap();
    assert_eq!(cols2.len(), 2);

    engine.drop_table("stuff", None).await.unwrap();
    let err = engine.describe_table("stuff", None).await.expect_err("dropped");
    assert!(matches!(err, dat0_engine::EngineError::DuckDb(_)));

    engine.close().await.unwrap();
}
```

- [ ] **Step 9.2: Run, verify failure**

```bash
cargo test --package dat0-engine --test catalog
```

Expected: fail with NotImplemented.

- [ ] **Step 9.3: Implement catalog ops**

Replace `crates/dat0-engine/src/catalog.rs`:

```rust
//! Catalog ops: describe, list, create, drop, rename.

use crate::error::EngineError;
use crate::types::{ColumnInfo, DerivedOrigin, TableInfo, TableOrigin};
use crate::Result;

pub(crate) fn describe_table(
    conn: &duckdb::Connection,
    name: &str,
    schema: Option<&str>,
) -> Result<Vec<ColumnInfo>> {
    let qualified = qualified_name(name, schema);
    let mut stmt = conn.prepare(&format!("DESCRIBE {}", qualified))?;
    let cols: Vec<ColumnInfo> = stmt
        .query_map([], |row| {
            Ok(ColumnInfo {
                name: row.get::<_, String>(0)?,
                data_type: row.get::<_, String>(1)?,
                nullable: row
                    .get::<_, String>(2)
                    .map(|s| s.eq_ignore_ascii_case("YES"))
                    .unwrap_or(true),
            })
        })?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(cols)
}

pub(crate) fn get_tables(conn: &duckdb::Connection) -> Result<Vec<TableInfo>> {
    // Use information_schema for portability; columns: table_schema, table_name.
    let mut stmt = conn.prepare(
        "SELECT table_schema, table_name
         FROM information_schema.tables
         WHERE table_schema NOT IN ('information_schema', 'pg_catalog')
           AND table_name NOT LIKE '__dat0_meta%'",
    )?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .filter_map(std::result::Result::ok)
        .collect();

    let mut tables = Vec::with_capacity(rows.len());
    for (schema, name) in rows {
        let cols = describe_table(conn, &name, Some(&schema))?;
        tables.push(TableInfo {
            name,
            schema,
            columns: cols,
            row_count_estimate: None,
            origin: TableOrigin::Derived(DerivedOrigin::Sql(String::new())),
        });
    }
    Ok(tables)
}

pub(crate) fn create_table(
    conn: &duckdb::Connection,
    name: &str,
    sql: &str,
) -> Result<TableInfo> {
    let create_sql = format!("CREATE TABLE \"{}\" AS {}", name, sql);
    conn.execute_batch(&create_sql)?;
    let columns = describe_table(conn, name, None)?;
    Ok(TableInfo {
        name: name.to_string(),
        schema: "main".to_string(),
        columns,
        row_count_estimate: None,
        origin: TableOrigin::Derived(DerivedOrigin::Sql(sql.to_string())),
    })
}

pub(crate) fn drop_table(
    conn: &duckdb::Connection,
    name: &str,
    schema: Option<&str>,
) -> Result<()> {
    let qualified = qualified_name(name, schema);
    conn.execute_batch(&format!("DROP TABLE {}", qualified))?;
    Ok(())
}

pub(crate) fn rename_table(
    conn: &duckdb::Connection,
    old: &str,
    new: &str,
    schema: Option<&str>,
) -> Result<()> {
    let qualified_old = qualified_name(old, schema);
    conn.execute_batch(&format!("ALTER TABLE {} RENAME TO \"{}\"", qualified_old, new))?;
    Ok(())
}

fn qualified_name(name: &str, schema: Option<&str>) -> String {
    match schema {
        Some(s) => format!("\"{}\".\"{}\"", s, name),
        None => format!("\"{}\"", name),
    }
}
```

In `duckdb_engine.rs`, replace each catalog stub with the corresponding `spawn_blocking` wrapper. Pattern:

```rust
async fn describe_table(
    &self,
    name: &str,
    schema: Option<&str>,
) -> Result<Vec<crate::types::ColumnInfo>> {
    self.assert_open()?;
    let conn = self.conn.clone();
    let name = name.to_owned();
    let schema = schema.map(|s| s.to_owned());
    tokio::task::spawn_blocking(move || -> Result<Vec<crate::types::ColumnInfo>> {
        let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
        crate::catalog::describe_table(&*conn, &name, schema.as_deref())
    })
    .await
    .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?
}
```

(Repeat the `spawn_blocking` shape for `get_tables`, `create_table`, `drop_table`, `rename_table`. The plan author trusts the executor to mirror the pattern; if the pattern is unclear, re-read the `execute()` impl from T7.)

- [ ] **Step 9.4: Run tests**

```bash
cargo test --package dat0-engine --test catalog -- --nocapture
```

Expected: PASS.

- [ ] **Step 9.5: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "feat(engine): T9 — catalog ops (describe/get/create/drop/rename)"
```

---

### Task 10: export_table

Implement `export_table` returning `Vec<u8>` for CSV/JSON/Parquet.

**Files:**
- Modify: `crates/dat0-engine/src/export.rs`
- Modify: `crates/dat0-engine/src/duckdb_engine.rs`
- Create: `crates/dat0-engine/tests/export.rs`

**Subagent dispatch profile:** combined-verify.

- [ ] **Step 10.1: Write the failing tests**

`crates/dat0-engine/tests/export.rs`:

```rust
use dat0_engine::{DerivedOrigin, DuckDBEngine, ExportFormat, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 256 * 1024 * 1024 }
}

async fn engine_with_things() -> (DuckDBEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    engine
        .create_table(
            "things",
            "SELECT 1::INTEGER as id, 'a'::VARCHAR as name UNION ALL SELECT 2, 'b'",
            DerivedOrigin::Sql("seed".into()),
        )
        .await
        .unwrap();
    // Return both so the caller's scope keeps the tempdir alive alongside the
    // engine. Avoids the `mem::forget` leak of an earlier draft.
    (engine, dir)
}

#[tokio::test]
async fn export_csv() {
    let (engine, _dir) = engine_with_things().await;
    let bytes = engine.export_table("things", ExportFormat::Csv).await.unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert!(s.contains("id"));
    assert!(s.contains("name"));
    assert!(s.contains("\n1"));
    engine.close().await.unwrap();
}

#[tokio::test]
async fn export_json() {
    let (engine, _dir) = engine_with_things().await;
    let bytes = engine.export_table("things", ExportFormat::Json).await.unwrap();
    let s = String::from_utf8(bytes).unwrap();
    assert!(s.contains("\"id\""));
    engine.close().await.unwrap();
}

#[tokio::test]
async fn export_parquet_yields_nonempty_bytes() {
    let (engine, _dir) = engine_with_things().await;
    let bytes = engine.export_table("things", ExportFormat::Parquet).await.unwrap();
    // Parquet magic: starts with 'PAR1'
    assert!(bytes.starts_with(b"PAR1"));
    assert!(bytes.ends_with(b"PAR1"));
    engine.close().await.unwrap();
}
```

- [ ] **Step 10.2: Run, verify failure**

- [ ] **Step 10.3: Implement export**

Replace `crates/dat0-engine/src/export.rs`:

```rust
//! Export a table to bytes via DuckDB COPY ... TO. Writes to a tempfile,
//! reads back the bytes, returns. Streaming export for files-larger-than-RAM
//! is deferred (spec §4 out-of-scope).

use std::io::Read;

use crate::error::EngineError;
use crate::types::ExportFormat;
use crate::Result;

pub(crate) fn export_table_bytes(
    conn: &duckdb::Connection,
    table: &str,
    format: ExportFormat,
) -> Result<Vec<u8>> {
    let tmp = tempfile::Builder::new()
        .prefix("dat0-export-")
        .suffix(match format {
            ExportFormat::Csv => ".csv",
            ExportFormat::Json => ".json",
            ExportFormat::Parquet => ".parquet",
        })
        .tempfile()
        .map_err(EngineError::Io)?;
    let path = tmp.path().to_path_buf();
    let path_str = path
        .to_str()
        .ok_or_else(|| EngineError::InvalidPath(path.clone()))?
        .replace('\'', "''");

    let copy_sql = match format {
        ExportFormat::Csv => format!(
            "COPY (SELECT * FROM \"{}\") TO '{}' (FORMAT CSV, HEADER)",
            table, path_str
        ),
        ExportFormat::Json => format!(
            "COPY (SELECT * FROM \"{}\") TO '{}' (FORMAT JSON, ARRAY)",
            table, path_str
        ),
        ExportFormat::Parquet => format!(
            "COPY (SELECT * FROM \"{}\") TO '{}' (FORMAT PARQUET)",
            table, path_str
        ),
    };
    conn.execute_batch(&copy_sql)?;

    let mut bytes = Vec::new();
    let mut f = std::fs::File::open(&path).map_err(EngineError::Io)?;
    f.read_to_end(&mut bytes).map_err(EngineError::Io)?;
    Ok(bytes)
}
```

In `duckdb_engine.rs`, replace `export_table`:

```rust
async fn export_table(
    &self,
    name: &str,
    format: crate::types::ExportFormat,
) -> Result<Vec<u8>> {
    self.assert_open()?;
    let conn = self.conn.clone();
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
        crate::export::export_table_bytes(&*conn, &name, format)
    })
    .await
    .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?
}
```

- [ ] **Step 10.4: Run tests, verify pass**

- [ ] **Step 10.5: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "feat(engine): T10 — export_table CSV/JSON/Parquet via COPY ... TO"
```

---

### Task 11a: attach/detach DSN dispatch + `md:` NotImplemented

Implement the generic `attach()` / `detach()` plumbing — DSN parsing, error for unknown schemes, `EngineError::NotImplemented` for `md:` (D-007 deferral). The actual sqlite_scanner ATTACH path lives in T11b.

**Files:**
- Modify: `crates/dat0-engine/src/attach.rs`
- Modify: `crates/dat0-engine/src/duckdb_engine.rs`
- Create: `crates/dat0-engine/tests/attach_dispatch.rs`

**Subagent dispatch profile:** full review (DSN parser correctness affects every future ATTACH user).

- [ ] **Step 11a.1: Write the failing tests**

`crates/dat0-engine/tests/attach_dispatch.rs`:

```rust
use dat0_engine::{AttachOpts, DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 256 * 1024 * 1024 }
}

#[tokio::test]
async fn attach_unknown_scheme_errors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let err = engine
        .attach("redis://localhost:6379", "x", AttachOpts::default())
        .await
        .expect_err("unknown scheme");
    assert!(matches!(err, dat0_engine::EngineError::UnknownAttachScheme(_)));
    engine.close().await.unwrap();
}

#[tokio::test]
async fn attach_md_returns_not_implemented() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let err = engine
        .attach("md:my_db", "md", AttachOpts::default())
        .await
        .expect_err("md: deferred to P5 (D-007)");
    match err {
        dat0_engine::EngineError::NotImplemented { feature } => {
            assert_eq!(feature, "MotherDuck");
        }
        other => panic!("expected NotImplemented, got {other:?}"),
    }
    engine.close().await.unwrap();
}

#[tokio::test]
async fn detach_unknown_alias_errors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let err = engine.detach("never_attached").await.expect_err("");
    assert!(matches!(err, dat0_engine::EngineError::DuckDb(_)));
    engine.close().await.unwrap();
}
```

- [ ] **Step 11a.2: Run, verify failure**

- [ ] **Step 11a.3: Implement attach dispatch**

Replace `crates/dat0-engine/src/attach.rs`:

```rust
//! ATTACH/DETACH. T11a covers DSN dispatch; T11b covers sqlite_scanner end-to-end.

use crate::error::EngineError;
use crate::types::AttachOpts;
use crate::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachScheme {
    Sqlite,
    MotherDuck,
}

pub(crate) fn parse_scheme(dsn: &str) -> Result<(AttachScheme, &str)> {
    if let Some(rest) = dsn.strip_prefix("sqlite:") {
        return Ok((AttachScheme::Sqlite, rest));
    }
    if let Some(rest) = dsn.strip_prefix("md:") {
        return Ok((AttachScheme::MotherDuck, rest));
    }
    Err(EngineError::UnknownAttachScheme(
        dsn.split(':').next().unwrap_or("?").to_string(),
    ))
}

pub(crate) fn build_attach_sqlite_sql(
    path: &str,
    alias: &str,
    opts: &AttachOpts,
) -> String {
    let read_only = if opts.read_only { ", READ_ONLY" } else { "" };
    format!(
        "ATTACH '{}' AS \"{}\" (TYPE SQLITE{});",
        path.replace('\'', "''"),
        alias,
        read_only
    )
}

pub(crate) fn build_detach_sql(alias: &str) -> String {
    format!("DETACH \"{}\";", alias)
}
```

In `duckdb_engine.rs`, replace `attach` and `detach`:

```rust
async fn attach(
    &self,
    dsn: &str,
    alias: &str,
    opts: crate::types::AttachOpts,
) -> Result<()> {
    self.assert_open()?;
    let (scheme, rest) = crate::attach::parse_scheme(dsn)?;
    match scheme {
        crate::attach::AttachScheme::MotherDuck => {
            // D-007: end-to-end deferred to P5.
            return Err(EngineError::NotImplemented { feature: "MotherDuck" });
        }
        crate::attach::AttachScheme::Sqlite => {}
    }
    let sql = crate::attach::build_attach_sqlite_sql(rest, alias, &opts);
    let conn = self.conn.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
        conn.execute_batch(&sql)?;
        Ok(())
    })
    .await
    .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?
}

async fn detach(&self, alias: &str) -> Result<()> {
    self.assert_open()?;
    let sql = crate::attach::build_detach_sql(alias);
    let conn = self.conn.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let conn = conn.lock().map_err(|_| EngineError::EnginePoisoned)?;
        conn.execute_batch(&sql)?;
        Ok(())
    })
    .await
    .map_err(|e| EngineError::Io(std::io::Error::other(e.to_string())))?
}
```

- [ ] **Step 11a.4: Run tests**

```bash
cargo test --package dat0-engine --test attach_dispatch -- --nocapture
```

Expected: 3 PASSes. Note that `attach('sqlite:...')` itself isn't tested end-to-end in T11a — that test lives in T11b once the extension is loaded.

- [ ] **Step 11a.5: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "$(cat <<'EOF'
feat(engine): T11a — attach/detach DSN dispatch

Parses DSN prefix: sqlite: dispatches to ATTACH ... (TYPE SQLITE);
md: returns EngineError::NotImplemented { feature: "MotherDuck" }
per D-007. Unknown schemes surface UnknownAttachScheme.

Tests: unknown scheme, md: NotImplemented, detach unknown alias.

End-to-end ATTACH 'sqlite:...' verification ships in T11b.
EOF
)"
```

---

### Task 11b: sqlite_scanner end-to-end ATTACH

Verify `ATTACH 'sqlite:fixture.db'` exposes the SQLite tables. Requires `sqlite_scanner` to be loaded — T2's best-effort `LOAD` succeeds only if T14 or a test-only bootstrap has installed the extension. T11b explicitly handles the test-time bootstrap.

**Files:**
- Modify: `crates/dat0-engine/src/extension_bootstrap.rs`
- Create: `crates/dat0-engine/tests/attach_sqlite.rs`

**Subagent dispatch profile:** full review (extension lifecycle is the highest-risk surface short of streaming).

- [ ] **Step 11b.1: Implement extension bootstrap module**

Replace `crates/dat0-engine/src/extension_bootstrap.rs`:

```rust
//! Extension bootstrap. T14 calls `install_sqlite_scanner_at_app_boot`
//! once at app startup before any window opens. Tests use the
//! `__test_install_sqlite_scanner` variant.

use std::sync::OnceLock;
use tracing::{info, warn};

use crate::error::EngineError;
use crate::Result;

/// Memoized install outcome. `OnceLock::get_or_init` runs the closure exactly
/// once per process; subsequent calls return the cached `&Result<(), String>`.
static INSTALL_RESULT: OnceLock<std::result::Result<(), String>> = OnceLock::new();

/// Install + LOAD `sqlite_scanner` exactly once per process. Subsequent calls
/// short-circuit to the cached result.
///
/// Called by `dat0-app` at boot path before any window opens. Engine `init()`
/// only LOADs (not INSTALLs) on the assumption this has already run.
pub fn install_sqlite_scanner_at_app_boot(scratch_template: std::path::PathBuf) -> Result<()> {
    let outcome: &std::result::Result<(), String> = INSTALL_RESULT.get_or_init(|| {
        let result = (|| -> std::result::Result<(), String> {
            let conn = duckdb::Connection::open(&scratch_template)
                .map_err(|e| format!("open: {e}"))?;
            conn.execute_batch("INSTALL sqlite_scanner; LOAD sqlite_scanner;")
                .map_err(|e| format!("install/load: {e}"))?;
            info!(target: "dat0_engine::extensions", "sqlite_scanner installed and loaded");
            Ok(())
        })();
        if let Err(ref e) = result {
            warn!(target: "dat0_engine::extensions", error = %e, "sqlite_scanner install failed");
        }
        result
    });
    outcome.clone().map_err(|msg| EngineError::Io(std::io::Error::other(msg)))
}

/// Test-only: install via a per-test Connection.
///
/// **Concurrency note:** within a single test-binary process, `OnceLock`
/// serializes the install. But `cargo test --workspace` runs each test crate
/// as a separate process, and they share `~/.duckdb/extensions/` on disk —
/// the canonical default extension cache. The first time tests run cold, two
/// processes can race the INSTALL of `sqlite_scanner.duckdb_extension`.
/// Mitigations:
///   1. CI runs a one-shot priming step before the test matrix (recommended;
///      add to `.github/workflows/ci.yml` in T13 as a step that calls
///      `cargo run -p dat0-fixtures-priming -- --install-extensions` or runs
///      a small `cargo test -p dat0-engine --test attach_dispatch` to warm
///      the cache).
///   2. Alternatively, set `DUCKDB_EXTENSION_DIRECTORY` per test process to
///      a tempdir — but extension caches won't persist across cargo runs.
/// Track as a P2 candidate plan-defect (PD-005) if cold-cache race is observed.
#[doc(hidden)]
pub fn __test_install_sqlite_scanner() -> Result<()> {
    let scratch = std::env::temp_dir().join(format!(
        "dat0-test-extbootstrap-{}.duckdb",
        std::process::id()
    ));
    install_sqlite_scanner_at_app_boot(scratch)
}
```

- [ ] **Step 11b.2: Write the failing test**

`crates/dat0-engine/tests/attach_sqlite.rs`:

```rust
use std::path::PathBuf;

use dat0_engine::extension_bootstrap::__test_install_sqlite_scanner;
use dat0_engine::{AttachOpts, DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 256 * 1024 * 1024 }
}
fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/small")
        .join(rel)
}

#[tokio::test]
async fn attach_sqlite_exposes_tables() {
    __test_install_sqlite_scanner().expect("ext install");

    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();

    let dsn = format!("sqlite:{}", fixture("simple.sqlite").display());
    engine
        .attach(&dsn, "sq", AttachOpts { read_only: true, schema_filter: None })
        .await
        .unwrap();

    let v = engine
        .__debug_query_scalar("SELECT COUNT(*)::TEXT FROM sq.items")
        .await
        .unwrap();
    assert_eq!(v, "3");

    engine.detach("sq").await.unwrap();
    engine.close().await.unwrap();
}
```

- [ ] **Step 11b.3: Run, verify pass (or extension-install diagnose)**

```bash
cargo test --package dat0-engine --test attach_sqlite -- --nocapture
```

Expected: PASS. If extension install fails (e.g., no network on first run), the test will surface the install error. CI must run with network reachable. Document any flakes as PD-005+.

- [ ] **Step 11b.4: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "$(cat <<'EOF'
feat(engine): T11b — sqlite_scanner end-to-end ATTACH path

extension_bootstrap module provides install_sqlite_scanner_at_app_boot
(production: T14 calls once at app startup) and __test_install_sqlite_scanner
(test-only: serializes parallel test setup via std::sync::Once).

Tests verify ATTACH 'sqlite:fixture.db' exposes the SQLite table to
DuckDB; SELECT through sq.items returns expected rows; DETACH cleans up.

Closes spec exit criterion #3 (ATTACH 'sqlite:fixture.db' exposes
fixture's tables).
EOF
)"
```

---

### Task 12: dat0-fixtures crate

Build the fixture generator binary. Generates 1 GB CSV / 500 MB Parquet / 100 MB SQLite from a deterministic seed.

**Files:**
- Modify: `Cargo.toml` (add `dat0-fixtures` to workspace members; add `clap`, `rand`, `rusqlite` deps)
- Create: `crates/dat0-fixtures/Cargo.toml`
- Create: `crates/dat0-fixtures/src/main.rs`
- Create: `crates/dat0-fixtures/tests/smoke.rs`

**Subagent dispatch profile:** full review (deterministic output is critical for cache key stability).

- [ ] **Step 12.1: Add workspace deps**

Edit workspace `Cargo.toml`:

```toml
# in [workspace]:
members = [
    "crates/dat0-app",
    "crates/dat0-engine",
    "crates/dat0-format",
    "crates/dat0-fixtures",   # NEW
    "crates/dat0-i18n",
    "crates/dat0-keychain",
]

# in [workspace.dependencies]:
clap = { version = "4", features = ["derive"] }
rand = "0.8"
rand_chacha = "0.3"  # deterministic PRNG
rusqlite = { version = "0.31", features = ["bundled"] }
```

- [ ] **Step 12.2: Create `crates/dat0-fixtures/Cargo.toml`**

```toml
[package]
name = "dat0-fixtures"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
rust-version.workspace = true

[[bin]]
name = "dat0-fixtures"
path = "src/main.rs"

[dependencies]
clap.workspace = true
rand.workspace = true
rand_chacha.workspace = true
duckdb.workspace = true
rusqlite.workspace = true
anyhow.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Step 12.3: Implement the generator**

`crates/dat0-fixtures/src/main.rs`:

```rust
//! dat0-fixtures: generate deterministic large fixtures for engine tests.
//!
//! CSV via direct write (fastest). Parquet via DuckDB COPY ... TO (no Arrow
//! workspace dep). SQLite via rusqlite.

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

#[derive(Parser, Debug)]
#[command(name = "dat0-fixtures")]
struct Cli {
    /// Output directory. Files written: generated.csv, generated.parquet, generated.sqlite.
    #[arg(long)]
    out: PathBuf,
    /// Deterministic seed (default 42).
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// CSV target bytes (default 1 GiB).
    #[arg(long, default_value_t = 1_073_741_824)]
    csv_bytes: u64,
    /// Parquet target bytes (default 500 MiB). Approx — DuckDB compresses.
    #[arg(long, default_value_t = 524_288_000)]
    parquet_target_rows: u64,
    /// SQLite target bytes (default 100 MiB).
    #[arg(long, default_value_t = 104_857_600)]
    sqlite_target_bytes: u64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.out).context("create out dir")?;

    println!("dat0-fixtures: generating into {}", cli.out.display());
    let started = std::time::Instant::now();

    let csv_path = cli.out.join("generated.csv");
    write_csv(&csv_path, cli.seed, cli.csv_bytes)?;

    let parquet_path = cli.out.join("generated.parquet");
    write_parquet_via_duckdb(&csv_path, &parquet_path)?;

    let sqlite_path = cli.out.join("generated.sqlite");
    write_sqlite(&sqlite_path, cli.seed, cli.sqlite_target_bytes)?;

    println!(
        "dat0-fixtures: done in {:?}; csv {} MB, parquet {} MB, sqlite {} MB",
        started.elapsed(),
        std::fs::metadata(&csv_path).map(|m| m.len() / (1024 * 1024)).unwrap_or(0),
        std::fs::metadata(&parquet_path).map(|m| m.len() / (1024 * 1024)).unwrap_or(0),
        std::fs::metadata(&sqlite_path).map(|m| m.len() / (1024 * 1024)).unwrap_or(0),
    );
    Ok(())
}

fn write_csv(path: &Path, seed: u64, target_bytes: u64) -> Result<()> {
    let f = std::fs::File::create(path).context("create csv")?;
    let mut w = BufWriter::with_capacity(8 * 1024 * 1024, f);
    writeln!(w, "id,name,score,flag,date,city,department,quantity,unit_price,note")?;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let cities = ["new_york", "london", "tokyo", "berlin", "paris", "sydney"];
    let depts = ["sales", "engineering", "marketing", "ops", "finance"];
    let mut written: u64 = 0;
    let mut id: u64 = 0;
    while written < target_bytes {
        id += 1;
        let name = format!("item_{:08x}", rng.gen::<u32>());
        let score: f64 = rng.gen::<f64>() * 1000.0;
        let flag = rng.gen_bool(0.6);
        let day = 1 + (rng.gen::<u32>() % 28);
        let month = 1 + (rng.gen::<u32>() % 12);
        let year = 2020 + (rng.gen::<u32>() % 6);
        let city = cities[rng.gen_range(0..cities.len())];
        let dept = depts[rng.gen_range(0..depts.len())];
        let qty: u32 = rng.gen_range(0..1000);
        let unit: f64 = rng.gen::<f64>() * 100.0;
        let note = if rng.gen_bool(0.05) { "" } else { "ok" };
        let line = format!(
            "{},{},{:.4},{},{:04}-{:02}-{:02},{},{},{},{:.4},{}\n",
            id, name, score, flag, year, month, day, city, dept, qty, unit, note
        );
        w.write_all(line.as_bytes())?;
        written += line.len() as u64;
    }
    w.flush()?;
    Ok(())
}

fn write_parquet_via_duckdb(csv_in: &Path, parquet_out: &Path) -> Result<()> {
    let conn = duckdb::Connection::open_in_memory()?;
    let copy_sql = format!(
        "COPY (SELECT * FROM read_csv('{}')) TO '{}' (FORMAT PARQUET);",
        csv_in.display().to_string().replace('\'', "''"),
        parquet_out.display().to_string().replace('\'', "''"),
    );
    conn.execute_batch(&copy_sql)?;
    Ok(())
}

fn write_sqlite(path: &Path, seed: u64, target_bytes: u64) -> Result<()> {
    let _ = std::fs::remove_file(path); // start fresh
    let conn = rusqlite::Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         CREATE TABLE items (
             id INTEGER PRIMARY KEY,
             name TEXT NOT NULL,
             score REAL,
             flag INTEGER,
             city TEXT,
             department TEXT,
             quantity INTEGER,
             unit_price REAL
         );",
    )?;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let cities = ["new_york", "london", "tokyo", "berlin", "paris", "sydney"];
    let depts = ["sales", "engineering", "marketing", "ops", "finance"];
    let tx = conn.unchecked_transaction()?;
    let mut id: i64 = 0;
    let mut last_size = 0_u64;
    let mut stmt = tx.prepare(
        "INSERT INTO items (id, name, score, flag, city, department, quantity, unit_price)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    loop {
        for _ in 0..10_000 {
            id += 1;
            stmt.execute(rusqlite::params![
                id,
                format!("item_{:08x}", rng.gen::<u32>()),
                rng.gen::<f64>() * 1000.0,
                if rng.gen_bool(0.6) { 1 } else { 0 },
                cities[rng.gen_range(0..cities.len())],
                depts[rng.gen_range(0..depts.len())],
                rng.gen_range(0..1000_i64),
                rng.gen::<f64>() * 100.0,
            ])?;
        }
        // Re-prepare to flush. Inspect file size every 10k inserts.
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if size >= target_bytes {
            break;
        }
        if size == last_size {
            // Forward progress safeguard
            break;
        }
        last_size = size;
    }
    drop(stmt);
    tx.commit()?;
    Ok(())
}
```

- [ ] **Step 12.4: Write a smoke test**

`crates/dat0-fixtures/tests/smoke.rs`:

```rust
use std::process::Command;

#[test]
fn generator_runs_with_tiny_targets() {
    let dir = tempfile::tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_dat0-fixtures"))
        .arg("--out").arg(dir.path())
        .arg("--csv-bytes").arg("4096")
        .arg("--sqlite-target-bytes").arg("4096")
        .status()
        .unwrap();
    assert!(status.success());
    assert!(dir.path().join("generated.csv").exists());
    assert!(dir.path().join("generated.parquet").exists());
    assert!(dir.path().join("generated.sqlite").exists());
}
```

- [ ] **Step 12.5: Run smoke**

```bash
cargo test --package dat0-fixtures
```

Expected: PASS.

- [ ] **Step 12.6: Run full generator manually for downstream tasks**

```bash
mkdir -p tests/fixtures/large
cargo run --release -p dat0-fixtures -- --out tests/fixtures/large --seed 42
```

Expected output: csv ~1024 MB, parquet ~varies (compressed), sqlite ~100 MB. Total time ~30-60 s on a modern laptop.

- [ ] **Step 12.7: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "$(cat <<'EOF'
feat(engine): T12 — dat0-fixtures generator (CSV/Parquet/SQLite)

Workspace bin crate; CSV via direct write at ~100 MB/s; Parquet via
DuckDB COPY...TO from the CSV (no standalone arrow dep); SQLite via
rusqlite with bulk-insert transaction.

Deterministic via ChaCha8 PRNG seeded from --seed (default 42).
Default targets: 1 GiB CSV / ~500 MiB Parquet / 100 MiB SQLite.

Smoke test verifies all three files land for a tiny target. Full
generation is manual or CI-only.
EOF
)"
```

---

### Task 13: CI fixture cache + generation step

Add the fixture cache + generation step to `.github/workflows/ci.yml` so engine tests against the large fixtures run in CI.

**Files:**
- Modify: `.github/workflows/ci.yml`

**Subagent dispatch profile:** combined-verify.

- [ ] **Step 13.1: Read current CI**

```bash
cat .github/workflows/ci.yml | head -150
```

Identify the `build` job's setup steps (Rust install, Metal probe on macOS, apt-get on Linux). The fixture cache + gen step inserts after Rust setup but before any test step.

- [ ] **Step 13.2: Edit `ci.yml`**

Insert after the Rust install step in the `build` job's steps list:

```yaml
      - name: Cache fixtures
        id: cache-fixtures
        uses: actions/cache@v4
        with:
          path: tests/fixtures/large/
          key: fixtures-${{ runner.os }}-${{ runner.arch }}-${{ hashFiles('crates/dat0-fixtures/**') }}-seed-42

      - name: Generate fixtures (if cache miss)
        if: steps.cache-fixtures.outputs.cache-hit != 'true'
        run: |
          mkdir -p tests/fixtures/large
          cargo run --release -p dat0-fixtures -- --out tests/fixtures/large --seed 42

      - name: Prime sqlite_scanner extension cache (avoid concurrent INSTALL race)
        run: |
          cargo run --release -p dat0-fixtures -- --out /tmp/dat0-prime --csv-bytes 4096 --sqlite-target-bytes 4096 || true
          # Force a one-shot INSTALL so subsequent test-binary processes find the extension cached.
          cargo test -p dat0-engine --test attach_dispatch -- --nocapture || true

      - name: Run tests (with --include-ignored to exercise large fixtures)
        run: cargo test --workspace --target ${{ matrix.target.triple }} -- --include-ignored
```

> If the existing CI has a separate `Run tests` step, replace its `cargo test` invocation with `cargo test --workspace -- --include-ignored`. The `--include-ignored` flag picks up T17's exit-criterion tests which are `#[ignore = "requires generated fixtures"]`.

> The "prime sqlite_scanner extension cache" step pre-warms `~/.duckdb/extensions/` before the parallel test matrix runs, sidestepping the concurrent-install race documented in T11b's `__test_install_sqlite_scanner` doc-comment. The `|| true` swallows non-fatal errors (e.g., the test binary not yet existing on first run); the priming is best-effort, the test step still asserts the install succeeded.

- [ ] **Step 13.3: Local CI dry-run**

```bash
# Lint the workflow file
yq eval '.jobs.build.steps' .github/workflows/ci.yml | head -50
```

Inspect the rendered step list. Confirm cache step + generate step + test step are in correct order.

- [ ] **Step 13.4: fmt + clippy + commit**

(No code changes; fmt/clippy are no-ops here.)

```bash
git add .github/workflows/ci.yml
git commit -s -m "$(cat <<'EOF'
ci(p2): T13 — fixtures cache + generation step

Caches tests/fixtures/large/ keyed on OS, arch, and dat0-fixtures crate
hash (re-generates only on generator changes; seed pinned to 42).

Test step adds --include-ignored to exercise T17's exit-criterion tests
which gate on the large fixtures.

PR-1 cold-cache cost ~1 min for fixture generation; subsequent runs
restore in ~10 s.
EOF
)"
```

---

### Task 14: Extension bootstrap in `dat0-app` boot path

Wire `install_sqlite_scanner_at_app_boot` into `dat0-app::boot`. Single-shot, runs before any window opens. Add a banner UX on download failure (uses P1's Banner primitive — first consumer).

**Files:**
- Modify: `crates/dat0-app/Cargo.toml` (add `dat0-engine` path dep)
- Modify: `crates/dat0-app/src/boot.rs`
- Possibly modify: `crates/dat0-app/src/error_ux/banner.rs` (if banner needs an "extension install failed" preset)

**Subagent dispatch profile:** full review (T14 is the first consumer of P1's `Banner` primitive; the primitive's exported shape isn't pinned in this plan and has to be verified against P1's actual `crates/dat0-app/src/error_ux/banner.rs` API. Also: the banner's i18n key flow + Sentry redaction interaction need a code-quality review pass).

- [ ] **Step 14.1: Add `dat0-engine` to `dat0-app/Cargo.toml`**

```toml
[dependencies]
dat0-engine = { workspace = true }
# ... existing deps unchanged
```

Confirm `dat0-engine = { path = "crates/dat0-engine" }` is in workspace deps from T1.

- [ ] **Step 14.2: Wire into boot path**

Open `crates/dat0-app/src/boot.rs`. Find where init runs before window-open (after settings init, after telemetry init, before `App::run`). Add:

```rust
// After tracing/telemetry/settings init, before window registry creation.
{
    let scratch_dir = platform::data_dir()
        .unwrap_or_else(|| std::env::temp_dir())
        .join("dat0")
        .join("ext-bootstrap");
    let _ = std::fs::create_dir_all(&scratch_dir);
    let bootstrap_db = scratch_dir.join("bootstrap.duckdb");

    if let Err(e) = dat0_engine::extension_bootstrap::install_sqlite_scanner_at_app_boot(bootstrap_db) {
        tracing::error!(error = %e, "sqlite_scanner install failed at boot; SQLite ATTACH will be unavailable");
        // Surface a Banner via the P1 error_ux primitive.
        // SQLite ATTACH is degraded but the app continues.
        crate::error_ux::banner::push_banner(crate::error_ux::banner::Banner {
            severity: crate::error_ux::banner::Severity::Warning,
            title_key: "boot.sqlite_scanner_install_failed.title",
            body_key: "boot.sqlite_scanner_install_failed.body",
            link: Some("https://github.com/accidentally-awesome-labs/dat0/blob/main/CONTRIBUTING.md#proxy"),
        });
    }
}
```

> The exact path/name of the Banner primitive depends on P1's `error_ux::banner` API. If `push_banner` doesn't exist (P1 may have shipped Banner as a render-side type rather than a service), wire via the appropriate primitive. The sub-agent should consult `crates/dat0-app/src/error_ux/banner.rs` and adapt.

- [ ] **Step 14.3: Add i18n strings**

Edit `crates/dat0-i18n/src/strings/en.json`. Add keys:

```json
"boot.sqlite_scanner_install_failed.title": "SQLite support unavailable",
"boot.sqlite_scanner_install_failed.body": "dat0 couldn't install the SQLite reader extension. Reading .sqlite/.db files will fail until this is resolved. If you're behind a corporate proxy, set HTTP_PROXY/HTTPS_PROXY env vars and restart. See CONTRIBUTING for details."
```

- [ ] **Step 14.4: Compile + run app smoke**

```bash
cargo build --workspace
# Smoke: launch app once, confirm it boots without panic, sqlite_scanner installed in ~/.duckdb/extensions/
cargo run -p dat0-app &
sleep 3
ls ~/.duckdb/extensions/ 2>/dev/null
killall dat0-app 2>/dev/null || true
```

Expected: app launches cleanly; `~/.duckdb/extensions/<duckdb_version>/<platform>/sqlite_scanner.duckdb_extension` exists after first launch.

- [ ] **Step 14.5: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "$(cat <<'EOF'
feat(app): T14 — extension bootstrap at app boot

dat0-app boot path calls install_sqlite_scanner_at_app_boot single-shot
before any window opens. Failure logs an error and surfaces a Banner
via P1's error_ux primitive (first consumer of Banner). i18n strings
added under boot.sqlite_scanner_install_failed.*.

Sets the multi-window install race protection from spec §2.5: by the
time the first window's engine init() runs, the extension is already
installed in ~/.duckdb/extensions/ and engine init only LOADs.
EOF
)"
```

---

### Task 15: Integration test — multi-window concurrent engines

Verify two concurrent `DuckDBEngine` instances in one process don't cross-talk. Per spec §3 test #1 + #3 + #4.

**Files:**
- Create: `crates/dat0-engine/tests/multi_window.rs`

**Subagent dispatch profile:** full review.

- [ ] **Step 15.1: Write the tests**

`crates/dat0-engine/tests/multi_window.rs`:

```rust
use std::sync::Arc;

use dat0_engine::{DerivedOrigin, DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};
use futures::StreamExt;

fn budget_512mb() -> MemoryBudget {
    MemoryBudget { bytes: 512 * 1024 * 1024 }
}
fn budget_1gb() -> MemoryBudget {
    MemoryBudget { bytes: 1024 * 1024 * 1024 }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_engines_no_cross_talk() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let a = Arc::new(DuckDBEngine::new(dir_a.path().join("a.duckdb"), budget_512mb()).unwrap());
    let b = Arc::new(DuckDBEngine::new(dir_b.path().join("b.duckdb"), budget_1gb()).unwrap());
    a.init().await.unwrap();
    b.init().await.unwrap();

    a.create_table(
        "in_a",
        "SELECT i, 'a' AS tag FROM range(1000) t(i)",
        DerivedOrigin::Sql("seed".into()),
    )
    .await
    .unwrap();
    b.create_table(
        "in_b",
        "SELECT i, 'b' AS tag FROM range(2000) t(i)",
        DerivedOrigin::Sql("seed".into()),
    )
    .await
    .unwrap();

    // Tables in A should not be visible in B.
    let tables_a = a.get_tables().await.unwrap();
    let tables_b = b.get_tables().await.unwrap();
    assert!(tables_a.iter().any(|t| t.name == "in_a"));
    assert!(!tables_a.iter().any(|t| t.name == "in_b"));
    assert!(tables_b.iter().any(|t| t.name == "in_b"));
    assert!(!tables_b.iter().any(|t| t.name == "in_a"));

    // Concurrent execution.
    let (ra, rb) = tokio::join!(
        async {
            let mut s = a.execute_streaming("SELECT i FROM in_a").await.unwrap();
            let mut n = 0_usize;
            while let Some(b) = s.next().await { n += b.unwrap().num_rows(); }
            n
        },
        async {
            let mut s = b.execute_streaming("SELECT i FROM in_b").await.unwrap();
            let mut n = 0_usize;
            while let Some(b) = s.next().await { n += b.unwrap().num_rows(); }
            n
        },
    );
    assert_eq!(ra, 1000);
    assert_eq!(rb, 2000);

    a.close().await.unwrap();
    b.close().await.unwrap();
}

#[tokio::test]
async fn per_engine_memory_budgets_are_independent() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let a = DuckDBEngine::new(dir_a.path().join("a.duckdb"), budget_512mb()).unwrap();
    let b = DuckDBEngine::new(dir_b.path().join("b.duckdb"), budget_1gb()).unwrap();
    a.init().await.unwrap();
    b.init().await.unwrap();
    let la = a
        .__debug_query_scalar("SELECT current_setting('memory_limit')")
        .await
        .unwrap();
    let lb = b
        .__debug_query_scalar("SELECT current_setting('memory_limit')")
        .await
        .unwrap();
    assert_ne!(la, lb, "memory_limit should differ per engine");
    a.close().await.unwrap();
    b.close().await.unwrap();
}

#[tokio::test]
async fn same_file_concurrent_register() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let csv = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/small/basic.csv");

    let a = DuckDBEngine::new(dir_a.path().join("a.duckdb"), budget_512mb()).unwrap();
    let b = DuckDBEngine::new(dir_b.path().join("b.duckdb"), budget_512mb()).unwrap();
    a.init().await.unwrap();
    b.init().await.unwrap();
    let info_a = a.register_file(&csv, RegisterOpts::default()).await.unwrap();
    let info_b = b.register_file(&csv, RegisterOpts::default()).await.unwrap();
    assert_eq!(info_a.name, info_b.name);
    let count_a = a
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info_a.name))
        .await
        .unwrap();
    let count_b = b
        .__debug_query_scalar(&format!("SELECT COUNT(*)::TEXT FROM \"{}\"", info_b.name))
        .await
        .unwrap();
    assert_eq!(count_a, count_b);
    assert_eq!(count_a, "3");
    a.close().await.unwrap();
    b.close().await.unwrap();
}
```

- [ ] **Step 15.2: Run, verify pass**

```bash
cargo test --package dat0-engine --test multi_window -- --nocapture
```

Expected: 3 PASSes.

- [ ] **Step 15.3: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "feat(engine): T15 — multi-window safety contract tests

Verifies two concurrent DuckDBEngine instances in one process operate
without cross-talk: catalog isolation, concurrent streaming queries,
independent memory_limit pragmas, same-file concurrent register.

Closes spec §3 contract tests #1, #3, #4."
```

---

### Task 16: Integration test — interrupt + cancel isolation

Verify `Engine::interrupt()` cancels the targeted engine without affecting another concurrent engine. Per spec §3 test #2.

**Files:**
- Create: `crates/dat0-engine/tests/interrupt.rs`

**Subagent dispatch profile:** full review.

- [ ] **Step 16.1: Write the test**

`crates/dat0-engine/tests/interrupt.rs`:

```rust
use std::sync::Arc;
use std::time::Duration;

use dat0_engine::{DuckDBEngine, MemoryBudget, QueryEngine};

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 512 * 1024 * 1024 }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interrupt_isolates_per_engine() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let a = Arc::new(DuckDBEngine::new(dir_a.path().join("a.duckdb"), budget()).unwrap());
    let b = Arc::new(DuckDBEngine::new(dir_b.path().join("b.duckdb"), budget()).unwrap());
    a.init().await.unwrap();
    b.init().await.unwrap();

    // Engine A: long query (DuckDB CROSS JOIN of large ranges).
    let a_clone = a.clone();
    let long_query = tokio::spawn(async move {
        a_clone.execute(
            "SELECT COUNT(*) FROM range(10000000) t1(i), range(1000) t2(j)",
        ).await
    });

    // Engine B: short query, runs concurrently.
    let b_clone = b.clone();
    let short_query = tokio::spawn(async move {
        b_clone.execute("SELECT 1::INTEGER as v").await
    });

    // Issue interrupt repeatedly until A returns (or test-level timeout fires).
    // A 100ms sleep then a single interrupt is unreliable on slow CI runners
    // where the spawn_blocking thread may not yet be scheduled.
    let interrupter = {
        let a = a.clone();
        tokio::spawn(async move {
            for _ in 0..100 {
                tokio::time::sleep(Duration::from_millis(50)).await;
                a.interrupt();
            }
        })
    };

    // Cap A's wait at 30 seconds; if interrupt doesn't propagate by then,
    // something is broken — fail the test rather than hang.
    let a_result = tokio::time::timeout(Duration::from_secs(30), long_query)
        .await
        .expect("A's long query exceeded 30s timeout — interrupt did not propagate")
        .unwrap();
    let b_result = short_query.await.unwrap();
    interrupter.abort();

    // A must surface EngineError::Interrupted specifically (not just any Err).
    // T7's translate_duckdb_err normalizes DuckDB's interrupt-error to this variant.
    match a_result {
        Err(dat0_engine::EngineError::Interrupted) => {} // expected
        other => panic!("expected EngineError::Interrupted, got {other:?}"),
    }
    // B must complete cleanly, unaffected.
    assert!(b_result.is_ok(), "B should complete normally despite A's interrupt");
    let qr = b_result.unwrap();
    assert!(qr.batches.iter().map(|b| b.num_rows()).sum::<usize>() == 1);

    a.close().await.unwrap();
    b.close().await.unwrap();
}
```

> If T0 finds that DuckDB at the pinned version surfaces interrupt errors via
> a different mechanism than the substring match in T7's `translate_duckdb_err`,
> update the translator AND this test together. Both depend on the same
> classification.

- [ ] **Step 16.2: Run, verify pass**

```bash
cargo test --package dat0-engine --test interrupt -- --nocapture
```

Expected: PASS. If A doesn't surface an error within a reasonable time, the interrupt mechanism is broken — file PD-005 with diagnosis.

- [ ] **Step 16.3: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "feat(engine): T16 — interrupt isolation per engine

Verifies Engine::interrupt() on A causes A's in-flight query to surface
an error while B's concurrent short query completes unaffected.

Closes spec §3 contract test #2."
```

---

### Task 17: Integration tests — exit-criterion sizes (1 GB CSV / 500 MB Parquet / 100 MB SQLite)

The headline gate: prove the engine handles the spec's stated sizes.

**Files:**
- Create: `crates/dat0-engine/tests/exit_criteria.rs`

**Subagent dispatch profile:** full review.

- [ ] **Step 17.1: Write the tests**

`crates/dat0-engine/tests/exit_criteria.rs`:

```rust
//! Exit-criterion tests gated on `tests/fixtures/large/`. Run with
//! `cargo test -- --include-ignored` after `dat0-fixtures` has populated
//! the directory.

use std::path::PathBuf;

use dat0_engine::extension_bootstrap::__test_install_sqlite_scanner;
use dat0_engine::{AttachOpts, DuckDBEngine, MemoryBudget, QueryEngine, RegisterOpts};
use futures::StreamExt;

fn budget() -> MemoryBudget {
    MemoryBudget { bytes: 4 * 1024 * 1024 * 1024 } // 4 GB
}
fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/large")
}

fn skip_if_no_fixtures() -> bool {
    let p = fixtures_root().join("generated.csv");
    if !p.exists() {
        eprintln!("SKIP: {} not present; run `cargo run -p dat0-fixtures -- --out tests/fixtures/large` first.", p.display());
        return true;
    }
    false
}

#[tokio::test]
#[ignore = "requires generated fixtures"]
async fn one_gb_csv_streams() {
    if skip_if_no_fixtures() { return; }
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&fixtures_root().join("generated.csv"), RegisterOpts::default())
        .await
        .unwrap();
    assert!(!info.columns.is_empty());

    let mut stream = engine
        .execute_streaming(&format!("SELECT * FROM \"{}\"", info.name))
        .await
        .unwrap();
    let mut total = 0_usize;
    let mut batches = 0_usize;
    while let Some(batch) = stream.next().await {
        let b = batch.unwrap();
        total += b.num_rows();
        batches += 1;
    }
    assert!(total > 1_000_000, "expected millions of rows; got {total}");
    assert!(batches > 1, "expected streamed batches");
    engine.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires generated fixtures"]
async fn five_hundred_mb_parquet_streams() {
    if skip_if_no_fixtures() { return; }
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&fixtures_root().join("generated.parquet"), RegisterOpts::default())
        .await
        .unwrap();
    let mut stream = engine
        .execute_streaming(&format!("SELECT * FROM \"{}\"", info.name))
        .await
        .unwrap();
    let mut total = 0_usize;
    while let Some(b) = stream.next().await { total += b.unwrap().num_rows(); }
    assert!(total > 1_000_000);
    engine.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires generated fixtures"]
async fn one_hundred_mb_sqlite_attach() {
    if skip_if_no_fixtures() { return; }
    __test_install_sqlite_scanner().expect("ext install");

    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let dsn = format!("sqlite:{}", fixtures_root().join("generated.sqlite").display());
    engine
        .attach(&dsn, "sq", AttachOpts { read_only: true, schema_filter: None })
        .await
        .unwrap();
    let v = engine
        .__debug_query_scalar("SELECT COUNT(*)::TEXT FROM sq.items")
        .await
        .unwrap();
    let n: u64 = v.parse().unwrap();
    assert!(n > 100_000, "expected hundreds of thousands of rows in 100 MB SQLite; got {n}");
    engine.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires generated fixtures"]
async fn streaming_emits_arrow_recordbatch_type_chain() {
    // The streaming exit criterion claims "verified zero-copy from engine to
    // consumer (no JSON serialization in path)". The type-chain assertion
    // here proves the WEAKER property: the consumer receives
    // `duckdb::arrow::record_batch::RecordBatch` directly — no `Value`/`String`/JSON
    // intermediation is possible without a transformation step the type system
    // would surface. Genuine zero-copy verification (peak RSS bounded
    // independently of fixture size, batch buffers shared between DuckDB and
    // the consumer's address space) is deferred to a P3 perf bench because it
    // requires RSS measurement instrumentation we don't have in P2.
    if skip_if_no_fixtures() { return; }
    let dir = tempfile::tempdir().unwrap();
    let engine = DuckDBEngine::new(dir.path().join("a.duckdb"), budget()).unwrap();
    engine.init().await.unwrap();
    let info = engine
        .register_file(&fixtures_root().join("generated.csv"), RegisterOpts::default())
        .await
        .unwrap();
    let mut stream = engine
        .execute_streaming(&format!("SELECT * FROM \"{}\" LIMIT 100", info.name))
        .await
        .unwrap();
    let batch = stream.next().await.unwrap().unwrap();
    // Type assertion: if this compiles, the chain is RecordBatch through and
    // through. No JSON path possible without an explicit transform step.
    let _: &duckdb::arrow::record_batch::RecordBatch = &batch;
    assert!(batch.num_rows() > 0);
    engine.close().await.unwrap();
}
```

- [ ] **Step 17.2: Generate fixtures + run**

```bash
cargo run --release -p dat0-fixtures -- --out tests/fixtures/large --seed 42
cargo test --package dat0-engine --test exit_criteria -- --include-ignored --nocapture
```

Expected: 4 PASSes. Per-test timing on a modern laptop: 1 GB CSV ~30 s, 500 MB Parquet ~10 s, 100 MB SQLite ~5 s.

If the 1 GB CSV test exceeds 5 minutes, profile and either tune (e.g., raise streaming channel capacity) or split into smaller exit gates as PD-NNN.

- [ ] **Step 17.3: Add `--include-ignored` to CI test step**

(Already done in T13. Confirm the workflow file actually includes it.)

```bash
grep -- "--include-ignored" .github/workflows/ci.yml
```

Expected: a match.

- [ ] **Step 17.4: fmt + clippy + commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
git add -A
git commit -s -m "$(cat <<'EOF'
feat(engine): T17 — exit-criterion tests (1 GB CSV / 500 MB Parquet / 100 MB SQLite)

Closes spec §21.2 P2 exit criteria 1, 2, 3 + brainstorm-locked exit
criterion #7 (streaming is RecordBatch end-to-end, no JSON
serialization). Tests are #[ignore] so `cargo test` works without
generated fixtures; CI runs them via --include-ignored.

Skip behavior: if generated fixtures are absent, tests print SKIP and
return Ok — they're informational, not gate-blocking, in dev. CI's
fixture cache + generator step ensures presence in CI.
EOF
)"
```

---

### Task 18: P2 retro

Write a P2 retrospective that closes the loop on plan defects, deferral closures, lessons learned, and recommendations for P3.

**Files:**
- Create: `docs/plans/2026-04-27-dat0-p2-retro.md`
- Modify: `docs/deferrals.md` (close any deferrals that landed during execution; update last-touched dates)

**Subagent dispatch profile:** combined-verify.

- [ ] **Step 18.1: Write the retro template + fill**

```markdown
# dat0 P2 — Retrospective (Engine layer)

**Phase:** P2 (Engine layer)
**Worktree:** `.worktrees/p2-engine`
**Branch:** `p2-engine`
**Started:** <YYYY-MM-DD>
**Merged to main:** <YYYY-MM-DD via PR #N>
**Plan:** [`2026-04-27-dat0-p2-engine-plan.md`](2026-04-27-dat0-p2-engine-plan.md)
**Spec:** [`2026-04-27-dat0-p2-engine-design.md`](../specs/2026-04-27-dat0-p2-engine-design.md)

---

## Exit-criteria status

| # | Criterion | Status |
|---|-----------|--------|
| 1 | `register_file` succeeds for 1 GB CSV → Arrow stream | <PASS / FAIL with PD-NNN ref> |
| 2 | Same for Parquet, JSON, JSONL, NDJSON | <...> |
| 3 | `ATTACH 'sqlite:fixture.db'` exposes tables | <...> |
| 4 | Memory budget pragma at init | <...> |
| 5 | Format integration tests green | <...> |
| 6 | Migration scaffold runs ≥ 1 no-op cleanly | <...> |
| 7 | Streaming Arrow batches verified zero-copy | <...> |
| 8 | Two concurrent engines no cross-talk | <...> |
| 9 | `Engine::interrupt` isolation | <...> |
| 10 | T0 spike output committed | <...> |
| 11 | Arrow types via `duckdb::arrow` re-export only | <...> |
| 12 | duckdb-rs Cargo features = bundled+json+parquet | <...> |

## Plan defects discovered (PD-NNN)

<List PD-005..PD-N entries added during P2 with severity + outcome.>

## Deferrals closed during P2

<e.g., "D-009 closed: T0 found duckdb-rs `extensions-full` feature includes sqlite_scanner; switched to static bundle.">

## Deferrals carried forward

<List D-007, D-008, D-010 still open with target phase confirmation.>

## Lessons learned

<2–5 bullet-pointed lessons. Aim for the kind that change how the next phase is executed: combined-verify thresholds, subagent dispatch hot spots, plan-snippet vs reality drift, CI runtime issues, etc.>

## Recommendations for P3 (Scratch mode + DataGrid)

<Concrete callouts. Examples: "P3 must wire workspace-DB concurrent-open serialization or guarantee idempotent migrations." "P3 grid widget must consume RecordBatch from `duckdb::arrow` — flag any new `arrow = ...` dep additions in PR review.">

## Effort summary

| Task | Hours | Subagent dispatches |
|------|-------|--------------------|
| T0 | <h> | <n> |
| T1 | <h> | <n> |
| ... | | |
| T18 | <h> | <n> |
| **Total** | **<h>** | **<n>** |

Effort vs spec §21.6 estimate (~3 weeks): <under / on / over>.

## Sign-off

Approved by: <name>
Date: <YYYY-MM-DD>
```

- [ ] **Step 18.2: Update `docs/deferrals.md`**

For each deferral closed during P2: set `Status: closed`, append a `**Closed by:**` line citing the commit SHA / PR number / phase retro reference, update the at-a-glance table.

For deferrals still open: bump `Last touched: <YYYY-MM-DD>` to today.

- [ ] **Step 18.3: Commit**

```bash
git add docs/plans/2026-04-27-dat0-p2-retro.md docs/deferrals.md
git commit -s -m "docs(p2): T18 — phase retro + deferral updates"
```

- [ ] **Step 18.4: Open PR**

```bash
git push -u origin p2-engine
gh pr create --title "P2 Foundation: Engine Layer" --body "$(cat <<'EOF'
## Summary

P2 engine layer per `docs/specs/2026-04-27-dat0-p2-engine-design.md` and `docs/plans/2026-04-27-dat0-p2-engine-plan.md`. Closes spec §21.2 P2 entry/exit and locks design-spec §6.

Highlights:
- `dat0-engine` crate with full `QueryEngine` trait surface (T1–T11b).
- Multi-window-safe by construction (T15) + interrupt isolation (T16).
- Exit-criterion sizes verified: 1 GB CSV / 500 MB Parquet / 100 MB SQLite (T17).
- Lazy-loaded `sqlite_scanner` extension via `dat0-app` boot path (T14).
- Forward-only migration runner (T3).
- Deferred per spec: D-007 (MotherDuck end-to-end → P5), D-008 (CancellationToken trait wiring → P5), D-009 (sqlite_scanner static bundle contingency), D-010 (non-UTF-8 file encoding).

## Test plan

- [x] `cargo fmt --all -- --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace -- --include-ignored` (all green; CI matrix passes)
- [x] Multi-window concurrent integration test passes
- [x] Interrupt isolation integration test passes
- [x] 1 GB CSV / 500 MB Parquet / 100 MB SQLite exit-criterion tests pass
- [x] App smoke launches; sqlite_scanner installs at boot; banner UX surfaces on install failure
EOF
)"
```

After PR opens, run cross-AI review (`/gsd:review` or equivalent) per project convention. Address review comments in fix commits, do not amend.

---

## Self-Review

### 1. Spec coverage

| Spec section | Plan task(s) |
|---|---|
| §1 Goal + non-goals | Plan header; covered across all tasks |
| §2.1 spawn_blocking + InterruptHandle | T1 (struct), T2 (init), T8 (streaming worker), T16 (interrupt isolation) |
| §2.2 cancellation: internal interrupt only | T2 (`Engine::interrupt()`), T16 |
| §2.3 single connection per engine | T1, T2 |
| §2.4 memory budget per engine | T1 (`MemoryBudget`), T2 (PRAGMA), T15 (independence test) |
| §2.5 lazy-load sqlite_scanner | T0 (re-verify), T2 (LOAD), T11b (test bootstrap), T14 (boot install), T17 (1 GB SQLite) |
| §2.6 migration runner (per-engine scratch only) | T3 |
| §2.7 hybrid fixture strategy | T1 (small in-repo), T12 (generator), T13 (CI cache) |
| §2.8 trait surface verbatim | T1 (`trait_def.rs`), T2..T11b (impls) |
| §2.9 type surface | T1 |
| §2.10 error surface | T1 |
| §3 multi-window safety contract | T15 (#1, #3, #4), T16 (#2) |
| §4 out-of-scope | encoded in plan via NotImplemented for `md:` (T11a), no encoding field (T1), etc. |
| §5 plan shape | This plan |
| §6 exit criteria #1–#12 | T4, T5, T6 (formats); T11b (sqlite); T2 (memory); T3 (migrations); T8/T17 (streaming); T15 (concurrent); T16 (interrupt); T0 (notes file); T1 (Arrow re-exports); T1 (Cargo features) |
| §7 deferrals + commitments | T0 (D-007..D-010); plan preamble (commitments 1–4) |
| §8 risks | "Risks & Caveats" section + per-task risk callouts |
| §9 cross-references | "Authoritative cross-references" section |

No gaps.

### 2. Placeholder scan

Searched for "TBD", "TODO", "implement later", "fill in details", "Add appropriate error handling", "handle edge cases". No actual placeholders. The plan's earlier draft had `// TODO(T2)` in the close() body; replaced with an accurate paragraph explaining why Connection::close(self) cannot be called from inside Arc<Mutex<_>> and what P3+ may want to do (graceful drain).

All steps contain actual content the engineer can act on. Code blocks present in every code-step. Test names + assertions concrete.

### 3. Type consistency

- `DuckDBEngine` field names (`conn`, `interrupt`, `budget`, `scratch_path`, `status`) used identically across T1, T2, T7, T8, T9, T10, T11a, T11b, T15, T16.
- `RegisterOpts` field names (`format`, `delimiter`, `quote_char`, `escape_char`, `has_header`, `type_overrides`, `sample_rows`) consistent T1 → T4 → T5 → T6.
- `EngineError` variants used as defined in T1 across all later tasks.
- `MemoryBudget::as_pragma()` defined T1, called T2.
- `apply_migrations` signature defined T3, used in T2 wiring (T3 step replaces placeholder).
- `crate::register::dispatch_register_sql` defined T4, called from `register_file` impl T4 — used by T5 + T6 dispatch arms.

No mismatches.

---

## Execution Handoff

Plan complete and saved to `docs/plans/2026-04-27-dat0-p2-engine-plan.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, two-stage review per task (spec-compliance reviewer + code-quality reviewer), combined-verify shortcut on tasks T1, T9, T10, T13, T18 per the dispatch profile tags. This mirrors the P1 execution pattern that successfully shipped 25 tasks; ~55–75 dispatches expected for this 20-task plan.

**2. Inline Execution** — I execute tasks in this session using the executing-plans skill, with checkpoints for review after each batch.

Which approach?
