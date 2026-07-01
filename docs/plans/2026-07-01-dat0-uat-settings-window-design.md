# dat0 — UAT automation: Settings-window content + behavioral tests (design)

> **Design doc.** Brainstormed 2026-07-01, off `main` `d7587dd` (UAT Gap 2 merged, PR #38).
> First slice of the "automate the manual-UAT backlog" effort, leveraging the Gap-2
> AccessKit content-assertion harness + the Gap-3 async harness. Target: the P10b
> **Settings window** UAT (§1–§9) plus the P10c **D-029 regression** items. Each
> remaining backlog slice (charts / update+About dialogs / crash dialogs) is its own
> later spec→plan→SDD cycle. Next step after user review: `writing-plans`.

---

## Context

The manual-UAT backlog spans P9a-2 / P9b / P10a / P10a-2 / P10b / P10c. A 3-agent triage
(2026-07-01) classified every item as automatable-now / stays-human / needs-new-capability.
The genuine gap is the **UI content + behavioral layer** — much else (contrast gate, PII
redaction, crash guard/staging, CLI parsing, settings-store round-trip, theme logic, the
settings-section registry+i18n, the updater's manifest/signature/SHA/version logic) is
**already unit-tested and not owed**. The Settings window was picked as the first slice:
highest automatable-yield-per-effort, self-contained, no external creds, no new capability.

The Settings window (P10b) was the never-mounted "T13 stub" made real. Verifying it renders
and persists headlessly is exactly what the new harness enables.

## Problem

`crates/dat0-app/tests/settings_ui.rs` already asserts the section **registry** (9 sections
in order + resolvable i18n keys) and the per-section persistence **logic** (store round-trips).
What is NOT tested is the **window-level integration**: that the real Settings window mounts,
renders all 9 section panes, switches panes on sidebar click, and that a toggle/input/Reset
interaction actually reaches `settings.toml`. That gap is manual-UAT-owed (P10b §1–§9, P10c
D-029 §6.2–6.4).

## Approach (decided in brainstorm)

New test file `crates/dat0-app/tests/settings_window.rs`, reusing the Gap-2 support module
(`tests/support/mod.rs`: `A11ySnapshot` + `has_label`/`query_by_role`/`has_label_contains`/
`has_label_any`) and clicking via the existing `debug_bounds(id)` + `simulate_click`.

The Settings window is a **real separate gpui window** with a **public** constructor
`SettingsPanel::new(store, window, cx)` (`settings_ui/panel.rs:31`) — NO `pub(crate)` wall
(unlike the inspector/SqlConsole surfaces). So a mount helper mirrors `open_sql_console_window`:
`cx.add_window_view(|window, cx| Root::new(cx.new(|c| SettingsPanel::new(store, window, c)), window, cx))`.
Wrapping in `gpui_component::Root` also renders the dialog layer, so the Reset-confirm
`window.open_dialog()` is capturable (proven by the onboarding carousel test).

*(Alternative considered: extend `a11y_content.rs`. Rejected — Settings is a distinct surface;
its own file is cleaner and `a11y_content.rs` is already multi-surface.)*

## Components

**Annotation (production render code in `crates/dat0-app/src/settings_ui/`, gated behind the
existing `a11y-capture` feature → identity no-op in release, zero production cost):**

| Widget | File:line | Annotation |
|---|---|---|
| Sidebar rows | `panel.rs:71–81` | `.a11y(s.id(), AccessRole::Button, t(name_key))` — `s.id()` is `&'static str` (`sections/mod.rs:29`) → clickable + content |
| Toggle rows | `panel.rs:85–107` | `.a11y(id, AccessRole::Button, t(label_key))` — `id: &'static str`; state verified via store reload (glyph `[x]`/`[ ]` annotated `.a11y_label` as bonus) |
| Buttons | theme-cycle `:182`, md-open `:343`, ai-open `:358`, adv-log-level `:243`, adv-reset `:262`, telemetry-privacy `:129` | `.a11y(static_id, AccessRole::Button, label)` (gpui-component `Button` impls `InteractiveElement`) |
| Inputs | name/email `:206–207`, budget `:307` | `.a11y(static_id, AccessRole::Label, …)` (gpui-component `Input` impls `InteractiveElement`); driven via `InputState::set_value` (`gpui-component .../input/state.rs:599`) |
| Version text | `panel.rs:223` | `.a11y_label(AccessRole::Label, version_string)` |

**Test-support:** reuse `A11ySnapshot::capture(cx)`; add a `open_settings_window(cx) ->
(Entity<SettingsPanel>, &mut VisualTestContext)` helper in the new test file.

## T0 spike (HARD GATE)

Before broad rollout, prove end-to-end on ONE slice: annotate the sidebar + the telemetry
section, mount the window, and verify (a) `A11ySnapshot` reads the 9 sidebar labels, (b) a
`debug_bounds`-located sidebar click switches the content pane, (c) a telemetry-toggle click
round-trips through `SettingsStore::load_or_default()`, and (d) `InputState::set_value` on an
input persists via the render-tick `persist_inputs()` path. If any of these can't be made
green (esp. the input path or the standalone-window render), STOP and report — don't grind.

## Test inventory (~12–14 tests, `settings_window.rs`)

1. Mount + all 9 sidebar sections render (query each `t(name_key)` present). *(covers D-029 all-9-render)*
2. Version text renders — `has_label_contains` on a stable prefix (NOT the git SHA).
3. Sidebar-click switches pane: click `theme` → theme cycle button present + profile inputs absent; bidirectional (teeth: pane-2 content absent before click).
4–6. Toggle round-trip ×3 (telemetry/workspace/updates): `debug_bounds` click → `load_or_default()` shows the bool flipped → click again → flipped back.
7. Reset → `has_active_dialog` true → confirm → reload shows `settings.toml == Settings::default()`.
8. Memory-budget input persist: `InputState::set_value` → render tick → store shows new budget. *(also asserts profile placeholder i18n for D-029)*
9–10. Theme + log-level cycle behavioral: click cycle button → displayed value changes + persists (logic already unit-tested; this proves the window click path).
11. MD/AI buttons present + clickable-without-panic (NOT dock-opens — `launch_dock` targets the shell, absent from the standalone settings window).
12. D-029 regression folded into #1 (all-9-render + updates section) + #8 (profile placeholder).

## Risks & mitigations

- **Standalone-window render / input path** → T0-spike-gated (both proven before rollout).
- **MD/AI `launch_dock` cross-window** → assert presence + safe-click (no panic), not dock-open. Characterized, not blocking.
- **Version/SHA non-determinism** → assert a stable substring/prefix only.
- **Toggle-state via glyph vs store** → verify persistence via `SettingsStore::load_or_default()` (authoritative); the `[x]`/`[ ]` glyph is a bonus content assertion.
- **Duplicate labels** (repeated section words) → use `has_label_any`/role-scoped queries where a label could recur (per the Gap-2 panic-on-2+ rule).

## Scope (NOT doing)

- **§10 keyboard-only reachability / focus-ring** — needs a NEW `dispatch_key_event` harness capability (its own enabler slice). Deferred.
- **Browser / file-manager link items** (§6.5 "Learn more", §9.1.2/3 open logs/config, §12.3 privacy link) — OS shell integration, stay human.
- **Visual/contrast appearance** — already gated by `theme_contrast_gate.rs`; the *look* stays human.
- No re-assertion of the section registry/order or per-section store logic (already covered).
- No production a11y — annotations are test-only, feature-gated; D-015 stays open.

## Testing

- **Teeth (house pattern):** every content/behavioral assertion shown to FAIL on wrong content (wrong section, un-flipped toggle, wrong reset state) before trusted.
- **Determinism:** i18n labels + fixed store values, no timestamps/paths/random → byte-stable macOS+Linux.
- **Release no-op:** `.a11y*` compile out with `a11y-capture` off; `cargo build --release` clean.
- Full workspace gate (clippy/test/fmt/i18n) before merge, same as Gap 2.

## Decomposition note

Single coherent slice (Settings window). The other backlog slices — charts persist/lineage
(P9a-2), update+About dialogs (P10a/a-2 UI), crash/report dialogs (P10c §1–2), and the two
capability enablers (`dispatch_key_event`; hidden-panic-trigger) — are separate later cycles.
