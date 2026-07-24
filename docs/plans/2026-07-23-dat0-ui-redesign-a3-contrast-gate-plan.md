# A3 Contrast-Gate Matrix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the WCAG contrast gate to the full A3 matrix (24 text + 10 non-text + composited + derived Dat0Colors + drift alarm) over all 3 builtin themes, with the 3 measured palette fixes.

**Architecture:** Pure-math extension of `contrast.rs` (`composite_over` + hard 6-digit assert), then the gate test grows table-driven checks. Derived Dat0Colors values are gated through the REAL `d0()` derivation (`Theme::default()` + `apply_config`, no App context — A2 lesson). Palette tuning lands in the same commit as the check that demands it, so every commit is CI-green.

**Tech Stack:** Rust, gpui 0.2.2 (`Rgba`/`Hsla`), gpui-component pinned rev 0f0ab35 (`Theme`, `ThemeConfig`), serde_json. Zero new dependencies.

**Design doc:** `docs/plans/2026-07-23-dat0-ui-redesign-a3-contrast-gate-design.md` (red-set table + pair rationale live there).

**SDD model map** (per subagent-model-selection): sonnet T1/T2/T4 + task reviews, haiku T3, controller runs T5; opus final whole-branch review.

## Global Constraints

- Branch `feat/ui-redesign-a3-contrast-gate` off main `a2d7361`; design doc already committed (`efdb610`).
- Files touched ONLY: `crates/dat0-app/src/theme/contrast.rs`, `crates/dat0-app/tests/theme_contrast_gate.rs`, `crates/dat0-app/src/theme/builtins/dark.json`, `crates/dat0-app/src/theme/builtins/light.json`, `crates/dat0-app/src/theme/tokens.rs`.
- Zero new deps, zero schema changes, zero i18n keys, zero event variants.
- Every pushed commit green: a check and the palette fix it demands land in the SAME commit (red is a local TDD step only).
- Commits: conventional message, `git commit -s`, end body with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`. NEVER write the CI-skip bracket marker anywhere in any commit message (A1 post-merge saga).
- `border` / `input.border` stay exempt from the gate (WCAG 1.4.11 decorative carve-out, same stance as A1).
- All commands run from repo root `/Users/salar/Projects/dat0`.

---

### Task 1: `contrast.rs` — `composite_over` + 6-digit hardening

**Files:**
- Modify: `crates/dat0-app/src/theme/contrast.rs` (65 lines total today)

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub fn composite_over(fg: &str, bg: &str) -> String` — `#rrggbbaa` fg source-over-composited onto opaque `#rrggbb` bg → `#rrggbb`; 6-digit fg passes through unchanged. `relative_luminance`/`contrast_ratio` now PANIC on non-6-digit hex payloads (message contains "use composite_over"). Tasks 2–4 rely on both behaviors.

- [ ] **Step 1: Write the failing tests** — append inside the existing `mod tests` in `contrast.rs`:

```rust
    #[test]
    fn composite_alpha_extremes_and_passthrough() {
        // α=0x00 → pure bg; α=0xff → pure fg; 6-digit fg → identity.
        assert_eq!(composite_over("#58a6ff00", "#0e1116"), "#0e1116");
        assert_eq!(composite_over("#58a6ffff", "#0e1116"), "#58a6ff");
        assert_eq!(composite_over("#58a6ff", "#0e1116"), "#58a6ff");
    }

    #[test]
    fn composite_known_selection_tints() {
        // Hand-computed source-over vectors (design doc red-set section):
        // dark selection.background over dark table.background,
        // light selection.background over light table.background.
        assert_eq!(composite_over("#58a6ff4d", "#0e1116"), "#243e5c");
        assert_eq!(composite_over("#0969da33", "#ffffff"), "#cee1f8");
    }

    #[test]
    #[should_panic(expected = "use composite_over")]
    fn contrast_ratio_rejects_alpha_hex() {
        // Pre-A3 this silently sliced the first 6 digits and read the color
        // as opaque — a latent false-pass. Now it must be loud.
        contrast_ratio("#58a6ff4d", "#0e1116");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p dat0-app --lib theme::contrast -- --nocapture`
Expected: compile FAIL — `cannot find function composite_over in this scope` (the should_panic test would also fail today because nothing panics).

- [ ] **Step 3: Implement** — in `contrast.rs`, add after `contrast_ratio` and harden `relative_luminance`:

Replace the body opening of `relative_luminance` (the `let h = ...` line and the three parse lines stay, add the assert between them):

```rust
pub fn relative_luminance(hex: &str) -> f64 {
    let h = hex.trim_start_matches('#');
    assert!(
        h.len() == 6,
        "relative_luminance needs opaque #rrggbb; use composite_over for alpha colors (got {hex})"
    );
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0);
    // ... rest of the function unchanged
```

Add the new function:

```rust
/// Source-over composite of `#rrggbbaa` fg onto an opaque `#rrggbb` bg,
/// returning the effective opaque `#rrggbb`. A 6-digit fg passes through
/// unchanged, so call sites need not care whether a token is tinted.
pub fn composite_over(fg: &str, bg: &str) -> String {
    let f = fg.trim_start_matches('#');
    if f.len() == 6 {
        return format!("#{f}");
    }
    assert!(f.len() == 8, "composite_over fg must be #rrggbb or #rrggbbaa (got {fg})");
    let b = bg.trim_start_matches('#');
    assert!(b.len() == 6, "composite_over bg must be opaque #rrggbb (got {bg})");
    let a = u8::from_str_radix(&f[6..8], 16).unwrap_or(0) as f64 / 255.0;
    let ch = |i: usize| {
        let fc = u8::from_str_radix(&f[i..i + 2], 16).unwrap_or(0) as f64;
        let bc = u8::from_str_radix(&b[i..i + 2], 16).unwrap_or(0) as f64;
        (fc * a + bc * (1.0 - a)).round() as u8
    };
    format!("#{:02x}{:02x}{:02x}", ch(0), ch(2), ch(4))
}
```

Also update the module doc comment first line to mention compositing, e.g. append: `Slice A3 adds source-over compositing for 8-digit tinted tokens.`

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p dat0-app --lib theme::contrast -- --nocapture`
Expected: PASS — 6 tests (3 pre-existing + 3 new). The pre-existing `black_on_white_is_21` / `known_pairs_match_reference` MUST still pass (6-digit path unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/src/theme/contrast.rs
git commit -s -m "feat(theme): contrast.rs composite_over + reject alpha hex in luminance (A3)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Gate matrix — text + non-text + drift alarm, with the 2 JSON palette fixes

**Files:**
- Modify: `crates/dat0-app/tests/theme_contrast_gate.rs` (full rewrite of the test body, keep header comment style)
- Modify: `crates/dat0-app/src/theme/builtins/dark.json` (one value)
- Modify: `crates/dat0-app/src/theme/builtins/light.json` (one value)

**Interfaces:**
- Consumes: `dat0_app::theme::contrast::contrast_ratio` (Task 1 assert behavior).
- Produces: `BUILTIN_SOURCES`, `colors_of()`, `color()`, `check_pairs()`, `TEXT_PAIRS` — Tasks 3–4 add more test fns to this same file and reuse all of these. Exact signatures below.

- [ ] **Step 1: Rewrite `tests/theme_contrast_gate.rs`** with the table-driven matrix:

```rust
//! WCAG AA contrast gate over the three builtin theme configs (P10b a11y,
//! retargeted by UI-redesign A1, extended to the full matrix by A3).
//!
//! Matrix: 24 text pairs ≥4.5:1, 10 non-text pairs ≥3:1 (WCAG 1.4.11),
//! composited tint checks, derived Dat0Colors checks, and a sibling-pair
//! drift alarm so new `X.foreground`/`X.background` families cannot dodge
//! the gate. `border`/`input.border` stay exempt (decorative; WCAG 1.4.11
//! carve-out). Values are read through serde serialization of the parsed
//! `ThemeConfigColors`, so the gate uses the exact rename keys and survives
//! field renames in the Rust struct.

use dat0_app::theme::contrast::contrast_ratio;
use gpui_component::ThemeConfig;

const BUILTIN_SOURCES: [(&str, &str); 3] = [
    ("dark", include_str!("../src/theme/builtins/dark.json")),
    ("light", include_str!("../src/theme/builtins/light.json")),
    (
        "high-contrast",
        include_str!("../src/theme/builtins/high-contrast.json"),
    ),
];

/// Parse a builtin JSON and serialize its colors back to a JSON object,
/// so lookups use the exact serde rename keys.
fn colors_of(json: &str) -> serde_json::Value {
    let cfg: ThemeConfig = serde_json::from_str(json).expect("builtin parses");
    serde_json::to_value(&cfg.colors).expect("colors serialize")
}

fn color(colors: &serde_json::Value, key: &str) -> String {
    colors[key]
        .as_str()
        .unwrap_or_else(|| {
            panic!("color key {key} missing/null (coverage gate should have caught this)")
        })
        .to_string()
}

/// Text pairs: (fg_key, bg_key, min_ratio). WCAG 1.4.3 AA = 4.5:1.
/// Covers ALL 18 sibling `X.foreground`/`X.background` families (enforced
/// by `sibling_pairs_all_gated`) plus 6 cross-family pairs.
const TEXT_PAIRS: &[(&str, &str, f64)] = &[
    ("foreground", "background", 4.5),
    ("muted.foreground", "muted.background", 4.5),
    ("muted.foreground", "background", 4.5),
    ("accent.foreground", "accent.background", 4.5),
    ("secondary.foreground", "secondary.background", 4.5),
    ("primary.foreground", "primary.background", 4.5),
    ("danger.foreground", "danger.background", 4.5),
    ("success.foreground", "success.background", 4.5),
    ("warning.foreground", "warning.background", 4.5),
    ("info.foreground", "info.background", 4.5),
    ("popover.foreground", "popover.background", 4.5),
    ("sidebar.foreground", "sidebar.background", 4.5),
    ("sidebar.accent.foreground", "sidebar.accent.background", 4.5),
    ("sidebar.primary.foreground", "sidebar.primary.background", 4.5),
    ("group_box.foreground", "group_box.background", 4.5),
    ("group_box.title.foreground", "group_box.background", 4.5),
    ("tab.foreground", "tab.background", 4.5),
    ("tab.active.foreground", "tab.active.background", 4.5),
    ("table.head.foreground", "table.head.background", 4.5),
    (
        "description_list.label.foreground",
        "description_list.label.background",
        4.5,
    ),
    ("link", "background", 4.5),
    ("foreground", "list.active.background", 4.5),
    ("foreground", "list.hover.background", 4.5),
    ("foreground", "table.even.background", 4.5),
];

/// Non-text pairs (WCAG 1.4.11 = 3:1). `ring` is held at 4.5 — it doubles
/// as the single accent since A1 killed the two-blues split.
const NON_TEXT_PAIRS: &[(&str, &str, f64)] = &[
    ("ring", "background", 4.5),
    ("caret", "background", 3.0),
    ("drag.border", "background", 3.0),
    ("list.active.border", "list.active.background", 3.0),
    ("table.active.border", "table.active.background", 3.0),
    ("danger.background", "background", 3.0),
    ("success.background", "background", 3.0),
    ("warning.background", "background", 3.0),
    ("info.background", "background", 3.0),
    ("primary.background", "background", 3.0),
];

fn check_pairs(
    name: &str,
    colors: &serde_json::Value,
    pairs: &[(&str, &str, f64)],
    failures: &mut Vec<String>,
) {
    for (fg_key, bg_key, min) in pairs {
        let fg = color(colors, fg_key);
        let bg = color(colors, bg_key);
        let r = contrast_ratio(&fg, &bg);
        eprintln!("{name}: {fg_key}/{bg_key} = {r:.2}:1 (min {min})");
        if r < *min {
            failures.push(format!("{name}: {fg_key}/{bg_key} = {r:.2}:1 < {min}"));
        }
    }
}

#[test]
fn text_pairs_meet_wcag_aa() {
    let mut failures = vec![];
    for (name, json) in BUILTIN_SOURCES {
        check_pairs(name, &colors_of(json), TEXT_PAIRS, &mut failures);
    }
    assert!(
        failures.is_empty(),
        "WCAG AA text-contrast failures:\n{}",
        failures.join("\n")
    );
}

#[test]
fn non_text_pairs_meet_wcag_1_4_11() {
    let mut failures = vec![];
    for (name, json) in BUILTIN_SOURCES {
        check_pairs(name, &colors_of(json), NON_TEXT_PAIRS, &mut failures);
    }
    assert!(
        failures.is_empty(),
        "WCAG 1.4.11 non-text-contrast failures:\n{}",
        failures.join("\n")
    );
}

/// Drift alarm: every sibling `X.foreground`/`X.background` family present
/// in the JSON must be listed in TEXT_PAIRS — new families can't silently
/// skip the gate. (Root `foreground`/`background` is a family too.)
#[test]
fn sibling_pairs_all_gated() {
    for (name, json) in BUILTIN_SOURCES {
        let colors = colors_of(json);
        let obj = colors.as_object().expect("colors is an object");
        for key in obj.keys() {
            let bg_key = if key == "foreground" {
                "background".to_string()
            } else if let Some(prefix) = key.strip_suffix(".foreground") {
                format!("{prefix}.background")
            } else {
                continue;
            };
            if !obj.contains_key(&bg_key) {
                continue; // no sibling background — not a fg/bg family
            }
            assert!(
                TEXT_PAIRS
                    .iter()
                    .any(|&(f, b, _)| f == key.as_str() && b == bg_key),
                "{name}: sibling pair ({key}, {bg_key}) missing from TEXT_PAIRS — \
                 add it with a threshold"
            );
        }
    }
}
```

(`composite_over` is deliberately NOT imported yet — Task 3 changes the
first `use` line to `use dat0_app::theme::contrast::{composite_over,
contrast_ratio};` when it first needs it, keeping this intermediate state
clippy-clean.)

- [ ] **Step 2: Run the gate — verify it fails RED on exactly the 2 measured pairs**

Run: `cargo test -p dat0-app --test theme_contrast_gate -- --nocapture`
Expected: FAIL with exactly two entries (this proves the gate bites):
- `dark: danger.foreground/danger.background = 3.35:1 < 4.5`
- `light: muted.foreground/muted.background = 4.50:1 < 4.5`
`non_text_pairs_meet_wcag_1_4_11` and `sibling_pairs_all_gated` PASS.
If anything ELSE is red, STOP — the design's measured red set is wrong; report instead of tuning blind.

- [ ] **Step 3: Apply the two approved palette fixes**

In `crates/dat0-app/src/theme/builtins/dark.json`, change the single line:

```json
    "danger.foreground": "#ffffff",
```

to:

```json
    "danger.foreground": "#0e1116",
```

(Owner-approved flip: dark-on-red 5.64:1, consistent with success/warning dark-on-vivid; hover/active share the key.)

In `crates/dat0-app/src/theme/builtins/light.json`, change the single line:

```json
    "muted.foreground": "#656d76",
```

to:

```json
    "muted.foreground": "#5f6771",
```

(One-shade darken: 4.91:1 on muted.background, 5.73:1 on white.)

- [ ] **Step 4: Run the gate again — verify GREEN**

Run: `cargo test -p dat0-app --test theme_contrast_gate -- --nocapture`
Expected: PASS, all 3 test fns. Then confirm the rest of the theme plumbing still passes:

Run: `cargo test -p dat0-app --test theme && cargo test -p dat0-app --lib theme::`
Expected: PASS (builtins still parse; tokens derivation tests unaffected — neither key feeds a `d0()` derivation except `muted.foreground`, which is derived by REFERENCE not value).

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/tests/theme_contrast_gate.rs \
        crates/dat0-app/src/theme/builtins/dark.json \
        crates/dat0-app/src/theme/builtins/light.json
git commit -s -m "feat(theme): A3 gate matrix — 24 text + 10 non-text pairs + drift alarm; tune dark danger fg + light muted fg

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Composited JSON tint checks

**Files:**
- Modify: `crates/dat0-app/tests/theme_contrast_gate.rs` (append one test fn; extend the `use` to include `composite_over` if Task 2 dropped it)

**Interfaces:**
- Consumes: `composite_over` (Task 1), `BUILTIN_SOURCES`/`colors_of`/`color` (Task 2).
- Produces: nothing downstream.

- [ ] **Step 1: Append the test** — first change the file's opening import to:

```rust
use dat0_app::theme::contrast::{composite_over, contrast_ratio};
```

then append:

```rust
/// Tinted (8-digit) JSON tokens: text must stay readable THROUGH the tint,
/// i.e. against the source-over-composited effective color.
/// `scrollbar.background` is intentionally fully transparent (α=0) and has
/// no text on it — deliberately unchecked.
#[test]
fn composited_tints_keep_text_readable() {
    let mut failures = vec![];
    for (name, json) in BUILTIN_SOURCES {
        let colors = colors_of(json);
        let fg = color(&colors, "foreground");
        for (tint_key, base_key) in [
            ("selection.background", "table.background"),
            ("drop_target.background", "background"),
        ] {
            let eff = composite_over(&color(&colors, tint_key), &color(&colors, base_key));
            let r = contrast_ratio(&fg, &eff);
            eprintln!("{name}: foreground over {tint_key}∘{base_key} ({eff}) = {r:.2}:1 (min 4.5)");
            if r < 4.5 {
                failures.push(format!(
                    "{name}: foreground over {tint_key}∘{base_key} ({eff}) = {r:.2}:1 < 4.5"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "composited-tint contrast failures:\n{}",
        failures.join("\n")
    );
}
```

- [ ] **Step 2: Run — expect immediate PASS with sane printed values**

Run: `cargo test -p dat0-app --test theme_contrast_gate composited_tints -- --nocapture`
Expected: PASS. Printed ratios ≈ dark 7.09/10.63, light 11.85/13.71, high-contrast 6.06/13.01 (±0.05 for rounding). If any value differs wildly, the compositing base key is wrong — stop and re-check against the design doc.

- [ ] **Step 3: Commit**

```bash
git add crates/dat0-app/tests/theme_contrast_gate.rs
git commit -s -m "test(theme): A3 composited selection/drop-target tint checks

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Derived Dat0Colors gate + fill_handle alpha 0.65 → 0.72

**Files:**
- Modify: `crates/dat0-app/tests/theme_contrast_gate.rs` (append helpers + one test fn)
- Modify: `crates/dat0-app/src/theme/tokens.rs:56` (fill_handle alpha), `:51` area (comment), `:357` (A2 alpha assertion)

**Interfaces:**
- Consumes: `dat0_app::theme::builtin_config(id) -> Option<&'static ThemeConfig>`; `dat0_app::theme::tokens::Dat0Theme` (`.d0()`); `gpui_component::Theme` (`Theme::default()` + `apply_config(&Rc<ThemeConfig>)` — works WITHOUT App context, A2-verified); `gpui::{Hsla, Rgba}` (`Rgba: From<Hsla>`, fields `r/g/b/a: f32` in 0..1); Task 1/2 helpers.
- Produces: nothing downstream.

- [ ] **Step 1: Append helpers + failing test** to `tests/theme_contrast_gate.rs`:

```rust
use dat0_app::theme::tokens::Dat0Theme;
use std::rc::Rc;

/// Standalone Theme styled by a builtin config — no gpui App needed
/// (`apply_config` is a plain `&mut self` method, A2-verified at rev 0f0ab35).
fn theme_for(id: &str) -> gpui_component::Theme {
    let cfg = dat0_app::theme::builtin_config(id)
        .expect("builtin theme id")
        .clone();
    let mut theme = gpui_component::Theme::default();
    theme.apply_config(&Rc::new(cfg));
    theme
}

/// Hsla → `#rrggbbaa` so every derived value flows through composite_over
/// uniformly (solid colors carry α=ff and composite to identity).
fn hex8(c: gpui::Hsla) -> String {
    let rgba: gpui::Rgba = c.into();
    let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        ch(rgba.r),
        ch(rgba.g),
        ch(rgba.b),
        ch(rgba.a)
    )
}

/// The A2 alpha factors' promised correctness gate (tokens.rs derivation
/// comment): derived tints checked against their REAL composited values.
#[test]
fn derived_dat0_colors_meet_wcag() {
    let mut failures = vec![];
    for (name, json) in BUILTIN_SOURCES {
        let colors = colors_of(json);
        let d0 = theme_for(name).d0();
        let bg = color(&colors, "background");
        let table_bg = color(&colors, "table.background");
        let fg = color(&colors, "foreground");

        let mut check = |label: &str, r: f64, min: f64| {
            eprintln!("{name}: {label} = {r:.2}:1 (min {min})");
            if r < min {
                failures.push(format!("{name}: {label} = {r:.2}:1 < {min}"));
            }
        };

        // Text stays readable through grid tints (4.5:1).
        let sel = composite_over(&hex8(d0.selection_tint), &table_bg);
        check("fg over selection_tint∘table.bg", contrast_ratio(&fg, &sel), 4.5);
        let cell = composite_over(&hex8(d0.active_cell_tint), &table_bg);
        check("fg over active_cell_tint∘table.bg", contrast_ratio(&fg, &cell), 4.5);
        let banner = composite_over(&hex8(d0.banner_tint), &bg);
        check("fg over banner_tint∘bg", contrast_ratio(&fg, &banner), 4.5);
        let pill = composite_over(&hex8(d0.pipeline_pill), &bg);
        check("fg over pipeline_pill∘bg", contrast_ratio(&fg, &pill), 4.5);

        // Non-text indicators distinguishable from their surface (3:1).
        let handle = composite_over(&hex8(d0.fill_handle), &table_bg);
        check(
            "fill_handle∘table.bg vs table.bg",
            contrast_ratio(&handle, &table_bg),
            3.0,
        );
        let ants = composite_over(&hex8(d0.marching_ants), &table_bg);
        check(
            "marching_ants vs table.bg",
            contrast_ratio(&ants, &table_bg),
            3.0,
        );
    }
    assert!(
        failures.is_empty(),
        "derived Dat0Colors contrast failures:\n{}",
        failures.join("\n")
    );
}
```

Note on pill text: `foreground` deliberately (design doc §2d). If A6f puts
`text_muted` on pills, the pair gets added THEN (dark measures ≈4.0 → forces
an explicit decision at migration time).

- [ ] **Step 2: Run — verify RED on exactly light fill_handle**

Run: `cargo test -p dat0-app --test theme_contrast_gate derived_dat0 -- --nocapture`
Expected: FAIL with exactly one entry:
`light: fill_handle∘table.bg vs table.bg = 2.79:1 < 3` (±0.05 for Hsla round-trip rounding).
All other printed ratios comfortably green (dark fill_handle ≈3.80, HC ≈8.08). Anything else red → STOP and report.

- [ ] **Step 3: Tune the alpha in `tokens.rs`**

Line 56, change:

```rust
            fill_handle: self.ring.opacity(0.65),
```

to:

```rust
            fill_handle: self.ring.opacity(0.72),
```

Derivation comment (lines 50–52), update the fill_handle entry — change `0xaa≈0.65` to `0xaa≈0.65→0.72 (A3: light-theme 3:1)` keeping the rest verbatim:

```rust
        // active palette. Alpha factors are eyeball-matched to the pre-A6
        // inline values (0x22≈0.13, 0xaa≈0.65→0.72 (A3: light-theme 3:1),
        // 0x11≈0.07, 0x40=0.25, 0x14≈0.08); the A3 contrast matrix is their
        // correctness gate.
```

A2 alpha assertion in `mod tests` (line 357), change:

```rust
        assert!((d0.fill_handle.a - dark.ring.a * 0.65).abs() < 1e-4);
```

to:

```rust
        assert!((d0.fill_handle.a - dark.ring.a * 0.72).abs() < 1e-4);
```

- [ ] **Step 4: Run — verify GREEN, including the A2 suites**

Run: `cargo test -p dat0-app --test theme_contrast_gate -- --nocapture`
Expected: PASS all 5 test fns; fill_handle prints ≈ light 3.17 / dark 4.38 / HC 9.89.

Run: `cargo test -p dat0-app --lib theme:: && cargo test -p dat0-app --test theme`
Expected: PASS (tokens.rs unit tests incl. the retuned 0.72 assertion + self-lint).

- [ ] **Step 5: Commit**

```bash
git add crates/dat0-app/tests/theme_contrast_gate.rs crates/dat0-app/src/theme/tokens.rs
git commit -s -m "feat(theme): A3 derived Dat0Colors gate; fill_handle alpha 0.65->0.72 for light 3:1

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5 (controller): Full local gate + PR

**Files:** none (verification + PR only).

- [ ] **Step 1: Full workspace gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p dat0-app
```

Expected: all green. `cargo test -p dat0-app` runs the full integration suite (a11y-capture auto-on via self-dev-dependency — the nav/a11y suites assert LABELS not colors, so the palette edits must be invisible to them; any a11y failure here means something unexpected happened → investigate, don't patch).

- [ ] **Step 2: Diff sanity** — confirm the change surface matches the design:

```bash
git diff a2d7361 --stat
```

Expected: exactly 6 files — the 5 in Global Constraints + this plan/design doc pair. No src file outside `theme/`.

- [ ] **Step 3: Push + PR**

```bash
git push -u origin feat/ui-redesign-a3-contrast-gate
gh pr create --title "feat(theme): A3 contrast-gate matrix (UI redesign)" --body "$(cat <<'EOF'
UI-redesign Slice A3 (design: docs/plans/2026-07-23-dat0-ui-redesign-a3-contrast-gate-design.md).

- contrast.rs: composite_over (source-over for #rrggbbaa) + hard assert kills the silent alpha-slice false-pass
- Gate matrix over all 3 builtin themes: 24 text pairs >=4.5:1, 10 non-text >=3:1, composited selection/drop-target checks, derived Dat0Colors alpha-factor gate, sibling-pair drift alarm
- Palette tuning (measured red set): dark danger.foreground -> #0e1116 (5.64:1, owner-approved), light muted.foreground -> #5f6771 (4.91:1), fill_handle alpha 0.65 -> 0.72 (light 3.17:1)
- Zero deps / schema / i18n / events; Table+delegate untouched

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

- [ ] **Step 4: Poll CI** — `gh pr checks <n> --watch` is banned by workflow lore; poll instead:

```bash
gh pr checks feat/ui-redesign-a3-contrast-gate
```

Repeat until both platform legs green. Then hand to user for merge. Remind at merge time: **explicit `--body` on the squash-merge** (A1 skip-ci saga), then WATCH the post-merge main run (macOS grid-scroll bench is push-to-main-only → silent-red risk) + crash-e2e.

---

## Post-merge bookkeeping (controller, after user merges)

- Verify post-merge main run: all 7 jobs + macOS bench artifact + crash-e2e.
- Update memory: `dat0_ui_redesign.md` A3 closed; owed human glances += dark danger button (dark-on-red) + light fill-handle stronger tint; NEXT = A4 (style_lint ratchet + gallery example).
