# dat0 P3a — Scratch + DataGrid hot path — Design Spec

**Date:** 2026-05-14
**Phase:** P3a (first half of P3 Scratch mode + DataGrid; ~2–2.5 weeks)
**Entry:** P2 Engine Layer merged to `main` (PR #2, merge `e55f2e1`); CI cost remediation merged (PR #3, merge `b3d99dd`).
**Authoritative source:** `docs/specs/2026-04-26-dat0-design.md` §5, §8.2, §21.2 P3.
This document is the brainstorm-output spec consumed by the forthcoming P3a implementation plan.

---

## 1. Goal

Land the engine-to-pixel hot path for Scratch mode: drop a file → it registers
into the per-window DuckDB engine → it renders in a virtualized DataGrid that
sustains 60 fps over 1 million rows. Plus the cross-cutting infrastructure
that has to land before the grid is meaningful (single-instance
enforcement, multi-window registry, sandbox lifecycle with crash recovery)
and the two deferral cleanups whose closure is gated on P3 (D-011, D-012).

UX polish (import wizard, command palette, crash-recovery banner content,
empty-state hero, editable Settings widgets D-001, theme live-switch D-002,
structured Banner refactor) is **P3b** and out of scope here.

**Non-goals (P3a):**
- Import wizard / CSV ambiguity prompts → P3b.
- Command palette → P3b.
- Editable Settings widgets (D-001) and theme live-switch (D-002) → P3b.
- Banner `{title, body, link, action}` structured-shape refactor (P2 retro #5) → P3b.
- Workspace mode + SQL Console (P4); P3a scaffolds the WorkspaceMutex API only.
- MotherDuck ATTACH end-to-end (D-007 → P5); cancellation-token trait (D-008 → P5).
- 1M-row scroll bench gated as merge requirement; P3a logs the metric, P10 enforces.

## 2. Phase decomposition (locked from brainstorm)

P3 was split into **P3a + P3b**. P3a = hot-path only. P3b = UX polish + deferral closures D-001/D-002 + Banner refactor.

Rationale: P3a contains the items most likely to surface API-contact or perf
surprises (gpui-component Table integration, GPUI-side multi-window mechanics,
1M-row virtualized rendering). De-risking these in their own merge keeps P3b
a pure UX phase on a stable hot path. Spec §21.2 P3 exit criteria are split
across the two sub-phases — see §11.

## 3. Architecture (locked from brainstorm)

One dat0 OS process. Inside the process: a global **AppLock** owns the
singleton invariant; an in-process **WindowRegistry** holds the live windows
and scaffolds the WorkspaceMutex consumed by P4; each window owns a
**Session** which owns one **DuckDBEngine** (the P2 surface, untouched) plus
a scratch directory plus the tab list.

```
┌─────────────────────────── dat0 process ────────────────────────────┐
│  ┌── AppLock ──────────────────────────────────────────────────────┐│
│  │  PID + flock @ $STATE/dat0.pid                                  ││
│  │  UDS @ $STATE/dat0.sock (IPC: {open_window, paths?})            ││
│  └─────────────────────────────────────────────────────────────────┘│
│  ┌── WindowRegistry (in-process) ──────────────────────────────────┐│
│  │  WorkspaceMutex { canonical_path → tokio::sync::Mutex } (P4)   ││
│  │  Vec<WindowHandle>                                              ││
│  └─────────────────────────────────────────────────────────────────┘│
│  ┌── Window N ─────────────────────────────────────────────────────┐│
│  │ Session { scratch_dir, DuckDBEngine, tabs, active_tab }         ││
│  │   ┌─ DataGrid (gpui-component Table) ─┐                         ││
│  │   │   + arrow batch pull adapter      │                         ││
│  │   │   + virtualized scroll            │                         ││
│  │   │   + cell renderers                │                         ││
│  │   └────────────────────────────────────┘                        ││
│  │ FileDropHandler ──► Engine::register_file (P2 API)              ││
│  └─────────────────────────────────────────────────────────────────┘│
└──────────────────────────────────────────────────────────────────────┘

$STATE/scratch/{windowId}/
  ├─ scratch.duckdb   (DuckDB durability supplies crash recovery)
  ├─ session.json     (tabs + active_tab; rewritten on tab-state change)
  └─ orphan = no live windowId in registry → recovery banner on next launch
```

### 3.1 Process model — single-instance (locked from brainstorm)

Second `dat0` launch detects flock contention on `$STATE/dat0.pid`,
connects to `$STATE/dat0.sock`, sends `{open_window, paths?}`, exits 0.
Window registry stays in-process; concurrent-open of any future workspace
file is protected by an in-process mutex over its canonical path, not a
filesystem advisory lock.

Stale state recovery:
- Stale PID file (no live process) → flock acquire succeeds, PID rewritten.
- Stale UDS socket file → `unlink` + recreate.
- Running instance but UDS unreachable (process hung) → fallback: stderr
  `dat0 is already running but unresponsive`, exit 1.

UDS protocol is JSON-lines, single round-trip per launch.
Backwards-compat surface kept minimal — single message type `{open_window, paths?}`.

### 3.2 Window registry

In-process `Arc<Mutex<WindowRegistry>>` held by the app singleton. Fields:

```rust
pub struct WindowRegistry {
    windows: Vec<WindowHandle>,                            // live windows
    workspace_mutex: HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>,  // P4 scaffold
}
```

`workspace_mutex` is added in P3a but **only exercised in P4** when SQL
Console / Workspace mode lands. P3a's `Session` does not touch it; Scratch
mode operates entirely within its own per-window dir. This is the P2 retro
recommendation #1 ("budget time for concurrent-open before SQL Console
lands") satisfied as scaffolding rather than just-in-time.

### 3.3 Session (per-window)

```rust
pub struct Session {
    window_id: Uuid,                  // v7, stable for the window's lifetime
    scratch_dir: PathBuf,             // $STATE/scratch/{window_id}/
    engine: Arc<DuckDBEngine>,        // P2 surface, unchanged
    tabs: Vec<Tab>,
    active_tab: Option<usize>,
}

pub struct Tab {
    table_name: String,           // matches TableInfo.name returned by engine
    source_path: Option<PathBuf>, // file backing the registration, for UX
}
```

Tab state is serialized to `scratch_dir/session.json` on every mutation
(`add_tab`, `remove_tab`, `set_active`). Re-read on recovery.

### 3.4 DataGrid (locked from brainstorm)

The grid widget wraps `gpui-component`'s `Table` and consumes
`duckdb::arrow::record_batch::RecordBatch` **directly** — no standalone
`arrow = "..."` workspace dependency. A grep gate in CI enforces this
(see §10).

```rust
pub struct GridDataSource {
    engine: Arc<DuckDBEngine>,
    table_name: String,
    schema: Arc<duckdb::arrow::datatypes::Schema>,
    cache: LruCache<RowRange, Arc<RecordBatch>>,
    row_count: u64,                 // computed once at init via COUNT(*)
}
```

`row_count` is computed eagerly at data-source init for grid scrollbar sizing.
For very large registered Parquet this is cheap; for CSV it forces a
scan, which is acceptable for Scratch mode (P2 already pays this cost).

The pull adapter implements `gpui_component::table::TableDelegate` (or
whichever trait T0 finds in the pinned crate). When the grid requests rows
`[start..end]`, the adapter:

1. Computes covering `RowRange { start, len }` (rounded to a 1024-row page).
2. Cache hit → return cached `Arc<RecordBatch>`.
3. Cache miss → spawn `engine.execute_paged("SELECT * FROM {table_name} LIMIT n OFFSET k", ...)`, await result, insert in cache, return.
4. Cache invalidated on tab close; LRU bound = 16 batches per grid (~16k rows).

P3a tables are read-only (no edits / inserts in scope until P4), so the
cache is a monotonic snapshot. No staleness handling required.

Cell renderers are per-column, dispatched at column-init based on `DataType`:
type badges, NULL highlighting (gray-italic placeholder), numeric right-alignment,
`Int64`/`UInt64` BigInt rendering (lossless string conversion at render time,
not at engine boundary).

### 3.5 FileDropHandler

Per-window component bound to the GPUI window's drop handler. On drop:

1. For each path: call `FileFormat::from_extension(&path)` (existing P2 API on `dat0-engine::types`). `None` → emit Banner `"Unsupported file type: <ext>"`, drop the path.
2. Build `RegisterOpts { format: Some(fmt), ..Default::default() }`. Wizard-driven options (delimiter, encoding, type overrides) ship in P3b; P3a uses defaults only.
3. `await engine.register_file(&path, opts)` (already async — engine handles `spawn_blocking` internally). Show indeterminate Banner spinner while awaiting. Engine returns `TableInfo` with auto-derived `name`.
4. On success: append `Tab { table_name: info.name, source_path: Some(path) }`, set active, persist `session.json`, push fresh `GridDataSource` to DataGrid.
5. On failure: emit error_ux Banner (existing P1 single-message form — the
   structured `{title, body, link, action}` refactor is **P3b**).

Drops are processed concurrently per-path (P2 engine serializes via internal `Mutex<Connection>` and handles back-pressure fine), but UI activation only ever sets the last successfully-registered file as the active tab.

**SQLite drops** are out of scope for P3a. `.sqlite` files require ATTACH + table-picker UX (multi-table source) which belongs with the import wizard in P3b. P3a's drop handler treats `.sqlite` as `from_extension() = None` → Banner.

## 4. Cleanup tasks folded into P3a

### 4.1 D-011 — remove `__debug_query_scalar`

The `#[deprecated]` test-only helper from P2's T2 fix-up is removed in
P3a's first batch of tests. The cleanup is a dedicated atomic task (call
it T-cleanup-1). All consumers in `crates/` are rewritten to use the
public `Engine::execute_paged` path. A grep gate enforces no
re-introduction (§10).

The scheduled cleanup agent `trig_01SNW3fxTeeR1gHkCTe37HHE` (fires
2026-05-19T13:00:00Z) is cancelled once P3a's T-cleanup-1 lands on a PR
branch — note: cancel from the agent dashboard, not in code.

### 4.2 D-012 — engine catalog `TableInfo` synthesis

Verified against live code (`crates/dat0-engine/src/catalog.rs` + `types.rs`):

- `catalog::get_tables` returns every `TableInfo` with `origin: TableOrigin::Derived(DerivedOrigin::Sql(String::new()))` regardless of how the table was created. Real `TableOrigin` enum already has the right variants (`File(PathBuf)`, `Derived(DerivedOrigin)`, `Attached { alias, source }`); the bug is that `get_tables` discards them.
- `catalog::create_table` hardcodes `schema: "main"` in its returned `TableInfo` (line 72). `get_tables` reads schema correctly from `information_schema.tables` (line 57). Only `create_table` needs the fix.

T-cleanup-2 wires real values:

- Engine internal state grows a `HashMap<String, TableOrigin>` populated on `register_file` (→ `File(path)`), `create_table` (→ `Derived(DerivedOrigin::Sql(sql))`), `attach` (→ `Attached { alias, source }`).
- `get_tables` joins `information_schema` rows against this map; tables not in the map fall through to `Derived(DerivedOrigin::Sql(String::new()))` (preserves current behavior for any unmapped table).
- `create_table` returns the resolved schema instead of the hardcoded `"main"` (read back via `information_schema` lookup after CREATE).

Catalog API surface is unchanged — only the values inside `TableInfo` are
corrected. Affects no P2-merged callers (catalog is consumed only by the
grid in P3a).

## 5. Data flow (locked from brainstorm)

### 5.1 Cold launch
1. flock `$STATE/dat0.pid` → if held, connect UDS, send `{open_window, paths?}`, exit 0.
2. Otherwise: write PID, listen on UDS, spawn first Window via GPUI main thread.
3. Window init: `windowId = Uuid::now_v7()`, mkdir `$STATE/scratch/{windowId}/`, build `DuckDBEngine` against `scratch.duckdb`, write empty `session.json`.
4. Scan `$STATE/scratch/*` for orphans (any dir not represented in the live registry). If any: emit recovery Banner with count + Open/Discard actions.

### 5.2 File drop (per path)
1. GPUI drop → `FileDropHandler::handle(path)`.
2. `FileFormat::from_extension(&path)` → `Some(Csv|Tsv|Json|Jsonl|Ndjson|Parquet)` or `None` (Banner + drop).
3. Build `RegisterOpts { format: Some(fmt), ..Default::default() }`.
4. `engine.register_file(&path, opts).await` (async, internally bridged via `spawn_blocking`). Banner spinner active.
5. On `Ok(info)`: append `Tab { table_name: info.name, source_path: Some(path) }`, persist `session.json`, instantiate `GridDataSource::new(engine.clone(), info.name.clone())`, hand to DataGrid.

### 5.3 Grid pull-stream
1. `TableDelegate::row_range(start, end)`.
2. Adapter rounds to page, checks LRU; on miss issues paged SELECT.
3. Batch returned; column `Arc<dyn Array>` flowed to cell renderers.

### 5.4 Tab switch
- Active tab change → previous `GridDataSource` dropped → new one mounted → `session.json` rewritten.

### 5.5 Second-launch IPC
- Running instance's UDS handler receives `{open_window, paths?}` → spawns new Window on main thread → if `paths` present, runs (5.2) per path on the new Window.

## 6. Error handling (locked from brainstorm)

| Class | Surface | Behavior |
|---|---|---|
| Orphan scratch dir on relaunch | Banner with `[Open] [Discard]` | Open → attach as new Window with restored tab list; Discard → `rm -rf` after confirm |
| Unsupported file (extension sniff = `None`) | Banner `Unsupported file type: <ext>` | Tab NOT created; sibling drops still process |
| Corrupt/malformed file at register | Banner `<file>: <EngineError>` (from `register_file` → `EngineError::DuckDb` / `Arrow` / `Io` / `UnsupportedFormat`) | Tab NOT created; sibling drops still process |
| AppLock contention | UDS round-trip | `{open_window, paths?}` ACK → exit 0; UDS unreachable → stderr message + exit 1 |
| Stale PID file | flock succeeds | Rewrite PID, proceed |
| Stale UDS socket | unlink + bind | Standard pattern |
| Large CSV register | Banner spinner indeterminate | No cancel in P3a (deferred to P3b alongside import wizard) |
| DuckDB OOM at grid pull (surfaces as `EngineError::DuckDb` with OOM-text payload — no dedicated `OutOfMemory` variant in P2 surface) | Cells render `<oom>` placeholder + Banner with docs link | Settings widgets ship P3b — no in-app remediation yet; pattern-match on `err.to_string().contains("out of memory")` for the OOM-specific Banner (loose; revisit if `EngineError` grows a dedicated variant) |
| User edits PID file manually (singleton broken) | Second instance starts | Scratch has no cross-instance conflict; WorkspaceMutex (P4) will catch workspace case |

## 7. Components and file layout

New module structure in `crates/dat0-app/`:

```
crates/dat0-app/src/
  app_lock.rs          // PID file, flock, UDS server + client
  window_registry.rs   // in-process registry + WorkspaceMutex scaffold
  session.rs           // per-window Session + tab persistence
  file_drop.rs         // drop handler + format detection wrapper
  grid/
    mod.rs             // public surface
    data_source.rs     // GridDataSource (arrow batch pull adapter)
    renderers.rs       // per-DataType cell renderers
    bench.rs           // 1M-row synthetic bench harness (criterion)
```

P1's `error_ux::Banner` consumed unchanged (single-message form).
P2's `DuckDBEngine` consumed unchanged.

## 8. Testing (locked from brainstorm)

### 8.1 Unit (per crate, fast)
- `dat0-app::app_lock` — flock acquire/release, stale-PID recovery, UDS message round-trip with tempdir-isolated `$STATE`.
- `dat0-app::session` — scratch-dir create/cleanup, `session.json` round-trip, tab append/active-tab persistence.
- `dat0-app::grid::data_source` — paged batch retrieval against in-memory `DuckDBEngine`; LRU eviction; cache drop on tab close.
- `dat0-app::file_drop` — extension→format detection table; collision-rename logic.
- `dat0-engine::catalog` — D-012 closure: `TableInfo { origin, schema }` correctly populated.

### 8.2 Integration (`tests/`)
- `tests/scratch_lifecycle.rs` — spawn process, drop CSV via harness, assert grid first-paint, `kill -9`, relaunch, assert orphan-recovery Banner emitted, assert tab list restored.
- `tests/multi_window.rs` — two windows in one process, drop different files, assert engine isolation (same `table_name` allowed across windows), assert registry has 2 entries.
- `tests/single_instance.rs` — spawn A, spawn B → B exits 0; A's UDS handler received message; A's window count 1→2.
- `tests/file_drop_formats.rs` — exercises P2 `register_file` for all 6 supported formats (CSV/TSV/JSON/JSONL/NDJSON/Parquet) via the UI drop path; asserts table appears in grid. `.sqlite` drop asserts Banner emitted + no Tab created (deferred to P3b).

### 8.3 Bench (`crates/dat0-app/benches/grid_scroll.rs`)
- 1M-row synthetic Arrow batch — int + float + string + bigint + nullable. Drives virtualized scroll across full range, samples frame times.
- Output: `p50 / p95 / p99 frame time + computed fps`.
- Runs in CI on macos-14 hosted, output uploaded as artifact, **no merge gate** (aligned with spec line 819 — perf benches not enforced as merge gates until P10).
- Spec target: 60 fps. Soft floor recorded in commit notes; manual review on PR.

### 8.4 Manual UAT (mac dev box)
- Drop 100 MB CSV (NYC taxi sample) → smooth scroll, type badges visible.
- Open second window, drop different file, switch windows → independent state.
- `kill -9 $(pgrep dat0)` mid-scroll → relaunch → recovery Banner emitted.
- Drop garbage `.bin` → Banner error; sibling CSV in same drop batch still loads.

## 9. CI

Matrix unchanged from PR #3 state: macos-14 (hosted) + linux-x86_64 (runnerkit self-hosted). `concurrency: cancel-in-progress` retained.

**Added to `ci.yml`** (macos-14 job only):
- Bench step: `cargo bench -p dat0-app --bench grid_scroll -- --output-format=bencher | tee bench.txt`, uploaded as artifact.

**Added grep gates (both jobs):**
- `! grep -r '^arrow = ' Cargo.toml crates/*/Cargo.toml` (P2 retro #2).
- `! grep -r '__debug_query_scalar' crates/` after T-cleanup-1 lands.

`heavy.yml` and `notice.yml` untouched.

## 10. Grep gates summary

| Gate | Where | Why |
|---|---|---|
| `! grep -r '^arrow = '` | ci.yml | Prevents standalone Arrow dep that would silently break the `duckdb::arrow::RecordBatch` type-chain (P2 exit criterion #11, P2 retro #2) |
| `! grep -r '__debug_query_scalar'` | ci.yml | Prevents re-introduction of the deprecated test helper after D-011 cleanup |

## 11. Exit criteria

P3a exit criteria — mapped to spec §21.2 P3 (those addressed in P3a only; the
rest are P3b):

| # | Criterion | Verification |
|---|---|---|
| 1 | Drop 100 MB CSV → table appears in grid, scrolls smoothly | UAT + integration test |
| 2 | Drop all 6 supported formats (CSV/TSV/JSON/JSONL/NDJSON/Parquet) → grid renders; `.sqlite` rejected with Banner | `tests/file_drop_formats.rs` |
| 3 | Open second window → independent scratch + engine | `tests/multi_window.rs` |
| 4 | Force-quit during import → next launch surfaces recovery Banner | `tests/scratch_lifecycle.rs` |
| 5 | Second `dat0` launch from terminal → forwards to running instance | `tests/single_instance.rs` |
| 6 | 1M-row scroll bench produces p99 frame-time output on macos-14 CI | bench artifact present, manual review against 60 fps target |
| 7 | D-011 `__debug_query_scalar` removed from codebase | grep gate green |
| 8 | D-012 `TableInfo { origin, schema }` populated with real values | `dat0-engine::catalog` unit test |
| 9 | `arrow = ` standalone dep grep gate active and green | CI step present |
| 10 | All exit criteria for P2 still pass | full `cargo test --workspace` green |

Out-of-scope-for-P3a spec §P3 exit items (deferred to P3b):
- Theme change reflects in grid immediately (D-002)
- Cmd-Shift-P opens command palette
- Import wizard prompts on ambiguous CSV
- Long import shows progress + cancel
- Fresh-launch empty-state hero + sample data

## 12. Open questions / risks

- **gpui-component `Table` API surface** — P3a T0 spike. Whether trait is `TableDelegate` or another name, whether row_range is start/end or start/len, whether cell rendering takes `&Array` or column-by-column. Spike resolves before any rendering task.
- **GPUI drop event surface** — verify what `Window::on_drop` (or equivalent) gives us: `Vec<PathBuf>` directly, or platform-typed payload requiring extraction. T0 verifies.
- **macos-14 bench variance** — hosted runners share metal with other tenants. Some run-to-run variance expected. P3a accepts artifact-only; if variance is extreme (p99 swings >2×), recommend skipping the bench on CI and running locally only, capturing the decision as a deferral.
- **WAL files in scratch dir** — DuckDB writes `.wal` alongside `.duckdb`. Orphan detection must ignore `.wal` siblings, not treat them as additional orphan dirs.
- **Tab-state write churn** — `session.json` rewritten on every tab change. Fine in scratch (tab changes are user-driven, low frequency). If P3b adds tab-state for scroll/column-width, that becomes hot — rate-limit at that point, not now.

## 13. Deferrals referenced

| Deferral | Status after P3a |
|---|---|
| D-001 | Open (P3b) |
| D-002 | Open (P3b) |
| D-007 | Open (P5) |
| D-008 | Open (P5) |
| D-010 | Open (TBD) — partial pressure from drop handler error surface |
| D-011 | **CLOSED** (T-cleanup-1) |
| D-012 | **CLOSED** (T-cleanup-2) |
| D-013 | Open (TBD) — bench runs on hosted macos-14 in the meantime |

---

**Next step:** This spec is the brainstorm output. The implementation plan
(`docs/plans/2026-05-14-dat0-p3a-plan.md`) is generated by the `writing-plans`
skill in the next session step.
