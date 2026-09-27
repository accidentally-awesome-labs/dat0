# Catalog & Inspector

dat0's left **Catalog** dock and right **Inspector** dock give you a structural
view of the window's tables and a one-scan statistical profile of the table in
the active tab.

## Toggling the docks

Both docks are off by default. Toggle them from the **View** menu:

- **View → Toggle Catalog** — show/hide the left Catalog tree.
- **View → Toggle Inspector** — show/hide the right Inspector panel.

Dock visibility is remembered across restarts (persisted in the session at schema
v8).

## Catalog tree

The Catalog lists what the window has open, in three sections:

- **FILES** — the window's tabs: the files it opened, the tables saved from a
  view or a query, and a query's rows. Click a row to bring its tab up; the
  Inspector follows the active tab.
- **CONNECTIONS** — attached databases and their tables (below).
- **PACKAGES** — recently opened `.dat0` packages. Click one to open it
  read-only in a window of its own.

Attached databases are listed under **CONNECTIONS**, one row per database with
its tables below it: open or drop a SQLite file and it is attached, read-only,
under a name made of the file's. A table's row opens it in a tab (a view of the
session's own over the attached table); the database's row folds its tables.
The session remembers the file and attaches it again when it is opened again.

**View → Connections…** (or Settings → MotherDuck → Manage) opens the
Connections panel. **Connect** asks for a MotherDuck token when none is kept
(it is kept in the OS keychain), then lists the account's databases in the
panel and under CONNECTIONS, where their tables open like a SQLite file's.
**Test connection** reports without changing anything; **Disconnect** and
**Forget token** take MotherDuck out of the session (a soft disconnect: the
account's own workspace is not changed). A session opened again with
MotherDuck in it connects again while a token is kept. The panel also lists
the attached SQLite files: **Detach** closes their tables' tabs and forgets
the file, and **Attach SQLite…** picks one.

## Inspector

The Inspector profiles the **active tab's table** in a single pass, off the
window's thread, whenever its pane is open, and shows:

- **Overview** — table name, row count, and column count.
- **Whole table ⇄ Current view toggle** — profile either the stored table or
  what the grid shows: the table with the tab's sorts, filters, edits and
  deleted rows laid over it. dat0 never writes those into the table, so an
  edited value shows in Current view and not in Whole table. The button label
  reflects the active mode; toggling re-profiles.
- **Per-column cards** — for every column: name and type, plus
  - numeric columns: `min · max · μ (mean) · med (median) · σ (std)`,
  - text columns: length stats (`len min–max`),
  - approximate distinct count (HLL — labelled *approx*),
  - null percentage.

  The cards mirror the grid's current **column projection**, not the physical
  table layout: they follow the same order, show renamed columns by their new
  label (as `New name · was <original>`), and move any columns you've hidden into
  a collapsed **"Hidden (N)"** section you can expand. The internal row-id
  surrogate is never shown. This holds in both Whole-table and Current-view modes
  — the toggle changes only which rows are profiled, not which cards appear or
  their order.
- **Inline charts** (inline SVG, Whole table mode):
  - **top-N bars** for low-cardinality columns (the most frequent values), and
  - a **histogram** for numeric high-cardinality columns (16 even-width bins over
    the column's true min/max, counts sampled from the data).
- **Lineage chain (live)** — the table's full ancestry and descendants as a
  clickable chain (see [Lineage](#lineage) below). Built again when the tab
  changes, a table is saved, a chart is saved, or the rows are read again.

### How profiling works

Profiling is built on DuckDB's `SUMMARIZE`, which computes all column statistics
in a single table scan. Whole-table mode runs `SUMMARIZE <table>`; current-view
mode runs `SUMMARIZE (SELECT * FROM <the view the grid reads>)`. Profiling a
1M-row table completes well under the 2-second target (≈85 ms measured on a
typical machine).

### When it profiles again

A Whole table profile is kept until the table's rows are read again: a Live
Refresh of its file, or a console run replacing a query's rows. A sort, a
filter or an edit is laid over the table, not written into it, so it leaves the
stored table, and its profile, as they were.

In Current view mode, every change to what the grid reads profiles again: a
sort, a filter, an edit, a deleted row, **undo/redo**, a Live Refresh.

A display-only column edit — rename, reorder, or hide a column — only re-arranges
the per-column cards to match the new projection; it does not re-profile, since
the underlying data (and so the stats) is unchanged. **Undo/redo** of such an
edit likewise re-projects the cards (and the grid header) without a re-profile.

### Lineage

The Inspector shows the selected table's full lineage as a clickable chain. Its
ancestry — the source files and upstream tables it derives from — is laid out
above, the selected table itself sits in the middle (marked `▸`), and its
descendants — the tables that use it, listed under **Used by** — appear below.
The chain is the full transitive closure in both directions, not just the
immediate neighbours, so you can see everything a table ultimately came from and
everything that ultimately depends on it.

- **Edge labels** name *how* one node feeds another: file imports, transforms
  (annotated with the op count), SQL references — a table named in a derived
  table's `CREATE TABLE AS` SQL, resolved from the query AST via DuckDB's
  `json_serialize_sql` — and charts (a saved chart built from the table).
- **Node glyphs** distinguish the kinds: files (📄), external/attached database
  tables (☁), regular tables (▦), and saved charts (📊).
- **Click any table node** to bring up its tab, opening one if it has none, and
  re-root the Inspector on it — this lets you walk the lineage hop by hop. File
  leaves are not clickable.
- **Click a saved-chart node** (📊) to show that chart as it was saved, over its
  table's tab, in the Charts pane.

### Saved charts

**Save** on the Charts pane's toolbar asks for a name, suggesting one from the
chart's type and axes, and keeps the chart in the session beside the saved
queries; a name already used is replaced. A saved chart joins its table's
lineage. A chart of a query's rows cannot be saved: the rows are a view that
goes with the window, so save them as a table first.

This chain replaces the P6a flat **Dependents** list, which only surfaced
transform children; descendants now include SQL references as well.

## Error banners

Operation feedback (e.g. export success/failure) now surfaces as an inline banner
strip at the top of the window — the banner host that was previously unmounted
(PD-021).
