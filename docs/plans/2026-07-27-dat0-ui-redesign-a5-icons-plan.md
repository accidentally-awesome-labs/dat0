# A5 Icon System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace dat0's Unicode-glyph icon-buttons and directional affordances with real Lucide SVG icons, served through a `Dat0Assets` `AssetSource` that layers 5 vendored icons over the 86 `gpui-component-assets` already bundles.

**Architecture:** One `AssetSource` impl checks dat0's own rust-embed folder first, then delegates to `gpui_component_assets::Assets`. A `Dat0IconName` enum implements upstream's `IconNamed` trait, making `Icon::new(Dat0IconName::Filter)` a drop-in for `Icon::new(IconName::Close)`. Call sites swap `.child("✕")` for `.child(Icon::new(IconName::Close))` plus an `a11y_label`. Icons inherit ambient text color and size, so no token plumbing and no new color literals are needed.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component (git rev `0f0ab35`), gpui-component-assets 0.5.1, rust-embed 8.7.2, `dat0_i18n::t()`.

**Design doc:** `docs/plans/2026-07-27-dat0-ui-redesign-a5-icons-design.md`
**Branch:** `feat/ui-redesign-a5-icons` off main `5b63d3e`

## Global Constraints

- **Dependency pinning:** `gpui-component-assets` MUST use the same git rev as `gpui-component`: `rev = "0f0ab35233212f8f3277028995caf0c41e13ee6c"`. A version-mismatched assets crate silently serves a different icon set.
- **`rust-embed` version:** `8.7.2` (matches what `gpui-component-assets` already resolves; avoids a second copy in the tree).
- **No new color literals.** A4's `tests/style_lint.rs` is a shrink-only ratchet that fails both over AND under its per-file counts. Icons inherit `window.text_style().color`, so no task should add or remove a color literal. If a per-file count changes, the change is wrong — do not edit the allowlist to compensate.
- **No new focus stops, no changed `focus_stop` ids.** `keyboard_nav` cycle counts must be identical before and after this slice.
- **Scope rule:** a glyph converts **iff it is its own element** (`.child("✕")`). Glyphs inside `format!`-produced `String`s stay text. The one deliberate exception is `pipeline_bar.rs:222` `"▣ base"`.
- **i18n:** all user-facing strings go through `dat0_i18n::t("key")`; keys are added to `crates/dat0-i18n/src/strings/en.json` in the same commit. Never construct a key with `format!`.
- **Vendored SVGs are verbatim upstream copies.** Do not reformat, minify, or re-indent them — unmodified copies keep the attribution honest.
- **Every commit:** `git commit -s` (DCO gate).
- **Per-task local gate:** `cargo fmt --all -- --check` and `cargo clippy -p dat0-app --all-targets -- -D warnings`. The full `cargo test -p dat0-app` sweep is the CONTROLLER's job, not the implementer's — do not background it.

## File Structure

| File | Responsibility |
|---|---|
| `crates/dat0-app/Cargo.toml` | Add `gpui-component-assets`, `rust-embed` deps |
| `Cargo.toml` (workspace) | Pin `gpui-component-assets` to the shared rev |
| `crates/dat0-app/assets/icons/*.svg` | 5 vendored Lucide SVGs (verbatim) |
| `crates/dat0-app/assets/icons/LICENSE-lucide` | Verbatim Lucide LICENSE (ISC + Feather/MIT subset) |
| `crates/dat0-app/src/assets.rs` | `Dat0Embedded`, `Dat0Assets: AssetSource`, `Dat0IconName: IconNamed` |
| `crates/dat0-app/src/lib.rs` | `pub mod assets;` |
| `crates/dat0-app/tests/icon_assets.rs` | Resolution, fallback, disjoint-set, payload-shape gates |
| `crates/dat0-app/src/window.rs:1741` | `.with_assets(Dat0Assets)` on the prod `Application` |
| `crates/dat0-app/examples/gallery.rs:22` | `.with_assets(Dat0Assets)` on the gallery `Application` |
| `crates/dat0-i18n/src/strings/en.json` | New icon-button label keys |
| 10 call-site files | Glyph → `Icon` + `a11y_label` |
| `crates/dat0-app/src/gallery.rs` | Sixth section: `icons_section` |
| `crates/dat0-app/tests/gallery_smoke.rs` | `"gallery.icons"` seam |
| `NOTICE.md` | Hand-written `## Bundled assets` section |

---

## Task 1: `Dat0Assets` + `Dat0IconName` (T0 GATE)

This is the load-bearing task. Everything downstream assumes asset resolution works; if the fallback chain is wrong, every icon renders blank and silent (A0 spike: a missing asset is a no-render, never a panic). Nothing else starts until this task's tests are green.

**Files:**
- Modify: `Cargo.toml` (workspace `[workspace.dependencies]`)
- Modify: `crates/dat0-app/Cargo.toml`
- Create: `crates/dat0-app/assets/icons/{funnel,play,layers,bookmark,clock}.svg`
- Create: `crates/dat0-app/assets/icons/LICENSE-lucide`
- Create: `crates/dat0-app/src/assets.rs`
- Modify: `crates/dat0-app/src/lib.rs`
- Test: `crates/dat0-app/tests/icon_assets.rs`

**Interfaces:**
- Consumes: nothing (first task)
- Produces:
  - `dat0_app::assets::Dat0Assets` — unit struct implementing `gpui::AssetSource`
  - `dat0_app::assets::Dat0IconName` — `enum { Filter, Play, Layers, Bookmark, History }`, implements `gpui_component::IconNamed`, `#[derive(Clone, Copy, Debug, PartialEq, Eq)]`
  - `dat0_app::assets::Dat0IconName::ALL` — `&'static [Dat0IconName; 5]`, used by the tests and the gallery
  - `dat0_app::assets::BUNDLED_USED` — `&'static [&'static str; 5]`, the upstream icon paths dat0 references, used by tests and gallery

- [ ] **Step 1: Vendor the five SVGs**

These are verbatim copies from `https://raw.githubusercontent.com/lucide-icons/lucide/main/icons/<name>.svg`, fetched 2026-07-27. Do not reformat.

`crates/dat0-app/assets/icons/funnel.svg`:
```svg
<svg
  xmlns="http://www.w3.org/2000/svg"
  width="24"
  height="24"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d="M10 20a1 1 0 0 0 .553.895l2 1A1 1 0 0 0 14 21v-7a2 2 0 0 1 .517-1.341L21.74 4.67A1 1 0 0 0 21 3H3a1 1 0 0 0-.742 1.67l7.225 7.989A2 2 0 0 1 10 14z" />
</svg>
```

`crates/dat0-app/assets/icons/play.svg`:
```svg
<svg
  xmlns="http://www.w3.org/2000/svg"
  width="24"
  height="24"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z" />
</svg>
```

`crates/dat0-app/assets/icons/layers.svg`:
```svg
<svg
  xmlns="http://www.w3.org/2000/svg"
  width="24"
  height="24"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z" />
  <path d="M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12" />
  <path d="M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17" />
</svg>
```

`crates/dat0-app/assets/icons/bookmark.svg`:
```svg
<svg
  xmlns="http://www.w3.org/2000/svg"
  width="24"
  height="24"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d="M17 3a2 2 0 0 1 2 2v15a1 1 0 0 1-1.496.868l-4.512-2.578a2 2 0 0 0-1.984 0l-4.512 2.578A1 1 0 0 1 5 20V5a2 2 0 0 1 2-2z" />
</svg>
```

`crates/dat0-app/assets/icons/clock.svg`:
```svg
<svg
  xmlns="http://www.w3.org/2000/svg"
  width="24"
  height="24"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="2"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <circle cx="12" cy="12" r="10" />
  <path d="M12 6v6l4 2" />
</svg>
```

- [ ] **Step 2: Vendor the Lucide license**

Fetch verbatim and save to `crates/dat0-app/assets/icons/LICENSE-lucide`:

```bash
curl -fsSL https://raw.githubusercontent.com/lucide-icons/lucide/main/LICENSE \
  -o crates/dat0-app/assets/icons/LICENSE-lucide
```

Verify it contains **both** licenses — Lucide is dual-licensed. The file must contain the string `ISC License` AND `The MIT License (MIT) (for the icons listed above)`. The MIT half covers icons derived from Feather, and dat0 ships several of them (`clock`, `x`, `check`, `chevron-*`).

```bash
grep -c 'ISC License' crates/dat0-app/assets/icons/LICENSE-lucide   # expect 1
grep -c 'Cole Bemis' crates/dat0-app/assets/icons/LICENSE-lucide    # expect 1
```

- [ ] **Step 3: Add dependencies**

In the workspace `Cargo.toml`, directly beneath the existing `gpui-component` line (currently line 65):

```toml
# A5: the default Lucide icon bundle for gpui-component's `IconName`. MUST stay
# on the same rev as gpui-component — a mismatched assets crate silently serves
# a different icon set (blank or wrong glyphs, no build error).
gpui-component-assets = { git = "https://github.com/longbridge/gpui-component", rev = "0f0ab35233212f8f3277028995caf0c41e13ee6c" }
```

In `crates/dat0-app/Cargo.toml`, in `[dependencies]` beneath `gpui-component`:

```toml
gpui-component-assets = { workspace = true }
# A5: embeds dat0's own icons/*.svg into the binary. Same major as the copy
# gpui-component-assets already resolves, so no second version lands in the tree.
rust-embed = { version = "8.7.2", features = ["interpolate-folder-path"] }
```

- [ ] **Step 4: Write the failing test**

Create `crates/dat0-app/tests/icon_assets.rs`:

```rust
//! Gates for the A5 icon asset chain.
//!
//! A5's central failure mode is SILENT: a missing or misnamed asset renders as
//! nothing at all — gpui does not panic on an unresolved `AssetSource` path (A0
//! spike). Without these tests a typo in `Dat0IconName::path()` ships a blank
//! button that only a human boot would catch.

use gpui::AssetSource;

use dat0_app::assets::{BUNDLED_USED, Dat0Assets, Dat0IconName};
use gpui_component::IconNamed as _;

/// Every dat0-owned icon resolves to a non-empty payload.
#[test]
fn dat0_icons_resolve() {
    for name in Dat0IconName::ALL {
        let path = name.path();
        let bytes = Dat0Assets
            .load(&path)
            .unwrap_or_else(|e| panic!("{path} failed to load: {e}"))
            .unwrap_or_else(|| panic!("{path} resolved to None"));
        assert!(!bytes.is_empty(), "{path} resolved to an empty payload");
    }
}

/// Every upstream icon dat0 references resolves through the fallback arm.
#[test]
fn bundled_icons_resolve_through_fallback() {
    for path in BUNDLED_USED {
        let bytes = Dat0Assets
            .load(path)
            .unwrap_or_else(|e| panic!("{path} failed to load: {e}"))
            .unwrap_or_else(|| panic!("{path} resolved to None"));
        assert!(!bytes.is_empty(), "{path} resolved to an empty payload");
    }
}

/// Own-first ordering means a dat0 filename that also exists upstream would
/// silently shadow it. We vendor only names absent upstream TODAY; this test
/// turns a future gpui-component rev that adds one into a build failure
/// instead of a silent divergence from everyone else's icon.
#[test]
fn dat0_icons_do_not_shadow_bundled() {
    let upstream: Vec<String> = gpui_component_assets::Assets
        .list("icons/")
        .expect("upstream list() failed")
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    for name in Dat0IconName::ALL {
        let path = name.path().to_string();
        assert!(
            !upstream.contains(&path),
            "{path} now also exists in gpui-component-assets — dat0's copy is \
             shadowing it. Delete the vendored file and point Dat0IconName at \
             the upstream IconName variant instead."
        );
    }
}

/// A truncated or mis-vendored file resolves fine but renders as nothing.
#[test]
fn payloads_are_svg() {
    for name in Dat0IconName::ALL {
        let path = name.path();
        let bytes = Dat0Assets.load(&path).unwrap().unwrap();
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(64)]);
        assert!(
            head.trim_start().starts_with("<svg"),
            "{path} does not begin with <svg: {head:?}"
        );
    }
}

/// An unresolved path must not panic — the production consequence of a typo is
/// a blank icon, and that is what the rest of this file exists to prevent.
#[test]
fn missing_path_is_not_a_panic() {
    let _ = Dat0Assets.load("icons/definitely-not-an-icon.svg");
}

/// The empty path is the documented "no asset" sentinel and must be Ok(None).
#[test]
fn empty_path_is_none() {
    assert!(Dat0Assets.load("").unwrap().is_none());
}
```

- [ ] **Step 5: Run the test to verify it fails**

Run: `cargo test -p dat0-app --test icon_assets`
Expected: FAIL to compile — `unresolved import dat0_app::assets`.

- [ ] **Step 6: Write the implementation**

Create `crates/dat0-app/src/assets.rs`:

```rust
//! Icon assets for dat0.
//!
//! `gpui::Application::with_assets` takes exactly ONE `AssetSource`, so
//! [`Dat0Assets`] serves both dat0's own icons and the 86 Lucide SVGs that
//! `gpui-component-assets` bundles for `gpui_component::IconName`.
//!
//! Lookup is own-first, then delegate. That ordering is what makes
//! `dat0_icons_do_not_shadow_bundled` (tests/icon_assets.rs) load-bearing: if a
//! future gpui-component rev ships one of our filenames, our copy wins silently
//! and dat0's icon diverges from every other consumer's.
//!
//! Missing assets stay a silent no-render rather than a panic — that is gpui's
//! existing behaviour (A0 spike), and `load` deliberately delegates upstream's
//! not-found `Err` rather than flattening it to `Ok(None)`.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};
use gpui_component::IconNamed;
use rust_embed::RustEmbed;

/// dat0's own icon files. Five Lucide SVGs that `gpui-component-assets` does
/// not bundle at the pinned rev.
#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
struct Dat0Embedded;

/// The single `AssetSource` registered on every `Application`.
pub struct Dat0Assets;

impl AssetSource for Dat0Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(f) = Dat0Embedded::get(path) {
            return Ok(Some(f.data));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut out: Vec<SharedString> = Dat0Embedded::iter()
            .filter(|p| p.starts_with(path))
            .map(|p| SharedString::from(p.to_string()))
            .collect();
        for up in gpui_component_assets::Assets.list(path)? {
            if !out.contains(&up) {
                out.push(up);
            }
        }
        Ok(out)
    }
}

/// dat0-owned icon names, usable anywhere `gpui_component::IconName` is.
///
/// The blanket `impl<T: IconNamed> From<T> for Icon` upstream means
/// `Icon::new(Dat0IconName::Filter)` works exactly like
/// `Icon::new(IconName::Close)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dat0IconName {
    Filter,
    Play,
    Layers,
    Bookmark,
    History,
}

impl Dat0IconName {
    /// Every variant — the tests and the gallery iterate this so a new icon
    /// cannot be added without being covered and displayed.
    pub const ALL: &'static [Dat0IconName; 5] = &[
        Dat0IconName::Filter,
        Dat0IconName::Play,
        Dat0IconName::Layers,
        Dat0IconName::Bookmark,
        Dat0IconName::History,
    ];
}

impl IconNamed for Dat0IconName {
    fn path(self) -> SharedString {
        match self {
            // Upstream Lucide renamed `filter` to `funnel`; the vendored file
            // keeps the current upstream name so it stays a verbatim copy.
            Self::Filter => "icons/funnel.svg",
            Self::Play => "icons/play.svg",
            Self::Layers => "icons/layers.svg",
            Self::Bookmark => "icons/bookmark.svg",
            Self::History => "icons/clock.svg",
        }
        .into()
    }
}

/// Upstream icon paths dat0 actually references. Kept as an explicit list so
/// `bundled_icons_resolve_through_fallback` fails if a rev bump drops one.
pub const BUNDLED_USED: &'static [&'static str; 5] = &[
    "icons/close.svg",
    "icons/chevron-down.svg",
    "icons/chevron-up.svg",
    "icons/chevron-right.svg",
    "icons/chevrons-up-down.svg",
];
```

In `crates/dat0-app/src/lib.rs`, add alongside the other `pub mod` declarations:

```rust
pub mod assets;
```

- [ ] **Step 7: Run the test to verify it passes**

Run: `cargo test -p dat0-app --test icon_assets`
Expected: PASS, 6 tests.

If `dat0_icons_resolve` fails with "resolved to None", the rust-embed `#[folder]` path is wrong: it is relative to the crate root (`crates/dat0-app/`), so `assets` means `crates/dat0-app/assets`.

- [ ] **Step 8: Verify the release binary does not grow unexpectedly**

The five SVGs total under 2 KB; `gpui-component-assets` adds ~90 KB. Confirm the assets crate resolved to the pinned rev rather than crates.io:

Run: `cargo tree -p dat0-app -i gpui-component-assets`
Expected: shows the git source with rev `0f0ab35…`, not a registry version.

- [ ] **Step 9: Local gate**

Run: `cargo fmt --all -- --check && cargo clippy -p dat0-app --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml Cargo.lock crates/dat0-app/Cargo.toml crates/dat0-app/src/assets.rs \
        crates/dat0-app/src/lib.rs crates/dat0-app/assets crates/dat0-app/tests/icon_assets.rs
git commit -s -m "feat(theme): A5 T1 — Dat0Assets + Dat0IconName (UI redesign)"
```

---

## Task 2: Register the AssetSource

**Files:**
- Modify: `crates/dat0-app/src/window.rs:1741`
- Modify: `crates/dat0-app/examples/gallery.rs:22`

**Interfaces:**
- Consumes: `dat0_app::assets::Dat0Assets` (Task 1)
- Produces: nothing new — but every later task's icons render blank until this lands

- [ ] **Step 1: Register on the production Application**

`crates/dat0-app/src/window.rs`, currently line 1741:

```rust
// before
let application = Application::new();

// after
// A5: the ONE AssetSource for the process. Without it every `Icon` renders as
// nothing at all — gpui does not panic on an unresolved asset path (A0 spike).
let application = Application::new().with_assets(crate::assets::Dat0Assets);
```

- [ ] **Step 2: Register on the gallery example**

`crates/dat0-app/examples/gallery.rs`, currently line 22. The file already carries an A5 note at lines 9-11 saying this must happen; delete that note and make the change:

```rust
// before
Application::new().run(|cx| {

// after
Application::new()
    .with_assets(dat0_app::assets::Dat0Assets)
    .run(|cx| {
```

- [ ] **Step 3: Verify the example still builds**

Run: `cargo build -p dat0-app --example gallery --features gallery`
Expected: builds clean.

- [ ] **Step 4: Verify the app still builds**

Run: `cargo build -p dat0-app`
Expected: builds clean.

- [ ] **Step 5: Local gate**

Run: `cargo fmt --all -- --check && cargo clippy -p dat0-app --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/dat0-app/src/window.rs crates/dat0-app/examples/gallery.rs
git commit -s -m "feat(theme): A5 T2 — register Dat0Assets on both Applications (UI redesign)"
```

---

## Task 3: i18n keys

Adding the keys before the call sites means every later task can reference them without touching the catalog, and no task ships a `t()` call whose key is missing.

**Files:**
- Modify: `crates/dat0-i18n/src/strings/en.json`

**Interfaces:**
- Consumes: nothing
- Produces: the i18n keys Tasks 4-6 call via `dat0_i18n::t("…")`

- [ ] **Step 1: Check which keys already exist**

Run:
```bash
grep -nE '"(common\.close|sql\.close_tab|sql\.history|grid\.sort|grid\.filter)"' \
  crates/dat0-i18n/src/strings/en.json
```
Expected: `sql.close_tab` already exists (`"Close query tab"`). Do not duplicate it.

- [ ] **Step 2: Add the new keys**

Insert into `crates/dat0-i18n/src/strings/en.json`, each in its existing namespace block (the file is a flat JSON object of `"key": "value"` pairs, sorted loosely by namespace):

```json
  "common.close": "Close",
  "common.cancel": "Cancel",
  "common.collapse": "Collapse",
  "common.expand": "Expand",
  "common.back": "Back",
  "common.forward": "Forward",
  "grid.sort": "Sort column",
  "grid.filter": "Filter column",
  "sql.history": "Query history",
  "pipeline.remove_step": "Remove step",
  "pipeline.base": "Base table",
  "pipeline.step_separator": "then",
  "catalog.toggle_group": "Toggle group",
  "inspector.toggle_section": "Toggle section",
  "hero.play_demo": "Open demo workspace"
```

- [ ] **Step 3: Verify the JSON parses**

Run: `python3 -m json.tool crates/dat0-i18n/src/strings/en.json > /dev/null && echo OK`
Expected: `OK`.

- [ ] **Step 4: Verify the i18n crate tests still pass**

Run: `cargo test -p dat0-i18n`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-i18n/src/strings/en.json
git commit -s -m "feat(theme): A5 T3 — i18n keys for icon-button labels (UI redesign)"
```

---

## Task 4: Convert the `✕` close buttons

Grouped because every site is the same mechanical edit against the same icon and the same i18n key, so one reviewer gate covers them all.

**Files:**
- Modify: `crates/dat0-app/src/window.rs:1529`, `crates/dat0-app/src/window.rs:6491`
- Modify: `crates/dat0-app/src/view/sql_console.rs:696,738,990,1100`
- Modify: `crates/dat0-app/src/view/pipeline_bar.rs:143`
- Modify: `crates/dat0-app/src/grid/cell_editor.rs:156`
- Modify: `crates/dat0-app/src/actions/view_actions.rs:142`

**Interfaces:**
- Consumes: `Dat0Assets` (T1), registration (T2), i18n keys (T3)
- Produces: nothing — presentational only

- [ ] **Step 1: Find every site**

Run: `grep -rn '"✕"' crates/dat0-app/src`
Expected: 9 hits across the 6 files listed above. If the count differs, stop and report — the plan's inventory is stale.

- [ ] **Step 2: Convert each element site**

For every hit of the form `.child("✕")` or `.child(SharedString::from("✕"))`, apply:

```rust
// before
.child(SharedString::from("✕"))

// after
.a11y_label(crate::a11y::AccessRole::Label, dat0_i18n::t("common.close"))
.child(gpui_component::Icon::new(gpui_component::IconName::Close))
```

Add `use crate::a11y::A11yExt as _;` to any file that does not already import it.

Where a more specific key exists, prefer it over `common.close`:
- `view/sql_console.rs:696` (tab strip close) → `dat0_i18n::t("sql.close_tab")`
- `view/sql_console.rs:990` (error dismiss) → `dat0_i18n::t("sql.error.dismiss")`
- `view/sql_console.rs:1100` (history overlay close) → `dat0_i18n::t("sql.history.close")`
- `view/pipeline_bar.rs:143` (chip remove) → `dat0_i18n::t("pipeline.remove_step")`

- [ ] **Step 3: Convert the one `.label()` site**

`crates/dat0-app/src/grid/cell_editor.rs:155-158` is a gpui-component `Button`, not a div. It is the header-rename **cancel** button (id `header-rename-cancel`, emits `HeaderRenameEvent::Cancel`), so the accessible name is "Cancel", not "Close".

`Button::icon(impl Into<Icon>)` exists at this rev (`crates/ui/src/button/button.rs:278`), and `impl InteractiveElement for Button` (`:418`) means the `A11yExt` blanket impl applies, so `a11y_label` chains directly onto a `Button`.

Drop `.label()` entirely rather than replacing the glyph with the word — a ghost button showing "Cancel" is visibly wider than the `✕` it replaces, and this slice must not move layout:

```rust
// before
let cancel_btn = Button::new("header-rename-cancel")
    .label("✕")
    .ghost()

// after
let cancel_btn = Button::new("header-rename-cancel")
    .icon(gpui_component::IconName::Close)
    .ghost()
    .a11y_label(crate::a11y::AccessRole::Label, dat0_i18n::t("common.cancel"))
```

- [ ] **Step 4: Verify no `✕` remains as an element**

Run: `grep -rn '"✕"' crates/dat0-app/src`
Expected: no hits. (`format!`-embedded `✗` at `sql_console.rs:1396,1491` and `ai/panel.rs:74` are a DIFFERENT character and stay — do not touch them.)

- [ ] **Step 5: Verify the ratchet and the nav suites**

Run:
```bash
cargo test -p dat0-app --test style_lint
cargo test -p dat0-app --features a11y-capture --test keyboard_nav --test sql_console_nav --test sql_console_transient_nav --test cell_editor_nav
```
Expected: all PASS, unchanged counts. A style_lint failure here means a color literal moved — revert and re-apply without touching color lines.

- [ ] **Step 6: Local gate**

Run: `cargo fmt --all -- --check && cargo clippy -p dat0-app --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/dat0-app/src
git commit -s -m "feat(theme): A5 T4 — close buttons use Icon (UI redesign)"
```

---

## Task 5: Convert chevrons, sort, and funnel

**Files:**
- Modify: `crates/dat0-app/src/view/sql_console.rs:862`
- Modify: `crates/dat0-app/src/view/pipeline_bar.rs:125,185,241,294`
- Modify: `crates/dat0-app/src/grid/mod.rs:288,314`
- Modify: `crates/dat0-app/src/catalog/panel.rs:121`
- Modify: `crates/dat0-app/src/inspector/panel.rs:105,153`

**Interfaces:**
- Consumes: `Dat0IconName` (T1), i18n keys (T3)
- Produces: nothing — presentational only

- [ ] **Step 1: Apply the mapping**

| File:line | Glyph | Replacement | a11y key |
|---|---|---|---|
| `view/sql_console.rs:862` | `▾` | `IconName::ChevronDown` | `common.expand` |
| `view/pipeline_bar.rs:125` | `›` | `IconName::ChevronRight` | `pipeline.step_separator` |
| `view/pipeline_bar.rs:185` | `⌃` | `IconName::ChevronUp` | `common.collapse` |
| `view/pipeline_bar.rs:241` | `›` | `IconName::ChevronRight` | `pipeline.step_separator` |
| `view/pipeline_bar.rs:294` | `⌄` | `IconName::ChevronDown` | `common.expand` |
| `grid/mod.rs:288` | `⇅` | `IconName::ChevronsUpDown` | `grid.sort` |
| `grid/mod.rs:314` | `⌄` | `Dat0IconName::Filter` | `grid.filter` |
| `catalog/panel.rs:121` | `▾` / `▸` | `ChevronDown` / `ChevronRight` | `catalog.toggle_group` |
| `inspector/panel.rs:153` | `▾` / `▸` | `ChevronDown` / `ChevronRight` | `inspector.toggle_section` |

`empty_state.rs` is **not** in this task. Lines 17, 18 and 128 are `//!` and `///` **doc comments** describing the enriched hero band ("Tagline + \"[Take a tour ›]\" button — wired in T7"), not render elements. The scope rule excludes them and no `Dat0IconName::Play` call site exists in this slice. `Dat0IconName::Play` and `Bookmark` are defined, tested and shown in the gallery, but have no production consumer until a later slice — that is intentional and must not be "fixed" by inventing one.

The shape at each element site:

```rust
// before
.child("▾")

// after
.a11y_label(crate::a11y::AccessRole::Label, dat0_i18n::t("common.expand"))
.child(gpui_component::Icon::new(gpui_component::IconName::ChevronDown))
```

`grid/mod.rs:314` is the funnel zone — the surrounding doc comment already calls it "the funnel-icon zone" while the glyph was a chevron. Use `Dat0IconName::Filter`:

```rust
.a11y_label(crate::a11y::AccessRole::Label, dat0_i18n::t("grid.filter"))
.child(gpui_component::Icon::new(dat0_app_icon()))
```
where the import is `use crate::assets::Dat0IconName;` and the expression is `Icon::new(Dat0IconName::Filter)`.

- [ ] **Step 2: Verify no converted glyph remains as an element**

Run: `grep -rnP '\.child\("[⌄⌃▾▸‹›⇅]"\)' crates/dat0-app/src`
Expected: no hits.

- [ ] **Step 3: Verify the ratchet and the nav suites**

Run:
```bash
cargo test -p dat0-app --test style_lint
cargo test -p dat0-app --features a11y-capture --test keyboard_nav --test catalog_nav --test sql_console_nav --test a11y_content
```
Expected: all PASS.

- [ ] **Step 4: Local gate**

Run: `cargo fmt --all -- --check && cargo clippy -p dat0-app --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/src
git commit -s -m "feat(theme): A5 T5 — chevrons, sort and funnel use Icon (UI redesign)"
```

---

## Task 6: Convert layers, bookmark, and history

The three remaining dat0-owned icons, including the one label restructure.

**Files:**
- Modify: `crates/dat0-app/src/view/pipeline_bar.rs:91,222`
- Modify: `crates/dat0-app/src/view/sql_console.rs:1209`

**Interfaces:**
- Consumes: `Dat0IconName` (T1), i18n keys (T3)
- Produces: nothing — presentational only

- [ ] **Step 1: Convert the simple element sites**

`view/pipeline_bar.rs:91`:
```rust
// before
.child("▣"),

// after
.a11y_label(crate::a11y::AccessRole::Label, dat0_i18n::t("pipeline.base"))
.child(gpui_component::Icon::new(crate::assets::Dat0IconName::Layers)),
```

`view/sql_console.rs:1209`:
```rust
// before
.child("🕘")

// after
.a11y_label(crate::a11y::AccessRole::Label, dat0_i18n::t("sql.history"))
.child(gpui_component::Icon::new(crate::assets::Dat0IconName::History))
```

- [ ] **Step 2: Restructure the one mixed label**

`view/pipeline_bar.rs:222` is `.child("▣ base")` — glyph AND text in one string on a clickable div. It becomes a flex row:

```rust
// before
.child("▣ base")

// after
.a11y_label(crate::a11y::AccessRole::Label, dat0_i18n::t("pipeline.base"))
.child(
    gpui_component::h_flex()
        .gap_sp(crate::theme::tokens::Sp::S4)
        .child(gpui_component::Icon::new(crate::assets::Dat0IconName::Layers))
        .child("base"),
)
```

This needs `use crate::theme::tokens::{Sp, SpStyled as _};` in the file if not already imported. `h_flex` comes from `gpui_component::h_flex`.

- [ ] **Step 3: Verify no dat0-icon glyph remains as an element**

Run: `grep -rnP '\.child\("(▣|🕘)' crates/dat0-app/src`
Expected: no hits.

- [ ] **Step 4: Verify the ratchet, pipeline tests, and nav suites**

Run:
```bash
cargo test -p dat0-app --test style_lint --test pipeline_bar
cargo test -p dat0-app --features a11y-capture --test sql_console_transient_nav --test keyboard_nav
```
Expected: all PASS. `pipeline_bar` is the suite most likely to notice the `"▣ base"` restructure — if it asserts on that string, update the assertion to match the new text-only `"base"` child and say so in the commit message.

- [ ] **Step 5: Local gate**

Run: `cargo fmt --all -- --check && cargo clippy -p dat0-app --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/dat0-app/src
git commit -s -m "feat(theme): A5 T6 — layers, bookmark and history use Icon (UI redesign)"
```

---

## Task 7: Gallery icons section

**Files:**
- Modify: `crates/dat0-app/src/gallery.rs`
- Modify: `crates/dat0-app/tests/gallery_smoke.rs`

**Interfaces:**
- Consumes: `Dat0IconName::ALL`, `BUNDLED_USED` (Task 1)
- Produces: the `"gallery.icons"` a11y seam

- [ ] **Step 1: Write the failing test**

In `crates/dat0-app/tests/gallery_smoke.rs`, add `"gallery.icons"` to the `SECTIONS` array:

```rust
const SECTIONS: &[&str] = &[
    "gallery.theme",
    "gallery.colors",
    "gallery.scales",
    "gallery.elevation",
    "gallery.components",
    "gallery.icons",
];
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p dat0-app --features a11y-capture,gallery --test gallery_smoke`
Expected: FAIL with `gallery section missing: gallery.icons`.

- [ ] **Step 3: Implement the section**

In `crates/dat0-app/src/gallery.rs`, add the import and the section function, then mount it.

Import (extend the existing `use` block):
```rust
use crate::assets::{BUNDLED_USED, Dat0IconName};
use gpui_component::{Icon, IconName};
```

Section function, placed after `components_section`:
```rust
/// One icon plus its name, rendered at a given text role so size inheritance
/// is visible — `Icon` derives its size from `window.text_style().font_size`
/// when no explicit size is set (gpui-component icon.rs RenderOnce).
fn icon_cell(theme: &ComponentTheme, icon: Icon, name: &str, role: TextRole) -> impl IntoElement {
    v_flex()
        .gap_sp(Sp::S2)
        .w(Sp::S32.pixels() * 3.0)
        .child(div().text_role(role).text_color(theme.foreground).child(icon))
        .child(
            div()
                .text_role(TextRole::Caption)
                .text_color(theme.muted_foreground)
                .child(name.to_string()),
        )
}

/// dat0-owned icons and the upstream ones dat0 references, side by side at
/// three text roles. Vendored and bundled icons sit in the same grid on
/// purpose: a stroke-weight or optical-size mismatch in a vendored file is only
/// obvious next to the set it has to live with.
fn icons_section(theme: &ComponentTheme) -> impl IntoElement {
    let roles = [TextRole::Caption, TextRole::Body, TextRole::Display];

    let mut rows = v_flex().gap_sp(Sp::S8);
    for role in roles {
        let mut row = h_flex().gap_sp(Sp::S8);
        for name in Dat0IconName::ALL {
            row = row.child(icon_cell(
                theme,
                Icon::new(*name),
                &format!("{name:?}"),
                role,
            ));
        }
        for path in BUNDLED_USED {
            let short = path
                .trim_start_matches("icons/")
                .trim_end_matches(".svg")
                .to_string();
            row = row.child(icon_cell(theme, Icon::empty().path(*path), &short, role));
        }
        rows = rows.child(row);
    }

    section(theme, "gallery.icons", "Icons", rows)
}
```

Mount it in `render` after `components_section`:
```rust
            .child(components_section(theme, &self.sample_input))
            .child(icons_section(theme))
```

`Icon::empty().path(p)` is used for the bundled set because `BUNDLED_USED` holds paths rather than `IconName` variants — `Icon::path` is public and documented for exactly this (`icon.rs:273`).

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p dat0-app --features a11y-capture,gallery --test gallery_smoke`
Expected: PASS.

- [ ] **Step 5: Verify the gallery is still literal-free**

Run: `cargo test -p dat0-app --test style_lint`
Expected: PASS — `gallery.rs` is gated at an allowance of 0, so any color literal added here fails.

- [ ] **Step 6: Local gate**

Run: `cargo fmt --all -- --check && cargo clippy -p dat0-app --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/dat0-app/src/gallery.rs crates/dat0-app/tests/gallery_smoke.rs
git commit -s -m "feat(theme): A5 T7 — gallery icons section (UI redesign)"
```

---

## Task 8: NOTICE.md attribution

**Files:**
- Modify: `NOTICE.md`

**Interfaces:**
- Consumes: the vendored `LICENSE-lucide` (Task 1)
- Produces: nothing

- [ ] **Step 1: Add the hand-written section**

Insert into `NOTICE.md` **between** the `## Third-party software` prose paragraph (currently ending line 18) and the `<!-- BEGIN cargo-about generated -->` marker (currently line 20). It must go ABOVE the marker so `scripts/notice-extract.sh` — which captures only what is between the markers — is unaffected.

```markdown
## Bundled assets

dat0 embeds Lucide icons in the application binary. `cargo-about` sees only the
crates dat0 depends on, not the artwork inside them, so the icons are recorded
here by hand.

- **86 icons** ship via the `gpui-component-assets` crate (listed in the
  generated section below as an Apache-2.0 dependency; the artwork inside it is
  Lucide's).
- **5 icons** are vendored directly into `crates/dat0-app/assets/icons/`:
  `funnel.svg`, `play.svg`, `layers.svg`, `bookmark.svg`, `clock.svg`.

Lucide is dual-licensed. Most icons are ISC; icons derived from the Feather
project are MIT (Copyright (c) 2013-present Cole Bemis). dat0 ships icons under
both — `clock`, `x`, `check` and the `chevron-*` family are among the
Feather-derived set. The complete upstream license text covering both, including
the authoritative list of Feather-derived icons, is vendored verbatim at
`crates/dat0-app/assets/icons/LICENSE-lucide`.

```
ISC License

Copyright (c) 2026 Lucide Icons and Contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
```
```

- [ ] **Step 2: Verify the extraction script is unaffected**

Run:
```bash
./scripts/notice-extract.sh NOTICE.md | head -3
./scripts/notice-extract.sh NOTICE.md | grep -c 'Bundled assets'
```
Expected: the first command prints the first generated license entry (a `## …` heading), and the second prints `0` — the new section must NOT be inside the markers.

- [ ] **Step 3: Regenerate the cargo-about block**

Adding `gpui-component-assets` changed `Cargo.lock`, so the generated list is now stale.

Run:
```bash
cargo install cargo-about --locked --features=cli   # if not present
cargo about generate -c about.toml docs/about-template.hbs > /tmp/third-party.txt
```

Replace the content between `<!-- BEGIN cargo-about generated -->` and
`<!-- END cargo-about generated -->` in `NOTICE.md` with `/tmp/third-party.txt`.

If `cargo about` cannot run locally, leave the generated block untouched and note it in the commit message — the NOTICE drift job is `continue-on-error: true` (warn-only, PD-003) because its per-platform license tiebreak is non-deterministic, so a stale block warns rather than blocks.

- [ ] **Step 4: Verify the diff check**

Run: `./scripts/notice-extract.sh NOTICE.md > /tmp/notice-current.txt && diff /tmp/notice-current.txt /tmp/third-party.txt && echo "NOTICE in sync"`
Expected: `NOTICE in sync`, or a diff you accept per Step 3's fallback.

- [ ] **Step 5: Commit**

```bash
git add NOTICE.md
git commit -s -m "docs(theme): A5 T8 — Lucide attribution in NOTICE (UI redesign)"
```

---

## Task 9: Full-suite verification (CONTROLLER ONLY)

Do not dispatch this to a task implementer. The full sweep is long and has stranded implementers before (A4 retro).

**Files:** none

- [ ] **Step 1: Full workspace format and lint**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 2: Full dat0-app suite**

Run: `cargo test -p dat0-app`
Expected: all PASS.

- [ ] **Step 3: Full a11y-capture suite**

Run: `cargo test -p dat0-app --features a11y-capture`
Expected: all PASS, with keyboard_nav cycle counts unchanged from `5b63d3e`.

- [ ] **Step 4: Gallery feature suite**

Run: `cargo test -p dat0-app --features a11y-capture,gallery`
Expected: all PASS including `gallery_smoke` with 6 seams.

- [ ] **Step 5: Confirm no glyph regressions**

Run:
```bash
grep -rnP '\.child\("(✕|⌄|⌃|▾|▸|‹|›|⇅|▣|🕘)' crates/dat0-app/src
```
Expected: no hits.

- [ ] **Step 6: Confirm the release binary carries the assets**

Run:
```bash
cargo build -p dat0-app --release
ls -la target/release/dat0
```
Expected: builds; binary grows by roughly 90-100 KB versus `5b63d3e`.

- [ ] **Step 7: Human glance (OWED — cannot be automated)**

Run: `cargo run -p dat0-app --example gallery --features gallery`

Check, in all three themes:
- Every icon renders — no blank cells. A blank cell is the failure this whole slice is designed around.
- The 5 vendored icons match the 5 bundled ones in stroke weight and optical size.
- Icon size tracks the text role across the three rows.
- Then boot the real app and check the SQL console tab-close, the grid sort/funnel zones, the catalog and inspector disclosure chevrons, and the pipeline bar.

---

## Self-Review

**Spec coverage:**

| Design section | Task |
|---|---|
| §1 `Dat0Assets` | 1 |
| §2 `Dat0IconName` | 1 |
| §3 call-site migration + scope rule | 4, 5, 6 |
| §4 i18n | 3 |
| §5 NOTICE.md | 8 |
| §6 gallery | 7 |
| §7 tests `icon_assets.rs` | 1 |
| §8 non-goals | enforced by the greps in 4/5/6 Step 3 and 9 Step 5 |
| §9 risks | blank-icon → 1 (tests) + 7 (gallery) + 9 Step 7; upstream shadowing → 1; vendored mismatch → 7; ratchet drift → per-task style_lint runs |
| §10 owed human glances | 9 Step 7 |

**Deviations from the design, and why:**
- The design named the vendored filter icon `filter.svg`. Upstream Lucide renamed it to **`funnel`** and `filter.svg` now 404s, so the vendored file is `funnel.svg` and `Dat0IconName::Filter` maps to `icons/funnel.svg`. The enum variant keeps the domain name; the file keeps the upstream name so it stays a verbatim copy.
- The design described Lucide as ISC. It is **dual-licensed** — ISC plus MIT for the Feather-derived subset, which includes `clock`, `x`, `check` and `chevron-*`, all of which dat0 ships. Task 8 records both. This is why the design said to copy the license at write time rather than from memory.
- `empty_state.rs:17,18,128` are flagged in Task 5 Step 2 as **verify-before-editing** rather than asserted conversions: the grep context suggests they are test-expectation strings, not render elements, and the scope rule excludes those.
