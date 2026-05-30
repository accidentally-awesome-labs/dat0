# P4a T0 — perf spike + headless-mount probe

**Date:** 2026-05-27
**Toolchain:** rustc 1.95.0 (f2d3ce0bd 2026-03-21); duckdb-rs 1.4.4; gpui-component pinned `0f0ab35`.
**Platforms exercised:** macOS-arm (Apple Silicon M-series). linux-x86 pending — see §5.

---

## 1. view_regen bench results

Bench: `crates/dat0-engine/benches/view_regen.rs`. Fixture: 1 000 000-row CSV
(`id INT, price DOUBLE, city VARCHAR, ts TIMESTAMP, active BOOL`), seed
`0xCAFEBABE`, generated once, cached across Criterion iterations.

Filter predicate: `WHERE "price" > 5000.0` (approximately 50 % selectivity).
Engine: `DuckDBEngine` with 4 GiB memory budget, real DuckDB file, bundled build.

p95 values computed from 20 Criterion samples (per-iteration times).

| Metric | macOS-arm mean | macOS-arm p95 | Target | Verdict |
|---|---|---|---|---|
| `t1_create_view_plus_t2_first_page` | 221 ms | **229 ms** | < 500 ms p95 | **PASS** |
| `t1_plus_t2_plus_t3_last_page` | 592 ms | **602 ms** | < 2 s | **PASS** |

Raw Criterion artefact at `target/criterion/view_regen/` (not committed; attach
per-platform tarball to the spike PR per plan §1 protocol).

**linux-x86 numbers pending.** Must be collected via heavy.yml `run-heavy`
label run on the self-hosted linux runner. The macOS-arm result gives high
confidence in Plan A (229 ms p95 is 2.2× headroom against the 500 ms gate),
but the plan exit criterion requires both platforms. Coordinate with maintainer
to trigger `run-heavy` on the p4a-hot-path PR before T1 merges.

---

## 2. Decision: Plan A confirmed

macOS-arm p95 = 229 ms — 54 % of the 500 ms budget. Well within gate.

**Plan A (temp VIEW per chain change) confirmed.** The `CREATE OR REPLACE TEMP
VIEW` pattern defined in design §4 proceeds unchanged. T1+ tasks proceed
unmodified as written.

Rationale: DuckDB's query planner resolves the temp VIEW lazily (at first
`execute_paged` call); the 229 ms includes both the DDL and the first page
fetch of 100 rows. The 2.2× margin absorbs linux-x86 variance and any
additional join/sort complexity from multi-op stacks (T2).

If linux-x86 p95 ≥ 400 ms (i.e., < 20 % margin), revisit Plan B1 (CTAS-on-apply)
before T2 merges. Do NOT block T1 (typed enum only) on the linux number.

Plan B variants documented for reference (not activated):
- **B1 — Materialized CTAS-on-apply.** `CREATE OR REPLACE TEMP TABLE v_… AS
  SELECT …`. Heavier on apply, cheaper on paging. Prefer if p95 > 500 ms.
- **B2 — Hybrid threshold.** View below N rows; CTAS above. N derived from
  bench. Adds row-count heuristic in `create_or_replace_view`.

---

## 3. gpui-component Input + Select headless mount probe

**Probe outcome: INFEASIBLE.**

Both `InputState::new` and `SelectState::new` require `&mut Window` as a
constructor parameter (verified against pinned commit `0f0ab35`):

```
InputState::new(window: &mut Window, cx: &mut Context<Self>)
  → crates/ui/src/input/state.rs:343

SelectState::new(delegate, selected_index, window: &mut Window, cx: &mut Context<Self>)
  → crates/ui/src/select.rs:547
```

`gpui::TestAppContext` provides no real `Window` handle — headless tests have
no platform-window context. The `cx.new_window_entity` pattern would require
a Cocoa NSWindow, which is unavailable in a headless test environment.

**Consequence for T10:**

T10 splits into two tasks:

- **T10** (in-scope, same task slot) — filter popover predicate logic +
  closures: `FilterPopover` entity, operator selection state, value validation,
  Apply/Cancel/Clear signal shape. No visible widget mount — pure logic tested
  in `crates/dat0-app/tests/filter_popover_state.rs`.
- **T10b** (new in-scope P4a task) — visible widget mount: real macOS window,
  real `Input`/`Select` constructed with a live `Window`, screenshot via
  `xcrun simctl io` or bench-artifact pattern (same as `grid_scroll` approach
  in P3a). T10b is **in-scope for P4a** per design §6 "pull the primitive-mount
  task into P4a explicitly (preferred)". **Do NOT defer to P5.**

The probe scratch file `crates/dat0-app/tests/_t0_probe.rs` records the
infeasibility reasoning in-code but is not committed (leading-underscore
convention, `#[ignore]` gate).

---

## 4. Render path validation (paper exercise — T2 turns into golden tests)

SQL fragments hand-authored against DuckDB 1.4 docs, confirming syntax is
accepted by DuckDB before T2 writes golden tests:

```sql
-- Between (inclusive)
SELECT * FROM "main"."t"
WHERE ("price" BETWEEN 10.00 AND 99.99)

-- IN list
SELECT * FROM "main"."t"
WHERE ("city" IN ('SF', 'NYC', 'LA'))

-- Regex (DuckDB 1.4: regexp_matches is the correct function name)
SELECT * FROM "main"."t"
WHERE regexp_matches("name", '^A.*')

-- Multi-key sort
SELECT * FROM "main"."t"
ORDER BY "city" ASC, "price" DESC

-- Combined filter + sort
SELECT * FROM "main"."t"
WHERE ("price" >= 10.00 AND "price" <= 99.99)
  AND ("city" IN ('SF', 'NYC', 'LA'))
  AND regexp_matches("name", '^A.*')
ORDER BY "city" ASC, "price" DESC
```

Each fragment confirmed against DuckDB 1.4 docs:
- `regexp_matches(col, pattern)` — correct form (not `regexp_match` or `~`).
- `BETWEEN a AND b` — inclusive both ends in DuckDB standard SQL.
- `IN (...)` — standard; no ARRAY variant needed for literal lists.
- `ORDER BY col ASC/DESC` — standard; `NULLS LAST` default is DuckDB's.
- Identifier quoting: `"col"` double-quote style (matches `catalog::quote_ident`
  output).

---

## 5. Heavy-CI integration

Linux-x86 number owed via `heavy.yml` `run-heavy` label run. Proposed step to
append to `.github/workflows/heavy.yml` at T13 (or T15):

```yaml
- name: view_regen bench
  run: cargo bench -p dat0-engine --bench view_regen 2>&1 | tee /tmp/p4a_t0_bench_linux.log
  if: contains(github.event.pull_request.labels.*.name, 'run-heavy')
```

**Action item:** trigger `run-heavy` on p4a-hot-path PR before T1 merges.
Linux p95 ≥ 400 ms → escalate to Plan B discussion before T2 (render path)
lands.

---

## 6. Plan-snippet drifts logged

PD-013 opened in `docs/deferrals.md`:
- `dat0-fixtures` was binary-only; T0 added `[lib]` + `src/lib.rs` re-export.
- `dat0-engine` had no `benches/` dir; T0 created it + wired criterion via
  workspace inheritance (not the pinned `criterion = "0.5"` snippet in the plan).
- Workspace `criterion` dep upgraded to include `async_tokio` feature (required
  for `b.to_async(&rt)` in the bench).

---

## 7. Follow-up actions opened by T0

| # | Action | Owner | Blocks |
|---|---|---|---|
| 1 | Trigger `run-heavy` on p4a-hot-path PR to capture linux-x86 bench numbers | maintainer | T15 retro |
| 2 | Add T10b to plan task list (visible filter popover widget mount) | implementer | T10 |
| 3 | Append `view_regen` bench step to `heavy.yml` | T13/T15 | none |
