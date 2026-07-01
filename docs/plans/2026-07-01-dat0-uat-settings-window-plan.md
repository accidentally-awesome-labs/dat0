# Settings-Window UAT Automation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Headless content + behavioral tests for the real P10b Settings window — mount it, prove all 9 sections render, sidebar-click switches panes, and toggles / inputs / Reset actually persist to `settings.toml`.

**Architecture:** New test file `crates/dat0-app/tests/settings_window.rs` reusing the Gap-2 AccessKit harness (`tests/support/mod.rs::A11ySnapshot`). Annotate the Settings render code (`src/settings_ui/panel.rs`) with the existing feature-gated `.a11y(id, role, label)` / `.a11y_label(role, text)` helpers (identity no-ops in release). Mount via the public `SettingsPanel::new` ctor wrapped in `gpui_component::Root` (mirrors `open_sql_console_window` in `tests/a11y_content.rs`). Clicks resolve through `debug_bounds(id)` + `simulate_click`; persistence is verified authoritatively via `SettingsStore::load_or_default()`.

**Tech Stack:** Rust, gpui `=0.2.2`, gpui-component `0f0ab35`, `#[gpui::test]` + `VisualTestContext`, kittest `0.3.0` / accesskit `0.21.1` / accesskit_consumer `0.30.1` (all already in `Cargo.lock` from Gap 2, PR #38).

## Global Constraints

- **No new dependencies.** kittest/accesskit are already in the lock from Gap 2; NOTICE.md must stay unchanged (verify in the final task). Adding a dep is out of scope.
- **Reuse the Gap-2 harness verbatim.** `.a11y(id, role, label)` / `.a11y_label(role, text)` live in `src/a11y/mod.rs`; `A11ySnapshot` (with `has_label` / `query_by_role` / `has_label_any` / `has_label_contains` / `count_label` / `click`) lives in `tests/support/mod.rs`. Do NOT add new roles — `AccessRole` already has `Button`, `Label`, `Cell`, `Row`, `Dialog`, `Alert`; Settings uses only `Button` (clickables) and `Label` (content).
- **Emitter rule (do not break):** `AccessRole::Label` → `set_value`; every other role → `set_label`. `has_label` / `query_by_role` PANIC on 2+ matches → for any label that can repeat, use `has_label_any` / `has_label_contains` / `count_label`.
- **Clickable ids must be `&'static str`.** `s.id()` (`sections/mod.rs:29`) and `toggle_row(id)` (`panel.rs:85`) are already `&'static str`; buttons/inputs use `&'static str` literal ids.
- **Feature gating:** capture is on for integration tests via the existing self-dev-dependency (`dat0-app = { path = ".", features = ["a11y-capture"] }`); `cargo test --workspace` stays flag-free. In release, `.a11y*` compile out → `cargo build --release` must stay clean.
- **Determinism:** assert i18n labels (`dat0_i18n::t(...)`) and fixed store values only. No paths, timestamps, or git SHA in assertions (version text → assert a stable substring, never the SHA). Byte-stable on macOS + Linux.
- **Teeth (house pattern):** every content/behavioral assertion must be shown to FAIL on wrong content (wrong section, un-flipped toggle, wrong reset state) before it is trusted. No vacuous greens.
- **Test-only, D-015 stays open.** No production a11y; annotations are instrumentation.
- **Commit style:** DCO sign-off (`git commit -s`) + trailer `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

---

## File Structure

- **Create** `crates/dat0-app/tests/settings_window.rs` — all new tests + the `open_settings_window` mount helper (module-private to this file). References `mod support;` the same way `tests/a11y_content.rs` does.
- **Modify** `crates/dat0-app/src/settings_ui/panel.rs` — add `.a11y` / `.a11y_label` annotations to sidebar rows, toggle rows, buttons, inputs, and version text. This is the only production file touched; each task annotates the widgets it tests.
- **Read-only references (templates — do not modify):** `tests/a11y_content.rs` (mount-helper + capture pattern), `tests/support/mod.rs` (`A11ySnapshot` API), `tests/settings_ui.rs` (`SettingsStore` construction pattern), `src/a11y/mod.rs` (`.a11y` signatures + `AccessRole`).

---

### Task 0: T0 SPIKE — mount helper + telemetry/sidebar/input proof (HARD GATE)

**Files:**
- Create: `crates/dat0-app/tests/settings_window.rs`
- Modify: `crates/dat0-app/src/settings_ui/panel.rs` (sidebar rows `:71–81`, telemetry toggle `:122`, budget input `:307`)
- Read first: `crates/dat0-app/tests/a11y_content.rs`, `crates/dat0-app/tests/support/mod.rs`, `crates/dat0-app/tests/settings_ui.rs`

**Interfaces:**
- Consumes: `dat0_app::a11y::{AccessRole, A11yExt}` (`.a11y(id, role, label)`, `.a11y_label(role, text)`); `support::A11ySnapshot::{capture, has_label, has_label_any, query_by_role, click}`; `dat0_app::settings_ui::panel::SettingsPanel::new(store, window, cx)`; `dat0_app::settings::SettingsStore`.
- Produces: `fn open_settings_window(cx) -> (Entity<SettingsPanel>, &mut VisualTestContext)` (or the closest shape matching `open_sql_console_window`); a proven annotation pattern for sidebar rows, toggle rows, and inputs that Tasks 1–6 reuse.

- [ ] **Step 1: Read the templates.** Read `tests/a11y_content.rs` (find `open_sql_console_window` — copy its `add_window_view` + `Root::new` + `VisualTestContext` shape), `tests/support/mod.rs` (exact `A11ySnapshot` method signatures + how `click` resolves via `debug_bounds`), and `tests/settings_ui.rs` (how `SettingsStore` is constructed against a temp dir). The exact helper signatures come from these files.

- [ ] **Step 2: Write the mount helper + first failing test.** In a new `tests/settings_window.rs` with `mod support;`, add `open_settings_window` mirroring `open_sql_console_window` but constructing `SettingsPanel::new(store, window, cx)` (store built against a `tempfile::TempDir` as in `settings_ui.rs`). First test asserts all 9 sidebar labels render:

```rust
mod support;
use support::A11ySnapshot;
use dat0_app::a11y::AccessRole;

#[gpui::test]
fn settings_window_renders_all_nine_sidebar_sections(cx: &mut gpui::TestAppContext) {
    let (_panel, vcx) = open_settings_window(cx);
    let snap = A11ySnapshot::capture(vcx);
    for key in [
        "settings.profile", "settings.theme", "settings.memory_budget",
        "settings.motherduck", "settings.ai", "settings.telemetry",
        "settings.workspace", "settings.updates", "settings.advanced",
    ] {
        let label = dat0_i18n::t(key);
        assert!(snap.has_label_any(&label), "missing sidebar section: {label}");
    }
}
```

- [ ] **Step 3: Run it — expect FAIL.** `cargo test -p dat0-app --test settings_window`
  Expected: FAIL — sidebar rows carry no a11y nodes yet (labels not found), or a compile error if the helper isn't wired.

- [ ] **Step 4: Annotate the sidebar rows.** In `panel.rs::render_sidebar` (`:71–81`), add `.a11y(s.id(), AccessRole::Button, dat0_i18n::t(s.name_key()))` to the per-section `div()`. `s.id()` is `&'static str`, so it is both the click id and satisfies the clickable-id constraint. Keep the existing `.id(...)` and `.on_click(...)`.

- [ ] **Step 5: Run it — expect PASS.** `cargo test -p dat0-app --test settings_window`
  Expected: PASS — all 9 section labels found.

- [ ] **Step 6: Prove sidebar-click switches the pane (behavioral).** Add:

```rust
#[gpui::test]
fn sidebar_click_switches_content_pane(cx: &mut gpui::TestAppContext) {
    let (_panel, vcx) = open_settings_window(cx);
    // default section = profile; theme's cycle button not yet visible
    let snap = A11ySnapshot::capture(vcx);
    assert!(!snap.has_label_any(&dat0_i18n::t("settings.theme.cycle")),
        "theme control leaked into profile pane");
    // click the "theme" sidebar row, re-capture
    A11ySnapshot::capture(vcx).click(vcx, "theme");
    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label_any(&dat0_i18n::t("settings.theme.cycle")),
        "theme pane did not render after sidebar click");
}
```
Use the actual theme-cycle label key from `panel.rs:182` / the theme section; if the button has no text label yet, annotate it in this task (`.a11y("settings-theme-cycle", AccessRole::Button, <label>)`). Match `A11ySnapshot::click` to the real signature you found in Step 1.

- [ ] **Step 7: Run it — expect PASS** (annotate theme-cycle button in `panel.rs:182` if needed). `cargo test -p dat0-app --test settings_window`

- [ ] **Step 8: Prove telemetry toggle round-trips the store (behavioral).** Annotate the telemetry toggle (`panel.rs:122`, inside `toggle_row`, add `.a11y(id, AccessRole::Button, dat0_i18n::t(label_key))`). Add:

```rust
#[gpui::test]
fn telemetry_toggle_click_persists(cx: &mut gpui::TestAppContext) {
    let (_panel, vcx) = open_settings_window(cx);
    let before = /* SettingsStore::load_or_default() crash-submission flag */;
    A11ySnapshot::capture(vcx).click(vcx, "tg-telemetry");
    let after = /* reload flag */;
    assert_ne!(before, after, "telemetry toggle did not persist");
}
```
Read the store field name + `load_or_default` signature from `settings_ui/sections/telemetry.rs` and `tests/settings_ui.rs`; the store path must be the same `TempDir` the helper used.

- [ ] **Step 9: Run it — expect PASS.** `cargo test -p dat0-app --test settings_window`

- [ ] **Step 10: Prove the input path (`InputState::set_value` → persist).** Annotate the budget input (`panel.rs:307`). Add a test that sets the budget input value programmatically, drives one render tick (`vcx.run_until_parked()` after `panel.update(...)`), and asserts the store shows the new budget:

```rust
#[gpui::test]
fn budget_input_set_value_persists(cx: &mut gpui::TestAppContext) {
    let (panel, vcx) = open_settings_window(cx);
    panel.update(vcx, |p, cx| { p.budget_input.set_value("512", /*window*/, cx); });
    vcx.run_until_parked(); // persist_inputs() runs on render tick (panel.rs:393)
    let mb = /* SettingsStore::load_or_default() memory-budget field */;
    assert_eq!(mb, 512, "budget input did not persist via render tick");
}
```
Match `set_value`'s real signature from `gpui-component .../input/state.rs:599` and the budget field access path from `sections/*` (the setter is `set_memory_budget_mb`, `panel.rs:378`). If `budget_input` is private, add a `#[cfg(feature = "a11y-capture")]` test-only accessor or drive it through the input entity as the templates do.

- [ ] **Step 11: Run it — expect PASS.** `cargo test -p dat0-app --test settings_window`
  **GO/NO-GO:** if the standalone window won't render, `A11ySnapshot::capture` returns nothing, or `set_value` can't be driven to persist, STOP and report — do not grind. Otherwise the mechanism is proven for Tasks 1–6.

- [ ] **Step 12: Commit.**
```bash
git add crates/dat0-app/tests/settings_window.rs crates/dat0-app/src/settings_ui/panel.rs
git commit -s -m "test(settings): T0 spike — mount window, sidebar/telemetry/input a11y proof

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 1: All-9-render + version text + sidebar-switch bidirectional

**Files:**
- Modify: `crates/dat0-app/src/settings_ui/panel.rs` (version text `:223`)
- Modify: `crates/dat0-app/tests/settings_window.rs`

**Interfaces:**
- Consumes: `open_settings_window`, `A11ySnapshot` (from Task 0).
- Produces: annotated version text (`.a11y_label(AccessRole::Label, <version string>)`).

- [ ] **Step 1: Write failing test — content-render of each section pane + version.** For each section, click its sidebar row, re-capture, and assert a distinctive widget/label of that pane is present (profile → name input label; memory_budget → budget label; advanced → version substring). Covers D-029 "all 9 sections render". Add a bidirectional check to the existing sidebar-switch test (click back to `profile` → theme control gone). Version test:

```rust
#[gpui::test]
fn advanced_section_shows_version(cx: &mut gpui::TestAppContext) {
    let (_p, vcx) = open_settings_window(cx);
    A11ySnapshot::capture(vcx).click(vcx, "advanced");
    let snap = A11ySnapshot::capture(vcx);
    assert!(snap.has_label_contains("dat0"), "version text missing in advanced pane");
}
```
(Assert a stable substring — `"dat0"` or the crate version prefix — never the git SHA.)

- [ ] **Step 2: Run — expect FAIL** (version not annotated). `cargo test -p dat0-app --test settings_window`
- [ ] **Step 3: Annotate version text** at `panel.rs:223`: `.a11y_label(AccessRole::Label, <the same version String already rendered>)`. Reuse the exact expression already passed to `.child(...)` so the label IS the rendered text.
- [ ] **Step 4: Run — expect PASS.** `cargo test -p dat0-app --test settings_window`
- [ ] **Step 5: Teeth check.** Temporarily assert `has_label_contains("NOTAVERSION")` → confirm FAIL; revert.
- [ ] **Step 6: Commit.**
```bash
git add crates/dat0-app/src/settings_ui/panel.rs crates/dat0-app/tests/settings_window.rs
git commit -s -m "test(settings): all-9 pane render + version text + bidirectional sidebar switch

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Workspace + Updates toggle round-trips

**Files:**
- Modify: `crates/dat0-app/src/settings_ui/panel.rs` (workspace toggle `:146`, updates toggle `:161`)
- Modify: `crates/dat0-app/tests/settings_window.rs`

**Interfaces:**
- Consumes: the telemetry-toggle test pattern from Task 0.
- Produces: annotated `tg-workspace` / `tg-updates` toggles.

- [ ] **Step 1: Write failing tests** — click `tg-workspace` and `tg-updates`, each asserting the corresponding `SettingsStore` bool flips (and flips back on second click). Mirror `telemetry_toggle_click_persists`. Field/setter names: `set_treat_all_as_networked` (`sections/workspace.rs`), `set_update_auto_check` (`sections/updates.rs`).
- [ ] **Step 2: Run — expect FAIL** (toggles not annotated). `cargo test -p dat0-app --test settings_window`
- [ ] **Step 3: Annotate** the workspace toggle (`:146`) and updates toggle (`:161`) with `.a11y(id, AccessRole::Button, dat0_i18n::t(label_key))` inside `toggle_row` — same edit already applied to telemetry in Task 0 (the `toggle_row` fn is shared, so verify the `.a11y` lives inside `toggle_row` and covers all three ids; if Task 0 already put it in `toggle_row`, these are covered and this step only adds the tests). Covers D-029 "updates section renders".
- [ ] **Step 4: Run — expect PASS.** `cargo test -p dat0-app --test settings_window`
- [ ] **Step 5: Teeth check.** Assert a toggle does NOT flip on a bad id (`click(vcx, "tg-nonexistent")` → store unchanged / or expect the click to no-op); confirm the real test would fail if the flip assertion were inverted. Revert any temporary change.
- [ ] **Step 6: Commit.**
```bash
git add crates/dat0-app/src/settings_ui/panel.rs crates/dat0-app/tests/settings_window.rs
git commit -s -m "test(settings): workspace + updates toggle persistence round-trips

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Memory-budget + profile input persistence (D-029 placeholder)

**Files:**
- Modify: `crates/dat0-app/src/settings_ui/panel.rs` (name/email inputs `:206–207`)
- Modify: `crates/dat0-app/tests/settings_window.rs`

**Interfaces:**
- Consumes: the `set_value` → render-tick → store pattern proven in Task 0 (budget).
- Produces: annotated name/email inputs; profile-placeholder assertion.

- [ ] **Step 1: Write failing tests.** (a) Profile name input `set_value("Ada")` → render tick → `SettingsStore` `author.name == "Ada"`. (b) Profile placeholder i18n present (D-029): capture profile pane, assert the placeholder label resolves (`has_label_any(&dat0_i18n::t(<profile placeholder key>))` — find the key in `sections/profile.rs`). The budget-persist test from Task 0 already covers memory budget; do not duplicate it.
- [ ] **Step 2: Run — expect FAIL** (inputs not annotated / placeholder not captured). `cargo test -p dat0-app --test settings_window`
- [ ] **Step 3: Annotate** name/email inputs at `:206–207` with `.a11y(<static id>, AccessRole::Label, <placeholder or current value>)`. For the placeholder assertion, annotate the input's placeholder text as a `Label` node (or assert via the value node) so `has_label_any` can find it.
- [ ] **Step 4: Run — expect PASS.** `cargo test -p dat0-app --test settings_window`
- [ ] **Step 5: Teeth check.** `set_value("Ada")` but assert `== "Grace"` → confirm FAIL; revert.
- [ ] **Step 6: Commit.**
```bash
git add crates/dat0-app/src/settings_ui/panel.rs crates/dat0-app/tests/settings_window.rs
git commit -s -m "test(settings): profile input persistence + placeholder i18n (D-029)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Theme + log-level cycle behavioral

**Files:**
- Modify: `crates/dat0-app/src/settings_ui/panel.rs` (theme-cycle `:182` if not done in T0, log-level `:243`)
- Modify: `crates/dat0-app/tests/settings_window.rs`

**Interfaces:**
- Consumes: click + store-reload pattern.
- Produces: annotated `adv-log-level` button (theme-cycle annotated in Task 0).

- [ ] **Step 1: Write failing tests.** (a) Click `settings-theme-cycle` → `SettingsStore` `theme.id` advances to the next of `["dark","light","high-contrast"]` (`panel.rs:185`). (b) Click `adv-log-level` → log level advances through its 4 states and persists (`set_log_level`, `panel.rs:257`). Assert the persisted value changed AND (optional) the displayed label updated on re-capture.
- [ ] **Step 2: Run — expect FAIL** (log-level button not annotated). `cargo test -p dat0-app --test settings_window`
- [ ] **Step 3: Annotate** `adv-log-level` (`:243`) with `.a11y("adv-log-level", AccessRole::Button, <label>)`; confirm `settings-theme-cycle` (`:182`) is annotated (from Task 0 Step 6, else add it here).
- [ ] **Step 4: Run — expect PASS.** `cargo test -p dat0-app --test settings_window`
- [ ] **Step 5: Teeth check.** Assert theme cycles to a wrong next-value → confirm FAIL; revert.
- [ ] **Step 6: Commit.**
```bash
git add crates/dat0-app/src/settings_ui/panel.rs crates/dat0-app/tests/settings_window.rs
git commit -s -m "test(settings): theme + log-level cycle behavioral persistence

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Reset-confirm dialog → restores defaults

**Files:**
- Modify: `crates/dat0-app/src/settings_ui/panel.rs` (`adv-reset` `:262`, confirm dialog `:271–298`)
- Modify: `crates/dat0-app/tests/settings_window.rs`

**Interfaces:**
- Consumes: `Root`-rendered dialog layer (mount helper already wraps in `Root`); dialog-presence check pattern from the onboarding carousel test.
- Produces: annotated `adv-reset` button + dialog confirm button.

- [ ] **Step 1: Write failing test.** Change a setting first (e.g. flip telemetry so state != default), click `adv-reset` → assert the confirm `Dialog` is present (via `has_active_dialog` or a captured dialog label — read the carousel test in `onboarding_gpui.rs` / `a11y_content.rs` for the exact dialog-presence API), click the confirm button, then assert `SettingsStore::load_or_default()` equals `dat0_app::settings::Settings::default()`.
- [ ] **Step 2: Run — expect FAIL** (reset/confirm not annotated). `cargo test -p dat0-app --test settings_window`
- [ ] **Step 3: Annotate** `adv-reset` (`:262`) `.a11y("adv-reset", AccessRole::Button, <label>)` and the confirm-dialog OK button inside `open_reset_confirm` (`:271–298`) with a stable `&'static str` id + `AccessRole::Button`.
- [ ] **Step 4: Run — expect PASS.** `cargo test -p dat0-app --test settings_window`
- [ ] **Step 5: Teeth check.** Skip the confirm click → assert settings are NOT default (dialog opened but not confirmed) → confirms the reset only fires on confirm. Keep this as a second real assertion if clean.
- [ ] **Step 6: Commit.**
```bash
git add crates/dat0-app/src/settings_ui/panel.rs crates/dat0-app/tests/settings_window.rs
git commit -s -m "test(settings): Reset-confirm dialog restores defaults

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: MD/AI buttons present + clickable-without-panic; final gate

**Files:**
- Modify: `crates/dat0-app/src/settings_ui/panel.rs` (`md-open` `:343`, `ai-open` `:358`)
- Modify: `crates/dat0-app/tests/settings_window.rs`

**Interfaces:**
- Consumes: click pattern.
- Produces: annotated `md-open` / `ai-open`; a green full workspace gate.

- [ ] **Step 1: Write failing tests.** Navigate to the `motherduck` and `ai` sections; assert the `md-open` / `ai-open` buttons render (labels present), then click each and assert no panic and the window still renders (re-capture succeeds). Do NOT assert a dock opens — `launch_dock` targets the shell, which is absent from the standalone settings window (documented in the design).
- [ ] **Step 2: Run — expect FAIL** (buttons not annotated). `cargo test -p dat0-app --test settings_window`
- [ ] **Step 3: Annotate** `md-open` (`:343`) and `ai-open` (`:358`) with `.a11y(<static id>, AccessRole::Button, <label>)`.
- [ ] **Step 4: Run — expect PASS.** `cargo test -p dat0-app --test settings_window`
- [ ] **Step 5: Full workspace gate.**
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release -p dat0-app   # feature off → .a11y* compile out, must be clean
git diff --exit-code NOTICE.md      # no new deps → NOTICE unchanged (expect no diff)
```
  Expected: all green; `NOTICE.md` shows no diff (no new dependencies were added).
- [ ] **Step 6: i18n check** (any new UI strings would trip the CI gate — this task adds none, but confirm): run the repo's i18n-check as CI does (see `.github/workflows/ci.yml`); expect no new warnings attributable to `settings_ui` or the test file.
- [ ] **Step 7: Commit.**
```bash
git add crates/dat0-app/src/settings_ui/panel.rs crates/dat0-app/tests/settings_window.rs
git commit -s -m "test(settings): MD/AI buttons present + clickable; full gate green

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**1. Spec coverage:**
- Mount + all-9-render → T0 (Step 2), T1. ✅
- Version text → T1. ✅
- Sidebar-click switches pane (bidirectional) → T0 Step 6, T1. ✅
- Toggle round-trip ×3 → telemetry T0, workspace/updates T2. ✅
- Reset dialog → T5. ✅
- Memory-budget input persist → T0 Step 10. ✅
- Profile input + placeholder i18n (D-029) → T3. ✅
- Theme + log-level cycle behavioral → T4. ✅
- MD/AI present + clickable → T6. ✅
- D-029 (all-9 render, updates section, profile placeholder) → T1 + T2 + T3. ✅
- Release no-op + NOTICE unchanged → T6 Step 5. ✅
- Deferred (§10 keyboard-nav, browser/file links, visual contrast) → not tasked, per design Scope. ✅

**2. Placeholder scan:** Test bodies reference store field/setter names to be read from named source files (`sections/telemetry.rs`, `workspace.rs`, `updates.rs`, `profile.rs`) and helper signatures from named template files (`a11y_content.rs`, `support/mod.rs`) — this is directed lookup of exact existing symbols, not "figure it out." Every annotation step names the exact `panel.rs` line + role. No "TBD"/"handle edge cases".

**3. Type consistency:** `AccessRole::{Button, Label}`, `.a11y(id, role, label)` / `.a11y_label(role, text)`, `A11ySnapshot::{capture, has_label_any, has_label_contains, query_by_role, click}`, `SettingsStore::load_or_default()`, `SettingsPanel::new` — used consistently across all tasks. Clickable ids (`s.id()`, `tg-*`, `adv-*`, `md-open`, `ai-open`, `settings-theme-cycle`) are all `&'static str`.

**Note for executor:** exact `A11ySnapshot` method names and the `open_sql_console_window` shape are authoritative in `tests/support/mod.rs` and `tests/a11y_content.rs` respectively — Task 0 Step 1 reads them before writing code. If a method name differs (e.g. `click` vs `click_id`), use the real one; the plan's intent (find-by-label, click-by-id, assert store) is what governs.
