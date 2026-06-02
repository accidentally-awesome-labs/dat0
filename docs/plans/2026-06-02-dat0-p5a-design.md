# dat0 P5a — Design (SQL Console: editor core + run + cancel + multi-tab · P5 first slice)

> Brainstorm output, 2026-06-02. P5 (spec §21.2 "SQL Console", ~3 weeks, 10 scope
> items + D-007 + D-008) is split **three ways** — same pattern as P3 (a/b) and P4
> (a/b/c):
>
> - **P5a** (this doc) — the "SQL is runnable" vertical slice: code editor wired,
>   run / run-selection, query progress + cancel (closes **D-008**), multi-tab,
>   per-tab persistence.
> - **P5b** — schema autocomplete, query history, saved queries, Save-as-Table +
>   lineage, command-palette SQL actions, local-vs-MotherDuck timing chip.
> - **P5c** — MotherDuck ATTACH end-to-end (**D-007**), credential-gated so a
>   missing dev token never blocks the editor work.
>
> Grounded against live code at `main` @ `e5c4e59` and the pinned gpui-component
> (`0f0ab35`) / duckdb-rs (`=1.4.4`) sources — verified by reading
> `crates/dat0-engine/src/trait_def.rs`, `crates/dat0-app/src/{session/mod.rs,window.rs,
> command_palette.rs}`, and gpui-component `crates/ui/src/{input/state.rs,input/mode.rs,
> input/lsp/completions.rs,highlighter/{registry.rs,languages.rs}}`. Facts below were
> read, not recalled.

## 0. Locked decisions (this brainstorm)

| # | Decision | Over | Because |
|---|---|---|---|
| 1 | **P5 splits three ways (P5a/P5b/P5c)**; this doc is P5a only | Single P5 / two-way | 10 items + 2 deferrals is P3/P4 heft; user chose three-way, peeling credential-gated MotherDuck into P5c |
| 2 | **Run default → reuse main DataGrid** (transient result via TEMP VIEW); **Run split-button dropdown → dedicated results pane** (opt-in) | Results-pane-only / new-result-tab-per-run | Default reuses all P4 grid machinery; the dropdown gives power users a side-by-side without clobbering the table view |
| 3 | **Token-free cancellation** — drop-guard + `engine.interrupt()`; **NO `QueryEngine` trait amendment** (closes D-008) | Amend trait with `CancellationToken` param / hybrid streaming-only | Per-window engine serializes on ONE Mutex'd connection → `interrupt()` hits exactly the running query; the deferral left the shape open "until a real call-site"; this is it |
| 4 | **Run granularity = statement-under-cursor**; selection overrides | Entire buffer / single-statement tab | DataGrip/TablePlus/psql muscle memory; keeps a scratchpad of statements in one tab |
| 5 | **Query errors render inline** in the console result region | Mount banner host (close PD-021) / both | Self-contained — sidesteps the broken `error_ux` queue (PD-021); error sits next to the SQL that caused it; PD-021 stays open for a later slice |
| 6 | **SQL highlight = runtime-register `tree-sitter-sequel` as a single direct dep** via `LanguageRegistry::register`; **never** the 28-grammar bundle feature | Enable gpui-component `tree-sitter-languages` / plain editor | One grammar, full SQL highlight, avoids ~28 grammar crates on a CI that already needed free-disk-space + `CARGO_BUILD_JOBS` caps. **T0-gated** with a defined fallback (decision 7) |
| 7 | **Highlight fallback** — if T0 shows the registry path does not drive `code_editor` highlighting, ship a **plain `code_editor`** (line numbers + multi-line, no colors) and move the highlight exit-criterion to **P5b**. Do **not** enable the bundle as the fallback | — | Registry-path highlighting is unproven (built-in path is cfg-gated through the `Language` enum); the bundle's CI cost is the thing we're avoiding, so it is not an acceptable fallback |
| 8 | **Result-producing statements** (leading keyword ∈ `{SELECT, WITH, VALUES, TABLE, FROM, PRAGMA, SHOW, DESCRIBE, EXPLAIN, SUMMARIZE}`) **run as a reused TEMP VIEW** (`__dat0_qr_<win>_<tab>`); non-result statements run via `execute()` → status line | Materialize temp table / eager `execute()` full result into RAM | View path reuses the entire P4 paged-grid read; create-or-replace overwrites in place; makes P5b "Save as Table" a trivial `CTAS … FROM <view>` |

## 1. Scope

**In:**
- **Editor** — gpui-component `InputState::code_editor("sql")`: line numbers, soft-wrap, multi-line. SQL syntax highlight via runtime-registered `tree-sitter-sequel` (decision 6, T0-gated → decision 7 fallback).
- **Multi-tab SQL editor** — console-local tab strip; per-tab buffer + active index; **persisted across reload** (session.json v4→v5).
- **Run / Run-selection** — `Cmd/Ctrl+Enter`: statement-under-cursor (decision 4); selection overrides. `;`-splitter that skips quoted strings + comments.
- **Execution** — off the main thread; result → main DataGrid by default (decision 2), via TEMP VIEW (decision 8); Run split-button dropdown → dedicated results pane.
- **Progress + cancel** — indeterminate spinner + elapsed timer + Cancel button; `Cmd+.`; token-free `interrupt()` + drop-guard (decision 3, **closes D-008**).
- **Inline error strip** — DuckDB message (+ line/col if available) in the console result region (decision 5).
- **Collapsible SQL Console panel** — mounted in `WorkspaceShell` below `PipelineBar` (spec §8.1).
- **Actions + keybinds** — `sql.run`, `sql.cancel`, `sql.new_tab`, `sql.close_tab`, `console.toggle`.
- session.json **v4→v5** (additive: `sql_tabs` + `active_sql_tab`).

**Out (explicit):**
- → **P5b**: schema autocomplete (`CompletionProvider`), query history (100/workspace), saved queries, **Save-as-Table** + lineage, command-palette **registration** of the SQL actions, local-vs-md timing chip.
- → **P5c**: MotherDuck ATTACH end-to-end (**D-007**), `md:` extension load + keychain token + per-query md routing.
- **Stays open** (not P5a's job): **PD-021** banner host (P5a uses inline errors instead); **D-015** AccessKit (P10); the **owed manual UAT** debt from P4b/P4c (P5a adds its own UAT items to the same backlog — see §6).
- DuckDB dialect-exact highlighting — `tree-sitter-sequel` is generic SQL; DuckDB-specific keywords (`QUALIFY`, `PIVOT`, list/struct syntax, `SUMMARIZE`) may render unhighlighted. Accepted; generic SQL coloring satisfies the exit criterion's intent.

## 2. Architecture

### 2.1 New per-window model — `QueryModel`

A gpui `Entity`, owned by `WorkspaceShell`, sibling to the existing `Session`/`ViewModel` surfaces.

```
QueryModel {
    tabs: Vec<SqlTab>,
    active: Option<usize>,
    exec: ExecState,            // Idle | Running { started_at, cancel } | Cancelled | Error(String)
}

SqlTab {
    id: Uuid,
    title: String,             // "Query 1", renameable later (P5b)
    input: Entity<InputState>, // gpui-component code editor; owns the buffer text
    result_target: ResultTarget,   // MainGrid (default) | Pane
    last_result_view: Option<String>,  // "__dat0_qr_<win>_<tab>" once run
}

ResultTarget = MainGrid | Pane
ExecState    = Idle | Running { started_at: Instant, cancel: QueryCancel } | Cancelled | Error(String)
```

Buffer text lives in the gpui-component `InputState` entity (one per tab); `QueryModel` reads it on Run and on persist. `result_target` is **not** persisted (resets to `MainGrid` on reload).

### 2.2 New view — `view/sql_console.rs` (`SqlConsole`)

Renders: tab strip (+ "new tab"), the active tab's code editor, the Run split-button (primary action + dropdown for "Run in results pane"), and the result region (inline error/cancel strip; the optional results pane when `result_target == Pane`). Mounts in `WorkspaceShell::render` below the `PipelineBar`, collapsible.

### 2.3 Result mechanism (decision 8)

On Run, after the statement is resolved (§3):

- **Result-producing** (leading keyword ∈ `{SELECT, WITH, VALUES, TABLE, FROM, PRAGMA, SHOW, DESCRIBE, EXPLAIN, SUMMARIZE}`):
  `engine.create_or_replace_view("__dat0_qr_<win>_<tab>", stmt)` → on `Ok`, bind the grid (or the pane) to that view name. The **existing P4 `execute_paged` grid path reads it unchanged** (column/type discovery from the view's Arrow schema). On `Err` (parse/bind) → inline error strip.
- **Non-result** (DDL/DML — `CREATE`, `INSERT`, `UPDATE`, `DELETE`, `COPY`, …): `engine.execute(stmt)` → a status line ("42 rows changed" / "OK"). On `Err` → inline error strip.

`WorkspaceShell`'s grid render branches on **table tab** vs **transient query result** (the active `last_result_view`). View lifecycle: create-or-replace reuses one stable name per tab; tab close → `drop_view`; window close → connection drop cleans temp views.

### 2.4 Cross-thread

Execution spawns on the engine task. Completion/error marshals to the main thread via the existing **`MainThreadDispatcher`** (the P3b / PD-010 `futures::mpsc` bridge) → `QueryModel` mutation → re-render. No new cross-thread primitive.

## 3. Execution + cancellation flow

1. **Trigger** — `Cmd/Ctrl+Enter` or Run button (or its dropdown → `Pane`).
2. **Resolve statement** — selection present → use it verbatim; else `statement_at(sql, cursor)` from the `;`-splitter.
3. **Classify** — `classify(stmt)` → result-producing vs non-result (leading keyword after stripping leading comments/whitespace).
4. **Enter Running** — `exec = Running { started_at, cancel }`; spinner + elapsed timer; Run button becomes Cancel.
5. **Spawn on engine** — VIEW path or EXEC path per §2.3.
6. **Marshal back** — via `MainThreadDispatcher` → `exec = Idle`; bind result / show status / show error; re-render.
7. **Cancel** — `Cmd+.`, Cancel button, a new Run, or `QueryCancel` drop → `engine.interrupt()`. The in-flight blocking call returns `EngineError::Interrupted`, rendered as a **muted "Cancelled"** status (not an error strip).

**`QueryCancel` drop-guard** (delivers D-008's "auto-interrupt-on-drop" without a `CancellationToken` type):

```rust
struct QueryCancel { engine: Weak<DuckDBEngine>, armed: bool }
impl QueryCancel { fn disarm(&mut self) { self.armed = false } }
impl Drop for QueryCancel {
    fn drop(&mut self) {
        if self.armed { if let Some(e) = self.engine.upgrade() { e.interrupt(); } }
    }
}
```

Normal completion calls `disarm()` before the guard drops. A new Run replaces the stored guard (the old one drops → interrupts any straggler). Safe because the Mutex-serialized connection runs one query at a time, so `interrupt()` targets exactly the running query.

### 3.1 Statement splitter (`query/statement.rs`, pure)

- `split_statements(sql) -> Vec<Span>` — scan chars tracking in-single-quote / in-double-quote / `--` line-comment / `/* */` block-comment state; `;` outside all states is a boundary.
- `statement_at(sql, cursor) -> Span` — the segment containing the cursor offset (trailing segment if cursor is past the last `;`).
- `classify(stmt) -> ResultKind` — leading keyword lookup after stripping leading whitespace/comments.
- **Known-naive** on dollar-quoting (`$tag$ … $tag$`) and nested block comments — acceptable for P5a, hardened later. Unit-tested across quote/comment/multi-statement cases.

## 4. Persistence (session.json v4 → v5)

- Bump `SESSION_SCHEMA_VERSION` 4 → 5.
- `SessionState` gains `sql_tabs: Vec<SqlTabState>` + `active_sql_tab: Option<usize>`, both `#[serde(default)]`. `SqlTabState = { id: Uuid, title: String, sql: String }` — buffer text + title only.
- **Migration v4→v5** is purely additive: existing `session.json` deserializes with empty `sql_tabs` / `None`. Reuses the existing migration ladder + forward-incompat guard. Round-trip test: a v4 fixture upgrades clean.
- **Write cadence** — persist on Run, tab switch, tab close, editor blur, and window close. **Not per keystroke** (dirty-flag + the existing durable atomic-write path; no fsync-per-char).

Note: the existing `SessionState.tabs` are **table tabs** (each views a DuckDB `table_name` + transform stack). SQL console tabs are a distinct, window-scoped concept → a separate top-level `sql_tabs` vec, not the `tabs` vec and not `Tab.extra`.

## 5. Components & files

**New:**
- `crates/dat0-app/src/query/mod.rs` — `QueryModel`, `SqlTab`, `ResultTarget`, `ExecState`, `QueryCancel`.
- `crates/dat0-app/src/query/statement.rs` — pure splitter / `statement_at` / `classify` (§3.1).
- `crates/dat0-app/src/query/highlight.rs` — `register_sql_language()` (`tree_sitter_sequel` → `LanguageRegistry::register("sql", …)`), called once at boot. T0-gated.
- `crates/dat0-app/src/view/sql_console.rs` — `SqlConsole` view (§2.2).
- `crates/dat0-app/tests/sql_console.rs` — splitter/at-cursor/classify; v4→v5 migration round-trip; result-kind routing; cancel-guard arm/disarm.

**Changed:**
- `crates/dat0-app/src/window.rs` (`WorkspaceShell`) — add `query_model`; console visibility + height; grid render branch (table tab vs transient result view); mount `SqlConsole`; keybinds `Cmd/Ctrl+Enter`, `Cmd+.`, console toggle, new/close tab; wire run → spawn → dispatcher → model.
- `crates/dat0-app/src/session/mod.rs` + `crates/dat0-app/src/session/migrate.rs` — v5 fields + migration + persist-cadence hookup.
- `crates/dat0-app/src/actions/*` + keymap — register `sql.run`, `sql.cancel`, `sql.new_tab`, `sql.close_tab`, `console.toggle` (command-**palette** registration deferred to P5b; actions + keybinds land here).
- `crates/dat0-app/Cargo.toml` — add `tree-sitter-sequel` as a **direct dep** (not the gpui-component `tree-sitter-languages` feature).
- `crates/dat0-i18n/src/strings/en.json` — console / run / cancel / status / error / tab-title strings.

**Engine crate (`dat0-engine`): no trait change.** Reuses `create_or_replace_view` / `drop_view` / `execute` / `execute_paged` / `interrupt` — all present in `trait_def.rs` today.

## 6. Testing

**TDD (per repo norm):**
- **Pure unit** — statement splitter (quotes/comments/multi-statement), `statement_at(cursor)`, `classify`, `QueryCancel` arm/disarm, session v4→v5 migration round-trip. Headless, fast.
- **Engine integration** — run SELECT → `create_or_replace_view` → `execute_paged` returns rows; run DDL → `execute` status; run bad SQL → `EngineError` surfaced; `interrupt()` mid-query → `Interrupted`. Engine-level, no GPUI.
- **GPUI render** — left to manual UAT, matching prior phases (overlay/mount paths resist headless testing).

**Manual UAT (owed; joins the P4b/P4c backlog):** type SQL → highlight visible (or plain, per T0 outcome); `Cmd+Enter` runs statement-under-cursor; result in grid; split-button → results pane; `Cmd+.` cancels a long query (e.g. a cross-join); error strip on bad SQL; tabs persist across relaunch.

## 7. Spec §21.2 P5 exit criteria → slice mapping

| Exit criterion | Slice |
|---|---|
| Editor opens, syntax highlights correctly across DuckDB SQL surface | **P5a** (T0-gated; fallback moves *highlight* to P5b, editor stays P5a) |
| Autocomplete suggests tables + columns from active workspace | P5b |
| Run query produces grid result; cancel button stops execution | **P5a** |
| Query history preserved across reload | P5b |
| "Save as Table" creates a derived table with parent lineage | P5b |
| Multi-tab persists across reload | **P5a** |

## 8. T0 spike (runs before plan tasks)

1. **Highlight registry path (#1 risk).** Does `LanguageRegistry::register("sql", LanguageConfig{ tree_sitter_sequel::LANGUAGE, HIGHLIGHTS_QUERY, … })` actually color an `InputState::code_editor("sql")`? Construct the `LanguageConfig`, mount a CodeEditor, confirm tokens are themed. **If NO →** decision 7 fallback (plain editor; defer highlight to P5b). **Do not** enable the bundle.
2. **`CompletionProvider` seam (forward-looking, cheap).** Confirm `InputState` exposes a way to attach `Rc<dyn CompletionProvider>` (it holds `lsp.completion_provider: Option<Rc<dyn CompletionProvider>>`). Verifies P5b is unblocked without a real LSP.
3. **Result-view → grid bind.** Confirm the existing grid bind/paging path can point at an arbitrary view name (`__dat0_qr_*`) and discover columns/types from it.
4. **`MainThreadDispatcher` reuse.** Confirm the P3b bridge is reachable from the console run path to marshal completion.
5. **`InputState` ergonomics.** Read/replace buffer text, get selection range + cursor offset, one `InputState` entity per tab (mount cost).

## 9. Register updates (apply at plan/spec time)

- **D-008** — record resolution: **token-free** (drop-guard + `interrupt()`), **no trait amendment**. Mark `in-progress` (P5a); close in the P5a retro with the closing commit.
- **D-007** — stays **open**; retarget note: P5 → **P5c** (credential-gated MotherDuck slice).
- **PD-021** — stays **open**; note P5a uses inline errors, so it does not depend on the banner queue.
- **Constraint note** — do **not** enable gpui-component `tree-sitter-languages` (28-grammar bundle); SQL highlight is a single direct `tree-sitter-sequel` dep + runtime registration. Add to `docs/internal/gpui-component-api-notes.md`.
- New PDs opened during execution: TBD.

## 10. Risks

- **Highlight registry path unproven** — T0 #1; fallback defined (decision 7), so it cannot block the slice, only descope highlight.
- **Naive `;`-splitter** — dollar-quote / nested block-comment edge cases mis-split; accepted for P5a, hardened later.
- **Classify heuristic** — `CREATE TABLE … AS SELECT` is DDL → EXEC path (status line, no result grid), which is correct; `EXPLAIN`/`SUMMARIZE` return rows → VIEW path, correct. Leading-comment stripping must precede keyword match.
- **Dual result targets** (main grid + pane) double the render wiring — flagged as the **first P5a trim** if T0 estimates run hot (drop the pane to P5b, keep main-grid default).
- **Temp-view name collisions** across tabs/windows — namespaced by window + tab id.
