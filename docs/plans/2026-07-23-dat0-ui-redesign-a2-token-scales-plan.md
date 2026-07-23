# UI Redesign Slice A2 — Token scales Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One new purely-additive module `crates/dat0-app/src/theme/tokens.rs` holding every dat0-built design-system scale: `Dat0Colors` (derived on read), `Sp`, `TextRole`, `Elevation`, `Density`, plus helper traits `SpStyled` / `TypoStyled` / `ElevationStyled`.

**Architecture:** All colors are pure functions of the single `gpui_component::Theme` global (strict zero-literal policy) — no second global, no caching, no staleness. Scales are C-like enums with exact-value map functions. Everything is testable without a gpui window: `gpui_component::Theme::default()` + `apply_config(&Rc<ThemeConfig>)` works standalone against the three A1 builtin JSONs.

**Tech Stack:** Rust, gpui 0.2.2 (crates.io), gpui-component pinned rev `0f0ab35` (workspace deps; **no new dependencies**).

**Spec:** `docs/plans/2026-07-23-dat0-ui-redesign-a2-token-scales-design.md` (approved 2026-07-23). Branch: `feat/ui-redesign-a2-token-scales` off main `b474b23`.

## Global Constraints

- **Purely additive**: only `crates/dat0-app/src/theme/tokens.rs` is created and `crates/dat0-app/src/theme/mod.rs` gains one `pub mod tokens;` line. NO other file changes; NO call-site migration (that is Slice A6).
- **Strict zero-literal policy**: no `rgb(`, `rgba(`, `hsla(`, `parse_hex`, or any hardcoded color in tokens.rs — every `Dat0Colors` field derives from `gpui_component::ThemeColor` tokens (Task 5 adds a self-lint test enforcing this).
- Zero new dependencies, zero i18n keys, zero schema/session changes, zero event-enum changes.
- All tests are inline `#[cfg(test)]` unit tests in tokens.rs; run with `cargo test -p dat0-app --lib tokens`. Implementers run ONLY this focused command; the controller runs the full gate at the end (dat0 workflow rule).
- Every commit: `git commit -s` (DCO) + `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` trailer. NEVER write the CI-skip bracket marker anywhere in any commit message (A1 post-merge lesson).
- Verified API ground truth (do not re-derive): `Theme::apply_config(&mut self, &Rc<ThemeConfig>)` needs no App and sets `mode`/`colors`/`shadow`; `Theme` derefs to `ThemeColor`; `Colorize::opacity(factor)` multiplies alpha (`a * factor.clamp(0,1)`); `Size::table_row_height()` → XSmall 26 / Small 30 / Medium 32 / Large 40 px; `FontWeight::{NORMAL, MEDIUM, SEMIBOLD}` consts exist; `Styled` has `.rounded(Pixels)`, `.border_1()`, `.shadow_sm/md/lg()`, `.text_size()`, `.font_weight()`, `.line_height(relative(f32))`, `.p/.px/.py/.gap/.m(Pixels)`.

---

### Task 1: Module skeleton + `Dat0Colors` + `Dat0Theme`

**Files:**
- Create: `crates/dat0-app/src/theme/tokens.rs`
- Modify: `crates/dat0-app/src/theme/mod.rs` (add `pub mod tokens;` after `pub mod contrast;`)
- Test: inline in `crates/dat0-app/src/theme/tokens.rs`

**Interfaces:**
- Consumes: `crate::theme::builtin_config(id: &str) -> Option<&'static ThemeConfig>` (A1 façade, `theme/mod.rs:49`); `gpui_component::{Theme, Colorize}`.
- Produces: `pub struct Dat0Colors { 21 pub Hsla fields }`; `pub trait Dat0Theme { fn d0(&self) -> Dat0Colors }` impl'd for `gpui_component::Theme`; test helper `theme_for(id) -> Theme` (later tasks' tests reuse it).

- [ ] **Step 1: Write the failing tests** — create `crates/dat0-app/src/theme/tokens.rs` with module doc, imports, and the test module only:

```rust
//! dat0 design-system scales (UI-redesign A2, master plan §3/§5).
//!
//! Everything here is a pure function of the single `gpui_component::Theme`
//! global — colors are DERIVED ON READ (`cx.theme().d0().focus_ring`), never
//! cached in a second global, so theme switches can never go stale and the
//! high-contrast palette propagates automatically. Strict zero-literal
//! policy: no color constructors in this file (self-lint test below).

use gpui::{px, relative, FontWeight, Hsla, Pixels, Styled};
use gpui_component::{Colorize as _, Theme};

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    /// Standalone Theme styled by a builtin config — no gpui App needed
    /// (`apply_config` is a plain `&mut self` method, verified at rev 0f0ab35).
    pub(super) fn theme_for(id: &str) -> Theme {
        let cfg = crate::theme::builtin_config(id).expect("builtin theme id");
        let mut theme = Theme::default();
        theme.apply_config(&Rc::new(cfg.clone()));
        theme
    }

    #[test]
    fn dat0_colors_derive_from_active_palette() {
        let dark = theme_for("dark");
        let light = theme_for("light");
        let hc = theme_for("high-contrast");

        // Contract: fields are pure functions of the theme's own tokens.
        assert_eq!(dark.d0().focus_ring, dark.ring);
        assert_eq!(hc.d0().focus_ring, hc.ring);
        assert_eq!(hc.d0().marching_ants, hc.success);
        assert_eq!(hc.d0().null_value_fg, hc.muted_foreground);
        assert_eq!(light.d0().drag_over, light.drop_target);
        assert_eq!(dark.d0().hover_tint, dark.list_hover);
        assert_eq!(dark.d0().chart_placeholder_a, dark.chart_2);
        assert_eq!(dark.d0().chart_placeholder_b, dark.chart_1);

        // The three palettes actually differ (ring: #58a6ff / #0969da /
        // #ffff00) → derived fields differ. Proves apply_config took effect
        // and high-contrast auto-propagates — the reason d0() exists.
        assert_ne!(dark.d0().focus_ring, light.d0().focus_ring);
        assert_ne!(dark.d0().focus_ring, hc.d0().focus_ring);
        assert_ne!(dark.d0().text_muted, light.d0().text_muted);
    }

    #[test]
    fn alpha_tints_scale_the_source_alpha() {
        let dark = theme_for("dark");
        let d0 = dark.d0();
        // Colorize::opacity multiplies alpha and leaves h/s/l untouched.
        assert!((d0.selection_tint.a - dark.ring.a * 0.13).abs() < 1e-4);
        assert!((d0.fill_handle.a - dark.ring.a * 0.65).abs() < 1e-4);
        assert!((d0.active_cell_tint.a - dark.ring.a * 0.07).abs() < 1e-4);
        assert!((d0.pipeline_pill.a - dark.ring.a * 0.25).abs() < 1e-4);
        assert!((d0.banner_tint.a - dark.muted_foreground.a * 0.08).abs() < 1e-4);
        assert_eq!(d0.selection_tint.h, dark.ring.h);
        assert_eq!(d0.selection_tint.l, dark.ring.l);
    }
}
```

- [ ] **Step 2: Wire the module and run tests to verify they fail.** In `crates/dat0-app/src/theme/mod.rs`, after `pub mod contrast;` add:

```rust
pub mod tokens;
```

Run: `cargo test -p dat0-app --lib tokens 2>&1 | tail -20`
Expected: COMPILE ERROR — `no method named d0` / `cannot find struct Dat0Colors` (the tests reference the not-yet-written API).

- [ ] **Step 3: Implement `Dat0Colors` + `Dat0Theme`** — insert between the imports and the test module:

```rust
/// dat0-specific color semantics, derived from the active
/// [`gpui_component::Theme`] every time [`Dat0Theme::d0`] is called.
/// Field-by-field derivation map + the inline-hex call sites each field
/// replaces in Slice A6: design doc §1
/// (`docs/plans/2026-07-23-dat0-ui-redesign-a2-token-scales-design.md`).
#[derive(Debug, Clone, PartialEq)]
pub struct Dat0Colors {
    pub focus_ring: Hsla,
    pub selection_tint: Hsla,
    pub fill_handle: Hsla,
    pub active_cell_tint: Hsla,
    pub marching_ants: Hsla,
    pub null_value_fg: Hsla,
    pub banner_info: Hsla,
    pub banner_warning: Hsla,
    pub banner_error: Hsla,
    pub banner_tint: Hsla,
    pub hover_tint: Hsla,
    pub drag_over: Hsla,
    pub pipeline_pill: Hsla,
    pub pipeline_accent: Hsla,
    pub pipeline_chip: Hsla,
    pub text_muted: Hsla,
    pub text_error: Hsla,
    pub chart_placeholder_a: Hsla,
    pub chart_placeholder_b: Hsla,
    pub pager_dot_active: Hsla,
    pub pager_dot_inactive: Hsla,
}

/// Access trait: `cx.theme().d0().focus_ring`.
pub trait Dat0Theme {
    fn d0(&self) -> Dat0Colors;
}

impl Dat0Theme for Theme {
    fn d0(&self) -> Dat0Colors {
        // `Theme` derefs to `ThemeColor`, so `self.ring` etc. read the
        // active palette. Alpha factors are eyeball-matched to the pre-A6
        // inline values (0x22≈0.13, 0xaa≈0.65, 0x11≈0.07, 0x40=0.25,
        // 0x14≈0.08); the A3 contrast matrix is their correctness gate.
        Dat0Colors {
            focus_ring: self.ring,
            selection_tint: self.ring.opacity(0.13),
            fill_handle: self.ring.opacity(0.65),
            active_cell_tint: self.ring.opacity(0.07),
            marching_ants: self.success,
            null_value_fg: self.muted_foreground,
            banner_info: self.info,
            banner_warning: self.warning,
            banner_error: self.danger,
            banner_tint: self.muted_foreground.opacity(0.08),
            hover_tint: self.list_hover,
            drag_over: self.drop_target,
            pipeline_pill: self.ring.opacity(0.25),
            pipeline_accent: self.primary,
            pipeline_chip: self.secondary,
            text_muted: self.muted_foreground,
            text_error: self.danger,
            chart_placeholder_a: self.chart_2,
            chart_placeholder_b: self.chart_1,
            pager_dot_active: self.foreground,
            pager_dot_inactive: self.muted_foreground,
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p dat0-app --lib tokens 2>&1 | tail -5`
Expected: `test result: ok. 2 passed`
(Unused-import warnings for `px`/`relative`/`FontWeight`/`Pixels`/`Styled` are expected until Tasks 2-4 land — if `cargo` promotes them to errors, keep only `Hsla` + the gpui_component imports now and let each later task add its own.)

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/src/theme/tokens.rs crates/dat0-app/src/theme/mod.rs
git commit -s -m "feat(theme): A2 tokens — Dat0Colors derived-on-read via Dat0Theme

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: `Sp` spacing scale + `SpStyled`

**Files:**
- Modify: `crates/dat0-app/src/theme/tokens.rs`

**Interfaces:**
- Consumes: nothing from other tasks (test helper `tests::theme_for` NOT needed).
- Produces: `pub enum Sp { S1,S2,S4,S6,S8,S12,S16,S24,S32 }` with `pub fn pixels(self) -> Pixels`, `impl From<Sp> for Pixels`; `pub trait SpStyled` blanket-impl'd for all `Styled`.

- [ ] **Step 1: Write the failing test** — add inside `mod tests`:

```rust
    #[test]
    fn sp_scale_exact_values() {
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
            assert_eq!(sp.pixels(), px(v), "{sp:?}");
            assert_eq!(Pixels::from(sp), px(v));
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dat0-app --lib tokens 2>&1 | tail -20`
Expected: COMPILE ERROR — `cannot find type Sp`

- [ ] **Step 3: Implement** — add after the `Dat0Theme` impl:

```rust
/// Spacing scale (px). The ONLY spacing values new dat0 UI should use
/// (master plan §3); A6 migrates magic px call sites onto it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sp {
    S1 = 1,
    S2 = 2,
    S4 = 4,
    S6 = 6,
    S8 = 8,
    S12 = 12,
    S16 = 16,
    S24 = 24,
    S32 = 32,
}

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

/// Spacing helpers so call sites stay terse: `.p_sp(Sp::S8)`.
pub trait SpStyled: Styled + Sized {
    fn p_sp(self, sp: Sp) -> Self {
        self.p(sp.pixels())
    }
    fn px_sp(self, sp: Sp) -> Self {
        self.px(sp.pixels())
    }
    fn py_sp(self, sp: Sp) -> Self {
        self.py(sp.pixels())
    }
    fn gap_sp(self, sp: Sp) -> Self {
        self.gap(sp.pixels())
    }
    fn m_sp(self, sp: Sp) -> Self {
        self.m(sp.pixels())
    }
}

impl<E: Styled> SpStyled for E {}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dat0-app --lib tokens 2>&1 | tail -5`
Expected: `test result: ok. 3 passed`

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/src/theme/tokens.rs
git commit -s -m "feat(theme): A2 tokens — Sp spacing scale + SpStyled helpers

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: `TextRole` typography ladder + `TypoStyled`

**Files:**
- Modify: `crates/dat0-app/src/theme/tokens.rs`

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `pub enum TextRole { Caption, Small, Body, BodyLg, Title, Display }` with `pub fn size(self) -> Pixels`, `pub fn weight(self) -> FontWeight`, `pub fn line_height_factor(self) -> f32`; `pub trait TypoStyled { fn text_role(self, role: TextRole) -> Self }` blanket-impl'd.

- [ ] **Step 1: Write the failing test** — add inside `mod tests`:

```rust
    #[test]
    fn text_role_ladder_exact_values() {
        use TextRole::*;
        let expect = [
            (Caption, 11., FontWeight::NORMAL, 1.4),
            (Small, 12., FontWeight::NORMAL, 1.4),
            (Body, 13., FontWeight::NORMAL, 1.5),
            (BodyLg, 14., FontWeight::NORMAL, 1.5),
            (Title, 16., FontWeight::MEDIUM, 1.3),
            (Display, 20., FontWeight::SEMIBOLD, 1.2),
        ];
        for (role, size, weight, lh) in expect {
            assert_eq!(role.size(), px(size), "{role:?} size");
            assert_eq!(role.weight(), weight, "{role:?} weight");
            assert!((role.line_height_factor() - lh).abs() < f32::EPSILON, "{role:?} line-height");
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dat0-app --lib tokens 2>&1 | tail -20`
Expected: COMPILE ERROR — `cannot find type TextRole`

- [ ] **Step 3: Implement** — add after `impl<E: Styled> SpStyled for E {}`:

```rust
/// Desktop typography ladder (master plan §3 + owner decision 2026-07-23:
/// size + weight + line-height per role, so surfaces can't half-apply the
/// ladder). Body is 13px against the A1 `font.size` 14 root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRole {
    Caption,
    Small,
    Body,
    BodyLg,
    Title,
    Display,
}

impl TextRole {
    pub fn size(self) -> Pixels {
        px(match self {
            TextRole::Caption => 11.,
            TextRole::Small => 12.,
            TextRole::Body => 13.,
            TextRole::BodyLg => 14.,
            TextRole::Title => 16.,
            TextRole::Display => 20.,
        })
    }

    pub fn weight(self) -> FontWeight {
        match self {
            TextRole::Title => FontWeight::MEDIUM,
            TextRole::Display => FontWeight::SEMIBOLD,
            _ => FontWeight::NORMAL,
        }
    }

    /// Line height as a multiple of the role's font size
    /// (`gpui::relative` fraction semantics).
    pub fn line_height_factor(self) -> f32 {
        match self {
            TextRole::Caption | TextRole::Small => 1.4,
            TextRole::Body | TextRole::BodyLg => 1.5,
            TextRole::Title => 1.3,
            TextRole::Display => 1.2,
        }
    }
}

/// `.text_role(TextRole::Title)` applies size + weight + line-height in one
/// call — the centralized map is the point (no per-site weight drift).
pub trait TypoStyled: Styled + Sized {
    fn text_role(self, role: TextRole) -> Self {
        self.text_size(role.size())
            .font_weight(role.weight())
            .line_height(relative(role.line_height_factor()))
    }
}

impl<E: Styled> TypoStyled for E {}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dat0-app --lib tokens 2>&1 | tail -5`
Expected: `test result: ok. 4 passed`

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/src/theme/tokens.rs
git commit -s -m "feat(theme): A2 tokens — TextRole ladder + TypoStyled

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: `Elevation` surface ladder + `ElevationStyled`

**Files:**
- Modify: `crates/dat0-app/src/theme/tokens.rs`

**Interfaces:**
- Consumes: `tests::theme_for` (Task 1).
- Produces: `pub enum Elevation { Background, Surface, Raised, Overlay, Modal }` with `pub fn resolve(self, theme: &Theme) -> ElevationStyle`; `pub enum ShadowLevel { None, Small, Medium, Large }`; `pub struct ElevationStyle { pub bg: Hsla, pub border: Hsla, pub radius: Pixels, pub shadow: ShadowLevel }`; `pub trait ElevationStyled { fn elevation(self, rung: Elevation, theme: &Theme) -> Self }` blanket-impl'd. B1 `ModalHost` uses `Modal`, B2 overlays use `Overlay`, B3 status bar uses `Surface`.

- [ ] **Step 1: Write the failing tests** — add inside `mod tests`:

```rust
    #[test]
    fn elevation_shadow_gates_on_theme_shadow() {
        let dark = theme_for("dark"); // shadow: true (A1 builtin)
        let hc = theme_for("high-contrast"); // shadow: false — HC stays flat
        assert!(dark.shadow && !hc.shadow, "A1 builtin shadow flags moved — update this test's premise");

        assert_eq!(Elevation::Background.resolve(&dark).shadow, ShadowLevel::None);
        assert_eq!(Elevation::Surface.resolve(&dark).shadow, ShadowLevel::None);
        assert_eq!(Elevation::Raised.resolve(&dark).shadow, ShadowLevel::Small);
        assert_eq!(Elevation::Overlay.resolve(&dark).shadow, ShadowLevel::Medium);
        assert_eq!(Elevation::Modal.resolve(&dark).shadow, ShadowLevel::Large);

        for rung in [
            Elevation::Background,
            Elevation::Surface,
            Elevation::Raised,
            Elevation::Overlay,
            Elevation::Modal,
        ] {
            assert_eq!(rung.resolve(&hc).shadow, ShadowLevel::None, "{rung:?} must be flat in HC");
        }
    }

    #[test]
    fn elevation_geometry_and_backgrounds() {
        let dark = theme_for("dark");
        // bg ladder: background → sidebar → popover (A1 palette #0e1116 →
        // #151a21 → #1a2029); floating rungs share popover, differ by
        // shadow strength + radius.
        assert_eq!(Elevation::Background.resolve(&dark).bg, dark.background);
        assert_eq!(Elevation::Surface.resolve(&dark).bg, dark.sidebar);
        assert_eq!(Elevation::Surface.resolve(&dark).border, dark.sidebar_border);
        assert_eq!(Elevation::Raised.resolve(&dark).bg, dark.popover);
        assert_eq!(Elevation::Overlay.resolve(&dark).bg, dark.popover);
        assert_eq!(Elevation::Modal.resolve(&dark).bg, dark.popover);

        assert_eq!(Elevation::Background.resolve(&dark).radius, px(0.));
        assert_eq!(Elevation::Surface.resolve(&dark).radius, px(0.));
        assert_eq!(Elevation::Raised.resolve(&dark).radius, dark.radius);
        assert_eq!(Elevation::Overlay.resolve(&dark).radius, dark.radius);
        assert_eq!(Elevation::Modal.resolve(&dark).radius, dark.radius_lg);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p dat0-app --lib tokens 2>&1 | tail -20`
Expected: COMPILE ERROR — `cannot find type Elevation`

- [ ] **Step 3: Implement** — add after `impl<E: Styled> TypoStyled for E {}`:

```rust
/// Surface-elevation ladder (master plan §3). One enum drives bg + border +
/// radius + shadow TOGETHER (Zed `ElevationIndex` pattern) so surfaces can't
/// mix rungs. Shadows are gated on `theme.shadow` — the A1 high-contrast
/// builtin sets `shadow:false`, so HC stays flat and the always-painted
/// border carries the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevation {
    Background,
    Surface,
    Raised,
    Overlay,
    Modal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowLevel {
    None,
    Small,
    Medium,
    Large,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElevationStyle {
    pub bg: Hsla,
    pub border: Hsla,
    pub radius: Pixels,
    pub shadow: ShadowLevel,
}

impl Elevation {
    /// Pure resolution — testable without a window.
    pub fn resolve(self, theme: &Theme) -> ElevationStyle {
        let gate = |level| if theme.shadow { level } else { ShadowLevel::None };
        match self {
            Elevation::Background => ElevationStyle {
                bg: theme.background,
                border: theme.border,
                radius: px(0.),
                shadow: ShadowLevel::None,
            },
            Elevation::Surface => ElevationStyle {
                bg: theme.sidebar,
                border: theme.sidebar_border,
                radius: px(0.),
                shadow: ShadowLevel::None,
            },
            Elevation::Raised => ElevationStyle {
                bg: theme.popover,
                border: theme.border,
                radius: theme.radius,
                shadow: gate(ShadowLevel::Small),
            },
            Elevation::Overlay => ElevationStyle {
                bg: theme.popover,
                border: theme.border,
                radius: theme.radius,
                shadow: gate(ShadowLevel::Medium),
            },
            Elevation::Modal => ElevationStyle {
                bg: theme.popover,
                border: theme.border,
                radius: theme.radius_lg,
                shadow: gate(ShadowLevel::Large),
            },
        }
    }
}

/// `.elevation(Elevation::Overlay, cx.theme())` — applies the whole resolved
/// rung (bg, border, radius, shadow) in one call.
pub trait ElevationStyled: Styled + Sized {
    fn elevation(self, rung: Elevation, theme: &Theme) -> Self {
        let style = rung.resolve(theme);
        let this = self
            .bg(style.bg)
            .border_1()
            .border_color(style.border)
            .rounded(style.radius);
        match style.shadow {
            ShadowLevel::None => this,
            ShadowLevel::Small => this.shadow_sm(),
            ShadowLevel::Medium => this.shadow_md(),
            ShadowLevel::Large => this.shadow_lg(),
        }
    }
}

impl<E: Styled> ElevationStyled for E {}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p dat0-app --lib tokens 2>&1 | tail -5`
Expected: `test result: ok. 6 passed`

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/src/theme/tokens.rs
git commit -s -m "feat(theme): A2 tokens — Elevation ladder gated on theme.shadow

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: `Density` + `grid_density()` + self-lint test

**Files:**
- Modify: `crates/dat0-app/src/theme/tokens.rs`

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `pub enum Density { Compact, Default, Comfortable }` with `pub fn size(self) -> gpui_component::Size`; `pub fn grid_density() -> Density` (A6f applies it via `Table…with_size(grid_density().size())`); the zero-literal self-lint test.

- [ ] **Step 1: Write the failing tests** — add inside `mod tests`:

```rust
    #[test]
    fn density_maps_to_component_size_row_heights() {
        use gpui_component::Size;
        assert_eq!(Density::Compact.size(), Size::XSmall);
        assert_eq!(Density::Default.size(), Size::Medium);
        assert_eq!(Density::Comfortable.size(), Size::Large);
        // Pin the upstream row heights the dense-workbench policy relies on
        // (styled.rs:250 at rev 0f0ab35) — a rev bump that moves these must
        // fail loudly here, not silently re-density the grid.
        assert_eq!(Density::Compact.size().table_row_height(), px(26.));
        assert_eq!(Density::Default.size().table_row_height(), px(32.));
        assert_eq!(Density::Comfortable.size().table_row_height(), px(40.));
        assert_eq!(grid_density(), Density::Compact);
    }

    #[test]
    fn tokens_module_stays_literal_free() {
        // Zero-literal policy (owner decision 2026-07-23): colors in this
        // module must derive from theme tokens. Patterns are assembled by
        // concatenation so this test can't match itself. Forerunner of the
        // A4 repo-wide style lint.
        let src = include_str!("tokens.rs");
        let banned = [
            format!("rgb{}", "(0x"),
            format!("rgba{}", "(0x"),
            format!("parse{}", "_hex"),
            format!("hsla{}", "("),
            format!("rgb{}", "a("),
        ];
        for pat in &banned {
            assert!(
                !src.contains(pat.as_str()),
                "tokens.rs must stay color-literal-free; found `{pat}`"
            );
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p dat0-app --lib tokens 2>&1 | tail -20`
Expected: COMPILE ERROR — `cannot find type Density`

- [ ] **Step 3: Implement** — add after `impl<E: Styled> ElevationStyled for E {}`:

```rust
/// Global density policy → gpui-component [`Size`](gpui_component::Size).
/// dat0 is a dense data workbench: the grid defaults to Compact (26px table
/// rows). A user-facing density setting is post-v1 (master plan §5 optional).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Density {
    Compact,
    Default,
    Comfortable,
}

impl Density {
    pub fn size(self) -> gpui_component::Size {
        match self {
            Density::Compact => gpui_component::Size::XSmall,
            Density::Default => gpui_component::Size::Medium,
            Density::Comfortable => gpui_component::Size::Large,
        }
    }
}

/// The grid's density policy (applied at A6f via `Table…with_size`).
pub fn grid_density() -> Density {
    Density::Compact
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p dat0-app --lib tokens 2>&1 | tail -5`
Expected: `test result: ok. 8 passed`

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/src/theme/tokens.rs
git commit -s -m "feat(theme): A2 tokens — Density policy + zero-literal self-lint

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Controller final gate (not a subagent task)

After Task 5, the controller (main session) runs the full slice gate:

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` (clippy catches what focused tests miss — carve-out #6 lesson)
- [ ] `cargo test -p dat0-app --lib` (whole lib, not just tokens)
- [ ] `cargo test -p dat0-app --features a11y-capture` (full nav/a11y suite — must be untouched-green; tokens are invisible to label oracles)
- [ ] `git diff b474b23 --stat` shows ONLY `theme/tokens.rs`, `theme/mod.rs` (+1 line), and the two docs/plans files — proves "purely additive"
- [ ] Final whole-branch review (opus) — cross-cutting review sees what per-task reviews can't (transient-bars lesson)

## Suggested per-task models ([[subagent-model-selection]])

T1 sonnet (derivation judgment) · T2 haiku (mechanical) · T3 haiku (mechanical) · T4 sonnet (mapping + gating) · T5 haiku · task reviews sonnet · final whole-branch review opus.
