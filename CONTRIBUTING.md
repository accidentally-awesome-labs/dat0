# Contributing to dat0

## Ways to contribute

Bug reports with a small synthetic reproduction, documentation fixes, accessibility
and Linux desktop improvements, format review, and focused code changes are welcome.
See [SUPPORT](SUPPORT.md) for questions and reporting routes. Maintainers curate
`good first issue` work with a bounded task and acceptance criteria; the label is
not a promise that every open issue is suitable for a newcomer.

Small fixes and documentation may go straight to a pull request. Discuss substantial
features and changes to the package format, persistence, or privacy in an issue or
[Discussion](https://github.com/accidentally-awesome-labs/dat0/discussions) first.
Report vulnerabilities and possible data leaks **privately** via [SECURITY](SECURITY.md),
not through that public discussion process.

## Setup

The supported development baseline is **Rust 1.97.0**, pinned in
[`rust-toolchain.toml`](rust-toolchain.toml), with Rust 2024. We do not claim a
separately tested older MSRV. Install [rustup](https://rustup.rs), then the tools below.

On **macOS**, install Xcode Command Line Tools with `xcode-select --install`.
On **Ubuntu/Debian**, install the native compiler and WebKit/keychain/chart dependencies:

```sh
sudo apt-get update
sudo apt-get install -y \
  build-essential pkg-config \
  libwebkit2gtk-4.1-dev libsoup-3.0-dev libxdo-dev \
  libsecret-1-dev dbus-x11 gnome-keyring \
  libpango1.0-dev libfontconfig1-dev \
  libxkbcommon-dev libxkbcommon-x11-dev libssl-dev
```

Running the desktop app requires a graphical session. Headless UI tests still
compile the native WebKit dependencies; a headless Rust test is not a way to
avoid installing them. Ordinary Rust development does not need system DuckDB or
Arrow, a Metal SDK, `dx`, Node, paid-service credentials, or signing keys.
Preserve [`.cargo/config.toml`](.cargo/config.toml): it includes the bundled-DuckDB
C++ shim and a development crash-service stub. Node/npm are needed only to rebuild
the checked-in CodeMirror bundle.

## Build, test, run

Run these commands from the repository root:

```sh
rustup toolchain install 1.97.0 --profile minimal --component rustfmt --component clippy
cargo install cargo-nextest --locked
cargo build --locked --workspace
cargo test --locked -p dat0-engine --test attach_sqlite -- --nocapture
cargo nextest run --locked --workspace
cargo test --locked --workspace --doc
cargo run --locked -p dat0-ui --bin dat0
```

The SQLite test primes the shared DuckDB extension cache before concurrent tests
and verifies attachment against a small fixture. Extension acquisition can require
network access. Nextest runs tests in separate processes, isolating process-global
state; it does not run doctests, hence the separate command. The built-in
`cargo test` runner is available, but is not an equivalent isolation model.
Ignored tests have distinct prerequisites; see [CI test categories](docs/ci.md#ignored-and-live-test-categories).
Never use workspace-wide `--include-ignored` as a routine contributor check.

To open a file, use `cargo run --locked -p dat0-ui --bin dat0 -- data.csv`.
A second launch with the same profile forwards its request to the existing process.
For speed investigations use a release build, not debug DuckDB:
`cargo run --locked --release -p dat0-ui --bin dat0`.

## Scratch profiles and troubleshooting

Generate small deterministic fixtures and run without changing your ordinary app settings:

```sh
scratch="$(mktemp -d /tmp/dat0-dev.XXXXXX)"
cargo run --locked -p dat0-fixtures -- \
  --out "$scratch/fixtures" --seed 42 \
  --csv-bytes 4096 --sqlite-target-bytes 4096
export DAT0_CONFIG_DIR="$scratch/config"
export DAT0_CACHE_DIR="$scratch/cache"
export XDG_DATA_HOME="$scratch/data"
export RUST_LOG=info
cargo run --locked -p dat0-ui --bin dat0
```

Complete or skip the first-run tour, then open `generated.csv` in the fixture directory
with the file picker; its columns are `id`, `name`, and `score`. From another terminal
with **identical profile variables**, run
`target/debug/dat0 "$scratch/fixtures/generated.csv"` to exercise forwarding. Close
all windows normally and reopen this profile to check that onboarding stays completed.
Keep the variables in every terminal; different profiles are different app instances.

`DAT0_CONFIG_DIR` relocates configuration and `DAT0_CACHE_DIR` relocates caches/logs.
Linux app data additionally uses `$XDG_DATA_HOME/dat0`; macOS app data shares the
configuration directory. These overrides do **not** isolate OS credential stores or
`~/.duckdb/extensions`. This is a convenience profile, not a security sandbox.
Default tests may download extensions. macOS keychain tests use a real `dat0-test`
item; explicitly opted-in Linux native-keychain tests need an unlocked Secret Service
session. Do not enter real service credentials for ordinary validation.

Missing `cc` or `pkg-config` means native build prerequisites are missing. WebKit,
Soup, or GTK lookup errors need the Linux packages above. Secret Service errors
need a session bus and unlocked keyring, not a fake keychain result. For extension
download failures check network access and the shared extension cache. Under memory
or disk pressure use `CARGO_BUILD_JOBS=2`, `CARGO_INCREMENTAL=0`, and consistently
set `CARGO_PROFILE_DEV_DEBUG=0` across build/test commands; changing profiles midway
causes rebuilds. [CI](docs/ci.md) explains disk reclamation, test isolation and
performance checks. Debug-versus-release timings are not comparable.

## Change-specific checks

| Contribution | Commands and contract |
|---|---|
| Rust changes | `cargo fmt --all -- --check`; `cargo clippy --locked --workspace --all-targets -- -D warnings`; relevant nextest targets plus doctests |
| Engine/package changes | `cargo nextest run --locked -p dat0-engine -p dat0-format`; preserve the [format](docs/dat0-format-v1.md) and [schema](docs/schemas/dat0-manifest-v1.schema.json), and exercise round-trip/corruption cases |
| UI behavior | `cargo nextest run --locked -p dat0-ui`; use the existing [`tests/support`](crates/dat0-ui/tests/support) harness and accessible handles, not calls that bypass user interaction |
| Appearance | `cargo nextest run --locked -p dat0-ui --test visual_snapshot`; intentional updates use `INSTA_UPDATE=always cargo test --locked -p dat0-ui --test visual_snapshot`, then inspect the actual snapshot changes |
| Real geometry/focus | `cargo run --locked -p dat0-ui --features visual --example visual_probe` on a desktop; CI runs eight real-window probes under Linux Xvfb via [windowed-probes.sh](.github/scripts/windowed-probes.sh) |
| Localization | Add entries in [`en.json`](crates/dat0-i18n/src/strings/en.json), use `dat0_i18n::t`, run `bash scripts/i18n-check.sh` and `cargo test --locked -p dat0-i18n`; referenced-key checking is blocking |
| CodeMirror | From `crates/dat0-ui/vendor/codemirror`: `npm ci`, `npm run build`; commit the rebuilt [`assets/codemirror.js`](crates/dat0-ui/assets/codemirror.js) and exercise `console_probe` |
| Dependencies/licensing | `cargo deny check`; retain deliberate DuckDB/Dioxus pins and the Arrow re-export policy. On Linux install `cargo-about` with `cargo install cargo-about --locked --features cli --version 0.9.2`, then run `scripts/notice-regen.sh`. On macOS use the failed-check correction artifact. Never overwrite NOTICE with raw cargo-about output |
| Heavy/performance/live tests | See [heavy tests](docs/ci.md#heavy-tests), [performance](docs/ci.md#performance) and [ignored/live categories](docs/ci.md#ignored-and-live-test-categories). Maintainers apply `run-heavy`/`run-perf` when relevant; these are not prerequisites for every PR |
| Public documentation/tree | `bash .github/scripts/check-public-tree.sh`; [offline link validation](docs/ci.md#documentation-and-publication-boundary). Run the tree check **before pushing** |

Keep `dat0-core` toolkit-free. Consume engine Arrow types through `duckdb::arrow`,
not a standalone Arrow dependency. Session shape changes require explicit versioning,
migration/default handling and round-trip compatibility checks. See [architecture](docs/architecture.md).
UI tests must drive keyboard events through production routing, preserve Escape
priority and release accessibility handles, use token-only styling, and check theme
contrast/switching. Reviewed SSR snapshots complement, not replace, real-window
geometry checks; see [accessibility](docs/a11y.md) and [CI](docs/ci.md#visual-and-interaction-checks).

## Pull requests, review and DCO

1. Fork [the canonical repository](https://github.com/accidentally-awesome-labs/dat0).
2. Clone your fork and add upstream (replace `YOUR-USER`):

   ```sh
   git clone https://github.com/YOUR-USER/dat0.git
   cd dat0
   git remote add upstream https://github.com/accidentally-awesome-labs/dat0.git
   git fetch upstream
   git switch -c your-focused-change upstream/main
   ```

3. Make a focused change and exercise the affected behavior. Record honest commands,
   results and any checks not run in the [PR template](.github/pull_request_template.md).
4. Sign off **every** commit: `git commit -s -m "Describe the change"`.
5. Push only your contribution branch and open a PR against upstream `main`.
6. Respond to maintainer review, resolve threads and update your branch if main moves.

The [Developer Certificate of Origin](https://developercertificate.org) certifies
that you wrote the contribution or have the right to submit it under Apache-2.0.
Use your own Git-configured name/email in `Signed-off-by`. Sign-off is **not** GPG
or SSH commit signing, and this project does not require a CLA. The installed DCO
app checks every PR; it remains a separately required check alongside **CI required**.
Do not use commit skip directives on PRs: required checks can remain pending.

To repair your last commit: `git commit --amend --no-edit --signoff`.
For your own contribution branch: `git rebase --signoff upstream/main`, then
`git push --force-with-lease` **only to that branch in your fork**. Never rewrite
another person's commits or upstream main. Check the DCO app's result after repair.

## Maintainer decisions and help

Maintainers decide scope and merges. Main requires a PR, successful CI/DCO checks,
an up-to-date branch and resolved review threads. With one maintainer there is no
required independent approval; outside contributors still cannot merge without
maintainer write authority. CODEOWNERS requests review, not enforcement by itself.
No phase-plan, deferrals-ledger or new RFC process is required.

Use [SUPPORT](SUPPORT.md) for help and [SECURITY](SECURITY.md) for private vulnerability
reports. Participation follows the [Code of Conduct](CODE_OF_CONDUCT.md).
Contributions are accepted under [Apache-2.0](LICENSE).
