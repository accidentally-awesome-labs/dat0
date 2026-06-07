# Catalog & Inspector

dat0's left **Catalog** dock and right **Inspector** dock give you a structural
view of every table in the workspace and a one-scan statistical profile of any
table you select. Both are introduced in P6a.

## Toggling the docks

Both docks are off by default. Toggle them from the **View** menu:

- **View → Toggle Catalog** — show/hide the left Catalog tree.
- **View → Toggle Inspector** — show/hide the right Inspector panel.

Dock visibility is remembered across restarts (persisted in the session at schema
v8).

## Catalog tree

The Catalog groups every table in the current workspace by where it came from:

- **Sources** — tables backed by an imported file (`TableOrigin::File`) or by an
  attached database (`TableOrigin::Attached`, e.g. a MotherDuck workspace database
  or an attached SQLite file).
- **Tables** — local base tables (created in-session, no transform lineage).
- **Derived** — tables produced by a transform pipeline or by a non-trivial
  `CREATE TABLE AS` (SQL-derived).

Each section header shows a live count, e.g. `Tables (4)`. **Click a node** to
open that table in the main grid; selecting it also drives the Inspector.

Attached databases are enumerated per table: after you connect MotherDuck or
attach a SQLite file, that catalog's tables/views appear under **Sources** and
carry their attached origin (this closed the long-standing attach-enumeration
remainder of D-012).

## Inspector

The Inspector profiles the **selected table** in a single pass and shows:

- **Overview** — table name, row count, and column count.
- **Whole table ⇄ Current view toggle** — profile either the full base table or
  the current grid view (the active filter/sort/projection pipeline). The button
  label reflects the active mode; toggling re-profiles.
- **Per-column cards** — for every column: name and type, plus
  - numeric columns: `min · max · μ (mean) · med (median) · σ (std)`,
  - text columns: length stats (`len min–max`),
  - approximate distinct count (HLL — labelled *approx*),
  - null percentage.
- **Inline charts** (drawn as lightweight GPUI quads, no chart library):
  - **top-N bars** for low-cardinality columns (the most frequent values), and
  - a **histogram** for numeric high-cardinality columns (16 even-width bins over
    the column's true min/max, counts sampled from the data).
- **Lineage chain (live)** — the selected table's full ancestry and descendants
  as a clickable chain (see [Lineage](#lineage) below). Updates whenever the
  catalog changes (create/drop/transform).

### How profiling works

Profiling is built on DuckDB's `SUMMARIZE`, which computes all column statistics
in a single table scan. Whole-table mode runs `SUMMARIZE <table>`; current-view
mode runs `SUMMARIZE (<the compiled view SQL>)`. Profiling a 1M-row table
completes well under the 2-second target (≈85 ms measured on a typical machine).

### Live refresh on edits

When you edit the inspected table (cell edits, paste, cut, delete, fill, column
rename/reorder/delete, or applying a transform), the Inspector re-profiles so the
stats, charts, and lineage stay current. Refresh now also fires on
**undo/redo** and on SQL-console grid-binds (this closed the PD-022 follow-up).

### Lineage

The Inspector shows the selected table's full lineage as a clickable chain. Its
ancestry — the source files and upstream tables it derives from — is laid out
above, the selected table itself sits in the middle (marked `▸`), and its
descendants — the tables that use it, listed under **Used by** — appear below.
The chain is the full transitive closure in both directions, not just the
immediate neighbours, so you can see everything a table ultimately came from and
everything that ultimately depends on it.

- **Edge labels** name *how* one node feeds another: file imports, transforms
  (annotated with the op count), and SQL references — a table named in a derived
  table's `CREATE TABLE AS` SQL, resolved from the query AST via DuckDB's
  `json_serialize_sql`.
- **Node glyphs** distinguish the kinds: files (📄), external/attached database
  tables (☁), and regular tables (▦).
- **Click any table node** to open it in a grid tab and re-root the Inspector on
  it — this lets you walk the lineage hop by hop. File leaves are not clickable.

This chain replaces the P6a flat **Dependents** list, which only surfaced
transform children; descendants now include SQL references as well.

## Error banners

Operation feedback (e.g. export success/failure) now surfaces as an inline banner
strip at the top of the window — the banner host that was previously unmounted
(PD-021).
