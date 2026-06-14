# P8 T0 — spike notes (.dat0 format + Package mode)

**Date:** 2026-06-13
**Toolchain:** rustc 1.95.0 (59807616e 2026-04-14); duckdb-rs 1.4.4 (bundled).
**Platform exercised:** macOS-arm (Apple Silicon).
**Purpose:** gate the three P8 unknowns BEFORE any real code — (S1) Parquet
type-fidelity round-trip, (S2) `zip` crate API for the `.dat0` container,
(S3) CLI front-door dispatch (branch before GPUI/AppLock).

---

## S1 — Parquet type-fidelity round-trip — **PASS**

Test: `crates/dat0-engine/tests/spike_parquet_roundtrip.rs` (kept as a real,
permanent test). Writes a 2-row table with the P6a gotcha types — `BIGINT`,
`DECIMAL(9,2)`, `DATE`, `TIMESTAMP` plus an all-NULL second row — through
`export_query_to_path(ExportFormat::Parquet)`, re-reads via
`read_parquet(...)` into a VIEW, and asserts the DuckDB DESCRIBE types.

**Observed `describe_table("rt")` types vector (verbatim):**

```
[("id", "BIGINT"), ("amt", "DECIMAL(9,2)"), ("d", "DATE"), ("ts", "TIMESTAMP")]
```

**Verdict: every type survived the round-trip with NO widening.**
- `DECIMAL(9,2)` came back as exactly `DECIMAL(9,2)` — it did **not** widen to
  `DECIMAL(38,…)`.
- `DATE` stayed `DATE` — it did **not** promote to `TIMESTAMP`.
- `BIGINT` and `TIMESTAMP` unchanged.
- The NULL row did not perturb inferred types.
- Scalar check: `SELECT amt::TEXT FROM rt WHERE id = 1` returns `"25.50"`
  (scale preserved). Scalar extraction copied from the house `scalar(...)`
  helper in `register_parquet.rs` (downcast `batches[0].column(0)` to
  `duckdb::arrow::array::StringArray`) — no new public API added.

**Implication for T2:** **No CAST-pinning projection is required.**
DuckDB's native Parquet writer/reader preserve DECIMAL precision/scale and
DATE vs TIMESTAMP faithfully on this round trip. T2's writer can use a plain
`SELECT * FROM …` (or the existing `render_export_select` projection) without
an explicit `SELECT … CAST(...)` type-pin. (If a future fixture ever surfaces a
widening case — e.g. a much larger DECIMAL precision or a TIME/INTERVAL type
not exercised here — revisit; the guard test will catch a regression on these
four types.)

Run command (expected PASS):

```
cargo test -p dat0-engine --test spike_parquet_roundtrip -- --nocapture
# test result: ok. 1 passed
```

---

## S2 — `zip` crate API — compile-proven (throwaway scratch, now deleted)

Proven in a throwaway `cargo` binary under the system temp dir (OUTSIDE the
workspace, so no workspace `Cargo.toml` was touched); scratch deleted after.

**Resolved version: `zip = "8.6.0"`** (locked `zip 8.6.0`).

**Pin recommendation for T1 (lean — pure-Rust deflate, no system zlib, no
zstd/lzma/bzip2):**

```toml
zip = { version = "8.6.0", default-features = false, features = ["deflate-flate2-zlib-rs"] }
```

This compiled and round-tripped byte-identically. Notes on features:
- `default-features = false` drops the heavy optional codecs (`zstd`, `lzma`,
  `xz`, `ppmd`, `bzip2`) we don't need for a Stored-Parquet / Deflated-JSON
  container.
- The bare `deflate-flate2` feature does **not** compile alone — `flate2`
  errors `No compression backend selected`. You must pick a backend; we used
  `deflate-flate2-zlib-rs` (pure-Rust `zlib-rs 0.6.3` backend → no C/system
  zlib dependency). `Stored` needs no compression backend at all, but JSON
  entries use `Deflated`, so the backend feature is required.
- If T1 prefers zero ceremony, plain `zip = "8.6.0"` (default features) also
  works but pulls in zstd/lzma/etc. Prefer the lean pin above.

**Exact API that compiled (zip 8.6.0):**

Imports:
```rust
use std::io::{Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};
```

Write side:
```rust
let file = std::fs::File::create(&zip_path)?;
let mut zw = ZipWriter::new(file);

// Parquet entries → Stored (already compressed columnar data).
let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
zw.start_file("data/x.parquet", stored)?;
zw.write_all(parquet_bytes)?;

// JSON / manifest entries → Deflated.
let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
zw.start_file("manifest.json", deflated)?;
zw.write_all(json_bytes)?;

zw.finish()?;   // returns the inner File; must be called to flush central dir.
```

Read side:
```rust
let file = std::fs::File::open(&zip_path)?;
let mut archive = ZipArchive::new(file)?;

let mut buf = Vec::new();
archive.by_name("data/x.parquet")?.read_to_end(&mut buf)?;   // identical bytes
```

Key API facts for T1/T2:
- `SimpleFileOptions` lives at `zip::write::SimpleFileOptions`;
  `.compression_method(CompressionMethod::Stored | Deflated)` is the builder
  call (type-state builder, `default()` then `.compression_method(...)`).
- `CompressionMethod::{Stored, Deflated}` are the two variants we use.
- `ZipWriter::new(File)` → `start_file(name, opts)` → `write_all(bytes)` per
  entry → `finish()`.
- `ZipArchive::new(File)` → `by_name(&str)` returns a `ZipFile` impl `Read`;
  `read_to_end` recovers byte-identical content (asserted for both Stored and
  Deflated entries).
- `start_file` takes the in-archive path with forward slashes
  (`"data/x.parquet"`), creating nested dirs implicitly.

Run output: `zip spike ok: stored+deflated round-trip byte-identical`.

---

## S3 — CLI front-door dispatch — **confirmed (branch before GPUI/AppLock)**

Goal: prove a subcommand can short-circuit and exit BEFORE
`AppContext::boot()`-driven window creation, `AppLock::try_acquire`, and the
GPUI `Application::run`. T4 will hang the real `dat0 <subcommand>` dispatch off
this anchor.

**Anchor — file:line + surrounding code (`crates/dat0-app/src/main.rs`):**

`main`'s real signature is:
```rust
fn main() -> anyhow::Result<()> {   // imported as `use anyhow::Result;` → `fn main() -> Result<()>`
```

Insertion point: **immediately after the `cli_paths` collection (currently
line 15) and BEFORE `AppLock::try_acquire` (currently line 16).** Surrounding
code as-is:
```rust
    let state_dir = dat0_app::platform::data_dir()?;
    let cli_paths: Vec<std::path::PathBuf> = std::env::args().skip(1).map(Into::into).collect();
    // <-- T4 dispatch block goes HERE (between cli_paths and the lock) -->
    let lock = match AppLock::try_acquire(&state_dir)? {
```

Temporary spike block used (then reverted):
```rust
    if std::env::args().nth(1).as_deref() == Some("selftest") {
        println!("dat0 cli selftest ok");
        return Ok(());        // matches main's anyhow::Result<()> via the `Result` alias
    }
```

**Results:**
- `cargo run -p dat0-app -- selftest` → printed `dat0 cli selftest ok`,
  process exit code **0**, **no window opened**, and it short-circuited before
  `AppLock::try_acquire` and GPUI (confirmed: no `dat0 starting` /
  WindowRegistry log lines that the GUI path emits).
- `dat0` (no subcommand) → GUI still launches normally: logs `dat0 starting`,
  applies migrations, builds the engine, and registers the first window in the
  WindowRegistry; process stays alive (killed after confirming launch).

**Caveat for T4:** `init_logging()` (line 6) and `AppContext::boot()` (line 7)
run BEFORE the `cli_paths` line, so a subcommand inserted at this anchor still
pays for logging init + the AppContext boot (engine/scratch setup). That is the
correct trade for the gate's purpose (short-circuit before AppLock + GPUI
window + the event loop). If T4 wants a subcommand that avoids even the
AppContext boot cost, it would need to branch earlier (right after
`init_logging`, before `AppContext::boot`) — but the boot is cheap and the
plan's anchor (post-`cli_paths`) is the right place for a path-aware
subcommand that may want `state_dir` / `cli_paths` in scope.

**`main.rs` was reverted to a no-diff state** after the spike
(`git diff crates/dat0-app/src/main.rs` empty).

---

## Gate summary

| Spike | Unknown | Verdict |
|---|---|---|
| S1 | Parquet preserves DECIMAL/DATE/TIMESTAMP/NULL? | **PASS** — no widening, no CAST-pin needed in T2 |
| S2 | `zip` API for `.dat0` container | **PROVEN** — pin `zip 8.6.0`, `deflate-flate2-zlib-rs`; `SimpleFileOptions` / `Stored` / `Deflated` / `by_name` as above |
| S3 | Subcommand short-circuit before GPUI/AppLock | **CONFIRMED** — anchor between `cli_paths` and `AppLock::try_acquire`, `main` is `anyhow::Result<()>` |

All three gates green → T1 may proceed.
