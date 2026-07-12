# UAT — AI config-panel keyboard-nav (design)

> **Date:** 2026-07-11 · **Branch:** `uat-ai-config-nav` off `main` (`02f7063`)
> Deferred keyboard-nav carve-out #3 (after Slice 6 keyboard-reachability, the
> recents-nav slice, and the catalog-tree slice). Ships **real production a11y**
> using the reusable `focus_stop` helper + AccessKit focus oracle. Covers the
> **AI-prompt config panel** surface flagged in P10b §10.7 / the kbd-nav backlog.

## Problem

Slice 6 made the Home hero buttons, the Settings DIY toggles, and the grid shell
keyboard-reachable, then deferred the *internal* nav of Catalog / AI-prompt /
SQL-editor / cell-editor to follow-on slices. Recents and Catalog landed. The
**AI dock** is still entirely mouse-only:

- `crates/dat0-app/src/ai/panel.rs` `render_ai_panel` (line 94) is a left dock
  (`window.rs:6686-6691`, gated on `ai_panel_visible`) of **fixed-id
  `action_button`s** (7 always rendered + a conditional 8th, `ai-key-forget`) —
  each a raw `div().id().on_click()` with **no `tab_index`,
  no `FocusHandle`, no keyboard activation, no `.a11y`**. `grep focus_stop /
  track_focus / tab_index / on_key_down / FocusHandle / .a11y` across the whole
  `ai/` dir returns **zero matches**.
- Two AI **trigger** buttons in the SQL Console toolbar are the same:
  `nl2sql-chip` (`view/sql_console.rs:1008`) and `sql-explain`
  (`sql_console.rs:1029`) are click-only `div`s, conditionally interactive
  (`.on_click` only in their `if enabled` branch, gated on `ai_ready && !busy`).

So a keyboard-only user cannot enable AI, pick a provider, set a key/model, run
Test-connection, or trigger NL→SQL / Explain. This is a genuine product a11y gap,
not just missing tests.

## Scope

**IN**
- The **AI-dock buttons** (8 ids: 7 always-rendered + the conditional
  `ai-key-forget`): `ai-toggle-enabled`, `ai-provider-cycle`, `ai-key-set`,
  `ai-key-forget` (only rendered when `key_set`), `ai-model-set`,
  `ai-toggle-advanced`, `ai-toggle-sample-rows`, `ai-test-connection`. Each
  becomes a real keyboard control (`focus_stop` + `.a11y` twin + Enter/Space
  activation + focus ring).
- The **2 SQL-Console AI trigger buttons**: `nl2sql-chip`, `sql-explain` — same
  treatment, wired inside their existing `if enabled` branch so they are tab
  stops only when operable.
- Tests: Tab-reachability of every button (7 dock + conditional forget + 2
  console) + Enter-operability on the safe draft-state toggle and on the two
  console triggers (event-emit assertion).

**DEFER** (own follow-on slice — the meaty SQL-Console carve-out)
- The rest of the SQL Console toolbar (Run, tab bar, Save-as-Table, Saved-queries)
  and the NL→SQL preview strip (`nl2sql-insert` / `nl2sql-discard` / `nl2sql-stop`).
  Wiring only `nl2sql-chip`/`sql-explain` here leaves those siblings unreachable —
  a **deliberate, documented** inconsistency the SQL-Console slice closes.
- The `NamePrompt` overlays (key/model entry, NL→SQL prompt entry): their text
  field is a gpui-component `Input` whose `InputState` **already registers a
  `tab_index`**, so the prompt Input is already Tab-reachable — no change owed.
  Its Confirm/Cancel `div`s are a separate (dialog) surface, deferred.
- The `Settings → AI` section is a 14-line stub (`settings_ui/sections/ai.rs`,
  "Real controls land in T9") — no controls, nothing to wire.

**STAYS HUMAN** (Gap 1 / owed glance)
- Focus-ring pixel appearance + WCAG ≥3:1 contrast. We assert reachability and
  operability, never ring pixels (no pixels under `TestPlatform`).

## Approach — per-button `focus_stop` (hero clone), NOT a listbox

The AI dock is a **heterogeneous config form** (toggles, a provider cycle, key/
model actions, a test probe), so each control is its own tab stop — the ARIA-
correct model and the exact shape Slice 6 shipped for the 3 Settings DIY toggles.
The single-container-+-arrows *listbox* pattern (recents / catalog) is **rejected
here** — it is for homogeneous lists/trees, wrong for a form.

`focus_stop` (`a11y/mod.rs:41`, ships in release) is chained onto each button
together with its `.a11y(id, Button, label)` twin keyed by the **same
`&'static str` id** (the label source the focus oracle joins on). Enter/Space
invokes the **same handler** the `on_click` does, so keyboard and mouse
activation cannot drift. All use `tab_index = 0` and rely on paint order (the
established hero convention), giving the Tab sequence:

```
enabled → provider → key-set → [forget] → model → advanced → sample → test
```

## Seams (production edits)

### `src/ai/panel.rs`
`action_button` grows a focus-handle parameter and chains the wiring:

```
fn action_button(id, label, ev, fh: &FocusHandle, cx) -> Stateful<Div> {
    let click = cx.listener(move |ws, _ev, window, cx| ws.handle_ai_panel_event(ev.clone(), window, cx));
    let key   = cx.listener(move |ws, _ev: &KeyDownEvent, window, cx| ws.handle_ai_panel_event(ev.clone(), window, cx));
    div().id(id)
        .focus_stop(id, fh, 0, key)             // NEW (release-real)
        .a11y(id, AccessRole::Button, label)     // NEW (label source; release no-op)
        .child(label).on_click(click)
}
```

The `ev` is cloned into both closures (it is already `Clone`). The stable
`FocusHandle`s live on the persistent `WorkspaceShell.hero_focus` map (keyed by
the 8 static ids). They are **hoisted + cloned** out before the render call —
exactly as the catalog slice did (`let catalog_fh = self.hero_focus_handle("catalog-tree", cx)`)
— to avoid the `&self` / `&mut Context` borrow clash inside `render`. So
`render_ai_panel` grows a handle accessor argument (a small map or a
`HeroHandles`-style view). The conditional `ai-key-forget` handle is created
unconditionally; the button (and its `focus_stop`) is emitted only when `key_set`.

### `src/window.rs`
- Before the `render_ai_panel` call site (`window.rs:6690`), build the AI handles
  via `hero_focus_handle(id, cx)` for each of the 8 ids and pass them in.
- `#[cfg(feature="a11y-capture")]` shims on `WorkspaceShell`:
  - `seed_ai_panel_for_test(panel: AiPanel)` — sets `self.ai_panel = panel;
    self.ai_panel_visible = true;` **without** calling `hydrate_ai_panel`
    (which probes the OS keychain via `KeychainKeyStore::new().get(p)` and reads
    `settings.toml` — the hermeticity trap). Slice-5 `seed_catalog_tree_for_test`
    idiom (bypass the clobbering hydrate path).
  - `ai_panel_enabled_for_test() -> bool` — reads `self.ai_panel.enabled` to prove
    the Enter-operability draft flip.

### `src/view/sql_console.rs`
- `SqlConsole` gains two stable `FocusHandle` fields (`nl2sql_focus`,
  `explain_focus`) minted once in `SqlConsole::new` (`sql_console.rs:240`).
- Inside the existing `if enabled { … }` branch of `nl2sql-chip` and `sql-explain`,
  chain `.focus_stop(id, &self.nl2sql_focus, 0, key)` + `.a11y(id, Button, label)`,
  where `key = cx.listener(|_c, _ev: &KeyDownEvent, _w, cx| cx.emit(SqlConsoleEvent::OpenNl2SqlPrompt))`
  (resp. `Explain`). `cx.listener` on a `Context<SqlConsole>` re-enters the entity
  context so `cx.emit` is legal (empty_state re-entry precedent). The emit is the
  **same** event the `on_click` fires. Disabled branch is unchanged → not a tab stop.
- `#[cfg(feature="a11y-capture")]` `set_ai_ready_for_test(bool)` shim so a test can
  enable the chip/explain gate without driving the full AI-config → `push_ai_ready_to_console`
  chain.

## Tests — `crates/dat0-app/tests/ai_nav.rs` (a11y-capture, full mounts)

Mount helpers copied per-binary (precedent: `keyboard_nav.rs`, `catalog_nav.rs`,
`motherduck_window.rs`).

1. **AI-dock reachability** (hermetic, no serial): `seed_ai_panel_for_test(AiPanel
   { enabled:false, provider:Some(Anthropic), key_set:false, .. })`, then
   `press_tab()` repeatedly asserting `focused_label()` walks the 7 buttons in
   paint order. A second case with `key_set:true` proves the conditional
   `ai-key-forget` joins the ring (8 stops).
2. **AI-dock panel-closed negative**: with `ai_panel_visible=false`, none of the
   AI ids are Tab-reachable (the dock is not painted → not a tab stop).
3. **AI-dock operability** (`DAT0_CONFIG_DIR` + `#[serial]`): seed with
   `enabled:false`, Tab to `ai-toggle-enabled`, `simulate_keystrokes("enter")`,
   assert `ai_panel_enabled_for_test() == true`. Sandbox `config_dir` because the
   handler also best-effort-writes `settings.toml` via `update_ai_settings` (we do
   not want to touch the real user config; the draft flip is the assertion).
4. **SQL chip/explain reachability + operability**: full `WorkspaceShell` mount →
   `toggle_sql_console(window, cx)` (Slice-5 invocation pattern) →
   `set_ai_ready_for_test(true)` on the console entity → `cx.subscribe(&console, …)`
   recording `SqlConsoleEvent`s → Tab to `nl2sql-chip`, assert `focused_label()`,
   `enter`, assert `OpenNl2SqlPrompt` observed; Tab to `sql-explain`, assert label,
   `enter`, assert `Explain` observed.

## Safety (Slice-4 "avoid side-effecting activation")

Only `ai-toggle-enabled` is Enter-activated in the AI-dock tests (pure draft flip;
the settings write is best-effort and sandboxed). We **never** press Enter on:

- `ai-test-connection` — `handle_ai_panel_event` → `maybe_show_ai_privacy_banner`
  (writes settings) + `spawn_ai_test` (async probe via the registry dispatcher).
- `ai-key-set` / `ai-model-set` — open a `NamePrompt` overlay (side effect).
- `ai-provider-cycle` / `forget` / advanced / sample — re-probe the keychain and/or
  write settings; not needed for the operability claim.

For `nl2sql-chip` / `sql-explain` the Enter path is **safe to activate** because
the observable is the *emit* itself — the `cx.subscribe` records
`OpenNl2SqlPrompt`/`Explain` before the downstream (WorkspaceShell subscription →
async AI) ever runs. No key, no network, no dispatcher is exercised.

## Risks / T0 spike gate (HARD GATE — spike EVERY asserted surface)

Per the Slice-3 lesson, T0 must prove ALL of the following before any breadth
build; any failure → STOP and re-scope:

1. An AI-dock `focus_stop` button is Tab-reachable under `TestPlatform` and
   `focused_label()` names it (the dock is a new surface for the oracle).
2. Enter on the focused `ai-toggle-enabled` flips `ai_panel.enabled` (operability).
3. **A `SqlConsole`-owned `focus_stop` button is reached by Tab crossing the
   shell → console-view boundary** from the self-focusing editor `Input`, and
   `focused_label()` names it. This is the primary risk (new mount surface + a
   cross-entity focus traversal).
   **STOP-clause:** if Tab will not cross into the console view, drop
   `nl2sql-chip`/`sql-explain` from this slice (defer to the SQL-Console slice)
   and ship the AI dock alone.
4. Enter on a focused `SqlConsole` `focus_stop` button emits its `SqlConsoleEvent`,
   observed via a test `cx.subscribe`.

Secondary: (a) AI-dock paint order must match the asserted Tab sequence; (b) the
settle bracket (`refresh` + `run_until_parked`) is mandatory before each focus
query; (c) `set_ai_ready_for_test` must flip the gate that makes chip/explain
render their interactive (`if enabled`) branch.

## Deps / CI / footprint

- **Zero new deps** — `focus_stop` / `.a11y` / the oracle already exist; D-015
  stays open (still no OS AccessKit adapter). Cargo.lock / NOTICE unchanged.
- **Release footprint is non-zero** (the `focus_stop` production wiring on up to
  10 buttons — 8 AI-dock ids + 2 SQL-Console chips) — the 4th slice to ship real
  release a11y. Owed: one **human focus-ring visual glance** across the AI-dock
  buttons + the 2 SQL-Console chips, in each theme, WCAG ≥3:1 (joins the standing
  About / Charts / Settings / Slice-6 / recents / catalog glances).
- New test binary auto-runs under `cargo test --workspace` via `a11y-capture`
  feature unification. Standard CI gate (fmt / clippy -D warnings on the pinned
  1.97.0 toolchain / workspace tests / i18n / dep-guards). WATCH the post-merge
  main run — the push-to-main-only macOS grid-scroll bench can redden main
  silently.

## What this deliberately does NOT do

- No focus-ring pixel/contrast assertion (Gap 1 → human).
- No wiring of the rest of the SQL-Console toolbar or the NL→SQL preview strip
  (deferred SQL-Console slice) — documented reachability inconsistency.
- No `NamePrompt` / Settings-AI-section changes.
- No OS AccessKit adapter (D-015 unchanged); no gpui / gpui-component fork or
  version bump (pinned 0.2.2 / 0.5.1).
