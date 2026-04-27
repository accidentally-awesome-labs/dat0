# Upstream Watch

dat0 depends on several pre-1.0 / fast-moving upstream components. This document defines the cadence and process for tracking them so breakage is caught early and addressed within one minor release.

## Tracked dependencies

| Component | Repo | Pin policy | Why we watch closely |
|---|---|---|---|
| **gpui** | <https://github.com/zed-industries/zed> (published to crates.io as of v0.2.0, Oct 2025) | Pinned to an exact crates.io version (`=x.y.z`); also record the publish-commit SHA in `docs/internal/gpui-api-notes.md` for traceability. Bump deliberately. | Pre-1.0; Zed monorepo evolves rapidly. Used as core UI runtime. |
| **gpui-component** | <https://github.com/longbridge/gpui-component> | Pinned to a tagged-release commit hash. | Pre-1.0; single-maintainer org (Longbridge). Used for Table, code editor, command palette, charts, themes. |
| **duckdb-rs** | <https://github.com/duckdb/duckdb-rs> | Semver minor pin. | Streaming Arrow surface (`stream_arrow`) has known rough edges (issue #418). Track fixes. |
| **DuckDB** (the engine) | <https://github.com/duckdb/duckdb> | Pin to a tested patch version; document upgrade impact. | Engine upgrades may break workspace DB compatibility. See spec §6.6 (migration scaffolding). |
| **sqlite_scanner** (DuckDB extension) | <https://github.com/duckdb/sqlite_scanner> | Bundled with DuckDB version. | Used for `.sqlite` / `.db` ingest. |
| **Sentry Rust SDK** | <https://github.com/getsentry/sentry-rust> | Semver minor pin. | Used as the GlitchTip-compatible error reporting client. |
| **Sparkle** (macOS) | <https://github.com/sparkle-project/Sparkle> | Pin a stable release. | Auto-update path. |
| **AppImageUpdate** (Linux) | <https://github.com/AppImage/AppImageUpdate> | Bundled binary subprocess. | Auto-update path. Subprocess only — see NOTICE.md. |

## Current verified pins

Verified by the P1.T0 spike on **2026-04-26**. Full API-surface notes live in [`docs/internal/gpui-api-notes.md`](internal/gpui-api-notes.md). When bumping, update both files.

| Component | Version / Tag | SHA (full 40-char) | Verified | Notes |
|---|---|---|---|---|
| `gpui-component` (longbridge) | `v0.5.1` | `0f0ab35233212f8f3277028995caf0c41e13ee6c` | 2026-04-26 | Tagged release. Fixes macOS `core-text` build failure present in v0.5.0. |
| `gpui` (Zed, via crates.io) | `=0.2.2` | `08d95ad9d31f616a43dacda8416568d658dca6ae` | 2026-04-26 | Publish commit in `zed-industries/zed` ("chore: Bump gpui to 0.2.2 (#40856)", 2025-10-22). Consumed via `cargo` from crates.io, not as a git dep. The `=` prefix is the literal `Cargo.toml` form (exact-version pin), per the policy in the table above. |
| `gpui-macros` (Zed, via crates.io) | `=0.2.2` | `08d95ad9d31f616a43dacda8416568d658dca6ae` | 2026-04-26 | Companion crate; published from the same Zed commit. Same exact-version pin form as `gpui`. |

> **Mechanism change (since planning snapshot):** `gpui-component` v0.5.1 declares `gpui = "0.2.2"` in its workspace `Cargo.toml`, consuming `gpui` as a published crates.io crate rather than as a git dependency. The pin policy still applies, but dat0 should pin via exact-version semver (`gpui = "=0.2.2"`) plus `Cargo.lock`, and record the publish-commit SHA in `docs/internal/gpui-api-notes.md` for audit.

**P1 audit closure (2026-04-26):** T23 audit confirmed SHAs above match workspace `Cargo.toml` after T0–T22 implementation. No bumps were required during P1 execution. Next scheduled bump-check: monthly cadence per "Cadence" below.

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

For Apache-2.0 components dat0 critically depends on (especially `gpui` and `gpui-component` while pre-1.0), `cargo vendor` is run periodically to mirror the current pinned versions into `vendor/` so dat0 remains buildable even if upstream goes dark. This is a defense-in-depth measure, not a fork.

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
