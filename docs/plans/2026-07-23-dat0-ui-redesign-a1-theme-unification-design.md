# Slice A1 — Theme Unification (design)

Date: 2026-07-23 · Parent: `docs/plans/2026-07-21-dat0-ui-redesign-master-plan.md` §5 (A1 row), §4 (verified API), §A0 (spike verdicts)
Branch: `feat/ui-redesign-a1-theme-unification` off main `c724957`.
Status: design approved in-session; plan next.

## Goal

Make theme switching actually restyle the app. Single color source of truth becomes
`gpui_component::Theme`; dat0's three themes become full-coverage `ThemeConfig` JSONs
applied via `apply_config`; dat0's `crate::theme::Theme` global shrinks to a façade
`{id, mode}` for persistence, the 3-way picker, and observer fan-out.

Everything here was de-risked by the A0 spike (branch `spike/ui-redesign-a0`,
commits `2eea0b5` wiring + `tests/spike_a0.rs`): `apply_config` sets `theme.mode`
itself (schema.rs:703) and stores the config into its matching light/dark slot;
nothing clobbers post-init; `font.size: 14` adopted; sparse configs leak shadcn
defaults (visually confirmed illegible HC) → full coverage is mandatory.

## 1. Façade — `crates/dat0-app/src/theme/mod.rs` rewrite

```rust
pub struct Theme {              // stays `impl gpui::Global`
    pub id: String,             // "dark" | "light" | "high-contrast"
    pub mode: gpui_component::ThemeMode,
}
```

- Existing `cx.observe_global::<crate::theme::Theme>` subscriber sites (window.rs,
  settings_ui) keep working unchanged — `set_global` still fires them.
- `pub fn builtin_config(id: &str) -> Option<&'static gpui_component::ThemeConfig>`
  — pure; `LazyLock` statics parsed from `include_str!` of the three builtin JSONs.
  `ThemeConfig` holds `SharedString`s (Arc-backed) so the static should satisfy
  `Send + Sync`; if the plan's compile check finds otherwise, fall back to
  parse-per-call (few-KB JSON, negligible).
  Parse failure of a builtin is a programmer error → `expect` inside the LazyLock
  (loud, same policy as today's `load_builtin_or_default` inner expect).
- `install(cx, settings)`: read `theme.id` (missing/unknown → `"dark"`, semantics
  preserved from `load_builtin_or_default`), set façade global, forward, refresh.
- `switch(cx, new_id)`: same body minus the settings read.
- Forwarding (spike pattern verbatim): no-op unless
  `cx.has_global::<gpui_component::Theme>()` (pure-test contexts that never ran
  `gpui_component::init`); then `Theme::global_mut(cx).apply_config(&Rc::new(cfg.clone()))`
  + `cx.refresh_windows()`. NEVER `gpui_component::Theme::change` — it re-applies
  from the stored light/dark slots and clobbers high-contrast.
- Deleted: `zed_schema.rs` (`ZedTheme`/`ZedStyle`), `load_builtin`,
  `load_builtin_or_default`, `background()`. `id()` accessor stays.
- `contrast.rs` untouched (A3 extends it).

### window.rs touch (only one)

Line ~1807 (a11y-capture/test-context install fallback):
`cx.set_global(Theme::load_builtin_or_default("dark"))` → the façade equivalent
(`Theme::install_default(cx)` or inline construction — plan decides the smaller
diff). No other window.rs changes.

## 2. Three full-coverage builtin JSONs

Replace the 7-token files in `src/theme/builtins/` (same filenames:
`dark.json`, `light.json`, `high-contrast.json`) with flat `ThemeConfig`
documents (spike-proven shape: top-level `name`/`mode`/`font.size`/`radius`/
`shadow`/`colors`).

Non-color config per file: `font.size: 14` (A0 verdict), `radius: 5` (spike
value; component default is 6, `radius.lg` left to default 8), `shadow`:
dark/light `true`, high-contrast `false` (flat).

`colors`: **every** color key in `ThemeConfigColors` at pinned rev 0f0ab35
(~109 keys — from `accent.background` through `base.yellow.light`, incl.
`chart.1..5`, all `sidebar.*`/`table.*`/`list.*`/`tab.*` families, `overlay`,
`window.border`). No sparse fallback anywhere — the shadcn-leak class dies here.

Palette identity (master-plan §5 pins the anchors):

| Anchor | dark | light | high-contrast |
|---|---|---|---|
| background | `#0e1116` | `#ffffff` | `#000000` |
| surface family (sidebar/table.head/tab_bar/…) | `#151a21` | `#f6f8fa` | `#000000` + strong borders |
| popover/overlay cards | `#1a2029` | `#ffffff` | `#000000` |
| foreground | `#c9d1d9` | `#1f2328` | `#ffffff` |
| primary.background | `#316dca` | `#0969da` | `#ffff00` on black (fg `#000000`) |
| **ring** (kills the two-blues split) | `#58a6ff` | `#0969da` | `#ffff00` |
| danger / success / warning | `#f85149` / `#3fb950` / `#d29922` | `#cf222e` / `#1a7f37` / `#9a6700` | white/yellow family, AA on black |
| base.* hues + chart.1..5 | GitHub Primer scale | Primer light scale | saturated-bright on black |

Remaining ~90 values are derived in-slice from these anchors (hover/active =
ladder steps; muted/secondary = desaturated steps; borders `#2d333b` /
`#d0d7de` / `#ffffff`). Constraint while authoring: the existing 5-pair
contrast gate (fg, danger, success, warning, ring — each vs background at
≥4.5:1) must pass for all three themes. Full ~25-pair matrix incl. alpha
compositing stays A3, where final tuning is expected.

## 3. Tests

**New coverage gate** (in retargeted `tests/theme.rs`) — the permanent
anti-leak lock, no hardcoded key list. Rev fact (0f0ab35): `ThemeConfigColors`
derives `Serialize` with zero `skip_serializing_if` → serializing emits every
field, `None` as `null`. Per builtin JSON:
1. Parse file → `ThemeConfig`; `serde_json::to_value(&cfg.colors)` → canonical
   object (always all keys).
2. Assert **zero null values** → every key specified (missing-key direction).
3. Assert raw file's `colors` key set == canonical key set → no unknown/typo'd
   keys silently ignored by serde (typo direction).

**`tests/theme_live_switch.rs`** — port of the spike round-trip
(`#[gpui::test]`, `cx.update(gpui_component::init)` first), driving the
PRODUCTION `Theme::install`/`Theme::switch`:
- dark → light → high-contrast → dark; each step asserts `cx.theme()` mode,
  `font_size == px(14.)`, background lightness band, ring hue band.
- Anti-leak assert: in HC, `cx.theme().secondary` ≠ `ThemeColor::dark().secondary`
  (sparse configs leaked exactly this in the spike).
- Façade assert: `cx.global::<crate::theme::Theme>().id` tracks each switch;
  unknown id falls back to `"dark"`.
- Existing 2 pure tests retarget: dark/light differ → compare
  `builtin_config` background values; unknown-fallback → via the façade global
  after `switch(cx, "does-not-exist")` (or pure `builtin_config(..).is_none()`
  + a switch-level check — plan decides placement).

**Retargets (minimal, same intent):**
- `tests/theme.rs`: 4 loads → `builtin_config("dark"/"light"/"high-contrast").is_some()`
  + name-field asserts; unknown → `is_none()`.
- `tests/p1_exit_smoke.rs:19-21`: 3 `load_builtin` asserts → `builtin_config` asserts.
- `tests/theme_contrast_gate.rs`: token reads move to the new JSON keys —
  `foreground`/`background` unchanged; `accent` → `ring`; `error` →
  `danger.background`; `success` → `success.background`; `warning` →
  `warning.background`. Same 5 pairs × ≥4.5:1 × 3 themes. Reads via parsed
  `ThemeConfig` (serde), not string poking.

**Suite invariants:** full nav/a11y suite green under `--features a11y-capture`
(labels not colors — token swap invisible); Escape ladder untouched; settings_ui
theme-dropdown round-trip tests green via the has_global guard; Table/delegate
byte-identical (no bench-gated files touched — still watch post-merge main).

## 4. Out of scope (later slices)

No `gpui-component-assets` dep / `.with_assets` (A5) · no `tokens.rs`/`Dat0Colors`
(A2) · no gate-matrix extension or palette fine-tuning (A3) · no style lint (A4)
· no inline-hex call-site migration (A6) · window.rs styling (B10). Zero new
deps, zero i18n keys, zero schema/event changes.

## 5. Risks

| Risk | Mitigation |
|---|---|
| ~330 hand-authored color values: plausible-but-ugly palette | anchors pinned above; ladder-derivation rules; 5-pair gate in-slice; A3 tunes with the full matrix; owed human glance logged |
| `LazyLock<ThemeConfig>` fails `Sync` | parse-per-call fallback, API unchanged |
| a11y-capture harness contexts hit the forward path | has_global guard (spike-proven) + full suite is the gate |
| HC `mode: dark` + system-appearance flip | spike: `sync_system_appearance` runs once at init, no observers; `Theme::change` banned |
| serde silently ignoring typo'd color keys | two-way key-set assert in the coverage gate |

## 6. Acceptance

1. Settings ▸ Theme cycle dark→light→HC→dark restyles gpui-component widgets
   live, both windows (headless proof in-slice; CGEvent live glance owed to
   batch UAT).
2. Coverage gate: 3/3 builtins full-coverage, two-way.
3. Contrast gate green over the new palettes (5 pairs × 3 themes).
4. `zed_schema.rs` gone; no `ZedStyle` references anywhere.
5. Full workspace suite + clippy green under `--features a11y-capture`.
6. Post-merge: main CI green both platforms + macOS bench artifact + crash-e2e;
   then delete `spike/ui-redesign-a0`.

## 7. Process

Per master plan §10: plan next (writing-plans), then SDD. Model per task shape:
sonnet for palette authoring + façade rewrite + test retargets; opus for final
whole-branch review (transient-bars lesson: cross-cutting review catches what
per-task reviews miss). No T0 gate — A0 was the T0. Owed human glances after
merge: in-app palette feel ×3 themes, HC legibility, focus-ring feel vs new ring.
