# dat0

> **dat0** (pronounced "data" / "dat-zero") is a local-first data workbench that scales to terabytes and travels as a single file.

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Status: Pre-Implementation](https://img.shields.io/badge/Status-Pre--Implementation-orange.svg)](docs/specs/2026-04-26-dat0-design.md)
[![Platform: macOS · Linux](https://img.shields.io/badge/Platform-macOS_·_Linux-lightgrey.svg)]()

Open any data file or database, edit and transform with full lineage, share the entire workflow as a `.dat0` package anyone can replay, push compute to the cloud only when you choose to.

## Status

dat0 is **pre-implementation**. The current artifact is the [design specification](docs/specs/2026-04-26-dat0-design.md). Code does not yet exist.

If you want to follow along or contribute to the planning phase, watch this repo and the [GitHub Discussions](https://github.com/accidentally-awesome-labs/dat0/discussions) (when the repo goes public).

## Three product pillars

1. **File-native at scale** — Drop a multi-GB Parquet, work like it's a 5 MB CSV. Native DuckDB + GPU-accelerated virtualized grid. No cloud upload, no infrastructure.
2. **Reproducible packaging** — A `.dat0` file bundles data + transforms + queries + UI session + lineage in one attachable artifact. Email it. Replay it on new source data. Diff two of them.
3. **Compute portability** — Same workbench works against a local file, a local database, or an attached MotherDuck workspace.

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
