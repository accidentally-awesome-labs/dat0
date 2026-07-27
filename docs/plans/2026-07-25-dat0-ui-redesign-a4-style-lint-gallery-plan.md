# dat0 UI redesign — Slice A4: style-lint ratchet + token gallery (implementation plan)

> **For agentic workers:** REQUIRED SUB-SKILL: use `superpowers:subagent-driven-development`
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the enforcement layer of the design system — a repo-wide gate that
bans inline color literals with a shrink-only per-file ratchet, plus a runnable
token gallery that becomes the manual-UAT vehicle for every later slice.

**Architecture:** Two independent deliverables, neither touching a production
render path. (1) `tests/style_lint.rs` — a pure text scanner over
`crates/dat0-app/src/**/*.rs` with substring + regex pattern matching, a per-line
escape comment, and a `&[(&str, usize)]` allowance table that fails both when a
file exceeds its count (regression) and when it falls below it (stale ratchet).
(2) `src/gallery.rs` behind a `gallery` cargo feature, rendered by a ~20-line
`examples/gallery.rs` and mounted headlessly by `tests/gallery_smoke.rs`.

**Tech Stack:** Rust 2024 · gpui `=0.2.2` · gpui-component pinned rev `0f0ab35` ·
`regex` (already a normal dependency, reachable from test targets) · kittest 0.3.0 +
accesskit 0.21.1 via the existing `support` harness.

**Design doc:** `docs/plans/2026-07-25-dat0-ui-redesign-a4-style-lint-gallery-design.md`
**Master plan:** `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §5 row A4
**Branch:** `feat/ui-redesign-a4-style-lint-gallery` off main `dca3c9c`

## Global Constraints

- **No production render path changes.** No file under `src/` is edited except
  `lib.rs` (one `#[cfg]` module line), the new `src/gallery.rs`, and the deletion
  of one test fn in `src/theme/tokens.rs`. The full nav/a11y suite must be
  byte-identical in behavior.
- **Zero new dependencies.** `regex` is already in `[dependencies]`; cargo passes
  normal deps *and* dev-deps to a package's test targets (proven in-repo:
  `tests/cli_replay_inspect.rs` uses `serde_json`, which is not a dev-dep).
- **Zero literal policy in new code.** `src/gallery.rs` is scanned by
  `tests/style_lint.rs` with an implicit allowance of 0. Every color comes from
  `cx.theme()`, `cx.theme().d0()`, `Sp`, `TextRole`, `Elevation`, or `Density`.
- **No i18n keys for gallery strings** — it is a dev-only surface that never ships.
- **`a11y-capture` is auto-on** for this crate's integration tests via the
  self-dev-dependency; no `ci.yml` change in this slice.
- **Never write the literal CI-skip marker** (`[` + `skip ci` + `]`) anywhere in a
  commit message or PR body, not even quoted in prose — it silently skipped two
  main runs after A1 (see `dat0-dev-workflow` memory).
- **DCO:** every commit uses `git commit -s` and ends with
  `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`.
- Toolchain gates that must pass before the PR: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, full `cargo test -p dat0-app`.

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `crates/dat0-app/tests/style_lint.rs` | create | The gate. Walk + patterns + escape parsing + allowance ratchet + failure report. Self-tests its own scanner on synthetic input. |
| `crates/dat0-app/src/theme/tokens.rs` | modify (delete ~25 lines) | Remove `tokens_module_stays_literal_free` — superseded by style_lint (design D7). |
| `crates/dat0-app/Cargo.toml` | modify | `[features] gallery = []`; add `"gallery"` to the self-dev-dependency's feature list. |
| `crates/dat0-app/src/lib.rs` | modify (+2 lines) | `#[cfg(feature = "gallery")] pub mod gallery;` |
| `crates/dat0-app/src/gallery.rs` | create | `GalleryView` + five section fns. Dev-only, zero-literal, one a11y seam per section. |
| `crates/dat0-app/examples/gallery.rs` | create | ~20-line `main` that boots a window around `GalleryView`. |
| `crates/dat0-app/tests/gallery_smoke.rs` | create | Headless mount + one forced frame + all five section labels asserted. |

## Task order and model assignment

| Task | Deliverable | Model |
|---|---|---|
| T1 | `tests/style_lint.rs` + D7 deletion | **opus** — load-bearing gate every A6 sub-slice depends on |
| T2 | Feature wiring + `GalleryView` + theme row + colors section + smoke test | sonnet |
| T3 | Scales + elevation sections | haiku |
| T4 | Components section | sonnet |
| T5 | `examples/gallery.rs` | haiku |
| T6 | Full gate + whole-branch review + PR | controller + **opus** review |

Per-task review after each (sonnet), whole-branch final review by opus before the
PR — the transient-bars lesson: only the cross-cutting review catches
design-defeating bugs that every per-task review and every green test miss.

---

### Task 1: `tests/style_lint.rs` — the enforcement ratchet

**Files:**
- Create: `crates/dat0-app/tests/style_lint.rs`
- Modify: `crates/dat0-app/src/theme/tokens.rs` (delete `tokens_module_stays_literal_free`)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: nothing other tasks import. T2-T5 must keep `src/gallery.rs` at 0
  violations — this task's gate is what enforces that.

**Context you need:**
- The scanner walks `concat!(env!("CARGO_MANIFEST_DIR"), "/src")` — i.e.
  `crates/dat0-app/src`. It must NOT walk `tests/` (theme gates legitimately hold
  `#rrggbb` fixture strings) nor `src/theme/builtins/*.json` (palette data, and
  the walk only collects `*.rs` anyway).
- This file lives in `tests/`, so it can never scan itself — the concat-splitting
  trick `tokens.rs` needed for its in-module self-lint is unnecessary here. Say so
  in the module doc so nobody re-adds it.
- `regex` has no lookaround support. The bare-hex pattern below is written with
  explicit boundary character classes for that reason.

- [ ] **Step 1: Write the scanner and its self-tests (synthetic input only)**

Create `crates/dat0-app/tests/style_lint.rs`:

```rust
//! Repo-wide style gate (UI redesign A4, master plan §5 row A4).
//!
//! Bans inline color construction in `crates/dat0-app/src/**/*.rs` so the A1-A3
//! token system cannot be bypassed, and ratchets the pre-A6 backlog DOWN: each
//! A6 sub-slice that migrates a file must lower that file's number here, and the
//! gate fails if a number is left too high.
//!
//! ## Why constructors are banned regardless of argument
//! The master plan's original sketch banned only the literal form (`rgb(0x`).
//! That misses `gpui::rgb(crate::a11y::FOCUS_RING)` — five real call sites where
//! the literal hides one `const` indirection away. Banning the constructor plus
//! bare 6/8-digit hex catches both halves.
//!
//! ## Why this file cannot self-match
//! The pattern table below is source code in `tests/`, and the walk covers `src/`
//! only. No concat-splitting needed (unlike the A2 in-module self-lint this
//! supersedes). Do not move this file into `src/`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

/// Banned as plain substrings, regardless of argument.
/// `hsl(` is not a prefix of `hsla(` (the next char differs), so both are listed.
const BANNED_SUBSTRINGS: &[&str] = &[
    "rgb(",
    "rgba(",
    "hsla(",
    "hsl(",
    "white()",
    "black()",
    "parse_hex",
];

/// Per-line escape. The reason text is mandatory and must be non-empty.
const ALLOW_MARKER: &str = "// style-lint: allow(";

/// Files that still hold pre-A6 inline colors, with their EXACT current count of
/// offending LINES (a line with two constructors counts once).
///
/// SHRINK-ONLY RATCHET. Each A6 sub-slice that migrates a file lowers its number
/// in the same PR; the gate fails if a count is left too high *or* too low.
/// A file absent from this table has an allowance of 0.
const ALLOW: &[(&str, usize)] = &[
    ("a11y/mod.rs", 2),
    ("catalog/panel.rs", 4),
    ("charts/mod.rs", 2),
    ("charts/panel.rs", 2),
    ("empty_state.rs", 1),
    ("error_ux/banner.rs", 4),
    ("grid/mod.rs", 7),
    ("onboarding/mod.rs", 2),
    ("settings_ui/panel.rs", 1),
    ("view/pipeline_bar.rs", 9),
    ("view/query_library.rs", 1),
    ("window.rs", 1),
];

/// Bare `0x` + exactly 6 or 8 hex digits, boundary-guarded on both sides.
///
/// The 6-or-8 anchor is what makes this rule affordable: 2-digit (`0x89` PNG
/// magic, `0xE2` UTF-8 continuation bytes) and 4-digit (`0xfc00` IPv6 prefixes)
/// hex stays legal, so the only thing it catches in `src/` today is the one real
/// color const, `a11y/mod.rs` `FOCUS_RING`.
fn bare_hex_re() -> Regex {
    Regex::new(r"(?i)(^|[^0-9a-z_])0x[0-9a-f]{6}([0-9a-f]{2})?([^0-9a-f]|$)")
        .expect("bare-hex pattern must compile")
}

#[derive(Debug)]
struct Violation {
    line_no: usize,
    pattern: String,
    text: String,
}

#[derive(Default, Debug)]
struct ScanResult {
    violations: Vec<Violation>,
    /// Lines carrying an allow marker but NO banned pattern — the escape
    /// outlived the code it excused.
    stale_allows: Vec<usize>,
    /// Lines whose allow marker has an empty reason.
    empty_reasons: Vec<usize>,
}

/// The first banned pattern on `line`, if any.
fn banned_hit(line: &str, hex: &Regex) -> Option<String> {
    for pat in BANNED_SUBSTRINGS {
        if line.contains(pat) {
            return Some((*pat).to_string());
        }
    }
    hex.find(line).map(|m| m.as_str().trim().to_string())
}

/// The reason text inside `// style-lint: allow(<reason>)`, or `None` when the
/// line carries no well-formed marker (a marker with no closing paren does not
/// excuse anything).
fn allow_reason(line: &str) -> Option<&str> {
    let rest = line.split_once(ALLOW_MARKER)?.1;
    let end = rest.find(')')?;
    Some(rest[..end].trim())
}

fn scan(src: &str, hex: &Regex) -> ScanResult {
    let mut out = ScanResult::default();
    for (i, line) in src.lines().enumerate() {
        let line_no = i + 1;
        match (banned_hit(line, hex), allow_reason(line)) {
            (_, Some("")) => out.empty_reasons.push(line_no),
            (Some(_), Some(_)) => {} // excused, with a reason
            (Some(pattern), None) => out.violations.push(Violation {
                line_no,
                pattern,
                text: line.trim().to_string(),
            }),
            (None, Some(_)) => out.stale_allows.push(line_no),
            (None, None) => {}
        }
    }
    out
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir must succeed") {
        let path = entry.expect("dir entry must resolve").path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn scanner_flags_constructors_and_bare_hex_only() {
    let hex = bare_hex_re();
    // (source line, expected violation count)
    let cases: &[(&str, usize)] = &[
        ("        BannerKind::Info => gpui::rgb(0x3b82f6),", 1),
        ("pub const FOCUS_RING: u32 = 0x3b82f6;", 1),
        ("            .border_color(gpui::rgb(crate::a11y::FOCUS_RING));", 1),
        ("                .text_color(gpui::white())", 1),
        ("        .bg(gpui::rgba(0x80808022))", 1),
        ("    area.fill(&WHITE)?;", 0), // plotters CONST, not a call
        ("        assert_eq!(&bytes[0..4], &[0x89, b'P', b'N', b'G']);", 0),
        ("            IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1)),", 0),
        ("        let x = self.ring.opacity(0.13);", 0),
    ];
    for (line, expected) in cases {
        let res = scan(line, &hex);
        assert_eq!(
            res.violations.len(),
            *expected,
            "wrong verdict for: {line}\n{res:?}"
        );
    }
}

#[test]
fn escape_comment_suppresses_and_ratchets() {
    let hex = bare_hex_re();

    // A reasoned escape suppresses the violation.
    let excused = "    let c = gpui::rgb(0x112233); // style-lint: allow(plotters needs a raw RGB)";
    let res = scan(excused, &hex);
    assert!(res.violations.is_empty(), "reasoned escape must suppress");
    assert!(res.stale_allows.is_empty());
    assert!(res.empty_reasons.is_empty());

    // An escape with no banned pattern is stale and must fail.
    let stale = "    let c = theme.ring; // style-lint: allow(leftover)";
    let res = scan(stale, &hex);
    assert_eq!(res.stale_allows, vec![1], "stale escape must be reported");

    // An escape with no reason must fail.
    let no_reason = "    let c = gpui::rgb(0x112233); // style-lint: allow()";
    let res = scan(no_reason, &hex);
    assert_eq!(res.empty_reasons, vec![1], "empty reason must be reported");

    // A marker with no closing paren excuses nothing.
    let malformed = "    let c = gpui::rgb(0x112233); // style-lint: allow(unterminated";
    let res = scan(malformed, &hex);
    assert_eq!(res.violations.len(), 1, "malformed marker must not excuse");
}
```

- [ ] **Step 2: Run the scanner self-tests — they must pass before the repo scan exists**

```bash
cargo test -p dat0-app --test style_lint
```

Expected: `scanner_flags_constructors_and_bare_hex_only` and
`escape_comment_suppresses_and_ratchets` both PASS. If `.fill(&WHITE)?` is
flagged, the substring list is wrong (it must be `white()` with parens, not
`WHITE`).

- [ ] **Step 3: Add the repo scan with an ALL-ZEROS allowance (red-first)**

Temporarily replace the `ALLOW` table body with `&[]` and append this test:

```rust
#[test]
fn src_holds_no_unallowed_color_literals() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let hex = bare_hex_re();

    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    files.sort();
    assert!(
        files.len() > 50,
        "walk found only {} files under {} — the walk is broken",
        files.len(),
        root.display()
    );

    let allow: BTreeMap<&str, usize> = ALLOW.iter().copied().collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut detail: BTreeMap<String, String> = BTreeMap::new();
    let mut errors = String::new();

    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .expect("path is under src/")
            .to_string_lossy()
            .replace('\\', "/");
        let src = fs::read_to_string(path).expect("source must be readable");
        let res = scan(&src, &hex);

        for line_no in &res.stale_allows {
            errors.push_str(&format!(
                "src/{rel}:{line_no}: STALE `style-lint: allow` — no banned pattern on this line; delete the comment\n"
            ));
        }
        for line_no in &res.empty_reasons {
            errors.push_str(&format!(
                "src/{rel}:{line_no}: `style-lint: allow()` needs a non-empty reason\n"
            ));
        }
        if !res.violations.is_empty() {
            let mut lines = String::new();
            for v in &res.violations {
                lines.push_str(&format!(
                    "    src/{rel}:{}: banned `{}` — {}\n",
                    v.line_no, v.pattern, v.text
                ));
            }
            detail.insert(rel.clone(), lines);
            counts.insert(rel, res.violations.len());
        }
    }

    // Over budget → a new literal entered the tree.
    for (rel, found) in &counts {
        let budget = allow.get(rel.as_str()).copied().unwrap_or(0);
        if *found > budget {
            errors.push_str(&format!(
                "\n{rel}: {found} color-literal lines, allowance {budget} — {} new.\n\
                 Use a token from `crate::theme::tokens` (`cx.theme().d0()`), or add\n\
                 `// style-lint: allow(reason)` if the color is genuinely not a theme color.\n{}",
                found - budget,
                detail.get(rel).map(String::as_str).unwrap_or("")
            ));
        }
    }

    // Under budget → the ratchet was not tightened after a migration.
    for (rel, budget) in &allow {
        let found = counts.get(*rel).copied().unwrap_or(0);
        if found < *budget {
            errors.push_str(&format!(
                "\n{rel}: down to {found} color-literal lines but ALLOW says {budget}.\n\
                 Lower ALLOW[\"{rel}\"] to {found} (or remove the entry if it is 0).\n"
            ));
        }
    }

    assert!(errors.is_empty(), "\nstyle-lint failures:\n{errors}");
}
```

- [ ] **Step 4: Run it red and read the real numbers off the failure**

```bash
cargo test -p dat0-app --test style_lint 2>&1 | tail -60
```

Expected: FAIL, listing 12 files over budget with 36 offending lines total. The
expected per-file counts are `view/pipeline_bar.rs` 9, `grid/mod.rs` 7,
`catalog/panel.rs` 4, `error_ux/banner.rs` 4, `a11y/mod.rs` 2, `charts/mod.rs` 2,
`charts/panel.rs` 2, `onboarding/mod.rs` 2, `empty_state.rs` 1,
`settings_ui/panel.rs` 1, `view/query_library.rs` 1, `window.rs` 1.

If the observed numbers differ from that list, **the observed numbers win** —
transcribe them into `ALLOW`. Do NOT "fix" the source to match the plan; A4
migrates nothing.

- [ ] **Step 5: Restore the real ALLOW table and go green**

Put back the `ALLOW` table from Step 1 (with any corrections from Step 4), then:

```bash
cargo test -p dat0-app --test style_lint
```

Expected: all three tests PASS.

- [ ] **Step 6: Prove the ratchet bites in both directions**

Temporarily verify by hand (do not commit either edit):

1. Add `let _ = gpui::rgb(0x123456);` inside any fn in `src/empty_state.rs` →
   test must FAIL with "1 new".
2. Revert, then change `("window.rs", 1)` to `("window.rs", 2)` → test must FAIL
   with `Lower ALLOW["window.rs"] to 1`.
3. Revert both. Re-run: PASS.

- [ ] **Step 7: Delete the superseded A2 self-lint (design D7)**

In `crates/dat0-app/src/theme/tokens.rs`, delete the whole
`fn tokens_module_stays_literal_free()` test (including its `#[test]` attribute
and doc comment). Then update the module doc comment at the top of the file,
replacing "(self-lint test below)" with:

```rust
//! policy: no color constructors in this file, enforced repo-wide by
//! `tests/style_lint.rs` (A4).
```

- [ ] **Step 8: Verify tokens.rs is still covered and the crate is clean**

```bash
cargo test -p dat0-app --lib theme::tokens
cargo test -p dat0-app --test style_lint
cargo fmt --all -- --check
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: token unit tests PASS (one fewer test than before), style_lint PASS
(`tokens.rs` has an implicit 0 allowance and is not in `ALLOW`, so it is still
gated), fmt and clippy clean.

- [ ] **Step 9: Commit**

```bash
git add crates/dat0-app/tests/style_lint.rs crates/dat0-app/src/theme/tokens.rs
git commit -s -m "$(cat <<'EOF'
test(theme): A4 T1 — repo-wide style-lint ratchet (UI redesign)

Bans color construction in crates/dat0-app/src: rgb(/rgba(/hsla(/hsl(/
white()/black()/parse_hex regardless of argument, plus bare 6- or 8-digit
hex. The constructor-any-arg rule is what catches the five
gpui::rgb(crate::a11y::FOCUS_RING) sites the literal-only sketch missed.

Per-file allowance table is a shrink-only ratchet: failing when a count is
too HIGH catches a new literal in an already-allowed file, and failing when
it is too LOW forces each A6 sub-slice to tighten the number it earned.
Escape hatch is "// style-lint: allow(reason)" with a mandatory reason; an
escape with no banned pattern on its line fails as stale.

Starting state: 36 offending lines across 12 files. Supersedes the A2
in-module self-lint in tokens.rs (deleted here — one source of truth).

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Gallery scaffolding — feature, view, theme row, colors section, smoke test

**Files:**
- Modify: `crates/dat0-app/Cargo.toml` (`[features]` + dev-dep feature list)
- Modify: `crates/dat0-app/src/lib.rs` (one `#[cfg]` module declaration)
- Create: `crates/dat0-app/src/gallery.rs`
- Create: `crates/dat0-app/tests/gallery_smoke.rs`

**Interfaces:**
- Consumes: T1's gate (this task's new file must scan clean at 0).
- Produces, for T3-T5:
  - `pub struct GalleryView { sample_input: Entity<InputState> }`
  - `pub fn GalleryView::new(window: &mut Window, cx: &mut Context<Self>) -> Self`
  - Section free fns, each returning `impl IntoElement`:
    `fn theme_row(theme: &ComponentTheme) -> impl IntoElement`,
    `fn colors_section(theme: &ComponentTheme) -> impl IntoElement`.
    T3 adds `scales_section` / `elevation_section` with the same signature; T4
    adds `components_section(theme: &ComponentTheme, input: &Entity<InputState>)`.
  - `fn section(theme: &ComponentTheme, seam: &'static str, title: &str, body: impl IntoElement) -> impl IntoElement`
    — the shared section shell that emits the a11y seam. Every section uses it.
  - a11y seam labels: `gallery.theme`, `gallery.colors`, `gallery.scales`,
    `gallery.elevation`, `gallery.components`.

**Verified API facts (pinned rev `0f0ab35` / gpui `0.2.2`) — do not re-derive:**
- `gpui_component::ActiveTheme` is re-exported at the crate root (`lib.rs:87
  pub use theme::*`), gives `cx.theme() -> &Theme`, and is implemented for `App`
  (so `Context<T>` reaches it through `Deref`).
- `gpui_component::Theme` derefs to `ThemeColor`, so `theme.foreground`,
  `theme.ring`, `theme.popover` etc. read directly.
- `crate::theme::tokens::Dat0Theme::d0()` returns an owned `Dat0Colors` (21
  fields — derived on read).
- `Elevation::resolve(&Theme) -> ElevationStyle`; `.elevation(rung, theme)` needs
  `&gpui_component::Theme`.
- `gpui_component::{h_flex, v_flex} -> Div`. `.size_full()` exists.
  `.overflow_y_scroll()` is on `StatefulInteractiveElement` — it requires an
  `.id(...)` first (`div.rs:1062`).
- Buttons: `use gpui_component::button::{Button, ButtonVariants as _};`
  `Button::new("id").label("text").on_click(|_ev, _window, cx| { … })`, variants
  `.primary()` / `.ghost()` / `.danger()`.
- `crate::theme::Theme::switch(cx: &mut App, id: &str)` applies the config AND
  calls `refresh_windows()`. Valid ids: `"dark"`, `"light"`, `"high-contrast"`.
- a11y seam: `use crate::a11y::{A11yExt as _, AccessRole};` then
  `.a11y_label(AccessRole::Label, "gallery.colors")` (content-only node, no click
  id). It compiles in both capture and stub builds.

- [ ] **Step 1: Wire the feature**

In `crates/dat0-app/Cargo.toml`, add to the existing `[features]` block (right
after the `a11y-capture` line):

```toml
# A4 token gallery: dev-only surface (`src/gallery.rs` + `examples/gallery.rs`).
# Turned on for this crate's test/example targets by the self-dev-dependency
# below — same mechanism as `a11y-capture` — so it is never in a release build.
gallery = []
```

and extend the self-dev-dependency in `[dev-dependencies]`:

```toml
dat0-app = { path = ".", features = ["a11y-capture", "gallery"] }
```

In `crates/dat0-app/src/lib.rs`, add next to the other `pub mod` declarations
(keep alphabetical position if the list is ordered):

```rust
/// Dev-only token gallery (UI redesign A4). Never compiled into the shipped
/// binary — the `gallery` feature is only enabled by the self-dev-dependency.
#[cfg(feature = "gallery")]
pub mod gallery;
```

- [ ] **Step 2: Write the failing smoke test**

Create `crates/dat0-app/tests/gallery_smoke.rs`:

```rust
//! Anti-rot gate for the A4 token gallery.
//!
//! The gallery exists to be LOOKED at, which no test can do — but a section
//! silently disappearing (a token renamed at A5/A6, a `render` early-return)
//! would go unnoticed until someone booted the example. Mounting the view
//! headlessly and asserting every section's a11y seam is the cheap half of that
//! problem, and it is the whole reason the gallery lives in the lib instead of
//! inside `examples/gallery.rs` (an example body is unreachable from any test).

mod support;

use gpui::TestAppContext;
use gpui_component::Root;

use dat0_app::gallery::GalleryView;
use dat0_app::theme::Theme;
use support::A11ySnapshot;

/// Every section seam the gallery is contracted to render.
const SECTIONS: &[&str] = &[
    "gallery.theme",
    "gallery.colors",
    "gallery.scales",
    "gallery.elevation",
    "gallery.components",
];

#[gpui::test]
fn gallery_renders_all_sections(cx: &mut TestAppContext) {
    // Required before any gpui-component widget is built; `install_default`
    // then applies the dark builtin so `cx.theme()` is the real A1 palette.
    cx.update(gpui_component::init);
    cx.update(Theme::install_default);

    let (_root, vcx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        let view = cx.new(|cx| GalleryView::new(window, cx));
        Root::new(view, window, cx)
    });

    let snap = A11ySnapshot::capture(vcx);
    for seam in SECTIONS {
        assert!(snap.has_label(seam), "gallery section missing: {seam}");
    }
}
```

- [ ] **Step 3: Run it — expect a compile failure**

```bash
cargo test -p dat0-app --test gallery_smoke
```

Expected: FAIL to compile with `unresolved import dat0_app::gallery` (the module
does not exist yet). If instead it fails with "feature `gallery` is not enabled",
Step 1's dev-dependency edit did not land.

- [ ] **Step 4: Create the gallery module with the shell, theme row, and colors section**

Create `crates/dat0-app/src/gallery.rs`:

```rust
//! Token gallery — the dat0 design system rendered as one scrollable page
//! (UI redesign A4, master plan §5 row A4).
//!
//! Dev-only: gated on the `gallery` feature, which only the self-dev-dependency
//! turns on, so none of this reaches the shipped binary. Boot it with
//! `cargo run -p dat0-app --example gallery`.
//!
//! This is the manual-UAT vehicle for every later slice — the accumulated "owed
//! human glance" backlog (palette feel, HC legibility, focus ring, elevation
//! shadows, A5 icons, B1 modal scrim) is paid here in one window instead of by
//! booting the whole app once per theme.
//!
//! STRICT ZERO-LITERAL. Every color comes from `cx.theme()` / `cx.theme().d0()`,
//! every gap from `Sp`, every text size from `TextRole`, every surface from
//! `Elevation`. `tests/style_lint.rs` scans this file with an allowance of 0. If
//! a section cannot be expressed in tokens, that is a missing token, and adding
//! it is the point.

use gpui::{Entity, IntoElement, ParentElement as _, Render, Styled as _, Window, div, prelude::*};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::InputState;
use gpui_component::{ActiveTheme as _, Theme as ComponentTheme, h_flex, v_flex};

use crate::a11y::{A11yExt as _, AccessRole};
use crate::theme::Theme;
use crate::theme::tokens::{
    Dat0Theme as _, Elevation, ElevationStyled as _, Sp, SpStyled as _, TextRole, TypoStyled as _,
};

pub struct GalleryView {
    /// Live widget for the components section (T4). Built once — `InputState`
    /// needs a `Window`, which `render` does not hand to child constructors.
    sample_input: Entity<InputState>,
}

impl GalleryView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Master-plan §8 invariant: every view entity holds a theme
        // subscription. `Theme::switch` also calls `refresh_windows()`, so this
        // is belt-and-braces rather than the only repaint path — but the
        // invariant is about not depending on that.
        cx.observe_global::<Theme>(|_, cx| cx.notify()).detach();
        Self {
            sample_input: cx.new(|cx| InputState::new(window, cx).placeholder("sample input")),
        }
    }
}

impl Render for GalleryView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .id("gallery-root")
            .size_full()
            .overflow_y_scroll()
            .elevation(Elevation::Background, theme)
            .text_color(theme.foreground)
            .p_sp(Sp::S16)
            .gap_sp(Sp::S24)
            .child(theme_row(theme))
            .child(colors_section(theme))
    }
}

/// Shared section shell: title + a11y seam + body. Every section goes through
/// this so the smoke test's seam contract cannot drift per-section.
fn section(
    theme: &ComponentTheme,
    seam: &'static str,
    title: &str,
    body: impl IntoElement,
) -> impl IntoElement {
    v_flex()
        .gap_sp(Sp::S8)
        .child(
            div()
                .text_role(TextRole::Display)
                .text_color(theme.foreground)
                .a11y_label(AccessRole::Label, seam)
                .child(title.to_string()),
        )
        .child(body)
}

/// One named color chip: the swatch itself plus its token name.
fn swatch(theme: &ComponentTheme, name: &str, color: gpui::Hsla) -> impl IntoElement {
    v_flex()
        .gap_sp(Sp::S2)
        .w(Sp::S32.pixels() * 4.0)
        .child(
            div()
                .h(Sp::S32.pixels())
                .w_full()
                .bg(color)
                .border_1()
                .border_color(theme.border)
                .rounded(theme.radius),
        )
        .child(
            div()
                .text_role(TextRole::Caption)
                .text_color(theme.muted_foreground)
                .child(name.to_string()),
        )
}

fn theme_row(theme: &ComponentTheme) -> impl IntoElement {
    section(
        theme,
        "gallery.theme",
        "Theme",
        h_flex()
            .gap_sp(Sp::S8)
            .child(
                Button::new("gallery-theme-dark")
                    .label("dark")
                    .primary()
                    .on_click(|_ev, _window, cx| Theme::switch(cx, "dark")),
            )
            .child(
                Button::new("gallery-theme-light")
                    .label("light")
                    .on_click(|_ev, _window, cx| Theme::switch(cx, "light")),
            )
            .child(
                Button::new("gallery-theme-hc")
                    .label("high-contrast")
                    .on_click(|_ev, _window, cx| Theme::switch(cx, "high-contrast")),
            ),
    )
}

fn colors_section(theme: &ComponentTheme) -> impl IntoElement {
    let d0 = theme.d0();
    // All 21 Dat0Colors fields — the derived layer A6 will consume.
    let dat0: Vec<(&str, gpui::Hsla)> = vec![
        ("focus_ring", d0.focus_ring),
        ("selection_tint", d0.selection_tint),
        ("fill_handle", d0.fill_handle),
        ("active_cell_tint", d0.active_cell_tint),
        ("marching_ants", d0.marching_ants),
        ("null_value_fg", d0.null_value_fg),
        ("banner_info", d0.banner_info),
        ("banner_warning", d0.banner_warning),
        ("banner_error", d0.banner_error),
        ("banner_tint", d0.banner_tint),
        ("hover_tint", d0.hover_tint),
        ("drag_over", d0.drag_over),
        ("pipeline_pill", d0.pipeline_pill),
        ("pipeline_accent", d0.pipeline_accent),
        ("pipeline_chip", d0.pipeline_chip),
        ("text_muted", d0.text_muted),
        ("text_error", d0.text_error),
        ("chart_placeholder_a", d0.chart_placeholder_a),
        ("chart_placeholder_b", d0.chart_placeholder_b),
        ("pager_dot_active", d0.pager_dot_active),
        ("pager_dot_inactive", d0.pager_dot_inactive),
    ];
    // The gpui-component families the A1 builtins define.
    let base: Vec<(&str, gpui::Hsla)> = vec![
        ("background", theme.background),
        ("foreground", theme.foreground),
        ("muted", theme.muted),
        ("muted_foreground", theme.muted_foreground),
        ("primary", theme.primary),
        ("primary_foreground", theme.primary_foreground),
        ("secondary", theme.secondary),
        ("danger", theme.danger),
        ("warning", theme.warning),
        ("success", theme.success),
        ("info", theme.info),
        ("ring", theme.ring),
        ("border", theme.border),
        ("popover", theme.popover),
        ("sidebar", theme.sidebar),
        ("list_hover", theme.list_hover),
        ("list_active", theme.list_active),
        ("drop_target", theme.drop_target),
    ];

    let grid = |title: &str, items: Vec<(&str, gpui::Hsla)>| {
        v_flex()
            .gap_sp(Sp::S4)
            .child(
                div()
                    .text_role(TextRole::Title)
                    .child(title.to_string()),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_sp(Sp::S8)
                    .children(items.into_iter().map(|(n, c)| swatch(theme, n, c))),
            )
    };

    section(
        theme,
        "gallery.colors",
        "Colors",
        v_flex()
            .gap_sp(Sp::S16)
            .child(grid("Dat0Colors (derived)", dat0))
            .child(grid("ThemeColor (gpui-component)", base)),
    )
}
```

If `use gpui::prelude::*` already brings `Context` into scope the explicit import
is unnecessary; if the compiler says `Context` is unresolved, add
`use gpui::Context;`.

- [ ] **Step 5: Run the smoke test — two of five seams pass, three fail**

```bash
cargo test -p dat0-app --test gallery_smoke
```

Expected: FAIL on `gallery section missing: gallery.scales` (T3 adds it). This is
the correct intermediate state. To keep the tree green at every commit, comment
out the three not-yet-implemented entries in `SECTIONS` **with a TODO naming the
task that restores them**:

```rust
const SECTIONS: &[&str] = &[
    "gallery.theme",
    "gallery.colors",
    // Restored by A4 T3: "gallery.scales", "gallery.elevation",
    // Restored by A4 T4: "gallery.components",
];
```

Re-run: PASS.

- [ ] **Step 6: Verify the gallery is literal-free and the crate is clean**

```bash
cargo test -p dat0-app --test style_lint
cargo fmt --all -- --check
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: style_lint PASS — `gallery.rs` must NOT appear in the failure list. If
it does, replace the offending color with a token; do not add it to `ALLOW`.

- [ ] **Step 7: Commit**

```bash
git add crates/dat0-app/Cargo.toml crates/dat0-app/src/lib.rs \
        crates/dat0-app/src/gallery.rs crates/dat0-app/tests/gallery_smoke.rs
git commit -s -m "$(cat <<'EOF'
feat(theme): A4 T2 — gallery scaffolding, theme row + colors section (UI redesign)

Dev-only token gallery behind a `gallery` feature that only the
self-dev-dependency enables, so it compiles for tests and examples but never
lands in a release build (same mechanism as a11y-capture).

Lib-module packaging is deliberate: an examples/*.rs body is unreachable from
any test, so a pure example would rot the first time A5/A6 renamed a token.
tests/gallery_smoke.rs mounts the view headlessly and asserts every section's
a11y seam.

This commit ships the shell, the theme-cycle row (dark/light/high-contrast via
the real Theme::switch facade) and the colors section: all 21 derived
Dat0Colors plus 18 gpui-component families. Zero literals — style_lint scans
this file at an allowance of 0.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Scales and elevation sections

**Files:**
- Modify: `crates/dat0-app/src/gallery.rs` (add two section fns + two `.child()` calls)
- Modify: `crates/dat0-app/tests/gallery_smoke.rs` (restore two `SECTIONS` entries)

**Interfaces:**
- Consumes from T2: `section(theme, seam, title, body)`, `swatch(...)`, the
  `Sp`/`TextRole`/`Elevation` imports, and `GalleryView::render`'s child chain.
- Produces: `fn scales_section(theme: &ComponentTheme) -> impl IntoElement` and
  `fn elevation_section(theme: &ComponentTheme) -> impl IntoElement`, mounted in
  `render` between `colors_section` and (later) `components_section`.

**Verified token facts — do not re-derive:**
- `Sp` variants: `S1 S2 S4 S6 S8 S12 S16 S24 S32`; `Sp::pixels() -> Pixels`.
- `TextRole` variants and sizes: `Caption` 11 · `Small` 12 · `Body` 13 ·
  `BodyLg` 14 · `Title` 16 (MEDIUM) · `Display` 20 (SEMIBOLD).
- `Density` variants `Compact | Default | Comfortable`;
  `Density::size() -> gpui_component::Size`;
  `Size::table_row_height() -> Pixels` gives 26 / 32 / 40 respectively.
- `Elevation` variants `Background Surface Raised Overlay Modal`;
  `.elevation(rung, theme)` applies bg + border + radius + shadow together;
  shadows are gated on `theme.shadow`, which the high-contrast builtin sets false.

- [ ] **Step 1: Restore the scales/elevation seams in the smoke test (red-first)**

In `crates/dat0-app/tests/gallery_smoke.rs`, uncomment those two entries:

```rust
const SECTIONS: &[&str] = &[
    "gallery.theme",
    "gallery.colors",
    "gallery.scales",
    "gallery.elevation",
    // Restored by A4 T4: "gallery.components",
];
```

- [ ] **Step 2: Run it and watch it fail**

```bash
cargo test -p dat0-app --test gallery_smoke
```

Expected: FAIL — `gallery section missing: gallery.scales`.

- [ ] **Step 3: Add both sections**

Append to `crates/dat0-app/src/gallery.rs` (and add
`use crate::theme::tokens::Density;` to the existing tokens import):

```rust
fn scales_section(theme: &ComponentTheme) -> impl IntoElement {
    let spacing = [
        ("S1", Sp::S1),
        ("S2", Sp::S2),
        ("S4", Sp::S4),
        ("S6", Sp::S6),
        ("S8", Sp::S8),
        ("S12", Sp::S12),
        ("S16", Sp::S16),
        ("S24", Sp::S24),
        ("S32", Sp::S32),
    ];
    let roles = [
        ("Caption", TextRole::Caption),
        ("Small", TextRole::Small),
        ("Body", TextRole::Body),
        ("BodyLg", TextRole::BodyLg),
        ("Title", TextRole::Title),
        ("Display", TextRole::Display),
    ];
    let densities = [
        ("Compact", Density::Compact),
        ("Default", Density::Default),
        ("Comfortable", Density::Comfortable),
    ];

    // Spacing: a bar whose WIDTH is the step, so the ratios are visible.
    let sp_rows = v_flex().gap_sp(Sp::S2).children(spacing.map(|(name, sp)| {
        h_flex()
            .gap_sp(Sp::S8)
            .items_center()
            .child(
                div()
                    .w(Sp::S32.pixels())
                    .text_role(TextRole::Caption)
                    .text_color(theme.muted_foreground)
                    .child(name),
            )
            .child(
                div()
                    .w(sp.pixels())
                    .h(Sp::S8.pixels())
                    .bg(theme.primary),
            )
    }));

    // Typography: each role rendered AS itself — size, weight and line-height
    // together, which is the whole reason TextRole carries all three.
    let type_rows = v_flex().gap_sp(Sp::S2).children(roles.map(|(name, role)| {
        div()
            .text_role(role)
            .child(format!("{name} — the quick brown fox jumps over the lazy dog"))
    }));

    // Density: three rows at their real table-row heights.
    let density_rows = v_flex().gap_sp(Sp::S4).children(densities.map(|(name, d)| {
        h_flex()
            .h(d.size().table_row_height())
            .items_center()
            .px_sp(Sp::S8)
            .gap_sp(Sp::S8)
            .bg(theme.list_hover)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_role(TextRole::Body)
                    .child(format!("{name} — {}px row", d.size().table_row_height().0)),
            )
    }));

    section(
        theme,
        "gallery.scales",
        "Scales",
        v_flex()
            .gap_sp(Sp::S16)
            .child(sub_title(theme, "Sp (spacing)"))
            .child(sp_rows)
            .child(sub_title(theme, "TextRole (typography)"))
            .child(type_rows)
            .child(sub_title(theme, "Density (row heights)"))
            .child(density_rows),
    )
}

fn elevation_section(theme: &ComponentTheme) -> impl IntoElement {
    let rungs = [
        ("Background", Elevation::Background),
        ("Surface", Elevation::Surface),
        ("Raised", Elevation::Raised),
        ("Overlay", Elevation::Overlay),
        ("Modal", Elevation::Modal),
    ];
    section(
        theme,
        "gallery.elevation",
        "Elevation",
        h_flex()
            .flex_wrap()
            .gap_sp(Sp::S16)
            .children(rungs.map(|(name, rung)| {
                v_flex()
                    .w(Sp::S32.pixels() * 5.0)
                    .h(Sp::S32.pixels() * 3.0)
                    .p_sp(Sp::S12)
                    .gap_sp(Sp::S4)
                    .elevation(rung, theme)
                    .child(div().text_role(TextRole::Title).child(name))
                    .child(
                        div()
                            .text_role(TextRole::Caption)
                            .text_color(theme.muted_foreground)
                            // HC sets shadow:false, so every rung reads flat
                            // there — that difference is the thing to look at.
                            .child(format!("{:?}", rung.resolve(theme).shadow)),
                    )
            })),
    )
}

/// Small heading inside a section body.
fn sub_title(theme: &ComponentTheme, text: &str) -> impl IntoElement {
    div()
        .text_role(TextRole::Title)
        .text_color(theme.foreground)
        .child(text.to_string())
}
```

Then mount both in `GalleryView::render`, after `.child(colors_section(theme))`:

```rust
            .child(scales_section(theme))
            .child(elevation_section(theme))
```

- [ ] **Step 4: Run the smoke test**

```bash
cargo test -p dat0-app --test gallery_smoke
```

Expected: PASS (four seams asserted).

- [ ] **Step 5: Verify literal-free + clean**

```bash
cargo test -p dat0-app --test style_lint
cargo fmt --all -- --check
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: all PASS, `gallery.rs` absent from any style_lint failure list.

- [ ] **Step 6: Commit**

```bash
git add crates/dat0-app/src/gallery.rs crates/dat0-app/tests/gallery_smoke.rs
git commit -s -m "$(cat <<'EOF'
feat(theme): A4 T3 — gallery scales + elevation sections (UI redesign)

Sp rendered as width-proportional bars, TextRole rendered as itself (size,
weight and line-height together — the reason the role carries all three),
Density as three rows at their real 26/32/40px table-row heights, and the
five Elevation rungs as cards showing bg + border + radius + shadow.

Each elevation card prints its resolved ShadowLevel, so switching to
high-contrast (shadow:false) visibly flattens all five at once.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Themed component samples section

**Files:**
- Modify: `crates/dat0-app/src/gallery.rs`
- Modify: `crates/dat0-app/tests/gallery_smoke.rs` (restore the last `SECTIONS` entry)

**Interfaces:**
- Consumes from T2/T3: `section`, `sub_title`, `GalleryView::sample_input`.
- Produces:
  `fn components_section(theme: &ComponentTheme, input: &Entity<InputState>) -> impl IntoElement`,
  mounted last in `render`.

**Verified API facts:**
- `use gpui_component::input::{Input, InputState};` — `Input::new(&entity)` is the
  render-side widget (`settings_ui/panel.rs:299`), `InputState` the state entity.
- `Button::new(id).label(text)` + `ButtonVariants` gives `.primary()`, `.ghost()`,
  `.danger()`. `Button::new` takes `impl Into<ElementId>`; a `&'static str`
  literal works directly (`window.rs:3846`).
- Do NOT build a real `Table` here: it needs a `TableDelegate` implementation, and
  the master plan pins `Table` + `GridTableDelegate` as byte-identical through
  workstream B. The "table stub" is a hand-rolled header + rows using
  `Density::Compact.size().table_row_height()`.

- [ ] **Step 1: Restore the last seam in the smoke test (red-first)**

```rust
const SECTIONS: &[&str] = &[
    "gallery.theme",
    "gallery.colors",
    "gallery.scales",
    "gallery.elevation",
    "gallery.components",
];
```

- [ ] **Step 2: Run it and watch it fail**

```bash
cargo test -p dat0-app --test gallery_smoke
```

Expected: FAIL — `gallery section missing: gallery.components`.

- [ ] **Step 3: Add the section**

Add `use gpui_component::input::Input;` to the existing input import, then append
to `crates/dat0-app/src/gallery.rs`:

```rust
fn components_section(
    theme: &ComponentTheme,
    input: &Entity<InputState>,
) -> impl IntoElement {
    let buttons = h_flex()
        .gap_sp(Sp::S8)
        .flex_wrap()
        .child(Button::new("gallery-btn-primary").label("Primary").primary())
        .child(Button::new("gallery-btn-secondary").label("Secondary"))
        .child(Button::new("gallery-btn-ghost").label("Ghost").ghost())
        .child(Button::new("gallery-btn-danger").label("Danger").danger());

    // Card at the Raised rung — the surface most dat0 panels will sit on.
    let card = v_flex()
        .w(Sp::S32.pixels() * 8.0)
        .p_sp(Sp::S12)
        .gap_sp(Sp::S4)
        .elevation(Elevation::Raised, theme)
        .child(div().text_role(TextRole::Title).child("Card"))
        .child(
            div()
                .text_role(TextRole::Body)
                .text_color(theme.muted_foreground)
                .child("Raised surface with body copy, as a panel would use it."),
        );

    // Hand-rolled table stub: real Table needs a TableDelegate, and the master
    // plan pins Table + GridTableDelegate byte-identical through workstream B.
    let row_h = crate::theme::tokens::grid_density().size().table_row_height();
    let header = h_flex()
        .h(row_h)
        .items_center()
        .px_sp(Sp::S8)
        .gap_sp(Sp::S16)
        .bg(theme.list_active)
        .child(div().text_role(TextRole::Caption).child("id"))
        .child(div().text_role(TextRole::Caption).child("name"))
        .child(div().text_role(TextRole::Caption).child("value"));
    let rows = v_flex().children((0..4).map(|i| {
        let mut row = h_flex()
            .h(row_h)
            .items_center()
            .px_sp(Sp::S8)
            .gap_sp(Sp::S16)
            .border_b_1()
            .border_color(theme.border)
            .child(div().text_role(TextRole::Body).child(format!("{i}")))
            .child(div().text_role(TextRole::Body).child(format!("row {i}")))
            .child(div().text_role(TextRole::Body).child(format!("{}", i * 7)));
        // Second row shows the selection tint over the table background —
        // the composited pair A3's contrast matrix gates.
        if i == 1 {
            row = row.bg(theme.d0().selection_tint);
        }
        row
    }));

    section(
        theme,
        "gallery.components",
        "Components",
        v_flex()
            .gap_sp(Sp::S16)
            .child(sub_title(theme, "Buttons"))
            .child(buttons)
            .child(sub_title(theme, "Input"))
            .child(div().w(Sp::S32.pixels() * 8.0).child(Input::new(input)))
            .child(sub_title(theme, "Card"))
            .child(card)
            .child(sub_title(theme, "Table (stub)"))
            .child(
                v_flex()
                    .w(Sp::S32.pixels() * 10.0)
                    .elevation(Elevation::Surface, theme)
                    .child(header)
                    .child(rows),
            ),
    )
}
```

Mount it last in `GalleryView::render`:

```rust
            .child(components_section(theme, &self.sample_input))
```

- [ ] **Step 4: Run the smoke test**

```bash
cargo test -p dat0-app --test gallery_smoke
```

Expected: PASS, all five seams. If `Input::new(&self.sample_input)` fights the
borrow checker against `cx.theme()`, bind `let theme = cx.theme().clone();` at the
top of `render` (`gpui_component::Theme` is `Clone`) and pass `&theme`.

- [ ] **Step 5: Verify literal-free + clean, and confirm no TODOs survive**

```bash
cargo test -p dat0-app --test style_lint
cargo fmt --all -- --check
cargo clippy -p dat0-app --all-targets -- -D warnings
grep -n "Restored by A4" crates/dat0-app/tests/gallery_smoke.rs
```

Expected: first three PASS; the `grep` must output NOTHING (every deferred-seam
TODO from T2/T3 is gone).

- [ ] **Step 6: Commit**

```bash
git add crates/dat0-app/src/gallery.rs crates/dat0-app/tests/gallery_smoke.rs
git commit -s -m "$(cat <<'EOF'
feat(theme): A4 T4 — gallery component samples (UI redesign)

Button variants, a live Input, a Raised card and a table stub at the grid's
Compact 26px row height, with one row carrying the composited selection tint
A3's contrast matrix gates. Token changes can now be judged on real chrome
rather than on swatches alone.

Table is hand-rolled on purpose: a real gpui-component Table needs a
TableDelegate, and the master plan pins Table + GridTableDelegate
byte-identical through workstream B.

All five gallery seams now assert in gallery_smoke.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: `examples/gallery.rs`

**Files:**
- Create: `crates/dat0-app/examples/gallery.rs`

**Interfaces:**
- Consumes: `dat0_app::gallery::GalleryView::new`, `dat0_app::theme::Theme::install_default`.
- Produces: nothing.

**Verified boot facts:**
- Production boots with `Application::new()` → `.run(|cx| { gpui_component::init(cx); … crate::theme::Theme::install_default(cx); … })`
  (`window.rs:1741`, `1782`, `1807`).
- The window root must be a `gpui_component::Root` (`window.rs` module doc).
- `settings_ui::open_settings_window` (`settings_ui/mod.rs:14-38`) is the
  small-window precedent: `Bounds::centered` + `WindowOptions` + `cx.open_window`.
- No `.with_assets(...)` in this slice — the asset source arrives at A5, which
  must also update this file.

- [ ] **Step 1: Write the example**

Create `crates/dat0-app/examples/gallery.rs`:

```rust
//! Boots the dat0 token gallery in a real window (UI redesign A4).
//!
//! `cargo run -p dat0-app --example gallery`
//!
//! Deliberately thin: everything renderable lives in `dat0_app::gallery` so the
//! headless `tests/gallery_smoke.rs` can mount it. An example body is
//! unreachable from any test — logic here would rot unseen.
//!
//! A5 note: when `Dat0Assets` lands, this must gain `.with_assets(...)` or the
//! icon section renders blank (silently — a missing AssetSource does not panic,
//! per the A0 spike).

use gpui::{AppContext as _, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;

use dat0_app::gallery::GalleryView;
use dat0_app::theme::Theme;

fn main() {
    Application::new().run(|cx| {
        // Required before any gpui-component widget is built.
        gpui_component::init(cx);
        // Applies the dark builtin so `cx.theme()` is the real A1 palette; the
        // in-gallery buttons switch from here via the same facade.
        Theme::install_default(cx);

        let bounds = Bounds::centered(None, size(px(1100.), px(860.)), cx);
        let _ = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("dat0 — token gallery".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| GalleryView::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        cx.activate(true);
    });
}
```

- [ ] **Step 2: Build it**

```bash
cargo build -p dat0-app --example gallery
```

Expected: compiles. If `dat0_app::gallery` is unresolved, the `gallery` feature is
not reaching example targets — re-check T2 Step 1's dev-dependency line.

- [ ] **Step 3: Boot it and look at it (manual, local only)**

```bash
cargo run -p dat0-app --example gallery
```

Expected, by eye: a scrolling page with five sections; the three theme buttons
restyle the whole window live; high-contrast flattens every elevation card (all
five print `None` for shadow) and stays legible. Note anything that looks wrong
in the PR description under "owed human glances" — do not fix palette values
here, that is A3 territory and would reopen the contrast matrix.

- [ ] **Step 4: Verify clippy covers the new target**

```bash
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo fmt --all -- --check
```

Expected: clean, and the clippy output must mention the `gallery` example target
(proof it is inside the lint's blast radius rather than silently skipped).

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/examples/gallery.rs
git commit -s -m "$(cat <<'EOF'
feat(theme): A4 T5 — runnable gallery example (UI redesign)

cargo run -p dat0-app --example gallery

~20-line main mirroring settings_ui::open_settings_window: gpui_component::init,
Theme::install_default, one centered window rooted at a gpui_component::Root.
No required-features declaration, so clippy --all-targets keeps compiling it
every run and it cannot rot.

Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Full gate, whole-branch review, PR

**Files:** none (verification only, plus the PR body).

- [ ] **Step 1: Full local gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app
cargo test --workspace
```

Expected: all green. The a11y/nav suites (`keyboard_nav`, `input_nav`,
`sql_console_nav`, `sql_console_transient_nav`, `cell_editor_nav`, `catalog_nav`,
`ai_nav`, `recents_nav`, `a11y_content`, `a11y_spike`) must be unchanged — this
slice edits no production render path, so any movement there is a real
regression, not an expected update.

- [ ] **Step 2: Confirm the release binary does not carry the gallery**

```bash
cargo build -p dat0-app --release --bin dat0 2>&1 | tail -3
nm -C target/release/dat0 2>/dev/null | grep -c "gallery::GalleryView" || true
```

Expected: build succeeds; the symbol count is `0` (dev-deps are absent from a
release build, so the `gallery` feature is off).

- [ ] **Step 3: Whole-branch review (opus)**

Review the complete diff against the design doc, not task-by-task. Specifically
check the cross-cutting failure classes that per-task reviews structurally cannot
see:
1. Does `style_lint` actually scan `src/gallery.rs`? (A section that quietly
   bypassed the gate would defeat the dogfood premise.)
2. Are all five seams asserted, with no `Restored by A4` TODO left behind?
3. Does the `ALLOW` table match a fresh scan exactly — no rounding, no file
   dropped?
4. Does anything in the slice touch a production render path?

- [ ] **Step 4: Push and open the PR**

```bash
git push -u origin feat/ui-redesign-a4-style-lint-gallery
gh pr create --title "A4 style-lint ratchet + token gallery (UI redesign)" --body "$(cat <<'EOF'
Slice A4 of the UI redesign: enforcement, not migration. No production render
path changes; no pixels move.

## 1. tests/style_lint.rs — the ratchet
Bans color construction across `crates/dat0-app/src`: `rgb(`, `rgba(`, `hsla(`,
`hsl(`, `white()`, `black()`, `parse_hex` regardless of argument, plus bare 6-
or 8-digit hex. Escape hatch `// style-lint: allow(reason)` with a mandatory
reason; an escape whose line holds no banned pattern fails as stale.

Per-file allowance is a **shrink-only ratchet** — failing when a count is too
high catches a new literal in an already-allowed file, and failing when it is
too low forces every A6 sub-slice to tighten the number it earned.

Starting state: **36 offending lines across 12 files.** The constructor-any-arg
rule found 5 sites a literal-only regex would have missed — the
`gpui::rgb(crate::a11y::FOCUS_RING)` call sites in `empty_state.rs`,
`catalog/panel.rs` (x2), `view/query_library.rs`, plus the const itself.

Supersedes the A2 in-module self-lint in `tokens.rs`, deleted here.

## 2. The token gallery
`src/gallery.rs` behind a `gallery` feature that only the self-dev-dependency
enables (same mechanism as `a11y-capture`) — compiled for tests and examples,
never in a release build. `examples/gallery.rs` is a ~20-line main;
`tests/gallery_smoke.rs` mounts the view headlessly and asserts all five section
seams. Lib-module packaging is what makes it testable: an example body is
unreachable from any test and would rot at the first A5/A6 token rename.

Sections: theme cycle (live dark/light/high-contrast switching), colors (21
derived `Dat0Colors` + 18 `ThemeColor` families), scales (`Sp`, `TextRole`
rendered as itself, `Density` at real row heights), elevation (5 rungs with
resolved shadow level), components (Button variants, Input, card, table stub
with the composited selection tint).

The gallery is itself scanned by style_lint at an allowance of 0.

## Verification
`cargo fmt` / `clippy --workspace --all-targets -D warnings` (now covering the
example) / full `dat0-app` suite including the untouched a11y + nav suites /
style_lint written red-first.

## Owed human glances
- The token set seen all at once, per theme — the first time that is possible.
- Elevation rungs: shadow feel in dark/light, flat-but-legible in HC.
- Button/Input chrome against the A1 palette.

Master plan: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §5 row A4

Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 5: Watch CI on both platforms**

```bash
gh pr checks --watch
```

Poll `gh pr checks`, not `gh run watch` (repo lesson). Watch the macOS job's disk
report (`grep DISK[` in the log) — this slice adds one extra debug link and the
macOS runner has historically ended jobs at ~4.8 G free.

- [ ] **Step 6: After merge — watch the post-merge main run**

The macOS grid-scroll bench is push-to-main-only and can redden main silently.
Confirm all 7 main-CI jobs green, the bench artifact exists, and crash-e2e
succeeded. Pass an explicit `--subject`/`--body` on the squash merge so no
commit-message marker leaks into the squash body.

---

## Self-review

**Spec coverage** (design §→task): §1a walk → T1 S1/S3 · §1b patterns → T1 S1 +
self-tests S1/S2 · §1c escape + stale → T1 S1 `escape_comment_suppresses_and_ratchets`
· §1d ratchet both directions → T1 S3/S6 · §1e failure output → T1 S3 · §2 gallery
module + feature + sections → T2/T3/T4 · §2 zero-literal dogfood → T2 S6, T3 S5,
T4 S5, T6 S3 · §3 example → T5 · §4 smoke test → T2 S2 + restored in T3 S1/T4 S1 ·
§5 verification → T6 S1 · §6 macOS disk risk → T6 S5 · §7 out-of-scope (no
migrations) → T1 S4 explicitly forbids "fixing" source to match the plan · §8 owed
human glances → T5 S3 + PR body. D7 → T1 S7.

**Corrections applied vs the design doc:** the design's allowlist said 31 lines /
10 files from a `rgb(0x`-shaped grep; a full simulation of the D2 pattern set
finds **36 lines / 12 files** (adds `empty_state.rs`, `view/query_library.rs`, two
more in `catalog/panel.rs`, and a second in `a11y/mod.rs`). The design doc is
corrected in the same commit as this plan.

**Type consistency:** `section(theme, seam, title, body)` and
`swatch(theme, name, color)` are defined in T2 and used unchanged in T3/T4;
`sub_title(theme, text)` is defined in T3 and used in T4;
`GalleryView::sample_input` is created in T2 and consumed in T4; seam strings are
a single `SECTIONS` list in the test and literal arguments to `section(...)` in
the module — cross-checked identical (`gallery.theme`, `gallery.colors`,
`gallery.scales`, `gallery.elevation`, `gallery.components`).
