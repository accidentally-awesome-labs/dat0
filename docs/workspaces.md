# Workspaces

Ordinary new windows start as anonymous **scratch sessions**. Opening a workspace
or a package in Inspect mode starts directly in that mode instead. A *workspace*
is a folder you choose — dat0 stores a `.dat0/` subdirectory inside it containing
your data, session state, and a single-window guard.

[Documentation index](README.md)

## Scratch vs workspace

| | Scratch | Workspace |
|---|---|---|
| Where data lives | App-managed scratch directory (auto-saved) | `<your folder>/.dat0/workspace.duckdb` |
| Reopenable by name | No — identified by UUID only | Yes — open the folder again via File → Open Workspace… or File → Open Recent |
| Shows in Open Recent | No | Yes |
| Single-window lock | No | Yes |

Closing a scratch window normally leaves work worth recovering in the app's state
directory; sessions with nothing unique to recover are swept at boot. On a later
launch, use **Review** in the recovery banner to choose what to reopen, rather
than automatic restoration. Transient SQL-result tabs need their queries rerun.
Promotion gives a scratch session a permanent home.

## The `.dat0/` layout

When you save a workspace, dat0 creates a `.dat0/` subdirectory inside the folder
you chose:

```
<your folder>/
└── .dat0/
    ├── manifest.json     — workspace identity (name, created-at, dat0 version)
    ├── workspace.duckdb  — tables and durable origins/read options
    ├── session.json      — tabs, transforms, column view state, SQL documents
    ├── lock              — single-window guard (flock; released on clean exit)
    └── lineage/          — reserved for future lineage snapshots
```

These files are the authoritative source of truth for the workspace. Do not
move or rename them individually — move the whole parent folder instead (the
`.dat0/` subdir travels with it).

## Saving a workspace (File → Save Workspace…)

1. Choose **File → Save Workspace…** (or use the Command Palette).
2. Pick a folder in the native folder-picker. The folder can be empty or an
   existing project directory — dat0 creates `.dat0/` inside it without
   disturbing other files.
3. dat0 transfers the scratch database and session state sequentially into
   `<folder>/.dat0/`. Individual files are renamed where possible, or copied and
   removed when crossing volumes. This is **not one atomic rename**: the
   completion manifest is written last. External attachments remain external.
4. The window title and recent-workspace storage update. The native Open Recent
   menu is a launch-time snapshot and may not reflect this change until restart.

The move is failure-safe: the old scratch directory is only removed after the
new workspace files are in place. If something interrupts the save mid-way, dat0
detects the incomplete state next time you try to open the folder and shows a
warning rather than silently loading a corrupted workspace.

An existing `.dat0/` destination is refused. Saving an already-promoted workspace
does not perform **Save As**; it already has a persistent home.

### The workspace prompt

After you make a few transforms or save a query, a banner nudge appears at the
top of the window:

> **Save this as a workspace to keep it?** · Save Workspace

This is an informational nudge — use **File → Save Workspace…** to act on it.
The prompt appears at most once per window session and does not repeat.

## Opening a workspace

**File → Open Workspace…** shows the native folder-picker. Pick the folder that
*contains* the `.dat0/` subdirectory (not the `.dat0/` folder itself).

dat0 validates the folder before opening a window:

- If the folder has no `.dat0/` subdir, a warning banner appears.
- If the `.dat0/` layout looks incomplete (interrupted save), a warning banner
  appears with guidance.
- If the workspace is already open in another window of this dat0 instance,
  dat0 focuses that window instead of opening a duplicate.

## Open Recent

**File → Open Recent** lists the workspaces you have opened most recently (up to
10 entries in the menu; the full list is available in the recents store). Click
any entry to open that workspace directly, without a folder-picker.

Recent-workspace storage is updated whenever you save or open a workspace.
The native menu is built at launch, so its entries can lag behind that storage.

## Single-window lock

A workspace can be open in **only one dat0 window at a time** (on the same
machine). When dat0 opens a workspace it acquires an exclusive `flock` lock on
the `lock` file. Opening it again in this dat0 instance focuses the existing
window rather than creating a duplicate.

**Stale locks self-heal.** If dat0 exited uncleanly (crash, force-quit), the OS
releases the flock when the process terminates. The next time you open the
workspace the lock is free.

Cross-machine locking adds the checks described below for sync-drive workspaces.

## Networked / sync-drive workspaces

dat0's single-window lock (above) is a per-machine `flock` — it cannot see a
*second machine* opening the same workspace folder over a sync drive (Dropbox,
iCloud Drive, OneDrive, Google Drive, Syncthing). Editing one workspace from two
machines at once can corrupt it, because each machine's DuckDB writes race
through the sync daemon. A **cross-machine lock** warns about this conflict.

**When it applies.** Only workspaces dat0 considers *networked* get the extra
lock. A folder is networked if its path sits under a known sync-provider
directory, or if you turn on **Settings → Networked Workspaces → "treat all as
networked"** (use this when your sync drive isn't auto-detected). Local
workspaces (e.g. on an internal SSD) are unaffected — they rely on the flock
alone. Detection only ever errs toward "networked": over-protecting a local
folder is harmless, under-protecting a synced one is not.

**How it works.** A networked workspace carries a small `.dat0/lock.json` holder
record (this machine's hostname + pid, written when you open it, tombstoned when
you close it). On open, dat0 reads any existing record and decides:

- **No record / tombstoned / your own machine's dead pid** → opens normally and
  claims it.
- **Already open in another window on _this_ machine** → offers to focus the
  existing window instead of opening a duplicate.
- **A live record from _another_ machine** → a blocking dialog warns the
  workspace may be open elsewhere (showing the other host and roughly how long
  ago it was opened). You can **Cancel** or **Open anyway** (which takes the
  lock).

A workspace you *save into* a sync drive is claimed the same way at save time.

**No heartbeat (by design).** The record has no time-to-live or heartbeat.
Sync-drive propagation lag would make a TTL "is it stale?" check itself a
corruption risk, so dat0 never auto-expires a foreign lock — it only *warns*,
and you decide. There is no automatic force-unlock yet; recovering from a truly
stale foreign lock (a machine that crashed mid-session) is a manual **Open
anyway**. A dedicated force-unlock UI is planned for a later release.

**Limitation.** dat0 does not *synchronize* across machines — propagating the
lock record is the sync provider's job. dat0 reads whatever record has already
arrived locally and warns on a conflict; it cannot prevent a true simultaneous
open if the providers haven't synced the record yet.

## What's deferred

- **Force-unlock UI:** there is no dedicated "Force unlock" button yet. A stale
  foreign lock (from a machine that crashed mid-session) is resolved by **Open
  anyway**. A richer force-unlock UI is planned for a later release.
- **Live-source refresh:** imported tables are materialized snapshots, not
  continuously synchronized data. Source-change banners offer an explicit
  refresh; see [Live-data refresh & recovery](live-data-recovery.md) for replay,
  edit-discard, and schema-drift behavior.
