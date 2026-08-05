# Slice B10 — cleanup + A6h `window.rs` styling (implementation plan)

> **For agentic workers:** steps use checkbox (`- [ ]`) syntax. This slice is
> executed **inline by the controller, no subagents**, per the B9 process.

**Goal:** Close the last colour literal in `src/`, unify dat0's two spacing
scales, fold the residual A6h chains, and leave the master plan telling the
truth as UI redesign v1 closes.

**Architecture:** Three independent changes plus documentation. (1) The
file-drop tint reads `cx.theme().d0().drag_over` instead of a hardcoded
`rgba`, with the two non-HC builtins retuned so the tint keeps its current
strength. (2) `Sp::pixels()` becomes `Sp::rems()`, making dat0's spacing token
scale rem-relative and therefore identical to the gpui helper scale that 196
sites already use. (3) The three `window.rs` element chains move onto `Sp`,
which is only zero-delta *because* of (2).

**Tech stack:** Rust, gpui 0.2.2, gpui-component pinned rev `0f0ab35`.

**Design doc:** `docs/plans/2026-08-05-dat0-ui-redesign-b10-cleanup-styling-design.md`

**Branch:** `feat/ui-redesign-b10-cleanup-styling` off main `136ef75`.

---

## Global Constraints

- Every commit: `cargo fmt --all` first, then `git commit -s` (DCO).
- **Never write the literal CI-skip marker in a commit message**, not even
  quoted in prose. Use `-F -` with a heredoc when a message contains backticks
  (zsh command-substitutes them inside `-m "…"`).
- Zero colour literals in `src/`: `tests/style_lint.rs` bans `rgb(`, `rgba(`,
  `hsl(`, `hsla(`, `white()`, `black()`, `parse_hex`, boundary-anchored colour
  fn names, `Hsla {` / `Rgba {`, and bare 6-or-8-digit hex.
- `cargo clippy --workspace --all-targets -D warnings` must exit 0. Note
  `clippy::items-after-test-module` — no item may follow a `#[cfg(test)] mod`
  in a file.
- `cargo test --workspace` and `cargo bench` are **unrunnable on this machine**
  (macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift). Use `-p dat0-app`; CI is
  the gate for the rest.
- dat0's rem size is **14px** (`gpui_component::Root::render` →
  `window.set_rem_size(cx.theme().font_size)`, root.rs:398; all three builtins
  set `"font.size": 14`). Every pixel figure in this plan is stated at rem 14.
- `src/grid/` must stay byte-identical (`git diff --stat main -- crates/dat0-app/src/grid`
  returns nothing).

---

## File Structure

| File | Change | Task |
| --- | --- | --- |
| `crates/dat0-app/src/theme/tokens.rs` | `Sp::rems()` added, then `pixels()` + `From<Sp> for Pixels` removed; `SpStyled` bodies; `fill_handle` doc; tests | T0, T2, T4 |
| `crates/dat0-app/src/window.rs` | drag tint → token (+2 imports); 3 element chains → `Sp` | T1, T3 |
| `crates/dat0-app/src/theme/builtins/dark.json` | `drop_target.background` α `1a` → `22` | T1 |
| `crates/dat0-app/src/theme/builtins/light.json` | `drop_target.background` α `1a` → `22` | T1 |
| `crates/dat0-app/tests/style_lint.rs` | `ALLOW` → `&[]` | T1 |
| `crates/dat0-app/src/view/status_bar.rs` | hairline → `px(1.)`, height → `Sp::S12.rems()` | T2 |
| `crates/dat0-app/src/gallery.rs` | 10 `.pixels()` sites → `px()` / `rems()` | T2 |
| `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` | §6 B10 row rewritten, B11 row added, §7 sequencing | T5 |
| `docs/plans/2026-08-05-…-b10-…-design.md` | §11 as-built appended | T5 |

No files are created. No new test binaries — the suite stays at **118**.

---

## Task 0: Hard gate — prove `Rems` flows before touching anything

**Files:**
- Modify: `crates/dat0-app/src/theme/tokens.rs` (additive only)

**Interfaces:**
- Produces: `Sp::rems(self) -> Rems`, `impl From<Sp> for Rems`. T2 makes these
  the only accessor; T3 depends on T2.

**Why a gate:** the design argues from gpui's `From<Rems>` impls that
`SpStyled`'s bodies compile unchanged. Argued is not compiled. This task proves
it additively, so a failure costs nothing.

**STOP clause:** if Step 5 fails to compile, the unification is off. Ship T1,
T4 and T5 only; drop T2 and T3; record §3 of the design doc as a measured
defect owed its own slice. Do **not** attempt T3 at +14%.

- [ ] **Step 1: Add `Rems`/`rems` to the imports**

`crates/dat0-app/src/theme/tokens.rs:10` — replace:

```rust
use gpui::{FontWeight, Hsla, Pixels, Styled, px, relative};
```

with:

```rust
use gpui::{FontWeight, Hsla, Pixels, Rems, Styled, px, relative, rems};
```

`Pixels` and `px` stay — `TextRole::size()` (`:143`) and `Elevation`'s radius
(`:230`, `:236`) still return absolute pixels, and that is correct: type sizes
and corner radii are not spacing.

- [ ] **Step 2: Add `rems()` beside the existing `pixels()`**

`crates/dat0-app/src/theme/tokens.rs:96-105` — replace:

```rust
impl Sp {
    pub fn pixels(self) -> Pixels {
        px(self as u16 as f32)
    }
}

impl From<Sp> for Pixels {
    fn from(sp: Sp) -> Pixels {
        sp.pixels()
    }
}
```

with:

```rust
impl Sp {
    pub fn pixels(self) -> Pixels {
        px(self as u16 as f32)
    }

    /// The scale as a rem-relative length, against the CSS-conventional 16px
    /// rem the gpui helper scale is defined in (`gpui-macros/src/styles.rs`
    /// emits `.gap_1()` as `rems(0.25)`, documented "4px (0.25rem)").
    ///
    /// This — not [`Sp::pixels`] — is the spacing unit, because dat0's rem is
    /// **14px**, not 16: `gpui_component::Root::render` calls
    /// `window.set_rem_size(cx.theme().font_size)` and A1 set `"font.size": 14`
    /// in all three builtins. An absolute `Sp` therefore sat 14% looser than
    /// every gpui-spaced element beside it. Expressed this way `Sp::S4` **is**
    /// `.gap_1()`, exactly.
    pub fn rems(self) -> Rems {
        rems(self as u16 as f32 / 16.)
    }
}

impl From<Sp> for Pixels {
    fn from(sp: Sp) -> Pixels {
        sp.pixels()
    }
}

impl From<Sp> for Rems {
    fn from(sp: Sp) -> Rems {
        sp.rems()
    }
}
```

- [ ] **Step 3: Write the three gate tests**

Append inside the existing `#[cfg(test)] mod tests` in
`crates/dat0-app/src/theme/tokens.rs`, immediately after `sp_scale_exact_values`
(which ends at `:384`):

```rust
    /// `Sp` and gpui's helper scale are the SAME scale, so an `Sp`-spaced
    /// container and a `.gap_1()`-spaced sibling agree. Without this, the two
    /// silently re-fork the next time `font.size` moves.
    #[test]
    fn sp_rems_matches_gpui_helper_scale() {
        // gpui-macros/src/styles.rs: .gap_1() == rems(0.25), .gap_2() ==
        // rems(0.5), .px_3() == rems(0.75).
        assert_eq!(Sp::S4.rems(), rems(0.25));
        assert_eq!(Sp::S8.rems(), rems(0.5));
        assert_eq!(Sp::S12.rems(), rems(0.75));
        assert_eq!(Sp::S16.rems(), rems(1.0));
        assert_eq!(Sp::S32.rems(), rems(2.0));
    }

    /// What a user actually sees, at dat0's real rem size. The assertion above
    /// only proves two constants match; this one states pixels.
    #[test]
    fn sp_rems_resolve_at_dat0_rem_size() {
        // gpui_component::Root::render sets rem_size from theme.font_size,
        // and A1 pinned "font.size": 14 in all three builtins.
        let rem = px(14.);
        assert_eq!(Sp::S1.rems().to_pixels(rem), px(0.875));
        assert_eq!(Sp::S4.rems().to_pixels(rem), px(3.5));
        assert_eq!(Sp::S8.rems().to_pixels(rem), px(7.));
        assert_eq!(Sp::S12.rems().to_pixels(rem), px(10.5));
        assert_eq!(Sp::S32.rems().to_pixels(rem), px(28.));
    }

    /// THE GATE: `Rems` must flow into every setter `SpStyled` calls —
    /// padding/gap take `impl Into<DefiniteLength>`, margin takes `Length`.
    /// gpui provides `From<Rems>` for both, but compiled beats argued.
    #[test]
    fn rems_flows_through_every_styled_setter() {
        let mut el = gpui::div()
            .p(Sp::S8.rems())
            .px(Sp::S8.rems())
            .py(Sp::S4.rems())
            .gap(Sp::S4.rems())
            .m(Sp::S2.rems());
        let style = el.style();
        assert_eq!(style.padding.top, Some(Sp::S8.rems().into()));
        assert_eq!(style.gap.width, Some(Sp::S4.rems().into()));
        assert_eq!(style.margin.top, Some(Sp::S2.rems().into()));
    }
```

- [ ] **Step 4: Prove the gate is non-vacuous before trusting it**

Temporarily change `rems()`'s body to `rems(self as u16 as f32 / 8.)` and run:

```bash
cargo test -p dat0-app --lib theme::tokens::tests::sp_rems -- --nocapture
```

Expected: `sp_rems_matches_gpui_helper_scale` and
`sp_rems_resolve_at_dat0_rem_size` both **FAIL**. Revert the body to `/ 16.`.

⚠ After reverting, `touch crates/dat0-app/src/theme/tokens.rs` before re-running
— A6 lost half an hour to cargo reusing a stale binary because a reverted file
was backwards-dated.

- [ ] **Step 5: Run the gate**

```bash
touch crates/dat0-app/src/theme/tokens.rs
cargo test -p dat0-app --lib theme::tokens -- --nocapture
```

Expected: PASS, including `rems_flows_through_every_styled_setter`.

**If this step fails to compile, invoke the STOP clause above.**

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/theme/tokens.rs
git commit -s -F - <<'EOF'
test(theme): B10 T0 — gate that Rems flows through SpStyled

Additive. Adds Sp::rems() alongside the existing Sp::pixels() and three
tests: Sp equals gpui's helper scale (Sp::S4 == rems(0.25) == .gap_1()),
the resolved pixel values at dat0's real 14px rem size, and — the gate —
that Rems compiles into every setter SpStyled calls.

Proves the premise T2 depends on before T2 touches any call site. The
scale assertions were verified non-vacuous by perturbing the divisor.
EOF
```

---

## Task 1: Drag tint → token, ratchet → empty

**Files:**
- Modify: `crates/dat0-app/src/window.rs:8014` and its import block
- Modify: `crates/dat0-app/src/theme/builtins/dark.json:105`
- Modify: `crates/dat0-app/src/theme/builtins/light.json:105`
- Modify: `crates/dat0-app/tests/style_lint.rs:49`
- Test: `crates/dat0-app/tests/style_lint.rs`, `crates/dat0-app/tests/theme_contrast_gate.rs`

**Interfaces:**
- Consumes: `Dat0Colors::drag_over` (exists since A2, `tokens.rs:31`, derived as
  `self.drop_target` at `:67`).
- Produces: nothing. Independent of T0/T2/T3.

- [ ] **Step 1: Confirm the ratchet is red-first in the direction that matters**

Before editing, confirm the gate currently passes at 1 and would fail at 0:

```bash
cargo test -p dat0-app --test style_lint
```

Expected: PASS (4 tests). This is the baseline the edit must preserve while
`ALLOW` drops to `&[]`.

- [ ] **Step 2: Retune the two non-HC builtins**

`crates/dat0-app/src/theme/builtins/dark.json:105`:

```diff
-    "drop_target.background": "#58a6ff1a",
+    "drop_target.background": "#58a6ff22",
```

`crates/dat0-app/src/theme/builtins/light.json:105`:

```diff
-    "drop_target.background": "#0969da1a",
+    "drop_target.background": "#0969da22",
```

`high-contrast.json:105` stays `"#ffff0033"` — its α 0.20 is already stronger
than the literal's 0.13 and A3 tuned it for the HC palette.

α `0x22` = 34/255 = 0.133, matching the literal `0x0088_ff22` being replaced.
The **hue** still moves to the theme accent; that is the point of the
substitution and is not something the retune preserves.

- [ ] **Step 3: Run the contrast gate**

```bash
cargo test -p dat0-app --test theme_contrast_gate -- --nocapture
```

Expected: PASS. `composited_tints_keep_text_readable` prints one line per
theme; confirm the `drop_target.background∘background` figures read
approximately:

| theme | expected |
| --- | --- |
| dark | ~10.06:1 |
| light | ~13.07:1 |
| high contrast | ~13.01:1 (unchanged) |

All against a 4.5 floor. If any is below 4.5, revert the retune and use the
token at its shipped α instead (`#…1a`) — the substitution matters, the retune
does not.

- [ ] **Step 4: Add the two trait imports `window.rs` has never needed**

`window.rs` currently performs **zero** theme reads — its only `cx.theme()`
occurrence is inside a comment at `:7309`. The tint is its first.

`crates/dat0-app/src/window.rs`, after the existing `use gpui_component::Root;`
line (`:42`):

```rust
use gpui_component::ActiveTheme as _;
```

and after `use crate::grid::{GridDataSource, GridTableDelegate};` (`:53`):

```rust
use crate::theme::tokens::Dat0Theme as _;
```

- [ ] **Step 5: Substitute the literal**

`crates/dat0-app/src/window.rs:8014`:

```diff
-            .drag_over::<ExternalPaths>(|style, _, _, _| style.bg(gpui::rgba(0x0088_ff22)))
+            .drag_over::<ExternalPaths>(|style, _, _, cx| style.bg(cx.theme().d0().drag_over))
```

The closure's fourth parameter is `&mut App`
(`gpui-0.2.2/src/elements/div.rs:940`), so the tint is read from the live theme
each time the closure runs — a theme switch mid-drag is handled for free, and
nothing needs capturing.

- [ ] **Step 6: Empty the ratchet**

`crates/dat0-app/tests/style_lint.rs:44-49` — replace the doc comment and the
constant:

```rust
/// Files that still hold pre-A6 inline colors, with their EXACT current count of
/// offending LINES (a line with two constructors counts once).
///
/// SHRINK-ONLY RATCHET. Each A6 sub-slice that migrates a file lowers its number
/// in the same PR; the gate fails if a count is left too high *or* too low.
/// A file absent from this table has an allowance of 0.
const ALLOW: &[(&str, usize)] = &[("window.rs", 1)];
```

with:

```rust
/// Files that still hold pre-A6 inline colors, with their EXACT current count of
/// offending LINES (a line with two constructors counts once).
///
/// SHRINK-ONLY RATCHET. Each A6 sub-slice that migrates a file lowers its number
/// in the same PR; the gate fails if a count is left too high *or* too low.
/// A file absent from this table has an allowance of 0.
///
/// **EMPTY since B10.** The last entry was `window.rs`'s file-drop tint, which
/// now reads `cx.theme().d0().drag_over`. `src/` holds no colour literal and no
/// `// style-lint: allow(…)` escape at all, so this gate has stopped describing
/// debt and become a pure regression guard. Re-adding an entry means a slice
/// introduced a literal it could not tokenise — that is a design question, not
/// a bookkeeping one.
const ALLOW: &[(&str, usize)] = &[];
```

- [ ] **Step 7: Run both gates plus the build**

```bash
cargo test -p dat0-app --test style_lint
cargo test -p dat0-app --test theme_contrast_gate
cargo clippy -p dat0-app --all-targets -- -D warnings
```

Expected: style_lint PASS (4 tests, now at allowance 0 everywhere),
contrast gate PASS, clippy exit 0.

- [ ] **Step 8: Prove the empty ratchet is non-vacuous**

Temporarily add a literal to a `src/` file that has none — append to
`crates/dat0-app/src/theme/tokens.rs` a line inside any function body:

```rust
    let _probe = gpui::rgba(0x0088_ff22);
```

Run `cargo test -p dat0-app --test style_lint`. Expected: **FAIL**, naming
`theme/tokens.rs: 1 color-literal lines, allowance 0 — 1 new.` Remove the
probe, `touch` the file, re-run, expect PASS.

- [ ] **Step 9: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs crates/dat0-app/src/theme/builtins/dark.json \
        crates/dat0-app/src/theme/builtins/light.json crates/dat0-app/tests/style_lint.rs
git commit -s -F - <<'EOF'
feat(theme): B10 T1 — file-drop tint reads the theme; ratchet empty

The last colour literal in src/ was window.rs:8014, the .drag_over tint,
hardcoded #0088ff at alpha 0.13. It now reads cx.theme().d0().drag_over
from the closure's &mut App, so it follows the live theme.

The B9 handoff recorded drag_over as SOLID, which is wrong: all three
builtins define drop_target.background as 8-digit hex carrying alpha
(dark #58a6ff1a, light #0969da1a, HC #ffff0033). Dark and light are
retuned to alpha 22 so the tint keeps today's strength; the hue moves to
the theme accent, which the retune does not and cannot preserve.

High contrast changes most, and it is a fix: the tint was painting a
hardcoded blue that ignored the HC theme entirely. It is now yellow.

composited_tints_keep_text_readable already gated this token; recomputed
before the edit and confirmed after — dark 10.06:1, light 13.07:1
against a 4.5 floor.

ALLOW is now empty. src/ holds no colour literal and no lint escape.
Verified non-vacuous by planting a literal in a clean file.
EOF
```

---

## Task 2: Unify the spacing scale

**Files:**
- Modify: `crates/dat0-app/src/theme/tokens.rs` (`pixels()` removal, `SpStyled`, two tests)
- Modify: `crates/dat0-app/src/view/status_bar.rs:168`
- Modify: `crates/dat0-app/src/gallery.rs` (10 sites)

**Interfaces:**
- Consumes: `Sp::rems()` and `impl From<Sp> for Rems` from T0.
- Produces: `Sp` with `rems()` as its **only** accessor. T3 depends on this;
  without it T3 is a +14% spacing change rather than a no-op.

**What moves, and by how much.** 26 production `Sp::` call sites re-space −14%:
`overlay.rs` 2, `view/command_palette.rs` 10, `view/status_bar.rs` 6,
`view/saved_query_picker.rs` 7, `view/pipeline_bar.rs` 1. They shrink to match
the 196 rem-relative sites around them. No call site in those five files needs
editing — they all go through `SpStyled`.

- [ ] **Step 1: Delete `pixels()` and its `From` impl**

`crates/dat0-app/src/theme/tokens.rs` — the block T0 produced becomes:

```rust
impl Sp {
    /// The scale as a rem-relative length, against the CSS-conventional 16px
    /// rem the gpui helper scale is defined in (`gpui-macros/src/styles.rs`
    /// emits `.gap_1()` as `rems(0.25)`, documented "4px (0.25rem)").
    ///
    /// **The only accessor, deliberately.** dat0's rem is **14px**, not 16:
    /// `gpui_component::Root::render` calls
    /// `window.set_rem_size(cx.theme().font_size)` and A1 set `"font.size": 14`
    /// in all three builtins. An absolute `Sp` therefore sat 14% looser than
    /// every gpui-spaced element beside it, and the codebase runs 196 gpui
    /// helper sites against 26 `Sp` sites. Expressed this way `Sp::S4` **is**
    /// `.gap_1()`, exactly, and the two scales are one.
    ///
    /// `Sp` still earns its keep: it is a restricted, named 9-step subset of
    /// gpui's open scale, and it survives a future `font.size` change without
    /// re-forking. `sp_rems_matches_gpui_helper_scale` is what holds that.
    ///
    /// Absolute lengths — hairline rules, fixed panel widths, type sizes,
    /// corner radii — are not spacing and must not come from here. Use
    /// `gpui::px` directly and say so at the call site.
    pub fn rems(self) -> Rems {
        rems(self as u16 as f32 / 16.)
    }
}

impl From<Sp> for Rems {
    fn from(sp: Sp) -> Rems {
        sp.rems()
    }
}
```

- [ ] **Step 2: Point `SpStyled` at `rems()`**

`crates/dat0-app/src/theme/tokens.rs:108-126` — replace the five bodies:

```rust
/// Spacing helpers so call sites stay terse: `.p_sp(Sp::S8)`.
pub trait SpStyled: Styled + Sized {
    fn p_sp(self, sp: Sp) -> Self {
        self.p(sp.rems())
    }
    fn px_sp(self, sp: Sp) -> Self {
        self.px(sp.rems())
    }
    fn py_sp(self, sp: Sp) -> Self {
        self.py(sp.rems())
    }
    fn gap_sp(self, sp: Sp) -> Self {
        self.gap(sp.rems())
    }
    fn m_sp(self, sp: Sp) -> Self {
        self.m(sp.rems())
    }
}
```

- [ ] **Step 3: Rewrite the scale test in the new unit**

`crates/dat0-app/src/theme/tokens.rs:368-384` — replace `sp_scale_exact_values`:

```rust
    #[test]
    fn sp_scale_exact_values() {
        // The scale's identity is its step values; the unit is rems, against a
        // 16px reference rem (see `Sp::rems`). Resolved pixels at dat0's real
        // 14px rem live in `sp_rems_resolve_at_dat0_rem_size`.
        let expect = [
            (Sp::S1, 1.),
            (Sp::S2, 2.),
            (Sp::S4, 4.),
            (Sp::S6, 6.),
            (Sp::S8, 8.),
            (Sp::S12, 12.),
            (Sp::S16, 16.),
            (Sp::S24, 24.),
            (Sp::S32, 32.),
        ];
        for (sp, v) in expect {
            assert_eq!(sp.rems(), rems(v / 16.), "{sp:?}");
            assert_eq!(Rems::from(sp), rems(v / 16.));
        }
    }
```

- [ ] **Step 4: Update the composition test's three expectations**

`crates/dat0-app/src/theme/tokens.rs:505-509`:

```diff
         // SpStyled: padding + gap land with the scale value.
         let mut el = gpui::div().p_sp(Sp::S8).gap_sp(Sp::S4);
         let style = el.style();
-        assert_eq!(style.padding.top, Some(Sp::S8.pixels().into()));
-        assert_eq!(style.padding.left, Some(Sp::S8.pixels().into()));
-        assert_eq!(style.gap.width, Some(Sp::S4.pixels().into()));
+        assert_eq!(style.padding.top, Some(Sp::S8.rems().into()));
+        assert_eq!(style.padding.left, Some(Sp::S8.rems().into()));
+        assert_eq!(style.gap.width, Some(Sp::S4.rems().into()));
```

- [ ] **Step 5: Fix the status-bar hairline**

`crates/dat0-app/src/view/status_bar.rs:167-169`:

```diff
                 .children(
-                    (i < last).then(|| div().w(Sp::S1.pixels()).h(Sp::S12.pixels()).bg(border)),
+                    // A 1px hairline is not spacing: a rem-relative S1 would be
+                    // 0.875px at dat0's 14px rem — a sub-pixel rule. The height
+                    // is decorative and stays on the scale.
+                    (i < last).then(|| div().w(gpui::px(1.)).h(Sp::S12.rems()).bg(border)),
                 )
```

The separator's height moves 12px → 10.5px with the rest of the −14%.

- [ ] **Step 6: Fix the gallery's ten width sites**

`Rems` implements `Mul<Pixels>`, **not** `Mul<f32>` (`geometry.rs:3121`), so
`Sp::S32.pixels() * 4.0` cannot survive the swap. These are fixed demo widths,
not spacing — A4 already parked "no width scale" as a ruling.

`crates/dat0-app/src/gallery.rs`, nine sites become absolute:

```diff
@@ fn swatch
-        .w(Sp::S32.pixels() * 4.0)
+        .w(gpui::px(128.))
@@
-                .h(Sp::S32.pixels())
+                .h(gpui::px(32.))
@@ scale rows (label column)
-                    .w(Sp::S32.pixels())
+                    .w(gpui::px(32.))
@@ elevation rungs
-                    .w(Sp::S32.pixels() * 5.0)
-                    .h(Sp::S32.pixels() * 3.0)
+                    .w(gpui::px(160.))
+                    .h(gpui::px(96.))
@@ card
-        .w(Sp::S32.pixels() * 8.0)
+        .w(gpui::px(256.))
@@ input
-            .child(div().w(Sp::S32.pixels() * 8.0).child(Input::new(input)))
+            .child(div().w(gpui::px(256.)).child(Input::new(input)))
@@ table stub
-                    .w(Sp::S32.pixels() * 10.0)
+                    .w(gpui::px(320.))
@@ fn icon_cell
-        .w(Sp::S32.pixels() * 3.0)
+        .w(gpui::px(96.))
```

The tenth site is different — it is the **scale demo bar** at `gallery.rs:249`,
whose job is to render each step at its true size, so it moves to `rems`:

```diff
-            .child(div().w(sp.pixels()).h(Sp::S8.pixels()).bg(theme.primary))
+            .child(div().w(sp.rems()).h(Sp::S8.rems()).bg(theme.primary))
```

⚠ The gallery's module doc (`gallery.rs:13-17`) claims "every gap from `Sp`".
That stays true — gaps still come from `Sp`; widths never did. No doc edit
needed, but do not "fix" the new `px()` calls back to `Sp` on a later read.

- [ ] **Step 7: Compile-check every consumer**

The type change is compiler-enforced, so this step is the real inventory:

```bash
cargo clippy -p dat0-app --all-targets --features a11y-capture,gallery -- -D warnings
```

Expected: exit 0. Any remaining `.pixels()` on an `Sp` shows up here as a
missing-method error, with the exact file and line. If one appears in a file
this task does not list, **stop and report** — the count was 10 + 1 (status
bar) and a difference means the inventory was stale, which is exactly what A5
T4 caught.

- [ ] **Step 8: Run the token tests**

```bash
cargo test -p dat0-app --lib theme::tokens -- --nocapture
```

Expected: PASS, including the three T0 gates and the rewritten
`sp_scale_exact_values`.

- [ ] **Step 9: Run the gallery smoke test**

```bash
cargo test -p dat0-app --features gallery --test gallery_smoke
```

Expected: PASS (the 6 section a11y seams). The gallery renders the scale
section, so this is the only automated check that the demo bars still build.

- [ ] **Step 10: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/theme/tokens.rs crates/dat0-app/src/view/status_bar.rs \
        crates/dat0-app/src/gallery.rs
git commit -s -F - <<'EOF'
refactor(theme): B10 T2 — Sp becomes rem-relative; one spacing scale

dat0 was carrying two spacing scales that disagreed by 14%. gpui's
helpers are rem-relative (gap_1 == rems(0.25)) and gpui_component's Root
sets the rem size from theme font.size, which A1 pinned at 14 — so every
helper resolves at 87.5% of its documented value. A2's Sp returned
absolute px and was never reconciled, though TextRole right beside it was
("Body is 13px against the A1 font.size 14 root").

The codebase runs 196 rem-relative call sites against 26 Sp sites, so Sp
was the minority scale sitting 14% looser than its neighbours. Sp::pixels
is replaced by Sp::rems, after which Sp::S4 is exactly .gap_1().

26 production sites re-space by -14% — overlay, command palette, status
bar, saved-query picker, pipeline bar — none needing a call-site edit,
since they all route through SpStyled.

Two call sites were using the spacing scale for something else and are
corrected rather than converted: the status bar's 1px hairline (a
rem-relative S1 would be 0.875px) now says px(1.), and the gallery's ten
fixed demo widths say px() — Rems has no Mul<f32>, so the compiler found
all of them. The gallery's scale-demo bar does move to rems, since its
job is to render the scale at true size.
EOF
```

---

## Task 3: A6h — the three `window.rs` chains

**Files:**
- Modify: `crates/dat0-app/src/window.rs:4123`, `:7516-7517`, `:7682-7689`, and imports

**Interfaces:**
- Consumes: `Sp`, `SpStyled` from T2. **Do not run this task if T0's STOP
  clause fired** — without T2 these conversions are a +14% spacing change.

- [ ] **Step 1: Add the spacing imports**

`crates/dat0-app/src/window.rs` — extend the line T1 added:

```diff
-use crate::theme::tokens::Dat0Theme as _;
+use crate::theme::tokens::{Dat0Theme as _, Sp, SpStyled as _};
```

- [ ] **Step 2: Convert the chart toolbar row**

`crates/dat0-app/src/window.rs:4123`:

```diff
-        let mut row = h_flex().gap_2().flex_wrap().p_2().child(type_btn);
+        let mut row = h_flex()
+            .gap_sp(Sp::S8)
+            .flex_wrap()
+            .p_sp(Sp::S8)
+            .child(type_btn);
```

- [ ] **Step 3: Convert the banner host**

`crates/dat0-app/src/window.rs:7513-7517`:

```diff
             gpui::div()
                 .flex()
                 .flex_col()
-                .gap_1()
-                .p_1()
+                .gap_sp(Sp::S4)
+                .p_sp(Sp::S4)
```

- [ ] **Step 4: Convert the tab strip**

`crates/dat0-app/src/window.rs:7681-7691`:

```diff
             let tab_label = h_flex()
-                .gap_1()
+                .gap_sp(Sp::S4)
                 .items_center()
                 .child(div().child(label))
                 .children(is_dirty.then(|| div().child("•")));
             h_flex()
                 .w_full()
-                .px_3()
-                .py_1()
+                .px_sp(Sp::S12)
+                .py_sp(Sp::S4)
                 .border_b_1()
                 .child(tab_label)
                 .into_any_element()
```

- [ ] **Step 5: Verify zero delta**

These conversions must change no pixels. `Sp::S4.rems() == rems(0.25) ==
.gap_1()`, `Sp::S8 == .gap_2()`, `Sp::S12 == .px_3()` — held by
`sp_rems_matches_gpui_helper_scale`, which T0 already proved non-vacuous.

```bash
cargo clippy -p dat0-app --all-targets -- -D warnings
cargo test -p dat0-app --lib theme::tokens
```

Expected: clippy exit 0, tests PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/window.rs
git commit -s -F - <<'EOF'
style(theme): B10 T3 — window.rs spacing onto Sp (A6h, zero delta)

The last of A6h. Three element chains — the chart toolbar row, the banner
host and the tab strip — move from gpui's helpers to Sp.

Zero pixel change, and only because T2 landed first: Sp::S4 is now
exactly .gap_1(), Sp::S8 is .gap_2(), Sp::S12 is .px_3(), held by
sp_rems_matches_gpui_helper_scale. Before T2 these same conversions would
have been a 14% spacing increase against the elements around them,
including the banner host's own children.

The master plan's other named A6h targets do not exist any more: the
recents ring migrated in A6 (empty_state.rs went 1 to 0), and window.rs
holds no magic pixels — its 14 px() sites are named dock-width consts.
EOF
```

---

## Task 4: `fill_handle` — record the search, keep the token

**Files:**
- Modify: `crates/dat0-app/src/theme/tokens.rs:22`

- [ ] **Step 1: Document the field**

`crates/dat0-app/src/theme/tokens.rs:22` — replace:

```rust
    pub fill_handle: Hsla,
```

with:

```rust
    /// No production consumer, and that is a finding rather than an oversight.
    /// A6 deviation 1 searched for a fill-handle render site and found none —
    /// `grid/mod.rs:72-76` records why the obvious candidate took `primary` /
    /// `primary_foreground` instead (it is the column-reorder ghost, it paints
    /// text on the fill, so it needs a text pair the contrast gate already
    /// covers; `fill_handle` is ring@0.72, tuned by A3 for the non-text 3:1
    /// bar). Rendered in the gallery only. Kept, on the A5 `Play`/`Bookmark`
    /// precedent: do not invent a consumer, and do not delete a tuned token a
    /// grid fill handle would want. Delete it if that feature is ruled out.
    pub fill_handle: Hsla,
```

- [ ] **Step 2: Verify nothing else moved**

```bash
cargo test -p dat0-app --lib theme::tokens
cargo test -p dat0-app --test style_lint
```

Expected: PASS both. The derivation assertion at `tokens.rs:359` is unchanged.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/theme/tokens.rs
git commit -s -F - <<'EOF'
docs(theme): B10 T4 — record why fill_handle has no consumer

The token is gallery-only. A6 deviation 1 established the tree has no
fill-handle render site, and grid/mod.rs:72-76 explains why the obvious
candidate took primary/primary_foreground instead. Writing that on the
field stops the next reader re-deriving the search, and states the
condition under which deleting it is right.
EOF
```

---

## Task 5: Make the planning artifacts true

**Files:**
- Modify: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §6 and §7
- Modify: `docs/plans/2026-08-05-dat0-ui-redesign-b10-cleanup-styling-design.md` (append §11)

- [ ] **Step 1: Rewrite the B10 row and add B11**

`docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md:97` — replace the B10
row with two rows:

```markdown
| **B10 Cleanup + A6h** | ~~Delete dead shell bools/`hero_focus` remnants, collapse hybrid scaffolding~~ — all three measured **live** at `136ef75` and struck. Shipped: file-drop tint → `cx.theme().d0().drag_over` + `drop_target` α retune (**lint allowlist now empty for colors**, and HC stops painting a hardcoded blue); `Sp` made rem-relative so dat0's two spacing scales, which disagreed by 14%, become one; the three remaining `window.rs` chains onto `Sp` (zero delta). Recents ring had already migrated in A6; `window.rs` holds no magic pixels. `<5k` target moved to B11. | S |
| **B11 window.rs extraction** | `window.rs` (8660 lines / 185 fns) → `window/mod.rs` + child modules, target `<5k`. Pure refactor, no UI change. Child modules see the parent's private items, so `WorkspaceShell`'s fields need **no** visibility change; splitting an `impl` across files is compiler-verified. Budget: test accessors ~390 · `cfg(test)` ~205 · AI ~480 · SQL ~486 · dock ~600 · charts ~350 · connections/MD ~267 · export+drop ~340. Must not lose the interim `DOCS_URL`/`DISCORD_URL` consts, and must keep the `a11y-capture` accessor block ahead of any `#[cfg(test)] mod` (clippy `items-after-test-module`). | M |
```

- [ ] **Step 2: Extend the sequencing line**

`docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md:103`:

```diff
-A0 → A1 → A2 → A3 → A4 → A5 → A6a-g → B1 → B2 → B3 → B4 → B5 → B6 → B7 → B8 → B9 → B10(+A6h).
+A0 → A1 → A2 → A3 → A4 → A5 → A6a-g → B1 → B2 → B3 → B4 → B5 → B6 → B7 → B8 → B9 → B10(+A6h) → B11.
```

and in the paragraph below it, change `B5-B10 are strictly ordered` to
`B5-B10 are strictly ordered; B11 is a pure refactor and depends only on B10`.

- [ ] **Step 3: Append the as-built section to the design doc**

Append to `docs/plans/2026-08-05-dat0-ui-redesign-b10-cleanup-styling-design.md`:

```markdown
---

## 11. As-built

Fill in at the end of execution, before opening the PR:

- Commits, one line each, task → sha.
- Any deviation from this plan, with why. State the deviation even when the
  outcome was better — A5's six deviations are the most reused part of that
  slice's record.
- The T0 gate's actual result, and whether the STOP clause fired.
- Local gate results: `fmt`, `clippy --workspace --all-targets -D`, the three
  `-p dat0-app` feature combinations with the binary count, `src/grid`
  byte-identity, and the seeded-`[ui.dock_layout]` boot-log diff against a
  `main` build.
- Anything measured that contradicts this document.
```

- [ ] **Step 4: Commit**

```bash
git add docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md \
        docs/plans/2026-08-05-dat0-ui-redesign-b10-cleanup-styling-design.md
git commit -s -F - <<'EOF'
docs(theme): B10 T5 — correct the master plan; record B11

Four of the B10 row's five clauses did not survive measurement against
136ef75. The dead shell bools are live (7-15 refs each), hero_focus is
load-bearing across 14 sites, B5's hybrid scaffolding was resolved
in-slice, and the recents ring migrated in A6. Struck rather than
silently dropped, so the next reader sees they were checked.

The <5k line target is an extraction project — 8660 lines, ~3700 to move
— and becomes B11 with its measured budget and the module-privacy lever
that makes it behaviour-neutral. Sequencing extended.
EOF
```

---

## Controller gate (after T5, before opening the PR)

Not a task — the controller runs this, per the B9 process.

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` → exit 0
- [ ] `cargo test -p dat0-app` → 0 failures
- [ ] `cargo test -p dat0-app --features a11y-capture` → **118 binaries**, 0 failures
- [ ] `cargo test -p dat0-app --features a11y-capture,gallery` → 0 failures

  ⚠ Redirect to a file and count there. Do **not** pipe through `head` — it
  SIGPIPEs cargo mid-write and truncates the count (A6 counted 51 of 109).

  ```bash
  cargo test -p dat0-app --features a11y-capture > /tmp/b10-a11y.txt 2>&1
  grep -c "^test result:" /tmp/b10-a11y.txt
  grep -c "FAILED" /tmp/b10-a11y.txt
  ```

- [ ] `git diff --stat main -- crates/dat0-app/src/grid` → empty
- [ ] `grep -rn "0x[0-9a-fA-F]\{6\}" crates/dat0-app/src --include=*.rs` → no colour hits
- [ ] **Boot the binary with a seeded dock layout.** A fresh-session boot
  exercises none of the restore path (B9's finding), so an unseeded boot check
  is vacuous.

  ```bash
  cargo build --bin dat0
  export DAT0_CONFIG_DIR=/tmp/b10-boot
  mkdir -p "$DAT0_CONFIG_DIR"
  # seed [ui.dock_layout] in settings.toml, then:
  ./target/debug/dat0 2>&1 | tee /tmp/b10-boot.log
  ```

  Diff against a `main` build's log. Expected: identical.

- [ ] Manually drag a file over the window in **all three themes** — the one
  check for a path with no automated coverage. HC must be yellow, not blue.

---

## Self-review

**Spec coverage.** Design §2 → T1. §3 → T0 + T2. §4 → T3. §5 `fill_handle` →
T4; §5's ruling to leave B9's empty `[ui]` table alone needs no task. §6
coverage → the gate commands in each task plus the controller gate. §7 B11 →
T5. §8 task shape → T0-T5 as written. §10 owed glance → the controller gate's
last item, and the memory update at merge.

**Placeholders.** None. Every code step carries the literal before/after. The
one deliberately unfilled section is design §11 as-built, which T5 Step 3
creates as a template because its content does not exist until execution ends.

**Type consistency.** `Sp::rems() -> Rems` is introduced in T0 Step 2 and used
under that exact name in T2 Steps 1-6, T3 Step 5 and the tests. `From<Sp> for
Rems` is added in T0 and asserted in T2 Step 3. `Sp::pixels()` survives T0
(additive) and is removed in T2 Step 1 — the only window where both exist is
between those two commits, which is intentional and is what makes T0 a gate
rather than a rewrite.
