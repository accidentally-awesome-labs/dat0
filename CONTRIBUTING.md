# Contributing to dat0

Thanks for your interest in dat0. This document covers how to contribute.

## Status

dat0 is implemented. The design specification
([`docs/specs/2026-04-26-dat0-design.md`](docs/specs/2026-04-26-dat0-design.md))
remains the reference for intent, but the workbench itself is real code —
build and run it with the steps under **Building from source** below.

Code-level contributions are open. Spec feedback, use-case scenarios that
reveal gaps, `.dat0` format review, and cross-platform tradeoff discussion
(especially Linux desktop polish) are all still welcome.

## Developer Certificate of Origin (DCO)

dat0 uses the **Developer Certificate of Origin** (DCO) — see <https://developercertificate.org>. By signing off on a commit, you certify that you wrote the code or have the right to submit it under this project's license (Apache-2.0).

### How to sign off

Add a `Signed-off-by` line to every commit. The simplest way is the `-s` flag:

```
git commit -s -m "your message"
```

This appends:

```
Signed-off-by: Your Name <your.email@example.com>
```

Use the same name and email as your `git config user.name` and `git config user.email` settings.

### DCO check

A bot enforces DCO sign-off on every pull request. Commits without a sign-off are flagged and must be amended before merge.

## Building from source

The repo builds with stable Rust. Prerequisites:

**All platforms**

- Rust toolchain — `rust-toolchain.toml` pins to stable. Install via [rustup](https://rustup.rs).
- `git` 2.30+

**macOS**

- Xcode Command Line Tools: `xcode-select --install`

That is the whole list. No Metal toolchain: dat0 renders through a WebKit
webview, not a GPU shader pipeline.

**Linux**

```sh
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev libsoup-3.0-dev \
  libsecret-1-dev dbus-x11 gnome-keyring \
  libpango1.0-dev libfontconfig1-dev \
  libxkbcommon-dev libxkbcommon-x11-dev \
  libssl-dev
```

- **webkit2gtk / soup** — the webview dat0's UI renders into. Nothing builds
  without these.
- **libsecret / dbus-x11 / gnome-keyring** — the `dat0-keychain` Secret Service
  backend.
- **pango / fontconfig** — text rendering for chart labels, through plotters'
  `ttf` feature.
- **xkbcommon** — keyboard handling in the windowing layer.
- **libssl-dev** — the `libduckdb-sys` build.

This is the same list CI installs (`.github/workflows/ci.yml`, "Linux deps"),
which is the set to trust if this one ever drifts again. Equivalent packages on
Fedora / Arch are similarly named.

**Build, test, run**

```sh
cargo build --workspace
cargo nextest run --workspace     # or: cargo test --workspace
cargo run --bin dat0
```

A standalone window titled "dat0" opens. Close it to exit. Pass a path to open
it — `cargo run --bin dat0 -- data.csv` — and note that a second launch while
one is already running forwards the file to the existing process rather than
starting a new one.

> **One test is expected to fail.** `dat0-core::update_key_is_production` is the
> release tripwire for the placeholder update-signing key; its own doc block
> opens "This test is EXPECTED TO FAIL on the current tree." A clean checkout
> reports one failure, and that is the one.

**Build a release binary for anything about speed.** DuckDB compiled without
optimisation is slow enough to mislead you about the thing dat0 exists to do:
`cargo run --release --bin dat0`.

**Testing against a scratch profile.** `DAT0_CONFIG_DIR` relocates the whole
config directory, so you can leave your own settings alone — and deleting it
gives you a genuine first run, which is the only way to see the onboarding tour
and the first-run empty state again:

```sh
DAT0_CONFIG_DIR=/tmp/dat0-scratch XDG_DATA_HOME=/tmp/dat0-scratch/data \
  RUST_LOG=info cargo run --bin dat0
```

**Note on `.cargo/config.toml`**: a stub value for `DAT0_GLITCHTIP_DSN_PUBLIC` is
baked at compile time; CI overrides it via secrets. Local dev needs no extra env
setup.

## How to propose changes

1. **Open an issue first** for non-trivial changes, especially around the spec. This avoids wasted work if the direction differs.
2. **Fork the repo**, branch from `main`.
3. **Make your changes**, with DCO-signed commits.
4. **Open a pull request**. The CI will run available checks; the DCO bot validates sign-off.
5. **Engage with review**. Maintainers may ask for changes or rationale.

## Coding standards

- Rust 2024 edition (workspace pins MSRV 1.85)
- `cargo fmt --all -- --check` clean (rustfmt config in `rustfmt.toml`; nightly-only options have been removed pending a CI fmt-strategy decision)
- `cargo clippy --workspace --all-targets -- -D warnings` clean
- Unit + integration tests required for new code. Component behaviour goes
  through the headless harness in `crates/dat0-ui/tests/support/`; appearance
  is covered by the SSR snapshots in `tests/visual_snapshot.rs` and, for real
  geometry, `examples/visual_probe.rs` (see "Visual gate" in `docs/ci.md`)
- All UI strings must pass through `dat0_i18n::t("…")`. The `scripts/i18n-check.sh` heuristic flags candidates; CI runs it in warn-only mode in P1 and tightens to a merge gate as the UI grows.

## Phase scope, deferrals, plan defects

Before opening a non-trivial PR, scan [`docs/deferrals.md`](docs/deferrals.md) — the canonical register of work split across phases and known plan defects. Phase plans live in `docs/plans/` and reference this register.

## Code of Conduct

By participating, you agree to abide by the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

Contributions are accepted under the terms of [Apache License 2.0](LICENSE).

## Where to ask questions

- GitHub Discussions (when the repo is public)
- Discord (link to be published at v1 launch)
- Spec questions: open an issue tagged `spec`
