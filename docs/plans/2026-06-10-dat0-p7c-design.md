# dat0 P7c — Live data + recovery polish (design STUB)

> **STATUS: STUB — not yet brainstormed.** This file captures the scope deferred *from* P7a
> (`docs/plans/2026-06-10-dat0-p7a-design.md`) and P7b so the P7c brainstorm has a starting point.
> The decisions below are **open questions**, not locked. Run `superpowers:brainstorming` before
> planning P7c.
>
> **P7 split (set in P7a):** P7a = workspace core. P7b = concurrency & sync-drive safety.
> **P7c** (this doc) = live data + recovery polish. Closes out P7.

## Goal (provisional)

Keep workspace tables **live** with respect to their on-disk source files: watch the source
files, and on external change, prompt to refresh + replay the lineage. Finish the crash-recovery
UX (the `recovery_panel` Sheet view, currently a stub).

## Scope (deferred from P7a)

- **File-watcher on workspace source files** — `notify` is already a dep (`Cargo.toml:30`, used by
  `SettingsWatcher`); reuse the pattern to watch registered source paths.
- **Replay-on-change** — when a watched source file changes externally, re-import it and replay the
  table's transform chain so derived views update.
- **Refresh prompt UX** — non-blocking "`<file>` changed on disk — refresh?" affordance.
- **Crash-recovery polish** — `recovery_panel::open` is a tracing stub today
  (`recovery_panel.rs`); the load-bearing `load_for_open` / `discard` helpers exist. Mount the
  `gpui_component::Sheet` view (needs a `&mut Window` hop via `WindowRegistry`) to surface the
  orphan-scratch + incomplete-workspace recovery list.

## Key tension to resolve in brainstorm

**The app materializes (CTAS), it doesn't view.** P7a established that `file_drop.rs` →
`register_file_as_table` → a **base table** (data copied into the DB), *not* a `read_csv` view over
the live file. So "watch the source file" raises a real question: a materialized table has **no
live dependency** on its source — the data is a snapshot in `workspace.duckdb`.

So P7c's watcher is really about **re-import on change**, not "the view auto-updates":

1. **What is watched?** `Tab.source_path` (the original import path, already persisted per tab).
   Files registered as views (`register_file`) *would* be live, but those aren't the drop path.
2. **What does "replay" mean for a materialized table?** Re-run `register_file_as_table` (re-CTAS
   from the changed file) → then re-apply the transform stack. This is a re-import, not a cheap
   refresh. Debounce + confirm before clobbering edits.
3. **Conflict with in-flight edits/transforms** — a re-import replaces the base table; display-only
   edits/projections (overlays, per P6) survive, but base-row edits (P4b) would be lost. Surface
   this in the refresh prompt.
4. **Should P7c also offer a "live view" import mode?** (register as `read_csv` view instead of
   CTAS, so it genuinely auto-updates) — possible scope, or defer.

## Open questions to brainstorm

1. Watch original `source_path`s vs a watched `sources/` dir inside the workspace (relates to the
   P7a-deferred "copy sources in" option).
2. Debounce window + dedupe rapid editor saves.
3. Replay semantics + edit-loss warning copy.
4. Recovery Sheet: scope to orphan-scratch only, or also incomplete-workspace (P7a writes the
   "interrupted save" case)?
5. Whether to introduce a live-view import mode (read_csv view) as a first-class choice.

## Foundations available (verified during P7a brainstorm, 2026-06-10)

- `notify` dep + `SettingsWatcher` pattern (`settings/watcher.rs`) — file-watch template.
- `Tab.source_path: Option<PathBuf>` (`session/mod.rs`) — already persisted; the watch target.
- `register_file_as_table` (re-import) + `register_file` (live view) both exist in the engine
  (`duckdb_engine.rs:159/230`).
- `recovery_panel.rs` — `load_for_open` / `discard` done; `open` Sheet view is the remaining stub.
- `window_registry.rs` — the `&mut Window` hop needed to mount the Sheet.

## Spec exit criteria this slice owns (spec §P7)

- "File watcher detects external CSV change → prompts refresh; refresh replays lineage
  successfully."
- "Force-quit during workspace work → next launch recovers workspace state from `.dat0/`." (P7a
  delivers the recover path; P7c finishes the *review/select* recovery UI.)

## Non-goals

- Concurrency / sync-drive / settings → **P7b**.
- `.dat0` portable package file → **P8b**.
