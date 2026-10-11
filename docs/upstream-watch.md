# Upstream Watch

dat0 depends on several pre-1.0 / fast-moving upstream components. This document defines the cadence and process for tracking them so breakage is caught early and addressed within one minor release.

[Documentation index](README.md)

## Tracked dependencies

| Component | Repo | Pin policy | Why we watch closely |
|---|---|---|---|
| **dioxus** | <https://github.com/DioxusLabs/dioxus> | Pinned to an exact crates.io version (`=0.7.x`) across `dioxus`, `dioxus-desktop`, `dioxus-core`, `dioxus-html` and `dioxus-ssr` — they are released together and a mixed set fails to compile. Bump deliberately, all at once. | Pre-1.0. The UI runtime since the GPUI→Dioxus migration. 0.7 is recent; the desktop renderer's `document::eval` semantics in particular are undocumented and dat0 depends on two of them (see `crates/dat0-ui/examples/eval_probe.rs`, which measures both and is the regression test if they change). |
| **wry / tao** | <https://github.com/tauri-apps/wry>, <https://github.com/tauri-apps/tao> | Transitive through `dioxus-desktop`; not pinned directly. Watch anyway. | The actual window and WebView. Platform bugs surface here rather than in Dioxus, and the Linux build links WebKitGTK and libsoup through it. |
| **CodeMirror 6** | <https://github.com/codemirror/dev> | Vendored: `crates/dat0-ui/vendor/codemirror/package.json` pins exact versions and `assets/codemirror.js` is the built bundle, committed. Rebuild with `node build.mjs` and commit both. | The SQL editor. Vendored deliberately so a release build needs no network and no Node — see the vendor README. |
| **duckdb-rs** | <https://github.com/duckdb/duckdb-rs> | Exact-version pin (`=1.4.x`); hold the maintenance line, not the CalVer (`1.10500.x`) line. Bump deliberately. | Streaming Arrow surface (`Statement::query_arrow` / `stream_arrow`) has known rough edges (upstream issue #418): `Iterator::Item` is bare `RecordBatch`, not `Result<…>`, so mid-stream errors collapse into end-of-stream. Engine wraps the iterator with explicit error-surfacing logic. Verified pin: see "Current verified pins" below. |
| **DuckDB** (the engine) | <https://github.com/duckdb/duckdb> | Pin to a tested patch version; document upgrade impact. | Engine upgrades may affect workspace compatibility; see [Architecture](architecture.md) and [CI](ci.md). |
| **sqlite_scanner** (DuckDB extension) | <https://github.com/duckdb/sqlite_scanner> | Runtime-installed for the pinned DuckDB version, not bundled in the application. | Used for `.sqlite` / `.db` ingest; extension acquisition can require network access. |
| **Sentry Rust SDK** | <https://github.com/getsentry/sentry-rust> | Semver minor pin. | GlitchTip-compatible crash-reporting client. |
| **Updater** | dat0's signed-manifest/download/apply implementation | Review signing, artifact hashes, and platform apply behavior on changes. | See [Release runbook](release-runbook.md); this is not a Sparkle or AppImageUpdate integration. |

## Current verified pins

The manifests and `Cargo.lock` are authoritative for current pins. When changing
one, update the relevant row and exercise its production integration; use
[CONTRIBUTING](../CONTRIBUTING.md) and [CI](ci.md) for checks.

| Component | Version / Tag | SHA (full 40-char) | Verified | Notes |
|---|---|---|---|---|
| `dioxus` (crates.io) | `=0.7.10` | n/a | 2026-08-09 | Exact pin, with `dioxus-desktop`, `dioxus-core`, `dioxus-html` and `dioxus-ssr` held at the same version. Two undocumented behaviours dat0 relies on are measured by `crates/dat0-ui/examples/eval_probe.rs`: `document::eval` scripts run CONCURRENTLY (a slow script does not block others), and a returned script's `dioxus.send` channel SURVIVES (the SQL editor's push channel depends on it). Re-run that probe on every bump. |
| `wry` (transitive) | `0.53.5` | n/a | 2026-08-09 | The WebView. Resolved through `dioxus-desktop`; recorded so a platform regression can be bisected against it. |
| `tao` (transitive) | `0.34.8` | n/a | 2026-08-09 | The window and event loop. Same. |
| `duckdb` (duckdb-rs, via crates.io) | `=1.4.4` | `46d2e094ae741a4e7a500ae4389abf2cfd7e1458` | 2026-04-27 | Bundled native DuckDB `1.4.4`; features `bundled`, `json`, `parquet`. SQLite scanner and MotherDuck are runtime-installed/loaded, not Cargo features. Preserve the deliberate maintenance-line pin when evaluating upgrades. |
| `fs4` (crates.io) | `0.9.1` | n/a | 2026-05-16 | Minor pin (`^0.9`). Advisory file lock for `AppLock` PID guard (P3a T2). Resolved via `Cargo.lock`. |
| `interprocess` (crates.io) | `2.4.2` | n/a | 2026-05-16 | Minor pin (`^2`), `tokio` feature. Cross-platform UDS for second-launch IPC (P3a T2). Resolved via `Cargo.lock`. |
| `uuid` (crates.io) | `1.23.1` | n/a | 2026-05-16 | Minor pin (`^1`), `v7` feature. Time-ordered window IDs (P3a T4). Resolved via `Cargo.lock`. |
| `lru` (crates.io) | `0.18` | n/a | Manifest baseline | Minor pin (`^0.18`). Paged Arrow-batch cache in `GridDataSource`; exact resolution is in `Cargo.lock`. |
| `reqwest` (crates.io) | `0.12.28` | n/a | 2026-05-25 | Minor pin (`^0.12`), `default-features = false`, features `rustls-tls` + `stream`. Sample-data fetch (`sample_data::fetch_remote`, P3b T8). rustls-only (no openssl) keeps the bundle hermetic. Resolved via `Cargo.lock`. |
| `sha2` (crates.io) | `0.11` | n/a | Manifest baseline | Minor pin (`^0.11`). SHA-256 verification of remote sample downloads; exact resolution is in `Cargo.lock`. |
| `futures` (crates.io) | `0.3.32` | n/a | 2026-05-25 | Minor pin (`^0.3`). `futures::channel::mpsc` for `MainThreadDispatcher` (P3b T1, closes PD-010). Resolved via `Cargo.lock`. |
| `mockito` (crates.io, dev-dep) | `1.7.2` | n/a | 2026-05-25 | Minor pin (`^1`). HTTP mocking for `sample_data_fetch.rs` (P3b T8). Dev-only — excluded from NOTICE.md by `ignore-dev-dependencies = true` in `about.toml`. Resolved via `Cargo.lock`. |



## Watched upstream API gaps

The pinned `duckdb-rs` Arrow iterator yields bare `RecordBatch` values rather
than `Result<RecordBatch, duckdb::Error>`. Watch
[upstream issue #418](https://github.com/duckdb/duckdb-rs/issues/418) for an
error-bearing iterator or post-bind error accessor. Mid-stream failure can
otherwise look like end-of-stream; the counted paged path reconciles received
rows with its count. Preserve error-surfacing safeguards when upgrading.

## Cadence

- **Weekly:** scan release feeds for all tracked components. Roughly 30 minutes of skim time.
  - Prefer GitHub Releases / Atom feeds aggregated into a single reader.
  - Note any breaking changes, new features dat0 might use, security patches.
- **Monthly:** bump pins where safe (no behavior change), test in CI, ship in next release.
- **Per security advisory:** drop everything, evaluate impact, fix or mitigate within the SLA from `SECURITY.md`.

## What "breakage" means here

A tracked component's change is considered a breakage if any of the following hold:
- A type signature dat0 calls changes
- A behavior dat0 relies on changes (e.g., `stream_arrow` semantics)
- A dependency-of-dependency raises minimum Rust version above what dat0 supports
- A license changes
- Upstream archives or yanks releases dat0 depends on

## Escalation path

1. **Detect** — weekly scan or CI failure.
2. **Triage** — open an issue with the component, change summary, and impact on dat0.
3. **Mitigate** — three options in order of preference:
   - Bump the pin and update dat0 code (preferred when small)
   - Vendor the component into the repo and freeze (when upstream is unstable)
   - Fork and maintain a private patch (last resort)
4. **Address** within one minor release of dat0.

## Vendoring

`cargo vendor` is an optional mitigation for critical Apache-2.0 / MIT components,
not a configured periodic mirror in this repository. If adopted, review licenses,
repository size, update policy, and Cargo source configuration together.

## Tooling

- `cargo deny check` runs in CI for advisories, licenses, and dependency policy.
- `scripts/notice-regen.sh` is the canonical Linux NOTICE regeneration path;
  CI gates drift and provides a correction artifact (see [CI](ci.md)).
- `cargo outdated` is run weekly as part of the scan (manual)

## When this document changes

This file is updated:
- On bootstrap (P0)
- Whenever a new tracked dependency is added
- Whenever the pin or vendoring policy for a tracked dependency changes
- After any breakage that exposes a process gap

History of substantive changes is captured in git.
