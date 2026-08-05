# Slice B10 — cleanup + A6h `window.rs` styling (design)

**Date:** 2026-08-05
**Branch:** `feat/ui-redesign-b10-cleanup-styling`, off main `136ef75` (B9)
**Master plan:** `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §6 row B10
**Size:** S (the master plan said M; §7 explains why the M content moved to B11)

B10 is the final slice of UI redesign v1. It closes the last colour literal in
`src/`, unifies the two spacing scales the codebase has been carrying since A2,
folds the residual A6h styling, and leaves the plan doc telling the truth about
what shipped.

---

## 1. What the kickoff block promised, and what the tree actually holds

The master plan row for B10 was written 2026-07-21, before B5–B9 existed. Four
of its five clauses did not survive measurement against `136ef75`. Recording
this here rather than silently re-scoping, because the same staleness will bite
whoever reads the row next.

| Master plan clause | Measured at `136ef75` |
| --- | --- |
| "delete dead shell bools" | **Stale.** Every `*_panel_visible` bool is live (7–15 references each). `right_dock_state` / `left_dock_state` are caches that prevent dock rebuilds, not duplicates. |
| "`hero_focus` remnants" | **Stale.** `hero_focus` is load-bearing across 14 sites — B7's activity rail and the AI dock both mint handles through `hero_focus_handle`. |
| "collapse hybrid scaffolding" | **Stale.** B5's hybrid was resolved in-slice. `window.rs` carries exactly one `#[allow(dead_code)]`, correctly justified (a `Subscription` keep-alive). |
| "window.rs styling migration (drag tint, tab strip, recents ring, magic px → `Sp`/consts)" | **Partly stale.** The recents ring migrated in A6 (`empty_state.rs` went 1 → 0). `window.rs` holds **10** styling calls and **14** `px(` sites, and all but two of the latter are already named consts (`LEFT_DOCK_WIDTH`, `INSPECTOR_DOCK_WIDTH`, …). There are no magic pixels left to migrate. |
| "lint allowlist → empty for colors" | **Live, and the one mandatory item.** |
| "window.rs target <5k lines" | **Live, and an extraction project.** `window.rs` is 8660 lines / 185 functions. See §7. |

So B10's real content is: the ratchet, a spacing defect discovered while
scoping it, three element chains, and the documentation debt.

---

## 2. The drag tint — closing the ratchet

### 2.1 The site

`src/window.rs:8014`, the only colour literal left in `src/` since A6:

```rust
.drag_over::<ExternalPaths>(|style, _, _, _| style.bg(gpui::rgba(0x0088_ff22)))
```

`ALLOW = &[("window.rs", 1)]` in `tests/style_lint.rs:49` exists solely for it.

### 2.2 The token is not solid — correcting the B9 handoff note

The B9 handoff recorded that `Dat0Colors::drag_over` is `self.drop_target` and
therefore **solid**, making the substitution a visible design change. That is
wrong. All three builtins define `drop_target.background` as **8-digit hex
carrying alpha**:

| theme | `drop_target.background` | α | literal being replaced |
| --- | --- | --- | --- |
| dark | `#58a6ff1a` | 0.102 | `#0088ff` @ 0.133 |
| light | `#0969da1a` | 0.102 | `#0088ff` @ 0.133 |
| high contrast | `#ffff0033` | 0.200 | `#0088ff` @ 0.133 |

The token is already a translucent tint. What changes is the **hue** — and in
high contrast, that is a defect being fixed rather than a change being made:
today the file-drop tint paints a hardcoded blue that ignores the HC theme
entirely.

### 2.3 Decision — substitute and retune alpha

```rust
.drag_over::<ExternalPaths>(|style, _, _, _| style.bg(cx.theme().d0().drag_over))
```

plus, in `src/theme/builtins/`:

```
dark.json    "drop_target.background": "#58a6ff1a" → "#58a6ff22"
light.json   "drop_target.background": "#0969da1a" → "#0969da22"
high-contrast.json                                   unchanged (#ffff0033)
```

Retuning buys **alpha parity only** — the tint keeps today's strength while the
hue moves to the theme's accent. It does not preserve today's pixels, and the
design does not claim it does.

`ALLOW` becomes `&[]`. `ratchet_report` handles an empty table without change:
every file's budget falls to the `unwrap_or(0)` default, and the under-budget
loop iterates nothing.

### 2.4 Blast radius, bounded by reading the consumers

`drop_target` is a `gpui_component::ThemeColor` field, so the JSON edit is a
global token change rather than a local one. It has exactly two other
consumers, both in `dock/tab_panel.rs` (`:789`, `:869`) painting the tab
drag-drop highlight. dat0 v1 does not enable dock tab dragging, so both are
inert. Blast radius is real and empty.

### 2.5 The gate that will actually run

`tests/theme_contrast_gate.rs:174-198` (`composited_tints_keep_text_readable`)
composites `drop_target.background` source-over `background` and requires
`foreground` to stay ≥ 4.5:1 through the tint. Computed ahead of the change
rather than discovered by it:

| theme | before (α 1a) | after (α 22) | floor |
| --- | --- | --- | --- |
| dark | 10.62:1 | **10.06:1** | 4.5 |
| light | 13.68:1 | **13.07:1** | 4.5 |
| high contrast | 13.01:1 (unchanged) | 13.01:1 | 4.5 |

Passes with better than 2× headroom.

---

## 3. The spacing scales disagree by 14%

Discovered while scoping the A6h chains. This is the substantive finding of the
slice.

### 3.1 The measurement

`gpui`'s spacing helpers are **rem-relative**. `gpui-macros/src/styles.rs:985`
emits `.gap_1()` as `rems(0.25)`, documented "4px (0.25rem)".

`gpui_component::Root::render` sets the rem size from the theme
(`crates/ui/src/root.rs:398`):

```rust
window.set_rem_size(cx.theme().font_size);
```

dat0 mounts `Root` (`window.rs:1147`, `:1926`), and A1 set `"font.size": 14` in
all three builtins. **dat0's rem is 14px, not gpui's default 16.** Every gpui
spacing helper therefore resolves at 87.5% of its documented value.

`Sp::pixels()` returns `px(self as u16 as f32)` — absolute.

| helper | doc says | actual at rem 14 | `Sp` equivalent | disagreement |
| --- | --- | --- | --- | --- |
| `.gap_1()` `.p_1()` `.py_1()` | 4px | **3.5px** | `Sp::S4` = 4px | +14% |
| `.gap_2()` `.p_2()` | 8px | **7px** | `Sp::S8` = 8px | +14% |
| `.px_3()` | 12px | **10.5px** | `Sp::S12` = 12px | +14% |

### 3.2 Which scale the codebase is actually on

- **196** rem-relative spacing-helper call sites (`sql_console.rs` 54,
  `settings_ui/panel.rs` 22, `pipeline_bar.rs` 19, `connections/panel.rs` 14,
  … `window.rs` 7).
- **26** production `Sp::` sites, across five files: `overlay.rs` 2,
  `view/command_palette.rs` 10, `view/status_bar.rs` 6,
  `view/saved_query_picker.rs` 7, `view/pipeline_bar.rs` 1.

`Sp` is the minority scale. Converting `window.rs` to it — the original A6h
instruction — would have made `window.rs` the outlier, not the conformer, and
would have spaced the banner host's container at 4px while
`error_ux::banner`'s children space at 3.5px.

### 3.3 Where the defect came from

A2 introduced both scales and reconciled only one. `TextRole`'s doc comment
reads "Body is 13px against the A1 `font.size` 14 root" — typography knows
about the 14px root. `Sp`'s reads only "Spacing scale (px)". Spacing was never
reconciled, and nothing measured it until now.

### 3.4 Decision — unify by making `Sp` rem-relative

```rust
// src/theme/tokens.rs
- pub fn pixels(self) -> Pixels { px(self as u16 as f32) }
+ pub fn rems(self) -> Rems     { rems(self as u16 as f32 / 16.) }
```

`SpStyled`'s five method bodies are unchanged — `Rems` flows into the same
setters through gpui's existing conversions:

- `impl From<Rems> for AbsoluteLength` (`gpui-0.2.2/src/geometry.rs:3183`)
- `impl From<Rems> for DefiniteLength` (`:3453`) — what padding and gap take
  (`gpui-macros/src/styles.rs:600` generates `DefiniteLength`)
- `impl From<Rems> for Length` (`:3623`) — what width, height and margin take

After this, `Sp::S4` **is** `.gap_1()`, exactly. The scales become one.

`Sp` still earns its place: it is a restricted, named 9-step subset of gpui's
open scale, and it survives a future `font.size` change without re-forking.

### 3.5 Consequences, each handled rather than absorbed

**(a) 26 production sites re-space −14%.** They shrink to match the 196 sites
around them. This is the point of the change, and it lands on B1/B2 overlay,
B3 status bar, B4 command palette and the saved-query picker — all surfaces
that already have visual glances owed, so the fix precedes the eyeball rather
than following it.

**(b) `view/status_bar.rs:168` paints a 1px hairline** through the spacing
scale:

```rust
div().w(Sp::S1.pixels()).h(Sp::S12.pixels())
```

At rem 14 a rem-relative `Sp::S1` becomes 0.875px — a sub-pixel rule. A
hairline is not spacing. It becomes `.w(gpui::px(1.))`, with the height staying
on the scale as `Sp::S12.rems()`.

**(c) `gallery.rs` uses `Sp` for width arithmetic** at 10 sites, e.g.
`Sp::S32.pixels() * 4.0`. `Rems` implements `Mul<Pixels>`, **not** `Mul<f32>`
(`geometry.rs:3121`), so these cannot survive the swap unchanged. They move to
plain `px()`. A4 already parked "no width scale" as a ruling; deleting
`pixels()` stops `Sp` pretending to be one.

The gallery's spacing-scale *demo bars* (`gallery.rs:249`,
`div().w(sp.pixels())`) do convert to `sp.rems()` — that bar's job is to render
the scale at its true size.

### 3.6 The gate that locks it

`tokens.rs` gains an equivalence test asserting that `Sp` and gpui's helper
scale are the same scale:

```rust
assert_eq!(Sp::S4.rems(), gpui::rems(0.25));   // == .gap_1()
assert_eq!(Sp::S8.rems(), gpui::rems(0.5));    // == .gap_2()
assert_eq!(Sp::S12.rems(), gpui::rems(0.75));  // == .px_3()
```

plus a resolved-pixel assertion at dat0's real rem size, using
`Rems::to_pixels` (`geometry.rs:3115`):

```rust
assert_eq!(Sp::S4.rems().to_pixels(px(14.)), px(3.5));
```

Without the resolved assertion the test proves only that two constants match;
with it, the test states what a user sees. The existing scale test at
`tokens.rs:370-381` (which asserts `sp.pixels() == px(v)`) is rewritten rather
than deleted — the mapping it guards is still the thing under test, expressed
in the new unit. The `StyleRefinement` composition test at `:505-509` updates
its three expectations from `Sp::S8.pixels().into()` to `Sp::S8.rems().into()`.

---

## 4. A6h — the three `window.rs` chains

With §3 landed these are zero-delta conversions, which is the only reason they
are in scope:

| site | today | after |
| --- | --- | --- |
| `window.rs:4123` `render_chart_toolbar` | `.gap_2().flex_wrap().p_2()` | `.gap_sp(Sp::S8).flex_wrap().p_sp(Sp::S8)` |
| `window.rs:7516` banner host | `.gap_1().p_1()` | `.gap_sp(Sp::S4).p_sp(Sp::S4)` |
| `window.rs:7682` tab strip | `.gap_1() … .w_full().px_3().py_1()` | `.gap_sp(Sp::S4) … .px_sp(Sp::S12).py_sp(Sp::S4)` |

These are the last of A6h. They are optional in the sense that they change no
pixels; they are included because the master plan named the tab strip
specifically, and because B11 is about to move this code and should move it in
its final form.

---

## 5. Cleanup

**`Dat0Colors::fill_handle`** has no production consumer — only `gallery.rs:146`
and its own derivation assertion. A6 deviation 1 established that the tree has
no fill-handle render site at all, and `grid/mod.rs:72-76` documents why the
obvious candidate (the column-reorder ghost) took `primary` /
`primary_foreground` instead. The token is kept and gains a doc comment
recording that, so the next reader does not re-derive the search. Same
precedent as A5's `Play` / `Bookmark`: do not invent a consumer.

`Dat0Colors::drag_over` gains a production consumer in §2 and needs no note.

**B9's cosmetic leftover** — an empty `[ui]` table header written to
`settings.toml` even when `dock_layout` is `None` — is left alone. It
round-trips, it is gated by a test, and changing it is a settings-format edit
for no user-visible gain.

---

## 6. Coverage, including what is deliberately not covered

**Deliberate gap: the drag tint has no automated test, and gets none.** There
is no window-level drag-drop test anywhere in the tree, and the site B10 must
touch is a `.drag_over::<ExternalPaths>` **style closure** — it runs only while
a real platform drag is over the window. gpui 0.2.2's test harness offers no
external-file-drag simulation, so a test could at best assert that the closure
reads the token, not that the tint paints. Recorded as glance-only, explicitly,
rather than left as an unexamined hole. The `on_drop` path it sits beside stays
covered by `file_drop.rs` unit tests.

Everything else in the slice is gated:

| change | gate |
| --- | --- |
| drag tint substitution | `tests/style_lint.rs` — `ALLOW = &[]`, zero literals in `src/` |
| JSON alpha retune | `theme_contrast_gate::composited_tints_keep_text_readable` |
| `Sp` unification | new equivalence + resolved-pixel tests in `tokens.rs` |
| `Sp` call-site fixups | `cargo clippy --workspace --all-targets -D warnings` (type change is compiler-enforced) |
| window.rs chains | full a11y suite; no a11y node changes expected |

Local gate, per the B9 handoff: `cargo fmt --check`, `clippy --workspace
--all-targets -D warnings`, `-p dat0-app` × {plain, `a11y-capture`,
`a11y-capture,gallery`} (118 binaries at B9), plus `cargo build --bin dat0` and
**boot it with a seeded `[ui.dock_layout]`**, diffing the log against a `main`
build. A fresh-session boot exercises none of the restore path — seeding is
what makes the boot check non-vacuous.

`cargo test --workspace` and `cargo bench` remain unrunnable on this machine
(macOS 27 / Xcode 26.6 vs vendored DuckDB Thrift). CI is the gate for both.

**Bench.** B10 is a bench-gated slice per the master plan, but B5 settled that
`benches/grid_scroll.rs` never builds a `Window` or the `Table` delegate — it
loops `renderers::render_cell` over a synthetic Arrow batch. It cannot see a
styling change. Verify at step level post-merge and `gh run download` the
artifact for the series; do not read meaning into the number.

---

## 7. Why the `<5k` target is not in this slice

`window.rs` is 8660 lines across 185 functions. Reaching under 5000 means
moving roughly 3700 lines. Measured extraction budget:

| region | lines |
| --- | --- |
| `#[cfg(feature = "a11y-capture")]` test accessors (L8066–8455) | ~390 |
| `#[cfg(test)]` unit tests (L8456–8660) | ~205 |
| AI (`handle_ai_panel_event`, `spawn_ai_explain`, `spawn_ai_nl2sql`, `spawn_ai_test`, `open_ai_entry_prompt`) | ~480 |
| SQL run / console (`finish_sql_run`, `mount_sql_console`, `on_sql_console_event`, `spawn_sql_run`, `save_console_as_table`, `refresh_completion_snapshot`) | ~486 |
| Dock (`ensure_dock_area` + four `render_*_body`) | ~600 |
| Charts (`run_plot_query`, `render_chart_toolbar`, `show_chart_with_spec`, free axis helpers) | ~350 |
| Connections / MotherDuck (`handle_connections_event`, `open_md_token_prompt`, `spawn_md_connect`, `spawn_md_test`) | ~267 |
| Export + drop routing (`run_export`, `route_export_event`, `route_drop_outcomes`, `open_sample_kind`) | ~340 |
| | **≈ 3100–3700** |

Hitting the target requires taking essentially all of it. That is a refactor
project, not a styling pass, and pairing a 3700-line move with the slice's one
visible pixel change would gut both reviews — in particular the "diff the boot
log against a `main` build" gate, which is only readable when the diff is
small.

It becomes **B11**, its own PR, no UI change. The mechanical lever is already
identified: converting `window.rs` into `window/mod.rs` plus child modules
needs **zero visibility changes**, because Rust makes a private item visible in
its defining module *and all descendants* — so `window/ai.rs` can touch
`WorkspaceShell`'s private fields directly. Splitting an `impl` block across
files within a crate is legal and compiler-verified, which makes the move
behaviour-neutral by construction.

Two things B11 must not lose: the interim `DOCS_URL` / `DISCORD_URL` consts
from the 2026-07-21 menu hotfix (P11b/P11c swap them before release), and the
`items-after-test-module` clippy constraint that forces the
`a11y-capture` accessor block to precede any `#[cfg(test)] mod`.

---

## 8. Task shape

**T0 — hard gate.** Two real unknowns, both proven before any production edit:

1. `Rems` flows through all five `SpStyled` methods (`p_sp`, `px_sp`, `py_sp`,
   `gap_sp`, `m_sp`) — argued from the `From` impls in §3.4, but compiled, not
   assumed.
2. `Sp::S4.rems()` resolves to the same pixels as gpui's `.gap_1()` at
   `rem_size = 14` — measured through `Rems::to_pixels`.

**STOP clause:** if (1) fails, the unification is off. B10 falls back to the
"leave the chains" ruling and ships **T1, T4 and T5 only** — §3 is then
recorded in the design doc as a measured defect owed its own slice, and T2/T3
are dropped rather than attempted at +14%.

| task | content |
| --- | --- |
| T1 | drag tint → token; two JSON alpha retunes; `ALLOW = &[]`; contrast gate |
| T2 | `Sp::pixels()` → `Sp::rems()`; `SpStyled`; status-bar hairline; gallery width math; `tokens.rs` tests rewritten + equivalence gate |
| T3 | the three `window.rs` chains → `Sp` (zero-delta) |
| T4 | `fill_handle` doc comment |
| T5 | master plan §6: B10 row rewritten as-built (S, not M), new B11 row, sequencing line |

Executed inline by the controller, no subagents, per the B9 process.

---

## 9. Decision register

| # | Decision | Alternative rejected | Why |
| --- | --- | --- | --- |
| 1 | B10 small; extraction → B11 | one PR to `<5k` | a 3700-line move beside one visible pixel change makes both reviews unreadable, and kills the boot-log diff gate |
| 2 | Substitute token **and** retune α `1a → 22` | substitute only | keeps today's tint strength; hue moves either way; gate verified passing |
| 3 | Drag tint stays glance-only | write the first window-level drop test | `.drag_over` style closures need a real platform drag; no harness support at gpui 0.2.2 |
| 4 | One combined visual pass **after** B10 | pass before | B10's visible surface is small and overlaps the owed B4–B9 list |
| 5 | Unify `Sp` to rem-relative | leave both scales; or convert `window.rs` at +14% | one line makes `Sp::S4` ≡ `.gap_1()`; the alternative left a 14% disagreement between 26 and 196 sites |
| 6 | Convert all three `window.rs` chains | leave them | zero-delta after decision 5; the master plan named the tab strip |
| 7 | Keep `fill_handle`, documented | delete the token | A5 `Play`/`Bookmark` precedent; 1 line, A3-tuned; the doc stops the next reader re-deriving the search |
| 8 | Record B11 in the master plan **and** memory | memory only | the plan doc's B10 row is stale in four clauses; leaving it wrong at series close is worse than the churn |

---

## 10. Owed human glance (added to the combined B4–B10 pass)

1. **File drop, all three themes** — drag a file over the window. Dark and
   light should read as today's tint at the theme's accent hue; **high
   contrast should now be yellow, not blue** (the defect fix). This is the only
   check for an otherwise untested path.
2. **The −14% re-space**, on the four `Sp` surfaces: command palette, status
   bar, saved-query picker, modal overlay padding. Looking for anything that
   now reads cramped, and for alignment against neighbouring rem-spaced
   elements — the whole point is that they should now agree.
3. Carried forward: the B4 palette, B5 dock pixels / narrow window / file drop,
   B6 title bars, B7 rail, B8 console chrome and double title bar, B9 restored
   layout and panel *contents*, all × 3 themes.

---

## 11. As-built

**Branch** `feat/ui-redesign-b10-cleanup-styling` off main `136ef75`. Executed
inline by the controller, no subagents.

| task | sha | content |
| --- | --- | --- |
| design | `e582657` | this document |
| plan | `a9cf787` | `…-b10-cleanup-styling-plan.md`, 6 tasks |
| T0 | `0e13c78` | `Sp::rems()` added additively + three gates |
| T1 | `cb9d8f5` | drag tint → token, α retune, `ALLOW = &[]` |
| T2 | `c158a6e` | `Sp::pixels()` → `Sp::rems()`, call-site fixups |
| T3 | `08b2561` | the three `window.rs` chains |
| T4 | `01fd609` | `fill_handle` doc comment |
| T5 | *(this commit)* | master plan B10 row + B11 row + this section |

### 11.1 The T0 gate

**Passed; the STOP clause did not fire.** `Rems` compiles into all five
`SpStyled` setters, so T2 and T3 both ran.

Non-vacuity of the scale gates was proven by perturbing `Sp::rems`'s divisor to
`/ 8.`, which reddened `sp_rems_matches_gpui_helper_scale` and
`sp_rems_resolve_at_dat0_rem_size` (`left: 1.75px, right: 0.875px`), then
reverting with a `touch` to force the rebuild.

### 11.2 Deviations from the plan

**One, and it was in the plan's own test code.** T0 Step 3's
`rems_flows_through_every_styled_setter` chained `.p()`, `.px()`, `.py()`,
`.gap()` and `.m()` on a single element and then asserted
`padding.top == Sp::S8`. It failed: `py` sets top *and* bottom, so it overwrote
the `padding.top` that `p` had set one call earlier, and the element reported
`0.25rem` where the assertion wanted `0.5rem`.

The failure was the test's, not the code's — and it still discharged the gate,
because reaching a runtime assertion at all proved the `Rems` conversion
compiles into every setter. Rewritten as one element per setter, with a comment
saying why chaining is wrong here.

Worth keeping as a general note: **a chained style assertion measures the
setter's semantics, not the value you passed it.** Any future read-back test
over gpui's overlapping shorthands (`p`/`px`/`py`, `m`/`mx`/`my`,
`border`/`border_x`) needs one element per setter.

### 11.3 Measurements that differ from what this document predicted

| quantity | predicted | measured |
| --- | --- | --- |
| dark, `foreground` over `drop_target∘background` at α `22` | 10.06:1 | **10.04:1** |
| light, same | 13.07:1 | 13.07:1 |
| high contrast, same (unchanged) | 13.01:1 | 13.01:1 |

The dark figure is 0.02 off because §2.5's arithmetic rounded the hex→float
conversion; the gate's own compositor is the authority and both clear the 4.5
floor by better than 2×.

Everything else held. In particular the `.pixels()` inventory was exactly the
predicted 10 gallery sites plus the one status-bar hairline — `cargo clippy
--all-targets --features a11y-capture,gallery` reached exit 0 with no
unlisted call site, so the type change enumerated its own blast radius. This is
the A6 lesson again: **change the signature first and let the compiler produce
the inventory.**

### 11.4 Independent verification of the ratchet's premise

The gate says `src/` holds no colour literal. Verified separately from the test
that asserts it, by scanning the live tree:

- `grep -rnE '(^|[^0-9a-zA-Z_])0x[0-9a-fA-F]{6}([0-9a-fA-F]{2})?([^0-9a-fA-F]|$)' crates/dat0-app/src --include='*.rs'` → **0 hits**
- `grep -rn "style-lint: allow(" crates/dat0-app/src` → **0 hits**

So the allowlist is empty *and* unused — no literal survived behind an escape.
The empty ratchet was itself proven non-vacuous by planting `gpui::rgba(…)` in
`tokens.rs`, which reddened the gate naming `src/theme/tokens.rs:605`.

`git diff --stat main -- crates/dat0-app/src/grid` is empty: the grid is
byte-identical, as every B slice has required.

### 11.5 Local gate — all green

| check | result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo clippy -p dat0-app --all-targets --features a11y-capture,gallery -- -D warnings` | exit 0 |
| `cargo test -p dat0-app` | **118** binaries, 0 failures |
| `cargo test -p dat0-app --features a11y-capture` | **118** binaries, 0 failures |
| `cargo test -p dat0-app --features a11y-capture,gallery` | **118** binaries, 0 failures |
| `style_lint` at `ALLOW = &[]` | 4/4 |
| `theme_contrast_gate` | 5/5 |
| `gallery_smoke` | 1/1 |
| `src/grid` vs `main` | byte-identical |
| colour hex in `src/` | 0 |
| `style-lint: allow(` in `src/` | 0 |

**118 binaries, unchanged from B9** — B10 adds no test binary, so the macOS CI
disk figure should not move.

The suite count is the same across all three feature combinations, which is the
expected shape: `a11y-capture` and `gallery` add tests *inside* existing
binaries rather than new targets.

### 11.6 The seeded boot check

B9 established that a fresh-session boot exercises **none** of the restore path,
so an unseeded boot check is vacuous. Two seeded boots were run against
`DAT0_CONFIG_DIR`, chosen to cover both restore arms:

1. `left_panel = "catalog"`, inspector visible, console open — the ordinary arm.
2. `left_panel = "ai"`, charts visible, console open — **B9's riskiest arm**,
   the one whose `on_left_panel_shown` fix puts a `tokio::spawn` in the first
   render, which no test can clear because the harness supplies its own runtime.

Both booted clean: no panic, and the only `error`-matching lines are the two
documented non-fatal update-check DEBUG entries. `settings.toml` round-tripped
byte-unchanged after boot — the seeded layout was neither reset nor rewritten.

Boot log compared against a `main` build booted from the same seed, normalising
timestamps, session UUIDs, durations and the config path: **identical**.
