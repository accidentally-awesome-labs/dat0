# dat0 UI Redesign v1 — Master Plan

Date: 2026-07-21
Status: Complete (B11, 95627c8)
Scope decisions (owner-approved): full redesign · overlay/modal hardening · icon system · command palette · status bar · DockArea adoption now (not deferred).

---

## 1. Why

A three-agent audit (dat0 code / Zed + gpui-component source / Rust desktop app survey: Rerun, libcosmic, Lapce, Halloy, Sniffnet) found the current UI has no functioning design system:

1. **The theme is decorative.** `src/theme/` holds 7 color tokens loaded from 3 JSON builtins — but render code never reads them, and `Theme::switch` never forwards to `gpui_component::Theme`. Switching themes repaints without restyling. On-screen colors come from three uncoordinated sources: gpui-component's default theme (its widgets), inline Tailwind hex copy-pasted per surface (29 `rgb(0x…)` sites in 9 files), and the mostly-unused JSON.
2. **Two different accent blues.** Focus ring / selection = `0x3b82f6` (Tailwind blue-500, const in `a11y/mod.rs:30`); theme accent = `#58a6ff` dark / `#0969da` light.
3. **No spacing / radius / elevation / typography tokens.** Zero radii and shadows in app code; overlays are flat bordered divs stacked at one hardcoded position (`.top_16().left_1_2()`) with "polish later" comments; magic `px()` sizes scattered.
4. **Icons = unmanaged Unicode glyphs** (`✕ • ▶ ‹ › ⌄ ⌃ ◆ ● ○ ▾ ▸ ▣ 📑`). No SVG assets; no `AssetSource` registered at all (`Application::new()` without `.with_assets` at `window.rs:1386`), so even gpui-component's own icons can't load.
5. **Structural gaps**: command palette is a stub (Cmd-Shift-P does nothing, `command_palette.rs:67`); no status bar; `window.rs` is a 7,220-line monolith with fixed-width bool-toggled docks; NamePrompt has no Tab focus-trap (WCAG 2.4.3, deferred from input-nav slice).

## 2. Research basis (what the winners do)

- **gpui-component (our pinned rev 0f0ab35 ≈ v0.5.1) already ships the answer we never wired in**: `ThemeColor` (~100 semantic tokens incl. dedicated `table_*`, `list_*`, `sidebar_*`, `tab_*`, `ring`, `selection`, `chart_1..5`, base hues `red/green/blue/…`), `Theme` global with `radius`/`font_size`/`shadow`, `ActiveTheme` → `cx.theme()`, JSON `ThemeConfig`/`ThemeRegistry` with hot reload, `Sizable` density (`table_row_height`: XSmall 26 / Small 30 / Medium 32 / Large 40), Lucide `Icon`/`IconName` + `gpui-component-assets`, `Kbd`, virtualized `Table`, `DockArea` with serializable state.
- **What Zed adds that gpui-component doesn't** (dat0 must build): a spacing scale (`DynamicSpacing` pattern), one elevation enum driving bg + shadow together (`ElevationIndex`), a semantic text ladder (desktop scale ~13-14px body / 11px section headers, not 16px web default), a global density policy, no-raw-hex-at-call-sites discipline, variant enums with a single centralized variant→token map per component.
- **Rerun / libcosmic**: tokens as the thing that separates polished Rust apps from generic ones; densities as first-class tokens (Rerun: dense 20px vs spacious 32px rows); contrast as an algorithm not a guess (libcosmic Oklch derivation — we approximate with full-coverage JSONs + an extended contrast gate); command palette as the keyboard-first spine; typed empty/loading/streaming/error states; exact macOS chrome (traffic lights (9,9), ~80px inset — gpui-component `TitleBar` matches).

## 3. Target architecture

**Single color source of truth: `gpui_component::Theme`.** dat0's 3 themes become full-coverage `ThemeConfig` JSONs applied via `apply_config`. dat0's own `crate::theme::Theme` global shrinks to a façade `{id, mode}` for persistence, the 3-way picker, and observer fan-out. dat0-specific semantics (`focus_ring`, `selection_tint`, `marching_ants`, `null_value_fg`, `pipeline_*`, banner colors…) are a **derived struct** `Dat0Colors` computed on read from `cx.theme()` via extension trait (`cx.theme().d0().focus_ring`) — no second global, no staleness, high-contrast propagates automatically.

dat0-built scales in `src/theme/tokens.rs`: `Sp` spacing enum (1/2/4/6/8/12/16/24/32), `TextRole` ladder (Caption 11 → Small 12 → Body 13 → BodyLg 14 → Title 16 → Display 20), `Elevation { Background, Surface, Raised, Overlay, Modal }` (bg + radius + shadow, gated on `theme.shadow` so high-contrast stays flat), `Density { Compact, Default, Comfortable }` → `gpui_component::Size` (grid default: Compact/XSmall 26px rows — dense-workbench policy).

**Shell** (end state):

```
WorkspaceShell (persistent root — shrinks, does not disappear)
├── banner host / tab strip / pipeline bar
├── DockArea (locked: resizable + collapsible, no drag-rearrange in v1)
│   ├── left    split: CatalogPanel | ConnectionsPanel | AiDockPanel
│   ├── center  GridPanel (Table / loading / empty-state hero)
│   ├── right   split: InspectorPanel | ChartsPanel
│   └── bottom  SqlConsole (gains impl Panel; moves below grid, resizable)
├── StatusBar (dat0-built, ~24px: row count · selection · timing chip · connection)
├── anchored overlays (filter popover, cell editor) — elevation-styled, non-modal
└── Root sheet/dialog layers — all modals via new ModalHost (scrim + trap + Escape)
```

## 4. Verified API ground truth (pinned rev 0f0ab35 — corrects earlier assumptions)

Checkout on disk: `~/.cargo/git/checkouts/gpui-component-95ce574d8a0da8b8/0f0ab35/`.

- `Theme::apply_config(&Rc<ThemeConfig>)` is pub and **DOES set `theme.mode` from the config** (schema.rs:703 — corrected by the A0 spike; an earlier read claimed otherwise). Callers only add `cx.refresh_windows()`. Still do NOT use `Theme::change` for the 3-way switch (it re-applies from stored light/dark slots and would clobber high-contrast when system appearance is light).
- ThemeConfig sparse colors fall back to **shadcn defaults**, not to other keys in the file → the high-contrast JSON must specify **every** color key or mid-contrast colors leak.
- `Icon`/`IconName` exist (Lucide, 86 SVGs via `gpui-component-assets` rust-embed; `IconNamed` trait is public for app-defined icon enums). Bundle lacks: filter/funnel, play, bookmark, database, table, layers, grip-vertical → dat0 adds those SVGs (ISC license, NOTICE update).
- **No `StatusBar` component and no `status_bar` tokens at this rev** (exists only on later main). Hand-roll (~80 lines) on foundation tokens. Do not bump the gpui-component rev inside this workstream.
- **DockArea persistence exists**: `dump(cx) -> DockAreaState` / `DockArea::load(state, window, cx)`; panels resolve by `panel_name` via global `PanelRegistry` (`register_panel(cx, name, builder)` per panel type; unknown names → graceful `InvalidPanel`). `Panel: EventEmitter<PanelEvent> + Render + Focusable`; `PanelView` blanket-implemented for `Entity<T: Panel>`.
- `DockArea::set_locked(true)` disables tab dragging → v1 = resizable + collapsible only.
- `Dialog` supports scrim/`overlay_closable`/Escape, **but no Tab focus-trap** — WCAG 2.4.3 trap must be dat0-side.
- Key contexts derive from the element tree → `.key_context("SqlConsole")` + the 5-rung Escape ladder survive re-parenting under Dock/TabPanel. Dock module binds no `escape`.
- Shell-root `.on_key_down` (grid arrows) keeps working — events bubble through DockArea.

## 4b. Pre-A1 HOTFIX (found by the A0 spike, 2026-07-21): dead menu items

Driving the real app exposed a production bug: **View ▸ Settings… is grayed out and the Settings window is unreachable** — `OpenSettings` (also `OpenDocs`, `OpenDiscord` under Help) is declared in `menu_macos.rs` and attached to a menu item but has **no gpui `on_action` handler anywhere**; its only wiring is an `ActionRegistry` descriptor whose sole consumer is the stub command palette. macOS menu auto-enablement therefore disables the items. Hidden until now because settings tests boot the panel entity headlessly and manual UAT was owed.

Fix (validated live on the spike branch, `ec4e05b`): global `cx.on_action` in `run_app` for each of the three actions (`OpenSettings → settings_ui::open_settings_window`; `OpenDocs`/`OpenDiscord` → their registry dispatch fns). Ship as a small hotfix PR on main BEFORE A1, with a menu-reachability regression test (assert every `MenuItem::action` type in `menu_macos.rs` has a registered gpui handler — a compile-time-ish inventory test beats per-item UI tests).

Automation notes for future live-driving: System Events AX `enabled` is STALE for gpui menus (reported all-disabled; only the visually opened menu is truth) and AX/`keystroke`/`click at` synthetic input does NOT reach gpui windows — post real CGEvents instead (tiny swiftc-compiled clicker in scratchpad worked; `keystroke` to menus also unreliable). gpui exposes no AX tree (D-015), so coordinate clicks from screenshots are the only handle.

## 5. Workstream A — Design-system foundation

Slices are PR-sized; each ends with the full suite green and (from A4 on) shrinks the style-lint allowlist.

| Slice | Content | Size |
|---|---|---|
| **A0 T0 spike** — ✅ **ANSWERED 2026-07-21** (branch `spike/ui-redesign-a0`, throwaway). (1) **YES**: `apply_config` restyles the gpui-component global (sets `mode` itself, schema.rs:703); headless round-trip dark→light→HC→dark green incl. via production `Theme::switch`; nothing clobbers post-init (`sync_system_appearance` runs once at `gpui_component::init`, no appearance observers anywhere); dark + light boots visually restyle Dialog/Button/banner/shadow. In-session pixel-watch CLOSED same day (Accessibility perm granted): CGEvent-driven clicks cycled Settings ▸ Theme dark→light→high-contrast→dark — **both windows restyled live**, sparse-HC leak reproduced live (illegible washed-out primary button, font popping 14→16). Grid boot also confirmed Table sort/filter header icons render once assets registered. (2) **YES, adopt `font.size` 14** — proportionate across hero/banner/dialog/buttons both themes. (3) **YES**: `.with_assets(gpui_component_assets::Assets)` + same-rev dep renders `IconName::Check` in the hero, icon inherits text color; missing-assets test harness = silent no-render, **no panic** (5 hero-mounting suites green without assets). (4) System font at 14px looks right — **skip font bundling for v1**. **Bonus — leak visually confirmed**: sparse HC config boots with washed-out shadcn primary button (illegible label) + gray dialog card → A1 HC JSON must specify every color key. Spike test kept as reference: `tests/spike_a0.rs`. | done |
| **A1 Theme unification** | 3 full-coverage ThemeSet JSONs (refined GitHub identity: dark bg `#0e1116`, surface `#151a21`, popover `#1a2029`, primary `#316dca`, **ring `#58a6ff` — kills the two-blues split**; light mirrors GitHub-light `#0969da`; HC every-key black/white/yellow, `shadow:false`). `theme/mod.rs` → façade `{id, mode}` + `builtin_config(id)` (pure, LazyLock) + rewired `install`/`switch` (apply_config + mode + set_global + refresh). Delete `zed_schema.rs`. Guard for pure-test contexts without the gpui-component global. Retarget `theme.rs`/`theme_live_switch.rs`/`theme_contrast_gate.rs` minimally. **This is the moment theme switching starts actually working.** | M |
| **A2 Token scales** | `src/theme/tokens.rs`: `Dat0Colors` + `Dat0Theme` trait, `Sp`, `TextRole`, `Elevation`, `Density` (+ helper traits `SpStyled`/`TypoStyled`). Purely additive; inline unit tests. | S-M |
| **A3 Contrast-gate extension** | `contrast.rs` gains 8-digit-hex alpha compositing. Gate matrix over all 3 JSONs: ~15 text pairs ≥4.5:1 (fg/bg per surface family) + ~10 non-text pairs ≥3:1 (ring, status accents, list_active_border, marching ants, pipeline pills) + composited selection-tint-over-table check. Expect red first — drives final palette tuning in-slice. | S |
| **A4 Enforcement** | `tests/style_lint.rs`: regex-ban `rgb(0x`/`rgba(0x`/`gpui::white()`/`gpui::black()` (later: glyph set) with per-line `// style-lint: allow(reason)` escape + shrinking file allowlist (charts/render.rs stays allowed until a chart-palette slice maps `chart_1..5` into plotters). Plus `examples/gallery.rs` — token swatches, scales, elevation cards, icons, themed samples, theme-cycle button; the manual-UAT vehicle for every later slice. | S |
| **A5 Icon system** | `gpui-component-assets` dep (same rev), `src/assets.rs` `Dat0Assets: AssetSource` (own rust-embed folder → fallback to gpui-component's), `.with_assets` at `window.rs:1386`, add 7 missing Lucide SVGs, `Dat0IconName: IconNamed`. Glyph→icon map (✕→Close, ⌄⌃▾▸‹›→Chevrons, funnel→Filter, ▶→Play, ▣→Layers, 📑→Bookmark, ✓→Check; pager dots/dirty-dot stay styled divs; grid `—` null placeholder stays text). Where an a11y label WAS the glyph, switch label to i18n word (`common.close` etc.) — better a11y anyway. | M |
| **A6 Surface migrations** (one PR each or batched 2-3) | a: `a11y/mod.rs` focus ring (trait gains `ring: Hsla` param; ~15 call sites) · b: `error_ux/banner.rs` · c: `view/pipeline_bar.rs` · d: `catalog/panel.rs` · e: `settings_ui/panel.rs` · f: `grid/mod.rs`+`cell_editor.rs` (incl. `Table…with_size(XSmall)` dense default; heavy test surface — full suite) · g: onboarding pager + chart placeholder colors. Each removes its file from the lint allowlist. | S each, f=M |

**A6h (`window.rs` styling)** deliberately folds into B10 — no double-touching the file both workstreams churn.

## 6. Workstream B — Shell + UX features (assumes A landed)

| Slice | Content | Size |
|---|---|---|
| **B1 Modal foundation** | `src/overlay.rs` `ModalHost`: full-window scrim (theme overlay token), centered elevation card, `key_context("Dat0Modal")` + escape→dismiss, **`focus_trap(ids)` helper** (manual Tab/Shift-Tab cycling over the modal's focus_stops — gpui tab_index is global, trapping must be manual). Migrate the 3 NamePrompt modals (name/AI-entry/MD-token); closes the deferred WCAG 2.4.3 item. New `tests/modal_trap_nav.rs` + regression: modal above console — one Escape closes modal only. Chosen over `Root::open_dialog` because prompts are Entity views with their own event subscriptions. | M |
| **B2 Modals pt 2 + anchored overlays** | Export dialog → ModalHost; saved-query picker extracted to `src/view/saved_query_picker.rs` (listbox pattern — deliberate palette prep); filter popover + cell editor stay `.absolute()` but get shared `anchored_overlay(elevation)` treatment (precise anchoring = stretch goal). | M |
| **B3 Status bar** | `src/status_bar.rs` free fn on shell, mounted under body row. v1: row count (`data_source.row_count` — the promised `grid/mod.rs:173` badge), selection count, query timing chip (`sql_console.last_elapsed_ms` + running state), connection summary. Content a11y labels only, **no tab stops** (keyboard_nav cycle counts unchanged). Reads cached scalars only — no bench risk. | S |
| **B4 Command palette** | Rewrite `command_palette.rs` as entity: gpui-component `Input` + dat0 listbox results (container focus_stop + active index), `key_context("CommandPalette")` + `register_command_palette_keys` (escape/up/down/enter as actions — the `register_sql_console_keys` precedent so real-keystroke tests prove reachability). Kill the global `cx.on_action` stub blocker → shell-root `.on_action` listener (has `&mut Window`). Mounts in ModalHost (scrim/trap/focus-restore free). Data v1 = `ActionRegistry` descriptors + existing `filter()` fuzzy matcher, render-time sort, `Kbd` hints right-aligned. Tables/recents as sources = follow-up. i18n: `palette.*` keys. New `tests/command_palette_nav.rs` (probe descriptor flips AtomicBool on Enter). | M-L |
| **B5 DockArea skeleton** (no visible change) | `dock_area` entity on shell, `set_locked(true)`, center = `GridPanel` (`panels/grid_panel.rs`; render delegates to today's body match; `recents_active` + hero handles move in — panel is persistent, simplifies the transient-hero constraint). Shell keeps `data_source`/`table_state`/selection (grid mutation stays shell-coupled; panel reads via `WeakEntity`). Hybrid: fixed docks still flank the DockArea → proves incremental conversion. **In-slice T0 spikes**: (a) TestPlatform renders DockArea children exactly once per forced frame (a11y single-frame capture; generation-counter fallback pre-designed in `a11y/mod.rs:24`); (b) TabPanel chrome injects no unlabeled tab stops; (c) single-tab center hides tab bar (`PanelStyle::Auto`). Bench gate. | M |
| **B6 Right dock** | `InspectorPanel` + `ChartsPanel` (`DockItem::split`, 288/560px initial). Menu toggles flip `panel.visible` + dock open/close; v10 bools keep persisting (schema-neutral). a11y-capture shims delegate to panels. | M |
| **B7 Left dock** | `CatalogPanel` (owns tree/collapsed/active + `catalog-tree` handle; refresh stays shell-side via event, `SqlConsoleEvent` precedent) + `ConnectionsPanel` + `AiDockPanel` (8 `ai-*` handles). **Focus-migration hot spot** — gate on catalog_nav, ai_nav, keyboard_nav, a11y_content, sql_console_transient_nav. | M-L |
| **B8 Bottom dock** | `impl Panel for SqlConsole` (`Focusable` → active tab editor handle); `toggle_sql_console` lazily constructs then `set_bottom_dock(…, px(260))`/`toggle_dock`. Console moves below grid, becomes resizable. `sql_console_visible` derived from dock state. Gate: both sql_console nav suites (full Escape ladder), integration, crash-e2e. Bench gate. | M |
| **B9 Layout persistence — session v11** | `SessionUiState` + `#[serde(default)] dock_layout: Option<serde_json::Value>` (the `DockAreaState` blob); `SESSION_SCHEMA_VERSION = 11`; identity migration v10→v11; version-ledger doc-comment. Save: `DockEvent::LayoutChanged` → ~500ms debounce → `dump`. Load: after `register_panel` for all 7 names — parse-or-default, never crash. `dock_layout` supersedes v10 bools when present. Tests: migration fixtures, corrupt-blob fallback, `tests/dock_layout_persist.rs` round-trip; audit crash-e2e/workspace fixtures for hardcoded `"schema_version": 10`. | M |
| **B10 Cleanup + A6h** | ~~Delete dead shell bools/`hero_focus` remnants, collapse hybrid scaffolding~~ — all three measured **live** at `136ef75` and struck (every `*_panel_visible` bool has 7-15 refs; `hero_focus` is load-bearing across 14 sites; B5's hybrid was resolved in-slice). Shipped: file-drop tint → `cx.theme().d0().drag_over` + `drop_target` α retune (**lint allowlist now empty for colors**, and HC stops painting a hardcoded blue); `Sp` made rem-relative so dat0's two spacing scales, which disagreed by 14%, become one; the three remaining `window.rs` chains onto `Sp` (zero delta). Recents ring had already migrated in A6; `window.rs` holds no magic pixels — its 14 `px(` sites are named dock-width consts. `<5k` target moved to B11. | S |
| **B11 window.rs extraction** | `window.rs` (8660 lines / 185 fns) → `window/mod.rs` + child modules, target `<5k`. Pure refactor, no UI change. Child modules see the parent's private items, so `WorkspaceShell`'s fields need **no** visibility change; splitting an `impl` across files is compiler-verified. Budget: test accessors ~390 · `cfg(test)` ~205 · AI ~480 · SQL ~486 · dock ~600 · charts ~350 · connections/MD ~267 · export+drop ~340. Must not lose the interim `DOCS_URL`/`DISCORD_URL` consts, and must keep the `a11y-capture` accessor block ahead of any `#[cfg(test)] mod` (clippy `items-after-test-module`). | M |

**Optional post-v1**: user themes via `ThemeRegistry::watch_dir(config_dir/themes)` + density toggle setting; palette sources (tables/recents); drag-rearrange docks; precise overlay anchoring; TitleBar adoption.

## 7. Sequencing

A0 → A1 → A2 → A3 → A4 → A5 → A6a-g → B1 → B2 → B3 → B4 → B5 → B6 → B7 → B8 → B9 → B10(+A6h) → B11.
A-slices are collision-free with B (different files) except A6h. B1-B4 don't depend on DockArea and could interleave earlier if a break from token migration is wanted; B5-B10 are strictly ordered; B11 is a pure refactor and depends only on B10. Session schema untouched until B9.

## 8. Invariants — every slice

- Full nav/a11y suite green under `--features a11y-capture` (keyboard_nav, input_nav, sql_console_nav, sql_console_transient_nav, cell_editor_nav, catalog_nav, ai_nav, recents_nav, a11y_content, a11y_spike). These assert labels/focus sequences, not colors — token swaps are invisible to them; the exceptions are slices changing a11y **label strings** (A5) — update in the same PR.
- Escape ladder 5 rungs intact; `register_sql_console_keys` called by prod + harness. Drive keyboard behavior with `simulate_keystrokes`, not `dispatch_action` (transient-bars lesson).
- Theme switching + contrast gates green; every new panel entity holds a theme subscription.
- Grid perf: `Table` + `GridTableDelegate` byte-identical through B; macOS bench gate on B5/B8/B10 (push-to-main-only → **watch post-merge main run every merge**).
- crash-e2e + session round-trip green; schema additive-only at B9.

## 9. Top risks

| Risk | Mitigation |
|---|---|
| DockArea double-render breaks single-frame a11y capture | B5 spike; generation-counter fallback pre-designed |
| apply_config/mode ordering quirk | A0 spike; fallback = prime light/dark slots + `Theme::change` |
| HC sparse-fallback leak (shadcn defaults) | full-coverage JSON + A3 gate over all pairs, all 3 themes |
| TabPanel chrome tab stops / residual bottom-dock bar | B5/B8 spikes; `closable(false)`, `zoomable(None)`, toggle-button hidden |
| Menu-toggle ↔ dock-state divergence | derive from `dock.is_open` + `panel.visible`, never parallel bools |
| `font.size: 14` shifts gpui-component widget metrics | A0 spike; fallback keep 16 + rely on `TextRole` for dat0 text |
| window.rs churn collision between workstreams | A6h folded into B10; A touches window.rs only at pinned line-sites (init, Table mount) |

## 10. Process

Per-slice: brainstorm→design→plan→SDD as with the kbd-nav slices. Model per task shape (haiku transcription / sonnet judgment / opus load-bearing gates + final whole-branch review — the transient-bars Escape-ladder bug was caught only by the cross-cutting final review). T0 hard gate before implementation slices with API unknowns (A0, inside B5). Owed human visual glances accumulate per slice (focus rings ≥3:1 both themes, modal scrim feel, icon rendering, dock resize feel, palette feel) — log per slice, batch UAT.
