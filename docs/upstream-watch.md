# Upstream Watch

dat0 depends on several pre-1.0 / fast-moving upstream components. This document defines the cadence and process for tracking them so breakage is caught early and addressed within one minor release.

## Tracked dependencies

| Component | Repo | Pin policy | Why we watch closely |
|---|---|---|---|
| **dioxus** | <https://github.com/DioxusLabs/dioxus> | Pinned to an exact crates.io version (`=0.7.x`) across `dioxus`, `dioxus-desktop`, `dioxus-core`, `dioxus-html` and `dioxus-ssr` — they are released together and a mixed set fails to compile. Bump deliberately, all at once. | Pre-1.0. The UI runtime since the GPUI→Dioxus migration. 0.7 is recent; the desktop renderer's `document::eval` semantics in particular are undocumented and dat0 depends on two of them (see `crates/dat0-ui/examples/eval_probe.rs`, which measures both and is the regression test if they change). |
| **wry / tao** | <https://github.com/tauri-apps/wry>, <https://github.com/tauri-apps/tao> | Transitive through `dioxus-desktop`; not pinned directly. Watch anyway. | The actual window and WebView. Platform bugs surface here rather than in Dioxus, and the Linux build links WebKitGTK and libsoup through it. |
| **CodeMirror 6** | <https://github.com/codemirror/dev> | Vendored: `crates/dat0-ui/vendor/codemirror/package.json` pins exact versions and `assets/codemirror.js` is the built bundle, committed. Rebuild with `node build.mjs` and commit both. | The SQL editor. Vendored deliberately so a release build needs no network and no Node — see the vendor README. |
| **duckdb-rs** | <https://github.com/duckdb/duckdb-rs> | Exact-version pin (`=1.4.x`); hold the maintenance line, not the CalVer (`1.10500.x`) line. Bump deliberately. | Streaming Arrow surface (`Statement::query_arrow` / `stream_arrow`) has known rough edges (upstream issue #418): `Iterator::Item` is bare `RecordBatch`, not `Result<…>`, so mid-stream errors collapse into end-of-stream. Engine wraps the iterator with explicit error-surfacing logic. Verified pin: see "Current verified pins" below. |
| **DuckDB** (the engine) | <https://github.com/duckdb/duckdb> | Pin to a tested patch version; document upgrade impact. | Engine upgrades may break workspace DB compatibility. See spec §6.6 (migration scaffolding). |
| **sqlite_scanner** (DuckDB extension) | <https://github.com/duckdb/sqlite_scanner> | Bundled with DuckDB version. | Used for `.sqlite` / `.db` ingest. |
| **Sentry Rust SDK** | <https://github.com/getsentry/sentry-rust> | Semver minor pin. | Used as the GlitchTip-compatible error reporting client. |
| **Sparkle** (macOS) | <https://github.com/sparkle-project/Sparkle> | Pin a stable release. | Auto-update path. |
| **AppImageUpdate** (Linux) | <https://github.com/AppImage/AppImageUpdate> | Bundled binary subprocess. | Auto-update path. Subprocess only — see NOTICE.md. |

## Current verified pins

Verified by phase T0 spikes. The Dioxus surface was verified by the GPUI→Dioxus migration's Phase 0 spikes on **2026-08-09** (see [`docs/internal/2026-08-09-gpui-to-dioxus-migration-log.md`](internal/2026-08-09-gpui-to-dioxus-migration-log.md)). DuckDB surface verified by P2.T0 on **2026-04-27** (see [`docs/internal/duckdb-arrow-api-notes.md`](internal/duckdb-arrow-api-notes.md)). When bumping any pin, update both this table and the corresponding API-notes file.

| Component | Version / Tag | SHA (full 40-char) | Verified | Notes |
|---|---|---|---|---|
| `dioxus` (crates.io) | `=0.7.10` | n/a | 2026-08-09 | Exact pin, with `dioxus-desktop`, `dioxus-core`, `dioxus-html` and `dioxus-ssr` held at the same version. Two undocumented behaviours dat0 relies on are measured by `crates/dat0-ui/examples/eval_probe.rs`: `document::eval` scripts run CONCURRENTLY (a slow script does not block others), and a returned script's `dioxus.send` channel SURVIVES (the SQL editor's push channel depends on it). Re-run that probe on every bump. |
| `wry` (transitive) | `0.53.5` | n/a | 2026-08-09 | The WebView. Resolved through `dioxus-desktop`; recorded so a platform regression can be bisected against it. |
| `tao` (transitive) | `0.34.8` | n/a | 2026-08-09 | The window and event loop. Same. |
| `duckdb` (duckdb-rs, via crates.io) | `=1.4.4` | `46d2e094ae741a4e7a500ae4389abf2cfd7e1458` | 2026-04-27 | Tag `v1.4.4` in `duckdb/duckdb-rs`, released 2025-01-27. Bundled DuckDB native = `1.4.4`. Features enabled: `bundled`, `json`, `parquet`. `sqlite_scanner` and `motherduck` are runtime-loaded (no Cargo feature exists; D-009 contingency open). Maintenance line `1.4.x` deliberately preferred over CalVer `1.10500.x` line at P2 entry — re-evaluate at P5/P6. |
| `fs4` (crates.io) | `0.9.1` | n/a | 2026-05-16 | Minor pin (`^0.9`). Advisory file lock for `AppLock` PID guard (P3a T2). Resolved via `Cargo.lock`. |
| `interprocess` (crates.io) | `2.4.2` | n/a | 2026-05-16 | Minor pin (`^2`), `tokio` feature. Cross-platform UDS for second-launch IPC (P3a T2). Resolved via `Cargo.lock`. |
| `uuid` (crates.io) | `1.23.1` | n/a | 2026-05-16 | Minor pin (`^1`), `v7` feature. Time-ordered window IDs (P3a T4). Resolved via `Cargo.lock`. |
| `lru` (crates.io) | `0.12.5` | n/a | 2026-05-16 | Minor pin (`^0.12`). LRU cache for paged Arrow batches in `GridDataSource` (P3a T8). Resolved via `Cargo.lock`. |
| `reqwest` (crates.io) | `0.12.28` | n/a | 2026-05-25 | Minor pin (`^0.12`), `default-features = false`, features `rustls-tls` + `stream`. Sample-data fetch (`sample_data::fetch_remote`, P3b T8). rustls-only (no openssl) keeps the bundle hermetic. Resolved via `Cargo.lock`. |
| `sha2` (crates.io) | `0.10.9` | n/a | 2026-05-25 | Minor pin (`^0.10`). SHA-256 verification of remote sample-data downloads (P3b T8). Resolved via `Cargo.lock`. |
| `futures` (crates.io) | `0.3.32` | n/a | 2026-05-25 | Minor pin (`^0.3`). `futures::channel::mpsc` for `MainThreadDispatcher` (P3b T1, closes PD-010). Resolved via `Cargo.lock`. |
| `mockito` (crates.io, dev-dep) | `1.7.2` | n/a | 2026-05-25 | Minor pin (`^1`). HTTP mocking for `sample_data_fetch.rs` (P3b T8). Dev-only — excluded from NOTICE.md by `ignore-dev-dependencies = true` in `about.toml`. Resolved via `Cargo.lock`. |


**P1 audit closure (2026-04-26)** — historical; the gpui pins it refers to were removed by the GPUI→Dioxus migration (2026-08-10):  T23 audit confirmed SHAs above match workspace `Cargo.toml` after T0–T22 implementation. No bumps were required during P1 execution. Next scheduled bump-check: monthly cadence per "Cadence" below.

**P3b audit closure (2026-05-25)** — historical, same note:  P3b T13 verified the four pre-P3b pins (`gpui-component`, `gpui`, `gpui-macros`, `duckdb`) are unchanged and added four new pins (`reqwest`, `sha2`, `futures`, `mockito`) above. `cargo about generate -c about.toml docs/about-template.hbs` produced byte-identical output to the existing `NOTICE.md` block — no NOTICE regen required this phase.

## Watched upstream API gaps

APIs dat0 needs and the pinned upstream does not have. Distinct from the pin
table above: nothing here is broken or outdated, so a bump-check will never
surface it. Each row is a capability a dat0 slice reached for, could not find,
and deferred — so the weekly scan has something concrete to look for, and the
deferral has somewhere to be closed from.

| Missing API | Upstream | Verified absent at | dat0 impact | Deferral |
|---|---|---|---|---|
| ~~Letter-spacing / tracking on text~~ | ~~`gpui`~~ | Resolved 2026-08-10 by the GPUI→Dioxus migration, not by upstream: CSS has had `letter-spacing` since CSS1, so the v4 shell's display tracking is written in `crates/dat0-ui/assets/app.css` alongside the rest of the type. | none | [D-031](deferrals.md#d-031--display-type-letter-spacing-unavailable-on-gpui-022) (closed) |
| A `Result`-yielding Arrow iterator. Needed: `Arrow::next` returning `Option<Result<RecordBatch, duckdb::Error>>` (or any accessor that exposes a post-bind statement error), so a mid-stream failure is distinguishable from end-of-stream. | `duckdb-rs` | `=1.4.4`. `Arrow::next` is `Some(RecordBatch::from(&self.stmt?.step()?))` (`duckdb-1.4.4/src/arrow_batch.rs:27-33`) — `step()` returns `Option`, `Item` is a bare `RecordBatch`, and there is no `Statement::error()`. Long-standing upstream issue #418. | A DuckDB error raised after a successful bind silently truncates the result on all three drain paths (`run_materialized`, `run_page`, `spawn_streaming`). Mitigated only on the counted path: `run_paged` reconciles the rows that arrived against its own `COUNT(*)` and fails with `"result stream ended early"`. The uncounted paths have no detector. | [D-030](deferrals.md#d-030--mid-stream-arrow-errors-are-invisible-on-uncounted-paths) |

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
2. **Triage** — open an issue tagged `upstream-watch` with: component, change summary, impact on dat0, affected phase(s).
3. **Mitigate** — three options in order of preference:
   - Bump the pin and update dat0 code (preferred when small)
   - Vendor the component into the repo and freeze (when upstream is unstable)
   - Fork and maintain a private patch (last resort)
4. **Address** within one minor release of dat0.

## Vendoring

For Apache-2.0 / MIT components dat0 critically depends on (especially `dioxus`, `wry` and `tao` while pre-1.0), `cargo vendor` is run periodically to mirror the current pinned versions into `vendor/` so dat0 remains buildable even if upstream goes dark. This is a defense-in-depth measure, not a fork.

## Tooling

- `cargo audit` runs in CI to flag advisories
- `cargo deny` runs in CI to enforce license + version policy
- `cargo about` regenerates `NOTICE.md` and gates PR merges if it drifts
- `cargo outdated` is run weekly as part of the scan (manual)

## When this document changes

This file is updated:
- On bootstrap (P0)
- Whenever a new tracked dependency is added
- Whenever the pin or vendoring policy for a tracked dependency changes
- After any breakage that exposes a process gap

History of substantive changes is captured in git.
