# CI and advanced validation

[Documentation index](README.md) · [Contributor setup](../CONTRIBUTING.md)

Use CONTRIBUTING for prerequisites and the ordinary build/test/run sequence. This
reference covers checks that depend on the kind of change, optional large/live
workloads, and maintainer workflow operations. Credential-free does not mean offline:
DuckDB extension acquisition and dependency downloads can use the network.

## Workflows and required verdict

| Workflow | Triggers and role | Runners |
|---|---|---|
| [`ci.yml`](../.github/workflows/ci.yml) | Main pushes and all PRs, including label additions/removals; ordinary checks, reusable NOTICE, docs and aggregate verdict | GitHub-hosted Ubuntu and macOS 14 arm64 |
| [`heavy.yml`](../.github/workflows/heavy.yml) | Monday 06:00 UTC, manual dispatch, `run-heavy` PRs; large fixtures and view-regeneration bench | GitHub-hosted Ubuntu |
| [`notice.yml`](../.github/workflows/notice.yml) | Reusable workflow called by every CI run; canonical Linux NOTICE generation | GitHub-hosted Ubuntu |
| [`crash-e2e.yml`](../.github/workflows/crash-e2e.yml) | Canonical main push/manual run; opt-in synthetic crash round trip when provisioned | GitHub-hosted Ubuntu |
| [`pd004-diagnose.yml`](../.github/workflows/pd004-diagnose.yml) | Manual development-ref diagnostic for Linux Secret Service; sanitized results belong on the relevant public issue | GitHub-hosted Ubuntu |
| [`release.yml`](../.github/workflows/release.yml) | Canonical main manual dispatch or a `v*` tag push in main history; protected release operations | GitHub-hosted macOS and Ubuntu |

Main requires **CI required** (GitHub Actions) and **DCO** (the installed external
DCO app). The always-running aggregate requires successful `fmt`, `clippy`,
`build-and-test` (both platforms), `windowed-probes`, `supply-chain`, `i18n-check`,
`no-standalone-arrow`, `dat0-core-purity`, `no-debug-query-scalar`, `notice` and
`docs-check`. Missing, failed, cancelled or unexpectedly skipped results fail.
`perf-gate` must succeed when `run-perf` is requested and must be skipped otherwise.
Coverage, optional heavy workloads, crash ingestion and release jobs are not part
of this required verdict. Commit skip directives can leave required checks pending;
do not use them on PRs.

All PR/label-triggered jobs stay GitHub-hosted. Labels authorize extra work, not a
sandbox for untrusted code. Checkouts do not retain credentials; workflow tokens
default to read-only, and the release publish job alone requests contents write.
External actions are pinned to commits and maintained through Dependabot.
Timeouts and per-ref cancellation prevent stalled or superseded runs from lingering.
Adding or removing a label recomputes CI; set opt-in labels before the push you want
covered when possible.

## Test isolation and resource pressure

CI builds release artifacts, primes SQLite with a blocking
`cargo test --locked -p dat0-engine --test attach_sqlite -- --nocapture`, then runs
nextest and doctests using its matrix target. Nextest isolates each test in its own
process, which matters for process-global state and singleton initialization.
`cargo test` is available but does not provide equivalent isolation. Nextest does
not run doctests; `cargo test --locked --workspace --doc` is a separate gate.

The native Linux package list is in [CONTRIBUTING](../CONTRIBUTING.md#setup).
Headless compilation still needs WebKitGTK/Soup and the other native dependencies.
Explicit native keychain runs additionally need a session bus and unlocked Secret
Service; absence is not a successful native-keychain measurement.

For constrained local builds, use `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`, and
consistent `CARGO_PROFILE_DEV_DEBUG=0` across all dev/test steps. Profile changes
invalidate artifacts. CI retains its target/profile disk-reclamation steps because
bundled DuckDB and many linked test binaries consume substantial space; registry/git
caching is useful even when target caching is disabled. Shell pipelines use explicit
Bash with pipefail, so successful `tee` does not conceal failed cargo commands.
Debug timings do not establish release performance.

## Visual and interaction checks

Behavior tests use [`crates/dat0-ui/tests/support`](../crates/dat0-ui/tests/support)
and production accessible handles. Drive user keyboard events through production
routing instead of invoking handlers directly. Preserve Escape priority and release
accessibility handles. Styling uses tokens; review contrast and theme switching.
The [accessibility checklist](a11y.md#1-accessibility-checklist) is the public contract.

SSR snapshots cover appearance without a display:

```sh
cargo nextest run --locked -p dat0-ui --test visual_snapshot
INSTA_UPDATE=always cargo test --locked -p dat0-ui --test visual_snapshot
```

The second command intentionally updates snapshots. Inspect the actual diffs before
committing. Markup is not geometry or focus proof. On a desktop run:

```sh
cargo run --locked -p dat0-ui --features visual --example visual_probe
```

CI's `windowed-probes` job builds and runs **eight** real WebKitGTK probes under
Linux Xvfb: `shell_probe`, `window_probe`, `modal_trap_probe`,
`settings_window_probe`, `console_probe`, `visual_probe`, `grid_probe`, and
`focus_probe`. The existing [helper](../.github/scripts/windowed-probes.sh) keeps
per-probe profile/session isolation, timeouts, and collects failures rather than
stopping at the first probe. Its Linux invocation after building the release examples is:

```sh
xvfb-run -a -s "-screen 0 1920x1200x24" bash .github/scripts/windowed-probes.sh
```

`shell_probe` boots a real session on a small synthetic CSV and samples sidebar
width and grid position while the window size stays fixed. This catches layout
feedback loops that a single good frame misses. Grid resize events are
element-local: they must not supply the shell's window-size measurements.

The human review gallery embeds fonts and writes scene/theme links to `target/visual`:

```sh
cargo run --locked -p dat0-ui --features visual --example visual_page
# Open target/visual/index.html in your browser.
```

Review those actual scenes and the affected desktop behavior; no private design
HTML is a prerequisite.

## Localization

Run `bash scripts/i18n-check.sh` and `cargo test --locked -p dat0-i18n`.
The heuristic pass for untranslated strings is advisory; the referenced-key pass
is blocking. Literal `t()` keys and declared `*_KEYS` lists must exist in
[`en.json`](../crates/dat0-i18n/src/strings/en.json). Runtime-composed keys need the
existing explicit key-list mechanism. Tests containing deliberately absent keys
are not part of the source scan.

## Dependencies, licenses and NOTICE

Run `cargo deny check` against [`deny.toml`](../deny.toml). Keep its SPDX policy
aligned with [`about.toml`](../about.toml). Deliberate DuckDB/Dioxus pins and the
`duckdb::arrow` re-export chain are described in [upstream watch](upstream-watch.md).
Dependabot maintains Cargo and action updates, not independent bumps of those pins.
The core-purity check must fail if `cargo tree` itself fails; absence of output is
not evidence that core is toolkit-free.

Every CI run calls the canonical NOTICE workflow, without secrets. On Linux:

```sh
cargo install cargo-about --locked --features cli --version 0.9.2
scripts/notice-regen.sh
```

The version pin lives in [the generator](../scripts/notice-regen.sh), and the workflow
extracts it. Only the marked generated block is replaced; handwritten icons/fonts/
JavaScript attribution remains. **Never redirect raw cargo-about output over NOTICE.**
The generator normalizes line endings and duplicate attribution, and the Linux
license selection is canonical. On macOS download and inspect the regenerated
`NOTICE.md` correction artifact from the failed check, then commit that file.

## Documentation and publication boundary

Before pushing, run:

```sh
bash .github/scripts/check-public-tree.sh
```

The helper checks tracked entries against the root `.gitignore` policy, not global
Git excludes. Local internal notes may remain ignored, but must not be tracked.
Public samples need a narrow root-policy exception; `git add -f` does not bypass
the check. CI can block merging a violation, not erase already-pushed content.
Cleanup applies to the current tree; historical commits, forks and old PR views
are not purged.

`docs-check` generates inputs from the Git index, so ignored local documents do not
enter validation. To reproduce the offline local-target/fragment check with Lychee
**0.24.2** from the repository root:

```sh
export RUNNER_TEMP="$(mktemp -d /tmp/dat0-links.XXXXXX)"
git ls-files -- README.md CONTRIBUTING.md SECURITY.md CODE_OF_CONDUCT.md SUPPORT.md \
  .github/pull_request_template.md ':(glob)docs/*.md' \
  > "$RUNNER_TEMP/dat0-docs-inputs.txt"
lychee --offline --include-fragments --no-progress \
  --root-dir "$(git rev-parse --show-toplevel)" \
  --files-from "$RUNNER_TEMP/dat0-docs-inputs.txt"
```

External-network availability is not a merge gate. Broken current local links are
fixed rather than broadly suppressed.

## Performance

Budgets and recorded host measurements live in
[`performance-baselines.json`](performance-baselines.json). The shared loader and
`--update-baseline` writer use this public input; missing/malformed input is an error.
For the two fixture-free startup scenarios on a supported desktop:

```sh
cargo xtask perf --check --scenario idle_rss --scenario cold_launch
```

The helper builds the selected scenarios' release artifacts itself. App and example
builds are separate to prevent development-feature leakage. A **SKIP is not a
measurement**. Do not change budgets or recorded baselines to conceal a regression.
Host keys distinguish developer hardware from virtualized CI; absolute budgets and
recorded-host regression checks serve different purposes. A new host does not supply
an old-host regression comparison.

Two CI entry points share that interface: the macOS main-push advisory records
`scroll_1m`, `cold_launch`, and `idle_rss` with `DAT0_PERF_HOST=ci-hosted`; the
`run-perf` PR job runs all six scenarios as a blocking verdict. Maintainers apply
`run-perf` when relevant: `gh pr edit <N> --add-label run-perf`. Real frame timing is
not a general throughput guarantee for arbitrary hardware or virtual displays.

## Heavy tests

`heavy.yml` runs weekly, manually, and for `run-heavy` PRs. Maintainers apply it for
changes to size-bound engine/attach/streaming behavior or the fixture generator:
`gh pr edit <N> --add-label run-heavy`. It generates/caches the large fixtures for
[`exit_criteria`](../crates/dat0-engine/tests/exit_criteria.rs) and runs the
view-regeneration bench. It is not a prerequisite for every documentation/UI PR.
Use the workflow's scoped fixture generation and test commands, not a workspace-wide
ignored-test invocation.

## Ignored and live test categories

| Category | Prerequisite and scope |
|---|---|
| Large fixtures | Generated 1 GB CSV, 500 MB Parquet and 100 MB SQLite; targeted `dat0-engine` `exit_criteria`, normally through heavy.yml |
| Live MotherDuck/OpenRouter | Chosen service credentials and network access; targeted `md_attach` / `ai_live`, not the default suite |
| Native keychain | macOS uses a real `dat0-test` item; Linux explicitly opted-in runs require unlocked Secret Service; preserve target-specific ignore behavior |
| Timing-sensitive cancellation | Dedicated targeted `view_lifecycle` ignored case; do not treat a loaded shared runner as reliable timing evidence |
| Production update key | Targeted ignored `update_key_is_production` release tripwire; provisioned release preparation, not ordinary contributors |

Live-service steps are permitted only for a push to `refs/heads/main` in
`accidentally-awesome-labs/dat0`. All PRs, including same-repository and Dependabot
PRs, and fork-main pushes explicitly skip them. MotherDuck remains a hard main gate;
OpenRouter live smoke is bounded and advisory. Ordinary CI/heavy use the development
DSN stub, not production crash credentials. Default tests may still acquire extensions.

## Coverage and release operations

Coverage uploads an LCOV report and is advisory, with no threshold. Instrumented
builds use reduced dev debug information for disk capacity; that does not replace
behavioral validation. Do not promote an unmeasured threshold into a merge gate.

Release preparation uses DCO-signed PRs and the normal required checks. See
[release prerequisites](release-prerequisites.md), [release runbook](release-runbook.md),
and [security operations](security-runbook.md). Main dispatch:
`gh workflow run release.yml --repo accidentally-awesome-labs/dat0 --ref main`.
Absent signing material permits the existing unsigned packaging dry run; it does
not exercise production manifest signing and never publishes. Signing/notary/minisign/
GPG material belongs in the protected `release` environment. Telemetry credentials
are separately scoped to the workflows that consume them; never put real values in
public logs or issue reports.
