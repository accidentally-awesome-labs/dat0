# dat0 P7b — Concurrency & sync-drive safety (design STUB)

> **STATUS: STUB — not yet brainstormed.** This file captures the scope deferred *from* P7a
> (`docs/plans/2026-06-10-dat0-p7a-design.md`) so the P7b brainstorm has a starting point. The
> decisions below are **open questions**, not locked. Run `superpowers:brainstorming` before
> planning P7b.
>
> **P7 split (set in P7a):** P7a = workspace core (shipped/in-flight). **P7b** (this doc) =
> concurrency & sync-drive safety. P7c = live data.

## Goal (provisional)

Make workspaces safe to open across **multiple machines via a sync drive** (Dropbox / iCloud /
OneDrive / Google Drive), where the P7a `flock` is unreliable. Add the rich
`WorkspaceInUseModal`, the sync-drive heuristic + override, and the Settings → Workspace section.

## Scope (deferred from P7a)

- **Hybrid lock.** P7a ships `flock` only (advisory, same-machine, auto-released on death). P7b
  adds a **JSON lock manifest** for sync-drive scenarios — flock semantics don't cross machines,
  so a file-based holder record is needed.
- **Sync-drive heuristic** — detect when the workspace path lives on a sync drive (path-prefix
  heuristics for known providers + filesystem signals) and switch to manifest-lock mode.
- **"Treat as networked" override toggle** (Settings) — manual override when the heuristic
  misfires either way.
- **Rich `WorkspaceInUseModal`** — replaces P7a's minimal banner; shows holder identity
  (machine / user / since-when) + cross-machine warning + "Focus existing" (same-machine) /
  "Open read-only?" / "Open anyway" affordances.
- **Settings → Workspace section** — per the spec's settings-sections table (§ "Settings UI",
  "Networked Workspaces (P7)"). v1 is process-global only; per-workspace overrides → v1.x.

## Open questions to brainstorm

1. **JSON lock-manifest schema** — holder identity (machine id, hostname, user, pid, dat0
   version), acquired-at, and a **heartbeat/lease** (manifest locks have no OS auto-release, so
   staleness must be inferred from a TTL'd heartbeat). What TTL? Who renews it and how often?
2. **Stale-lock detection** — without flock's auto-release, a crashed holder leaves a live-looking
   manifest. Heartbeat-expiry → "looks stale, take over?" Force-unlock itself stays **v1.x** per
   spec; P7b only *detects* + warns, does not force.
3. **Sync-drive heuristic** — which signals? (path prefixes: `~/Dropbox`, `~/Library/Mobile
   Documents` [iCloud], `~/OneDrive`, `~/Google Drive`; plus possibly reparse/xattr signals). False
   positives/negatives → the override toggle is the safety valve.
4. **Interaction with P7a's flock** — on a sync drive, is flock *also* held (belt-and-suspenders for
   the same-machine case) or replaced entirely by the manifest lock? Likely both: flock for
   same-machine, manifest for cross-machine.
5. **`WorkspaceInUseModal` UX** — read-only open? force-open-anyway (with corruption warning)? The
   spec's P7 exit says "force-unlock NOT yet present (deferred v1.x)".

## Foundations available (verified during P7a brainstorm, 2026-06-10)

- `fs4` flock pattern (`app_lock.rs:39-55`) — P7a's same-machine lock; P7b layers the manifest on
  top.
- `error_ux::modal` exists (`error_ux/modal.rs`) — host for `WorkspaceInUseModal`.
- `settings/schema.rs` + `settings/store.rs` + live `SettingsWatcher` — host for the Workspace
  settings section; settings already hot-reload.
- P7a's `manifest.json` (`.dat0/manifest.json`) — the lock manifest is a **sibling** file
  (`.dat0/lock.json` or similar), distinct from the identity manifest.

## Spec exit criteria this slice owns (spec §P7)

- "Sync-drive workspace gets manifest-based protection; force-unlock NOT yet present (deferred
  v1.x)."
- "Two windows trying same workspace: second blocked, modal surfaced, 'focus existing' works."
  (P7a covers the same-machine/in-process half via banner + window_registry focus; P7b adds the
  full modal + cross-machine.)

## Non-goals

- Force-unlock → **v1.x**.
- Per-workspace settings overrides → **v1.x** (v1 = process-global).
- File-watcher / live sources → **P7c**.
