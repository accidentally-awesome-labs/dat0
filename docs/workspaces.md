# Workspaces

dat0 windows start as anonymous **scratch sessions**. A *workspace* is a folder
you choose — dat0 stores a `.dat0/` subdirectory inside it that holds your data,
session state, and a single-window guard so the same workspace is never opened
twice at once.

Workspaces are introduced in P7a.

## Scratch vs workspace

| | Scratch | Workspace |
|---|---|---|
| Where data lives | App-managed scratch directory (auto-saved) | `<your folder>/.dat0/workspace.duckdb` |
| Reopenable by name | No — identified by UUID only | Yes — open the folder again via File → Open Workspace… or File → Open Recent |
| Shows in Open Recent | No | Yes |
| Single-window lock | No | Yes |

Every window starts as scratch. Nothing is lost when you close a scratch window
— it remains in the app's state directory and will recover on next launch
(session restore). Promoting to a workspace is a deliberate step you take when
you want to give the session a permanent home.

## The `.dat0/` layout

When you save a workspace, dat0 creates a `.dat0/` subdirectory inside the folder
you chose:

```
<your folder>/
└── .dat0/
    ├── manifest.json     — workspace identity (name, created-at, dat0 version)
    ├── workspace.duckdb  — your data (tables, base rows, transform history)
    ├── session.json      — tab layout, column view state, saved queries
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
3. dat0 moves your scratch data into `<folder>/.dat0/` losslessly. This is a
   filesystem move, not a copy: your existing DuckDB tables, rows, and session
   state are transferred atomically. External attachments (MotherDuck databases,
   attached SQLite files) remain external and are re-attached automatically when
   the workspace is next opened.
4. The window title and Open Recent list update immediately.

The move is failure-safe: the old scratch directory is only removed after the
new workspace files are in place. If something interrupts the save mid-way, dat0
detects the incomplete state next time you try to open the folder and shows a
warning rather than silently loading a corrupted workspace.

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
- If the workspace is already open in another window of this dat0 instance, dat0
  declines to open a second one (bringing the existing window to the front is a
  later refinement).

## Open Recent

**File → Open Recent** lists the workspaces you have opened most recently (up to
10 entries in the menu; the full list is available in the recents store). Click
any entry to open that workspace directly, without a folder-picker.

The recent list is updated whenever you save a new workspace or open an existing
one.

## Single-window lock

A workspace can be open in **only one dat0 window at a time** (on the same
machine). When dat0 opens a workspace it acquires an exclusive `flock` lock on
the `lock` file. If you try to open the same workspace again in this dat0
instance, dat0 detects the open window and declines to open a duplicate.

**Stale locks self-heal.** If dat0 exited uncleanly (crash, force-quit), the OS
releases the flock when the process terminates. The next time you open the
workspace the lock is free.

> **P7b note:** Cross-machine locking for sync-drive workspaces shipped in P7b.
> See "Networked / sync-drive workspaces" below for details.

## Networked / sync-drive workspaces

dat0's single-window lock (above) is a per-machine `flock` — it cannot see a
*second machine* opening the same workspace folder over a sync drive (Dropbox,
iCloud Drive, OneDrive, Google Drive, Syncthing). Editing one workspace from two
machines at once can corrupt it, because each machine's DuckDB writes race
through the sync daemon. P7b adds a **cross-machine lock** to detect this.

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
- **Live-source refresh:** if you modify the original CSV/Parquet/JSON file that
  you imported, the open table does not update automatically — dat0's tables are
  materialized at import time. File-watcher re-import (debounced re-CTAS + transform
  replay) is planned in D-020 / P7c.
