# Slice A6 — Surface migrations (UI redesign)

Date: 2026-07-28
Branch: `feat/ui-redesign-a6-surface-migrations` off main `1b8e5cb` (A5 icons)
Master plan: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §5 row A6
Predecessors: A1 theme unification `6d68c82` · A2 token scales `a2d7361` ·
A3 contrast matrix `dca3c9c` · A4 style-lint ratchet `819d068` · A5 icons `1b8e5cb`

---

## 0. What this slice is

A1–A5 built the token system and the gate that protects it, but changed almost
no pixels. A6 is the payoff: it points every remaining inline colour literal in
`crates/dat0-app/src/**` at the token that A2 already defined for it, and
lowers `tests/style_lint.rs`'s shrink-only `ALLOW` ratchet to match.

**A6 is the first A-slice with broadly visible output.** Banner hues leave
Tailwind for the theme's `info`/`warning`/`danger`; the focus ring stops being
the hardcoded `#3b82f6` and becomes theme-tracking `#58a6ff` (dark) / `#0969da`
(light), finally killing the two-blues split the original research flagged; and
every hover/selection/pager tint re-derives from the active palette. The
high-contrast theme gets correct colours on these surfaces for the first time.

It is **not** a redesign of any layout. No spacing, sizing, typography or
density change ships here — only colour sources.

### Verified starting inventory

`tests/style_lint.rs`'s `ALLOW` table, re-verified line-by-line against the tree
at `1b8e5cb`: **36 offending lines across 12 files**. The table is the work list.

## 1. The central fact: A2 already named every destination

`theme/tokens.rs` defines `Dat0Colors` with 21 fields whose names were chosen,
in A2, specifically for these call sites — A2's design doc §1 carries the
field → call-site map. A6 therefore **does not design new tokens**. It is
substitution against an existing map, with two documented corrections (§4).

Colours are derived on read (`cx.theme().d0().focus_ring`), so a theme switch
can never leave a migrated surface stale. Access idiom, already dogfooded by
`src/gallery.rs`: `use gpui_component::ActiveTheme as _;` then `cx.theme()`.

> Note: **none** of the 12 target files currently imports `ActiveTheme`. Every
> migrated file gains that import — a useful signal that the file really was
> outside the token system.

## 2. The one API change: `focus_stop` gains a ring parameter

`a11y::FocusStopExt::focus_stop` paints the focus ring from a module constant:

```rust
pub const FOCUS_RING: u32 = 0x3b82f6;
// …
.focus(|s| s.border_2().border_color(gpui::rgb(FOCUS_RING)))
```

The trait's default method has no `&App` in scope, so it cannot read the theme.
The signature gains `ring: Hsla` (closure stays last so call sites keep reading
naturally), the body becomes `.focus(move |s| s.border_2().border_color(ring))`,
and `pub const FOCUS_RING` is **deleted**.

```rust
fn focus_stop(
    self,
    id: &'static str,
    fh: &FocusHandle,
    tab_index: isize,
    ring: Hsla,
    on_activate: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
) -> Self
```

**Rejected: passing `cx: &App` instead.** `focus_stop` would then read the
global itself, which needs a `has_global` guard (A1's façade pattern) and
therefore a silent fallback colour when no theme is installed — exactly the
class of bug A1 removed. Passing a resolved `Hsla` keeps the value explicit and
testable, and pushes theme access to render functions that provably have it.

**Rejected: retargeting the constant.** One line, zero churn — but the ring
stays a literal, `a11y/mod.rs` never leaves the ratchet, and the ring stops
tracking theme switches (it would be wrong in high-contrast).

### Edit surface

**29** `.focus_stop(` call sites, not the master plan's "~15":

| file | sites |
|---|---|
| `view/sql_console.rs` | 18 |
| `empty_state.rs` | 6 |
| `view/name_prompt.rs` | 2 |
| `catalog/panel.rs`, `settings_ui/panel.rs`, `ai/panel.rs` | 1 each |

Plus **4** sites that paint an active-row ring from the same constant directly:
`catalog/panel.rs:140`, `catalog/panel.rs:173`, `empty_state.rs:462`,
`view/query_library.rs:62` → `cx.theme().d0().focus_ring`.

The ring hoists **once per containing render function** (`let ring =
cx.theme().d0().focus_ring;`), not once per call site — roughly 12–15 hoist
points. Several sites sit inside helpers that already take `cx`
(`ai/panel.rs::action_button`, `settings_ui::render_sidebar`), so those helpers
derive the ring locally and their own callers are untouched.

If any containing function turns out to have no theme access, thread `ring:
Hsla` down from its caller rather than reaching for a global — and report it,
per the A5 lesson that a plan's "stop and report if reality differs"
instruction earns its keep.

## 3. Sub-slice table

One PR, one commit per sub-slice. `ALLOW` entries that reach zero are
**removed** from the table (a file absent from `ALLOW` has an allowance of 0).

| T | Slice | Files touched | Tokens | `ALLOW` effect |
|---|---|---|---|---|
| T1 | **a** focus ring | `a11y/mod.rs`, `empty_state.rs`, `view/sql_console.rs`, `view/name_prompt.rs`, `view/query_library.rs`, `catalog/panel.rs`, `settings_ui/panel.rs`, `ai/panel.rs` | `focus_ring` | `a11y/mod.rs` 2→**gone**; `empty_state.rs` 1→**gone**; `view/query_library.rs` 1→**gone**; `catalog/panel.rs` 4→**2** |
| T2 | **b** banner | `error_ux/banner.rs` | `banner_info`, `banner_warning`, `banner_error`, `banner_tint` | 4→**gone** |
| T3 | **c** pipeline | `view/pipeline_bar.rs` | `text_muted`×3, `pipeline_accent`, `text_error`, `pipeline_pill`×3, `pipeline_chip` | 9→**gone** |
| T4 | **d** catalog + chevrons | `catalog/panel.rs`, `inspector/panel.rs` | `hover_tint`×2 | `catalog/panel.rs` 2→**gone** |
| T5 | **e** settings | `settings_ui/panel.rs` | `selection_tint` | 1→**gone** |
| T6 | **f** grid | `grid/mod.rs` | `selection_tint`, `marching_ants`, `focus_ring`, `active_cell_tint`, `null_value_fg`, `primary`/`primary_foreground` | 7→**gone** |
| T7 | **g** charts + onboarding | `charts/mod.rs`, `charts/panel.rs`, `onboarding/mod.rs` | `chart_placeholder_a`/`_b`, `text_error`, `text_muted`, `pager_dot_active`/`_inactive` | 2+2+2→**gone** |

**Exit state:** `const ALLOW: &[(&str, usize)] = &[("window.rs", 1)];`

T1 must land first: it re-baselines three files' counts before any later
sub-slice touches them. T2–T7 are mutually independent.

### Out of scope, deliberately

- **`window.rs:6836`** (`drag_over` external-paths tint) stays at 1. Master plan
  folds all `window.rs` styling into **B10**, so the file the DockArea
  workstream churns is touched once, not twice. A6 therefore exits with the
  colour ratchet at 1, not empty; B10 empties it.
- **Grid density** (`Table…with_size(XSmall)`, 26 px rows) — master plan bundles
  it into A6f. Deferred so that A6 is a pure colour substitution and any macOS
  bench movement has exactly one candidate cause. Density lands later with its
  own before/after measurement.
- **Spacing/`Sp` migration** of magic px values — B10.

## 4. Two documented deviations from A2's field map

### 4.1 The column-reorder ghost pill (`grid/mod.rs:73-74`)

A2's map sends `rgba(0x3b82f6aa)` to `fill_handle` and labels it "grid fill
handle". That label is wrong: the line is the **column-reorder drag ghost**
(`impl Render for ReorderDrag`), and there is no fill-handle render site in the
tree at all. The pill paints `gpui::white()` text on that fill, making it a
**text-on-fill pair**, whereas A3 tuned `fill_handle` to α 0.72 as a *non-text*
token against a 3:1 threshold. Migrating as mapped would leave the ghost
label's contrast ungated.

**Decision:** the ghost uses `cx.theme().primary` for the fill and
`cx.theme().primary_foreground` for the label. `tests/theme_contrast_gate.rs`
already carries `("primary.foreground", "primary.background", 4.5)` in
`TEXT_PAIRS`, gated across all three builtins — so this pair lands pre-proven,
with no new gate work and no new legibility glance.

`ReorderDrag::render` currently binds `_cx`; it is renamed to `cx`.

**Consequence:** `fill_handle` and `drag_over` both exit A6 with zero
production consumers (`drag_over` because `window.rs` defers to B10). Both stay
defined, tested and displayed in the gallery — the same precedent A5 set with
`Dat0IconName::{Play, Bookmark}`. Do not invent consumers for them.

### 4.2 Chevron accessible names (`catalog/panel.rs:121`, `inspector/panel.rs:105,153`)

A5 deferred these three glyphs into A6. Each interpolates its chevron into a
`format!` String that is **also** passed to `.a11y_label`, which put them
outside A5's scope rule ("a glyph converts iff it is its own element").

Splitting the glyph into an `Icon` changes the accessible name — `"▾ main (3)"`
becomes `"main (3)"`. This is an **improvement**: a screen reader should not
announce "▾". Verified safe: no test in `tests/` or `src/` asserts any of
`▾ ▸ ⌄ ⇅`, and chevron SVGs are already bundled upstream (A5 established that
`close`/`check`/all chevrons ship with `gpui-component-assets`), so no new
vendored asset and no `Dat0IconName` variant is needed.

> ⚠ **A5's hard-won rule applies here.** `a11y()` and `a11y_label()` both
> `push()` a **new node** into the capture tree — they do not set an attribute.
> All three sites **already carry `.a11y_label`**. T4 must **edit the existing
> label's text**, never add a second label, or these rows gain duplicate
> accessible names.

## 5. Grid paint-path risk (T6)

`GridTableDelegate::render_td` runs **per visible cell, per frame**, and today
touches the theme zero times. `d0()` constructs all 21 fields (~8 float
multiplies) per call. A6f is the first A-slice to add real work to the paint
path — A5 held the macOS grid-scroll bench only because `a11y_label` compiles
to an identity stub in release, leaving rendering byte-unchanged. That
protection does not apply here.

**Mitigation:** call `d0()` **lazily inside each styling branch**
(`is_selected`, copied-boundary, `is_active`, `is_null`) rather than
unconditionally at the top of `render_td`. An untinted cell — the common case —
pays nothing.

**Measurement:** T6 runs `cargo bench -p dat0-app --bench grid_scroll` before
and after its own change and records both numbers. Local macOS numbers are
noisy, so this is a smoke check, not the gate; the gate remains the
push-to-main-only CI bench, which must be watched post-merge (`grid/mod.rs` is
in the diff, as it was for A5).

## 6. Invariants and gates

**Structural expectations.** Colour swaps are invisible to the nav/a11y suites
(they assert labels and focus ids, not colours) — the same reasoning that held
for A2 and A3. The exception is T4, whose chevron work **does** move the capture
tree, so the full suite must run there and be read, not assumed.

**Per task:** `cargo fmt --all` → focused tests → commit with DCO `-s`.

**Controller gate** (`cargo test --workspace` is **not runnable on this
machine** — pre-existing macOS 27 / Xcode 26.6 libduckdb-sys Thrift breakage on
`main`, see the dev-workflow memory; CI still runs it on both platforms):

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p dat0-app`
- `cargo test -p dat0-app --features a11y-capture` (109 binaries)
- `cargo test -p dat0-app --features a11y-capture,gallery` (gallery stays at allowance 0)
- `cargo test -p dat0-app --test style_lint` — 4/4 against the new `ALLOW`
- glyph grep: `▾ ▸` gone from `catalog/panel.rs` and `inspector/panel.rs`

**Ratchet discipline.** The gate fails both ways — a count left too high
silently re-opens a migrated file. Every sub-slice edits `ALLOW` in the **same
commit** as its migration.

**Doc drift.** Deleting `FOCUS_RING` staleness-checks two comments in
`tests/style_lint.rs` (the module doc's "five real call sites where the literal
hides one `const` indirection away", and the `bare_hex_re` doc's claim that the
only thing it catches in `src/` is `FOCUS_RING`). The scanner's own unit-test
cases use those strings as **synthetic input** and stay valid; only the prose
needs updating.

**Post-merge:** watch the main run — all jobs, macOS grid-scroll bench held,
crash-e2e spawned. Squash-merge with an explicit `--subject`/`--body-file` so no
skip-ci marker leaks from any commit subject.

**macOS CI disk:** job-end headroom is 4.8 Gi after A5 (down from 5.3). A6 adds
**no new test binaries** — it edits existing files only — so headroom should
hold. Verify on the PR run's `DISK[` telemetry rather than assuming.

## 7. Owed human glances (grows substantially)

A6 changes visible colour on nearly every surface, in all three themes. The
headless harness draws no pixels (UAT Gap 1), so these are human-only:

- Focus ring `#3b82f6` → theme `ring` on all 29 `focus_stop` surfaces plus 4
  active-row rings — the two-blues fix, wanted a glance since A1.
- Banner info/warning/error hues (Tailwind → theme `info`/`warning`/`danger`).
- Pipeline bar: 3 muted labels, accent, error, 3 pills, chip.
- Catalog row hover + chevron icons; inspector chevron icons.
- Settings active-nav tint.
- Grid: selection tint, active-cell ring + fill, marching ants, NULL text,
  reorder ghost pill (new `primary` fill).
- Onboarding pager dots (active/inactive).
- **High contrast especially** — these surfaces were literal-coloured and so
  have never respected the HC palette.

Consolidate into one checklist with the standing backlog rather than glancing
per sub-slice.
