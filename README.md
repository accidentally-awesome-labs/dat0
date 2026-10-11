# dat0

> **dat0** (pronounced “data” / “dat-zero”) is a local-first data workbench powered by Rust and DuckDB.

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![CI](https://github.com/accidentally-awesome-labs/dat0/actions/workflows/ci.yml/badge.svg)](https://github.com/accidentally-awesome-labs/dat0/actions/workflows/ci.yml)

Open CSV, TSV, JSON, Parquet and SQLite data, explore it in a paged virtualized grid, and work with SQL, filters, edits and charts. Keep mutable work in a workspace, or share data and recorded recipes/lineage, view overlays, saved queries and charts in a `.dat0` package. MotherDuck and bring-your-own-key AI are optional.

## Current status

**Pre-release: no published binary is available yet.** Build from source today; [GitHub Releases](https://github.com/accidentally-awesome-labs/dat0/releases) is the future distribution location. Packaging and release procedures are in the [release runbook](docs/release-runbook.md).

| Current workflows | Limitations |
|---|---|
| Open supported files through the picker, drag-and-drop or command line; attach SQLite read-only | The import wizard configures UTF-8 dialect and column options; it does not convert unsupported encodings |
| Run SQL, use completion and history, and save query documents | PRAGMA and EXPLAIN execute but their rows are not displayed |
| Sort, filter, edit, undo, export and refresh file-backed tables | One edit is limited to 10,000 cells |
| Inspect tables, columns and lineage; create, save and export charts | |
| Open/save workspaces and review recoverable scratch sessions | Closing scratch does not offer to save; transient query results must be rerun after recovery |
| Open packages read-only; unpack, export, replay and compare packages in the app or CLI | Launching a package or workspace can also leave an extra empty scratch window |
| Use recent workspaces and single-instance launch forwarding | Native Open Recent menus are snapshots taken at launch |
| Connect to MotherDuck or request AI SQL assistance with your own credentials | These chosen services send requests outside your machine |
| Check for updates and opt in to crash reporting | There is no published release to update to yet |

## Source quick start

Install the platform prerequisites and pinned **Rust 1.97.0** described in [CONTRIBUTING.md](CONTRIBUTING.md), then run from the repository root:

```sh
cargo build --locked --release -p dat0-ui --bin dat0
./target/release/dat0
```

Ordinary source builds need neither cloud credentials nor signing keys. The [contribution guide](CONTRIBUTING.md) contains the development build, SQLite extension-cache priming, isolated nextest and doctest commands, plus scratch-profile setup and troubleshooting.

## First run

1. Launch dat0 and finish or skip the first-run tour.
2. Open the bundled demo from the start screen to explore tables, SQL and a saved chart.
3. Open or drop a CSV, TSV, JSON, Parquet or SQLite file. For text imports, the wizard lets you review dialect and column options. SQLite tables appear under connections and are attached read-only.
4. Save a workspace to continue editing later, or export a `.dat0` package to share a recorded workflow. Packages can contain sensitive data, SQL and source paths: review them before sharing.

Local file workflows do not automatically upload tables. AI sends your prompt or Explain SQL text and schema to the selected provider; the desktop currently attaches no automatic sample rows. Arbitrary text you enter can contain private data. Crash submission is default-off, while launch update checks are default-on; extension downloads, selected remote samples and chosen MotherDuck/AI services have their own traffic. Read the [privacy guide](docs/privacy.md) and [AI provider guide](docs/ai-providers.md) before enabling remote services.

## Documentation, contributions and help

- [Documentation index](docs/README.md): current user guides, contributor references and maintainer procedures.
- [Contributing](CONTRIBUTING.md): setup, change-specific checks, review and DCO sign-off.
- [Architecture](docs/architecture.md): crate boundaries, startup and persistence.
- [Support](SUPPORT.md): bugs, feature requests and usage questions.
- [Security policy](SECURITY.md): report vulnerabilities and data leaks privately.
- [Code of Conduct](CODE_OF_CONDUCT.md): community expectations and conduct reporting.

## Stack

- Rust 2024, with the pinned Rust 1.97.0 development toolchain.
- Dioxus desktop components in a wry/WebKit WebView on macOS and Linux.
- Native DuckDB through the `duckdb` crate; its Arrow re-export supplies in-process record batches.
- Tokio for asynchronous orchestration; a paged data source and virtualized grid for rendering.
- A versioned `.dat0` ZIP [format and schema](docs/dat0-format-v1.md).

## License

Apache License 2.0. See [LICENSE](LICENSE) and [third-party notices](NOTICE.md).

Copyright 2026 Accidentally Awesome Labs.
