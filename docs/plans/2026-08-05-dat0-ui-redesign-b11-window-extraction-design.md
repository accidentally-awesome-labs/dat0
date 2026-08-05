# Slice B11 — `window.rs` extraction (design)

**Status:** approved at brainstorm 2026-08-05 (11 owner decisions, all on the recommended path).
**Branch:** `feat/ui-redesign-b11-window-extraction` off main `68f01c3`.
**Master plan row:** §6 B11, size M.
**Kind:** pure refactor. No UI change, no behaviour change, no schema change, no new dependency.

---

## 1. What the master plan promised, and what the tree actually holds

The master plan's B11 row budgets the extraction as:

> test accessors ~390 · `cfg(test)` ~205 · AI ~480 · SQL ~486 · dock ~600 · charts ~350 ·
> connections/MD ~267 · export+drop ~340

Sum ≈ 3,100 against a file of 8,672 — which is why the row concludes that `<5k` "requires taking
essentially ALL of it".

**That budget is stale in exactly the way B10 found four of its own five clauses stale.** It
inventories only `impl WorkspaceShell` methods. It misses the entire free-function half of the file:

| Missed region | Lines |
|---|---|
| workspace lifecycle free fns (L68–470) | 403 |
| package flows free fns (L471–889) | 419 |
| misc + chart-axis free helpers (L890–1121) | 232 |
| window spawn + orphan/recovery scan (L1122–1349) | 228 |
| `register_menu_action_handlers` + sql flush (L1350–1703) | 354 |
| `run_app` (L1704–2160) | 457 |
| **total missed** | **~2,093** |

Real extractable budget is therefore **~6,100 of 8,672**, not ~3,100. `<5k` is not a scrape. The
target stops being the binding constraint and **cohesion sets the boundaries instead** — which is
what this design does.

Measured region map of `window.rs` at `68f01c3` (8,672 lines, 229 fns):

| Region | Lines | Size |
|---|---|---|
| module doc + imports | 1–67 | 67 |
| free fns: workspace lifecycle | 68–470 | 403 |
| free fns: package flows | 471–889 | 419 |
| free fns: misc + chart-axis helpers | 890–1121 | 232 |
| free fns: window spawn + orphan/recovery scan | 1122–1349 | 228 |
| `register_menu_action_handlers` + `flush_focused_workspace_sql` | 1350–1703 | 354 |
| `run_app` | 1704–2160 | 457 |
| `NamePromptIntent` / `MountedModal` / `push_modal` | 2111–2178 | 68 |
| `pub struct WorkspaceShell` (fields only) | 2179–2539 | 361 |
| `impl WorkspaceShell` #1 | 2540–6653 | 4,114 |
| `AiEntryKind` + 3 free helpers | 6654–6718 | 65 |
| `impl WorkspaceShell` #2 (dock + panel bodies) | 6704–7443 | 740 |
| `impl Render for WorkspaceShell` | 7444–8075 | 632 |
| `cfg(a11y-capture)` accessors (~40 fns) | 8077–8461 | 385 |
| `cfg(test)` unit-test mods ×2 | 8463–8672 | 209 |

The three largest single functions are `render` (632), `run_app` (457) and
`register_menu_action_handlers` (314); the largest single *item* is the `WorkspaceShell` struct at
361 lines of fields.

---

## 2. The lever, stated precisely

The kickoff block says converting `window.rs` → `window/mod.rs` + children "needs ZERO visibility
changes because Rust makes private items visible in the defining module AND ALL DESCENDANTS."

**Half of that is true, and the false half is the useful half.**

Rust visibility flows *downward*. A private item declared in `window` (i.e. `mod.rs`) is visible in
`window::ai`. So:

- ✅ `WorkspaceShell`'s private fields genuinely need **no** visibility change. Every child module
  can read and write them directly. This is the claim's real content and it holds.
- ❌ An item declared *in* `window::ai` and left private is **not** visible in `window`. The parent
  is not a descendant of the child. So every moved method that `mod.rs` or a sibling still calls
  needs `pub(super)` (or wider).

This is not a hidden cost. It is **compiler-enumerated**: move the code, compile, and `E0624` names
every cross-module call, one error per site. That is a complete and trustworthy inventory of the
shell's internal call surface — something no grep can produce. It is the same technique A6 used when
adding `ring: Hsla` turned a guessed "~15 focus_stop sites" into an exact 29 + 4.

**T0 probe 1 proves this mechanically before any bulk move**, rather than trusting the paragraph
above.

### External surface

Other files and the 118 test binaries reach into this module by path. Measured references:

`WorkspaceShell` ×51 · `LeftPanel` ×11 · `run_app` · `register_menu_action_handlers` · `spawn_window`
· `spawn_recovered_scratch` · `spawn_workspace_window` · `orphan_scan_emit` · `recovery_scan_emit` ·
`count_orphan_scratch` · `open_demo_workspace` · `open_workspace_flow` · `save_workspace_flow` ·
`dispatch_live_refresh` · `now_epoch_secs`

A file→directory conversion does not change the module path, so `crate::window::X` stays valid for
the two types. The 13 free functions move into children and are restored by **explicit `pub use`
re-exports** in `mod.rs` (explicit list, not a glob). Net effect: **zero call-site edits anywhere in
the tree** — no test file, no other `src/` module changes.

---

## 3. The split

14 child modules. Every one of the 229 functions is assigned; the sizes below are computed from the
tree, not estimated.

| Module | ~Lines | Contents |
|---|---|---|
| `mod.rs` | **900** | `WorkspaceShell` struct (361) · `new` (102) · grid/view core wiring (436) · `mod` decls · `pub use` re-exports · new directory-map module doc |
| `boot.rs` | 900 | `run_app` (457) · `register_menu_action_handlers` (314) · `flush_focused_workspace_sql` · `open_window_view` · `spawn_window` · `paths_from_open_urls` + `open_urls_decode_to_local_paths` test · `focused_session_arc` · **the existing 67-line module doc** · `DOCS_URL` / `DISCORD_URL` |
| `render.rs` | 778 | `impl Render for WorkspaceShell` (632) · `render_grid_body` (123) · `bounding_rect` · `grid_focus_handle` |
| `dock.rs` | 672 | `ensure_dock_area` (256) · `sync_left_dock` / `sync_right_dock` · 6 `render_*_body` · rail cursor/activate/click · `*_visible` predicates · `open_left_panel` / `activate_left_panel` / `on_left_panel_shown` · B9 dock-layout persist |
| `sql.rs` | 644 | `mount_sql_console` · `toggle_sql_console` · `refresh_completion_snapshot` · `on_sql_console_event` · `spawn_sql_run` · `finish_sql_run` · `save_console_as_table` · query library save/delete · `classify_run_err` · `format_exec_status` · `now_unix_millis` · `bare_table_name` + its unit test |
| `ai.rs` | 615 | `AiEntryKind` · `handle_ai_panel_event` (126) · `spawn_ai_test` / `spawn_ai_nl2sql` / `spawn_ai_explain` · `open_ai_entry_prompt` · hydrate / settings / privacy banner |
| `charts.rs` | 552 | 5 free axis helpers (`axis_field`, `set_axis_field`, `axis_role_key`, `axis_required`, `cycle_axis`) + their 4 unit tests · `toggle_chart_panel` · `run_plot_query` · `render_chart_toolbar` · `save_named_chart` · `open_saved_chart` · `show_chart_with_spec` · `export_chart` |
| `package_ops.rs` | 547 | `.dat0` export / open / unpack / replay · `open_demo_workspace` · `orphan_scan_emit` · `recovery_scan_emit` · `count_orphan_scratch` · `spawn_recovered_scratch` |
| `workspace_ops.rs` | 476 | open / save / promote / recents · `promote_focused_into` (142) · `load_workspace_settings` · `configured_memory_budget` · `now_epoch_secs` |
| `catalog_inspector.rs` | 471 | `refresh_catalog` · `catalog_nav_key` · `toggle_catalog_parent` · `open_table_tab` · `load_inspector_profile` · `load_column_extras` (94) · `dispatch_extra` · `recompute_lineage` · `inspector_projection` |
| `data_io.rs` | 459 | `open_export_dialog` · `route_export_event` · `run_export` · save-view-as-table · `route_drop_outcomes` · `open_sample_kind` · `open_recent_entry` · `open_file_picker` |
| `modals.rs` | 438 | `NamePromptIntent` · `MountedModal` · `push_modal` · name prompt (B1/B2) · saved picker · command palette (B4) · `restore_modal_focus` · `open_modal_count` |
| `test_support.rs` | 385 | the ~40 `#[cfg(feature = "a11y-capture")]` accessors, kept as one block |
| `live_refresh.rs` | 364 | `dispatch_live_refresh` · `active_source_path` · `retarget_source_watch` · `on_source_changed` · `run_refresh` · `perform_reimport` · `apply_refresh_replay` · `partition_replay_on_drift` · `on_table_mutated_structural` · the `live_refresh_tests` mod |
| `connections.rs` | 344 | `handle_connections_event` · `disconnect_md` · `detach_attachment` · `spawn_md_connect` · `spawn_md_test` · `reconnect_persisted_md` · `open_md_token_prompt` |

### Why `mod.rs` lands at ~900, not the ~2,500 estimated at brainstorm

Assigning all 229 functions rather than sampling surfaced two clusters the option list did not carry
— `live_refresh` and `catalog_inspector`, 835 lines between them — and `render` and `modals` took
more than projected. Owner approved both additions (14 modules, not 12).

### What deliberately stays in `mod.rs`

The ~334 lines of grid/view core wiring — `set_data_source`, `apply_view_change`,
`prefetch_visible_rows`, `prefetch_rows_for`, `spawn_rebind`, `on_sort_zone_click`,
`on_funnel_click`, `route_filter_outcome`, `pipeline_jump_to`, `pipeline_remove_at`,
`header_rename_for` — plus the small accessors (`engine`, `session_arc`, `view_model_mut`,
`base_table`) and the `new` constructor (102).

(`maybe_prompt_save_workspace` goes to `workspace_ops.rs`; `column_name` and `refresh_column_view`
go to `catalog_inspector.rs`, beside the profiling code that drives them.)

These are the shell's own data binding — the methods connecting `WorkspaceShell` to its `ViewModel`
and grid delegate. Keeping them beside the struct and constructor makes `mod.rs` read as *what the
shell is and what it holds*. Owner chose this over a 15th `grid_view.rs` (which would have put the
shell's most central behaviour one hop from its definition).

### Total line count grows ~200

14 files each carry their own `use` block, ~15 lines apiece. Stated plainly rather than hidden: the
split trades a little import duplication for locality. The `<5k` target is about `mod.rs`, and it is
met more than five times over.

---

## 4. Two hazards the kickoff block flagged — both handled structurally

1. **`DOCS_URL` / `DISCORD_URL`.** The interim consts from the 2026-07-21 menu hotfix must survive;
   P11b/P11c swaps them before release. They move to `boot.rs`, which is where
   `register_menu_action_handlers` — their only consumer — now lives. Adjacency replaces vigilance.
   The pre-release ops note referencing "window.rs" is updated to `window/boot.rs`.
2. **`clippy::items-after-test-module`** (warn-by-default, an error under `-D warnings`) currently
   forces the `a11y-capture` accessor block to sit ahead of the `#[cfg(test)] mod`s, maintained by a
   hand-written comment. After the split, the accessors are their own file and each unit-test mod is
   the last item in its own file. **The ordering constraint becomes structurally unviolatable** and
   the comment documenting it is no longer load-bearing.

---

## 5. Coverage — how a pure refactor is proven neutral

Three independent gates. Each catches something the others cannot.

### 5.1 The standing suite (catches behaviour change)

`-p dat0-app` × {plain, `a11y-capture`, `a11y-capture,gallery`} — **118 binaries** as of B10, all
green, plus `cargo fmt --check` and `clippy --workspace --all-targets -D warnings` exit 0.

Blind spot: a line that no test exercises.

### 5.2 The body digest (catches edits in transit)

The suite cannot see an accidental edit inside a moved function body on an uncovered line, and this
diff moves ~6,000 lines. So: a one-off script parses `window.rs` at the merge-base and
`window/**/*.rs` after, extracts every function as `(name, normalized body)` — normalizing away
leading whitespace, visibility keywords, and `use` lines — and asserts the two multisets are **equal**.

Run at T0 (against a trivial probe move) and again at the end of the branch. **Not committed**: after
B11 there is no "before" side left to compare against, so a committed version could only freeze a
hash of today's bodies and would then redden on every legitimate future edit. Recorded as-built in
§10 instead. Owner-approved.

Non-vacuity is probe 4: perturb one moved body, confirm red, revert, `touch`, confirm green. (The
`touch` is A6's stale-binary trap — a correctly-reverted source reported RED until the file's mtime
was bumped.)

### 5.3 The boot check (catches wiring/ordering change)

`cargo build --bin dat0`, then boot the real binary **with a seeded `[ui.dock_layout]`** under a
scratch `DAT0_CONFIG_DIR`, and diff the log against a `main` build booted from the same seed
(normalising timestamps / UUIDs / durations / config path).

The seed is not optional: B9 established that **a fresh-session boot exercises none of the restore
path**, so an unseeded boot check is vacuous. This slice moves `ensure_dock_area`, the dock persist
methods and `run_app` itself, so the restore path is exactly what needs exercising.

### 5.4 Anti-regrowth ratchet (catches the failure mode that produced this slice)

`window.rs` reached 8,672 lines because nothing ever objected. A new test carries a per-file
max-line table over `src/window/`, failing **both** directions:

- **over** — a file grew past its allowance (new code piled into an existing module);
- **under** — a file shrank but its number was left high, silently re-opening the budget.

This is the two-sided form A4 proved for the style-lint colour ratchet, and `ratchet_report()` in
`tests/style_lint.rs` is a working precedent to copy. Owner chose this over a single global cap on
`mod.rs`, which would say nothing about a child module becoming the new dumping ground.

### 5.5 Not gated, deliberately

`grid_scroll` bench. The master plan does not bench-gate B11, and B5 settled that the bench is a pure
`render_cell` watchdog that never touches the table delegate or a `Window` — it cannot see a mounting
change. Step-verify it post-merge and `gh run download` the artifact for series continuity, but **do
not read meaning into the number**.

`cargo test --workspace` and `cargo bench` remain unrunnable on this machine (macOS 27 / Xcode 26.6
vs vendored DuckDB Thrift). CI is the only place they run.

---

## 6. T0 hard gate

Four probes, all before any bulk move. Owner approved the full set.

1. **Visibility.** Move one private method into a child `impl WorkspaceShell` block, confirm `mod.rs`
   fails with `E0624`, confirm `pub(super)` fixes it, and confirm the error set is a complete
   inventory (no silent fallback to a different resolution). This is the design's load-bearing
   assumption; if the compiler behaves otherwise the split strategy changes.
2. **`impl Render` from a child module.** Trait impls are crate-coherent, not module-coherent, so
   this should compile — but B7's design also had a "settled" central choice that turned out
   unbuildable inside gpui's entity-leasing rules. Prove it.
3. **`#[cfg(feature = "a11y-capture")] impl` from a child module**, and confirm the 118 test binaries
   still resolve `shell.x_for_test()` unchanged.
4. **Digest non-vacuity**, per §5.2.

**STOP clause:** if probe 1 or 2 fails, stop and re-report before writing any further task. The split
shape is downstream of both.

---

## 7. Task shape

One commit per module, each fully green (`fmt` + `clippy --workspace --all-targets -D warnings` +
`-p dat0-app` suite). ~16 commits. Owner-approved over end-only gating: every commit stays
bisectable, and a mid-branch break never becomes a 6,000-line search. This is how B5–B10 were
executed and is why their whole-branch reviews stayed readable.

Order is **lowest-risk first**, so the mechanics are proven on free functions before any `impl` block
is split:

| # | Commit | Risk |
|---|---|---|
| T0 | 4 probes + digest script | gate |
| T1 | `window.rs` → `window/mod.rs` (pure rename, no content move) + directory-map doc | none |
| T2 | `boot.rs` — free fns + the module doc + the two URL consts | low |
| T3 | `workspace_ops.rs` — free fns | low |
| T4 | `package_ops.rs` — free fns | low |
| T5 | `test_support.rs` — `cfg(a11y-capture)` block | low |
| T6 | `modals.rs` — first `impl` split; enums + `push_modal` | medium |
| T7 | `charts.rs` — free helpers + methods + 4 unit tests | medium |
| T8 | `live_refresh.rs` — methods + `live_refresh_tests` mod | medium |
| T9 | `catalog_inspector.rs` | medium |
| T10 | `connections.rs` | medium |
| T11 | `ai.rs` | medium |
| T12 | `sql.rs` | medium |
| T13 | `data_io.rs` | medium |
| T14 | `dock.rs` | medium |
| T15 | `render.rs` — `impl Render` | medium |
| T16 | ratchet test + final digest + boot check + docs | gate |

T1 as a standalone rename commit is deliberate: it isolates the one operation git might otherwise
render as a delete-plus-add, keeping every later commit a readable move.

---

## 8. Decision register

| # | Decision | Chosen | Over | Because |
|---|---|---|---|---|
| 1 | Granularity | cohesion-first, 14 modules | minimum-to-target (5–6); two PRs | budget is ~6,100 not ~3,100, so the target need not drive the boundaries; the 4,114-line impl slab is the actual problem and minimum-to-target leaves it intact |
| 2 | `impl Render` | move to `render.rs` | keep in `mod.rs` | self-contained trait impl, largest contiguous block after the slab; `mod.rs` becomes type + ctor + wiring |
| 3 | Test code | accessors one file, unit tests per domain | both split; both stay | accessors are one API surface used by 118 binaries and stay auditable in one place; unit tests belong beside their subject |
| 4 | No-drift gate | one-off digest at T0 + final | committed permanent test; suite-only | suite is blind to uncovered edited lines; a committed version has no "before" side after B11 |
| 5 | `modals.rs` | add it | leave in `mod.rs` | B1/B2 ModalHost + B4 palette = two slices' work, currently scattered |
| 6 | Commits | one per module, each green | end-only; 4 risk tiers | bisectability; per-topic review readability |
| 7 | Ratchet | per-file two-sided line ratchet | global `mod.rs` cap; none | a global cap says nothing about a child becoming the new dumping ground |
| 8 | T0 scope | all four probes | 1+4; 4 only | probes 2 and 3 are "settled Rust" — B7's central choice was also settled until gpui's leasing rules said otherwise |
| 9 | Extra clusters | add `live_refresh` + `catalog_inspector` | fold into `data_io` / `dock` | folding recreates the grab-bag the slice exists to eliminate |
| 10 | Grid wiring | keep in `mod.rs` | extract `grid_view.rs` | it is the shell's own data binding; belongs beside the struct |
| 11 | Module doc | move to `boot.rs` + new map doc in `mod.rs` | keep; move without replacement | the boot narrative follows its code; a 15-file directory needs an orientation doc that does not exist today |

---

## 9. Owed human glance

**None from this slice.** B11 changes no pixels — no token, no layout, no label, no a11y node. The
capture tree is byte-identical by construction.

The standing **B4→B10 combined pass remains owed and is due**; it can run in parallel with this
branch precisely because B11 cannot affect it. Top of that list (from B10): file drop in all 3 themes
— **HC must be YELLOW, not blue** — and the −14% re-space on the four `Sp` surfaces.

---

## 10. As-built

### T0 — hard gate (2026-08-05)

Toolchain `rustc 1.97.0 (2d8144b78 2026-07-07)`, as pinned. Baseline `cargo check -p dat0-app`
green in 1m16s before any probe, so the known macOS 27 / Xcode 26.6 blocker does not affect this
crate's check path.

**Probe 1 — visibility. PASS, both halves, and §2 is now measured rather than argued.**

In miniature (`rustc --edition 2021`): a child module reading the parent's private field compiled;
the parent calling the child's private method produced

```
error[E0624]: method `hidden` is private
```

with no error on the field access. Changing `fn hidden` to `pub(super) fn hidden` compiled and ran.

**Probe 1b — the same inside `dat0-app`**, with the 361-field struct and gpui traits in scope. A
child module reading `self.tour_auto_shown` compiled (only `never used`). Adding a parent-side call
produced exactly one `error[E0624]: method 'probe_reads_private_field' is private`; `pub(super)`
cleared it. Reverted and `touch`ed; `diff` against `HEAD` clean.

**Probe 2 — `impl Render` from a child module. PASS.** Moved the 624-line
`impl Render for WorkspaceShell` block verbatim into `window/render.rs`. No coherence error at any
point — the only failures were missing imports, which is the expected and desired signal.

★ **This produced the slice's first real execution finding.** Supplying those imports as
`use super::*;` compiled *and* passed `cargo clippy -p dat0-app --all-targets` with no warning. The
mechanism is the same downward-visibility rule as the fields: a `use` declaration is an item, it is
private by default, and a private item is visible in its module **and all descendants** — so a child
inherits the parent's entire private import block through one glob. `wildcard_imports` is
pedantic-only and not enabled here. **The plan's original recipe step 5 — hand-curate an import
block per file — was unnecessary work and has been rewritten.**

**Probe 3 — `#[cfg(feature = "a11y-capture")] impl` from a child module. PASS.** The 392-line
accessor block moved to `window/test_support.rs` and
`cargo check -p dat0-app --features a11y-capture --all-targets` finished green in 51s. `--all-targets`
compiles the integration tests, so the 118 binaries resolve `shell.x_for_test()` unchanged through
the unmoved `crate::window::WorkspaceShell` path.

**Probe 4 — digest. PASS only after finding and fixing TWO real defects in the gate.**

The plan's authoring-time smoke test (identity green, in-place perturbation red) passed against a
digest that was badly broken. Running it across a **real move** exposed both:

1. **cwd-dependent paths.** `win.is_dir()` resolved against the caller's cwd, so from
   `crates/dat0-app/src` it took the wrong branch and crashed. Now resolves the repo root via
   `git rev-parse --show-toplevel`, and raises explicitly when neither form of the module exists.
2. ★★ **Bodies were bounded by "until the next `fn`"**, so the last function before any non-`fn`
   item silently swallowed that item. `bounding_rect` had absorbed
   `impl Render for WorkspaceShell {`; moving `render` out therefore reported **both** as `CHANGED`
   — two false positives on a completely correct move. Bodies are now bounded by **brace matching**,
   so a body depends only on itself.

Final behaviour, all four verified: identity green (229/229) · in-place edit red with the exact ±
pair · **legitimate move green** (`229 fns across 2 file(s)`, `DIGEST OK`) · **edit inside a moved
file red**, naming `CHANGED render (now in render.rs)` and the exact line · cwd-independent.

★★ **THE REUSABLE LESSON: a gate whose job is to tolerate movement cannot be validated by a probe
that does not move anything.** The in-place perturbation test is the obvious non-vacuity check, it
is what the plan originally specified, and it certifies a gate that would have emitted false
positives at every one of T2–T15 — at which point its output becomes noise to skim past, and the
slice silently loses its primary evidence. **Validate a gate against the transformation it exists to
watch, not against a convenient stand-in.** (Same family as B10's "a chained style read-back tests
the setter, not your value", and the standing rule to drive keyboard behaviour with
`simulate_keystrokes` rather than `dispatch_action`.)

**STOP clause not triggered.** All four probes pass; the split shape in §3 stands unchanged.

Probe state fully reverted after each probe; `git status` clean and digest identity re-confirmed
before T1.
