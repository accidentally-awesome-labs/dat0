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
- **Dependents (live)** — the tables derived (via a transform) from the selected
  table. This list updates whenever the catalog changes (create/drop/transform).

### How profiling works

Profiling is built on DuckDB's `SUMMARIZE`, which computes all column statistics
in a single table scan. Whole-table mode runs `SUMMARIZE <table>`; current-view
mode runs `SUMMARIZE (<the compiled view SQL>)`. Profiling a 1M-row table
completes well under the 2-second target (≈80 ms on a typical machine).

### Live refresh on edits

When you edit the inspected table (cell edits, paste, cut, delete, fill, column
rename/reorder/delete, or applying a transform), the Inspector re-profiles so the
stats, charts, and dependents stay current. (Refresh on **undo/redo** and on
SQL-console grid-binds is tracked as a follow-up — see PD-022.)

## Error banners

Operation feedback (e.g. export success/failure) now surfaces as an inline banner
strip at the top of the window — the banner host that was previously unmounted
(PD-021).
