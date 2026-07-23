# dat0 UI redesign — Slice A3: contrast-gate matrix (design)

Date: 2026-07-23 · Branch: `feat/ui-redesign-a3-contrast-gate` off main `a2d7361` · Size: S
Master plan: `2026-07-21-dat0-ui-redesign-master-plan.md` (slice table §5, A3 row).

## Goal

Extend the P10b/A1 contrast gate from a 5-pair floor into the full WCAG AA matrix
over all 3 builtin theme JSONs, add 8-digit-hex alpha compositing so tinted
surfaces are checked against their *effective* (composited) colors, gate the
Dat0Colors derived alpha factors (A2 recorded the A3 matrix as their correctness
gate — `tokens.rs` comment), and do the final palette tuning the matrix demands.

Measured red set (python pre-compute, verified in-slice by the Rust gate):

| # | Theme | Pair | Now | Min | Fix (owner-approved where marked) |
|---|-------|------|-----|-----|-----------------------------------|
| 1 | dark  | `danger.foreground` on `danger.background` | 3.35 | 4.5 | **APPROVED:** flip `danger.foreground` #ffffff → #0e1116 → 5.64:1 (hover 6.74:1); consistent with success/warning dark-on-vivid; hover/active share the fg key → auto-fixed |
| 2 | light | `muted.foreground` #656d76 on `muted.background` #eaeef2 | 4.4996 | 4.5 | darken `muted.foreground` → #5f6771 → 4.91:1 on muted.bg, 5.73:1 on white (minimal one-shade delta; no other light pair reads this key except via drift-covered muted pairs) |
| 3 | light | `fill_handle` (ring@0.65 composited over white table bg) vs table bg | 2.79 | 3.0 | raise `fill_handle` alpha in `d0()` 0.65 → 0.72 → light 3.17:1, dark 4.38:1, HC 9.89:1 (all green; A2 test asserting 0.65 at tokens.rs:357 updates same commit) |

HC: fully clean. All composited selection/drop-target checks pass 6:1–13:1.

## Decisions (owner)

1. **Danger fix = dark text on red** (over darkening the red / deferring): single-key
   change, consistent family styling, auto-fixes hover/active.
2. **Gate shape = hand-listed tables + drift alarm** (over pure hand-list or
   auto-enumeration): explicit reviewable pairs; a completeness assertion makes
   new `X.foreground`/`X.background` sibling families in the JSON fail the gate
   until listed — no silent dodge, no auto-pairing false positives.

## 1. `contrast.rs` extension (stays pure — no GPUI)

- `pub fn composite_over(fg: &str, bg: &str) -> String` — `#rrggbbaa` fg
  alpha-composited over opaque `#rrggbb` bg → `#rrggbb`. A 6-digit fg is an
  identity passthrough (lets call sites not care whether a key is tinted).
  Standard source-over: `out = fg*α + bg*(1-α)`, per-channel on u8 with rounding.
- `relative_luminance` hardens: **assert** hex payload is exactly 6 digits with a
  "use composite_over for alpha colors" message. Today `#rrggbbaa` silently
  slices the first 6 chars — an alpha color reads as opaque, a latent
  false-pass. Loud gate failure beats silent pass. (`contrast_ratio` inherits
  the assert via `relative_luminance`.)
- Unit tests: composite vectors hand-computed (α=0x00 → bg, α=0xff → fg, real
  `selection.background` values per theme), 6-digit passthrough, assert fires
  on raw 8-digit input to `contrast_ratio`.

## 2. Gate matrix (`tests/theme_contrast_gate.rs` — absorbs the A1 5-pair floor)

Values keep flowing through serde serialization of parsed `ThemeConfigColors`
(exact rename keys; survives Rust field renames). Split into 4 test fns for
readable failures; each loops all 3 JSONs and collects failures before asserting.

### 2a. Text pairs ≥4.5:1 — `(fg_key, bg_key)`, 24 pairs

All 18 sibling `X.foreground`/`X.background` pairs present in the JSONs:
root, accent, muted, secondary, primary, danger, success, warning, info,
popover, sidebar, sidebar.accent, sidebar.primary, group_box, tab, tab.active,
table.head, description_list.label.

Plus 6 cross-family pairs:
`muted.foreground`/`background` (muted text on main surface),
`group_box.title.foreground`/`group_box.background`,
`link`/`background`,
`foreground`/`list.active.background`,
`foreground`/`list.hover.background`,
`foreground`/`table.even.background` (zebra rows).

### 2b. Non-text pairs ≥3:1 (WCAG 1.4.11) — 10 pairs

`ring`/`background` (**kept at 4.5** — it doubles as the accent),
`caret`/`background`, `drag.border`/`background`,
`list.active.border`/`list.active.background`,
`table.active.border`/`table.active.background`,
`danger.background`/`background`, `success.background`/`background`,
`warning.background`/`background`, `info.background`/`background`
(status fills as non-text indicators),
`primary.background`/`background` (progress-bar fill + `pipeline_accent`).

`border` / `input.border` stay exempt — WCAG 1.4.11 decorative carve-out,
same stance as A1.

### 2c. Composited JSON checks

- `composite_over(selection.background, table.background)` → `foreground` ≥4.5
  (selected-cell text stays readable through the tint).
- `composite_over(drop_target.background, background)` → `foreground` ≥4.5.
- `scrollbar.background` is intentionally fully transparent (α=0) — excluded;
  no check reads it.

### 2d. Derived Dat0Colors checks (the A2 alpha-factor correctness gate)

Build the REAL values — `gpui_component::Theme::default()` +
`apply_config(&Rc<ThemeConfig>)` per JSON (A2 lesson: works without App
context), then `.d0()`. Hsla→`#rrggbb` via `Rgba::from(Hsla)` in a small
test-local helper (keeps `contrast.rs` GPUI-free). Checks per theme:

| Derived value | Check | Min |
|---|---|---|
| `selection_tint` (ring@0.13) | composited over `table.background` → `foreground` | 4.5 |
| `active_cell_tint` (ring@0.07) | composited over `table.background` → `foreground` | 4.5 |
| `fill_handle` (ring@0.65) | composited over `table.background` vs `table.background` | 3.0 |
| `banner_tint` (muted_fg@0.08) | composited over `background` → `foreground` | 4.5 |
| `marching_ants` (= success) | vs `table.background` | 3.0 |
| `pipeline_pill` (ring@0.25) | composited over `background` → `foreground` | 4.5 |

Pill text = `foreground` deliberately. If A6f migrates PipelineBar onto
`text_muted` for pill labels, add that pair then (dark measures ≈4.0 → it will
force an explicit decision at migration time, which is correct).

Not gated (decorative / out of family): `pager_dot_*`, `chart_placeholder_*`,
`null_value_fg`+`hover_tint`+`drag_over`+`pipeline_chip`+`text_*`+`banner_{info,warning,error}`
(each equals a JSON key already covered by 2a/2b pairs).

### 2e. Drift alarm

Walk the serialized color key set; for every prefix `X` where both
`X.foreground` and `X.background` exist, assert the pair appears in the 2a
table (root pair handled explicitly). New sibling families fail the gate until
listed with a threshold.

## 3. Palette tuning

Exactly the 3 reds in the table above. TDD order inside the slice: land the
matrix, watch it fail red locally on the measured pairs (proves the gate
bites), then tune, then green. The matrix and the tuning land in the SAME
commit — every pushed commit stays CI-green.

Files touched: `builtins/dark.json` (danger.foreground), `builtins/light.json`
(muted.foreground), `tokens.rs` (fill_handle alpha + comment update),
`contrast.rs`, `tests/theme_contrast_gate.rs`. Nothing else.

## 4. Invariants & risks

- Zero deps, zero schema, zero i18n, zero event variants.
- a11y-capture suite asserts labels not colors → token swaps invisible to it.
- Table + delegate untouched → macOS grid-scroll bench unaffected (color-only
  JSON edits; still WATCH the post-merge main run per standing rule).
- `tokens.rs` fill_handle alpha change touches A2 code: purely a constant;
  A2's alpha self-lint tests reference 0.65 in one assertion
  (`fill_handle.a - dark.ring.a * 0.65` style) → update in the same commit.
- Existing `contrast.rs` unit tests keep passing (6-digit paths unchanged).

## 5. Out of scope

- Hover/active-state pairs beyond the shared-fg-key effect (matrix stays ~base
  states; A4 style_lint is the ratchet slice).
- `input.border`/`border` 1.4.11 checks (exempt, as A1).
- Any surface migration onto tokens (A6a–g).
- style_lint / gallery example (A4), icons (A5).

## 6. Owed human glances (append to running list)

- Dark danger button new look: dark text on red — Settings/destructive
  confirms, both windows.
- Light fill-handle slightly stronger tint (alpha bump) — grid selection drag.
