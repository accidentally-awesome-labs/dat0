# UI Redesign Slice A2 — Token scales (`theme/tokens.rs`) — Design

Date: 2026-07-23 · Slice: A2 of the UI-redesign master plan
(`docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §5) · Branch:
`feat/ui-redesign-a2-token-scales` off main `b474b23` (post-A1).

## Goal

One new module, `crates/dat0-app/src/theme/tokens.rs`, holding every dat0-built
design-system scale: `Dat0Colors` (+ `Dat0Theme` access trait), `Sp` spacing,
`TextRole` typography, `Elevation`, `Density`, and the helper traits
`SpStyled` / `TypoStyled`. **Purely additive** — no call site changes, no
visible pixel changes. A6a–g migrate surfaces onto these tokens; A3 extends the
contrast gate over them; A4 adds the lint + gallery.

Decisions taken with the owner (2026-07-23):

- **Strict zero-literal policy**: every `Dat0Colors` field is a pure function
  of `gpui_component::ThemeColor` tokens (via `Colorize` helpers where alpha is
  needed). No hardcoded hex/Hsla anywhere in `tokens.rs`. High-contrast
  propagates automatically; A3's gate audits the derived values.
- **`TextRole` carries size + weight + line-height** (Zed-style centralized
  typography map), not size alone — so A6 migrations cannot half-apply the
  ladder.

## 1. `Dat0Colors` + `Dat0Theme`

Plain `struct Dat0Colors { … Hsla … }` computed **on read**:

```rust
pub trait Dat0Theme {
    fn d0(&self) -> Dat0Colors;
}
impl Dat0Theme for gpui_component::Theme { … }
// call shape: cx.theme().d0().focus_ring
```

No second global, no caching, no staleness: a theme switch mutates the single
`gpui_component::Theme` global and the next render recomputes. Construction is
a handful of `Hsla` copies — negligible.

Field map (derivation → the inline-hex site(s) it will replace in A6):

| field | derivation | replaces (A6) |
|---|---|---|
| `focus_ring` | `ring` | `a11y::FOCUS_RING` 0x3b82f6; grid active-cell ring `grid/mod.rs:567` |
| `selection_tint` | `ring.opacity(0.13)` | grid selected-region 0x3b82f622; settings active-nav 0x3b82f622 |
| `fill_handle` | `ring.opacity(0.65)` | grid fill handle 0x3b82f6aa |
| `active_cell_tint` | `ring.opacity(0.07)` | grid active-cell fill 0x3b82f611 |
| `marching_ants` | `success` | grid copy-region dashed border 0x22c55e |
| `null_value_fg` | `muted_foreground` | grid NULL placeholder 0x9ca3af |
| `banner_info` | `info` | banner.rs 0x3b82f6 |
| `banner_warning` | `warning` | banner.rs 0xd97706 |
| `banner_error` | `danger` | banner.rs 0xdc2626 |
| `banner_tint` | `muted_foreground.opacity(0.08)` | banner bg 0x80808014 |
| `hover_tint` | `list_hover` | catalog row hover 0x80808022 ×2 |
| `drag_over` | `drop_target` | window.rs external-paths drop 0x0088ff22 |
| `pipeline_pill` | `ring.opacity(0.25)` | pipeline_bar pill bg 0x3b82f640 ×3 |
| `pipeline_accent` | `primary` | pipeline_bar blue-500 fg |
| `pipeline_chip` | `secondary` | pipeline_bar gray-100 chip bg |
| `text_muted` | `muted_foreground` | pipeline_bar gray-500 ×3; charts/panel 0x888888 |
| `text_error` | `danger` | pipeline_bar red-500; charts/panel 0xcc4444 |
| `chart_placeholder_a` | `chart_2` | charts/mod 0x55bb88 |
| `chart_placeholder_b` | `chart_1` | charts/mod 0x6699cc |
| `pager_dot_active` | `foreground` | onboarding 0xffffff |
| `pager_dot_inactive` | `muted_foreground` | onboarding 0x666666 |

Notes:

- `ring`-family derivations are what actually kills the two-blues split at the
  call sites: A1 set `ring` = #58a6ff (dark) / #0969da (light), so once A6a
  swaps `FOCUS_RING` for `d0().focus_ring`, focus and accent share one hue.
- `a11y/mod.rs` is **not** touched in A2. `FOCUS_RING` stays until A6a (the
  focus trait gains a `ring: Hsla` parameter there, ~15 call sites).
- Alpha constants (0.13/0.25/0.65/0.07/0.08) are eyeball-matched to today's
  values against the dark palette; A3's composited-contrast checks and the
  owed human glance at A6 are the correctness gates for light/HC.

## 2. `Sp` — spacing scale

```rust
pub enum Sp { S1, S2, S4, S6, S8, S12, S16, S24, S32 }   // → px(1|2|4|6|8|12|16|24|32)
```

`impl From<Sp> for Pixels` + `SpStyled: Styled` extension with thin wrappers:
`.p_sp(Sp)`, `.px_sp(Sp)`, `.py_sp(Sp)`, `.gap_sp(Sp)`, `.m_sp(Sp)` (the set
the A6 surfaces actually need; more wrappers added on demand, YAGNI).

## 3. `TextRole` — typography ladder

| role | size px | weight | line-height |
|---|---|---|---|
| `Caption` | 11 | Normal | 1.4 |
| `Small` | 12 | Normal | 1.4 |
| `Body` | 13 | Normal | 1.5 |
| `BodyLg` | 14 | Normal | 1.5 |
| `Title` | 16 | Medium | 1.3 |
| `Display` | 20 | Semibold | 1.2 |

One centralized map fn returning `(Pixels, FontWeight, f32)`;
`TypoStyled::text_role(TextRole)` applies all three via `Styled` (`text_size`,
`font_weight`, `line_height`). Desktop ladder — body 13px against the A1
`font.size` 14 root; the gallery (A4) is where the ladder gets eyeballed.

## 4. `Elevation` — surface ladder

```rust
pub enum Elevation { Background, Surface, Raised, Overlay, Modal }
```

Resolved against `&gpui_component::Theme` to `{ bg, border, radius, shadow }`:

| rung | bg | border | radius | shadow (only if `theme.shadow`) |
|---|---|---|---|---|
| `Background` | `background` | `border` | 0 | none |
| `Surface` | `sidebar` | `sidebar_border` | 0 | none |
| `Raised` | `popover` | `border` | `theme.radius` | sm |
| `Overlay` | `popover` | `border` | `theme.radius` | md |
| `Modal` | `popover` | `border` | `theme.radius_lg` | lg |

Shadows use the gpui `shadow_sm/md/lg` presets, **gated on `theme.shadow`** —
the A1 high-contrast JSON sets `shadow:false`, so HC stays flat and borders
carry the edges (every rung always paints its border). Applied via an
`.elevation(rung, cx)` helper on the `SpStyled`-style extension (takes the
resolved struct; pure function, testable without a window).

Consumers: A6 cards/popovers, B1 `ModalHost` (Modal rung), B2 anchored
overlays (Overlay rung), B3 status bar (Surface rung).

## 5. `Density`

```rust
pub enum Density { Compact, Default, Comfortable }
```

→ `gpui_component::Size`: `Compact→XSmall` (26px table rows), `Default→Medium`
(32), `Comfortable→Large` (40). Plus the policy const fn
`pub fn grid_density() -> Density { Density::Compact }` — the dense-workbench
default the master plan pins (grid rows 26px, applied at A6f via
`Table…with_size`). No global state, no setting (density toggle is post-v1).

## 6. Tests (inline `#[cfg(test)]` in tokens.rs)

All pure — construct `ThemeColor`/`Theme` values from the three builtin
`ThemeConfig`s directly (`crate::theme::builtin_config`), no gpui window:

1. **Derivation propagation**: `Dat0Colors` from dark vs light configs differ
   on every ring/`muted_foreground`-derived field; from the HC config, fields
   equal the HC palette's tokens (proves HC auto-propagation, the reason the
   derived-struct architecture exists).
2. **Exact-value tables**: `Sp` → px, `TextRole` → (size, weight, line-height),
   `Density` → `Size` (+ `Size::table_row_height()` 26/32/40 — pins the
   upstream mapping we rely on).
3. **Elevation shadow gating**: resolving any rung against the HC config
   (`shadow:false`) yields no shadow; against dark, Raised/Overlay/Modal do
   shadow and Background/Surface never do. Radius: Modal uses `radius_lg`.
4. **Zero-literal guard (self-lint)**: a test greps `include_str!` of the
   module source for `rgb(0x` / `rgba(0x` / `parse_hex` — tokens.rs must stay
   literal-free ahead of A4's repo-wide lint.

## 7. Non-goals / invariants

- **No call-site migration** (A6), no contrast-matrix additions (A3), no lint
  or gallery (A4), no icons (A5).
- Zero new dependencies, zero i18n keys, zero schema/session changes, zero
  event-enum changes.
- Full nav/a11y suite untouched and green (`--features a11y-capture`) — tokens
  are invisible to label/focus oracles.
- Table/grid render path byte-identical (nothing consumes the tokens yet) —
  macOS grid-scroll bench unaffected; still watch the post-merge main run per
  standing policy.
- `pub` items in the lib target ⇒ no dead-code warnings for the not-yet-used
  API (verified: `crates/dat0-app/Cargo.toml` has a `[lib]` target).

## 8. Verified API ground truth (pinned rev 0f0ab35, checked 2026-07-23)

- `ThemeColor` fields used here all exist at the pin: `ring`, `selection`,
  `success`, `danger`, `info`, `warning`, `muted_foreground`, `drop_target`,
  `list_hover`, `secondary`, `primary`, `foreground`, `chart_1..5`,
  `sidebar`, `sidebar_border`, `popover`, `background`, `border`.
- `Colorize::opacity(f32)` exists (`crates/ui/src/theme/color.rs`).
- `Size { XSmall, Small, Medium, Large, Size(Pixels) }`;
  `Size::table_row_height()` → 26/30/32/40 (`crates/ui/src/styled.rs:250`).
- `Theme { colors, mode, font_size, radius, radius_lg, shadow }` — `radius_lg`
  and the `shadow` bool exist for the Elevation gating.
