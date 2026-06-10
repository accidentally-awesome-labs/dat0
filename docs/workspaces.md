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
- If the workspace is already open in another window, dat0 silently focuses
  that window instead of opening a second one.

## Open Recent

**File → Open Recent** lists the workspaces you have opened most recently (up to
10 entries in the menu; the full list is available in the recents store). Click
any entry to open that workspace directly, without a folder-picker.

The recent list is updated whenever you save a new workspace or open an existing
one.

## Single-window lock

A workspace can be open in **only one dat0 window at a time** (on the same
machine). When dat0 opens a workspace it acquires an exclusive `flock` lock on
the `lock` file. If you try to open the same workspace again, dat0 detects the
open window and focuses it instead.

**Stale locks self-heal.** If dat0 exited uncleanly (crash, force-quit), the OS
releases the flock when the process terminates. The next time you open the
workspace the lock is free.

> **P7b note:** Cross-machine locking (e.g. a workspace folder on a sync drive
> shared between two machines) is not yet enforced. See D-019 for the planned
> heartbeat/lease approach and sync-drive detection coming in P7b.

## What's deferred

- **Sync drives (Dropbox, iCloud, OneDrive, Google Drive):** dat0 does not yet
  detect or warn when a workspace folder sits on a sync-drive path. Concurrent
  writes from the sync daemon while dat0 has the workspace open can corrupt the
  DuckDB file. Until D-019 lands (P7b), keep workspace folders on a local,
  non-synced volume.
- **Cross-machine lock and force-unlock UI:** coming in D-019 / P7b.
- **Live-source refresh:** if you modify the original CSV/Parquet/JSON file that
  you imported, the open table does not update automatically — dat0's tables are
  materialized at import time. File-watcher re-import (debounced re-CTAS + transform
  replay) is planned in D-020 / P7c.
