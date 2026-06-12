# dat0 P7c — Live data + recovery polish (design)

> **STATUS: BRAINSTORMED + APPROVED 2026-06-12** via `superpowers:brainstorming`.
> Supersedes the stub `docs/plans/2026-06-10-dat0-p7c-design.md` (which captured the
> scope deferred *from* P7a/P7b as open questions). All decisions below are **locked**.
> Next step: `superpowers:writing-plans` → subagent-driven execution.
>
> **P7 split (set in P7a):** P7a = workspace core (MERGED, PR #18 `0c0dca8`).
> P7b = concurrency & sync-drive safety (MERGED, PR #19 `d75ca4c`).
> **P7c** (this doc) = live data + recovery polish. **Closes out P7.**

## Goal

Two independent subsystems, shipped as the closing slice of P7:

1. **Live-data refresh** — watch each open tab's on-disk source file; on external change,
   surface a non-blocking refresh affordance that re-imports the table and replays the
   tab's (structural) transform chain.
2. **Recovery review** — finish the `recovery_panel` Sheet UI (currently a tracing stub),
   surfacing both orphaned scratch sessions and interrupted workspace promotions for
   review (Open / Resume / Discard).

A required enabler for (1) closes **D-021** (banners render title+body only — no action
buttons): the refresh affordance is a banner with a clickable button.

## Locked decisions

| # | Decision | Rationale |
|---|----------|-----------|
| **D1** | **Re-import only** — no live-view (`read_csv` VIEW) import mode this slice. | Spec exit criterion already commits to the re-import model ("watcher detects change → prompts refresh → replays lineage"). A live-view mode forks the table model (views can't carry base-row edits or the `__dat0_rowid` surrogate) and touches import UX + P4b edit path + profiling. Smallest blast radius; live-view = future slice. |
| **D2** | **Banner with inline Refresh button** (closes D-021). | Matches the stub's "non-blocking affordance" intent. A blocking Dialog interrupts on every external save. Building the banner action-button closes a long-standing app-wide UI gap (D-021). |
| **D3** | **Smart-split edit-loss** — replay structural ops, confirm-discard rowid-keyed ops. | Re-CTAS regenerates `__dat0_rowid`, so `Edit`/`RowDelete` (rowid-keyed) can't safely carry over, but `Filter`/`Sort`/`Reorder`/`Rename`/`DeleteColumn` (column-keyed) replay cleanly. Preserves safe work; never silently misapplies an edit to a wrong row. |
| **D4** | **Recovery Sheet covers orphan-scratch + incomplete-workspace.** | Fully satisfies the spec criterion ("force-quit during workspace work → recovers from `.dat0/`") and gives the ONLY UI for the interrupted-promote state P7a can produce. |
| **D5** | **Refresh = watched tab only** — no cascade to downstream derived tables. | Satisfies the exit criterion ("replays lineage" = the tab's own transform chain). Cross-table cascade (re-materialize the P6b dependency closure in topological order) is large with many partial-failure modes → its own future slice. |
| **D6** | **Always-prompt; no auto-refresh toggle in v1.** | The banner-button is low-friction enough; auto-refresh is additive and consistent-to-defer given D1 chose explicit/snapshot semantics. |
| **D7** | **Watch in both scratch + workspace modes** — any open tab with `TableOrigin::File(path)` where `path` exists. No mode gating. | The source file exists regardless of mode; refreshing a scratch import is equally valid. Simpler than gating. |
| **D8** | **No session-schema bump (stays v8).** | Watcher is runtime-only; recovery reads the existing `session.json` / `manifest.json`. T0 confirms no new persisted field. |
| **D9** | **Architecture: per-window `SourceWatcher`** (Approach 1). | Maximal reuse of existing machinery (`SettingsWatcher` pattern, `register_file_as_table`, `compile_view_sql`, `apply_view_change`, P7b's active-window `update` hop). Per-window ownership matches the one-workspace-per-window model. Central watcher service (Approach 2) and mtime-poll (Approach 3) rejected — overkill / loses the proven `notify` path respectively (mtime-poll kept as the T0 fallback). |

## Architecture

### Components

| Unit | Purpose | Depends on |
|------|---------|-----------|
| `workspace/source_watcher.rs` (**new**) | Per-`WorkspaceShell` `notify` watcher over the dedup'd set of File-origin `source_path`s. Debounced 500 ms quiet window. Emits `SourceChanged { path }` to the main thread. Drops a watch silently on file remove/move. | `notify` (dep present), `settings/watcher.rs` pattern |
| `WorkspaceShell` refresh path (`window.rs`) | On `SourceChanged` → raise refresh banner (dedup by path). On Refresh action → edit-loss check → off-thread `register_file_as_table` (re-CTAS) → structural-only replay via `apply_view_change`. | `register_file_as_table`, `compile_view_sql`, `apply_view_change`, `MainThreadDispatcher` |
| `split_replayable` (pure, in `transform.rs` or app-side helper) | Partition a `transform_stack` into `{ replayable: Vec<Transformation> (column-keyed), dropped: { edits: usize, deletes: usize } }`. | `Transformation` enum |
| `error_ux::render_banner` button (`error_ux/banner.rs`) | If `Banner.primary` is set, render a gpui-component `Button(label)` → on click, dispatch `action_id` through `MainThreadDispatcher`. **Closes D-021.** Title-only banners keep working (button optional). | gpui-component `Button`, dispatcher |
| `recovery_panel::open` Sheet (`recovery_panel.rs`) | Mount a gpui-component `Sheet` via the `WindowRegistry` / active-window `update` hop. Render orphan-scratch + incomplete-workspace rows; wire Open / Resume / Discard. | existing `load_for_open` / `discard`, P7a `detect_incomplete` / `recover_workspace`, `WindowRegistry`, `Recents` |

### Live-data flow

```
notify event (source_path modified)
  → debounce 500 ms quiet window (coalesce editor save bursts)
  → main-thread hop → push refresh banner (dedup'd by path):
       "<file> changed on disk — [Refresh]"
  → user clicks Refresh:
       split_replayable(transform_stack): any dropped edits/deletes?
         NO  → proceed
         YES → confirm Dialog:
                "Re-import discards N cell edits + M row deletes;
                 filters / sorts / column changes will be kept. Continue?"
                 → Cancel  = keep current snapshot, dismiss banner
                 → Continue = proceed
  → off-thread: register_file_as_table(source_path)   (re-CTAS, same sniffing path)
  → apply_view_change(new base, replayable ops)        (grid refreshes; recompute_lineage)
  → success toast; refresh banner clears
```

**Watch set:** open tabs whose `TableOrigin == File(path)` and `path` exists on disk, in
**both** scratch and workspace modes (D7). Re-evaluated when tabs open/close.

**Scope (D5):** watched-tab only — downstream derived tables are not refreshed and not
proactively flagged this slice.

### Edit-loss / replay semantics (D3)

- **Replayed** (column-keyed, stable across re-CTAS): `Filter`, `Sort`, `Reorder`,
  `Rename`, `DeleteColumn`.
- **Dropped** (rowid-keyed; `__dat0_rowid` surrogate regenerated by re-CTAS): `Edit`,
  `RowDelete`. The confirm Dialog appears **only** when ≥1 such op is present.
- **Schema drift** (re-imported file gained / lost / renamed a column): replay is
  **best-effort**. If a replayable op fails to compile (`compile_view_sql` errs on a
  now-missing column), surface an error banner *"refreshed, but some transforms couldn't
  replay — schema changed"* and land on the bare re-imported base (transforms cleared to a
  safe point). No silent corruption.

### Recovery Sheet (D4)

**Boot scan** — two candidate sources, consolidated into one count banner → **Review**:

- **Orphan scratch** — `orphan_scan_emit(scratch_root)` (existing): scratch dirs containing
  a `session.json` with no live window.
- **Incomplete workspace** — for each `Recents` workspace path, `detect_incomplete(.dat0)`
  → `true` means an interrupted promote (missing `manifest.json` or `workspace.duckdb`).
  No full-filesystem scan — `Recents` is the candidate set.

**Sheet UI** (gpui-component `Sheet` drawer, mounted via the active-window `update` hop
that P7b established for its Dialog):

```
┌─ Recover unfinished work ───────────────────────────┐
│ Orphaned sessions (2)                                │
│  • sales.csv, orders.csv          [Open]  [Discard]  │
│  • adhoc                           [Open]  [Discard]  │
│ Interrupted workspaces (1)                           │
│  • ~/proj/q2  (promote didn't finish) [Resume][Discard] │
└──────────────────────────────────────────────────────┘
```

- **Open** (orphan) → spawn a window with restored tabs (`load_for_open`, existing helper).
- **Resume** (incomplete) → `recover_workspace(root)` best-effort adopt of the partial
  `.dat0/`; on hard failure → error banner, row stays for retry/discard.
- **Discard** → `discard(dir)` (orphan) / `fs::remove_dir_all(.dat0)` (incomplete) +
  decrement the count.

Two row-kinds, one list. The load-bearing helpers (`load_for_open` / `discard` /
`detect_incomplete`) stay pure + unit-tested; the Sheet render is the thin GPUI layer.

### D-021 — banner action button

`error_ux::render_banner` today paints `title` + `body` only; `Banner::with_primary(label,
action_id)` stores the action but nothing renders it. Add: when `primary` is set, render a
gpui-component `Button(label)` whose click dispatches `action_id` through the existing
`MainThreadDispatcher` action path. Closes D-021 **app-wide**; both the refresh banner and
the recovery count banner consume it.

## Error handling

| Case | Behavior |
|------|----------|
| Source file **deleted / moved** | `notify` remove event → drop that path's watch silently. Materialized data is intact. No nag. |
| Rapid editor saves | 500 ms debounce coalesces into a single refresh prompt. |
| Re-CTAS fails (corrupt / locked file) | Error banner *"couldn't re-import `<file>`"*; the old snapshot is untouched. |
| Replay schema-drift | Best-effort; on compile failure → error banner + bare re-imported base (see D3). |
| `Resume` incomplete-workspace fails | Error banner; the row remains for retry / discard. |
| Watcher thread dies | Logged `warn` (matches `SettingsWatcher`); no crash. |

## Testing

- **Pure / headless:** `split_replayable` partition (structural kept / rowid dropped +
  counts); debounce coalescing; `detect_incomplete` + the `Recents` incomplete-scan
  candidate collection; banner-button dispatch wiring.
- **Integration:** watcher fires on a real file write → `SourceChanged` (mirror
  `tests/settings_watcher.rs`); re-import round-trip preserves structural transforms and
  drops rowid edits (mirror `tests/workspace_promote.rs` 42-row pattern); orphan +
  incomplete consolidated banner count.
- **Owed to manual UAT** (headless can't click GUI): Sheet render + row buttons, refresh
  banner button click, confirm Dialog Cancel / Continue. Flagged as UAT items (P7b
  precedent — GUI dialog clicks aren't headless-testable).

## T0 spikes (gate first)

1. **`notify` watcher inside the GPUI run-loop** — confirm the callback → main-thread
   bridge works (re-CTAS must run on the app's async/main hop, not the `notify` thread).
   The real risk. Fallback: Approach 3 (mtime poll on window focus).
2. **`Sheet` mount via active-window `update`** — verify
   `cx.active_window()?.update(|_, window, cx| open_sheet(...))` renders a drawer (P7b
   proved the sibling `Dialog` variant).
3. **Banner button render + dispatch** — gpui-component `Button` inside the banner chip
   fires `action_id`. Doc-only, compile-proven (P7b T0 precedent).
4. **Confirm session stays v8** — grep for any new persisted field; expect none (D8).

## Scope & task estimate

One slice, ~13 tasks (≈ P7a's 14):

T0 spikes → `split_replayable` (pure) → `source_watcher.rs` → refresh banner + flow →
confirm-Dialog escalation → D-021 banner button → `Recents` incomplete-scan → recovery
Sheet render → row actions (Open / Resume / Discard) → i18n → deferrals (close D-020 +
D-021; open notes for live-view / cascade / auto-refresh) → docs
(`docs/live-data-recovery.md`) → e2e + full local gate.

## Closes / opens

- **Closes:** D-020 (live-data refresh), D-021 (banner action buttons). **Closes out P7.**
- **Opens (deferred, noted in `docs/deferrals.md`):** live-view import mode
  (`read_csv` VIEW); cross-table refresh cascade (P6b closure, topological re-materialize);
  per-table / global auto-refresh toggle.

## Non-goals

- Concurrency / sync-drive / settings → **P7b** (done).
- `.dat0` portable package file → **P8b**.
- Cross-table refresh cascade, live-view import mode, auto-refresh toggle → future
  (deferred above).
