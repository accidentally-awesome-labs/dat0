# dat0 `.dat0` package format — v1 (normative)

**Status:** Ratified (P8a). **Format version:** 1. **Last updated:** 2026-06-13.

This document is the normative specification for the dat0 `.dat0` *package*
format, version 1. The key words **MUST**, **MUST NOT**, **SHOULD**, and **MAY**
are to be interpreted as in RFC 2119.

The canonical model types live in the `dat0-format` crate
(`crates/dat0-format/src/model.rs`); the published JSON Schema for the manifest
is at [`docs/schemas/dat0-manifest-v1.schema.json`](schemas/dat0-manifest-v1.schema.json).

> A public mirror of this spec at `dat0.dev/format/v1` is a P11 deliverable;
> until then this file in the repo is authoritative.

---

## §1 Scope & terminology

- A **`.dat0` *file*** (a *package*) is the subject of this document: a single,
  **immutable** zip archive that captures a portable, self-describing snapshot
  of a dat0 workspace's data, recipe, sources, views, and saved queries. It is
  the unit of sharing and archival.
- A **`.dat0/` *directory*** (a *workspace*) is a different thing entirely — the
  on-disk, mutable workspace home introduced in P7 (manifest + DuckDB +
  session + lock + lineage). It is **not** a package. Do not conflate them: one
  is a sharable file, the other is a working directory.
- A package is produced by an **exporter** (the dat0 app) and consumed by a
  **reader** (dat0, or any third-party tool — see §11).

---

## §2 Container

A `.dat0` package **MUST** be a ZIP archive.

- JSON entries (`manifest.json`, `recipe.json`, `sources.json`, `views.json`,
  `queries.json`) **MUST** be stored with the `Deflated` compression method.
- `data/*.parquet` entries **MUST** be stored with the `Stored` (no-compression)
  method — Parquet is already columnar-compressed, so re-deflating wastes CPU
  for no size gain.

Required entries (a conforming package **MUST** contain all of these):

| Entry            | Purpose                                              |
|------------------|------------------------------------------------------|
| `manifest.json`  | Package identity, version, checksums (§3).            |
| `recipe.json`    | Canonical table graph (§4). **Authoritative.**       |
| `sources.json`   | Source provenance + replay fingerprints (§5).         |
| `views.json`     | Per-table display overlays (§6).                      |
| `queries.json`   | Saved SQL queries (§6).                               |
| `data/`          | One Parquet file per recipe table (§7).               |

A reader **MUST** reject a package that is missing `manifest.json`. A reader
**SHOULD** reject a package missing any other required entry, surfacing a clear
diagnostic.

In-archive paths use forward slashes (e.g. `data/sales.parquet`).

---

## §3 `manifest.json`

`manifest.json` is the package's identity card. Its shape is fixed by the
published JSON Schema:
[`docs/schemas/dat0-manifest-v1.schema.json`](schemas/dat0-manifest-v1.schema.json)
(JSON Schema draft 2020-12; `additionalProperties: false` for the ratified v1
shape).

| Field            | Type             | Notes                                                                 |
|------------------|------------------|-----------------------------------------------------------------------|
| `format_version` | integer          | **MUST** be `1`.                                                       |
| `kind`           | string           | **MUST** be `"package"`.                                               |
| `dat0_version`   | string           | The producing dat0 build version.                                     |
| `package_id`     | string (uuid)    | UUID v7 (time-ordered) identifying this package.                      |
| `workspace_id`   | string (uuid)    | UUID of the origin workspace (provenance only).                       |
| `created_at`     | string (RFC3339) | Package creation timestamp.                                           |
| `table_count`    | integer          | Number of tables in the recipe.                                       |
| `checksums`      | object           | Map of in-archive entry path → `"sha256:<hex>"`.                      |

`checksums` **MUST** include an entry for **every** `data/*.parquet` file and
for `recipe.json`, each value formatted as `sha256:<lowercase-hex>`. A reader
**MUST** verify these checksums and **MUST** refuse a package on mismatch
(reporting the offending entry).

### Example `manifest.json`

```json
{
  "format_version": 1,
  "kind": "package",
  "dat0_version": "0.1.0",
  "package_id": "0192f1b2-3c4d-7e8f-9a0b-1c2d3e4f5a6b",
  "workspace_id": "0192f1a0-1111-7222-8333-444455556666",
  "created_at": "2026-06-13T00:00:00Z",
  "table_count": 2,
  "checksums": {
    "recipe.json": "sha256:3a7bd3e2360a3d29eea436fcfb7e44c735d117c42d1c1835420b6b9942dd4f1b",
    "data/sales.parquet": "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
    "data/monthly.parquet": "sha256:2c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae"
  }
}
```

---

## §4 `recipe.json` (canonical)

`recipe.json` is the **authoritative** description of the package's table graph.
The materialized Parquet in `data/` is a regenerable *cache* of what the recipe
describes; on any disagreement, the recipe is the source of truth.

`recipe.json` is `{ "tables": [ RecipeTable, … ] }`. Each `RecipeTable`:

| Field        | Type                    | Notes                                                          |
|--------------|-------------------------|----------------------------------------------------------------|
| `id`         | string                  | Stable table id, e.g. `"t_sales"`.                             |
| `name`       | string                  | DuckDB table name.                                            |
| `kind`       | `"base"` \| `"derived"` | Table kind.                                                   |
| `schema`     | array of column         | `{ "name", "type" }` per column; `type` is a DuckDB type literal. |
| `row_count`  | integer (u64)           | Row count at export.                                          |
| `data`       | string                  | In-archive path, `"data/<name>.parquet"`.                    |
| `source_ref` | string (optional)       | **Base tables MUST carry this** → `PackageSource.id` (§5).    |
| `derivation` | object (optional)       | **Derived tables MUST carry this** (§4.1).                   |

A `base` table **MUST** carry `source_ref` and **MUST NOT** carry `derivation`.
A `derived` table **MUST** carry `derivation` and **MUST NOT** carry
`source_ref`.

### §4.1 `derivation`

`derivation` is a tagged object (`kind` discriminator):

- `{ "kind": "sql", "sql": "<SQL>", "parents": ["<table>", …] }` — derived by a
  SQL statement referencing the listed parent tables.
- `{ "kind": "transform", "parent": "<table>", "ops": [ Transformation, … ] }` —
  derived by applying a stack of dat0 transform ops to a single parent. `ops`
  uses the engine's self-describing `Transformation` wire format (each op is a
  tagged JSON object keyed on `kind`; see `dat0-engine`'s `transform` module).

---

## §5 `sources.json`

`sources.json` is `{ "sources": [ PackageSource, … ] }`. Each `PackageSource`
records the provenance and the *replay fingerprint* of an imported source:

| Field                | Type            | Notes                                                       |
|----------------------|-----------------|-------------------------------------------------------------|
| `id`                 | string          | e.g. `"src_sales"`; referenced by base tables' `source_ref`.|
| `logical_name`       | string          | e.g. `"sales.csv"`.                                          |
| `original_uri`       | string          | Informational only (where it was imported from).            |
| `schema_fingerprint` | array of column | `{ "name", "type" }`; drives replay compatibility (§8).     |
| `content_hash`       | string          | sha256 of the source bytes at export time.                  |
| `row_count`          | integer (u64)   | Source row count at export.                                  |

`schema_fingerprint` is the column-name/type set the recipe was built against;
it is the contract a replacement source is checked against in §8.

---

## §6 `views.json` / `queries.json`

`views.json` is `{ "views": [ PackageView, … ] }`. Each `PackageView` is the
portable subset of an app session tab:

| Field             | Type                       | Notes                                          |
|-------------------|----------------------------|------------------------------------------------|
| `table_name`      | string                     | The table this view is over.                  |
| `transform_stack` | array of `Transformation`  | Display/transform overlay (default `[]`).      |
| `undo_cursor`     | integer                    | Position within the stack (default `0`).       |

Views are **overlays**: a reader restores a view by re-applying `transform_stack`
through the producing engine (dat0's DuckDB-backed view compiler). They are not
materialized separately in `data/`.

`queries.json` is `{ "queries": [ PackageQuery, … ] }`, each
`{ "id": <uuid>, "name": <string>, "sql": <string>, "saved_at": <i64> }`. Saved
queries are informational and **SHOULD** be restored into the saved-queries list
on unpack.

---

## §7 Data files

`data/` holds one Apache Parquet file per `RecipeTable`, named
`<RecipeTable.name>.parquet`. These files:

- **MUST** be written by DuckDB `COPY … TO (FORMAT PARQUET)`.
- **MUST** be readable by any standard Parquet/Arrow consumer (the format is
  cross-language; see §11). dat0 reads them via DuckDB `read_parquet`.

The P8 T0 spike (`docs/internal/2026-06-13-p8-spike-notes.md` §S1) verified that
this round trip preserves type fidelity with no widening: `DECIMAL(p,s)` keeps
its precision/scale, `DATE` does not promote to `TIMESTAMP`, `BIGINT` and
`TIMESTAMP` are unchanged, and an all-NULL row does not perturb inferred types.
No explicit CAST-pinning projection is therefore required by the writer.

---

## §8 Replay compatibility

When a reader replays a package against a **replacement** source (e.g. a fresh
extract of the same data), it checks the replacement against the recipe's
**referenced columns** — the structural compatibility rule:

- A replacement source **MUST** provide every column the recipe references for
  that source, matched **by name**, each with a **type-compatible** DuckDB type.
- Extra columns in the replacement are **ignored**.
- Column **order is irrelevant**.

If the replacement satisfies the rule, replay proceeds. Otherwise the reader
**MUST** refuse the replay and surface a **schema diff** identifying the missing
or type-incompatible columns. (The fingerprint to check against is the source's
`schema_fingerprint`, §5.)

---

## §9 Versioning & compatibility

Format versioning is semantic: **dat0 1.x reads format 1.x.**

- On read, a reader **MUST** ignore unknown JSON fields (forward compatibility at
  the serde field level), and missing **optional** fields default.
- A package whose `format_version` **major** exceeds the reader's supported major
  **MUST** be refused with a clear message (e.g. "this package needs a newer
  dat0").
- The published JSON Schema (§3) is intentionally **strict**
  (`additionalProperties: false`) for the ratified v1 manifest shape; runtime
  forward-compatibility is provided by the serde reader, not by the published
  schema.

---

## §10 Non-goals (v1)

The v1 format deliberately does **not** provide: encryption, signing,
delta/incremental packaging, or merge of two packages. These are out of scope
for this version.

---

## §11 Cross-language reading

A `.dat0` package is readable without dat0 or Rust:

- **Data** is plain Apache Parquet (§7) — readable by Arrow, pandas, Polars,
  DuckDB, Spark, etc.
- **Metadata** is self-describing tagged JSON. Following the PD-014 tagged-wire
  convention (see `docs/deferrals.md` and the `dat0-engine` `transform` module
  docs), every polymorphic node carries an explicit `kind`/`tag` discriminator:
  `manifest.kind == "package"`, each `derivation.kind ∈ {sql, transform}`, and
  each `Transformation` op is internally tagged on `kind`. A non-Rust reader can
  therefore **route on these discriminators** without any Rust-side type
  knowledge — no `#[serde(untagged)]` ambiguity to disambiguate.

### Example `recipe.json`

One base table (`sales`, imported from a CSV source) and one derived SQL table
(`monthly`):

```json
{
  "tables": [
    {
      "id": "t_sales",
      "name": "sales",
      "kind": "base",
      "schema": [
        { "name": "order_id", "type": "BIGINT" },
        { "name": "amount", "type": "DECIMAL(9,2)" },
        { "name": "ordered_at", "type": "DATE" }
      ],
      "row_count": 10000,
      "data": "data/sales.parquet",
      "source_ref": "src_sales"
    },
    {
      "id": "t_monthly",
      "name": "monthly",
      "kind": "derived",
      "schema": [
        { "name": "month", "type": "DATE" },
        { "name": "total", "type": "DECIMAL(18,2)" }
      ],
      "row_count": 12,
      "data": "data/monthly.parquet",
      "derivation": {
        "kind": "sql",
        "sql": "SELECT date_trunc('month', ordered_at) AS month, SUM(amount) AS total FROM sales GROUP BY 1 ORDER BY 1",
        "parents": ["sales"]
      }
    }
  ]
}
```
