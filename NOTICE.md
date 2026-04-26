# NOTICE

```
dat0
Copyright 2026 Accidentally Awesome Labs

This product includes software developed by Accidentally Awesome Labs
and contributors.

Licensed under the Apache License, Version 2.0 (the "License").
You may obtain a copy of the License at:

    http://www.apache.org/licenses/LICENSE-2.0
```

## Third-party software

dat0 incorporates the following third-party components. This list is the **initial NOTICE** at project bootstrap; it is regenerated mechanically (via `cargo-about` or equivalent) as the dependency tree is finalized in subsequent phases.

### Apache License 2.0

- **GPUI** — UI framework. Copyright (c) Zed Industries, Inc. <https://github.com/zed-industries/zed>
- **gpui-component** — Component library. Copyright (c) Longbridge HK Limited. <https://github.com/longbridge/gpui-component>
- **Apache Arrow** (Rust crates) — Columnar in-memory format. <https://github.com/apache/arrow-rs>
- **tokio** — Async runtime. <https://github.com/tokio-rs/tokio>
- **tracing** — Application-level diagnostic system. <https://github.com/tokio-rs/tracing>
- **serde** — Serialization framework. <https://github.com/serde-rs/serde>
- **sentry** — Rust SDK for Sentry-compatible error reporting. <https://github.com/getsentry/sentry-rust>

### MIT License

- **DuckDB** — Embedded analytical database. Copyright (c) DuckDB Foundation. <https://github.com/duckdb/duckdb>
- **duckdb-rs** — Rust bindings for DuckDB. <https://github.com/duckdb/duckdb-rs>
- **anyhow / thiserror** — Error handling. <https://github.com/dtolnay/anyhow>
- **tree-sitter** — Parser generator. <https://github.com/tree-sitter/tree-sitter>

### BSD-3-Clause

- **Sparkle** (macOS auto-update framework, used via XPC bridge from Rust) — Copyright (c) Sparkle Project. <https://github.com/sparkle-project/Sparkle>

### Mixed / project-specific

- **AppImageUpdate** (Linux self-update for AppImage builds) — GPL-2.0+. **Used as a separate subprocess invoked at runtime, not statically or dynamically linked into dat0.** Per Apache-2.0 §4 and GPLv2 boundaries, subprocess invocation does not impose copyleft on dat0 itself. The user-facing AppImage bundle therefore contains a GPL-2.0+ binary (AppImageUpdate) alongside an Apache-2.0 binary (dat0); both licenses are honored.

## How this file is maintained

- The initial NOTICE is committed during P0 (project bootstrap).
- A CI gate (`cargo-about` or equivalent) regenerates this list against the actual dependency tree on every release; merges fail if NOTICE drifts.
- When upstream components are pinned to specific commits (e.g., `gpui` and `gpui-component`, both pre-1.0), the pinned commit hashes will be recorded in this file alongside the version.
- See [`docs/upstream-watch.md`](docs/upstream-watch.md) for the cadence of upstream tracking.

## Trademarks

The names "dat0", "Accidentally Awesome Labs", and the `.dat0` file extension are not trademarks of any third party listed above. Use of those names by third parties is governed by Apache License 2.0 §6 (Trademarks).

External names referenced in this project (DuckDB, MotherDuck, GPUI, Apache Arrow, etc.) are the trademarks or registered trademarks of their respective owners and are used only descriptively.
