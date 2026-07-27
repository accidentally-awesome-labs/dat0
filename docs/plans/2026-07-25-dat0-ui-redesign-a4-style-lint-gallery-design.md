# dat0 UI redesign — Slice A4: style-lint ratchet + token gallery (design)

Date: 2026-07-25
Master plan: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §5 (row A4)
Branch: `feat/ui-redesign-a4-style-lint-gallery` off main `dca3c9c` (A3)

## Goal

Two deliverables, both *enforcement* rather than *migration* — no production
pixels move in this slice:

1. **`tests/style_lint.rs`** — a repo-wide gate that bans inline color
   constructors in `crates/dat0-app/src/**`, with a per-line escape comment and
   a per-file **shrink-only count ratchet**. A1-A3 built the token system; A4 is
   what stops new code from bypassing it, and what forces A6a-g to actually
   retire the 36 remaining literal sites instead of leaving them.
2. **`src/gallery.rs` + `examples/gallery.rs` + `tests/gallery_smoke.rs`** — a
   runnable token gallery. It is the manual-UAT vehicle for every later slice:
   the accumulating "owed human glance" backlog (palette feel ×3, HC legibility,
   focus ring vs new ring, elevation/shadow feel, icons at A5, modal scrim at
   B1) is currently paid by booting the whole app per theme. The gallery makes
   all of it one window with a theme-cycle button.

Neither deliverable touches a production render path, so the a11y/nav suite is
invisible to this slice by construction (same property A2/A3 relied on).

## Decisions (owner, 2026-07-25)

| # | Decision | Chosen | Rejected |
|---|---|---|---|
| D1 | Allowlist shape | **Per-file max-count ratchet** (`&[(&str, usize)]`), fails both over *and* under | Binary per-file allowlist (a 10th literal in an allowed file would slip through); zero-tolerance-now (absorbs A6a-g, makes A4 an L, collides with window.rs/B-workstream) |
| D2 | Pattern strictness | **Constructor-any-arg + bare 6/8-digit hex** | Literal-arg-only (`rgb(0x`…) as written in the master plan — misses `a11y/mod.rs` `FOCUS_RING` and any const indirection |
| D3 | Gallery packaging | **Lib module behind a `gallery` feature + thin example + headless smoke test** | Pure `examples/gallery.rs` (untestable by construction, rots between slices); hidden screen in the prod bin (ships dev code, touches window.rs) |
| D4 | Gallery sections v1 | **All four**: theme cycle + colors, scales, elevation, themed component samples | — |
| D5 | Contrast readouts in gallery | **No** — A3's matrix owns the numbers in CI; the gallery's unique job is *feel* | Per-pair ratio labels (duplicates the gate, needs an `Hsla`→hex helper with one consumer) |
| D6 | Smoke-test depth | **Renders + all section labels captured** | No-panic-only (a deleted section wouldn't fail CI); per-swatch assertions (most churn at A5/A6 for little added protection) |
| D7 | A2's inline `tokens_module_stays_literal_free` | **Delete** — style_lint walks `tokens.rs` with an implicit 0 allowance and a strict superset of patterns | Keep both (duplicate enforcement, two places to update) |

## 1. `tests/style_lint.rs`

### 1a. Walk

Recursive `std::fs::read_dir` from `concat!(env!("CARGO_MANIFEST_DIR"), "/src")`,
collecting `*.rs`. No `walkdir` dependency. `regex` is already in
`[dependencies]` and is reachable from test targets (cargo passes normal deps
*and* dev-deps to a package's test targets), so no `Cargo.toml` dep change.

Scope is `src/` only — deliberately not `tests/`, which legitimately holds hex
strings (the A1/A3 theme gates parse `#rrggbb` fixtures) and not
`src/theme/builtins/*.json`, which is palette *data*, not code. Because the lint
binary itself lives in `tests/`, it can never match its own pattern table — the
concat-splitting trick A2 needed for its in-module self-lint is unnecessary
here.

### 1b. Banned patterns

Substring match, any argument:

```
rgb(   rgba(   hsla(   hsl(   white()   black()   parse_hex
```

Plus one regex for bare literals: `(?i)(^|[^0-9a-z_])0x[0-9a-f]{6}([0-9a-f]{2})?([^0-9a-f]|$)`
(6 or 8 hex digits, boundary-guarded on both sides with explicit character
classes rather than `\b`, since the `regex` crate has no lookaround support).

The 6-or-8-digit anchor is what makes the bare-hex rule affordable. Measured
against `src/` at `dca3c9c` it matches exactly one line — `a11y/mod.rs:30`
`pub const FOCUS_RING: u32 = 0x3b82f6;` — the site the master plan's
`rgb(0x`-only regex would have missed entirely. Together with the
constructor-any-arg rule it also catches that const's four consumers
(`a11y/mod.rs:65`, `empty_state.rs:462`, `catalog/panel.rs:140,173`,
`view/query_library.rs:61`), all written `gpui::rgb(crate::a11y::FOCUS_RING)` —
five real sites the literal-only sketch was blind to, and exactly the A6a work
item. Everything else in `src/` that spells
hex is 2- or 4-digit and never trips: PNG magic `0x89` (`charts/export.rs`),
SSE UTF-8 continuation bytes `0xE2`/`0x86` (`ai/sse.rs`), IPv6 prefixes
`0xfc00`/`0xfe80`/`0x2606` (`ai/ssrf.rs`), and the alpha-factor comments in
`theme/tokens.rs` (`0x22`, `0xaa`, `0x14`).

Note `charts/render.rs:20 area.fill(&WHITE)?` is a plotters **const**, not a
call, so `white()` does not match it. The master plan's "charts/render.rs stays
allowlisted until a chart-palette slice" is therefore moot under D2's pattern
set — no allowlist entry is needed, and a future chart-palette slice has one
less thing to unwind.

Glyph bans (`✕ ⌄ ⌃ ▾ ▸ ‹ ›` …) are **out of scope** — they arrive with A5, which
is the slice that replaces glyphs with Lucide icons. Magic-`px` spacing bans are
also out of scope (B10 territory; banning them now would be pure noise against a
codebase that hasn't migrated to `Sp`).

### 1c. Escape hatch

A line containing `// style-lint: allow(<reason>)` is exempt, where `<reason>`
must be non-empty. The escape ratchets in both directions: a line that carries
an `allow` but contains **no** banned pattern fails as a *stale escape*. Without
that check, escape comments outlive the code they excused and the allowlist
silently loosens.

No `src/` line needs an escape at `dca3c9c`; the mechanism exists for genuine
non-theme colors (a plotters palette, u32 bit-math) that later slices may add.

### 1d. Allowlist ratchet

```rust
/// Files that still hold pre-A6 inline colors, with their EXACT current count
/// of offending LINES (a line with two constructors counts once).
/// Shrink-only: lower the number in the same PR that removes the literals.
const ALLOW: &[(&str, usize)] = &[
    ("view/pipeline_bar.rs", 9),
    ("grid/mod.rs", 7),
    ("catalog/panel.rs", 4),
    ("error_ux/banner.rs", 4),
    ("a11y/mod.rs", 2),
    ("charts/mod.rs", 2),
    ("charts/panel.rs", 2),
    ("onboarding/mod.rs", 2),
    ("empty_state.rs", 1),
    ("settings_ui/panel.rs", 1),
    ("view/query_library.rs", 1),
    ("window.rs", 1),
];
```

Paths are relative to `src/`. Any file absent from the table has an implicit
allowance of 0. Total: **36 lines across 12 files**.

- `found > allowed` → fail: *regression*, a new literal entered an allowed file.
- `found < allowed` → fail: *stale ratchet*, with the message
  `lower ALLOW["view/pipeline_bar.rs"] to N`.

The under-check is the load-bearing half. Without it, A6c could migrate
`pipeline_bar.rs` to tokens, leave the entry at 9, and silently re-open the file
to nine fresh literals. Failing on "you're better than your number" is what
makes the gate monotonic across the seven A6 sub-slices.

Counts above are derived by grep at design time and are **re-derived by the lint
itself** during implementation (red-first: land the table as all-zeros, read the
failure list, then write the real numbers — the same discipline A3 used for the
contrast matrix).

### 1e. Failure output

For each violation: `src/<rel_path>:<line>: <pattern> — <trimmed source line>`,
then the per-file count summary. A red run must be actionable without
re-grepping, because the primary consumer of this failure is a future A6
sub-agent, not a human at a terminal.

## 2. `src/gallery.rs` (feature `gallery`)

`pub struct GalleryView;` + `impl Render`, one free fn per section. Section
order = theme cycle, colors, scales, elevation, components.

Feature wiring reuses the established self-dev-dependency trick — the same
mechanism that already turns `a11y-capture` on for this crate's own test
targets without a `ci.yml` change, and which keeps the code out of the shipped
binary:

```toml
[features]
gallery = []

[dev-dependencies]
dat0-app = { path = ".", features = ["a11y-capture", "gallery"] }
```

Sections:

| Section | a11y seam | Content |
|---|---|---|
| Theme cycle | `gallery.theme` | Three buttons → `Theme::switch(cx, "dark"/"light"/"high-contrast")`. The façade already calls `apply_config` + `refresh_windows`; no `Session` or `SettingsStore` needed (`theme/mod.rs:84`). |
| Colors | `gallery.colors` | All 21 `Dat0Colors` fields as named swatches, plus the core `ThemeColor` families: background, foreground, muted(+fg), primary(+fg), secondary, danger, warning, success, info, ring, border, popover, list_hover, list_active, drop_target. |
| Scales | `gallery.scales` | `Sp` 9-step bars; `TextRole` ladder rendered as real text (size + weight + line-height visible); `Density` row-height comparison (Compact 26 / Default 32 / Comfortable 40). |
| Elevation | `gallery.elevation` | 5 rungs as cards via `.elevation(rung, theme)`, showing bg + border + radius + shadow. The one place HC's `shadow: false` flattening is visible at a glance. |
| Components | `gallery.components` | Button variants, an `Input`, a card, a small table stub — token changes judged on real chrome, not just swatches. |

**Zero-literal, dogfooded.** The gallery paints exclusively from
`cx.theme()` / `cx.theme().d0()` / `Sp` / `TextRole` / `Elevation`, so
`style_lint` (which walks all of `src/`) covers it with an implicit 0 allowance.
If a section can't be built from tokens, that is a missing token — which is
exactly the signal the gallery exists to produce.

**No i18n keys.** Section headings and swatch names are English string literals.
The gallery is a dev tool that never ships; adding `gallery.*` keys would push
dev-tool strings into the translation surface. Nothing in the repo gates
hardcoded UI strings (`tests/i18n_p10c_keys.rs` asserts key *presence*, not
literal absence).

## 3. `examples/gallery.rs`

~20 lines: `Application::new().run(…)` → `gpui_component::init(cx)` →
`Theme::install_default(cx)` → `cx.open_window(…)` with
`Root::new(GalleryView)`. Structure mirrors `settings_ui::open_settings_window`
(`settings_ui/mod.rs:14-38`), the existing small-window precedent.

Run: `cargo run -p dat0-app --example gallery`.

No `[[example]] required-features` declaration: the dev-dep enables `gallery`
unconditionally for dev targets, and leaving the example undeclared keeps it
inside `cargo clippy --workspace --all-targets -- -D warnings` (ci.yml:47), so
it cannot rot silently.

## 4. `tests/gallery_smoke.rs`

`mod support;` (repo precedent is to copy the harness per test binary rather
than extract further), mount `GalleryView` under `VisualTestContext`, force one
frame, capture with `A11ySnapshot`, assert all five section labels are present.

This is the payoff for D3's lib-module packaging: an `examples/*.rs` body is
unreachable from any test, so a pure-example gallery would rot the first time
A5/A6 renamed a token. The smoke test fails loudly instead.

## 5. Verification

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings` (now covers the example)
- full `--features a11y-capture` suite (expected untouched — no production render path changes)
- `cargo test -p dat0-app --test style_lint` written **red-first**
- `cargo test -p dat0-app --test gallery_smoke`
- `cargo run -p dat0-app --example gallery` boots and cycles all three themes (local, manual)

## 6. Invariants & risks

| Risk | Mitigation |
|---|---|
| One extra debug link per CI run (the example) | Linux is covered by `CARGO_PROFILE_DEV_DEBUG: 0` (job-level, build-and-test); macOS ends jobs at ~4.8 G free — **watch the macOS job's disk report on the PR run** |
| Feature unification enables `gallery` for the `dat0` bin during `cargo test` | Harmless: dead code, no behavior, never in a release build (dev-deps are absent from `cargo build --release`) |
| Bare-hex rule false-positives on future non-color hex | `// style-lint: allow(reason)` escape, with the stale-escape check keeping it honest |
| Gallery drifts from tokens at A5/A6 | `gallery_smoke` section assertions + `clippy --all-targets` compiling the example every run |
| A5 needs the example updated for `.with_assets(Dat0Assets)` | Noted in A5's scope; icons section is A5's to add |

## 7. Out of scope

- Migrating any of the 36 literal sites (that is A6a-g, one file per sub-slice).
- Glyph bans (A5), magic-`px` bans (B10).
- Contrast readouts in the gallery (D5).
- i18n keys for gallery strings.
- Linting `tests/` or the builtin theme JSONs.

## 8. Owed human glances (append to the running list)

- Gallery itself: does the token set *look* like a system when seen all at once,
  in each of the three themes? (This is the first time that is possible.)
- Elevation rungs 1-5: shadow feel in dark/light, flat-but-legible in HC.
- Themed component samples: Button/Input chrome against the A1 palette.
