# dat0

> **dat0** (pronounced "data" / "dat-zero") is a local-first data workbench that scales to terabytes and travels as a single file.

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Platform: macOS · Linux](https://img.shields.io/badge/Platform-macOS_·_Linux-lightgrey.svg)]()

Open any data file or database, edit and transform with full lineage, share the entire workflow as a `.dat0` package anyone can replay, push compute to the cloud only when you choose to.

## Three product pillars

1. **File-native at scale** — Drop a multi-GB Parquet, work like it's a 5 MB CSV. Native DuckDB + a virtualized grid. No cloud upload, no infrastructure.
2. **Reproducible packaging** — A `.dat0` file bundles data + transforms + queries + UI session + lineage in one attachable artifact. Email it. Replay it on new source data. Diff two of them.
3. **Compute portability** — Same workbench works against a local file, a local database, or an attached MotherDuck workspace.

## Status

**Pre-release.** No binary has been published yet. The engine, the `.dat0`
package format and the command line are complete and tested; the desktop UI is
being reconnected to them after its move from GPUI to Dioxus (tracked as
PD-023 in [`docs/deferrals.md`](docs/deferrals.md)).

| Works in the current build | Being reconnected |
|---|---|
| Opening CSV, TSV, JSON, Parquet and SQLite — file picker, drag-and-drop, command line — into a virtualized grid, and the import wizard for a CSV whose delimiter or encoding needs asking | |
| MotherDuck, with your token: the account's databases listed and their tables opened | |
| The SQL console: running a query into the grid, history, saved queries, completion | |
| AI assist, with your own key: SQL written from a question, and a statement explained, from the schema alone | |
| Sort, filter, cell edits, undo, export and Live Refresh | |
| Charts of the active tab, drawn from its filters and edits, saved, and exported as PNG or SVG | |
| The inspector: a profile of the active tab's table, its columns' small charts, and its lineage | |
| Workspaces — open, save, recent — and the demo workspace; a closed or crashed window's work comes back | |
| Help → Check for Updates, against the signed release manifest (no release is published yet) | |
| Crash reports, opt-in: offered at the launch after a crash, and Help → Report a Bug | |
| `.dat0` packages: open read-only, unpack, export and replay, in the app and with `dat0 inspect` / `unpack` / `replay` / `diff` / `export` | |

## Quick start

> **Privacy first:** dat0 is local-first — your data stays on your machine. AI
> features are **off by default** and do nothing until you supply your own API key;
> nothing is proxied through dat0's servers. See the [privacy policy](docs/privacy.md).

### Install

There is no release yet. Signed builds will be published on
[**GitHub Releases**](https://github.com/accidentally-awesome-labs/dat0/releases)
from the first beta:

| Platform | Artifact |
|----------|----------|
| macOS (arm64 + x86_64) | `dat0-<version>-universal.dmg` — mount, drag to Applications |
| Linux x86_64 | `dat0-<version>-x86_64.AppImage` — `chmod +x`, then run |

Linux aarch64 is planned but not yet built by the release pipeline.

Until then, build from source: `cargo build --release` (requires the pinned Rust
toolchain and the system libraries listed in [CONTRIBUTING.md](CONTRIBUTING.md)).

### First run

1. **Launch dat0.** On first launch the enriched hero is shown and the tour carousel
   opens automatically. Click **[ Skip ]** any time, or **[ Get started ]** to
   finish the tour.
2. **Try the demo workspace.** Click **[ ▶ Open demo.dat0 ]** on the hero to open a
   curated Chinook dataset — multi-table SQL, a saved chart, and a pre-filled query
   ready to run.
3. **Or drop your own file.** Drag a CSV, TSV, JSON, Parquet or SQLite file onto
   the drop zone. Most files open at once; a CSV whose delimiter or encoding
   dat0 cannot settle opens in a short import wizard first. A SQLite file is
   attached read-only, with its tables listed under CONNECTIONS.

<!--
  Screenshot owed: enriched first-run hero capture.

  To produce it: `cargo run --bin dat0` against a clean state root (no recents,
  so the enriched hero + tour carousel show), capture the window, and commit the
  PNG as `docs/img/first-run-hero.png` before linking it here.

  This capture is owed by the AX2 manual UAT pass of the production-v1 plan; it
  cannot be produced headlessly. No such image exists in the tree today, so this
  section deliberately carries no image reference rather than a broken link.
-->

> _Screenshot pending — the first-run hero capture is owed by the production-v1
> manual UAT pass (AX2)._

### What you get

The v1 feature set — see [Status](#status) for what the current build does today.

- **Native-fast grid** — sort, filter, and inspect millions of rows at 60 fps. No
  cloud upload, no infrastructure.
- **SQL + charts** — full DuckDB SQL with autocomplete, an NL→SQL AI assist chip,
  and one-click bar / line / scatter charts with PNG export.
- **Bring-your-own AI** — add an Anthropic, OpenAI, or OpenRouter key when you want
  AI features; remove it to stay fully local. See
  [AI provider setup](docs/ai-providers.md).

---

## Tech stack

The design spec's §3 still names the original GPUI renderer; the UI moved to
Dioxus in August 2026 (see
[the migration log](docs/internal/2026-08-09-gpui-to-dioxus-migration-log.md)).

- **Language:** Rust 2024
- **UI:** Dioxus 0.7 (desktop), rendering into a wry/WebKit webview
- **Engine:** DuckDB native via the `duckdb` crate
- **Wire format:** Apache Arrow (record batches, in-process)
- **Async:** tokio
- **Targets:** macOS arm64 + x86_64; Linux x86_64 + aarch64

## What dat0 deliberately is not

- Not a BI tool (no dashboards, no scheduled reports).
- Not a notebook (code cells are not the primary surface).
- Not a database client (file-first, DB-attach is secondary).
- Not cloud-only / not cloud-required.

## Documentation

- [**Design specification**](docs/specs/2026-04-26-dat0-design.md) — full v1 design including phasing, gates, risks, and `.dat0` format
- [Contributing](CONTRIBUTING.md)
- [Code of Conduct](CODE_OF_CONDUCT.md)
- [Security policy](SECURITY.md)
- [Third-party notices](NOTICE.md)

## License

Apache License 2.0. See [LICENSE](LICENSE).

Copyright 2026 Accidentally Awesome Labs.
