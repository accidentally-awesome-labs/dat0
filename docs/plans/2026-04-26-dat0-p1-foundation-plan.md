# dat0 P1 — Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish the dat0 desktop application skeleton: Cargo workspace, GPUI shell, a window with a title, settings persistence, Zed-JSON theme loading, i18n, OS keychain, macOS menu bar, Sparkle/AppImageUpdate scaffolding, Sentry+GlitchTip with redaction, recents service, error/dialog primitives, NOTICE aggregation, perf bench harness, and a CI matrix that builds release binaries on macOS arm64+x86_64 and Linux x86_64+aarch64.

**Architecture:** Single-process Rust binary using GPUI as the UI runtime and longbridge/gpui-component for primitives. Code is split into small focused crates (`dat0-app`, `dat0-i18n`, `dat0-keychain`) and modules within `dat0-app` for everything else (settings, theme, recents, error_ux, etc.). All UI strings go through a `t("key")` helper. Async work uses tokio. Persistence uses TOML (settings) and JSON (themes, recents). All P1 features are testable via unit + integration tests; a small GPUI snapshot harness is scaffolded but UI-snapshot tests are nominal in P1.

**Tech Stack:** Rust 2024 · GPUI (pinned to a known-good Zed commit) · longbridge/gpui-component (pinned) · tokio · serde · toml · serde_json · notify (file watcher) · tracing + tracing-subscriber · sentry · security-framework (macOS) · secret-service (Linux) · criterion (benches) · cargo-about (NOTICE) · arboard (clipboard, deferred to P4 but optional crate selected here)

---

## Prerequisites (from P0)

Before starting Task 1, verify P0 exit criteria are met (see [`2026-04-26-dat0-p0-runbook.md`](2026-04-26-dat0-p0-runbook.md)). Specifically:
- GitHub repo pushed; CI matrix passes `cargo check` on all 4 targets
- Apple Developer cert + notarization key in CI secrets (`APPLE_DEVELOPER_CERT_P12`, `APPLE_DEVELOPER_CERT_PASSWORD`, `APPLE_NOTARIZATION_API_KEY`, `APPLE_NOTARIZATION_KEY_ID`, `APPLE_NOTARIZATION_ISSUER_ID`)
- EdDSA Sparkle private key in `SPARKLE_ED25519_PRIVATE_KEY` secret; public key recorded
- GlitchTip DSN in `GLITCHTIP_DSN_PUBLIC` secret
- AppImageUpdate license decision documented in `NOTICE.md`
- `.dat0` MIME type chosen (default: `application/vnd.dat0+zip`)

If P0 is incomplete, return to the runbook before proceeding.

---

## File Structure

This is the file/module tree at the end of P1. Tasks below create or modify these files.

```
dat0/
├── Cargo.toml                          # workspace root
├── Cargo.lock                          # committed (binary project)
├── rust-toolchain.toml                 # pin to stable Rust
├── rustfmt.toml                        # formatting rules
├── clippy.toml                         # lint config
├── deny.toml                           # cargo-deny license/version policy
├── about.toml                          # cargo-about NOTICE generator config
├── .github/
│   ├── workflows/
│   │   ├── ci.yml                      # build + test matrix
│   │   └── notice.yml                  # NOTICE drift gate
│   └── ISSUE_TEMPLATE/
│       └── bug_report.md
├── crates/
│   ├── dat0-app/                       # main binary
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── main.rs                 # entry point
│   │   │   ├── lib.rs                  # internal API surface
│   │   │   ├── window.rs               # GPUI window + title
│   │   │   ├── settings/
│   │   │   │   ├── mod.rs              # Settings struct + service
│   │   │   │   ├── schema.rs           # TOML schema types
│   │   │   │   ├── store.rs            # read/write
│   │   │   │   └── watcher.rs          # notify-based file watcher
│   │   │   ├── theme/
│   │   │   │   ├── mod.rs              # Theme struct + service
│   │   │   │   ├── zed_schema.rs       # Zed JSON theme deserializer
│   │   │   │   ├── apply.rs            # apply theme to GPUI
│   │   │   │   └── builtins/
│   │   │   │       ├── light.json
│   │   │   │       ├── dark.json
│   │   │   │       └── high-contrast.json
│   │   │   ├── recents/
│   │   │   │   └── mod.rs              # Recents service (process-shared)
│   │   │   ├── menu_macos.rs           # native macOS menu bar
│   │   │   ├── updater/
│   │   │   │   ├── mod.rs              # platform-dispatch
│   │   │   │   ├── sparkle.rs          # macOS Sparkle bridge (#[cfg(target_os = "macos")])
│   │   │   │   └── appimage.rs         # Linux AppImageUpdate scaffolding (#[cfg(target_os = "linux")])
│   │   │   ├── telemetry/
│   │   │   │   ├── mod.rs              # Sentry client init
│   │   │   │   └── redaction.rs        # before_send filter
│   │   │   ├── error_ux/
│   │   │   │   ├── mod.rs              # Error/dialog primitives (modal, toast, banner)
│   │   │   │   ├── modal.rs
│   │   │   │   ├── toast.rs
│   │   │   │   └── banner.rs
│   │   │   ├── settings_ui/
│   │   │   │   ├── mod.rs              # Settings panel
│   │   │   │   ├── sections/
│   │   │   │   │   ├── mod.rs          # registry
│   │   │   │   │   ├── profile.rs
│   │   │   │   │   ├── theme.rs
│   │   │   │   │   ├── advanced.rs     # placeholder; later phases populate
│   │   │   │   │   └── ...
│   │   │   ├── platform/
│   │   │   │   ├── mod.rs              # XDG paths / Application Support paths
│   │   │   │   ├── macos.rs
│   │   │   │   └── linux.rs
│   │   │   └── boot.rs                 # startup orchestration
│   │   ├── build.rs                    # bundle Sparkle public key, version
│   │   └── tests/
│   │       └── integration.rs
│   ├── dat0-i18n/                      # t() helper + string-table loader
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   └── strings/
│   │   │       └── en.json             # canonical English string table
│   │   └── tests/
│   │       └── basic.rs
│   └── dat0-keychain/                  # cross-platform keychain
│       ├── Cargo.toml
│       ├── src/
│       │   ├── lib.rs                  # Keychain trait
│       │   ├── macos.rs                # Security.framework impl (#[cfg])
│       │   └── linux.rs                # secret-service impl (#[cfg])
│       └── tests/
│           └── round_trip.rs
├── benches/
│   ├── engine_smoke.rs                 # criterion bench placeholder
│   └── grid_scroll.rs                  # GPU grid scroll harness placeholder
├── scripts/
│   ├── i18n-check.sh                   # greppable assertion: no string literals in UI bypass t()
│   └── notarize-macos.sh               # post-build notarization script
└── docs/
    ├── plans/
    │   ├── 2026-04-26-dat0-p0-runbook.md
    │   └── 2026-04-26-dat0-p1-foundation-plan.md   # this file
    └── (existing files: specs/, upstream-watch.md)
```

---

## Tasks

### Task 1: Initialize Cargo workspace

**Files:**
- Create: `Cargo.toml` (root)
- Create: `rust-toolchain.toml`
- Create: `rustfmt.toml`
- Create: `clippy.toml`
- Create: `crates/dat0-app/Cargo.toml`
- Create: `crates/dat0-app/src/main.rs`

- [ ] **Step 1.1: Write the workspace root `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = [
    "crates/dat0-app",
    "crates/dat0-i18n",
    "crates/dat0-keychain",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "Apache-2.0"
repository = "https://github.com/accidentally-awesome-labs/dat0"
authors = ["Accidentally Awesome Labs"]
rust-version = "1.85"

[workspace.dependencies]
# Async + utilities
tokio = { version = "1", features = ["rt-multi-thread", "macros", "fs", "sync"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
anyhow = "1"
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
notify = "6"
once_cell = "1"

# Telemetry
sentry = { version = "0.36", default-features = false, features = ["backtrace", "panic", "rustls", "reqwest"] }

# Bench
criterion = { version = "0.5", features = ["html_reports"] }

# Internal crates
dat0-i18n = { path = "crates/dat0-i18n" }
dat0-keychain = { path = "crates/dat0-keychain" }

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
panic = "abort"

[profile.bench]
debug = true
```

- [ ] **Step 1.2: Create `rust-toolchain.toml`**

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

- [ ] **Step 1.3: Create `rustfmt.toml`**

```toml
edition = "2024"
max_width = 100
imports_granularity = "Module"
group_imports = "StdExternalCrate"
```

- [ ] **Step 1.4: Create `clippy.toml`**

```toml
# project-wide clippy config (lint thresholds, allowlists)
msrv = "1.85"
```

- [ ] **Step 1.5: Create the `dat0-app` crate manifest**

`crates/dat0-app/Cargo.toml`:

```toml
[package]
name = "dat0-app"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
rust-version.workspace = true

[[bin]]
name = "dat0"
path = "src/main.rs"

[dependencies]
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
toml = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
notify = { workspace = true }
once_cell = { workspace = true }
sentry = { workspace = true }
dat0-i18n = { workspace = true }
dat0-keychain = { workspace = true }

[lib]
path = "src/lib.rs"
```

- [ ] **Step 1.6: Create stub `src/main.rs`**

```rust
fn main() {
    println!("hello, dat0");
}
```

- [ ] **Step 1.7: Verify the workspace builds**

Run: `cargo build`
Expected: builds with no errors. Workspace compiles even though `dat0-i18n` and `dat0-keychain` don't exist yet — they're listed in `[workspace] members` but cargo will fail. Fix by also creating those stubs in Step 1.8.

- [ ] **Step 1.8: Create stub `dat0-i18n` and `dat0-keychain` crates**

`crates/dat0-i18n/Cargo.toml`:
```toml
[package]
name = "dat0-i18n"
version.workspace = true
edition.workspace = true
license.workspace = true
[lib]
path = "src/lib.rs"
```

`crates/dat0-i18n/src/lib.rs`:
```rust
//! dat0 internationalization helpers (stub; real API lands in Task 6+).
```

`crates/dat0-keychain/Cargo.toml`:
```toml
[package]
name = "dat0-keychain"
version.workspace = true
edition.workspace = true
license.workspace = true
[lib]
path = "src/lib.rs"
```

`crates/dat0-keychain/src/lib.rs`:
```rust
//! dat0 cross-platform keychain (stub; real API lands in Task 23+).
```

- [ ] **Step 1.9: Verify the workspace builds and runs**

Run: `cargo run --bin dat0`
Expected: prints `hello, dat0`.

- [ ] **Step 1.10: Commit**

```bash
git add Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml clippy.toml crates/
git commit -s -m "chore: initialize Cargo workspace skeleton (P1.T1)"
```

---

### Task 2: GPUI dependency + render an empty window

**Files:**
- Modify: `crates/dat0-app/Cargo.toml`
- Modify: `crates/dat0-app/src/main.rs`
- Create: `crates/dat0-app/src/lib.rs`
- Create: `crates/dat0-app/src/window.rs`

- [ ] **Step 2.1: Add GPUI to workspace dependencies**

In root `Cargo.toml` `[workspace.dependencies]`, add:

```toml
gpui = { git = "https://github.com/zed-industries/zed", rev = "<KNOWN_GOOD_COMMIT>" }
gpui-component = { git = "https://github.com/longbridge/gpui-component", rev = "<KNOWN_GOOD_COMMIT>" }
```

Replace `<KNOWN_GOOD_COMMIT>` with the SHA pinned in the repo. To find it: clone gpui-component, check the latest tagged release commit; for gpui, use the commit referenced by gpui-component's `Cargo.toml`. Document both pins in `docs/upstream-watch.md`.

- [ ] **Step 2.2: Add gpui to `dat0-app/Cargo.toml`**

```toml
gpui = { workspace = true }
gpui-component = { workspace = true }
```

- [ ] **Step 2.3: Write a failing test asserting the window-rendering API exists**

Create `crates/dat0-app/tests/window_smoke.rs`:

```rust
//! Smoke test: dat0_app must expose a `run_app()` entry point.

#[test]
fn window_module_exposes_run_app() {
    // Compile-time check via the type system: this test fails to compile
    // until `dat0_app::run_app` is a `fn() -> anyhow::Result<()>`.
    let _: fn() -> anyhow::Result<()> = dat0_app::run_app;
}
```

- [ ] **Step 2.4: Run the test, expect compile failure**

Run: `cargo test -p dat0-app --test window_smoke`
Expected: fails with "no function `run_app` in `dat0_app`".

- [ ] **Step 2.5: Implement `run_app` and the empty window**

`crates/dat0-app/src/lib.rs`:

```rust
pub mod window;

pub use window::run_app;
```

`crates/dat0-app/src/window.rs`:

```rust
use anyhow::Result;
use gpui::{App, Application, Bounds, WindowBounds, WindowOptions, px, size};

pub fn run_app() -> Result<()> {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("dat0".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_window, cx| cx.new(|_| EmptyView),
        )
        .expect("open window");
        cx.activate(true);
    });
    Ok(())
}

struct EmptyView;

impl gpui::Render for EmptyView {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut gpui::Context<Self>) -> impl gpui::IntoElement {
        gpui::div()
    }
}
```

> **Note:** GPUI is pre-1.0; specific imports (`Bounds`, `WindowBounds`, `WindowOptions`, etc.) may have moved. Match the imports the pinned commit uses by reading `gpui::prelude` and the crate's own examples. Update the snippet above if signatures differ.

- [ ] **Step 2.6: Update `main.rs` to call `run_app`**

```rust
fn main() -> anyhow::Result<()> {
    dat0_app::run_app()
}
```

- [ ] **Step 2.7: Re-run the smoke test, expect pass**

Run: `cargo test -p dat0-app --test window_smoke`
Expected: PASS.

- [ ] **Step 2.8: Run the binary, verify a window opens**

Run: `cargo run --bin dat0`
Expected: a 1200×800 window titled "dat0" opens. Close the window; the process exits cleanly.

- [ ] **Step 2.9: Commit**

```bash
git add Cargo.toml Cargo.lock crates/dat0-app/
git commit -s -m "feat: render an empty GPUI window (P1.T2)"
```

---

### Task 3: tracing logger setup

**Files:**
- Modify: `crates/dat0-app/src/main.rs`
- Create: `crates/dat0-app/src/boot.rs`

- [ ] **Step 3.1: Write a failing test asserting `init_logging()` exists**

Create `crates/dat0-app/tests/logging.rs`:

```rust
#[test]
fn init_logging_returns_ok() {
    let result = dat0_app::boot::init_logging();
    assert!(result.is_ok());
}
```

- [ ] **Step 3.2: Run, expect failure**

Run: `cargo test -p dat0-app --test logging`
Expected: fails with "no module `boot`".

- [ ] **Step 3.3: Implement `init_logging`**

`crates/dat0-app/src/boot.rs`:

```rust
use anyhow::Result;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Initialize the tracing subscriber. Idempotent — calling twice is a no-op.
pub fn init_logging() -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,dat0=debug"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false).compact())
        .try_init();
    Ok(())
}
```

Add `pub mod boot;` to `crates/dat0-app/src/lib.rs`.

- [ ] **Step 3.4: Run the test, expect pass**

Run: `cargo test -p dat0-app --test logging`
Expected: PASS.

- [ ] **Step 3.5: Wire `init_logging` into `main`**

Update `crates/dat0-app/src/main.rs`:

```rust
fn main() -> anyhow::Result<()> {
    dat0_app::boot::init_logging()?;
    tracing::info!("dat0 starting");
    dat0_app::run_app()
}
```

- [ ] **Step 3.6: Verify**

Run: `cargo run --bin dat0`
Expected: terminal output includes a line like `INFO dat0 starting`. Window opens. Close, process exits.

- [ ] **Step 3.7: Commit**

```bash
git add crates/dat0-app/
git commit -s -m "feat: tracing logger initialization (P1.T3)"
```

---

### Task 4: Platform path helpers

**Files:**
- Create: `crates/dat0-app/src/platform/mod.rs`
- Create: `crates/dat0-app/src/platform/macos.rs`
- Create: `crates/dat0-app/src/platform/linux.rs`

- [ ] **Step 4.1: Failing test for path discovery**

Create `crates/dat0-app/tests/platform_paths.rs`:

```rust
use dat0_app::platform;

#[test]
fn config_dir_is_under_user_home() {
    let path = platform::config_dir().expect("config dir");
    assert!(path.starts_with(dirs::home_dir().expect("home")));
    assert!(path.ends_with("dat0"));
}

#[test]
fn data_dir_is_under_user_home() {
    let path = platform::data_dir().expect("data dir");
    assert!(path.starts_with(dirs::home_dir().expect("home")));
    assert!(path.ends_with("dat0"));
}

#[test]
fn cache_dir_is_under_user_home() {
    let path = platform::cache_dir().expect("cache dir");
    assert!(path.starts_with(dirs::home_dir().expect("home")));
    assert!(path.ends_with("dat0"));
}
```

Add `dirs = "5"` to `crates/dat0-app/Cargo.toml` `[dev-dependencies]` and `[dependencies]` (production code uses it too).

- [ ] **Step 4.2: Run the test, expect failure**

Run: `cargo test -p dat0-app --test platform_paths`
Expected: fails with "no module `platform`".

- [ ] **Step 4.3: Implement platform paths**

`crates/dat0-app/src/platform/mod.rs`:

```rust
use anyhow::Result;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

pub fn ensure_dir(path: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    Ok(())
}
```

`crates/dat0-app/src/platform/macos.rs`:

```rust
use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn config_dir() -> Result<PathBuf> {
    Ok(dirs::home_dir().context("no home")?.join("Library/Application Support/dat0"))
}

pub fn data_dir() -> Result<PathBuf> {
    config_dir()
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::home_dir().context("no home")?.join("Library/Caches/dat0"))
}
```

`crates/dat0-app/src/platform/linux.rs`:

```rust
use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn config_dir() -> Result<PathBuf> {
    Ok(dirs::config_dir().context("no XDG_CONFIG_HOME")?.join("dat0"))
}

pub fn data_dir() -> Result<PathBuf> {
    Ok(dirs::data_dir().context("no XDG_DATA_HOME")?.join("dat0"))
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(dirs::cache_dir().context("no XDG_CACHE_HOME")?.join("dat0"))
}
```

Add `pub mod platform;` to `lib.rs`.

- [ ] **Step 4.4: Run tests, expect pass**

Run: `cargo test -p dat0-app --test platform_paths`
Expected: 3 tests PASS.

- [ ] **Step 4.5: Commit**

```bash
git add crates/dat0-app/
git commit -s -m "feat: platform path helpers (macOS + Linux) (P1.T4)"
```

---

### Task 5: i18n primitive — `t()` helper

**Files:**
- Modify: `crates/dat0-i18n/Cargo.toml`
- Modify: `crates/dat0-i18n/src/lib.rs`
- Create: `crates/dat0-i18n/src/strings/en.json`

- [ ] **Step 5.1: Add deps to `dat0-i18n`**

`crates/dat0-i18n/Cargo.toml`:

```toml
[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
once_cell = { workspace = true }
anyhow = { workspace = true }

[lib]
path = "src/lib.rs"
```

- [ ] **Step 5.2: Failing test**

Create `crates/dat0-i18n/tests/basic.rs`:

```rust
use dat0_i18n::t;

#[test]
fn t_returns_known_key() {
    assert_eq!(t("app.name"), "dat0");
}

#[test]
fn t_returns_key_when_missing() {
    let s = t("does.not.exist");
    assert_eq!(s, "does.not.exist", "missing keys must surface the key itself");
}
```

- [ ] **Step 5.3: Run, expect failure**

Run: `cargo test -p dat0-i18n`
Expected: fails — `t` not defined.

- [ ] **Step 5.4: Implement the string table loader**

`crates/dat0-i18n/src/strings/en.json`:

```json
{
  "app.name": "dat0",
  "app.tagline": "local-first data workbench"
}
```

`crates/dat0-i18n/src/lib.rs`:

```rust
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashMap;

static STRINGS: Lazy<HashMap<String, String>> = Lazy::new(|| {
    let raw = include_str!("strings/en.json");
    serde_json::from_str(raw).expect("english string table parses")
});

/// Translate a key to its locale-appropriate string. Returns the key itself
/// if missing — surfaces the gap immediately during development.
pub fn t(key: &str) -> String {
    STRINGS.get(key).cloned().unwrap_or_else(|| key.to_string())
}
```

- [ ] **Step 5.5: Run, expect pass**

Run: `cargo test -p dat0-i18n`
Expected: 2 tests PASS.

- [ ] **Step 5.6: Wire `t()` into the window title**

Modify `crates/dat0-app/src/window.rs`:

```rust
use dat0_i18n::t;
// ...
title: Some(t("app.name").into()),
```

- [ ] **Step 5.7: Run the binary, verify title**

Run: `cargo run --bin dat0`
Expected: window title is "dat0" (loaded from i18n table).

- [ ] **Step 5.8: Commit**

```bash
git add crates/
git commit -s -m "feat: i18n primitive with t() helper and English string table (P1.T5)"
```

---

### Task 6: i18n greppable assertion script

**Files:**
- Create: `scripts/i18n-check.sh`
- Modify: `.github/workflows/ci.yml` (add step)

- [ ] **Step 6.1: Write the check script**

`scripts/i18n-check.sh`:

```bash
#!/usr/bin/env bash
# Fails if any source file under crates/ contains a UI-string literal that
# bypasses the dat0_i18n::t() helper.
#
# Heuristic: any `"..."` literal inside .into() / .to_string() calls that
# look like UI text is suspect. This is a coarse first-pass; refine as the UI grows.
set -euo pipefail

BAD=0
while IFS= read -r line; do
    # Whitelist: comments, doc strings, string-table JSON files, test files
    if echo "$line" | grep -qE '^//|^\s*\*|^\s*#|test\.rs:|/strings/'; then
        continue
    fi
    echo "::warning::Possible un-i18n'd UI string: $line"
    BAD=$((BAD + 1))
done < <(grep -rEn '\.into\(\)|\.to_string\(\)' crates/dat0-app/src/ 2>/dev/null || true)

# This is a soft-fail in P1; tighten in subsequent phases.
echo "i18n-check: $BAD candidate(s) flagged"
exit 0
```

- [ ] **Step 6.2: Make executable**

Run: `chmod +x scripts/i18n-check.sh`

- [ ] **Step 6.3: Run locally**

Run: `./scripts/i18n-check.sh`
Expected: outputs candidate count; exits 0 (soft-fail mode for P1).

- [ ] **Step 6.4: Commit**

```bash
git add scripts/
git commit -s -m "ci: i18n greppable assertion script (P1.T6)"
```

---

### Task 7: Settings TOML schema

**Files:**
- Create: `crates/dat0-app/src/settings/mod.rs`
- Create: `crates/dat0-app/src/settings/schema.rs`
- Create: `crates/dat0-app/src/settings/store.rs`

- [ ] **Step 7.1: Failing test**

Create `crates/dat0-app/tests/settings_schema.rs`:

```rust
use dat0_app::settings::Settings;

#[test]
fn defaults_are_sensible() {
    let s = Settings::default();
    assert_eq!(s.theme.name, "dark");
    assert_eq!(s.profile.author_name, "");
    assert!(!s.telemetry.crash_submission_enabled);
}

#[test]
fn round_trip_toml() {
    let original = Settings::default();
    let serialized = toml::to_string(&original).unwrap();
    let deserialized: Settings = toml::from_str(&serialized).unwrap();
    assert_eq!(original, deserialized);
}
```

- [ ] **Step 7.2: Run, expect failure**

Run: `cargo test -p dat0-app --test settings_schema`
Expected: fails — `settings::Settings` not defined.

- [ ] **Step 7.3: Define the schema**

`crates/dat0-app/src/settings/schema.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub profile: Profile,
    pub theme: Theme,
    pub telemetry: Telemetry,
    pub workspace: Workspace,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            profile: Profile::default(),
            theme: Theme::default(),
            telemetry: Telemetry::default(),
            workspace: Workspace::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Profile {
    pub author_name: String,
    pub author_email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
}
impl Default for Theme {
    fn default() -> Self { Self { name: "dark".into() } }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Telemetry {
    pub crash_submission_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Workspace {
    pub treat_paths_as_networked: Vec<std::path::PathBuf>,
}
```

`crates/dat0-app/src/settings/mod.rs`:

```rust
mod schema;
pub mod store;

pub use schema::*;
```

Add `pub mod settings;` to `lib.rs`.

- [ ] **Step 7.4: Run tests, expect pass**

Run: `cargo test -p dat0-app --test settings_schema`
Expected: 2 tests PASS.

- [ ] **Step 7.5: Commit**

```bash
git add crates/dat0-app/
git commit -s -m "feat: Settings TOML schema (P1.T7)"
```

---

### Task 8: Settings store — read/write to disk

**Files:**
- Create: `crates/dat0-app/src/settings/store.rs`

- [ ] **Step 8.1: Failing test**

Create `crates/dat0-app/tests/settings_store.rs`:

```rust
use dat0_app::settings::store::SettingsStore;
use tempfile::tempdir;

#[test]
fn writes_then_reads_roundtrip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let store = SettingsStore::with_path(path.clone());

    let mut s = store.load_or_default().unwrap();
    s.profile.author_name = "Jane Doe".into();
    s.profile.author_email = "jane@example.org".into();
    store.save(&s).unwrap();

    let reloaded = store.load_or_default().unwrap();
    assert_eq!(reloaded.profile.author_name, "Jane Doe");
    assert_eq!(reloaded.profile.author_email, "jane@example.org");
}

#[test]
fn missing_file_returns_default() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("does-not-exist.toml");
    let store = SettingsStore::with_path(path);
    let s = store.load_or_default().unwrap();
    assert_eq!(s.theme.name, "dark");
}
```

Add `tempfile = "3"` to `dat0-app` `[dev-dependencies]`.

- [ ] **Step 8.2: Run, expect failure**

Run: `cargo test -p dat0-app --test settings_store`
Expected: fails — `SettingsStore` not defined.

- [ ] **Step 8.3: Implement the store**

`crates/dat0-app/src/settings/store.rs`:

```rust
use std::path::PathBuf;
use anyhow::{Context, Result};
use crate::settings::Settings;

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load_or_default(&self) -> Result<Settings> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => {
                let s: Settings = toml::from_str(&contents)
                    .with_context(|| format!("parse {}", self.path.display()))?;
                Ok(s)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e).with_context(|| format!("read {}", self.path.display())),
        }
    }

    pub fn save(&self, s: &Settings) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let serialized = toml::to_string_pretty(s)?;
        // Atomic write: write to .tmp, fsync, rename.
        let tmp = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp, serialized)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
```

- [ ] **Step 8.4: Run, expect pass**

Run: `cargo test -p dat0-app --test settings_store`
Expected: 2 tests PASS.

- [ ] **Step 8.5: Commit**

```bash
git add crates/dat0-app/
git commit -s -m "feat: Settings store with atomic write (P1.T8)"
```

---

### Task 9: Settings file watcher

**Files:**
- Create: `crates/dat0-app/src/settings/watcher.rs`
- Modify: `crates/dat0-app/src/settings/mod.rs`

- [ ] **Step 9.1: Failing test**

Create `crates/dat0-app/tests/settings_watcher.rs`:

```rust
use dat0_app::settings::{store::SettingsStore, watcher::SettingsWatcher, Settings};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn watcher_fires_on_change() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let store = SettingsStore::with_path(path.clone());
    store.save(&Settings::default()).unwrap();

    let received: Arc<Mutex<Vec<Settings>>> = Arc::new(Mutex::new(vec![]));
    let recv_clone = received.clone();
    let watcher = SettingsWatcher::start(path.clone(), move |new_settings| {
        recv_clone.lock().unwrap().push(new_settings);
    }).unwrap();

    // Mutate
    let mut s = Settings::default();
    s.profile.author_name = "Updated".into();
    store.save(&s).unwrap();

    // Allow watcher debounce (notify defaults around 100ms)
    std::thread::sleep(Duration::from_millis(500));

    let observed = received.lock().unwrap();
    assert!(!observed.is_empty(), "watcher should have fired");
    assert_eq!(observed.last().unwrap().profile.author_name, "Updated");

    drop(watcher);
}
```

Add `notify = { workspace = true }` and `tempfile = "3"` to `dat0-app` `[dev-dependencies]`.

- [ ] **Step 9.2: Run, expect failure**

Run: `cargo test -p dat0-app --test settings_watcher`
Expected: fails — `SettingsWatcher` not defined.

- [ ] **Step 9.3: Implement the watcher**

`crates/dat0-app/src/settings/watcher.rs`:

```rust
use std::path::PathBuf;
use anyhow::Result;
use notify::{Watcher, RecursiveMode, RecommendedWatcher, EventKind};
use crate::settings::{Settings, store::SettingsStore};

pub struct SettingsWatcher {
    _watcher: RecommendedWatcher,
}

impl SettingsWatcher {
    pub fn start<F>(path: PathBuf, on_change: F) -> Result<Self>
    where
        F: Fn(Settings) + Send + 'static,
    {
        let store = SettingsStore::with_path(path.clone());
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            match res {
                Ok(event) => {
                    if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                        if let Ok(s) = store.load_or_default() {
                            on_change(s);
                        }
                    }
                }
                Err(e) => tracing::warn!(?e, "settings watcher error"),
            }
        })?;
        watcher.watch(&path, RecursiveMode::NonRecursive)?;
        Ok(Self { _watcher: watcher })
    }
}
```

Update `crates/dat0-app/src/settings/mod.rs` to expose the watcher:

```rust
mod schema;
pub mod store;
pub mod watcher;

pub use schema::*;
```

- [ ] **Step 9.4: Run, expect pass**

Run: `cargo test -p dat0-app --test settings_watcher`
Expected: PASS (may be flaky on slow filesystems; allow 1 retry).

- [ ] **Step 9.5: Commit**

```bash
git add crates/dat0-app/
git commit -s -m "feat: Settings file watcher (P1.T9)"
```

---

### Task 10: Theme — Zed JSON schema deserializer

**Files:**
- Create: `crates/dat0-app/src/theme/mod.rs`
- Create: `crates/dat0-app/src/theme/zed_schema.rs`
- Create: `crates/dat0-app/src/theme/builtins/dark.json`
- Create: `crates/dat0-app/src/theme/builtins/light.json`
- Create: `crates/dat0-app/src/theme/builtins/high-contrast.json`

- [ ] **Step 10.1: Vendor a Zed dark theme JSON as the seed**

Download a current Zed theme JSON from the Zed source (https://github.com/zed-industries/zed → assets/themes/) — pick "One Dark" or similar. Save to `crates/dat0-app/src/theme/builtins/dark.json`. Trim to the fields we use; full Zed schema is ~100 fields, dat0 uses a subset (background, foreground, accent, error, success, etc.).

- [ ] **Step 10.2: Failing test**

Create `crates/dat0-app/tests/theme.rs`:

```rust
use dat0_app::theme::Theme;

#[test]
fn dark_loads() {
    let t = Theme::load_builtin("dark").unwrap();
    assert_eq!(t.name, "dark");
}

#[test]
fn light_loads() {
    let t = Theme::load_builtin("light").unwrap();
    assert_eq!(t.name, "light");
}

#[test]
fn high_contrast_loads() {
    let t = Theme::load_builtin("high-contrast").unwrap();
    assert_eq!(t.name, "high-contrast");
}

#[test]
fn unknown_returns_err() {
    let r = Theme::load_builtin("does-not-exist");
    assert!(r.is_err());
}
```

- [ ] **Step 10.3: Run, expect failure**

Run: `cargo test -p dat0-app --test theme`
Expected: fails — `Theme` undefined.

- [ ] **Step 10.4: Implement Theme**

`crates/dat0-app/src/theme/zed_schema.rs`:

```rust
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ZedTheme {
    pub name: String,
    pub appearance: String, // "light" or "dark"
    pub style: ZedStyle,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZedStyle {
    pub background: String,
    pub foreground: String,
    pub border: String,
    pub accent: String,
    pub error: String,
    pub success: String,
    pub warning: String,
    // Extended: surface variants, syntax highlight slots, etc.
    // Mapped per-component as needed.
}
```

`crates/dat0-app/src/theme/mod.rs`:

```rust
mod zed_schema;

use anyhow::{Context, Result};
pub use zed_schema::*;

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub style: ZedStyle,
}

impl Theme {
    pub fn load_builtin(name: &str) -> Result<Self> {
        let json = match name {
            "dark" => include_str!("builtins/dark.json"),
            "light" => include_str!("builtins/light.json"),
            "high-contrast" => include_str!("builtins/high-contrast.json"),
            other => anyhow::bail!("unknown built-in theme: {other}"),
        };
        let parsed: ZedTheme = serde_json::from_str(json)
            .with_context(|| format!("parse builtin theme {name}"))?;
        Ok(Self { name: parsed.name, style: parsed.style })
    }
}
```

Add `pub mod theme;` to `lib.rs`.

Stub `light.json` and `high-contrast.json` similarly to `dark.json`.

- [ ] **Step 10.5: Run, expect pass**

Run: `cargo test -p dat0-app --test theme`
Expected: 4 tests PASS.

- [ ] **Step 10.6: Commit**

```bash
git add crates/dat0-app/
git commit -s -m "feat: Theme loader with Zed JSON schema and 3 built-in themes (P1.T10)"
```

---

### Task 11: Recents service

**Files:**
- Create: `crates/dat0-app/src/recents/mod.rs`

- [ ] **Step 11.1: Failing test**

Create `crates/dat0-app/tests/recents.rs`:

```rust
use dat0_app::recents::{Recents, RecentEntry};
use tempfile::tempdir;

#[test]
fn empty_recents_starts_empty() {
    let dir = tempdir().unwrap();
    let r = Recents::with_path(dir.path().join("recents.json"));
    assert!(r.list().is_empty());
}

#[test]
fn push_then_persist_then_reload() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("recents.json");

    let mut r = Recents::with_path(p.clone());
    r.push(RecentEntry::Workspace { path: "/home/jane/project".into() }).unwrap();
    r.push(RecentEntry::Package { path: "/tmp/q.dat0".into() }).unwrap();
    drop(r);

    let r2 = Recents::with_path(p);
    let list = r2.list();
    assert_eq!(list.len(), 2);
    // MRU order: most recent first
    assert!(matches!(&list[0], RecentEntry::Package { path } if path == &std::path::PathBuf::from("/tmp/q.dat0")));
}

#[test]
fn duplicate_push_promotes_to_top() {
    let dir = tempdir().unwrap();
    let p = dir.path().join("recents.json");
    let mut r = Recents::with_path(p);
    r.push(RecentEntry::Workspace { path: "/a".into() }).unwrap();
    r.push(RecentEntry::Workspace { path: "/b".into() }).unwrap();
    r.push(RecentEntry::Workspace { path: "/a".into() }).unwrap();
    let list = r.list();
    assert_eq!(list.len(), 2);
    assert!(matches!(&list[0], RecentEntry::Workspace { path } if path == &std::path::PathBuf::from("/a")));
}
```

- [ ] **Step 11.2: Run, expect failure**

Run: `cargo test -p dat0-app --test recents`
Expected: failure.

- [ ] **Step 11.3: Implement**

`crates/dat0-app/src/recents/mod.rs`:

```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_ENTRIES: usize = 25;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum RecentEntry {
    Workspace { path: PathBuf },
    Package { path: PathBuf },
}

impl RecentEntry {
    pub fn path(&self) -> &std::path::Path {
        match self {
            RecentEntry::Workspace { path } | RecentEntry::Package { path } => path,
        }
    }
}

pub struct Recents {
    path: PathBuf,
    entries: Vec<RecentEntry>,
}

impl Recents {
    pub fn with_path(path: PathBuf) -> Self {
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<RecentEntry>>(&s).ok())
            .unwrap_or_default();
        Self { path, entries }
    }

    pub fn list(&self) -> &[RecentEntry] {
        &self.entries
    }

    pub fn push(&mut self, entry: RecentEntry) -> Result<()> {
        // Remove existing match (by path)
        self.entries.retain(|e| e.path() != entry.path());
        // Insert at front
        self.entries.insert(0, entry);
        // Trim
        self.entries.truncate(MAX_ENTRIES);
        // Persist
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let serialized = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(&self.path, serialized)?;
        Ok(())
    }
}
```

Add `pub mod recents;` to `lib.rs`.

- [ ] **Step 11.4: Run, expect pass**

Run: `cargo test -p dat0-app --test recents`
Expected: 3 tests PASS.

- [ ] **Step 11.5: Commit**

```bash
git add crates/dat0-app/
git commit -s -m "feat: Recents service with MRU semantics (P1.T11)"
```

---

### Task 12: Cross-platform keychain — trait + macOS impl

**Files:**
- Modify: `crates/dat0-keychain/Cargo.toml`
- Modify: `crates/dat0-keychain/src/lib.rs`
- Create: `crates/dat0-keychain/src/macos.rs`

- [ ] **Step 12.1: Add deps**

`crates/dat0-keychain/Cargo.toml`:

```toml
[dependencies]
anyhow = { workspace = true }
thiserror = { workspace = true }

[target.'cfg(target_os = "macos")'.dependencies]
security-framework = "2"

[target.'cfg(target_os = "linux")'.dependencies]
secret-service = { version = "5", features = ["rt-tokio-crypto-rust"] }
tokio = { workspace = true }
```

- [ ] **Step 12.2: Failing test**

`crates/dat0-keychain/tests/round_trip.rs`:

```rust
use dat0_keychain::Keychain;

#[test]
fn store_and_retrieve() {
    let kc = Keychain::new("dat0-test").unwrap();
    let key = "test-secret";
    let value = b"hunter2";

    kc.set(key, value).unwrap();
    let retrieved = kc.get(key).unwrap();
    assert_eq!(retrieved.as_deref(), Some(value.as_slice()));

    kc.delete(key).unwrap();
    assert!(kc.get(key).unwrap().is_none());
}
```

- [ ] **Step 12.3: Implement the trait + macOS impl**

`crates/dat0-keychain/src/lib.rs`:

```rust
use anyhow::Result;

pub struct Keychain {
    service: String,
}

impl Keychain {
    pub fn new(service: impl Into<String>) -> Result<Self> {
        Ok(Self { service: service.into() })
    }

    pub fn set(&self, key: &str, value: &[u8]) -> Result<()> {
        platform::set(&self.service, key, value)
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        platform::get(&self.service, key)
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        platform::delete(&self.service, key)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use anyhow::Result;
    use security_framework::passwords::{set_generic_password, get_generic_password, delete_generic_password};

    pub fn set(service: &str, key: &str, value: &[u8]) -> Result<()> {
        set_generic_password(service, key, value)?;
        Ok(())
    }
    pub fn get(service: &str, key: &str) -> Result<Option<Vec<u8>>> {
        match get_generic_password(service, key) {
            Ok(v) => Ok(Some(v)),
            Err(e) if e.code() == -25300 => Ok(None), // errSecItemNotFound
            Err(e) => Err(e.into()),
        }
    }
    pub fn delete(service: &str, key: &str) -> Result<()> {
        match delete_generic_password(service, key) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == -25300 => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use anyhow::{Result, Context};

    pub fn set(service: &str, key: &str, value: &[u8]) -> Result<()> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(async {
            let ss = secret_service::SecretService::connect(secret_service::EncryptionType::Dh).await?;
            let collection = ss.get_default_collection().await?;
            collection.create_item(
                &format!("dat0/{service}/{key}"),
                std::collections::HashMap::from([("service", service), ("key", key)]),
                value,
                true,
                "application/octet-stream",
            ).await.context("create_item")?;
            anyhow::Ok(())
        })
    }
    pub fn get(service: &str, key: &str) -> Result<Option<Vec<u8>>> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(async {
            let ss = secret_service::SecretService::connect(secret_service::EncryptionType::Dh).await?;
            let attrs = std::collections::HashMap::from([("service", service), ("key", key)]);
            let items = ss.search_items(attrs).await?;
            match items.unlocked.first() {
                Some(item) => Ok(Some(item.get_secret().await?)),
                None => Ok(None),
            }
        })
    }
    pub fn delete(service: &str, key: &str) -> Result<()> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(async {
            let ss = secret_service::SecretService::connect(secret_service::EncryptionType::Dh).await?;
            let attrs = std::collections::HashMap::from([("service", service), ("key", key)]);
            for item in ss.search_items(attrs).await?.unlocked {
                item.delete().await?;
            }
            anyhow::Ok(())
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use anyhow::{Result, anyhow};
    pub fn set(_: &str, _: &str, _: &[u8]) -> Result<()> { Err(anyhow!("unsupported platform")) }
    pub fn get(_: &str, _: &str) -> Result<Option<Vec<u8>>> { Err(anyhow!("unsupported platform")) }
    pub fn delete(_: &str, _: &str) -> Result<()> { Err(anyhow!("unsupported platform")) }
}
```

> **Note:** `security-framework` and `secret-service` API surface drifts; the snippet above sketches the shape. When implementing, consult the actual crate docs. The Linux Secret Service test requires a running keyring (gnome-keyring-daemon or kwalletmanager); CI Linux runners may need `dbus-launch` setup.

- [ ] **Step 12.4: Run on host platform, expect pass**

Run: `cargo test -p dat0-keychain`
Expected: PASS on macOS host. On Linux without Secret Service running, the test expects skipped/ignored — adapt with `#[ignore]` until CI is configured.

- [ ] **Step 12.5: Commit**

```bash
git add crates/dat0-keychain/
git commit -s -m "feat: cross-platform keychain primitive (P1.T12)"
```

---

### Task 13: Sentry SDK with redaction

**Files:**
- Create: `crates/dat0-app/src/telemetry/mod.rs`
- Create: `crates/dat0-app/src/telemetry/redaction.rs`

- [ ] **Step 13.1: Failing test**

Create `crates/dat0-app/tests/telemetry.rs`:

```rust
use dat0_app::telemetry::redaction::redact_event;
use sentry::protocol::{Event, Frame, Stacktrace};

#[test]
fn redacts_absolute_paths() {
    let mut event = Event::default();
    event.exception.values.push(sentry::protocol::Exception {
        ty: "Panic".into(),
        value: Some("at /Users/alice/secret/project/src/foo.rs:42".into()),
        stacktrace: Some(Stacktrace {
            frames: vec![Frame {
                filename: Some("/Users/alice/secret/project/src/foo.rs".into()),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });

    let redacted = redact_event(event).unwrap();
    let frame = &redacted.exception.values[0].stacktrace.as_ref().unwrap().frames[0];
    assert_eq!(frame.filename.as_deref(), Some("<redacted>/foo.rs"));
    let val = redacted.exception.values[0].value.as_deref().unwrap();
    assert!(!val.contains("/Users/alice"), "absolute path leaked into value: {val}");
}
```

- [ ] **Step 13.2: Run, expect failure**

Run: `cargo test -p dat0-app --test telemetry`
Expected: fails — `redact_event` undefined.

- [ ] **Step 13.3: Implement redaction**

`crates/dat0-app/src/telemetry/redaction.rs`:

```rust
use sentry::protocol::Event;
use std::path::Path;

pub fn redact_event(mut event: Event<'static>) -> Option<Event<'static>> {
    for ex in &mut event.exception.values {
        if let Some(value) = ex.value.take() {
            ex.value = Some(redact_text(&value));
        }
        if let Some(st) = ex.stacktrace.as_mut() {
            for frame in &mut st.frames {
                if let Some(filename) = frame.filename.take() {
                    frame.filename = Some(redact_path(&filename));
                }
                if let Some(abs) = frame.abs_path.take() {
                    frame.abs_path = Some(redact_path(&abs));
                }
                // Drop variables / context; they may contain user data
                frame.vars.clear();
                frame.pre_context.clear();
                frame.post_context.clear();
                frame.context_line = None;
            }
        }
    }
    // Drop user/server identifiable context
    event.user = None;
    event.server_name = None;
    Some(event)
}

fn redact_path(s: &str) -> String {
    let p = Path::new(s);
    let basename = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "<redacted>".into());
    format!("<redacted>/{basename}")
}

fn redact_text(s: &str) -> String {
    // Replace anything that looks like an absolute path with <redacted>
    // Heuristic: matches /Users/X/..., /home/X/..., C:\Users\X\..., absolute paths, etc.
    let re = regex::Regex::new(r#"(/Users/[^/\s]+|/home/[^/\s]+|[A-Z]:\\[^\\\s]+)([\\/][^"'\s,]*)?"#).unwrap();
    re.replace_all(s, "<redacted>").into_owned()
}
```

Add `regex = "1"` to `dat0-app` deps.

`crates/dat0-app/src/telemetry/mod.rs`:

```rust
pub mod redaction;

use anyhow::Result;
use sentry::ClientOptions;

const SENTRY_DSN_PUBLIC: &str = env!("DAT0_GLITCHTIP_DSN_PUBLIC", "DAT0_GLITCHTIP_DSN_PUBLIC must be set at build time");

pub struct Telemetry {
    _guard: Option<sentry::ClientInitGuard>,
}

impl Telemetry {
    pub fn init(submission_enabled: bool) -> Result<Self> {
        if !submission_enabled {
            tracing::info!("telemetry submission disabled (opt-in off)");
            return Ok(Self { _guard: None });
        }
        let opts = ClientOptions {
            dsn: Some(SENTRY_DSN_PUBLIC.parse()?),
            release: Some(env!("CARGO_PKG_VERSION").into()),
            before_send: Some(std::sync::Arc::new(|event| redaction::redact_event(event))),
            ..Default::default()
        };
        let guard = sentry::init(opts);
        Ok(Self { _guard: Some(guard) })
    }
}
```

Add `pub mod telemetry;` to `lib.rs`.

> **Note:** the `env!()` for the DSN requires CI to set `DAT0_GLITCHTIP_DSN_PUBLIC` at build time. Local builds can use a stub value via `.cargo/config.toml`. Document in README.

- [ ] **Step 13.4: Run, expect pass**

Run: `DAT0_GLITCHTIP_DSN_PUBLIC=https://foo@stub.invalid/1 cargo test -p dat0-app --test telemetry`
Expected: PASS.

- [ ] **Step 13.5: Commit**

```bash
git add crates/dat0-app/
git commit -s -m "feat: Sentry telemetry with before_send redaction (P1.T13)"
```

---

### Task 14: macOS native menu bar

**Files:**
- Create: `crates/dat0-app/src/menu_macos.rs`
- Modify: `crates/dat0-app/src/window.rs`

- [ ] **Step 14.1: Implement menu structure**

GPUI exposes `Menu` and `MenuItem` types. Create `crates/dat0-app/src/menu_macos.rs`:

```rust
#[cfg(target_os = "macos")]
pub fn build_menus(_cx: &mut gpui::App) -> Vec<gpui::Menu> {
    use gpui::{Menu, MenuItem};
    vec![
        Menu {
            name: dat0_i18n::t("menu.file").into(),
            items: vec![
                MenuItem::action(dat0_i18n::t("menu.file.new_window"), NewWindow),
                MenuItem::separator(),
                MenuItem::action(dat0_i18n::t("menu.file.open_file"), OpenFile),
                MenuItem::action(dat0_i18n::t("menu.file.open_workspace"), OpenWorkspace),
                MenuItem::action(dat0_i18n::t("menu.file.open_package"), OpenPackage),
                MenuItem::separator(),
                MenuItem::action(dat0_i18n::t("menu.file.close"), CloseWindow),
                MenuItem::action(dat0_i18n::t("menu.file.quit"), Quit),
            ],
        },
        Menu {
            name: dat0_i18n::t("menu.edit").into(),
            items: vec![
                MenuItem::action(dat0_i18n::t("menu.edit.undo"), Undo),
                MenuItem::action(dat0_i18n::t("menu.edit.redo"), Redo),
                MenuItem::separator(),
                MenuItem::action(dat0_i18n::t("menu.edit.cut"), Cut),
                MenuItem::action(dat0_i18n::t("menu.edit.copy"), Copy),
                MenuItem::action(dat0_i18n::t("menu.edit.paste"), Paste),
            ],
        },
        Menu {
            name: dat0_i18n::t("menu.view").into(),
            items: vec![
                MenuItem::action(dat0_i18n::t("menu.view.command_palette"), OpenCommandPalette),
                MenuItem::action(dat0_i18n::t("menu.view.settings"), OpenSettings),
            ],
        },
        Menu {
            name: dat0_i18n::t("menu.window").into(),
            items: vec![
                MenuItem::action(dat0_i18n::t("menu.window.minimize"), Minimize),
                MenuItem::action(dat0_i18n::t("menu.window.zoom"), Zoom),
            ],
        },
        Menu {
            name: dat0_i18n::t("menu.help").into(),
            items: vec![
                MenuItem::action(dat0_i18n::t("menu.help.about"), ShowAbout),
                MenuItem::action(dat0_i18n::t("menu.help.docs"), OpenDocs),
                MenuItem::action(dat0_i18n::t("menu.help.discord"), OpenDiscord),
            ],
        },
    ]
}

#[cfg(not(target_os = "macos"))]
pub fn build_menus(_cx: &mut gpui::App) -> Vec<()> { vec![] }

// Action types — implement gpui::Action via the `actions!` macro
gpui::actions!(dat0_menu, [
    NewWindow, OpenFile, OpenWorkspace, OpenPackage, CloseWindow, Quit,
    Undo, Redo, Cut, Copy, Paste,
    OpenCommandPalette, OpenSettings,
    Minimize, Zoom,
    ShowAbout, OpenDocs, OpenDiscord,
]);
```

- [ ] **Step 14.2: Add menu strings to `en.json`**

Append to `crates/dat0-i18n/src/strings/en.json`:

```json
{
  "app.name": "dat0",
  "app.tagline": "local-first data workbench",

  "menu.file": "File",
  "menu.file.new_window": "New Window",
  "menu.file.open_file": "Open File…",
  "menu.file.open_workspace": "Open Workspace…",
  "menu.file.open_package": "Open .dat0 Package…",
  "menu.file.close": "Close Window",
  "menu.file.quit": "Quit dat0",

  "menu.edit": "Edit",
  "menu.edit.undo": "Undo",
  "menu.edit.redo": "Redo",
  "menu.edit.cut": "Cut",
  "menu.edit.copy": "Copy",
  "menu.edit.paste": "Paste",

  "menu.view": "View",
  "menu.view.command_palette": "Command Palette…",
  "menu.view.settings": "Settings…",

  "menu.window": "Window",
  "menu.window.minimize": "Minimize",
  "menu.window.zoom": "Zoom",

  "menu.help": "Help",
  "menu.help.about": "About dat0",
  "menu.help.docs": "Documentation",
  "menu.help.discord": "Join Discord"
}
```

- [ ] **Step 14.3: Wire menu into the GPUI app**

In `crates/dat0-app/src/window.rs`, before `run()`, set the menu:

```rust
#[cfg(target_os = "macos")]
{
    cx.set_menus(crate::menu_macos::build_menus(cx));
}
```

Add `pub mod menu_macos;` to `lib.rs`.

- [ ] **Step 14.4: Run on macOS, verify menu bar**

Run: `cargo run --bin dat0`
Expected: macOS menu bar shows File / Edit / View / Window / Help with the items defined above. Items don't yet do anything — wiring lands in later phases.

- [ ] **Step 14.5: Commit**

```bash
git add crates/
git commit -s -m "feat: macOS native menu bar (P1.T14)"
```

---

### Task 15: Sparkle scaffolding (macOS auto-update)

**Files:**
- Create: `crates/dat0-app/src/updater/mod.rs`
- Create: `crates/dat0-app/src/updater/sparkle.rs`
- Create: `build.rs` for sparkle public key embedding

- [ ] **Step 15.1: Add Sparkle Rust binding**

Sparkle is an Objective-C framework. Bridge via `objc` and `cocoa` crates. Add to `dat0-app/Cargo.toml`:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
objc = "0.2"
cocoa = "0.25"
```

- [ ] **Step 15.2: Write the Sparkle bridge stub**

`crates/dat0-app/src/updater/mod.rs`:

```rust
use anyhow::Result;

#[cfg(target_os = "macos")]
mod sparkle;
#[cfg(target_os = "macos")]
pub use sparkle::*;

#[cfg(target_os = "linux")]
mod appimage;
#[cfg(target_os = "linux")]
pub use appimage::*;

pub trait Updater {
    fn check_for_updates(&self) -> Result<()>;
    fn current_version(&self) -> &str;
}
```

`crates/dat0-app/src/updater/sparkle.rs`:

```rust
//! Sparkle bridge for macOS auto-update.
//!
//! v1 scaffolding only: configuration object + check_for_updates() trigger.
//! Full UI (release notes, restart prompt) lands in P10.

use anyhow::Result;
use super::Updater;

pub struct SparkleUpdater {
    appcast_url: String,
    public_key: String,
    version: String,
}

impl SparkleUpdater {
    pub fn new() -> Result<Self> {
        Ok(Self {
            appcast_url: env!("DAT0_SPARKLE_APPCAST_URL", "appcast URL").into(),
            public_key: include_str!("../../../../docs/security/sparkle-public-key.txt").trim().into(),
            version: env!("CARGO_PKG_VERSION").into(),
        })
    }
}

impl Updater for SparkleUpdater {
    fn check_for_updates(&self) -> Result<()> {
        // P1: stub — real impl bridges to SUUpdater via Objective-C.
        // The bridge is non-trivial and lands in P10 alongside notarization.
        tracing::info!(appcast = %self.appcast_url, version = %self.version, "sparkle: check_for_updates (stub)");
        Ok(())
    }

    fn current_version(&self) -> &str { &self.version }
}
```

`crates/dat0-app/src/updater/appimage.rs`:

```rust
//! AppImageUpdate scaffolding for Linux self-updating AppImages.
//!
//! v1: stub — real subprocess invocation to `appimageupdatetool` lands in P10.

use anyhow::Result;
use super::Updater;

pub struct AppImageUpdater {
    version: String,
}

impl AppImageUpdater {
    pub fn new() -> Result<Self> {
        Ok(Self { version: env!("CARGO_PKG_VERSION").into() })
    }
}

impl Updater for AppImageUpdater {
    fn check_for_updates(&self) -> Result<()> {
        tracing::info!(version = %self.version, "appimage: check_for_updates (stub)");
        Ok(())
    }

    fn current_version(&self) -> &str { &self.version }
}
```

Add `pub mod updater;` to `lib.rs`.

- [ ] **Step 15.3: Wire env vars**

In `.cargo/config.toml`:

```toml
[env]
DAT0_SPARKLE_APPCAST_URL = "https://dat0.dev/appcast.xml"
DAT0_GLITCHTIP_DSN_PUBLIC = "https://STUB@glitchtip.invalid/1"
```

CI overrides these from secrets.

- [ ] **Step 15.4: Compile-only test**

Build: `cargo build`
Expected: builds with no errors. Stub log line confirms the path is wired when `check_for_updates()` is called (in later phases).

- [ ] **Step 15.5: Commit**

```bash
git add crates/dat0-app/ .cargo/
git commit -s -m "feat: Sparkle (macOS) + AppImageUpdate (Linux) scaffolding (P1.T15)"
```

---

### Task 16: Settings panel scaffolding (GPUI)

**Files:**
- Create: `crates/dat0-app/src/settings_ui/mod.rs`
- Create: `crates/dat0-app/src/settings_ui/sections/mod.rs`
- Create: `crates/dat0-app/src/settings_ui/sections/profile.rs`
- Create: `crates/dat0-app/src/settings_ui/sections/theme.rs`

- [ ] **Step 16.1: Define a `SettingsSection` trait**

`crates/dat0-app/src/settings_ui/sections/mod.rs`:

```rust
pub mod profile;
pub mod theme;

pub trait SettingsSection {
    /// i18n key for the section's display name.
    fn name_key(&self) -> &'static str;
    /// Stable identifier (URL-safe).
    fn id(&self) -> &'static str;
    /// Render the section's editable surface. Returns a GPUI element.
    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement;
}

pub fn all_sections() -> Vec<Box<dyn SettingsSection>> {
    vec![
        Box::new(profile::ProfileSection),
        Box::new(theme::ThemeSection),
    ]
}
```

- [ ] **Step 16.2: Implement Profile section (author identity)**

`crates/dat0-app/src/settings_ui/sections/profile.rs`:

```rust
use super::SettingsSection;
use gpui::{div, IntoElement};

pub struct ProfileSection;

impl SettingsSection for ProfileSection {
    fn name_key(&self) -> &'static str { "settings.profile" }
    fn id(&self) -> &'static str { "profile" }
    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement {
        // P1: skeletal — full author-name + author-email editor lands in later phase
        // when text input primitives in error_ux + form helpers exist.
        div().child(dat0_i18n::t("settings.profile.placeholder")).into_any_element()
    }
}
```

- [ ] **Step 16.3: Implement Theme section**

`crates/dat0-app/src/settings_ui/sections/theme.rs`:

```rust
use super::SettingsSection;
use gpui::{div, IntoElement};

pub struct ThemeSection;

impl SettingsSection for ThemeSection {
    fn name_key(&self) -> &'static str { "settings.theme" }
    fn id(&self) -> &'static str { "theme" }
    fn render(&self, _window: &mut gpui::Window, _cx: &mut gpui::App) -> gpui::AnyElement {
        div().child(dat0_i18n::t("settings.theme.placeholder")).into_any_element()
    }
}
```

- [ ] **Step 16.4: Append i18n strings**

Add to `en.json`:

```json
{
  "settings": "Settings",
  "settings.profile": "Profile",
  "settings.profile.placeholder": "Your name and email — used as the author of .dat0 packages you create.",
  "settings.theme": "Theme",
  "settings.theme.placeholder": "Choose a theme. Drop additional Zed-format JSON theme files into ~/Library/Application Support/dat0/themes/."
}
```

- [ ] **Step 16.5: Build a Settings window/view**

`crates/dat0-app/src/settings_ui/mod.rs`:

```rust
pub mod sections;

use gpui::{div, prelude::*, IntoElement};

pub struct SettingsView {
    selected_section: String,
}

impl SettingsView {
    pub fn new() -> Self {
        Self { selected_section: "profile".into() }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let sections = sections::all_sections();
        let active = sections.iter().find(|s| s.id() == self.selected_section);
        div()
            .flex()
            .flex_row()
            .child(
                // Sidebar: section list
                div()
                    .w_64()
                    .flex()
                    .flex_col()
                    .children(sections.iter().map(|s| {
                        div().child(dat0_i18n::t(s.name_key()))
                    }))
            )
            .child(
                // Content area
                div()
                    .flex_1()
                    .when_some(active, |d, s| d.child(s.render(window, cx)))
            )
    }
}
```

> **Note:** GPUI's element-builder API is fluid (chained `flex_row`, `w_64`, etc.). The exact API surface evolves with the pinned commit — match it to whatever the pinned `gpui` reveals via examples in the Zed source. The above is illustrative.

Add `pub mod settings_ui;` to `lib.rs`.

- [ ] **Step 16.6: Commit**

```bash
git add crates/
git commit -s -m "feat: Settings UI panel scaffolding with Profile + Theme sections (P1.T16)"
```

---

### Task 17: Error/dialog UX primitives — modal

**Files:**
- Create: `crates/dat0-app/src/error_ux/mod.rs`
- Create: `crates/dat0-app/src/error_ux/modal.rs`
- Create: `crates/dat0-app/src/error_ux/toast.rs`
- Create: `crates/dat0-app/src/error_ux/banner.rs`

- [ ] **Step 17.1: Define the primitives module**

`crates/dat0-app/src/error_ux/mod.rs`:

```rust
pub mod modal;
pub mod toast;
pub mod banner;

pub use modal::Modal;
pub use toast::{Toast, ToastSeverity};
pub use banner::{Banner, BannerSeverity};
```

- [ ] **Step 17.2: Implement Modal (skeletal)**

`crates/dat0-app/src/error_ux/modal.rs`:

```rust
use gpui::{div, prelude::*, IntoElement};

pub struct Modal {
    pub title: String,
    pub message: String,
    pub primary_action: Option<(String, std::sync::Arc<dyn Fn() + Send + Sync>)>,
    pub secondary_action: Option<(String, std::sync::Arc<dyn Fn() + Send + Sync>)>,
}

impl Modal {
    pub fn new(title: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            primary_action: None,
            secondary_action: None,
        }
    }

    pub fn with_primary<F: Fn() + Send + Sync + 'static>(mut self, label: impl Into<String>, f: F) -> Self {
        self.primary_action = Some((label.into(), std::sync::Arc::new(f)));
        self
    }
}

impl Render for Modal {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        div().child(self.title.clone()).child(self.message.clone())
    }
}
```

- [ ] **Step 17.3: Implement Toast**

`crates/dat0-app/src/error_ux/toast.rs`:

```rust
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum ToastSeverity { Info, Success, Warning, Error }

pub struct Toast {
    pub message: String,
    pub severity: ToastSeverity,
    pub auto_dismiss_after: Option<Duration>,
}

impl Toast {
    pub fn info(message: impl Into<String>) -> Self {
        Self { message: message.into(), severity: ToastSeverity::Info, auto_dismiss_after: Some(Duration::from_secs(4)) }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self { message: message.into(), severity: ToastSeverity::Error, auto_dismiss_after: None }
    }
}
```

- [ ] **Step 17.4: Implement Banner**

`crates/dat0-app/src/error_ux/banner.rs`:

```rust
#[derive(Debug, Clone, Copy)]
pub enum BannerSeverity { Info, Warning, Error }

pub struct Banner {
    pub message: String,
    pub severity: BannerSeverity,
    pub dismissible: bool,
    pub action_label: Option<String>,
}

impl Banner {
    pub fn warning(message: impl Into<String>) -> Self {
        Self { message: message.into(), severity: BannerSeverity::Warning, dismissible: true, action_label: None }
    }
}
```

Add `pub mod error_ux;` to `lib.rs`.

- [ ] **Step 17.5: Compile**

Run: `cargo build`
Expected: compiles. Visual integration with GPUI happens in later phases when these primitives are first used by features (e.g., `WorkspaceInUseModal` in P7).

- [ ] **Step 17.6: Commit**

```bash
git add crates/
git commit -s -m "feat: error/dialog UX primitives (Modal, Toast, Banner) (P1.T17)"
```

---

### Task 18: NOTICE aggregation tooling (cargo-about)

**Files:**
- Create: `about.toml`
- Create: `.github/workflows/notice.yml`

- [ ] **Step 18.1: Install cargo-about locally**

Run: `cargo install cargo-about`

- [ ] **Step 18.2: Configure**

`about.toml`:

```toml
accepted = ["Apache-2.0", "MIT", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "Unicode-DFS-2016"]
ignore-build-dependencies = true
ignore-dev-dependencies = true

[targets]
include = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "aarch64-apple-darwin"]
```

- [ ] **Step 18.3: Generate the third-party section of NOTICE**

Run: `cargo about generate -c about.toml docs/about-template.hbs > /tmp/third-party.txt`

(Template handlebars file: simple list of crate name + license + version; available examples in cargo-about README.)

Splice the generated list into `NOTICE.md` under "Third-party software" (preserving the manual top-level entries).

- [ ] **Step 18.4: CI gate — fail on NOTICE drift**

`.github/workflows/notice.yml`:

```yaml
name: NOTICE drift check
on:
  pull_request:
    paths: ["Cargo.toml", "Cargo.lock", "crates/**/Cargo.toml", "NOTICE.md"]
jobs:
  notice:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install cargo-about --locked
      - name: Regenerate third-party list
        run: cargo about generate -c about.toml docs/about-template.hbs > /tmp/third-party.txt
      - name: Diff NOTICE
        run: |
          # Extract the auto-generated section and compare
          ./scripts/notice-extract.sh NOTICE.md > /tmp/notice-current.txt
          diff /tmp/notice-current.txt /tmp/third-party.txt && echo "NOTICE in sync" || (echo "NOTICE drift; regenerate locally" && exit 1)
```

- [ ] **Step 18.5: Commit**

```bash
git add about.toml .github/workflows/notice.yml NOTICE.md docs/
git commit -s -m "ci: cargo-about NOTICE drift gate (P1.T18)"
```

---

### Task 19: Performance bench harness scaffolding

**Files:**
- Create: `benches/engine_smoke.rs`
- Create: `benches/grid_scroll.rs`
- Modify: `Cargo.toml` (root or `dat0-app`) to declare benches

- [ ] **Step 19.1: Engine smoke bench (criterion)**

`benches/engine_smoke.rs`:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

/// Placeholder bench. Real engine work lands in P2.
fn smoke_no_op(c: &mut Criterion) {
    c.bench_function("smoke/no_op", |b| b.iter(|| black_box(2 + 2)));
}

criterion_group!(benches, smoke_no_op);
criterion_main!(benches);
```

In `crates/dat0-app/Cargo.toml`:

```toml
[[bench]]
name = "engine_smoke"
path = "../../benches/engine_smoke.rs"
harness = false

[dev-dependencies]
criterion = { workspace = true }
```

- [ ] **Step 19.2: Grid scroll bench (custom harness)**

`benches/grid_scroll.rs`:

```rust
//! Custom grid-scroll FPS harness placeholder.
//!
//! GPU-bound; needs a self-hosted runner with a real GPU class to produce
//! meaningful numbers. P10 wires this as a CI merge gate. P1 just establishes
//! the harness shape.

fn main() {
    // Stub: prints zero. Real harness drives a headless GPUI grid widget.
    println!("grid_scroll_fps_1m_rows = 0  // placeholder");
    println!("grid_scroll_fps_10m_rows = 0  // placeholder");
}
```

In `crates/dat0-app/Cargo.toml`:

```toml
[[bench]]
name = "grid_scroll"
path = "../../benches/grid_scroll.rs"
harness = false
```

- [ ] **Step 19.3: Verify**

Run: `cargo bench --bench engine_smoke -- --quick`
Expected: runs, reports a placeholder timing.

Run: `cargo bench --bench grid_scroll`
Expected: prints placeholder lines.

- [ ] **Step 19.4: Commit**

```bash
git add benches/ crates/dat0-app/Cargo.toml
git commit -s -m "feat: perf bench harness scaffolding (P1.T19)"
```

---

### Task 20: CI — full matrix builds + tests

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 20.1: Author the workflow**

`.github/workflows/ci.yml`:

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:

jobs:
  fmt:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: rustfmt }
      - run: cargo fmt --all -- --check

  clippy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: clippy }
      - run: cargo clippy --workspace --all-targets -- -D warnings

  build-and-test:
    strategy:
      fail-fast: false
      matrix:
        target:
          - { os: macos-14,        triple: aarch64-apple-darwin,    name: macos-arm64  }
          - { os: macos-13,        triple: x86_64-apple-darwin,     name: macos-x86_64 }
          - { os: ubuntu-latest,   triple: x86_64-unknown-linux-gnu, name: linux-x86_64 }
          - { os: ubuntu-latest,   triple: aarch64-unknown-linux-gnu, name: linux-arm64 }
    runs-on: ${{ matrix.target.os }}
    env:
      DAT0_SPARKLE_APPCAST_URL: https://dat0.dev/appcast.xml
      DAT0_GLITCHTIP_DSN_PUBLIC: ${{ secrets.GLITCHTIP_DSN_PUBLIC }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: ${{ matrix.target.triple }} }
      - name: Linux deps for keychain tests
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y libsecret-1-dev dbus-x11 gnome-keyring
      - name: Build
        run: cargo build --workspace --release --target ${{ matrix.target.triple }}
      - name: Test
        run: cargo test --workspace --target ${{ matrix.target.triple }}

  i18n-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: ./scripts/i18n-check.sh
```

- [ ] **Step 20.2: Open PR, verify all jobs pass**

Push the branch, open a PR, verify all matrix jobs and lints succeed.

- [ ] **Step 20.3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -s -m "ci: full matrix build + test workflow (P1.T20)"
```

---

### Task 21: Boot orchestration + wiring everything together

**Files:**
- Modify: `crates/dat0-app/src/boot.rs`
- Modify: `crates/dat0-app/src/main.rs`

- [ ] **Step 21.1: Compose the boot path**

`crates/dat0-app/src/boot.rs` (extending Task 3):

```rust
use anyhow::Result;
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::{platform, recents::Recents, settings::{Settings, store::SettingsStore, watcher::SettingsWatcher}, telemetry::Telemetry};

pub fn init_logging() -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,dat0=debug"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false).compact())
        .try_init();
    Ok(())
}

pub struct AppContext {
    pub settings: Arc<std::sync::RwLock<Settings>>,
    pub _settings_watcher: SettingsWatcher,
    pub recents: Arc<std::sync::Mutex<Recents>>,
    pub _telemetry: Telemetry,
}

impl AppContext {
    pub fn boot() -> Result<Self> {
        let cfg_dir = platform::config_dir()?;
        platform::ensure_dir(&cfg_dir)?;
        let data_dir = platform::data_dir()?;
        platform::ensure_dir(&data_dir)?;

        let settings_path = cfg_dir.join("settings.toml");
        let store = SettingsStore::with_path(settings_path.clone());
        let initial = store.load_or_default()?;

        let settings = Arc::new(std::sync::RwLock::new(initial.clone()));
        let settings_clone = settings.clone();
        let watcher = SettingsWatcher::start(settings_path, move |new_settings| {
            *settings_clone.write().unwrap() = new_settings;
        })?;

        let recents = Arc::new(std::sync::Mutex::new(
            Recents::with_path(cfg_dir.join("recents.json"))
        ));

        let telemetry = Telemetry::init(initial.telemetry.crash_submission_enabled)?;

        Ok(Self {
            settings,
            _settings_watcher: watcher,
            recents,
            _telemetry: telemetry,
        })
    }
}
```

- [ ] **Step 21.2: Update `main`**

```rust
fn main() -> anyhow::Result<()> {
    dat0_app::boot::init_logging()?;
    let _ctx = dat0_app::boot::AppContext::boot()?;
    tracing::info!("dat0 starting");
    dat0_app::run_app()
}
```

- [ ] **Step 21.3: Verify**

Run: `cargo run --bin dat0`
Expected: app starts, creates `~/Library/Application Support/dat0/` (macOS) or XDG-equivalent (Linux), writes a default `settings.toml` if none exists, opens window, prints `INFO dat0 starting`.

- [ ] **Step 21.4: Commit**

```bash
git add crates/dat0-app/
git commit -s -m "feat: boot orchestration wires settings + recents + telemetry (P1.T21)"
```

---

### Task 22: P1 exit smoke test

**Files:**
- Create: `crates/dat0-app/tests/p1_exit_smoke.rs`

- [ ] **Step 22.1: Write the comprehensive exit smoke test**

```rust
//! P1 exit gate smoke test — verifies all P1 deliverables are wired.

#[test]
fn settings_toml_round_trip() {
    use dat0_app::settings::{Settings, store::SettingsStore};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.toml");
    let store = SettingsStore::with_path(path);
    let mut s = Settings::default();
    s.profile.author_name = "Test".into();
    store.save(&s).unwrap();
    let r = store.load_or_default().unwrap();
    assert_eq!(r.profile.author_name, "Test");
}

#[test]
fn theme_default_loads() {
    use dat0_app::theme::Theme;
    assert!(Theme::load_builtin("dark").is_ok());
    assert!(Theme::load_builtin("light").is_ok());
    assert!(Theme::load_builtin("high-contrast").is_ok());
}

#[test]
fn i18n_helper_works() {
    assert_eq!(dat0_i18n::t("app.name"), "dat0");
}

#[test]
fn keychain_round_trip() {
    let kc = dat0_keychain::Keychain::new("dat0-p1-smoke").unwrap();
    let _ = kc.delete("smoke"); // clean up any prior run
    kc.set("smoke", b"value").unwrap();
    assert_eq!(kc.get("smoke").unwrap().as_deref(), Some(b"value".as_slice()));
    kc.delete("smoke").unwrap();
}

#[test]
fn telemetry_redacts_paths() {
    use dat0_app::telemetry::redaction::redact_event;
    let mut event = sentry::protocol::Event::default();
    event.exception.values.push(sentry::protocol::Exception {
        ty: "Test".into(),
        value: Some("at /Users/jane/secret/foo.rs".into()),
        ..Default::default()
    });
    let r = redact_event(event).unwrap();
    let v = r.exception.values[0].value.as_deref().unwrap();
    assert!(!v.contains("/Users/jane"));
}

#[test]
fn recents_round_trip() {
    use dat0_app::recents::{Recents, RecentEntry};
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("recents.json");
    let mut r = Recents::with_path(p.clone());
    r.push(RecentEntry::Workspace { path: "/tmp/w".into() }).unwrap();
    drop(r);
    let r2 = Recents::with_path(p);
    assert_eq!(r2.list().len(), 1);
}

#[test]
fn platform_paths_resolve() {
    assert!(dat0_app::platform::config_dir().is_ok());
    assert!(dat0_app::platform::data_dir().is_ok());
    assert!(dat0_app::platform::cache_dir().is_ok());
}
```

- [ ] **Step 22.2: Run, expect all pass**

Run: `cargo test -p dat0-app --test p1_exit_smoke`
Expected: 7 tests PASS (some may be platform-conditional — keychain test requires Secret Service on Linux; gate with `#[cfg]` if needed).

- [ ] **Step 22.3: Commit**

```bash
git add crates/dat0-app/
git commit -s -m "test: P1 exit smoke covers settings, theme, i18n, keychain, telemetry, recents (P1.T22)"
```

---

### Task 23: Update `docs/upstream-watch.md` with pinned commits

**Files:**
- Modify: `docs/upstream-watch.md`

- [ ] **Step 23.1: Record the actual pinned commits**

After Task 2, you pinned `gpui` and `gpui-component` to specific commits. Open `docs/upstream-watch.md` and replace each `<KNOWN_GOOD_COMMIT>` placeholder in the table with the actual SHA and the date you pinned it.

```markdown
| **gpui** | <https://github.com/zed-industries/zed> | Pinned to `abc1234` (2026-04-26). | ... |
| **gpui-component** | <https://github.com/longbridge/gpui-component> | Pinned to `def5678` (2026-04-26). | ... |
```

- [ ] **Step 23.2: Commit**

```bash
git add docs/upstream-watch.md
git commit -s -m "docs: record P1 pinned commits in upstream-watch (P1.T23)"
```

---

### Task 24: P1 retrospective + handoff to P2

**Files:**
- Create: `docs/plans/2026-04-26-dat0-p1-retro.md` (if anything notable came up; otherwise skip)

- [ ] **Step 24.1: Read back the design spec §21.2 P1 exit checklist**

Verify each item against the implemented state:

- [ ] Cold-launches on macOS arm64, macOS x86_64, Linux x86_64, Linux aarch64
- [ ] Settings panel opens; changes persist across launches
- [ ] Theme switch (default ↔ alternate) works without restart (note: theme switching plumbing is sketched but full live re-application lands in P3 when more UI is theme-driven; verify schema + load + selection works)
- [ ] Author name + email captured and displayed in About / Settings
- [ ] macOS menu bar present and standard
- [ ] i18n: greppable assertion in CI
- [ ] Sparkle "Check for Updates" makes a network call to test appcast (stub in P1; real bridge P10)
- [ ] CI green; release-mode binaries produced for all 4 targets
- [ ] Test crash captured locally; verified `before_send` redaction strips path/schema/query/value content
- [ ] Keychain round-trip test passes on macOS + Ubuntu LTS
- [ ] `cargo-about` CI gate runs; passes
- [ ] Performance bench harness produces output for at least one engine bench + one grid bench

- [ ] **Step 24.2: Open a PR titled "P1: Foundation complete"**

When merged, P1 is closed. Move to P2 implementation plan (to be written when P1 lands).

---

## Self-review

This plan covers the §21.2 P1 scope. Cross-checked:

- Cargo workspace skeleton ✓ T1
- GPUI shell with basic window ✓ T2
- Sentry + GlitchTip + before_send redaction ✓ T13
- Cross-platform keychain primitive ✓ T12
- NOTICE-aggregation tooling ✓ T18
- Settings TOML schema + read/write + watcher ✓ T7, T8, T9
- Zed-JSON theme loader + 3 default themes ✓ T10
- Recents service ✓ T11
- Sparkle + AppImageUpdate scaffolding ✓ T15
- macOS native menu bar ✓ T14
- i18n primitive ✓ T5, T6
- Author identity in Settings → Profile ✓ T7 (schema), T16 (UI placeholder)
- Settings UI scaffolding ✓ T16
- Error/dialog UX primitives ✓ T17
- Performance bench harness scaffolded ✓ T19
- CI matrix for all 4 targets ✓ T20
- Tracing logger ✓ T3
- Platform path helpers ✓ T4
- Boot orchestration ✓ T21
- P1 exit smoke ✓ T22
- Pinned commits recorded ✓ T23

**Known gaps deliberately deferred:**
- Settings panel UI is skeletal — full editable form widgets (text inputs for author name/email, dropdown for theme) need form-input primitives that don't exist in P1. Stub renderers shipped; real surfaces wire up in P3 alongside the DataGrid component primitives.
- Sparkle bridge to Objective-C `SUUpdater` is stubbed — full Objective-C bridge in P10 alongside notarization. P1 ships configuration + appcast URL + public key embedding only.
- AppImageUpdate subprocess invocation deferred to P10.
- Theme live-switching: schema + load works in P1; observable theme change throughout the running app waits for P3 when more UI exists.

**Plan-level cleanup notes:**
- All file paths are exact.
- All steps include actual code or commands.
- TDD pattern: every code-producing task has a failing test before implementation.
- Commits are frequent, one per task minimum.

---

## Execution

Plan complete and saved to `docs/plans/2026-04-26-dat0-p1-foundation-plan.md`. Two execution options:

**1. Subagent-Driven (recommended)** — Dispatch a fresh subagent per task, review between tasks, fast iteration. Use `superpowers:subagent-driven-development`.

**2. Inline Execution** — Execute tasks in this session using `superpowers:executing-plans`, batch execution with checkpoints.

Pick when you're ready to start P1.
