# dat0 `.dat0` packages — user guide

A **`.dat0` package** is an immutable, shareable snapshot of a dat0 workspace:
one file you can send to a colleague, attach to an issue, or archive. This guide
covers what a package is, how to create and open one in the app, and the
headless `dat0` CLI for scripting and CI.

For the normative on-disk format (the zip layout, the JSON schemas, the replay
compatibility rule in RFC-2119 terms), see
[`docs/dat0-format-v1.md`](dat0-format-v1.md).

---

## Package vs. workspace — two different things

These look similar but are not the same:

| | `.dat0` **package** (a *file*) | `.dat0/` **workspace** (a *directory*) |
|---|---|---|
| What | An **immutable, shareable** zip archive | A **mutable** working home on disk (P7) |
| Contains | Parquet data cache + canonical recipe / sources / views / queries JSON | A live DuckDB database + session + lock |
| Use | Sharing, archival, replay, diffing | Day-to-day editing in the app |
| Filename | `report.dat0` (a single file) | `report/.dat0/` (a subdirectory of a project folder) |

A package is a **snapshot you share**; a workspace is the **folder you work in**.
You *export* a workspace to a package, and *unpack* a package back into a fresh
workspace.

### What's inside a package

- **Data** — one Apache Parquet file per table (the cached rows). Parquet is
  readable by Arrow, pandas, Polars, DuckDB, Spark, and more (see
  [format spec §11](dat0-format-v1.md)).
- **Recipe** — the canonical table graph: each table's schema, row count, and
  whether it's a **base** table or a **derived** table (with the SQL / transform
  that produces it and its parent tables).
- **Sources** — provenance for base tables imported from files: the logical
  source name, schema fingerprint, and content hash used for **replay**.
- **Views** — per-table display overlays (filters / sorts / column order) from
  your open tabs.
- **Saved queries** — your saved SQL.

The metadata is self-describing tagged JSON, so a package is readable without
dat0 or Rust.

---

## In the app

All package actions live under the **File** menu.

- **File → Export .dat0 Package** — write the current workspace to a `.dat0`
  file you choose. Exporting from a **live session** captures the full
  recipe, including derived tables and their lineage (see the known limitation
  below).
- **File → Open .dat0 Package** — open a package **read-only** to inspect it (see
  below).
- **File → Unpack .dat0 Package** — materialize a package into a fresh workspace
  directory you can edit.
- **File → Replay .dat0 Package** — rebuild a package's derived tables against a
  fresh source file (see *Replay* below).

### Read-only Inspect mode

**Open .dat0 Package** opens the package in a **read-only** shell: you can browse
the tables, schema, profiles, lineage, and saved queries, but **edits are
refused** — a package is immutable. To make changes, use **File → Unpack** to
get an editable workspace, then work in that.

---

## CLI reference

The `dat0` binary doubles as a headless tool. When the first argument is a
package verb (`export`, `unpack`, `inspect`, `replay`, `diff`), dat0 runs that
command and exits — no window opens. Any other invocation launches the GUI.

### `dat0 export` — workspace → package

```sh
dat0 export <workspace-dir> -o out.dat0
```

Opens the workspace directory, materializes every table to Parquet, and writes
the package to `out.dat0`.

### `dat0 inspect` — print a package's recipe

```sh
dat0 inspect <pkg.dat0>          # human-readable tree
dat0 inspect <pkg.dat0> --json   # machine-readable JSON
```

Lists the tables (name, kind, row/column counts), the lineage edges
(`derived <- parent`), and the saved queries. No engine is started — this reads
metadata only, so it's instant.

### `dat0 unpack` — package → workspace

```sh
dat0 unpack <pkg.dat0> <dir>
```

Materializes the package into a fresh `.dat0/` workspace under `<dir>`, ready to
open in the app (or to re-export).

### `dat0 replay` — rebuild against fresh sources

```sh
dat0 replay <pkg.dat0> --source <logical=path> [--source <logical=path>...] [-o out.dat0]
```

Rebinds one or more of the package's base sources to **new files** and re-runs
the derived tables on top of the fresh data, writing a new package. The
`logical` name is the source's logical name as shown by `inspect` (for a
file-imported table it's the original filename, e.g. `sales.csv`). Repeat
`--source` for multiple sources. With no `-o`, the output defaults to
`<pkg>-replayed.dat0` next to the original.

```sh
# Re-run a monthly report against this month's extract:
dat0 replay report.dat0 --source sales.csv=./2026-06-sales.csv -o report-june.dat0
```

### `dat0 diff` — compare two packages

```sh
dat0 diff <a.dat0> <b.dat0>          # human-readable
dat0 diff <a.dat0> <b.dat0> --json   # machine-readable JSON
```

Compares two packages across four dimensions — **schema** (columns
added/removed/retyped), **lineage** (tables added/removed, derivation changed),
**row counts**, and **saved queries** — matched by name. The diff is metadata-only
(it never reads the Parquet data), so it's fast.

#### Exit codes

`dat0 diff` follows the Unix `diff(1)` convention, which makes it scriptable:

| Code | Meaning |
|------|---------|
| `0`  | No differences (the packages are recipe-equivalent). |
| `1`  | Differences found. |
| `2`  | Error (e.g. a package couldn't be opened). |

```sh
# Fail a CI job if a regenerated package drifted from the committed one:
dat0 diff committed.dat0 fresh.dat0 || echo "package changed!"
```

The other verbs exit `0` on success and `2` on error.

---

## Replay compatibility rule

Replay checks each replacement source **structurally**, against the columns the
recipe actually **references**:

- The replacement **must provide every referenced column by name**, each with a
  **type-compatible** DuckDB type.
- **Extra** columns in the replacement are **ignored**.
- Column **order does not matter**.

If a replacement is missing a referenced column (or one has an incompatible
type), replay is **refused** with a schema diff identifying the problem — it
never silently produces wrong data. See
[format spec §8](dat0-format-v1.md) for the normative wording.

---

## Known limitation — cold CLI export flattens derived tables

> **`dat0 export` of a workspace *directory* (on disk) flattens derived tables
> to base tables.**

Derived-table provenance — the SQL/transform that produces a table and the link
to its parents — currently lives **only in memory** while a workspace is open in
the app; it is **not persisted** across a workspace reopen. Because the CLI
`dat0 export` reopens the workspace from disk before exporting, every table is
classified as a plain **base** table, and the resulting package is **data-only**
(no replayable recipe / lineage).

The **in-app Export Package** (from a live session) does **not** have this
limitation — it exports straight from the running session, so the derived
recipe and lineage are preserved, and `inspect` / `replay` work as expected.

**Practical guidance:** if you need a replayable package (one whose derived
tables can be re-run against fresh sources), export it **from the app** rather
than via the cold CLI. A CLI-exported package still round-trips its data
faithfully — it just records every table as base.

This is tracked as deferral **D-025** in
[`docs/deferrals.md`](deferrals.md); the fix is to persist `table_origins`
(into `session.json` or a `.dat0/` sidecar) and restore it on workspace reopen.
