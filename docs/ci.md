# CI Setup

> Operational reference for dat0's GitHub Actions setup. Covers per-PR
> validation (`ci.yml`), heavy exit-criteria tests (`heavy.yml`), NOTICE
> drift gate (`notice.yml`), and the runnerkit self-hosted Linux runner
> that backs the Linux matrix entry.

---

## Workflows

| File | Triggers | Jobs | Runner |
|---|---|---|---|
| `.github/workflows/ci.yml` | push to `main`, all PRs | fmt, clippy, i18n-check, build-and-test (macos-arm64, linux-x86_64) | hosted macos-14 + self-hosted Linux |
| `.github/workflows/heavy.yml` | `workflow_dispatch`, PRs with `run-heavy` label | exit-criteria (1 GB CSV + 500 MB Parquet + 100 MB SQLite) | self-hosted Linux |
| `.github/workflows/notice.yml` | NOTICE.md drift check | notice | hosted |

Nightly cron on `heavy.yml` is currently **commented out** — see workflow
header for re-enable criteria. Solo-dev + pre-P3 + low PR frequency
makes scheduled runs net-negative right now.

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

## Re-enabling nightly cron

If you reach one of the criteria documented in `heavy.yml`'s header
(team grows to 2+ devs, release cadence demands it, or drift incident
proves nightly value), uncomment the `schedule` block:

```yaml
on:
  schedule:
    - cron: '0 6 * * *'   # 06:00 UTC
  workflow_dispatch:
  pull_request:
    types: [opened, synchronize, reopened, labeled]
```

## OOM mitigations (if needed in the future)

If self-hosted Linux Test step OOMs during dev-profile link, three knobs
in order of preference:

1. **Workspace `Cargo.toml`** — `[profile.dev] debug = "line-tables-only"`. Cuts dev-profile link RAM ~40-60% workspace-wide. Tradeoff: panic backtraces without inlined frames. Acceptable for CI.
2. **Per-job cargo concurrency** — set `CARGO_BUILD_JOBS: '2'` (or `'1'` on very low RAM) on the linux-x86_64 matrix entry's env block. Caps concurrent rustc/link processes.
3. **Host swap** — add 8-16 GB swap (`/swapfile` + `swapon`). Won't make CI fast but stops OOM cascade. Recommend regardless of (1)/(2).

Also: protect the runner agent itself from OOM. Set `OOMScoreAdjust=-500`
on the runner systemd service so the kernel reaps the linker before
killing the agent. Check runnerkit's unit file for the equivalent
setting.

## See also

- `docs/deferrals.md` — D-006 (macOS Intel), D-013 (macOS self-host).
- `docs/upstream-watch.md` — pinned-dep cadence.
- `.github/workflows/ci.yml`, `heavy.yml`, `notice.yml` — sources of truth.
