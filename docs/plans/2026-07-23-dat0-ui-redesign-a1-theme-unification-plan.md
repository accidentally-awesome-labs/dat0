# Slice A1 — Theme Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make theme switching actually restyle the app — `gpui_component::Theme` becomes the single color source of truth, fed by three full-coverage `ThemeConfig` JSONs; dat0's `Theme` global shrinks to an `{id, mode}` façade.

**Architecture:** Two-phase swap. Phase 1 (Tasks 1–2): the three builtin JSONs become *hybrid* documents — full `ThemeConfig` fields **plus** the legacy `appearance`/`style` block — so old and new parsers both read them and the suite stays green at every commit; a new two-way coverage gate locks in full key coverage. Phase 2 (Tasks 3–4): the façade rewrite lands (spike-proven `apply_config` wiring), `zed_schema.rs` dies, the legacy block is stripped, and the live-switch round-trip test proves production `install`/`switch` restyles `cx.theme()`.

**Tech Stack:** Rust, gpui 0.2.2, gpui-component pinned rev `0f0ab35` (checkout: `~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/0f0ab35/`), serde/serde_json.

**Design:** `docs/plans/2026-07-23-dat0-ui-redesign-a1-theme-unification-design.md` (approved). Parent: master plan §5 A1 row.

## Global Constraints

- Branch: `feat/ui-redesign-a1-theme-unification`, base main `c724957`. Design doc already committed (`17d9317`).
- **Zero new dependencies.** `gpui-component-assets` is slice A5, NOT here. Zero i18n keys, zero session-schema/event changes.
- **Palette anchors are immutable** (owner-approved): dark bg `#0e1116` · surface `#151a21` · popover `#1a2029` · primary `#316dca` · ring `#58a6ff`; light primary+ring `#0969da`; high-contrast black/white/yellow. All three configs: `"font.size": 14`, `"radius": 5`; `"shadow"`: true/true/false (HC flat). Non-anchor values may be tuned only to satisfy the contrast gate.
- **Never call `gpui_component::Theme::change`** — it re-applies from stored light/dark slots and clobbers high-contrast (master plan §4).
- Verified rev facts you may rely on: `apply_config` sets `theme.mode` itself and stores the config into its matching slot; sparse color keys fall back to shadcn defaults (the leak the coverage gate kills); `Hsla::parse_hex` accepts 6- AND 8-digit hex (`color.rs:150`); `ThemeConfig`/`ThemeConfigColors` are `#[serde(default)]`, all color fields `Option<SharedString>`, `Serialize` with **no** `skip_serializing_if`; `ThemeMode` is `Copy`, serde `snake_case` (`"dark"`/`"light"`); serde ignores unknown keys (no `deny_unknown_fields`) — hence the two-way key check.
- Every commit: `git commit -s` (DCO) and end the message with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- Task implementers run ONLY their focused test targets (listed per task). The controller runs the full workspace gate between tasks and at the end (`cargo test --workspace` — the a11y-capture feature auto-activates for integration tests via the self-dev-dependency, no flag needed — plus `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings`).
- Model per task shape ([[subagent-model-selection]]): Tasks 1–4 sonnet (impl + per-task review); final whole-branch review opus (transient-bars lesson: only the cross-cutting review catches ladder-class bugs).

---

### Task 1: Full-coverage palettes + coverage gate (hybrid JSONs)

**Files:**
- Modify: `crates/dat0-app/src/theme/builtins/dark.json` (full replace)
- Modify: `crates/dat0-app/src/theme/builtins/light.json` (full replace)
- Modify: `crates/dat0-app/src/theme/builtins/high-contrast.json` (full replace)
- Modify: `crates/dat0-app/tests/theme.rs` (append 4 tests; do NOT touch the existing 4)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: the three hybrid JSON documents (legacy `appearance` + `style` block retained verbatim from today's files, so `Theme::load_builtin` and the legacy contrast gate stay green until Task 3 strips them) and the coverage-gate tests later tasks keep green.

- [ ] **Step 1: Append the coverage-gate tests to `tests/theme.rs`**

Append below the existing tests (leave the existing `use dat0_app::theme::Theme;` and 4 tests untouched — Task 3 rewrites them):

```rust
// ---------------------------------------------------------------------------
// UI-redesign A1: the builtins are gpui_component::ThemeConfig documents and
// must specify EVERY color key. Sparse keys fall back to shadcn defaults at
// rev 0f0ab35 (NOT to other keys in the file) — the A0 spike showed that leak
// producing an illegible high-contrast theme. Serialize-side fact that makes
// this checkable without a hand-maintained key list: ThemeConfigColors derives
// Serialize with no skip_serializing_if, so serializing a parsed config emits
// every field, None as null.
// ---------------------------------------------------------------------------

use gpui_component::ThemeConfig;
use std::collections::BTreeSet;

const BUILTIN_SOURCES: [(&str, &str); 3] = [
    ("dark", include_str!("../src/theme/builtins/dark.json")),
    ("light", include_str!("../src/theme/builtins/light.json")),
    (
        "high-contrast",
        include_str!("../src/theme/builtins/high-contrast.json"),
    ),
];

#[test]
fn builtin_configs_parse_as_theme_config() {
    for (name, json) in BUILTIN_SOURCES {
        let cfg: ThemeConfig = serde_json::from_str(json)
            .unwrap_or_else(|e| panic!("{name}.json must parse as ThemeConfig: {e}"));
        assert_eq!(cfg.name.as_ref(), name, "name field must match the id");
    }
}

#[test]
fn builtin_configs_specify_every_color_key() {
    for (name, json) in BUILTIN_SOURCES {
        let cfg: ThemeConfig = serde_json::from_str(json).expect(name);
        let canonical = serde_json::to_value(&cfg.colors).expect("colors serialize");
        let obj = canonical.as_object().expect("colors serializes to an object");
        let missing: Vec<&String> = obj
            .iter()
            .filter(|(_, v)| v.is_null())
            .map(|(k, _)| k)
            .collect();
        assert!(
            missing.is_empty(),
            "{name}.json is missing {} color keys (shadcn-default leak): {missing:?}",
            missing.len()
        );
    }
}

#[test]
fn builtin_configs_have_no_unknown_color_keys() {
    // serde silently ignores unknown keys — a typo'd key would otherwise
    // no-op AND pass the null check above (its real key would leak).
    for (name, json) in BUILTIN_SOURCES {
        let cfg: ThemeConfig = serde_json::from_str(json).expect(name);
        let canonical: BTreeSet<String> = serde_json::to_value(&cfg.colors)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        let raw: serde_json::Value = serde_json::from_str(json).unwrap();
        let file_keys: BTreeSet<String> = raw["colors"]
            .as_object()
            .expect("colors object in file")
            .keys()
            .cloned()
            .collect();
        let unknown: Vec<&String> = file_keys.difference(&canonical).collect();
        assert!(
            unknown.is_empty(),
            "{name}.json has color keys serde would silently ignore: {unknown:?}"
        );
    }
}

#[test]
fn builtin_configs_pin_font_radius_shadow() {
    for (name, json) in BUILTIN_SOURCES {
        let cfg: ThemeConfig = serde_json::from_str(json).expect(name);
        assert_eq!(cfg.font_size, Some(14.0), "{name}: font.size 14 (A0 verdict)");
        assert_eq!(cfg.radius, Some(5), "{name}: radius 5 (A0 spike value)");
        let expect_shadow = name != "high-contrast";
        assert_eq!(
            cfg.shadow,
            Some(expect_shadow),
            "{name}: shadow (high-contrast is flat)"
        );
    }
}
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run: `cargo test -p dat0-app --test theme`
Expected: the 4 existing tests PASS, and `builtin_configs_parse_as_theme_config` also passes (ThemeConfig is `#[serde(default)]` — today's 7-token files "parse" as an all-None config with a matching `name`). The REAL failures: `builtin_configs_specify_every_color_key` lists ~109 missing keys per file, `builtin_configs_pin_font_radius_shadow` fails on `font.size`, and `builtin_configs_have_no_unknown_color_keys` panics on the missing `colors` object. Three failing tests = the gate works.

- [ ] **Step 3: Replace `dark.json` with the hybrid full-coverage document**

Full new content of `crates/dat0-app/src/theme/builtins/dark.json`. The `appearance` + `style` block is TODAY's file content verbatim (legacy parser + legacy contrast gate keep passing until Task 3). Identity: refined GitHub dark. Ladder: bg `#0e1116` → even-row `#12161c` → surface `#151a21` → popover/hover `#1a2029` → active/accent `#1e2836` → muted `#21262e` → border `#2d333b` → strong border `#3d444d`; muted fg `#8b949e`.

```json
{
  "name": "dark",
  "appearance": "dark",
  "style": {
    "background": "#0e1116",
    "foreground": "#c9d1d9",
    "border": "#30363d",
    "accent": "#58a6ff",
    "error": "#f85149",
    "success": "#3fb950",
    "warning": "#d29922"
  },
  "mode": "dark",
  "font.size": 14,
  "radius": 5,
  "shadow": true,
  "colors": {
    "background": "#0e1116",
    "foreground": "#c9d1d9",
    "border": "#2d333b",
    "ring": "#58a6ff",
    "caret": "#58a6ff",
    "selection.background": "#58a6ff4d",
    "overlay": "#00000080",
    "window.border": "#2d333b",
    "input.border": "#3d444d",

    "primary.background": "#316dca",
    "primary.foreground": "#ffffff",
    "primary.hover.background": "#3a7ce0",
    "primary.active.background": "#2b5fb0",
    "secondary.background": "#21262e",
    "secondary.foreground": "#c9d1d9",
    "secondary.hover.background": "#262c36",
    "secondary.active.background": "#2d333b",
    "accent.background": "#1e2836",
    "accent.foreground": "#c9d1d9",
    "muted.background": "#21262e",
    "muted.foreground": "#8b949e",

    "danger.background": "#f85149",
    "danger.foreground": "#ffffff",
    "danger.hover.background": "#ff6a5f",
    "danger.active.background": "#e0443c",
    "success.background": "#3fb950",
    "success.foreground": "#0e1116",
    "success.hover.background": "#56d364",
    "success.active.background": "#2ea043",
    "warning.background": "#d29922",
    "warning.foreground": "#0e1116",
    "warning.hover.background": "#e3b341",
    "warning.active.background": "#b98617",
    "info.background": "#1f6feb",
    "info.foreground": "#ffffff",
    "info.hover.background": "#388bfd",
    "info.active.background": "#1a5fc8",

    "link": "#58a6ff",
    "link.hover": "#79b8ff",
    "link.active": "#4184e4",

    "popover.background": "#1a2029",
    "popover.foreground": "#c9d1d9",
    "sidebar.background": "#151a21",
    "sidebar.foreground": "#c9d1d9",
    "sidebar.border": "#2d333b",
    "sidebar.primary.background": "#316dca",
    "sidebar.primary.foreground": "#ffffff",
    "sidebar.accent.background": "#1e2836",
    "sidebar.accent.foreground": "#c9d1d9",
    "title_bar.background": "#0e1116",
    "title_bar.border": "#2d333b",
    "tiles.background": "#12161c",

    "tab_bar.background": "#151a21",
    "tab_bar.segmented.background": "#151a21",
    "tab.background": "#151a21",
    "tab.foreground": "#8b949e",
    "tab.active.background": "#0e1116",
    "tab.active.foreground": "#c9d1d9",

    "list.background": "#0e1116",
    "list.even.background": "#12161c",
    "list.head.background": "#151a21",
    "list.hover.background": "#1a2029",
    "list.active.background": "#1e2836",
    "list.active.border": "#58a6ff",
    "table.background": "#0e1116",
    "table.even.background": "#12161c",
    "table.head.background": "#151a21",
    "table.head.foreground": "#8b949e",
    "table.hover.background": "#1a2029",
    "table.active.background": "#1e2836",
    "table.active.border": "#58a6ff",
    "table.row.border": "#21262e",

    "accordion.background": "#151a21",
    "accordion.hover.background": "#1a2029",
    "group_box.background": "#151a21",
    "group_box.foreground": "#c9d1d9",
    "group_box.title.foreground": "#8b949e",
    "description_list.label.background": "#151a21",
    "description_list.label.foreground": "#8b949e",

    "scrollbar.background": "#0e111600",
    "scrollbar.thumb.background": "#30363d",
    "scrollbar.thumb.hover.background": "#3d444d",
    "skeleton.background": "#21262e",
    "progress.bar.background": "#316dca",
    "slider.background": "#2d333b",
    "slider.thumb.background": "#c9d1d9",
    "switch.background": "#2d333b",
    "switch.thumb.background": "#ffffff",
    "drag.border": "#58a6ff",
    "drop_target.background": "#58a6ff1a",

    "chart.1": "#58a6ff",
    "chart.2": "#3fb950",
    "chart.3": "#d29922",
    "chart.4": "#bc8cff",
    "chart.5": "#39c5cf",
    "bullish.background": "#3fb950",
    "bearish.background": "#f85149",

    "base.blue": "#58a6ff",
    "base.blue.light": "#79b8ff",
    "base.cyan": "#39c5cf",
    "base.cyan.light": "#56d4dd",
    "base.green": "#3fb950",
    "base.green.light": "#56d364",
    "base.magenta": "#bc8cff",
    "base.magenta.light": "#d2a8ff",
    "base.red": "#f85149",
    "base.red.light": "#ff7b72",
    "base.yellow": "#d29922",
    "base.yellow.light": "#e3b341"
  }
}
```

- [ ] **Step 4: Replace `light.json` with the hybrid full-coverage document**

GitHub-light mirror. Ladder: bg `#ffffff` → even `#fafbfc` → surface `#f6f8fa` → hover `#f3f4f6` → muted `#eaeef2` → strong `#dde3e9` → border `#d0d7de` → strong border `#afb8c1`; muted fg `#656d76`; selection tint `#ddf4ff`.

```json
{
  "name": "light",
  "appearance": "light",
  "style": {
    "background": "#ffffff",
    "foreground": "#1f2328",
    "border": "#d0d7de",
    "accent": "#0969da",
    "error": "#cf222e",
    "success": "#1a7f37",
    "warning": "#9a6700"
  },
  "mode": "light",
  "font.size": 14,
  "radius": 5,
  "shadow": true,
  "colors": {
    "background": "#ffffff",
    "foreground": "#1f2328",
    "border": "#d0d7de",
    "ring": "#0969da",
    "caret": "#0969da",
    "selection.background": "#0969da33",
    "overlay": "#0000004d",
    "window.border": "#d0d7de",
    "input.border": "#d0d7de",

    "primary.background": "#0969da",
    "primary.foreground": "#ffffff",
    "primary.hover.background": "#0860ca",
    "primary.active.background": "#0757ba",
    "secondary.background": "#f6f8fa",
    "secondary.foreground": "#1f2328",
    "secondary.hover.background": "#eaeef2",
    "secondary.active.background": "#dde3e9",
    "accent.background": "#ddf4ff",
    "accent.foreground": "#1f2328",
    "muted.background": "#eaeef2",
    "muted.foreground": "#656d76",

    "danger.background": "#cf222e",
    "danger.foreground": "#ffffff",
    "danger.hover.background": "#a40e26",
    "danger.active.background": "#8c0e22",
    "success.background": "#1a7f37",
    "success.foreground": "#ffffff",
    "success.hover.background": "#116329",
    "success.active.background": "#0a4420",
    "warning.background": "#9a6700",
    "warning.foreground": "#ffffff",
    "warning.hover.background": "#7d5500",
    "warning.active.background": "#664400",
    "info.background": "#0969da",
    "info.foreground": "#ffffff",
    "info.hover.background": "#218bff",
    "info.active.background": "#0757ba",

    "link": "#0969da",
    "link.hover": "#0757ba",
    "link.active": "#033d8b",

    "popover.background": "#ffffff",
    "popover.foreground": "#1f2328",
    "sidebar.background": "#f6f8fa",
    "sidebar.foreground": "#1f2328",
    "sidebar.border": "#d0d7de",
    "sidebar.primary.background": "#0969da",
    "sidebar.primary.foreground": "#ffffff",
    "sidebar.accent.background": "#ddf4ff",
    "sidebar.accent.foreground": "#1f2328",
    "title_bar.background": "#ffffff",
    "title_bar.border": "#d0d7de",
    "tiles.background": "#fafbfc",

    "tab_bar.background": "#f6f8fa",
    "tab_bar.segmented.background": "#f6f8fa",
    "tab.background": "#f6f8fa",
    "tab.foreground": "#656d76",
    "tab.active.background": "#ffffff",
    "tab.active.foreground": "#1f2328",

    "list.background": "#ffffff",
    "list.even.background": "#fafbfc",
    "list.head.background": "#f6f8fa",
    "list.hover.background": "#f3f4f6",
    "list.active.background": "#ddf4ff",
    "list.active.border": "#0969da",
    "table.background": "#ffffff",
    "table.even.background": "#fafbfc",
    "table.head.background": "#f6f8fa",
    "table.head.foreground": "#656d76",
    "table.hover.background": "#f3f4f6",
    "table.active.background": "#ddf4ff",
    "table.active.border": "#0969da",
    "table.row.border": "#eaeef2",

    "accordion.background": "#f6f8fa",
    "accordion.hover.background": "#eaeef2",
    "group_box.background": "#f6f8fa",
    "group_box.foreground": "#1f2328",
    "group_box.title.foreground": "#656d76",
    "description_list.label.background": "#f6f8fa",
    "description_list.label.foreground": "#656d76",

    "scrollbar.background": "#ffffff00",
    "scrollbar.thumb.background": "#d0d7de",
    "scrollbar.thumb.hover.background": "#afb8c1",
    "skeleton.background": "#eaeef2",
    "progress.bar.background": "#0969da",
    "slider.background": "#d0d7de",
    "slider.thumb.background": "#ffffff",
    "switch.background": "#d0d7de",
    "switch.thumb.background": "#ffffff",
    "drag.border": "#0969da",
    "drop_target.background": "#0969da1a",

    "chart.1": "#0969da",
    "chart.2": "#1a7f37",
    "chart.3": "#9a6700",
    "chart.4": "#8250df",
    "chart.5": "#1b7c83",
    "bullish.background": "#1a7f37",
    "bearish.background": "#cf222e",

    "base.blue": "#0969da",
    "base.blue.light": "#54aeff",
    "base.cyan": "#1b7c83",
    "base.cyan.light": "#3192aa",
    "base.green": "#1a7f37",
    "base.green.light": "#4ac26b",
    "base.magenta": "#8250df",
    "base.magenta.light": "#a475f9",
    "base.red": "#cf222e",
    "base.red.light": "#ff8182",
    "base.yellow": "#9a6700",
    "base.yellow.light": "#d4a72c"
  }
}
```

- [ ] **Step 5: Replace `high-contrast.json` with the hybrid full-coverage document**

Every key black/white/yellow family, `shadow: false`, `mode: dark`. Gray affordance ladder: `#0d0d0d` even → `#1a1a1a` hover/secondary → `#333333` active/accent → `#4d4d4d` pressed. Status hues stay ≥7:1 on black (`#ff6666` red 7.3:1, `#66ff66` green 16:1, `#ffcc00` amber 13.9:1, `#66ccff` blue 10.9:1). Legacy `style` block stays TODAY's values verbatim.

```json
{
  "name": "high-contrast",
  "appearance": "dark",
  "style": {
    "background": "#000000",
    "foreground": "#ffffff",
    "border": "#ffffff",
    "accent": "#ffff00",
    "error": "#ff0000",
    "success": "#00ff00",
    "warning": "#ffaa00"
  },
  "mode": "dark",
  "font.size": 14,
  "radius": 5,
  "shadow": false,
  "colors": {
    "background": "#000000",
    "foreground": "#ffffff",
    "border": "#ffffff",
    "ring": "#ffff00",
    "caret": "#ffff00",
    "selection.background": "#ffff0066",
    "overlay": "#000000cc",
    "window.border": "#ffffff",
    "input.border": "#ffffff",

    "primary.background": "#ffff00",
    "primary.foreground": "#000000",
    "primary.hover.background": "#ffff66",
    "primary.active.background": "#e6e600",
    "secondary.background": "#1a1a1a",
    "secondary.foreground": "#ffffff",
    "secondary.hover.background": "#333333",
    "secondary.active.background": "#4d4d4d",
    "accent.background": "#333333",
    "accent.foreground": "#ffffff",
    "muted.background": "#1a1a1a",
    "muted.foreground": "#cccccc",

    "danger.background": "#ff6666",
    "danger.foreground": "#000000",
    "danger.hover.background": "#ff8080",
    "danger.active.background": "#ff4d4d",
    "success.background": "#66ff66",
    "success.foreground": "#000000",
    "success.hover.background": "#99ff99",
    "success.active.background": "#33ff33",
    "warning.background": "#ffcc00",
    "warning.foreground": "#000000",
    "warning.hover.background": "#ffdb4d",
    "warning.active.background": "#e6b800",
    "info.background": "#66ccff",
    "info.foreground": "#000000",
    "info.hover.background": "#99ddff",
    "info.active.background": "#33bbff",

    "link": "#66ccff",
    "link.hover": "#99ddff",
    "link.active": "#33bbff",

    "popover.background": "#000000",
    "popover.foreground": "#ffffff",
    "sidebar.background": "#000000",
    "sidebar.foreground": "#ffffff",
    "sidebar.border": "#ffffff",
    "sidebar.primary.background": "#ffff00",
    "sidebar.primary.foreground": "#000000",
    "sidebar.accent.background": "#333333",
    "sidebar.accent.foreground": "#ffffff",
    "title_bar.background": "#000000",
    "title_bar.border": "#ffffff",
    "tiles.background": "#0d0d0d",

    "tab_bar.background": "#000000",
    "tab_bar.segmented.background": "#000000",
    "tab.background": "#000000",
    "tab.foreground": "#ffffff",
    "tab.active.background": "#000000",
    "tab.active.foreground": "#ffff00",

    "list.background": "#000000",
    "list.even.background": "#0d0d0d",
    "list.head.background": "#000000",
    "list.hover.background": "#1a1a1a",
    "list.active.background": "#333333",
    "list.active.border": "#ffff00",
    "table.background": "#000000",
    "table.even.background": "#0d0d0d",
    "table.head.background": "#000000",
    "table.head.foreground": "#ffffff",
    "table.hover.background": "#1a1a1a",
    "table.active.background": "#333333",
    "table.active.border": "#ffff00",
    "table.row.border": "#ffffff",

    "accordion.background": "#000000",
    "accordion.hover.background": "#1a1a1a",
    "group_box.background": "#000000",
    "group_box.foreground": "#ffffff",
    "group_box.title.foreground": "#ffffff",
    "description_list.label.background": "#000000",
    "description_list.label.foreground": "#cccccc",

    "scrollbar.background": "#000000",
    "scrollbar.thumb.background": "#ffffff",
    "scrollbar.thumb.hover.background": "#ffff00",
    "skeleton.background": "#333333",
    "progress.bar.background": "#ffff00",
    "slider.background": "#333333",
    "slider.thumb.background": "#ffffff",
    "switch.background": "#333333",
    "switch.thumb.background": "#ffffff",
    "drag.border": "#ffff00",
    "drop_target.background": "#ffff0033",

    "chart.1": "#ffff00",
    "chart.2": "#66ff66",
    "chart.3": "#66ccff",
    "chart.4": "#ff66ff",
    "chart.5": "#ff6666",
    "bullish.background": "#66ff66",
    "bearish.background": "#ff6666",

    "base.blue": "#66ccff",
    "base.blue.light": "#99ddff",
    "base.cyan": "#66ffff",
    "base.cyan.light": "#99ffff",
    "base.green": "#66ff66",
    "base.green.light": "#99ff99",
    "base.magenta": "#ff66ff",
    "base.magenta.light": "#ff99ff",
    "base.red": "#ff6666",
    "base.red.light": "#ff9999",
    "base.yellow": "#ffff00",
    "base.yellow.light": "#ffff66"
  }
}
```

- [ ] **Step 6: Run the focused suites to verify everything passes**

Run: `cargo test -p dat0-app --test theme --test theme_live_switch --test theme_contrast_gate --test p1_exit_smoke`
Expected: ALL PASS — the 4 new coverage tests pass against the full-coverage colors blocks; the legacy tests (`load_builtin` shape, legacy 5-pair gate over the `style` block) still pass because the hybrid keeps `appearance`/`style` verbatim. If `builtin_configs_specify_every_color_key` reports missing keys, add exactly those keys (the canonical list comes from the crate itself — the test output IS the todo list). If it reports unknown keys, fix the typo'd names.

- [ ] **Step 7: Format and commit**

```bash
cargo fmt --all
git add crates/dat0-app/src/theme/builtins/ crates/dat0-app/tests/theme.rs
git commit -s -m "feat(theme): full-coverage builtin ThemeConfig palettes + coverage gate

Hybrid documents: full ThemeConfig fields (109 color keys, font.size 14,
radius 5, HC shadow:false) + the legacy style block so both parsers stay
green until the A1 facade lands. Two-way coverage gate kills the
shadcn-default sparse-fallback leak class (A0 spike).

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Contrast-gate retarget to ThemeConfig keys

**Files:**
- Modify: `crates/dat0-app/tests/theme_contrast_gate.rs` (full replace)

**Interfaces:**
- Consumes: Task 1's hybrid JSONs (reads their `colors` block via `gpui_component::ThemeConfig`).
- Produces: the A1-shape gate later slices extend (A3 grows the matrix + alpha compositing). Key mapping locked here: `accent`→`ring`, `error`→`danger.background`, `success`→`success.background`, `warning`→`warning.background`.

- [ ] **Step 1: Replace `tests/theme_contrast_gate.rs` entirely**

```rust
//! WCAG AA contrast gate over the three builtin theme configs (P10b a11y,
//! retargeted by UI-redesign A1 to the gpui_component::ThemeConfig shape —
//! the SAME parsed document production applies via `apply_config`).
//!
//! A1 keeps the P10b 5-pair floor; slice A3 extends the matrix (~15 text
//! pairs, ~10 non-text 3:1 pairs, 8-digit-hex alpha compositing) and does
//! the final palette tuning. `border` stays exempt (decorative; WCAG
//! 1.4.11 carve-out). Values are read through serde serialization of the
//! parsed `ThemeConfigColors`, so the gate uses the exact rename keys and
//! survives field renames in the Rust struct.

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

fn color(colors: &serde_json::Value, key: &str) -> String {
    colors[key]
        .as_str()
        .unwrap_or_else(|| panic!("color key {key} missing/null (coverage gate should have caught this)"))
        .to_string()
}

#[test]
fn builtin_themes_meet_wcag_aa() {
    // (fg_key, min_ratio) — each checked against "background".
    // ring replaces the old `accent` token: it IS the single accent now
    // (the two-blues split died in A1).
    let matrix: &[(&str, f64)] = &[
        ("foreground", 4.5),
        ("danger.background", 4.5),
        ("success.background", 4.5),
        ("warning.background", 4.5),
        ("ring", 4.5),
    ];
    let mut failures = vec![];
    for (name, json) in BUILTIN_SOURCES {
        let cfg: ThemeConfig = serde_json::from_str(json).expect("builtin parses");
        let colors = serde_json::to_value(&cfg.colors).expect("colors serialize");
        let bg = color(&colors, "background");
        for (key, min) in matrix {
            let fg = color(&colors, key);
            let r = contrast_ratio(&fg, &bg);
            eprintln!("{name}: {key}/background = {r:.2}:1 (min {min})");
            if r < *min {
                failures.push(format!("{name}: {key}/background = {r:.2}:1 < {min}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "WCAG AA contrast failures:\n{}",
        failures.join("\n")
    );
}
```

- [ ] **Step 2: Run the gate**

Run: `cargo test -p dat0-app --test theme_contrast_gate -- --nocapture`
Expected: PASS with all 15 ratios printed (5 pairs × 3 themes). Precomputed: every pair clears 4.5 (tightest: light `warning.background` `#9a6700` ≈ 4.9:1, light `ring` `#0969da` ≈ 5.2:1). If any pair is red, tune the NON-anchor value in the JSON (never the anchors listed in Global Constraints) and re-run both this gate and `--test theme` (coverage must stay green).

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/theme_contrast_gate.rs
git commit -s -m "test(theme): retarget contrast gate to ThemeConfig keys

accent->ring, error->danger.background, success/warning -> *.background;
same 5-pair 4.5:1 floor x 3 themes. A3 extends the matrix.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Façade rewrite — the swap

**Files:**
- Modify: `crates/dat0-app/src/theme/mod.rs` (full replace)
- Delete: `crates/dat0-app/src/theme/zed_schema.rs`
- Modify: `crates/dat0-app/src/theme/builtins/dark.json`, `light.json`, `high-contrast.json` (strip legacy keys)
- Modify: `crates/dat0-app/tests/theme.rs` (rewrite the 4 legacy tests)
- Modify: `crates/dat0-app/tests/p1_exit_smoke.rs:19-21`
- Modify: `crates/dat0-app/src/window.rs:1807`

**Interfaces:**
- Consumes: Task 1's JSONs (their `colors`/`mode`/`font.size` fields; strips `appearance`/`style`).
- Produces (Task 4 relies on these exact signatures):
  - `pub struct Theme { pub id: String, pub mode: gpui_component::ThemeMode }` — `impl gpui::Global`, plus `pub fn id(&self) -> &str`.
  - `pub fn builtin_config(id: &str) -> Option<&'static gpui_component::ThemeConfig>` (module-level, `pub use`d from `dat0_app::theme`).
  - `Theme::install(cx, &SettingsStore)`, `Theme::install_default(cx)`, `Theme::switch(cx, id)` — unknown id → warn + `"dark"` fallback; forwards to `gpui_component::Theme::apply_config` + `cx.refresh_windows()` ONLY when `cx.has_global::<gpui_component::Theme>()`.

- [ ] **Step 1: Rewrite the 4 legacy tests in `tests/theme.rs`**

Replace the file's OLD header (`use dat0_app::theme::Theme;`) and the 4 legacy tests with the block below. Keep the Task-1 coverage tests (and their `use` lines) untouched below it.

```rust
use dat0_app::theme::builtin_config;

#[test]
fn dark_loads() {
    assert_eq!(builtin_config("dark").expect("dark builtin").name.as_ref(), "dark");
}

#[test]
fn light_loads() {
    assert_eq!(builtin_config("light").expect("light builtin").name.as_ref(), "light");
}

#[test]
fn high_contrast_loads() {
    assert_eq!(
        builtin_config("high-contrast").expect("hc builtin").name.as_ref(),
        "high-contrast"
    );
}

#[test]
fn unknown_returns_none() {
    assert!(builtin_config("does-not-exist").is_none());
}
```

- [ ] **Step 2: Run to verify the retargeted tests fail to compile**

Run: `cargo test -p dat0-app --test theme`
Expected: COMPILE ERROR — `builtin_config` doesn't exist yet. (This is the failing-test step for the façade.)

- [ ] **Step 3: Rewrite `src/theme/mod.rs`** (full replacement)

```rust
//! dat0 theme façade (UI-redesign A1). The single color source of truth is
//! `gpui_component::Theme` — [`Theme::install`] / [`Theme::switch`] apply a
//! full-coverage builtin `ThemeConfig` via `apply_config` and refresh every
//! window. This global only carries the active `{id, mode}` for persistence
//! (`theme.id` in the SettingsStore), the 3-way Settings picker, and
//! `cx.observe_global::<Theme>` fan-out (unchanged subscriber contract).
//!
//! NEVER use `gpui_component::Theme::change` for the 3-way switch: it
//! re-applies from the stored light/dark slots and clobbers high-contrast
//! (master plan §4, verified at rev 0f0ab35).

pub mod contrast;

use std::rc::Rc;
use std::sync::LazyLock;

use gpui_component::{ThemeConfig, ThemeMode};

#[derive(Debug, Clone)]
pub struct Theme {
    /// Logical id: `"dark" | "light" | "high-contrast"`. Matches the value
    /// persisted at `theme.id` in the SettingsStore.
    pub id: String,
    /// The gpui-component mode this id maps to (high-contrast is a `Dark`
    /// config; `apply_config` sets the component-side mode itself).
    pub mode: ThemeMode,
}

impl gpui::Global for Theme {}

fn parse(name: &str, json: &str) -> ThemeConfig {
    // Builtins are compiled in; a parse failure is a programmer error and
    // the coverage gate in tests/theme.rs keeps them well-formed. Loud
    // failure over silent fallback (same policy as the old
    // `load_builtin_or_default` inner expect).
    serde_json::from_str(json)
        .unwrap_or_else(|e| panic!("built-in theme '{name}' must parse: {e}"))
}

static DARK: LazyLock<ThemeConfig> =
    LazyLock::new(|| parse("dark", include_str!("builtins/dark.json")));
static LIGHT: LazyLock<ThemeConfig> =
    LazyLock::new(|| parse("light", include_str!("builtins/light.json")));
static HIGH_CONTRAST: LazyLock<ThemeConfig> =
    LazyLock::new(|| parse("high-contrast", include_str!("builtins/high-contrast.json")));

/// The parsed builtin `ThemeConfig` for a dat0 theme id, or `None` for
/// unknown ids (callers that want fallback semantics use
/// [`Theme::switch`], which maps unknown → `"dark"`).
pub fn builtin_config(id: &str) -> Option<&'static ThemeConfig> {
    match id {
        "dark" => Some(&DARK),
        "light" => Some(&LIGHT),
        "high-contrast" => Some(&HIGH_CONTRAST),
        _ => None,
    }
}

impl Theme {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// App-boot install: read the persisted `theme.id` (missing/unknown →
    /// `"dark"`), set the façade global, and restyle gpui-component.
    /// Called once from `run_app` before any window opens.
    pub fn install(cx: &mut gpui::App, settings: &crate::settings::store::SettingsStore) {
        let id = settings
            .get_string("theme.id")
            .unwrap_or_else(|| "dark".into());
        Self::activate(cx, &id);
    }

    /// Install the default (`"dark"`) theme — the no-config-dir fallback
    /// path in `run_app` and pure-test convenience.
    pub fn install_default(cx: &mut gpui::App) {
        Self::activate(cx, "dark");
    }

    /// Switch to `new_id` and fan out: sets the façade global (observers
    /// registered with `cx.observe_global::<Theme>` re-render next tick)
    /// and re-applies the matching config to the gpui-component global so
    /// widgets actually restyle. Unknown ids fall back to `"dark"`.
    pub fn switch(cx: &mut gpui::App, new_id: &str) {
        Self::activate(cx, new_id);
    }

    fn activate(cx: &mut gpui::App, requested: &str) {
        let (id, cfg) = match builtin_config(requested) {
            Some(cfg) => (requested, cfg),
            None => {
                tracing::warn!(
                    requested,
                    "unknown theme id; falling back to 'dark'"
                );
                ("dark", builtin_config("dark").expect("'dark' is a builtin"))
            }
        };
        cx.set_global(Self {
            id: id.to_string(),
            mode: cfg.mode,
        });
        // Forward to the gpui-component global so widgets restyle. No-op in
        // pure-test contexts that never ran `gpui_component::init` — the
        // façade global still installs so observer-based tests keep working
        // (A0 spike pattern).
        if cx.has_global::<gpui_component::Theme>() {
            gpui_component::Theme::global_mut(cx).apply_config(&Rc::new(cfg.clone()));
            cx.refresh_windows();
        }
    }
}
```

Then: `git rm crates/dat0-app/src/theme/zed_schema.rs`.

- [ ] **Step 4: Strip the legacy keys from the three JSONs**

In each of `dark.json` / `light.json` / `high-contrast.json`, delete the `"appearance"` key and the entire `"style"` object (the Task-1 hybrid scaffolding). Everything else stays byte-identical.

- [ ] **Step 5: Retarget `tests/p1_exit_smoke.rs:19-21`**

Replace the three `Theme::load_builtin(...)` asserts:

```rust
    assert!(dat0_app::theme::builtin_config("dark").is_some());
    assert!(dat0_app::theme::builtin_config("light").is_some());
    assert!(dat0_app::theme::builtin_config("high-contrast").is_some());
```

If the file's `use dat0_app::theme::Theme;` import becomes unused, delete it (the compiler will tell you).

- [ ] **Step 6: Retarget `src/window.rs:1807`**

Replace exactly one line inside the `else` branch of the config-dir check:

```rust
            cx.set_global(crate::theme::Theme::load_builtin_or_default("dark"));
```

with:

```rust
            crate::theme::Theme::install_default(cx);
```

(Behavioral improvement: this fallback path now ALSO restyles gpui-component when its global exists — previously it only set the dat0 global. Update the comment above it: drop the "same shape as the fallback path in `Theme::install`" sentence in favor of "Installs the built-in default via the same activate path `Theme::install` uses.")

- [ ] **Step 7: Run the focused suites**

Run: `cargo test -p dat0-app --test theme --test theme_contrast_gate --test p1_exit_smoke`
Expected: PASS. `theme_live_switch` still references `load_builtin` and now FAILS TO COMPILE — expected; it is rewritten in Task 4. Confirm with `cargo build -p dat0-app` that the library itself compiles clean (window.rs, settings_ui — `Theme::switch` call sites are signature-compatible).

- [ ] **Step 8: Remove `theme_live_switch.rs` (Task 4 recreates it)**

The old suite is welded to `load_builtin`/`background()` which no longer exist. Task 4 writes its replacement from scratch in the next commit, so delete rather than leave a transitional stub:

```bash
git rm crates/dat0-app/tests/theme_live_switch.rs
```

Run: `cargo test -p dat0-app --test theme` — Expected: PASS (and no `theme_live_switch` target remains to fail the build).

- [ ] **Step 9: Format and commit**

```bash
cargo fmt --all
git add -A crates/dat0-app/src/theme crates/dat0-app/src/window.rs crates/dat0-app/tests
git commit -s -m "feat(theme): facade rewrite — gpui-component Theme becomes the color source of truth

Theme {id, mode} facade + LazyLock builtin_config + install/switch through
apply_config (guarded for pure-test contexts, A0 spike pattern). Deletes
zed_schema.rs and the hybrid legacy JSON block. Theme switching now
actually restyles widgets.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Live-switch round-trip (the A1 acceptance test)

**Files:**
- Modify: `crates/dat0-app/tests/theme_live_switch.rs` (full replace of the Task-3 marker)

**Interfaces:**
- Consumes: Task 3's `Theme::switch`, `builtin_config`, façade fields; Task 1's palette values (dark bg `#0e1116` → Hsla lightness < 0.15; light bg `#ffffff` → > 0.95; HC bg `#000000` → 0.0; ring `#ffff00` → yellow ≈ l 0.5).
- Produces: the regression suite proving "switching actually restyles" — the slice's headline claim.

- [ ] **Step 1: Write the new suite** (full file content)

```rust
//! Live theme switching (UI-redesign A1) — the PRODUCTION
//! `Theme::switch` path drives the `gpui_component::Theme` global:
//! dark → light → high-contrast → dark round-trip, full-coverage
//! anti-leak, unknown-id fallback, and façade-global tracking.
//! Ports the A0 spike round-trip (`tests/spike_a0.rs` on
//! `spike/ui-redesign-a0`) onto the shipped façade.

use gpui::TestAppContext;
use gpui_component::ActiveTheme as _;

use dat0_app::theme::{builtin_config, Theme};

#[test]
fn builtin_dark_and_light_differ() {
    let dark = serde_json::to_value(&builtin_config("dark").expect("dark").colors).unwrap();
    let light = serde_json::to_value(&builtin_config("light").expect("light").colors).unwrap();
    assert_ne!(
        dark["background"], light["background"],
        "dark and light must paint different backgrounds — otherwise a live \
         switch is visually indistinguishable"
    );
}

#[gpui::test]
fn switch_round_trip_restyles_gpui_component(cx: &mut TestAppContext) {
    cx.update(gpui_component::init);

    // dark
    cx.update(|cx| {
        Theme::switch(cx, "dark");
        let t = cx.theme();
        assert!(t.mode.is_dark(), "dark config must set dark mode");
        assert_eq!(t.font_size, gpui::px(14.), "font.size 14 must apply");
        assert!(
            t.background.l < 0.15 && t.background.l > 0.0,
            "dark bg (#0e1116) lightness, got {}",
            t.background.l
        );
        assert_eq!(cx.global::<Theme>().id, "dark");
    });

    // light on top of dark
    cx.update(|cx| {
        Theme::switch(cx, "light");
        let t = cx.theme();
        assert!(!t.mode.is_dark(), "light config must set light mode");
        assert!(t.background.l > 0.95, "light bg (#ffffff), got {}", t.background.l);
        assert_eq!(t.font_size, gpui::px(14.));
        assert_eq!(cx.global::<Theme>().id, "light");
    });

    // high-contrast (third config, mode=dark) on top of light
    cx.update(|cx| {
        Theme::switch(cx, "high-contrast");
        let t = cx.theme();
        assert!(t.mode.is_dark());
        assert_eq!(t.background.l, 0.0, "HC bg must be pure black");
        assert!(
            t.ring.l > 0.45 && t.ring.l < 0.55,
            "HC ring must be yellow (#ffff00), got l={}",
            t.ring.l
        );
        // FULL-COVERAGE anti-leak: with every key specified, nothing falls
        // back to the shadcn dark defaults (the A0 sparse-config bug that
        // produced an illegible HC theme).
        let shadcn_dark = gpui_component::ThemeColor::dark();
        assert_ne!(
            t.secondary, shadcn_dark.secondary,
            "HC secondary must be authored (#1a1a1a), not the shadcn default"
        );
        assert_eq!(t.font_size, gpui::px(14.), "HC must keep font.size 14");
        assert_eq!(cx.global::<Theme>().id, "high-contrast");
    });

    // back to dark — round-trip complete
    cx.update(|cx| {
        Theme::switch(cx, "dark");
        let t = cx.theme();
        assert!(t.mode.is_dark());
        assert_eq!(t.font_size, gpui::px(14.));
        assert!(t.background.l < 0.15);
        assert_eq!(cx.global::<Theme>().id, "dark");
    });

    // unknown id falls back to dark (load_builtin_or_default semantics
    // preserved across the façade rewrite)
    cx.update(|cx| {
        Theme::switch(cx, "does-not-exist");
        assert_eq!(cx.global::<Theme>().id, "dark");
        assert!(cx.theme().mode.is_dark());
    });
}

#[gpui::test]
fn switch_without_component_global_still_installs_facade(cx: &mut TestAppContext) {
    // Pure-test contexts never run gpui_component::init — the forward must
    // no-op while the façade global (and its observers) still work.
    cx.update(|cx| {
        assert!(!cx.has_global::<gpui_component::Theme>());
        Theme::switch(cx, "light");
        assert_eq!(cx.global::<Theme>().id, "light");
        assert!(!cx.has_global::<gpui_component::Theme>());
    });
}
```

- [ ] **Step 2: Run the suite**

Run: `cargo test -p dat0-app --test theme_live_switch`
Expected: 3/3 PASS. If `t.secondary` unexpectedly EQUALS the shadcn default, the HC `secondary.background` was dropped or mistyped in the JSON — the coverage gate output (`--test theme`) names the key.

- [ ] **Step 3: Commit**

```bash
cargo fmt --all
git add crates/dat0-app/tests/theme_live_switch.rs
git commit -s -m "test(theme): live-switch round-trip through production install/switch

dark->light->HC->dark on cx.theme() (mode/font/bg/ring), full-coverage
anti-leak vs shadcn defaults, unknown-id fallback, no-component-global
guard. The A1 acceptance proof: switching actually restyles.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Whole-branch gate + final review (controller-run — not a subagent implementation task)

**Files:** none new (fixes only, if the gate or review finds any).

- [ ] **Step 1: Full workspace gate**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: all green — including the full nav/a11y suite (labels not colors: the token swap must be invisible to it; ANY nav/a11y failure here means A1 leaked behavior, not styling — stop and diagnose, do not patch the test) and the settings_ui theme-dropdown round-trips (exercise the has_global guard).

- [ ] **Step 2: Grep-level dead-code sweep**

```bash
git grep -n "ZedStyle\|ZedTheme\|zed_schema\|load_builtin" -- crates
```

Expected: zero hits (docs/ may still mention them historically; `crates/` must be clean).

- [ ] **Step 3: Final whole-branch review (opus)**

Dispatch a code-reviewer subagent (opus) over `git diff c724957..HEAD` with the design doc + this plan as context. Focus prompts: (a) does any production path still read a color from a source other than `gpui_component::Theme`? (that's expected — inline hex sites migrate in A6 — but flag any NEW ones this branch adds); (b) is the `has_global` guard airtight for every `install`/`switch` caller; (c) do the tests drive the production path (no test-only shims); (d) palette sanity spot-check against the anchors.

- [ ] **Step 4: Push and open the PR**

```bash
git push -u origin feat/ui-redesign-a1-theme-unification
gh pr create --title "feat(theme): A1 theme unification — switching actually restyles (UI redesign)" --body "..."
```

PR body: link master plan §5 A1 + design doc; note invariants held (zero deps/i18n/schema; a11y suite untouched); owed human glances (in-app palette feel ×3 themes, HC legibility, focus-ring feel vs new ring — batch UAT). End with the standard generated-with footer. Then poll `gh pr checks` (NOT `gh run watch`); after the user merges: WATCH THE POST-MERGE MAIN RUN (macOS grid-scroll bench is push-to-main-only — silent-red risk), then delete `spike/ui-redesign-a0`.

---

## Self-review notes (plan-time)

- Spec coverage: design §1 façade → Task 3; §2 JSONs → Task 1; §3 coverage gate → Task 1, contrast retarget → Task 2, live-switch port + pure retargets → Task 4, `p1_exit_smoke`/`theme.rs`/window.rs → Task 3; §6 acceptance → Tasks 4–5. Design's "fallback-test placement" open point resolved: pure `is_none` check in `tests/theme.rs` (Task 3) + switch-level fallback in `theme_live_switch.rs` (Task 4). Design's LazyLock/Sync open point: `ThemeConfig` is `SharedString`/`Option` fields — `Send + Sync` holds; if a future rev breaks it, switch `builtin_config` to parse-per-call (API unchanged).
- Green-at-every-commit: Task 1 hybrid keeps both parsers green; Task 3 neutralizes `theme_live_switch` within the same commit that breaks its old API; Task 4 restores it. No commit on the branch leaves the workspace red.
- Type consistency: `builtin_config(id: &str) -> Option<&'static ThemeConfig>` used identically in Tasks 3, 4, and `p1_exit_smoke`; façade field `id: String` + `id()` accessor used by Task 4 asserts.
