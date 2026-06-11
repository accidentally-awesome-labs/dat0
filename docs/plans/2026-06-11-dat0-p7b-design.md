# dat0 P7b — Concurrency & sync-drive safety (design)

> **P7 split (set in P7a):** P7a = workspace core (shipped, PR #18 / `0c0dca8`). **P7b** (this doc)
> = concurrency & sync-drive safety. P7c = live data.
>
> Brainstormed 2026-06-11 via `superpowers:brainstorming`. Supersedes the stub
> `docs/plans/2026-06-10-dat0-p7b-design.md`. Three load-bearing decisions (lock model, override
> model, conflict surface) confirmed interactively with the user.

## Goal

Make workspaces safe to open across **multiple machines via a sync drive** (Dropbox / iCloud /
OneDrive / Google Drive / Syncthing), where P7a's `flock` is single-machine-only. Add a **JSON
lock manifest** as the cross-machine layer, a **networked-path heuristic + override**, a real
blocking **`WorkspaceInUseModal`**, real same-machine **bring-to-front**, and the **Settings →
Workspace** section.

**Framing fact that bounds the phase:** dat0 is a **single OS-process** (the `app_lock` PID-file +
flock + UDS singleton — second launches forward over the socket and exit). So same-machine
second-opens are always routed through the one process and its `window_registry`; flock
*contention* same-machine effectively never happens in normal use. **The manifest's real job is
purely cross-machine** — a separate dat0 process on machine B observing machine A's record on the
shared drive. This keeps the new lock layer small and its blast radius cross-machine-only.

## Decisions (locked in brainstorm)

| # | Decision | Over | Why |
|---|----------|------|-----|
| D1 | **Acquire + tombstone manifest; NO heartbeat/lease** | TTL'd heartbeat (renew timer) | Sync propagation lag (minutes) > any sane TTL → a live remote holder would read as "stale" → false reclaim = corruption. The one outcome v1 must never produce. Same-machine staleness is solvable locally (pid liveness); cross-machine is unprovable → **warn, don't auto-resolve** (matches spec: "tombstone on release", no heartbeat). `heartbeat_at` stays an additive future field. |
| D2 | **Heuristic + per-path force-on list + global toggle; NO force-local** | full per-path tri-state / global-only | Risk is asymmetric: under-detection (missed sync path → no manifest → silent cross-machine corruption) is dangerous; over-detection is **free** (a falsely-"networked" local-only folder never sees a foreign hostname, so the warn never fires and the open path is identical). Only the force-**on** override is load-bearing. Reuses the pre-laid `treat_paths_as_networked` field; adds one global bool. |
| D3 | **Real blocking `WorkspaceInUseModal`** (gpui-component Dialog) | reuse non-blocking Banner | The conflict is a corruption risk; a banner opens the window first and is dismissible (you may already be editing a shared DB before reading it). A modal is a **gate** — nothing opens until you choose. Also stands up the app's first actionable dialog. |
| D4 | **Manifest only for networked workspaces; local keeps P7a shape (flock only)** | always write `lock.json` | Don't litter `lock.json` into every local workspace; the manifest is the cross-machine layer only. Local workspaces are byte-for-byte P7a on disk. |
| D5 | **Same-machine in-process conflict → modal with [Focus existing]** | silent activate | User pick (Q3). Explicit + discoverable for the "two windows" case; the alternative (silent bring-to-front, no modal) is the rejected option, noted for the retro. |

## Grounded facts (verified live, 2026-06-11)

- **P7a merged.** Foundations in `crates/dat0-app/src/`: `workspace/lock.rs` (`WorkspaceLock`
  flock RAII, `try_acquire -> Result<Option<Self>>`); `workspace/manifest.rs` (identity manifest);
  `workspace/promote.rs`; `Home` enum on `Session` with `lock_path()` / `manifest_path()`.
- **`app_lock.rs` = the mirror pattern.** PID file + `fs4` `try_lock_exclusive` + UDS singleton +
  `Drop` cleanup. P7b's `LockManifest` mirrors its acquire/contention/stale shape but adds the
  cross-machine record.
- **Banner draws no buttons (D-021).** `error_ux::render_banner` (`error_ux/banner.rs:163`) renders
  title + body only; `Banner` *carries* `primary`/`secondary` `BannerAction` but they're unrendered.
  `error_ux::modal::Modal::render` is a title+message stub. → a real actionable dialog is net-new.
- **gpui-component has Dialog + Sheet** at the pinned rev `0f0ab35`
  (`Root::render_dialog_layer` / `render_sheet_layer`, `DialogStory`). Infra present; the exact
  "push a blocking dialog + wire button callbacks" API is the **T0 unknown**.
- **Settings schema pre-laid the override seam.** `settings/schema.rs` already has
  `Workspace { treat_paths_as_networked: Vec<PathBuf> }` (per-path force-on list). `Settings` is
  `#[serde(default)]` → adding `treat_all_as_networked: bool` (default `false`) is
  backward-compatible; **`schema_version` stays 1**. Live `SettingsWatcher` already hot-reloads.
- **Settings UI section pattern.** `SettingsSection` trait (`name_key`/`id`/`render`) in
  `settings_ui/sections/`; `ThemeSection` is the template. A `WorkspaceSection` slots in beside it.
- **No hostname / pid-liveness dep yet.** App is unix-only (UDS, per `Cargo.toml:38`). Plan: add
  `libc` (direct) for `kill(pid, 0)` liveness + a tiny hostname source (`gethostname` preferred —
  one fn, no transitive deps; finalize at T0). `dat0_version` = `env!("CARGO_PKG_VERSION")`.

## On-disk layout — one new file

The P7a `.dat0/` gains a single sibling, written **only when the workspace is networked**:

```
<user-folder>/.dat0/
├── manifest.json        # P7a — workspace identity (format_version, workspace_id, timestamps)
├── workspace.duckdb      # P7a — the DB
├── session.json          # P7a — window/view state (v8)
├── lock                  # P7a — advisory flock target (content irrelevant; OS lock only)
├── lock.json             # NEW (P7b) — cross-machine holder record; networked workspaces only
└── lineage/              # P7a — reserved
```

`lock.json` (distinct from both `lock` and `manifest.json`):

```json
{
  "pid": 4821,
  "hostname": "salar-mbp",
  "started_at": "2026-06-11T10:04:00Z",
  "dat0_version": "0.1.0",
  "tombstoned": false
}
```

- Atomic write (temp + rename, same pattern as `session::persist` / `manifest::write`).
- `tombstoned: true` on clean release → next open is friction-free.
- `started_at` is the modal's "since when" (the static 80% of a heartbeat's UX value, zero churn).

## Modules

1. **`workspace/networked.rs`** — pure `is_networked(path: &Path, settings: &Workspace) -> bool`:
   ```
   is_networked = known_sync_prefix(path)
               || settings.treat_paths_as_networked.iter().any(|p| path starts_with p)
               || settings.treat_all_as_networked
   ```
   Known prefixes (home-expanded + canonicalized before compare):
   `~/Library/Mobile Documents/` (iCloud), `~/Dropbox`, `~/OneDrive`, `~/Google Drive`,
   `~/.var/syncthing`, plus a `<file>.icloud` sibling signal. Table-tested. No I/O beyond
   canonicalize.

2. **`workspace/lock_manifest.rs`** — `LockManifest` (serde struct above) + `read` / `write`
   (atomic) / `tombstone`, and the decision core:
   ```rust
   enum AcquireOutcome {
       Acquired,                       // absent | tombstoned  -> wrote ours
       Reclaimed,                      // same host + pid dead  -> wrote ours
       HeldSameMachine,                // same host + pid alive -> in-process path owns it
       ConflictForeign(LockManifest),  // foreign host + live   -> NO write; modal decides
   }
   fn acquire(dat0: &Path, me: &Identity) -> Result<AcquireOutcome>;
   fn release(dat0: &Path) -> Result<()>; // set tombstoned = true
   ```
   Decision table (read existing `lock.json`):
   | existing record | outcome |
   |---|---|
   | absent or `tombstoned: true` | `Acquired` (write ours) |
   | `hostname == me` && `!pid_alive(pid)` | `Reclaimed` (write ours) |
   | `hostname == me` && `pid_alive(pid)` | `HeldSameMachine` |
   | `hostname != me` && not tombstoned | `ConflictForeign(holder)` |

3. **`workspace/identity.rs`** (or folded into `lock_manifest`) — `Identity { pid, hostname,
   dat0_version }`; `current() ` (pid = `std::process::id()`, hostname via the chosen crate,
   version = `env!("CARGO_PKG_VERSION")`); `pid_alive(pid) -> bool` via `libc::kill(pid, 0)`
   (`Ok`/`EPERM` ⇒ alive; `ESRCH` ⇒ dead). Unix-only, matching the app.

4. **`workspace_in_use_modal.rs`** (new; gpui-component Dialog) — blocking, two modes:
   - **Cross-machine** (`ConflictForeign`): ⚠ title, `"Open on <hostname> since <started_at>.
     Editing on two machines at once can corrupt this workspace."`, buttons `[Cancel]`
     (default) / `[Open anyway]`.
   - **Same-machine in-process**: `"Already open in this app."`, buttons `[Cancel]` /
     `[Focus existing window]`.

5. **Settings** — `treat_all_as_networked: bool` added to `schema::Workspace` (no version bump);
   new `settings_ui/sections/workspace.rs` `WorkspaceSection`: a toggle widget bound to a
   `workspace_networked_handler` (SettingsStore write, mirroring `theme_change_handler`'s split)
   + a read-only render of the `treat_paths_as_networked` list. Editing the per-path list via UI
   is **v1.x** (v1 surfaces it; the field is still writable via the settings file).

6. **Window** — replace P7a's logged no-op focus-existing with a real bring-to-front
   (GPUI window `activate`), used by the same-machine modal's `[Focus existing]`.

## Open / acquire flow (revises P7a `window::open_workspace_flow`)

```
open_workspace(folder):
  dat0 = folder/.dat0; missing -> banner "Not a dat0 workspace"; abort
  window_registry has folder (this process)?
      -> WorkspaceInUseModal(same-machine):
           Focus existing -> activate that window. Done.
           Cancel         -> abort.
  flock(dat0/lock):
      held    -> (single-instance: rare) treat as same-machine in-use; banner; abort
      acquired:
        if is_networked(folder):
          match LockManifest::acquire(dat0, identity):
            Acquired | Reclaimed     -> recover_workspace; register; recents.push
            HeldSameMachine          -> (registry missed it) banner "open in another dat0"; abort
            ConflictForeign(holder)  -> WorkspaceInUseModal(cross-machine, holder):
                 Cancel      -> drop flock; abort
                 Open anyway -> LockManifest::write(ours); recover_workspace (warned)
        else:
          recover_workspace            -- P7a path; no lock.json
```

- **`Open anyway` writes our identity (last-opener-wins) but performs NO deliberate eviction.** It
  does not tombstone or specially clear a record it believes stale — that confirmed-eviction flow
  (with holder details + a "this lock looks stale, break it?" prompt) is **v1.x force-unlock**. v1
  only warns and proceeds; the file then reflects the most-recent opener, and a third machine still
  sees a live holder. The correctness invariant preserved across v1: **no automated reclaim of a
  live foreign holder without explicit user consent** (`Open anyway` *is* that consent).
- On clean window close of a networked workspace: `LockManifest::release` (tombstone). Crash → no
  tombstone → next foreign open sees a live record → warns (correct: we can't prove the crash
  remotely). Same-machine reopen after crash → pid-dead → `Reclaimed` silently.
- The flock guard is released on `Cancel` (RAII drop) so a cancelled open leaves nothing held.

## Settings → Workspace section

- New section in the existing Settings panel (P10 verifies all sections present; this lands the
  "Networked Workspaces (P7)" row from the spec's §24.2 table).
- v1 surface: a **"Treat all workspaces as networked"** toggle (`treat_all_as_networked`) + a
  read-only list of the force-on paths (`treat_paths_as_networked`). Hot-reloads via the live
  `SettingsWatcher`.
- Per-path add/remove **UI** → v1.x; the field remains file-editable in v1.

## Testing (applies the P5c / P7a lesson — no hidden untestable assumption)

- **Cross-machine is unit-tested via synthetic foreign manifests, NOT real sync.** Write a
  `lock.json` fixture and assert each `AcquireOutcome`:
  - foreign hostname + not tombstoned → `ConflictForeign`
  - same hostname + dead pid (e.g. `999999`) → `Reclaimed`
  - same hostname + live pid (`std::process::id()`) → `HeldSameMachine`
  - tombstoned / absent → `Acquired`
- **Heuristic** table-test: Dropbox/iCloud/OneDrive/GDrive/syncthing paths → true; `/tmp/x` → false;
  force-on list hit; `treat_all_as_networked` → true.
- **Settings** round-trip + hot-reload (`treat_all_as_networked`), reusing the watcher test pattern.
- **Modal** render + button-action (light gpui test or pure handler unit).
- **Explicitly out of automated scope (documented limitation):** real sync-drive propagation
  timing and a genuine two-machine race. v1 detection is correct *given a propagated record*; the
  propagation itself is the OS/provider's job. This mirrors P7a's honesty about engine behavior —
  no credential-/two-machine-gated assumption is allowed to hide design-invalidating surprises.

## T0 spikes (GATE before building)

1. **gpui-component Dialog** — push a blocking dialog onto `Root`'s dialog layer from a dat0 use
   site and wire `[Cancel]` / `[Open anyway]` callbacks. Infra is confirmed present (`DialogStory`,
   `render_dialog_layer`); the API surface is the unknown. Capture in
   `docs/internal/gpui-component-api-notes.md`. **The one real risk — gate it first.**
2. **hostname + `libc::kill(pid, 0)`** on macOS — confirm the hostname crate returns the device
   name and that `kill(pid, 0)` distinguishes alive (`Ok`/`EPERM`) from dead (`ESRCH`). Low risk.

No data-loss mechanic this phase (P7a's close→move→reopen was the dangerous one). T0 here is API
discovery, not correctness-of-data.

## Error / edge handling

- **Unreadable / malformed `lock.json`** → treat as absent (acquire), log a warning. A corrupt
  record must not wedge the open.
- **`lock.json` write fails** (read-only drive) on a networked workspace → banner; still open
  (the workspace itself is openable; we just can't advertise our hold). Degrade to flock-only.
- **Heuristic false-positive** (local-only flagged networked) → harmless (no foreign hostname ever
  appears; open path identical). Covered by D2's asymmetry argument.
- **Clock skew across machines** → `started_at` is display-only ("since 10:04"); never used for a
  liveness *decision* (no heartbeat/TTL). Skew at worst mislabels the "since" text, never reclaims.

## Non-goals (explicitly out — deferred)

| Item | Target |
|------|--------|
| Force-unlock (clearing another machine's live record) | **v1.x** (per spec) |
| Read-only / Inspect open of an in-use workspace | **P8b** (Inspect mode) |
| Per-path force-**local** override | dropped (YAGNI — over-detection is free) |
| Per-path force-on **UI** (add/remove in Settings) | **v1.x** (field is file-editable in v1) |
| Heartbeat / lease staleness | dropped for v1 (additive field reserved) |
| Banner action buttons (D-021) | stays deferred (modal ≠ banner; modal gets real buttons) |
| File-watcher on source files + replay-on-change | **P7c** |
| Real two-machine / sync-propagation automated test | out of scope (documented) |

## Spec exit criteria this slice owns (spec §P7)

- "Two windows trying same workspace: second blocked, modal surfaced, 'focus existing' works."
  (P7a covered the same-machine *banner* half; P7b lands the **modal** + real bring-to-front.)
- "Sync-drive workspace gets manifest-based protection; force-unlock NOT yet present (deferred
  v1.x)."
- Settings panel gains the **Networked Workspaces** section (P10 verifies all sections present).

## Deferrals touched

- **Closes D-019** (P7b concurrency / sync-drive — opened at P7a execution).
- Does **not** close D-021 (banner buttons); the modal's real buttons are a *separate* surface.
- Opens nothing expected; if Dialog infra forces a reusable wrapper, log it at execution per the
  deferral protocol.

## Forward-compat note

`lock.json` is additive: P7a writers/readers are untouched; local workspaces keep their exact P7a
shape. `heartbeat_at` and a holder-list (for v1.x force-unlock / multi-reader) are additive fields
the v1 reader tolerates (serde `#[serde(default)]`). The separate-file split (`lock` vs
`lock.json` vs `manifest.json`) keeps each concern independently evolvable and P8a-ratifiable.

## Decisions register (for the retro)

- **CHOSE** acquire+tombstone (no heartbeat) **OVER** TTL'd lease — sync lag makes TTL staleness a
  corruption risk; warn-don't-resolve is the only honest cross-machine contract. WOULD REVISIT IF a
  concrete "last seen" need appears (additive `heartbeat_at`).
- **CHOSE** heuristic + force-on + global toggle, no force-local **OVER** tri-state / global-only —
  asymmetric risk (under-detection dangerous, over-detection free); reuses pre-laid schema field.
- **CHOSE** real blocking modal **OVER** banner reuse — corruption risk needs a gate, not a
  dismissible notice; banner draws no buttons anyway (D-021).
- **CHOSE** manifest only when networked **OVER** always-write — keep local workspaces byte-identical
  to P7a; manifest is the cross-machine layer.
- **CHOSE** same-machine modal w/ [Focus existing] **OVER** silent activate — user pick (explicit +
  discoverable).

## Slicing

**One slice**, ~11 tasks (comparable to P6b's 9): T0 spikes (Dialog + hostname/liveness) · networked
heuristic · lock_manifest + identity/liveness · schema field + settings handler · WorkspaceSection
UI · WorkspaceInUseModal · open-flow integration · bring-to-front · i18n + docs + deferrals · e2e
(synthetic-foreign-manifest contention + full gate). Plan authored next via
`superpowers:writing-plans`.
