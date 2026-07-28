# dat0 UI redesign — Slice A5: icon system (design)

Date: 2026-07-27
Master plan: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §5 (row A5)
Branch: `feat/ui-redesign-a5-icons` off main `5b63d3e` (A4 + the macOS CI reclaim hotfix)

## Goal

dat0 renders every icon today as a Unicode glyph in a text node — `✕` for close,
`▾`/`▸` for disclosure, `⇅`/`↑`/`↓` for sort, `🕘` for history. Glyph coverage
varies by installed font, weight and metrics are uncontrollable, and none of it
participates in the token system A1-A3 built.

A5 registers a real `AssetSource`, adds a dat0 icon namespace on top of the 86
Lucide SVGs `gpui-component-assets` already bundles, and converts the
icon-buttons and directional affordances to `Icon`. It is the last Workstream-A
slice before the A6a-g surface migrations.

**No new keyboard reachability and no new focus stops.** A5 is a presentation
slice; the one behavioral improvement it does make is additive `a11y_label`s on
buttons that currently announce nothing.

## Verified API facts (pinned rev `0f0ab35`)

Checked against `~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/0f0ab35/`,
not from memory:

- **`Icon` inherits ambient text style.** `impl RenderOnce for Icon`
  (`crates/ui/src/icon.rs:319-330`) resolves color from
  `window.text_style().color` and, when no explicit size is set, applies
  `.size(text_size)` derived from `window.text_style().font_size`. Icons
  therefore follow A2's `TextRole` sizing and the active theme's text color with
  **zero plumbing and zero new color literals** — the A4 style_lint ratchet is
  untouched by this slice.
- **`IconNamed` is the extension point.** `pub trait IconNamed { fn path(self)
  -> SharedString; }` plus a blanket `impl<T: IconNamed> From<T> for Icon` means
  a dat0-side enum is a drop-in for `IconName` at every call site
  (`icon.rs:12-21`).
- **`Icon::path("icons/foo.svg")`** bypasses the enum entirely if ever needed.
- **`Sizable::with_size(Size)`** overrides the inherited size.
- **The bundled icons are Lucide.** `close.svg` carries
  `class="lucide lucide-x"`; gpui-component's README credits Lucide twice.
  86 SVGs ship in `gpui-component-assets` (v0.5.1, Apache-2.0 as a crate).
- **Lucide SVGs are stroke-based** (`stroke="currentColor"`, `fill="none"`).
  Rendering through gpui's SVG path is already proven live: the A0 spike
  confirmed Table header sort/filter icons render once assets are registered.
  No T0 rendering spike is needed.
- **`Application::with_assets` takes exactly one source**, so dat0's own icons
  and the upstream set must be served by a single `AssetSource` impl.
- **Only two `Application::new()` sites exist**: `window.rs:1741` (prod) and
  `examples/gallery.rs:22`. Tests drive `TestAppContext` and never construct an
  `Application`, so no test harness needs asset registration.

## Findings that correct the master plan

| # | Master plan said | Actually |
|---|---|---|
| F1 | "add 7 missing Lucide SVGs" | **5** are missing: `filter`, `play`, `layers`, `bookmark`, `clock` (history). `close`, `check`, all four `chevron-*`, `chevrons-up-down`, `sort-ascending`, `sort-descending` are already bundled. |
| F2 | Glyph list omits history | `🕘` at `view/sql_console.rs:1209` is the history-overlay trigger and needs `clock`. |
| F3 | "Where an a11y label WAS the glyph, switch label to i18n word … update the a11y suite in the same PR" | **Almost no glyph is an a11y label.** `focus_stop` registers identity from its `id` string via `record_focus_id` (`a11y/mod.rs:41-66`), not from visible text. Across all test files every glyph occurrence is in a **comment**; there are zero glyph assertions and no insta snapshot contains one. Exactly one site has a glyph as a label: `grid/cell_editor.rs:156` `.label("✕")`. **The nav/a11y suites are structurally invisible to this slice**, same as A2/A3. |
| F4 | Glyph map is complete | `view/filter_popover_entity.rs:109-124` holds a 14-glyph class the plan never mentions — relational operator prefixes (`≠ ≤ ≥ ↔ ⊇ ⊅ ↦ ↤ ∈ ⌗ ∅ ◉ ✓ ✗`) used as text in dropdown labels. Lucide has no equivalents. Out of scope (D1). |

## Decisions (owner, 2026-07-27)

| # | Decision | Chosen | Rejected |
|---|---|---|---|
| D1 | Conversion scope | **Icon-buttons + directional affordances only** — `✕`, `⌄⌃▾▸‹›`, `⇅↑↓`, `▣`, `📑`, `▶`, funnel, `🕘` | Bundled-only (defers the 5 vendored icons into A6 slices that would each re-open this question); everything incl. operator glyphs (no Lucide equivalents — would mean dropping semantic prefixes or drawing custom relational SVGs, and the operator dropdown carries existing test coverage) |
| D2 | a11y depth | **Icons + i18n `a11y_label`s, no new focus stops** | Icons only (a bare icon with no accessible name is a small regression vs a glyph that at least renders as text in the content tree); icons + labels + focus stops (changes `keyboard_nav` cycle counts and Tab order → a behavioral slice riding inside a presentation slice, and per-tab dynamic handles re-open the listbox-vs-per-row-handle problem from carve-out #1) |
| D3 | Attribution | **One hand-written `## Bundled assets` NOTICE.md section covering all Lucide icons dat0 ships, both sources**, + `LICENSE-lucide` beside the vendored SVGs | Attribute only dat0's own 5 (leaves the artwork-vs-crate distinction unaddressed for the 86 actually shipped); defer to pre-release ops (exactly the class of item that gets missed in a launch scramble, and far cheaper to write with context loaded) |
| D4 | AssetSource shape | **Own embed first, delegate to `gpui_component_assets::Assets`** | Vendor all 91 into dat0 (duplicates upstream, drifts on every rev bump); rust-embed `interpolate-folder-path` pointing into `~/.cargo` (fragile, breaks on a clean checkout) |
| D5 | Shadowing protection | **Hard test asserting the two filename sets are disjoint** | Comment only — a rev bump adding upstream `filter.svg` would silently diverge dat0's icon from everyone else's with no signal |
| D6 | Gallery | **Add a sixth "icons" section** rendering every `Dat0IconName` plus the bundled names dat0 uses, at three `TextRole` sizes | No gallery change (a blank-icon regression would have no cheap visual check, and A5's central failure mode is silent) |
| D7 | Sizing/color integration | **Inherit ambient text style; no explicit sizes** | An explicit dat0 `IconSize` scale (duplicates `TextRole`, and `Icon` already derives size from `font_size` for free) |

## 1. `crates/dat0-app/src/assets.rs`

```rust
#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
struct Dat0Embedded;

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
        // union of both embeds, deduped, dat0's entries first
    }
}
```

Own-first ordering is what makes D5 load-bearing: if upstream later ships
`icons/filter.svg`, dat0's copy silently wins and diverges. The disjoint-set
test converts that into a build failure at the rev bump.

`load` delegates rather than translating upstream's missing-asset `Err` into
`Ok(None)`. That preserves today's semantics exactly (A0 spike: a missing asset
is a silent no-render, never a panic).

New dependency: `gpui-component-assets` at the **same pinned rev** as
`gpui-component`, plus `rust-embed` for dat0's own folder.

## 2. `Dat0IconName`

```rust
pub enum Dat0IconName { Filter, Play, Layers, Bookmark, History }

impl IconNamed for Dat0IconName {
    fn path(self) -> SharedString {
        match self {
            Self::Filter   => "icons/filter.svg",
            Self::Play     => "icons/play.svg",
            Self::Layers   => "icons/layers.svg",
            Self::Bookmark => "icons/bookmark.svg",
            Self::History  => "icons/clock.svg",
        }
        .into()
    }
}
```

The blanket `From<T: IconNamed>` makes `Icon::new(Dat0IconName::Filter)` work
identically to `Icon::new(IconName::Close)`. Bundled icons use `IconName`
directly — no wrapper enum, no re-export layer.

## 3. Call-site migration

### The scope rule

A glyph converts **iff it is its own element** (`.child("✕")`). A glyph
embedded in a `format!`-produced `String` stays text. That boundary is
mechanical, reviewable, and keeps the diff presentational — converting an
in-string glyph would mean restructuring a `String` label into a flex row of
`Icon` + text, which changes layout and, in two cases, breaks unit tests that
assert the exact string.

Out of scope by this rule:

| Site | Glyph | Why it stays |
|---|---|---|
| `view/pipeline_bar.rs:14-18` | `↑` / `↓` | Sort-direction indicator built by `format!` into a chip label `String` |
| `ai/panel.rs:72-76` | `✓` / `✗` | `test_result_message()` returns a `String`; `test_result_message_formats` asserts `"✓ Connected"` verbatim |
| `view/filter_popover_entity.rs:109-124` | 14 relational glyphs | Operator label text, no Lucide equivalents (F4/D1) |

The one deliberate exception is `pipeline_bar.rs:222` `"▣ base"` — a mixed
glyph+text label on a genuine clickable. It converts, becoming a flex row of
`Icon::new(Dat0IconName::Layers)` + `"base"`.

### Per-site inventory

| File:line | Glyph | Icon | Kind |
|---|---|---|---|
| `window.rs:1529`, `window.rs:6491` | `✕` | `IconName::Close` | button |
| `view/sql_console.rs:696,738,990,1100` | `✕` | `IconName::Close` | button |
| `view/sql_console.rs:862` | `▾` | `IconName::ChevronDown` | disclosure |
| `view/sql_console.rs:1209` | `🕘` | `Dat0IconName::History` | button |
| `view/pipeline_bar.rs:91` | `▣` | `Dat0IconName::Layers` | affordance |
| `view/pipeline_bar.rs:125,241` | `›` | `IconName::ChevronRight` | separator |
| `view/pipeline_bar.rs:143` | `✕` | `IconName::Close` | button |
| `view/pipeline_bar.rs:185` | `⌃` | `IconName::ChevronUp` | button |
| `view/pipeline_bar.rs:222` | `▣ base` | `Dat0IconName::Layers` + text | button (restructure) |
| `view/pipeline_bar.rs:294` | `⌄` | `IconName::ChevronDown` | button |
| `grid/mod.rs:288` | `⇅` | `IconName::ChevronsUpDown` | sort zone |
| `grid/mod.rs:314` | `⌄` | `Dat0IconName::Filter` | funnel zone — the comment already calls it "the funnel-icon zone"; the glyph never matched the intent |
| `grid/cell_editor.rs:156` | `✕` | `IconName::Close` | button — **the one `.label()` site** |
| `catalog/panel.rs:121` | `▾` / `▸` | `ChevronDown` / `ChevronRight` | disclosure |
| `inspector/panel.rs:105,153` | `▸` / `▾` | `ChevronRight` / `ChevronDown` | disclosure |
| `empty_state.rs:17,18,128` | `›` / `▶` | `ChevronRight` / `Dat0IconName::Play` | button |
| `actions/view_actions.rs:142` | `✕` | `IconName::Close` | button |

Bundled names used: `close`, `chevron-down`, `chevron-up`, `chevron-right`,
`chevrons-up-down`. Vendored: all five.

### Mechanical shape

```rust
// before
div().id(("sql-tab-close", i))
    .child(SharedString::from("✕"))
    .on_click(…)

// after
div().id(("sql-tab-close", i))
    .a11y_label(AccessRole::Label, t!("sql.close_tab"))
    .child(Icon::new(IconName::Close))
    .on_click(…)
```

`view/filter_popover_entity.rs` is **not** touched: the funnel affordance lives
at `grid/mod.rs:314`, and the popover's own glyphs are all operator-label text.

**Invariants:** `focus_stop` ids unchanged · `keyboard_nav` cycle counts
unchanged · zero new tab stops · zero new color literals, so every style_lint
per-file count stays exactly where A4 left it.

## 4. i18n

New keys under existing namespaces (`common.close`, `common.filter`,
`sql.close_tab`, `sql.history`, …). `i18n-check` is a CI gate; keys are added
to every locale file in the same commit. No format-constructed keys — the
P9c-1 review caught that class once already.

## 5. NOTICE.md

A new `## Bundled assets` section **above** `<!-- BEGIN cargo-about generated -->`,
so `scripts/notice-extract.sh` and the drift check are unaffected. It records
that dat0 embeds Lucide icons from two places — 86 via `gpui-component-assets`
and 5 vendored under `crates/dat0-app/assets/icons/` — with the ISC text copied
from the authoritative Lucide source at write time, plus
`crates/dat0-app/assets/icons/LICENSE-lucide`.

Adding the `gpui-component-assets` dependency changes `Cargo.lock`, which
triggers the (warn-only, `continue-on-error`) NOTICE drift job; the generated
block is regenerated in the same commit.

## 6. Gallery

Sixth section in `src/gallery.rs`: the five `Dat0IconName` variants plus the
five bundled names dat0 uses (`close`, `chevron-down`, `chevron-up`,
`chevron-right`, `chevrons-up-down`), each at three `TextRole` sizes to make
size inheritance visible, under the existing live theme cycle. Vendored and
bundled icons render side by side, which is where a stroke-weight or
optical-size mismatch in a vendored file becomes obvious. `examples/gallery.rs` gains
`.with_assets(Dat0Assets)` — the A4 file already carries a comment saying it
must. `tests/gallery_smoke.rs` asserts the new section seam.

## 7. Tests — `crates/dat0-app/tests/icon_assets.rs`

1. Every `Dat0IconName::path()` resolves to non-empty bytes through `Dat0Assets`.
2. Every bundled name dat0 uses (`Close`, `ChevronDown`, `ChevronUp`,
   `ChevronRight`, `ChevronsUpDown`) resolves through the fallback.
3. **Disjoint-set assertion** (D5): dat0's embedded filenames ∩
   `gpui_component_assets::Assets` filenames = ∅.
4. A nonexistent path yields not-found rather than panicking.
5. Every resolved payload parses as SVG (starts with `<svg`), catching a
   truncated or mis-vendored file.

`gallery_smoke` gains one seam assertion. No other suite changes.

## 8. Non-goals

- Every glyph embedded in a `format!`-produced `String` — the relational
  operator labels, `pipeline_bar`'s `↑`/`↓` sort direction, and `ai/panel.rs`'s
  `✓`/`✗` status prefixes. See the scope rule in §3.
- Onboarding pager dots (`●`/`○`) and the grid `—` null placeholder — styled
  divs and text respectively, per the master plan.
- Tooltips. No `.tooltip()` helper exists at this rev (noted at
  `sql_console.rs:695`); tooltip polish stays a later task.
- Any change to keyboard reachability (D2).

## 9. Risks

| Risk | Mitigation |
|---|---|
| A missing or misnamed asset renders **blank and silent** — no panic (A0 spike) | Test 1 resolves every `Dat0IconName` path; test 5 validates payloads; the gallery icons section makes it visible at a glance. **The human glance is owed and non-optional.** |
| A future gpui-component rev adds one of dat0's 5 filenames upstream | D5 disjoint-set test fails the build at the bump |
| Vendored SVG differs visually from its Lucide upstream (wrong variant, wrong stroke width) | Vendor at 24×24 / stroke-width 2 to match the bundled set; gallery section shows all icons side by side, where an odd one out is obvious |
| style_lint per-file counts shift because a converted line merges or splits | Ratchet is shrink-only and fails both directions; `cargo test -p dat0-app --test style_lint` is part of the per-task gate |

## 10. Owed human glances (batch with the existing backlog)

- Icons render at all, in all three themes, in the gallery.
- Icon weight/size against adjacent text at each `TextRole` — the glyphs being
  replaced had different optical weight.
- The 5 vendored icons next to the 86 bundled ones: consistent stroke and
  optical size.
- Disclosure chevrons in catalog/inspector at `Density::Compact` (26px rows).
