# Live-data refresh & recovery

dat0 imports a file by **copying its data into the workspace** (a materialized
table), so what you see in the grid is a snapshot taken at import time. Two
features keep that snapshot honest when the world around it changes:

- **Live-data refresh** — when a table's source file changes on disk, dat0
  offers to re-import it and replay your work.
- **Recovery** — if dat0 (or a workspace save) was interrupted, the next launch
  offers to bring the unfinished work back.

Both are introduced in P7c.

## Live-data refresh

### When a source file changes

dat0 watches the source file behind the table you're currently looking at. When
that file changes on disk, a banner appears at the top of the window:

> **`<file>` changed on disk** — **[Refresh]**

Click **Refresh** to pull the new data in. dat0 re-imports the file (the same
sniffing and import path as the original drag-and-drop) and then replays the
work you've done on that tab onto the fresh data.

The watch follows the **active** table. When you switch to a different table, the
watch re-targets to that table's source file.

### What survives a refresh

Your **structural / column-keyed** work is replayed onto the re-imported data:

- **Filters**
- **Sorts**
- **Column reorder, rename, and hide**

These reference columns by name, so they reattach cleanly to the new data.

### What's discarded

**Cell edits** and **row deletions** are **not** carried over. They reference
internal row identities (the `__dat0_rowid` surrogate dat0 assigns at import),
and a re-import regenerates those ids from scratch — so a re-import cannot safely
re-apply them to the right rows.

If the tab has any cell edits or row deletions when you click Refresh, dat0 asks
first:

> **Refresh will discard edits**
> Re-importing discards _N_ cell edit(s) and _M_ row deletion(s). Filters, sorts,
> and column changes will be kept. Continue?
> **[Refresh anyway]**

Choose **Refresh anyway** to proceed, or **Cancel** to keep your current snapshot
untouched (the file on disk has changed, but your tab stays as it is — you can
Refresh later).

If the tab has no edits or deletions, Refresh runs immediately with no prompt.

### Schema drift

If the re-imported file's columns have changed — for example a column a filter or
sort depends on is gone — dat0 won't silently drop rows or corrupt your view.
Instead the refresh lands on the **bare re-imported table** (the new data, no
replayed transforms) and shows a warning banner:

> Refreshed, but some transforms couldn't replay — the file's columns changed.

From there you can re-apply the filters and sorts against the new column set.

## Recovery

dat0 saves your scratch work continuously, and "Save Workspace" promotes that
work into a folder on disk. If either is interrupted — dat0 quits unexpectedly, or
a Save Workspace doesn't finish — the next launch detects the leftover work and
shows a banner:

> **_N_ item(s) to recover from a previous session** — Restore them or discard
> them. **[Review]**

Click **Review** to open the **recovery Sheet**, which lists everything dat0
found, in two groups:

### Orphaned sessions

Scratch work from a session that didn't exit cleanly. Each row names the tables
that were open. For each one:

- **Open** — bring the scratch session back, with its tabs restored.
- **Discard** — permanently remove that orphaned scratch session.

### Interrupted workspaces

A **Save Workspace** that started but didn't finish (the workspace's `.dat0/`
folder is half-written). Each row is labelled with the workspace folder and a
_"(promote didn't finish)"_ note. For each one:

- **Resume** — finish opening the workspace from where it stopped.
- **Discard** — clear the half-finished save.

### What "Discard" removes

Discard only removes the **recovery artifacts** — the orphaned scratch directory,
or the unfinished `.dat0/` subfolder inside an interrupted workspace. It **never**
touches your project folder or your original source files. Discarding an
interrupted workspace leaves the workspace folder and all your data files exactly
where they are; it just clears the incomplete save so dat0 stops offering to
resume it.
