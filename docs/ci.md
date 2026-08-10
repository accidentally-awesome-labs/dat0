# CI Setup

> Operational reference for dat0's GitHub Actions setup. Covers per-PR
> validation (`ci.yml`), the perf gate, coverage and supply-chain jobs, heavy
> exit-criteria tests (`heavy.yml`), the NOTICE drift gate (`notice.yml`), the
> crash round-trip (`crash-e2e.yml`), the PD-004 diagnostic
> (`pd004-diagnose.yml`), and the runnerkit self-hosted Linux runner that backs
> the Linux matrix entry.

---

## Workflows

| File | Triggers | Jobs | Runner |
|---|---|---|---|
| `.github/workflows/ci.yml` | push to `main`, all PRs | fmt, clippy, i18n-check, build-and-test (macos-arm64, linux-x86_64), coverage, supply-chain, no-standalone-arrow, no-debug-query-scalar | hosted macos-14 + hosted/self-hosted Linux |
| `.github/workflows/ci.yml` (`perf-gate`) | PRs labelled `run-perf` | perf-gate — blocking `cargo xtask perf --check`, all six scenarios | hosted macos-14 |
| `.github/workflows/heavy.yml` | **weekly cron (Mon 06:00 UTC)**, `workflow_dispatch`, PRs with `run-heavy` label | exit-criteria (1 GB CSV + 500 MB Parquet + 100 MB SQLite), view-regen-bench | hosted Linux |
| `.github/workflows/notice.yml` | NOTICE.md / manifest drift | notice — **hard gate** since QA2 (PD-003 closed) | hosted `ubuntu-latest` (pinned; see below) |
| `.github/workflows/crash-e2e.yml` | push to `main`, `workflow_dispatch` | glitchtip-roundtrip | hosted `ubuntu-latest` |
| `.github/workflows/pd004-diagnose.yml` | `workflow_dispatch` only | keychain-linux — **diagnostic, not a gate** (PD-004) | hosted `ubuntu-latest` |
| `.github/workflows/release.yml` | tags, `workflow_dispatch` | macos, linux, publish | hosted |

Every job carries a `timeout-minutes`. That is a hard rule, not a style
preference: a hung step burns to GitHub's 6-hour default, and because the job
never finishes, a REQUIRED check goes down with it — `continue-on-error`
cannot rescue a hang. This happened twice on PR #49.

### Which jobs gate

| Gate | Blocking? | Why |
|---|---|---|
| fmt, clippy, build-and-test, i18n-check (pass 2), supply-chain, notice, no-standalone-arrow, no-debug-query-scalar | **yes** | deterministic, hermetic |
| coverage | no — reporting only | no threshold exists yet; see below |
| perf advisory step (inside build-and-test) | no | virtualized GPU cannot defend a frame-rate claim |
| perf-gate | **yes**, when the `run-perf` label is applied | same command, real verdict |
| OpenRouter live AI step | no | third-party outage must not redden main |
| crash-e2e **assertion step** | no | live GlitchTip ingestion; build + run above it ARE blocking |
| i18n-check pass 1 | no | heuristic; false-positives on ~646 lines |

`heavy.yml`'s cron runs **weekly** (Mondays 06:00 UTC), not nightly. The
original argument for disabling it entirely — solo-dev, low PR frequency, most
nightly runs validating nothing — was about cadence, not about whether the
heaviest gate in the repo should depend on a human remembering a label. Weekly
answers the cost objection at one-seventh the minutes. Result lands in the
job summary.

## CI wall time

### Pre-change (measured)

These are the numbers `build-and-test` ran at before QA1, and they are the
reason the job-level `timeout-minutes: 120` was chosen ("well clear of normal
cost", `.github/workflows/ci.yml`):

| Leg | Pre-change wall time | Source |
|---|---|---|
| `build-and-test` / linux-x86_64 | **~75 min** | `ci.yml` job-timeout comment, from observed runs |
| `build-and-test` / macos-arm64 | **~40 min** | same |

Both legs ran with **zero Rust caching** — every job re-fetched and rebuilt
the DuckDB + GPUI + Arrow source graph from scratch.

### Post-change

> **NOT YET MEASURED.** QA1 landed three changes that move this number —
> `Swatinem/rust-cache@v2` on `fmt` / `clippy` / `build-and-test`,
> `cargo-nextest` replacing `cargo test`, and a separate `--doc` step. None of
> them has run on `main` yet, so there is no post-change figure. Do not
> estimate one here.
>
> **Fill this table from the FIRST `main` run after QA1 merges:**
>
> | Leg | Post-change wall time | Run URL |
> |---|---|---|
> | `build-and-test` / linux-x86_64 | _(fill in)_ | _(fill in)_ |
> | `build-and-test` / macos-arm64 | _(fill in)_ | _(fill in)_ |
> | `fmt` | _(fill in)_ | _(fill in)_ |
> | `clippy` | _(fill in)_ | _(fill in)_ |
>
> Note that the **first** post-merge run is a cache MISS by definition —
> `rust-cache` has nothing to restore until a run on the default branch has
> populated it. Record the first run AND the second; the second is the steady
> state, the first is the cold cost.

### Why `cache-targets: false` on `build-and-test`

That job fights a disk ceiling, not a compile-time one. It deliberately
`rm -rf`s `target/<triple>/release` before the debug test build, and on macOS
`rm -rf`s the whole `target/<triple>` before the perf harness — both reclaims
exist because the runner PROCESS has died with `No space left on device`
mid-step (it reddened `main` on b55758b). Caching a target dir the job then
deletes would upload gigabytes per run for nothing, and restoring one would
recreate the exhaustion the reclaims prevent. The registry + git half of
`rust-cache` still applies and is pure win: it removes the DuckDB / GPUI /
Arrow source fetch. `fmt` and `clippy` have no such ceiling and use
`cache-targets: true`.

### Why nextest

`cargo nextest run` executes each test in its own process. dat0 has 21 tests
using `serial_test` and several that mutate process-global `OnceCell`s
(`crates/dat0-app/src/window_registry.rs:33-66`). Inside a single `cargo test`
binary those must serialize against one another, and a global left dirty by
one test is visible to the next. Per-test processes remove both problems.

nextest does **not** run doctests, so `cargo test --workspace --doc` runs as a
separate step. Dropping it would silently stop gating every doc example.

## Coverage

`ci.yml`'s `coverage` job runs
`cargo llvm-cov nextest --workspace --lcov --output-path lcov.info` and uploads
`lcov.info` as an artifact. It is **reporting only — there is deliberately no
threshold.**

> **NOT YET MEASURED.** Record the line coverage from the first run here:
>
> - First measured line coverage: _(fill in)_ % — run URL _(fill in)_
>
> That measured figure — not a guessed round number — becomes the floor of a
> later ratchet. A threshold chosen before the number is known either sits
> uselessly below reality or fails on day one; either way it teaches everyone
> to ignore it.

## Supply chain

`ci.yml`'s `supply-chain` job runs `EmbarkStudios/cargo-deny-action@v2` against
`deny.toml` at the repo root. **This one gates.**

`deny.toml` and `about.toml` both carry the same 12-entry SPDX allow-list, and
they must stay in sync — `about.toml` drives NOTICE.md rendering, `deny.toml`
is the enforcing copy. `deny.toml` mirrors `about.toml`'s graph settings
(same four targets, `exclude-dev = true` matching `ignore-dev-dependencies`)
so the allow-list is enforced against the graph it was derived from.

Notable settings and why:

- `[advisories] yanked = "deny"` — a yanked crate in the lockfile means we build
  a version upstream withdrew.
- `[bans] multiple-versions = "warn"` — the gpui + duckdb + arrow + sentry graph
  carries duplicate minor versions dat0 neither controls nor can unify. Denying
  would make the gate unpassable for reasons unrelated to dat0's own choices.
- `[sources] unknown-git = "deny"` with exactly one allowed URL,
  `https://github.com/longbridge/gpui-component` (covers both `gpui-component`
  and `gpui-component-assets`, which share a repo and a pinned rev).

`.github/dependabot.yml` runs weekly for `cargo` and `github-actions`, grouping
minor+patch into one PR each. It **ignores** `gpui`, `gpui-macros`,
`gpui-component`, `gpui-component-assets` and `duckdb`: all five are exact-pinned
with recorded rationale (root `Cargo.toml:53-69`, `:141-144`) and their upgrade
cadence is governed by `docs/upstream-watch.md`, not by a bot. A silent bump of
`gpui-component-assets` alone serves a different icon set with **no build error**.

## Perf gate

Two entry points, deliberately:

1. **Advisory step** inside `build-and-test`, macOS + push-to-main only,
   `continue-on-error: true`, `DAT0_PERF_HOST=ci-hosted`. Runs three scenarios
   (`scroll_1m`, `cold_launch`, `idle_rss`). A virtualized macOS VM with no
   dedicated GPU cannot honestly defend a frame-rate claim, and a gate that
   reddens `main` on runner variance trains everyone to ignore it. This step
   exists to record the `ci-hosted` numbers on every merge so drift is visible.
2. **`perf-gate` job**, PRs with the `run-perf` label, **no**
   `continue-on-error`, all six scenarios. This is the real verdict.

```bash
gh label create run-perf --description "Run the blocking perf-gate job on this PR" --color 1D76DB   # one-time
gh pr edit <N> --add-label run-perf
```

When dedicated (non-virtualized) macOS hardware arrives, the only line in
`perf-gate` that changes is `runs-on:`. Promoting it from label-triggered to
every-PR is tracked as its own deferral (see `docs/deferrals.md`, D-013's
successor).

> **Labelling re-runs CI.** `ci.yml`'s `pull_request` trigger includes
> `labeled`, which it must — otherwise applying `run-perf` to an existing PR
> fires nothing, and re-running the workflow re-evaluates `if:` against the
> original event payload, which does not carry the new label. The cost is that
> **any** label change re-runs the whole workflow, and `cancel-in-progress`
> means labelling mid-run cancels and restarts it. Apply `run-perf` and
> `run-heavy` before, or together with, the push you want them to cover.

## i18n gate

`scripts/i18n-check.sh` has two passes with different authority:

- **Pass 1 — advisory.** Heuristic hunt for UI strings that bypass
  `dat0_i18n::t()`. It flags ~646 lines, nearly all false positives (every
  non-UI `.into()` / `.to_string()`), so it warns and never gates.
- **Pass 2 — gate.** Every key the code references must exist in
  `crates/dat0-i18n/src/strings/en.json`. Exact, not heuristic: `t()` echoes the
  key back on a miss, so an unresolved key renders `catalog.empty.files` into
  the user's face. Non-zero exit, naming each key.

Two extractors feed pass 2, because there are two ways a key reaches `t()`:
string-literal call sites (a regex sees these), and **const key-lists** —
`pub const *_KEYS: &[&str] = &[…]` — which exist precisely because their call
sites compose the key at runtime from an enum variant or a group name, making
them invisible to the regex. Declaring the list is the contract that makes
those keys checkable. Current consumers: `error_ux::engine::ENGINE_ERROR_KEYS`,
`catalog::CATALOG_EMPTY_KEYS`.

The scan is scoped to `crates/*/src`. Tests are excluded on purpose: several
reference deliberately-absent keys to assert the miss behaviour itself
(`crates/dat0-i18n/tests/basic.rs:10`, `crates/dat0-app/tests/i18n_p10c_keys.rs:18`),
so scanning them would make the gate demand the very keys other tests demand be
absent.

## Matrix coverage

Per-PR matrix kept narrow on purpose:

- `macos-14` (Apple Silicon arm64) — primary platform, hosted, **10× billing**.
- `linux-x86_64` — secondary platform, self-hosted via runnerkit, **0× billing**.

Removed from per-PR (tracked in `docs/deferrals.md`):

- `macos-13` Intel — D-006. Hosted pool oversubscribed; revisit when capacity allows or self-hosted Mac arrives.
- `linux-arm64` — moved to `heavy.yml` (was originally in per-PR matrix). Adds ~30% minutes for marginal coverage delta vs linux-x86_64. Re-add if a real arm-Linux user surfaces.

## Self-hosted Linux runner (runnerkit)

The `linux-x86_64` matrix entry routes to a runnerkit self-hosted runner
matching label set `[self-hosted, Linux, runnerkit]`. Hosted vs self-hosted
is selected at workflow load time via `runs-on: ${{ fromJSON(matrix.target.labels) }}`.

### Host requirements

The runner host **must** have these packages installed. The `Linux deps`
step in both workflows is gated to `runner.environment == 'github-hosted'`
because runnerkit images do not configure NOPASSWD sudo.

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential pkg-config \
  libsecret-1-dev dbus-x11 gnome-keyring \
  libpango1.0-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libfontconfig1-dev libssl-dev
```

`build-essential` provides `cc` (the linker rust uses). `pkg-config` is
required by libsecret and several arrow/duckdb native deps.

If runnerkit supports image baking, add the above to the base image so
re-provisions inherit. Otherwise install manually on each new runnerkit
host.

### dbus + gnome-keyring

The keychain test surface uses libsecret → Secret Service → dbus session
bus. On hosted runners the `dbus-launch` step runs per-job; on self-hosted
the user session bus is expected to be running already (e.g., systemd
user service). The dbus-launch step is gated to hosted runners only.

### Memory

Earlier runnerkit runners OOM-killed `ld` during dev-profile test
binary linking. Resolved by provisioning runners with enough RAM
(persistent cloud runner suffices). If OOM recurs, see the
"OOM mitigations" section below.

### Cargo cache

The persistent runner reuses `~/.cargo/registry` and `target/` across
runs. First run on a fresh image takes ~25 min compile + ~25 min test
(~50 min total). Subsequent runs hit the cache and finish much faster.

If the runner is re-provisioned (cold), expect the long-run timing
again on the first build.

## Heavy tests (`run-heavy` label)

`heavy.yml` runs the `#[ignore = "requires generated fixtures"]`
exit-criteria suite. Triggers:

- **Weekly cron** — Mondays 06:00 UTC. The floor; runs whether or not anyone
  remembers a label.
- `workflow_dispatch` — manual run from the Actions tab.
- PRs labeled `run-heavy` — apply when a PR touches engine internals,
  fixture generator, or the attach/streaming code paths.

```bash
# Apply label from CLI:
gh label create run-heavy --description "Run heavy.yml exit_criteria suite on this PR" --color FF6B35   # one-time
gh pr edit <N> --add-label run-heavy
```

Fixture cache is keyed by `hashFiles('crates/dat0-fixtures/**')`. Cache
miss → full generation (~15-20 min for 1 GB CSV + 500 MB Parquet + 100 MB
SQLite). Cache hit → tests run against existing fixture directory.

## Concurrency

`ci.yml` has `concurrency: cancel-in-progress: true` grouped by
`github.ref`. Rapid-fire pushes to the same branch cancel earlier
in-progress runs to prevent minute-stacking. `heavy.yml` has the same.

## When to apply `run-heavy` to a PR

Apply the label if any of:

- PR touches `crates/dat0-engine/src/**` (engine internals).
- PR touches `crates/dat0-fixtures/**` (generator changes).
- PR touches the `tests/exit_criteria.rs` assertions.
- PR is the consumer of a deferred attach/streaming fix.

Otherwise, the per-PR `ci.yml` suite is sufficient — exit_criteria is
about size-bound IO behavior which doesn't depend on most product code.

## Bypassing the linux-x86_64 self-hosted runner

If the runnerkit host is offline or being re-provisioned, the per-PR
linux-x86_64 job will queue indefinitely. To unblock without removing
the matrix entry:

1. Temporarily change `ci.yml` linux matrix entry's `labels` field to
   `'["ubuntu-latest"]'`.
2. Land your PR.
3. Revert the `labels` change once the self-hosted runner is healthy.

This burns hosted Linux minutes during the bypass window but unblocks
release-critical PRs.

## Escalating heavy.yml from weekly to nightly

QA4 enabled the weekly cron (`0 6 * * 1`). The original re-enable criteria
documented in `heavy.yml`'s header — team grows to 2+ devs, release cadence
demands continuous validation, or a drift incident shows a shorter interval
would have caught it earlier — now govern the step from **weekly to nightly**,
not from off to on. Change the one line:

```yaml
  schedule:
    - cron: '0 6 * * *'   # 06:00 UTC daily
```

Cost check before you do: the job is ~45 min on hosted Linux (1× billing), so
nightly is ~5.25 h/week against weekly's ~45 min.

## OOM mitigations (if needed in the future)

If self-hosted Linux Test step OOMs during dev-profile link, three knobs
in order of preference:

1. **Workspace `Cargo.toml`** — `[profile.dev] debug = "line-tables-only"`. **APPLIED in P8** after the hosted ubuntu Test step hit `No space left on device` (full-DWARF test binaries × ~9 new P8 test bins exhausted the ~14 GB disk; macOS has more disk and passed). Cuts dev-profile link RAM ~40-60% AND test-binary disk ~60% workspace-wide. Tradeoff: panic backtraces without inlined frames. Acceptable for CI.
2. **Per-job cargo concurrency** — set `CARGO_BUILD_JOBS: '2'` (or `'1'` on very low RAM) on the linux-x86_64 matrix entry's env block. Caps concurrent rustc/link processes.
3. **Host swap** — add 8-16 GB swap (`/swapfile` + `swapon`). Won't make CI fast but stops OOM cascade. Recommend regardless of (1)/(2).

Also: protect the runner agent itself from OOM. Set `OOMScoreAdjust=-500`
on the runner systemd service so the kernel reaps the linker before
killing the agent. Check runnerkit's unit file for the equivalent
setting.

## See also

- `docs/deferrals.md` — D-006 (macOS Intel), D-013 (macOS self-host),
  PD-003 (closed by QA2), PD-004 (open; `pd004-diagnose.yml` is its instrument).
- `docs/upstream-watch.md` — pinned-dep cadence; governs the five crates
  `.github/dependabot.yml` ignores.
- `deny.toml` — supply-chain policy; keep its `allow` list in sync with
  `about.toml`'s `accepted`.
- `scripts/i18n-check.sh` — the i18n gate; pass 2 is blocking.
- `.github/workflows/ci.yml`, `heavy.yml`, `notice.yml`, `crash-e2e.yml`,
  `pd004-diagnose.yml`, `release.yml` — sources of truth.
