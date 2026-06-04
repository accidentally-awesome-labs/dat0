# P5b — SQL Console: Intelligence, Reuse, Promotion (design)

> Date: 2026-06-04
> Phase: P5b (middle slice of the three-way P5 SQL-Console split: P5a editor/run/cancel/multi-tab → **P5b** → P5c MotherDuck ATTACH)
> Status: design approved, plan pending
> Spec ref: `docs/specs/2026-04-26-dat0-design.md` §8.3 (SQL Console), §21.2 P5 scope/exit gates
> Predecessor: `docs/plans/2026-06-02-dat0-p5a-design.md` (merged PR #10 squash `dfe08f4`)

P5b delivers the remaining P5 scope minus MotherDuck: schema-driven autocomplete, query
history, saved queries, "Save as Table" with lineage, command-palette action registration,
and a query-timing chip. It is deliberately the **fat middle slice** — the user chose to keep
it whole with a trim valve rather than split again.

---

## §0. Locked decisions

| # | Decision | Rationale |
|---|----------|-----------|
| **D1** | P5b ships **all 6 features whole**. **Save as Table is the trim valve**: if the T0 autocomplete spike runs hot, Save-as-Table (both paths) drops to P5c. | Preserves the agreed 3-way a/b/c split; mirrors P5a's "results pane is first trim" tactic. |
| **D2** | Query history + saved queries persist in **session.json, additive v5→v6**. | Workspace mode (`.dat0/queries/`) is unbuilt — everything is per-window scratch today. Reuses the proven migration ladder + atomic persist. De-facto per-workspace until real workspace mode lands (P7), then migrate into `.dat0/queries/`. |
| **D3** | Timing chip shows **plain elapsed** (`⏱ N ms · local`). The local-vs-md comparison waits for P5c (no `md` to compare against until ATTACH). | P5a already tracks `started_at`/elapsed; the `· local` label reserves the P5c slot. |
| **D4** | Command palette = **register descriptors only**. The overlay UI stays stubbed. | The palette overlay (open→filter→dispatch) is a pre-existing P3b deferral; wiring it would pull a deferred UI item into an already-heavy slice. Spec asks for "actions registered". |
| **D5** | Save as Table ships **both entry points**: grid transform-stack promotion (`DerivedOrigin::Transform`) **and** SQL-console result promotion (`DerivedOrigin::Sql`). | Grid path populates the never-used lineage type and satisfies the exit criterion "parent lineage attached"; console path matches the §8.3 "promotes a query result" wording. |

---

## §1. Grounded API facts (verified against live code, not recalled)

All confirmed this session by reading source — these are load-bearing for the design.

### CompletionProvider seam (gpui-component `0f0ab35`) — CONFIRMED PRESENT
- Trait `CompletionProvider` at `crates/ui/src/input/lsp/completions.rs:20`. Required methods:
  - `fn completions(&self, text: &Rope, offset: usize, trigger: CompletionContext, window: &mut Window, cx: &mut Context<InputState>) -> Task<Result<CompletionResponse>>`
  - `fn is_completion_trigger(&self, offset: usize, new_text: &str, cx: &mut Context<InputState>) -> bool`
  - `inline_completion`, `inline_completion_debounce`, `resolve_completions` all have **default impls** — we only implement the two above.
- Attach point: `InputState.lsp` is a **public field** (`state.rs:317`); `Lsp.completion_provider: Option<Rc<dyn CompletionProvider>>` is **public** (`lsp/mod.rs:25`). No builder method → set the field directly after construction.
- Types `CompletionContext`, `CompletionResponse`, `CompletionItem` come from `lsp_types`. gpui-component re-exports only `Rope` and `Position` (`input/mod.rs:31,33`), **not** the Completion* types → dat0 needs a **direct `lsp_types` dep** at the exact lock version. `ropey` is already transitive (Cargo.lock).
- Trigger flow: `InputState::handle_completion_trigger` (`completions.rs:107`) calls the provider only when `lsp.completion_provider` is set; menu rendering is `popovers/completion_menu.rs`.

### Engine — Save-as-Table + autocomplete schema, ALL READY
- `catalog::get_tables(conn, origins) -> Vec<TableInfo{name, schema, columns, origin}>`; `catalog::describe_table(...) -> Vec<ColumnInfo{name, data_type, nullable}>`.
- Trait (`trait_def.rs`): `async fn get_tables() -> Result<Vec<TableInfo>>` (:79), `async fn describe_table(...)` (:78), `async fn create_table(&self, name, sql, origin: DerivedOrigin) -> Result<TableInfo>` (:43), `async fn execute_paged`, `create_or_replace_view`, `execute`.
- `create_table` does CTAS + eager `__dat0_rowid` injection (`duckdb_engine.rs:327`), stores the passed origin.
- `DerivedOrigin` (`types.rs:108`): `Sql(String)` | `Transform { parent: String, ops: Vec<Transformation> }`. The `Transform` variant **exists but is never populated** in P4b/P4c.
- `compile_view_sql(base, ops) -> Result<String>` (`render.rs:40`) compiles a transform stack to a SELECT.
- `ViewModel.present: Vec<Transformation>` (`view/model.rs:22`) holds the active grid transform stack; `active()` getter.

### Session / persistence — v5, additive ladder proven
- `SESSION_SCHEMA_VERSION = 5` (`session/mod.rs:40`). `SessionState` (`:77`) is the persisted shape: `schema_version`, `tabs`, `active_tab`, `sql_tabs`, `active_sql_tab`, all `#[serde(default)]`.
- `SqlTabState { id, title, sql }` (`:64`). Persist is atomic write-rename-fsync (`Session::persist`, `:314`). `set_sql_tabs` (`:296`) mutates + persists.
- Migration ladder: literal-arm chain; `migrate_v4_to_v5` (`migrate.rs:281`) = parse → stamp version → return (purely additive template).

### SQL console (P5a) — extend points
- `SqlConsole { tabs: Vec<ConsoleTab>, active, region: ResultRegion, running, started_at: Option<Instant>, pane_source, pane_table_state, pane_ws }` (`view/sql_console.rs:60`).
- `ConsoleTab { meta: SqlTabMeta, input: Entity<InputState> }` (:36); input built `InputState::new(window,cx).code_editor("sql").line_number(true)` (:113).
- Events `SqlConsoleEvent::{Run{target}, Cancel, Persist}` (:84) routed to `WorkspaceShell::on_sql_console_event` (`window.rs:1147`). Run flow `spawn_sql_run`/`finish_sql_run` (`window.rs:1215`/`1292`).
- `query/statement.rs`: `statement_at(sql, cursor) -> Span`, `classify(stmt) -> ResultKind`. `query/highlight.rs`: `register_sql_language()`.

### Actions / palette
- `ActionRegistry` (`actions/registry.rs:91`), `ActionDescriptor { id, title, group, keybinding, dispatch }` (:64). 24 actions today (test `tests/action_registry.rs:76`), incl. P5a's 5 SQL ids (`builtin.rs:46`). `command_palette::filter` works; **overlay UI is a P3b stub, not wired to open/dispatch**.

---

## §2. Feature design

### 2.1 Schema autocomplete (`query/completion.rs` — new) — marquee, highest risk

```
SchemaSnapshot { tables: Vec<TableEntry>, functions: Vec<SharedString> }
TableEntry     { name: SharedString, columns: Vec<SharedString> }
SchemaCompletionProvider { snapshot: Rc<RefCell<SchemaSnapshot>> }
```

- **`impl CompletionProvider for SchemaCompletionProvider`**:
  - `is_completion_trigger(offset, new_text, _)` → `true` when the char immediately left of the cursor is `[A-Za-z0-9_.]`. Avoids firing on whitespace/operators (except `.`).
  - `completions(text: &Rope, offset, _trigger, _window, _cx)` → **pure, synchronous, from the cached snapshot**; returns `Task::ready(Ok(CompletionResponse::Array(items)))`. No async, no engine call inside (keeps it main-thread-safe and instant). Algorithm:
    1. Walk left from `offset` over `[A-Za-z0-9_]` to get `word`; if the char before `word` is `.`, walk further to get `qualifier`.
    2. `qualifier.` present and matches a table name → suggest that table's columns (kind `Field`).
    3. else → prefix-match `word` against union {table names (kind `Class`), all column names (kind `Field`), function names (kind `Function`)}.
    4. Map to `lsp_types::CompletionItem { label, kind, .. }`.
- **Attach**: in `ConsoleTab` construction, after `.code_editor("sql")`, set `state.lsp.completion_provider = Some(Rc::new(provider))`. The `Rc<RefCell<SchemaSnapshot>>` is **one per window**, cloned into every tab's provider so a refresh updates all tabs.
- **Snapshot refresh**: owned by `WorkspaceShell` (has the `Arc<DuckDBEngine>`). `refresh_completion_snapshot()` calls `engine.get_tables()` (async, spawned) → rebuilds `SchemaSnapshot` → writes through the `RefCell`. Triggered on: (a) console first opened, (b) every successful run finish (covers CREATE/DROP/Save-as-Table), (c) file import / table add.
- **Functions list**: a curated static array (~40 common DuckDB funcs: `count sum avg min max coalesce cast try_cast nullif greatest least abs round floor ceil length lower upper trim substring replace concat date_trunc strftime strptime epoch now current_date list_aggr regexp_matches regexp_replace row_number rank dense_rank lag lead first last`). **YAGNI** on `duckdb_functions()` introspection — it's huge and noisy.
- **New deps**: add `lsp_types` and `ropey` as **direct** deps pinned to the exact versions gpui-component resolves to in `Cargo.lock`. `notice`/cargo-about gate (see P5a) will need a `NOTICE.md` regen for any newly-direct license line.
- **T0 spike (task 1, mandatory before everything else)** proves:
  1. lsp_types/ropey version pin compiles + links against `0f0ab35`.
  2. Setting `lsp.completion_provider` makes the menu appear while typing an identifier.
  3. Enter/Tab inserts the highlighted item into the buffer.
  4. The `qualifier.` path suggests that table's columns.
  - **Fallback (decision discipline, mirrors P5a decision-7)**: if the menu won't render/insert at this rev, ship the provider anyway + document the menu-UX gap and move the visual exit-criterion to P5c; do **not** fork into a heavier upstream bump inside P5b.

### 2.2 Query history (last 100) — D2

```
HistoryEntry { sql: String, ran_at: i64 /*unix ms*/, ok: bool, elapsed_ms: u64 }
```
- Ring buffer, cap **100**, drop-oldest. Persisted in `SessionState.query_history: Vec<HistoryEntry>` (v6).
- **Capture**: `finish_sql_run` pushes an entry on every run (success *and* error), then persists (session.json already persists per run, so no extra write pressure beyond a ~100-element vec).
- **UI**: a clock button in the console toolbar + action `sql.history` → scrollable list (newest first, sql preview + relative time + ok/err dot + elapsed). Click → load that SQL into a **new tab**.

### 2.3 Saved queries (named) — D2

```
SavedQuery { id: Uuid, name: String, sql: String, saved_at: i64 }
```
- Persisted in `SessionState.saved_queries: Vec<SavedQuery>` (v6).
- **Save**: toolbar button + action `sql.save_query` → name modal (reuse the `view/export_dialog.rs` modal pattern) → push + persist.
- **Load**: toolbar button + action `sql.load_query` → picker list → load into a new tab.
- **Delete**: `x` per row in the picker.

### 2.4 Save as Table — D5 both paths (TRIM VALVE)

- **Console path** (`DerivedOrigin::Sql`): action `sql.save_as_table` + button in results pane/toolbar → name modal → `engine.create_table(name, format!("SELECT * FROM ({stmt})", stmt = statement-under-cursor-or-last-run), DerivedOrigin::Sql(stmt))`.
- **Grid path** (`DerivedOrigin::Transform`): a "Save as Table" button on `PipelineBar` → `let sql = compile_view_sql(base, present)?;` → `engine.create_table(name, sql, DerivedOrigin::Transform { parent: base.clone(), ops: present.clone() })`. **This populates the previously-unused lineage type and satisfies the exit criterion "parent lineage attached".**
- Both paths: after a successful create, call `refresh_completion_snapshot()` so the new table appears in autocomplete. **No catalog tree** (that's P6) — the table is discoverable via autocomplete + `SHOW TABLES`.
- **Name collision**: `CREATE TABLE` fails if the name exists; surface the DuckDB error **inline** in the console (reuse P5a's `ResultRegion::Error` rendering — keeps sidestepping the open PD-021 banner-queue bug).

### 2.5 Timing chip (plain elapsed) — D3

- P5a already stores `started_at: Option<Instant>`. On `finish_sql_run`, compute `elapsed_ms`.
- Render a small chip in the results header: `⏱ 42 ms · local`. The `· local` suffix reserves the slot for P5c's local-vs-md comparison.
- The same `elapsed_ms` feeds `HistoryEntry.elapsed_ms`. Wall-clock `ran_at` uses `SystemTime::now()` (app runtime — fine).

### 2.6 Command-palette registration — D4

- New action ids in `actions/builtin.rs`: `sql.save_query`, `sql.load_query`, `sql.history`, `sql.save_as_table`, and grid `view.save_as_table`.
- Register descriptors in a new `actions/sql_actions.rs` (called from `register_all`, same pattern as `view_actions`/`edit_actions`): real App-path dispatch where it works, view-scoped stubs where `&mut Window` is required (exactly like P5a's `console.toggle`).
- Bump the action-count test (`tests/action_registry.rs`, 24 → 24 + N).
- **Overlay stays stubbed** — out of scope per D4.

### 2.7 Persistence / migration

- `SESSION_SCHEMA_VERSION` **5 → 6**. `SessionState` gains `#[serde(default)] query_history: Vec<HistoryEntry>` and `#[serde(default)] saved_queries: Vec<SavedQuery>`.
- `migrate_v5_to_v6` = additive stamp (template = `migrate_v4_to_v5`, `migrate.rs:281`); add the ladder arm.
- New `Session::set_query_history(...)` and `Session::set_saved_queries(...)` (mutate + persist), mirroring `set_sql_tabs`.

---

## §3. Module layout

| File | Change |
|------|--------|
| `crates/dat0-app/src/query/completion.rs` | **new** — `SchemaSnapshot`, `SchemaCompletionProvider`, word/qualifier extraction, functions list |
| `crates/dat0-app/src/view/query_library.rs` | **new** — history list + saved-query picker views |
| `crates/dat0-app/src/actions/sql_actions.rs` | **new** — P5b action registrations |
| `crates/dat0-app/src/view/sql_console.rs` | extend — toolbar buttons (history/save/load/save-as-table), timing chip, completion-provider wiring per tab |
| `crates/dat0-app/src/view/pipeline_bar.rs` | extend — "Save as Table" button (grid Transform-origin path) |
| `crates/dat0-app/src/window.rs` | extend — `refresh_completion_snapshot`, history capture in `finish_sql_run`, save/load/save-as-table handlers |
| `crates/dat0-app/src/session/mod.rs` | extend — v6 fields + `set_query_history`/`set_saved_queries` |
| `crates/dat0-app/src/session/migrate.rs` | extend — `migrate_v5_to_v6` + ladder arm |
| `crates/dat0-app/src/actions/builtin.rs` | extend — new action ids |
| `crates/dat0-app/Cargo.toml` | extend — `lsp_types`, `ropey` direct deps |
| `NOTICE.md` | regen if any new direct dep adds a license line (cargo-about/`notice` gate) |

---

## §4. Testing

**Unit (`cargo test`, pure logic):**
- Word/qualifier extraction from a `Rope` at an offset — table-driven (bare word, `tbl.col`, mid-identifier, after operator, empty).
- History ring buffer: cap-100 enforcement, drop-oldest order.
- Saved-query CRUD (add/find/delete) on the vec.
- Session v5→v6 migration round-trip (old json with no history/saved fields → defaults populated; re-serialize stable).
- `classify`/`statement_at` unchanged (regression).

**Integration (engine):**
- `create_table` with `DerivedOrigin::Sql` and with `DerivedOrigin::Transform{parent,ops}` → `get_tables()` lists the table with the recorded origin.
- Snapshot refresh after create → new table name present in the rebuilt `SchemaSnapshot`.

**UAT-owed** (joins the P4b/P4c/P5a manual backlog; not gating merge):
- Autocomplete menu renders on typing; Enter/Tab inserts.
- `tbl.` suggests that table's columns.
- History list loads a past query across relaunch.
- Saved query persists + loads across relaunch.
- Save-as-Table both paths create a queryable table; grid path records Transform lineage.
- Timing chip shows a plausible elapsed value.

---

## §5. Risks & open items

| Risk | Mitigation |
|------|------------|
| **R1 (top)** — completion menu won't render/insert at `0f0ab35` | T0 spike gates it; documented fallback (ship provider, defer visual exit-criterion to P5c) per §2.1. |
| **R2** — `lsp_types` version pin / API drift | T0 confirms compile+link before any feature work. |
| **R3** — slice runs hot | **Trim valve**: Save-as-Table (both paths) drops to P5c (D1). |
| **R4** — persisting 100-entry history each run | Negligible — session.json stays small; atomic write already per-run. |
| PD-021 (banner-queue) still open | Errors render **inline** in console, sidestepping it (continues P5a's choice). |
| Palette overlay still stubbed | Out of scope (D4); pre-existing P3b deferral, unchanged. |

---

## §6. Task outline (~14–16; full breakdown in the plan doc)

0. **T0 spike** — add `lsp_types`/`ropey` deps; prove menu render/insert/`.`-path; NOTICE regen. **Gate.**
1. `query/completion.rs` — snapshot types + word/qualifier extraction + functions list + unit tests.
2. `SchemaCompletionProvider` impl + attach per tab + `refresh_completion_snapshot` wiring.
3. Session v6 — fields, `migrate_v5_to_v6`, set-methods, migration test.
4. Query history — `HistoryEntry`, ring logic, capture in `finish_sql_run`, unit tests.
5. History UI — clock button + list view + load-into-tab.
6. Saved queries — `SavedQuery`, save/load/delete, name modal.
7. Saved-query UI — picker view.
8. Save-as-Table console path (`DerivedOrigin::Sql`) + name modal + snapshot refresh. *(trim candidate)*
9. Save-as-Table grid path (`DerivedOrigin::Transform`) on PipelineBar + integration test. *(trim candidate)*
10. Timing chip render + feed history elapsed.
11. `actions/sql_actions.rs` — register new ids; bump count test.
12. Whole-feature review + workspace gate + UAT doc.

---

*Approved whole 2026-06-04. Next: writing-plans → `docs/plans/2026-06-04-dat0-p5b-plan.md`.*
