# dat0

> **dat0** (pronounced "data" / "dat-zero") is a local-first data workbench that scales to terabytes and travels as a single file.

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Platform: macOS · Linux](https://img.shields.io/badge/Platform-macOS_·_Linux-lightgrey.svg)]()

Open any data file or database, edit and transform with full lineage, share the entire workflow as a `.dat0` package anyone can replay, push compute to the cloud only when you choose to.

## Three product pillars

1. **File-native at scale** — Drop a multi-GB Parquet, work like it's a 5 MB CSV. Native DuckDB + GPU-accelerated virtualized grid. No cloud upload, no infrastructure.
2. **Reproducible packaging** — A `.dat0` file bundles data + transforms + queries + UI session + lineage in one attachable artifact. Email it. Replay it on new source data. Diff two of them.
3. **Compute portability** — Same workbench works against a local file, a local database, or an attached MotherDuck workspace.

## Quick start

> **Privacy first:** dat0 is local-first — your data stays on your machine. AI
> features are **off by default** and do nothing until you supply your own API key;
> nothing is proxied through dat0's servers. See the [privacy policy](docs/privacy.md).

### Install

Download the latest signed binary for your platform from
[**GitHub Releases**](https://github.com/accidentally-awesome-labs/dat0/releases):

| Platform | Artifact |
|----------|----------|
| macOS (arm64 + x86_64) | `dat0-<version>-universal.dmg` — mount, drag to Applications |
| Linux x86_64 | `dat0-<version>-x86_64.AppImage` — `chmod +x`, then run |
| Linux aarch64 | `dat0-<version>-aarch64.AppImage` — `chmod +x`, then run |

Or build from source: `cargo build --release` (requires Rust stable and the system
libraries listed in [CONTRIBUTING.md](CONTRIBUTING.md)).

### First run

1. **Launch dat0.** On first launch the enriched hero is shown and the tour carousel
   opens automatically. Click **[ Skip ]** any time, or **[ Get started ]** to
   finish the tour.
2. **Try the demo workspace.** Click **[ ▶ Open demo.dat0 ]** on the hero to open a
   curated Chinook dataset — multi-table SQL, a saved chart, and a pre-filled query
   ready to run. No setup required.
3. **Or drop your own file.** Drag a CSV, Parquet, JSON, or SQLite file onto the
   drop zone. No import wizard, no waiting.

<!--
  Screenshot owed: enriched first-run hero capture.

  To produce it: `cargo run -p dat0-app` against a clean state root (no recents,
  so the enriched hero + tour carousel show), capture the window, and commit the
  PNG as `docs/img/first-run-hero.png` before linking it here.

  This capture is owed by the AX2 manual UAT pass of the production-v1 plan; it
  cannot be produced headlessly. No such image exists in the tree today, so this
  section deliberately carries no image reference rather than a broken link.
-->

> _Screenshot pending — the first-run hero capture is owed by the production-v1
> manual UAT pass (AX2)._

### What you get

- **Native-fast grid** — sort, filter, and inspect millions of rows at 60 fps. No
  cloud upload, no infrastructure.
- **SQL + charts** — full DuckDB SQL with autocomplete, an NL→SQL AI assist chip,
  and one-click bar / line / scatter charts with PNG export.
- **Bring-your-own AI** — add an Anthropic, OpenAI, or OpenRouter key when you want
  AI features; remove it to stay fully local. See
  [AI provider setup](docs/ai-providers.md).

---

## Tech stack (from design spec §3)

- **Language:** Rust 2024
- **UI:** GPUI + longbridge/gpui-component
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
